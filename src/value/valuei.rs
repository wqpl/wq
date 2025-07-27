use std::collections::HashMap;
use std::fmt;
use std::io::{BufRead, Write};
use std::process::Child;
use std::sync::{Arc, Mutex};

use crate::parser::AstNode;
use crate::value::box_mode;
use crate::vm;

/// handle for a streaming io source
pub struct StreamHandle {
    pub reader: Option<Box<dyn BufRead + Send>>,
    pub writer: Option<Box<dyn Write + Send>>,
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
    /// dict (symbol -> value mapping)
    Dict(HashMap<String, Value>),
    Function {
        params: Option<Vec<String>>,
        body: Box<AstNode>,
    },
    /// pre-compiled function represented as bytecode instructions
    BytecodeFunction {
        params: Option<Vec<String>>,
        instructions: Vec<vm::instruction::Instruction>,
    },
    /// handle to an input/output stream, e.g. a process pipe
    Stream(Arc<Mutex<StreamHandle>>),
    Null,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WqError {
    TypeError(String),
    IndexError(String),
    DomainError(String),
    ZeroDivisionError(String),
    ArithmeticOverflowError(String),
    LengthError(String),
    SyntaxError(String),
    FnArgCountMismatchError(String),
    RuntimeError(String),
    EofError(String),
    AssertionFailError(String),
    Break,
    Continue,
    Return(Value),
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
                BytecodeFunction {
                    params: pa,
                    instructions: ia,
                },
                BytecodeFunction {
                    params: pb,
                    instructions: ib,
                },
            ) => pa == pb && ia == ib,
            (Stream(a), Stream(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Value {
    /// Create a new integer value
    pub fn int(n: i64) -> Self {
        Value::Int(n)
    }
    /// Create a new float value
    pub fn float(f: f64) -> Self {
        Value::Float(f)
    }
    pub fn inf() -> Self {
        Value::Float(f64::INFINITY)
    }
    pub fn neg_inf() -> Self {
        Value::Float(f64::NEG_INFINITY)
    }
    pub fn nan() -> Self {
        Value::Float(f64::NAN)
    }
    /// Create a new character value
    pub fn char(c: char) -> Self {
        Value::Char(c)
    }
    /// Create a new symbol value
    pub fn symbol(s: String) -> Self {
        Value::Symbol(s)
    }
    /// Create a new boolean value
    pub fn bool(b: bool) -> Self {
        Value::Bool(b)
    }
    /// Create a new list value
    pub fn list(items: Vec<Value>) -> Self {
        Value::List(items)
    }
    /// Create a new dict value
    pub fn dict(map: HashMap<String, Value>) -> Self {
        Value::Dict(map)
    }
    /// Create a new stream value
    pub fn stream(handle: StreamHandle) -> Self {
        Value::Stream(Arc::new(Mutex::new(handle)))
    }

    pub fn is_atom(&self) -> bool {
        matches!(
            self,
            Value::Int(_)
                | Value::Float(_)
                | Value::Char(_)
                | Value::Symbol(_)
                | Value::Bool(_)
                | Value::Null
        )
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Value::List(_) | Value::IntList(_))
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Value::Dict(_))
    }

    pub fn is_string(&self) -> bool {
        match self {
            Value::List(items) => items.iter().all(|v| matches!(v, Value::Char(_))),
            _ => false,
        }
    }

    /// Get the length of a value
    pub fn len(&self) -> usize {
        match self {
            Value::List(items) => items.len(),
            Value::IntList(items) => items.len(),
            Value::Dict(map) => map.len(),
            Value::Symbol(s) => s.len(),
            _ => 1, // Atoms have length 1
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the type name of a value
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Symbol(_) => "symbol",
            Value::Bool(_) => "bool",
            Value::IntList(_) => "list",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Function { .. } => "function",
            Value::BytecodeFunction { .. } => "function",
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

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(fl) => {
                if fl.is_infinite() {
                    if fl.is_sign_positive() {
                        write!(f, "Inf")
                    } else {
                        write!(f, "-Inf")
                    }
                } else if fl.is_nan() {
                    write!(f, "NaN")
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
                    let rows: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    return write!(f, "({})", rows.join(";"));
                }
                if items.len() == 1 {
                    write!(f, ",{}", items[0])
                } else {
                    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    write!(f, "({})", strs.join(";"))
                }
            }
            Value::List(items) => {
                // 1. Empty list
                if items.is_empty() {
                    return write!(f, "()");
                }

                // 2. Non-empty char-only list - quoted string
                if items.iter().all(|v| matches!(v, Value::Char(_))) {
                    let s: String = items
                        .iter()
                        .map(|v| {
                            // this match should not fail
                            if let Value::Char(c) = v {
                                *c
                            } else {
                                unreachable!()
                            }
                        })
                        .collect();
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
            Value::BytecodeFunction { params, .. } => match params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::Stream(_) => write!(f, "<stream>"),
            Value::Null => write!(f, "null"),
        }
    }
}

impl fmt::Display for WqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WqError::TypeError(msg) => write!(f, "type error: {msg}"),
            WqError::IndexError(msg) => write!(f, "index error: {msg}"),
            WqError::DomainError(msg) => write!(f, "domain error: {msg}"),
            WqError::ZeroDivisionError(msg) => write!(f, "zero division error: {msg}"),
            WqError::ArithmeticOverflowError(msg) => write!(f, "arithmetic overflow error: {msg}"),
            WqError::LengthError(msg) => write!(f, "length error: {msg}"),
            WqError::SyntaxError(msg) => write!(f, "syntax error: {msg}"),
            WqError::FnArgCountMismatchError(msg) => {
                write!(f, "function argument count mismatch error: {msg}")
            }
            WqError::RuntimeError(msg) => write!(f, "runtime error: {msg}"),
            WqError::EofError(msg) => write!(f, "end of file error: {msg}"),
            WqError::Break => write!(f, "break"),
            WqError::Continue => write!(f, "continue"),
            WqError::Return(v) => write!(f, "return {v}"),
            WqError::AssertionFailError(msg) => write!(f, "assertion failed: {msg}"),
        }
    }
}

