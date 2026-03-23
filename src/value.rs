pub mod bc;
pub mod cmp;
pub mod index;
pub mod list;
pub mod list_meta;
pub mod mat;
pub mod math;
pub mod op;

mod wqerror_helper;

use std::{
    borrow::Cow,
    fmt,
    io::{BufRead, Seek, Write},
    sync::{Arc, Mutex},
};

use crate::{
    astnode::{BinaryOperator, UnaryOperator},
    vm,
    wqdb::ChunkId,
    wqerror::{WqError, WqErrorType},
};

use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use unicode_segmentation::UnicodeSegmentation as _;

pub type WqResult<T> = Result<T, WqError>;

/// Heap cell shared between frames and closures for captured locals.
pub type ValueCell = Arc<Mutex<Value>>;

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionData {
    pub params: Option<Arc<[String]>>,
    pub locals: u16,
    /// Shared immutable instruction array
    pub instructions: Arc<[vm::instruction::Instruction]>,
    /// Debug chunk id for this function's code
    pub dbg_chunk: Option<ChunkId>,
    /// Statement spans for the function body (byte start,end in source)
    pub dbg_stmt_spans: Option<Arc<[(usize, usize)]>>,
    /// Local variable names by slot index (for wqdb)
    pub dbg_local_names: Option<Arc<[String]>>,
}

#[derive(Debug, Clone)]
pub struct ClosureData {
    pub params: Option<Arc<[String]>>,
    pub locals: u16,
    pub captured: Vec<ValueCell>,
    /// Shared immutable instruction array
    pub instructions: Arc<[vm::instruction::Instruction]>,
    /// Debug chunk id for this function's code
    pub dbg_chunk: Option<ChunkId>,
    /// Statement spans for the function body (byte start,end in source)
    pub dbg_stmt_spans: Option<Arc<[(usize, usize)]>>,
    /// Local variable names by slot index (for wqdb)
    pub dbg_local_names: Option<Arc<[String]>>,
}

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    BigInt(Box<BigInt>),
    Float(f64),
    Char(char),
    Symbol(String),
    Bool(bool),
    IntList(Vec<i64>),
    List(Vec<Value>),
    Dict(Box<IndexMap<String, Value>>),
    CompiledFunction(Arc<FunctionData>),
    /// closure with captured cells (upvalues)
    Closure(Arc<ClosureData>),
    BuiltinFunction(String),
    Stream(Arc<Mutex<StreamHandle>>),
}

/// handle for a streaming io source
pub trait BufReadSeek: BufRead + Seek {}
impl<T: BufRead + Seek> BufReadSeek for T {}

pub trait WriteSeek: Write + Seek {}
impl<T: Write + Seek> WriteSeek for T {}

pub struct StreamHandle {
    pub reader: Option<Box<dyn BufReadSeek + Send>>,
    pub writer: Option<Box<dyn WriteSeek + Send>>,
}

impl fmt::Debug for StreamHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // e.g. Some("std::io::BufReader<std::fs::File>")
        let reader_ty = self
            .reader
            .as_ref()
            .map(|r| std::any::type_name_of_val(&**r));
        let writer_ty = self
            .writer
            .as_ref()
            .map(|w| std::any::type_name_of_val(&**w));
        f.debug_struct("StreamHandle")
            .field("reader", &reader_ty)
            .field("writer", &writer_ty)
            .finish()
    }
}

impl Drop for StreamHandle {
    fn drop(&mut self) {
        self.reader = None;
        self.writer = None;
    }
}

impl Value {
    pub fn unit() -> Self {
        Value::IntList(vec![])
    }

    /// Create a new stream value
    pub fn stream(handle: StreamHandle) -> Self {
        Value::Stream(Arc::new(Mutex::new(handle)))
    }

    pub fn is_unit(&self) -> bool {
        self.is_empty()
    }

