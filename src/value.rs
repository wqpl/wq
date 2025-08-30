pub mod bc;
pub mod box_mode;
pub mod cmp;
pub mod list;
pub mod math;
pub mod op;
pub mod str;

use indexmap::IndexMap;
use std::fmt;
use std::io::{BufRead, Seek, Write};
use std::process::Child;
use std::sync::{Arc, Mutex};

use crate::astnode::AstNode;
use crate::vm;
use crate::wqdb::ChunkId;
use crate::wqerror::WqError;

// pub type WqResult<T> = Result<T, WqError>;
pub type WqResult<T> = Result<T, WqError>;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Char(char),
    Symbol(String),
    Bool(bool),
    /// specialized list of integers
    IntList(Vec<i64>),
    List(Vec<Value>),
    /// dict (symbol -> value mapping) - preserves insertion order
    Dict(IndexMap<String, Value>),
    Function {
        params: Option<Vec<String>>,
        body: Box<AstNode>,
    },
    /// pre-compiled function (no captures)
    CompiledFunction {
        params: Option<Vec<String>>,
        locals: u16,
        instructions: Vec<vm::instruction::Instruction>,
        /// Debug chunk id for this function's code
        dbg_chunk: Option<ChunkId>,
        /// Statement spans for the function body (byte start,end in source)
        dbg_stmt_spans: Option<Vec<(usize, usize)>>,
        /// Local variable names by slot index (for wqdb)
        dbg_local_names: Option<Vec<String>>,
    },
    /// closure with captured values
    Closure {
        params: Option<Vec<String>>,
        locals: u16,
        captured: Vec<Value>,
        instructions: Vec<vm::instruction::Instruction>,
        /// Debug chunk id for this function's code
        dbg_chunk: Option<ChunkId>,
        /// Statement spans for the function body (byte start,end in source)
        dbg_stmt_spans: Option<Vec<(usize, usize)>>,
        /// Local variable names by slot index (for wqdb)
        dbg_local_names: Option<Vec<String>>,
    },
    /// handle for builtin functions
    BuiltinFunction(String),
    /// handle to an input/output stream, e.g. a process pipe
    Stream(Arc<Mutex<StreamHandle>>),
    Null,
}

impl Value {
    /// Create a new stream value
    pub fn stream(handle: StreamHandle) -> Self {
        Value::Stream(Arc::new(Mutex::new(handle)))
    }

