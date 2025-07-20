use std::collections::HashMap;
use std::fmt;

use crate::parser::AstNode;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Char(char),
    Symbol(String),
    Bool(bool),
    List(Vec<Value>),
    /// Dictionary (symbol -> value mapping)
    Dict(HashMap<String, Value>),
    Function {
        params: Option<Vec<String>>,
        body: Box<AstNode>,
    },
    Null,
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
        matches!(self, Value::List(_))
    }

    pub fn is_dict(&self) -> bool {
        matches!(self, Value::Dict(_))
    }

    /// Get the length of a value
    pub fn len(&self) -> usize {
        match self {
            Value::List(items) => items.len(),
            Value::Dict(map) => map.len(),
            Value::Symbol(s) => s.len(),
            _ => 1, // Atoms have length 1
        }
    }

    /// Get the type name of a value
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Char(_) => "char",
            Value::Symbol(_) => "symbol",
            Value::Bool(_) => "bool",
            Value::List(_) => "list",
            Value::Dict(_) => "dict",
            Value::Function { .. } => "function",
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
            (Value::Dict(map), Value::Symbol(key)) => map.get(key).cloned(),
            _ => None,
        }
    }

    /// Arithmetic operations
    pub fn add(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a + *b as f64)),
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.add(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.add(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.add(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.add(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    fn is_scalar(&self) -> bool {
        matches!(
            self,
            Value::Int(_) | Value::Float(_) | Value::Char(_) | Value::Symbol(_) | Value::Bool(_)
        )
    }

    pub fn subtract(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a - b)),
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a - *b as f64)),
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.subtract(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.subtract(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.subtract(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.subtract(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn multiply(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a * b)),
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a * *b as f64)),
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.multiply(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.multiply(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.multiply(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.multiply(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn divide(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else if a % b == 0 {
                    Some(Value::Int(a / b))
                } else {
                    Some(Value::Float(*a as f64 / *b as f64))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(a / b))
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(*a as f64 / b))
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Float(a / *b as f64))
                }
            }
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.divide(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.divide(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.divide(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.divide(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
            (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
            (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
            (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
            (Value::List(a), Value::List(b)) => Value::compare_lists(a, b),
            _ => None,
        }
    }

    fn compare_lists(a: &[Value], b: &[Value]) -> Option<std::cmp::Ordering> {
        let len = a.len().min(b.len());
        for i in 0..len {
            match Value::compare_values(&a[i], &b[i])? {
                std::cmp::Ordering::Equal => continue,
                ord => return Some(ord),
            }
        }
        Some(a.len().cmp(&b.len()))
    }

    /// Comparison operations
    pub fn equals(&self, other: &Value) -> Value {
        let result = match Value::compare_values(self, other) {
            Some(std::cmp::Ordering::Equal) => true,
            _ => false,
        };
        Value::Bool(result)
    }

    pub fn not_equals(&self, other: &Value) -> Value {
        match self.equals(other) {
            Value::Bool(b) => Value::Bool(!b),
            _ => Value::Bool(true), // Should not happen
        }
    }

    pub fn less_than(&self, other: &Value) -> Value {
        let result = match Value::compare_values(self, other) {
            Some(std::cmp::Ordering::Less) => true,
            _ => false,
        };
        Value::Bool(result)
    }

    pub fn less_than_or_equal(&self, other: &Value) -> Value {
        let result = match Value::compare_values(self, other) {
            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal) => true,
            _ => false,
        };
        Value::Bool(result)
    }

    pub fn greater_than(&self, other: &Value) -> Value {
        let result = match Value::compare_values(self, other) {
            Some(std::cmp::Ordering::Greater) => true,
            _ => false,
        };
        Value::Bool(result)
    }

    pub fn greater_than_or_equal(&self, other: &Value) -> Value {
        let result = match Value::compare_values(self, other) {
            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal) => true,
            _ => false,
        };
        Value::Bool(result)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::Float(fl) => {
                if fl.fract() == 0.0 {
                    write!(f, "{fl:.0}")
                } else {
                    write!(f, "{fl}")
                }
            }
            Value::Char(c) => write!(f, "\"{c}\""),
            Value::Symbol(s) => write!(f, "`{s}`"),
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
            Value::List(items) => {
                if items.is_empty() {
                    write!(f, "()")
                } else if items.iter().all(|v| matches!(v, Value::Char(_))) {
                    let mut s = String::new();
                    for v in items {
                        if let Value::Char(c) = v {
                            s.push(*c);
                        }
                    }
                    write!(f, "\"{}\"", s)
                } else if items.len() == 1 {
                    write!(f, ",{}", items[0])
                } else {
                    let items_str: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    write!(f, "({})", items_str.join(";"))
                }
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    write!(f, "()!()")
                } else {
                    let mut pairs = Vec::new();
                    for (k, v) in map {
                        pairs.push(format!("`{k}:{v}"));
                    }
                    write!(f, "({})", pairs.join(";"))
                }
            }
            Value::Function { params, .. } => match params {
                Some(p) => write!(f, "{{[{}]...}}", p.join(";")),
                None => write!(f, "{{...}}"),
            },
            Value::Null => write!(f, "null"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum WqError {
    TypeError(String),
    IndexError(String),
    DomainError(String),
    LengthError(String),
    SyntaxError(String),
    FnArgCountMismatchError(String),
    RuntimeError(String),
}

impl fmt::Display for WqError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WqError::TypeError(msg) => write!(f, "type error: {msg}"),
            WqError::IndexError(msg) => write!(f, "index error: {msg}"),
            WqError::DomainError(msg) => write!(f, "domain error: {msg}"),
            WqError::LengthError(msg) => write!(f, "length error: {msg}"),
            WqError::SyntaxError(msg) => write!(f, "syntax error: {msg}"),
            WqError::FnArgCountMismatchError(msg) => {
                write!(f, "function argument count mismatch error: {msg}")
            }
            WqError::RuntimeError(msg) => write!(f, "runtime error: {msg}"),
        }
    }
}

impl std::error::Error for WqError {}

pub type WqResult<T> = Result<T, WqError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_value_creation() {
        assert_eq!(Value::int(42), Value::Int(42));
        assert_eq!(Value::float(3.14), Value::Float(3.14));
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
    }

    #[test]
    fn test_list_operations() {
        let list = Value::list(vec![Value::int(1), Value::int(2), Value::int(3)]);

        assert_eq!(list.len(), 3);
        assert_eq!(list.index(&Value::int(0)), Some(Value::int(1)));
        assert_eq!(list.index(&Value::int(-1)), Some(Value::int(3)));
    }
}