    pub fn is_atom(&self) -> bool {
        !matches!(self, Value::Dict(_) | Value::List(_) | Value::IntList(_))
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Value::List(_) | Value::IntList(_))
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Value::Dict(_))
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Value::Char(_))
            || self.is_unit()
            || matches!(self, Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))))
    }

    pub fn as_char_list<'a>(&'a self) -> Option<Cow<'a, [Value]>> {
        match self {
            Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))) => {
                Some(Cow::Borrowed(items))
            }
            Value::Char(c) => Some(Cow::Owned(vec![Value::Char(*c)])),
            v if v.is_unit() => Some(Cow::Owned(vec![])),
            _ => None,
        }
    }

    // pub fn is_bytelist(&self) -> bool {
    //     match self {
    //         Value::Int(n) => u8::try_from(*n).is_ok(),
    //         Value::BigInt(n) => n.to_u8().is_some(),
    //         Value::IntList(items) => items.iter().all(|&n| u8::try_from(n).is_ok()),
    //         Value::List(items) => items.iter().all(|v| match v {
    //             Value::Int(n) => u8::try_from(*n).is_ok(),
    //             Value::BigInt(n) => n.to_u8().is_some(),
    //             _ => false,
    //         }),
    //         _ => false,
    //     }
    // }

    pub fn try_to_string(&self) -> WqResult<String> {
        const EXP: &str = "expected char or list<char>";
        match self {
            Value::Char(c) => Ok(c.to_string()),
            Value::List(items) => {
                let mut s = String::with_capacity(items.len());
                for (i, v) in items.iter().enumerate() {
                    if let Value::Char(c) = v {
                        s.push(*c);
                    } else {
                        return Err(WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(v, i));
                    }
                }
                Ok(s)
            }
            _ if self.is_unit() => Ok(String::new()),
            _ => Err(WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
        }
    }

    pub fn try_to_vec_u8(&self) -> WqResult<Vec<u8>> {
        const EXP: &str = "expected list<int in 0..=255>";

        match self {
            Value::IntList(l) => l
                .iter()
                .enumerate()
                .map(|(i, &n)| {
                    u8::try_from(n).map_err(|_| {
                        WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(&Value::Int(n), i)
                    })
                })
                .collect(),
            Value::List(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| match v {
                    Value::Int(n) => u8::try_from(*n).map_err(|_| {
                        WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(v, i)
                    }),
                    Value::BigInt(n) => n.to_u8().ok_or_else(|| {
                        WqError::new(WqErrorType::Domain)
                            .msg(EXP)
                            .offending_elem(v, i)
                    }),
                    _ => Err(WqError::new(WqErrorType::Domain)
                        .msg(EXP)
                        .offending_elem(v, i)),
                })
                .collect(),
            Value::Int(n) => u8::try_from(*n)
                .map(|b| vec![b])
                .map_err(|_| WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
            Value::BigInt(n) => n
                .to_u8()
                .map(|b| vec![b])
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
            _ => Err(WqError::new(WqErrorType::Domain).msg(EXP).got1(self)),
        }
    }

    /// Get the length of a value
    pub fn len(&self) -> usize {
        match self {
            Value::List(items) => items.len(),
            Value::IntList(items) => items.len(),
            Value::Dict(map) => map.len(),
            _ => 1, // Atoms have length 1
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn is_callable(v: &Value) -> bool {
        matches!(
            v,
            Value::CompiledFunction { .. } | Value::Closure { .. } | Value::BuiltinFunction(_)
        )
    }

    /// Get the type name of a value
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::BigInt(_) => "bigint",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Symbol(_) => "symbol",
            Value::Bool(_) => "bool",
            Value::IntList(_) => "intlist",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::CompiledFunction { .. } => "fn",
            Value::Closure { .. } => "closure",
            Value::BuiltinFunction(_) => "bfn",
            Value::Stream(_) => "stream",
        }
    }

    /// Extract numeric value as i64
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::BigInt(n) => n.to_i64(),
            Value::Float(f) => Some(*f as i64),
            _ => None,
        }
    }

    /// Extract numeric value as f64
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(n) => Some(*n as f64),
            Value::BigInt(n) => n.to_f64(),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    // /// Extract numeric value as BigInt (for int-like values)
    // pub fn as_bigint(&self) -> Option<BigInt> {
    //     match self {
    //         Value::Int(n) => Some(BigInt::from(*n)),
    //         Value::BigInt(n) => Some(n.clone()),
    //         _ => None,
    //     }
    // }

    pub fn from_bigint(n: BigInt) -> Value {
        n.to_i64()
            .map(Value::Int)
            .unwrap_or_else(|| Value::BigInt(Box::new(n)))
    }

    /// Construct a list value from items, promoting to IntList if all items are ints.
    pub fn from_items(items: Vec<Value>) -> Value {
        // Try to collect all ints in one pass; returns None on first non-int.
        if let Some(ints) = items
            .iter()
            .map(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
        {
            Value::IntList(ints)
        } else {
            Value::List(items)
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Int(a), Int(b)) => a == b,
            (BigInt(a), BigInt(b)) => a == b,
            (Int(a), BigInt(b)) => num_bigint::BigInt::from(*a) == **b,
            (BigInt(a), Int(b)) => **a == num_bigint::BigInt::from(*b),
            (Float(a), Float(b)) => a == b,
            (Char(a), Char(b)) => a == b,
            (Symbol(a), Symbol(b)) => a == b,
            (Bool(a), Bool(b)) => a == b,
            // (Null, Null) => true,
            (List(a), List(b)) => a == b,
            (IntList(a), IntList(b)) => a == b,
            (IntList(a), List(b)) | (List(b), IntList(a)) => {
                if a.len() != b.len() {
                    return false;
                }
                a.iter().zip(b).all(|(x, y)| matches!(y, Int(n) if n == x))
            }
            (Dict(a), Dict(b)) => a == b,
            (CompiledFunction(a), CompiledFunction(b)) => Arc::ptr_eq(a, b),
            (Closure(a), Closure(b)) => {
                if !Arc::ptr_eq(&a.instructions, &b.instructions)
                    || a.captured.len() != b.captured.len()
                {
                    return false;
                }
                a.captured
                    .iter()
                    .zip(b.captured.iter())
                    .all(|(lhs, rhs)| Arc::ptr_eq(lhs, rhs))
            }
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
            Value::BigInt(n) => write!(f, "{n}"),
            Value::Float(fl) => {
                if fl.is_infinite() && fl.is_sign_positive() {
                    write!(f, "inf")
                } else if fl.is_infinite() && fl.is_sign_negative() {
                    write!(f, "-inf")
                } else if fl.is_nan() {
                    write!(f, "nan")
                } else if fl.fract() == 0.0 {
                    write!(f, "{fl:.1}")
                } else {
                    write!(f, "{fl}")
                }
            }
            Value::Char(c) => {
                let esc = escape_str_for_display(&c.to_string());
                write!(f, "\"{esc}\"")
            }
            Value::Symbol(s) => write!(f, "`{s}"),
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            Value::IntList(items) => {
                if items.is_empty() {
                    return write!(f, "()");
                }
                if items.len() == 1 {
                    write!(f, ",{}", items[0])
                } else {
                    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    write!(f, "({})", strs.join(";"))
                }
            }
            Value::List(items) => {
                // Empty list
                if items.is_empty() {
                    return write!(f, "()");
                }
                // Non-empty char-only list -> quoted string
                if let Ok(s) = self.try_to_string() {
                    let esc = escape_str_for_display(&s);
                    return write!(f, "\"{esc}\"");
                }
                // 1-elem list
                if items.len() == 1 {
                    return write!(f, ",{}", items[0]);
                }
                // General case
                let items_str: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                write!(f, "({})", items_str.join(";"))
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    write!(f, "(`)")
                } else {
                    let mut pairs = Vec::new();
                    for (k, v) in &**map {
                        pairs.push(format!("`{k}:{v}"));
                    }
                    write!(f, "({})", pairs.join(";"))
                }
            }
            Value::CompiledFunction(func) => match &func.params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::Closure(c) => match &c.params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::BuiltinFunction(name) => write!(f, "<bfn '{name}'>"),
            Value::Stream(_) => write!(f, "<stream>"),
        }
    }
}