impl std::error::Error for WqError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_creation() {
        assert_eq!(Value::int(42), Value::Int(42));
        assert_eq!(Value::float(3.1), Value::Float(3.1));
        assert_eq!(Value::char('a'), Value::Char('a'));
        assert_eq!(
            Value::symbol("test".to_string()),
            Value::Symbol("test".to_string())
        );
    }

    #[test]
    fn test_arithmetic() {
        let a = Value::int(5);
        let b = Value::int(3);

        assert_eq!(a.add(&b), Some(Value::int(8)));
        assert_eq!(a.subtract(&b), Some(Value::int(2)));
        assert_eq!(a.multiply(&b), Some(Value::int(15)));
        assert_eq!(a.divide(&b), Some(Value::float(5.0 / 3.0)));
        assert_eq!(a.modulo(&b), Some(Value::int(2)));
    }

    #[test]
    fn test_list_operations() {
        let list = Value::list(vec![Value::int(1), Value::int(2), Value::int(3)]);

        assert_eq!(list.len(), 3);
        assert_eq!(list.index(&Value::int(0)), Some(Value::int(1)));
        assert_eq!(list.index(&Value::int(-1)), Some(Value::int(3)));
    }

    #[test]
    fn test_set_index() {
        let mut list = Value::list(vec![Value::int(1), Value::int(2)]);
        assert_eq!(list.set_index(&Value::int(1), Value::int(5)), Some(()));
        assert_eq!(list.index(&Value::int(1)), Some(Value::int(5)));
        assert_eq!(list.set_index(&Value::int(5), Value::int(0)), None);

        let mut map = HashMap::new();
        map.insert("a".to_string(), Value::int(1));
        let mut dict = Value::dict(map);
        assert_eq!(
            dict.set_index(&Value::symbol("a".into()), Value::int(2)),
            Some(())
        );
        assert_eq!(dict.index(&Value::symbol("a".into())), Some(Value::int(2)));
        assert_eq!(
            dict.set_index(&Value::symbol("b".into()), Value::int(3)),
            Some(())
        );
    }

    #[test]
    fn test_vectorized_comparisons() {
        let scalar = Value::int(1);
        let vec = Value::list(vec![Value::int(1), Value::int(1)]);
        assert_eq!(
            scalar.equals(&vec),
            Value::List(vec![Value::Bool(true), Value::Bool(true)])
        );

        let a = Value::list(vec![Value::int(1)]);
        let b = Value::list(vec![Value::int(1), Value::int(2)]);
        assert_eq!(
            a.equals(&b),
            Value::List(vec![Value::Bool(true), Value::Bool(false)])
        );

        let list = Value::list(vec![Value::int(1), Value::int(2)]);
        assert_eq!(
            list.greater_than(&Value::int(1)),
            Value::List(vec![Value::Bool(false), Value::Bool(true)])
        );

        let str_a = Value::list(vec![Value::Char('a'), Value::Char('b')]);
        let str_b = Value::list(vec![Value::Char('a'), Value::Char('b')]);
        assert_eq!(str_a.equals(&str_b), Value::Bool(true));
    }

    #[test]
    fn test_bool_equals() {
        assert_eq!(
            Value::Bool(true).equals(&Value::Bool(true)),
            Value::Bool(true)
        );
        assert_eq!(
            Value::Bool(true).equals(&Value::Bool(false)),
            Value::Bool(false)
        );
        assert_eq!(
            Value::Bool(false).equals(&Value::Bool(false)),
            Value::Bool(true)
        );
    }

    #[test]
    fn test_vectorized_logical_ops() {
        let a = Value::Bool(true);
        let b = Value::list(vec![Value::Bool(true), Value::Bool(false)]);
        assert_eq!(
            a.and_bool(&b),
            Some(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );

        assert_eq!(
            b.or_bool(&Value::Bool(false)),
            Some(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );

        let c = Value::list(vec![Value::Bool(true)]);
        let d = Value::list(vec![Value::Bool(false), Value::Bool(true)]);
        assert_eq!(
            c.xor_bool(&d),
            Some(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );

        assert_eq!(
            d.not_bool(),
            Some(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        );
    }

    #[test]
    fn test_vectorized_modulo() {
        let a = Value::list(vec![Value::int(5), Value::int(10)]);
        let b = Value::int(3);
        assert_eq!(
            a.modulo(&b),
            Some(Value::list(vec![Value::int(2), Value::int(1)]))
        );

        let c = Value::list(vec![Value::int(5)]);
        let d = Value::list(vec![Value::int(2), Value::int(3)]);
        assert_eq!(
            c.modulo(&d),
            Some(Value::list(vec![Value::int(1), Value::int(2)]))
        );
    }

    #[test]
    fn test_bitwise_ops() {
        let a = Value::int(6);
        let b = Value::int(3);
        assert_eq!(a.bitand(&b), Some(Value::int(2)));
        assert_eq!(a.bitor(&b), Some(Value::int(7)));
        assert_eq!(a.bitxor(&b), Some(Value::int(5)));
        assert_eq!(a.bitnot(), Some(Value::int(!6)));
        assert_eq!(Value::int(1).shl(&Value::int(3)), Some(Value::int(8)));
        assert_eq!(Value::int(8).shr(&Value::int(2)), Some(Value::int(2)));
        let arr = Value::IntList(vec![1, 2, 3]);
        let res = arr.bitor(&Value::int(1));
        assert_eq!(res, Some(Value::IntList(vec![1 | 1, 2 | 1, 3 | 1])));
    }

    #[test]
    fn test_zero_division_and_dot_ops() {
        let zero = Value::int(0);
        assert_eq!(zero.divide(&zero), None);
        match zero.divide_dot(&zero) {
            Some(Value::Float(f)) => assert!(f.is_infinite()),
            _ => panic!("expected infinity"),
        }
        match zero.modulo_dot(&zero) {
            Some(Value::Float(f)) => assert!(f.is_nan()),
            Some(Value::Int(_)) => panic!("expected NaN"),
            _ => (),
        }
    }

    #[test]
    fn test_dict_multi_index() {
        let mut map = HashMap::new();
        map.insert("a".to_string(), Value::int(1));
        map.insert("b".to_string(), Value::int(2));
        let dict = Value::dict(map);
        let keys = Value::list(vec![Value::symbol("b".into()), Value::symbol("a".into())]);
        assert_eq!(
            dict.index(&keys),
            Some(Value::list(vec![Value::int(2), Value::int(1)]))
        );
    }
}