    pub fn is_atom(&self) -> bool {
        !matches!(self, Value::List(_) | Value::IntList(_) | Value::Dict(_))
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Value::List(_) | Value::IntList(_))
    }

    pub fn is_str(&self) -> bool {
        match self {
            Value::List(items) => items.iter().all(|v| matches!(v, Value::Char(_))),
            Value::Char(_) => true,
            _ => false,
        }
    }

    pub fn try_str(&self) -> Option<String> {
        match self {
            Value::Char(c) => Some(c.to_string()),
            // experimental: interpret () as empty str
            il @ Value::IntList(_) if il.is_empty() => Some(String::new()),
            Value::List(items) => {
                let mut s = String::with_capacity(items.len());
                for v in items {
                    if let Value::Char(c) = v {
                        s.push(*c);
                    } else {
                        return None; // bail out on the fly
                    }
                }
                Some(s)
            }
            _ => None,
        }
    }

    /// Get the length of a value
    pub fn len(&self) -> usize {
        match self {
            Value::List(items) => items.len(),
            Value::IntList(items) => items.len(),
            Value::Dict(map) => map.len(),
            Value::Symbol(_) => 1,
            Value::BuiltinFunction(_) => 1,
            _ => 1, // Atoms have length 1
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the type name of a value
    pub fn type_name_verbose(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Symbol(_) => "symbol",
            Value::Bool(_) => "bool",
            Value::IntList(_) => "intlist",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Function { .. } => "fn",
            Value::CompiledFunction { .. } => "fn",
            Value::Closure { .. } => "closure",
            Value::BuiltinFunction(_) => "bfn",
            Value::Stream(_) => "stream",
            Value::Null => "null",
        }
    }

    pub fn type_name_simple(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Symbol(_) => "symbol",
            Value::Bool(_) => "bool",
            Value::IntList(_) => "list",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Function { .. } => "fn",
            Value::CompiledFunction { .. } => "fn",
            Value::Closure { .. } => "fn",
            Value::BuiltinFunction(_) => "fn",
            Value::Stream(_) => "stream",
            Value::Null => "null",
        }
    }

    /// Convert to integer if possible
    pub fn to_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Float(f) => Some(*f as i64),
            Value::Char(c) => Some(*c as i64),
            _ => None,
        }
    }

    /// Convert to float if possible
    pub fn to_float(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::Float(f) => Some(*f),
            Value::Char(c) => Some(*c as u8 as f64),
            _ => None,
        }
    }

    /// Index into a list or dictionary
    pub fn index(&self, key: &Value) -> Option<Value> {
        match (self, key) {
            (Value::List(items), Value::Int(i)) => {
                let idx = if *i < 0 { items.len() as i64 + i } else { *i } as usize;
                items.get(idx).cloned()
            }
            (Value::IntList(items), Value::Int(i)) => {
                let idx = if *i < 0 { items.len() as i64 + i } else { *i } as usize;
                items.get(idx).cloned().map(Value::Int)
            }
            (Value::List(items), Value::List(idxs)) => {
                let mut result = Vec::new();
                for idx_val in idxs {
                    if let Value::Int(i) = idx_val {
                        let len = items.len() as i64;
                        let idx_i64 = if *i < 0 { len + *i } else { *i };
                        if idx_i64 < 0 || idx_i64 >= len {
                            return None;
                        }
                        let idx = idx_i64 as usize;
                        if let Some(v) = items.get(idx) {
                            result.push(v.clone());
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(Value::List(result))
            }
            (Value::List(items), Value::IntList(idxs)) => {
                let mut result = Vec::new();
                for i in idxs {
                    let len = items.len() as i64;
                    let idx_i64 = if *i < 0 { len + *i } else { *i };
                    if idx_i64 < 0 || idx_i64 >= len {
                        return None;
                    }
                    let idx = idx_i64 as usize;
                    if let Some(v) = items.get(idx) {
                        result.push(v.clone());
                    } else {
                        return None;
                    }
                }
                Some(Value::List(result))
            }
            (Value::IntList(items), Value::List(idxs)) => {
                let mut out = Vec::new();
                for idx_val in idxs {
                    if let Value::Int(i) = idx_val {
                        let len = items.len() as i64;
                        let idx_i64 = if *i < 0 { len + *i } else { *i };
                        if idx_i64 < 0 || idx_i64 >= len {
                            return None;
                        }
                        let idx = idx_i64 as usize;
                        if let Some(v) = items.get(idx) {
                            out.push(*v);
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(Value::IntList(out))
            }
            (Value::IntList(items), Value::IntList(idxs)) => {
                let mut out = Vec::new();
                for i in idxs {
                    let len = items.len() as i64;
                    let idx_i64 = if *i < 0 { len + *i } else { *i };
                    if idx_i64 < 0 || idx_i64 >= len {
                        return None;
                    }
                    let idx = idx_i64 as usize;
                    if let Some(v) = items.get(idx) {
                        out.push(*v);
                    } else {
                        return None;
                    }
                }
                Some(Value::IntList(out))
            }
            (Value::Dict(map), Value::Symbol(key)) => map.get(key).cloned(),
            (Value::Dict(map), Value::List(keys)) => {
                let mut result = Vec::new();
                for key_val in keys {
                    if let Value::Symbol(k) = key_val {
                        if let Some(v) = map.get(k) {
                            result.push(v.clone());
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(Value::List(result))
            }
            _ => None,
        }
    }

    /// Mutate list or dictionary element by index/key. Returns `Some(())` on
    /// success and `None` if the key does not exist or the types are
    /// incompatible.
    pub fn set_index(&mut self, key: &Value, value: Value) -> Option<()> {
        match self {
            Value::List(items) => {
                if let Value::Int(i) = key {
                    let len = items.len() as i64;
                    let idx_i64 = if *i < 0 { len + *i } else { *i };
                    if idx_i64 < 0 || idx_i64 >= len {
                        return None;
                    }
                    let idx = idx_i64 as usize;
                    if idx < items.len() {
                        items[idx] = value;
                        Some(())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Value::IntList(items) => {
                if let Value::Int(i) = key {
                    let len = items.len() as i64;
                    let idx_i64 = if *i < 0 { len + *i } else { *i };
                    if idx_i64 < 0 || idx_i64 >= len {
                        return None;
                    }
                    let idx = idx_i64 as usize;
                    if let Value::Int(v) = value.clone() {
                        if idx < items.len() {
                            items[idx] = v;
                            return Some(());
                        } else {
                            return None;
                        }
                    }
                    let mut list: Vec<Value> = items.iter().map(|&x| Value::Int(x)).collect();
                    if idx < list.len() {
                        list[idx] = value;
                        *self = Value::List(list);
                        Some(())
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            Value::Dict(map) => {
                if let Value::Symbol(key_str) = key {
                    // if map.contains_key(key_str) {
                    //     map.insert(key_str.clone(), value);
                    //     Some(())
                    // } else {
                    //     None
                    // }
                    map.insert(key_str.clone(), value);
                    Some(())
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Char(a), Char(b)) => a == b,
            (Symbol(a), Symbol(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,
            (Null, Null) => true,
            (List(a), List(b)) => a == b,
            (IntList(a), IntList(b)) => a == b,
            (IntList(a), List(b)) | (List(b), IntList(a)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter().zip(b).all(|(x, y)| matches!(y, Int(n) if n == x))
            }
            (Dict(a), Dict(b)) => a == b,
            (
                Function {
                    params: pa,
                    body: ba,
                },
                Function {
                    params: pb,
                    body: bb,
                },
            ) => pa == pb && ba == bb,
            (
                CompiledFunction {
                    params: pa,
                    locals: la,
                    instructions: ia,
                    ..
                },
                CompiledFunction {
                    params: pb,
                    locals: lb,
                    instructions: ib,
                    ..
                },
            ) => pa == pb && la == lb && ia == ib,
            (
                Closure {
                    params: pa,
                    locals: la,
                    captured: ca,
                    instructions: ia,
                    ..
                },
                Closure {
                    params: pb,
                    locals: lb,
                    captured: cb,
                    instructions: ib,
                    ..
                },
            ) => pa == pb && la == lb && ca == cb && ia == ib,
            (BuiltinFunction(a), BuiltinFunction(b)) => a == b,
            (Stream(a), Stream(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(fl) => {
                if fl.is_infinite() {
                    if fl.is_sign_positive() {
                        write!(f, "inf")
                    } else {
                        write!(f, "-inf")
                    }
                } else if fl.is_nan() {
                    write!(f, "nan")
                } else if fl.fract() == 0.0 {
                    write!(f, "{fl:.0}")
                } else {
                    write!(f, "{fl}")
                }
            }
            Value::Char(c) => write!(f, "\"{c}\""),
            Value::Symbol(s) => write!(f, "`{s}"),
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            Value::IntList(items) => {
                if items.is_empty() {
                    return write!(f, "()");
                }
                if box_mode::is_boxed() {
                    let rows: Vec<Value> = items.iter().map(|&n| Value::Int(n)).collect();
                    if let Some(b) = box_mode::format_boxed(&rows) {
                        return write!(f, "{b}");
                    }
                }
                if items.len() == 1 {
                    write!(f, ",{}", items[0])
                } else {
                    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    write!(f, "({})", strs.join(";"))
                }
            }
            l @ Value::List(items) => {
                // 1. Empty list
                if items.is_empty() {
                    return write!(f, "()");
                }

                // 2. Non-empty char-only list -> quoted string
                if let Some(s) = l.try_str() {
                    return write!(f, "\"{s}\"");
                }

                // 3. boxed mode
                if box_mode::is_boxed() {
                    if let Some(b) = box_mode::format_boxed(items) {
                        return write!(f, "{b}");
                    }
                }

                // 4. Singleton list - ,1
                if items.len() == 1 {
                    return write!(f, ",{}", items[0]);
                }

                // 5. General case - (1;2)
                let items_str: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                write!(f, "({})", items_str.join(";"))
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    write!(f, "()!()")
                } else {
                    let mut pairs = Vec::new();
                    let was_boxed = box_mode::is_boxed();
                    if was_boxed {
                        box_mode::set_boxed(false);
                    }
                    for (k, v) in map {
                        pairs.push(format!("`{k}:{v}"));
                    }
                    if was_boxed {
                        box_mode::set_boxed(true);
                    }
                    write!(f, "({})", pairs.join(";"))
                }
            }
            Value::Function { params, .. } => match params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::CompiledFunction { params, .. } => match params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::Closure { params, .. } => match params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::BuiltinFunction(name) => write!(f, "bfn '{name}'"),
            Value::Stream(_) => write!(f, "<stream>"),
            Value::Null => write!(f, "null"),
        }
    }
}

/// handle for a streaming io source
pub trait BufReadSeek: BufRead + Seek {}
impl<T: BufRead + Seek> BufReadSeek for T {}

pub trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

pub struct StreamHandle {
    pub reader: Option<Box<dyn BufReadSeek + Send>>,
    pub writer: Option<Box<dyn WriteSeek + Send>>,
    pub child: Option<Child>, // process handle when spawning
}

impl fmt::Debug for StreamHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamHandle").finish()
    }
}

unsafe impl Send for StreamHandle {}
unsafe impl Sync for StreamHandle {}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.reader = None;
        self.writer = None;

        // terminate spawned child processes if any
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::wqerror::WqError;

    use super::*;

    #[test]
    fn test_arithmetic() {
        let a = Value::Int(5);
        let b = Value::Int(3);

        assert_eq!(a.add(&b), Ok(Value::Int(8)));
        assert_eq!(a.subtract(&b), Ok(Value::Int(2)));
        assert_eq!(a.multiply(&b), Ok(Value::Int(15)));
        assert_eq!(a.divide(&b), Ok(Value::Float(5.0 / 3.0)));
        assert_eq!(a.modulo(&b), Ok(Value::Int(2)));
    }

    #[test]
    fn test_list_operations() {
        let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);

        assert_eq!(list.len(), 3);
        assert_eq!(list.index(&Value::Int(0)), Some(Value::Int(1)));
        assert_eq!(list.index(&Value::Int(-1)), Some(Value::Int(3)));
    }

    #[test]
    fn test_set_index() {
        let mut list = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(list.set_index(&Value::Int(1), Value::Int(5)), Some(()));
        assert_eq!(list.index(&Value::Int(1)), Some(Value::Int(5)));
        assert_eq!(list.set_index(&Value::Int(5), Value::Int(0)), None);

        let mut map = IndexMap::new();
        map.insert("a".to_string(), Value::Int(1));
        let mut dict = Value::Dict(map);
        assert_eq!(
            dict.set_index(&Value::Symbol("a".into()), Value::Int(2)),
            Some(())
        );
        assert_eq!(dict.index(&Value::Symbol("a".into())), Some(Value::Int(2)));
        assert_eq!(
            dict.set_index(&Value::Symbol("b".into()), Value::Int(3)),
            Some(())
        );
    }

    #[test]
    fn test_vectorized_comparisons() {
        let scalar = Value::Int(1);
        let vec = Value::List(vec![Value::Int(1), Value::Int(1)]);
        assert_eq!(
            scalar.eq(&vec),
            Ok(Value::List(vec![Value::Bool(true), Value::Bool(true)]))
        );

        let a = Value::List(vec![Value::Int(1)]);
        let b = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert!(matches!(a.eq(&b), Err(WqError::LengthError(_))));

        let list = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(
            list.gt(&Value::Int(1)),
            Ok(Value::List(vec![Value::Bool(false), Value::Bool(true)]))
        );

        let str_a = Value::List(vec![Value::Char('a'), Value::Char('b')]);
        let str_b = Value::List(vec![Value::Char('a'), Value::Char('c')]);
        assert_eq!(
            str_a.eq(&str_b),
            Ok(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );
    }

    #[test]
    fn test_bool_equals() {
        assert_eq!(
            Value::Bool(true).eq(&Value::Bool(true)),
            Ok(Value::Bool(true))
        );
        assert_eq!(
            Value::Bool(true).eq(&Value::Bool(false)),
            Ok(Value::Bool(false))
        );
        assert_eq!(
            Value::Bool(false).eq(&Value::Bool(false)),
            Ok(Value::Bool(true))
        );
    }

    #[test]
    fn test_vectorized_logical_ops() {
        let a = Value::Bool(true);
        let b = Value::List(vec![Value::Bool(true), Value::Bool(false)]);
        assert_eq!(
            a.and_bool(&b),
            Ok(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );

        assert_eq!(
            b.or_bool(&Value::Bool(false)),
            Ok(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );

        let c = Value::List(vec![Value::Bool(true)]);
        let d = Value::List(vec![Value::Bool(false), Value::Bool(true)]);
        assert!(matches!(c.xor_bool(&d), Err(WqError::LengthError(_))));

        assert_eq!(
            d.not_bool(),
            Ok(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );
    }

    #[test]
    fn test_vectorized_modulo() {
        let a = Value::List(vec![Value::Int(5), Value::Int(10)]);
        let b = Value::Int(3);
        assert_eq!(
            a.modulo(&b),
            Ok(Value::List(vec![Value::Int(2), Value::Int(1)]))
        );

        let c = Value::List(vec![Value::Int(5)]);
        let d = Value::List(vec![Value::Int(2), Value::Int(3)]);
        assert!(matches!(c.modulo(&d), Err(WqError::LengthError(_))));
    }

    #[test]
    fn test_bitwise_ops() {
        let a = Value::Int(6);
        let b = Value::Int(3);
        assert_eq!(a.bitand(&b), Ok(Value::Int(2)));
        assert_eq!(a.bitor(&b), Ok(Value::Int(7)));
        assert_eq!(a.bitxor(&b), Ok(Value::Int(5)));
        assert_eq!(a.bitnot(), Ok(Value::Int(!6)));
        assert_eq!(Value::Int(1).shl(&Value::Int(3)), Ok(Value::Int(8)));
        assert_eq!(Value::Int(8).shr(&Value::Int(2)), Ok(Value::Int(2)));
        let arr = Value::IntList(vec![1, 2, 3]);
        let res = arr.bitor(&Value::Int(1));
        assert_eq!(res, Ok(Value::IntList(vec![1 | 1, 2 | 1, 3 | 1])));
    }

    // #[test]
    // fn test_zero_division_and_dot_ops() {
    //     let zero = Value::Int(0);
    //     assert!(matches!(zero.divide(&zero), Err(WqError::DomainError(_))));
    //     match zero.divide_dot(&zero) {
    //         Ok(Value::Float(f)) => assert!(f.is_nan()),
    //         Ok(Value::Int(_)) => panic!("expected nan"),
    //         _ => panic!("expected nan"),
    //     }
    //     match zero.modulo_dot(&zero) {
    //         Ok(Value::Float(f)) => assert!(f.is_nan()),
    //         Ok(Value::Int(_)) => panic!("expected nan"),
    //         _ => (),
    //     }
    // }

    #[test]
    fn test_dict_multi_index() {
        let mut map = IndexMap::new();
        map.insert("a".to_string(), Value::Int(1));
        map.insert("b".to_string(), Value::Int(2));
        let dict = Value::Dict(map);
        let keys = Value::List(vec![Value::Symbol("b".into()), Value::Symbol("a".into())]);
        assert_eq!(
            dict.index(&keys),
            Some(Value::List(vec![Value::Int(2), Value::Int(1)]))
        );
    }

    #[test]
    fn test_intlist_multi_index() {
        let arr = Value::IntList(vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
        let idxs = Value::IntList(vec![2, 4]);
        assert_eq!(arr.index(&idxs), Some(Value::IntList(vec![2, 4])));
    }

    #[test]
    fn test_intlist_list_arith_and_cmp() {
        let arr = Value::IntList(vec![1, 2, 3]);
        let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(
            arr.add(&list),
            Ok(Value::List(vec![
                Value::Int(2),
                Value::Int(4),
                Value::Int(6)
            ]))
        );
        assert_eq!(
            list.add(&arr),
            Ok(Value::List(vec![
                Value::Int(2),
                Value::Int(4),
                Value::Int(6)
            ]))
        );
        assert_eq!(
            arr.eq(&list),
            Ok(Value::List(vec![
                Value::Bool(true),
                Value::Bool(true),
                Value::Bool(true)
            ]))
        );
    }

    #[test]
    fn test_intlist_boxed_display() {
        box_mode::set_boxed(true);
        let arr = Value::IntList(vec![0, 0, 0]);
        let disp = format!("{arr}");
        box_mode::set_boxed(false);
        assert!(disp.contains('\n'));
    }
}