pub fn eval_unary(op: &UnaryOperator, val: Value) -> WqResult<Value> {
    use UnaryOperator::*;

    macro_rules! up {
        ($s:literal) => {
            concat!("unary operator ", $s)
        };
    }

    match op {
        Negate => val.neg().map_err(|e| e.into_wqerror().src(up!("-"))),
        Count => Ok(val.len().into_wq_value()),
    }
}

pub fn eval_binary(op: &BinaryOperator, left: Value, right: Value) -> WqResult<Value> {
    use BinaryOperator::*;

    macro_rules! bp {
        ($s:literal) => {
            concat!("binary operator ", $s)
        };
    }

    match op {
        Add => left.add(&right).map_err(|e| e.into_wqerror().src(bp!("+"))),
        Subtract => left
            .subtract(&right)
            .map_err(|e| e.into_wqerror().src(bp!("-"))),
        Multiply => left
            .multiply(&right)
            .map_err(|e| e.into_wqerror().src(bp!("*"))),
        Power => left
            .power(&right)
            .map_err(|e| e.into_wqerror().src(bp!("^"))),
        Divide => left
            .divide(&right)
            .map_err(|e| e.into_wqerror().src(bp!("/"))),
        DivideDot => left
            .divide_dot(&right)
            .map_err(|e| e.into_wqerror().src(bp!("/."))),
        Modulo => left
            .modulo(&right)
            .map_err(|e| e.into_wqerror().src(bp!("%"))),
        ModuloDot => left
            .modulo_dot(&right)
            .map_err(|e| e.into_wqerror().src(bp!("%."))),
        Matmul => left.mm(&right).map_err(|e| e.src(bp!("**"))),

        Equal => Ok(left.eq(&right)),
        NotEqual => Ok(left.neq(&right)),
        LessThan => left.lt(&right).map_err(|e| e.into_wqerror().src(bp!("<"))),
        LessThanOrEqual => left
            .leq(&right)
            .map_err(|e| e.into_wqerror().src(bp!("<="))),
        GreaterThan => left.gt(&right).map_err(|e| e.into_wqerror().src(bp!(">"))),
        GreaterThanOrEqual => left
            .geq(&right)
            .map_err(|e| e.into_wqerror().src(bp!(">="))),
        Cat => Ok(left.cat(right)),
    }
}

pub fn escape_str_for_display(s: &str) -> String {
    crate::escape::escape_string_inner(s, '"')
}

pub fn into_wq_str<S: AsRef<str>>(s: S) -> Value {
    Value::List(s.as_ref().chars().map(Value::Char).collect())
}

pub trait IntoWqValue {
    fn into_wq_value(self) -> Value;
}

impl IntoWqValue for String {
    fn into_wq_value(self) -> Value {
        into_wq_str(self)
    }
}

impl IntoWqValue for &str {
    fn into_wq_value(self) -> Value {
        into_wq_str(self)
    }
}

impl IntoWqValue for usize {
    fn into_wq_value(self) -> Value {
        match i64::try_from(self) {
            Ok(n) => Value::Int(n),
            Err(_) => Value::BigInt(Box::new(BigInt::from(self))),
        }
    }
}

impl IntoWqValue for u16 {
    fn into_wq_value(self) -> Value {
        Value::Int(i64::from(self))
    }
}

impl IntoWqValue for u64 {
    fn into_wq_value(self) -> Value {
        match i64::try_from(self) {
            Ok(n) => Value::Int(n),
            Err(_) => Value::BigInt(Box::new(BigInt::from(self))),
        }
    }
}

pub trait Excerpt {
    fn excerpt(&self) -> String;
}

impl<T: std::fmt::Display> Excerpt for T {
    fn excerpt(&self) -> String {
        let s = self.to_string();
        let mut g = s.graphemes(true);
        let head: String = g.by_ref().take(20).collect();
        if g.next().is_some() {
            format!("{head}...")
        } else {
            head
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::value::bc::BcError;

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
        assert_eq!(
            list.assign_by_index(&Value::Int(1), Value::Int(5)),
            Some(())
        );
        assert_eq!(list.index(&Value::Int(1)), Some(Value::Int(5)));
        assert_eq!(list.assign_by_index(&Value::Int(5), Value::Int(0)), None);

        let mut map = IndexMap::new();
        map.insert("a".to_string(), Value::Int(1));
        let mut dict = Value::Dict(Box::new(map));
        assert_eq!(
            dict.assign_by_index(&Value::Symbol("a".into()), Value::Int(2)),
            Some(())
        );
        assert_eq!(dict.index(&Value::Symbol("a".into())), Some(Value::Int(2)));
        assert_eq!(
            dict.assign_by_index(&Value::Symbol("b".into()), Value::Int(3)),
            Some(())
        );
    }

    #[test]
    fn test_vectorized_comparisons() {
        // let scalar = Value::Int(1);
        // let vec = Value::List(vec![Value::Int(1), Value::Int(1)]);
        // assert_eq!(
        //     scalar.eq(&vec),
        //     Ok(Value::List(vec![Value::Bool(true), Value::Bool(true)]))
        // );

        // let a = Value::List(vec![Value::Int(1)]);
        // let b = Value::List(vec![Value::Int(1), Value::Int(2)]);
        // assert!(matches!(a.eq(&b), Err(WqError::Length(_))));

        let list = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert_eq!(
            list.gt(&Value::Int(1)),
            Ok(Value::List(vec![Value::Bool(false), Value::Bool(true)]))
        );

        // let str_a = Value::List(vec![Value::Char('a'), Value::Char('b')]);
        // let str_b = Value::List(vec![Value::Char('a'), Value::Char('c')]);
        // assert_eq!(
        //     str_a.eq(&str_b),
        //     Ok(Value::List(vec![Value::Bool(true), Value::Bool(false)]))
        // );
    }

    // #[test]
    // fn test_bool_equals() {
    //     assert_eq!(
    //         Value::Bool(true).eq(&Value::Bool(true)),
    //         Ok(Value::Bool(true))
    //     );
    //     assert_eq!(
    //         Value::Bool(true).eq(&Value::Bool(false)),
    //         Ok(Value::Bool(false))
    //     );
    //     assert_eq!(
    //         Value::Bool(false).eq(&Value::Bool(false)),
    //         Ok(Value::Bool(true))
    //     );
    // }

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
        assert!(matches!(c.xor_bool(&d), Err(BcError::Length { .. })));

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
        assert!(matches!(c.modulo(&d), Err(BcError::Length { .. })));
    }

    #[test]
    fn test_bitwise_ops() {
        let a = Value::Int(6);
        let b = Value::Int(3);
        assert_eq!(a.band(&b), Ok(Value::Int(2)));
        assert_eq!(a.bor(&b), Ok(Value::Int(7)));
        assert_eq!(a.bxor(&b), Ok(Value::Int(5)));
        assert_eq!(a.bnot(), Ok(Value::Int(!6)));
        assert_eq!(Value::Int(1).shl(&Value::Int(3)), Ok(Value::Int(8)));
        assert_eq!(Value::Int(8).shr(&Value::Int(2)), Ok(Value::Int(2)));
        let arr = Value::IntList(vec![1, 2, 3]);
        let res = arr.bor(&Value::Int(1));
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
        let dict = Value::Dict(Box::new(map));
        let keys = Value::List(vec![Value::Symbol("b".into()), Value::Symbol("a".into())]);
        assert_eq!(
            dict.index(&keys),
            Some(Value::List(vec![Value::Int(2), Value::Int(1)]))
        );
    }

    #[test]
    fn test_dict_integer_index_and_assign() {
        let mut map = IndexMap::new();
        map.insert("a".to_string(), Value::Int(1));
        map.insert("b".to_string(), Value::Int(2));
        map.insert("c".to_string(), Value::Int(3));
        let mut dict = Value::Dict(Box::new(map));

        assert_eq!(dict.index(&Value::Int(1)), Some(Value::Int(2)));
        assert_eq!(dict.index(&Value::Int(-1)), Some(Value::Int(3)));
        assert_eq!(
            dict.index(&Value::IntList(vec![0, 2])),
            Some(Value::List(vec![Value::Int(1), Value::Int(3)]))
        );
        assert_eq!(
            dict.index(&Value::List(vec![
                Value::Int(1),
                Value::Symbol("a".to_string()),
            ])),
            Some(Value::List(vec![Value::Int(2), Value::Int(1)]))
        );

        assert_eq!(
            dict.assign_by_index(&Value::Int(1), Value::Int(99)),
            Some(())
        );
        assert_eq!(dict.index(&Value::Int(1)), Some(Value::Int(99)));
        assert_eq!(dict.assign_by_index(&Value::Int(10), Value::Int(0)), None);
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
        // assert_eq!(
        //     arr.eq(&list),
        //     Ok(Value::List(vec![
        //         Value::Bool(true),
        //         Value::Bool(true),
        //         Value::Bool(true)
        //     ]))
        // );
    }
}
