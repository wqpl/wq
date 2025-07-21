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
            (Value::Dict(map), Value::Symbol(key)) => map.get(key).cloned(),
            (Value::Dict(map), Value::List(keys)) => {
                let mut result = HashMap::new();
                for key_val in keys {
                    if let Value::Symbol(k) = key_val {
                        if let Some(v) = map.get(k) {
                            result.insert(k.clone(), v.clone());
                        } else {
                            return None;
                        }
                    } else {
                        return None;
                    }
                }
                Some(Value::Dict(result))
            }
            _ => None,
        }
    }

    /// Mutate list or dictionary element by index/key. Returns `Some(())` on
    /// success and `None` if the key does not exist or the types are
    /// incompatible.
    pub fn set_index(&mut self, key: &Value, value: Value) -> Option<()> {
        match (self, key) {
            (Value::List(items), Value::Int(i)) => {
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
            }
            (Value::Dict(map), Value::Symbol(key_str)) => {
                if map.contains_key(key_str) {
                    map.insert(key_str.clone(), value);
                    Some(())
                } else {
                    None
                }
            }
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

    pub fn modulo(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Int(a % b))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(a % b))
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(*a as f64 % b))
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Float(a % *b as f64))
                }
            }
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.modulo(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.modulo(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.modulo(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.modulo(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical AND with broadcasting support
    pub fn and_bool(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a && *b)),
            (scalar @ Value::Bool(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.and_bool(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Bool(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.and_bool(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.and_bool(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.and_bool(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical OR with broadcasting support
    pub fn or_bool(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a || *b)),
            (scalar @ Value::Bool(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.or_bool(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Bool(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.or_bool(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.or_bool(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.or_bool(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical XOR with broadcasting support
    pub fn xor_bool(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a ^ *b)),
            (scalar @ Value::Bool(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.xor_bool(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Bool(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.xor_bool(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.xor_bool(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.xor_bool(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical NOT with broadcasting support
    pub fn not_bool(&self) -> Option<Value> {
        match self {
            Value::Bool(b) => Some(Value::Bool(!b)),
            Value::List(items) => {
                let result: Option<Vec<Value>> = items.iter().map(|v| v.not_bool()).collect();
                result.map(Value::List)
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

    /// Helper for vectorized comparisons
    fn cmp_broadcast<F>(&self, other: &Value, cmp: F) -> Value
    where
        F: Fn(&Value, &Value) -> bool + Copy,
    {
        match (self, other) {
            (Value::List(_), Value::List(_)) if self.is_string() && other.is_string() => {
                Value::Bool(cmp(self, other))
            }
            (Value::List(a), Value::List(b)) => {
                if a.is_empty() || b.is_empty() {
                    Value::List(Vec::new())
                } else if a.len() == b.len() {
                    let result: Vec<Value> = a
                        .iter()
                        .zip(b.iter())
                        .map(|(x, y)| Value::Bool(cmp(x, y)))
                        .collect();
                    Value::List(result)
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Vec<Value> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            Value::Bool(cmp(left, right))
                        })
                        .collect();
                    Value::List(result)
                }
            }
            (Value::List(a), b) => {
                let result: Vec<Value> = a.iter().map(|x| Value::Bool(cmp(x, b))).collect();
                Value::List(result)
            }
            (a, Value::List(b)) => {
                let result: Vec<Value> = b.iter().map(|y| Value::Bool(cmp(a, y))).collect();
                Value::List(result)
            }
            (a, b) => Value::Bool(cmp(a, b)),
        }
    }

    /// Comparison operations
    pub fn equals(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(Value::compare_values(a, b), Some(std::cmp::Ordering::Equal))
        })
    }

    pub fn not_equals(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            !matches!(Value::compare_values(a, b), Some(std::cmp::Ordering::Equal))
        })
    }

    pub fn less_than(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(Value::compare_values(a, b), Some(std::cmp::Ordering::Less))
        })
    }

    pub fn less_than_or_equal(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(
                Value::compare_values(a, b),
                Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
            )
        })
    }

    pub fn greater_than(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(
                Value::compare_values(a, b),
                Some(std::cmp::Ordering::Greater)
            )
        })
    }

    pub fn greater_than_or_equal(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(
                Value::compare_values(a, b),
                Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
            )
        })
    }
}

pub mod box_mode {
    use std::sync::atomic::{AtomicBool, Ordering};
    static BOXED: AtomicBool = AtomicBool::new(false);

    pub fn set_boxed(on: bool) {
        BOXED.store(on, Ordering::Release);
    }
    pub fn is_boxed() -> bool {
        BOXED.load(Ordering::Acquire)
    }
}

fn repr(v: &Value) -> String {
    match v {
        Value::List(items) => {
            let inner: Vec<String> = items.iter().map(|c| repr(c)).collect();
            format!("({})", inner.join(" "))
        }
        other => other.to_string(),
    }
}

fn format_table(rows: &[Value]) -> Option<String> {
    if rows.is_empty() {
        return None;
    }

    let all_rows_are_strings = rows.iter().all(|row| match row {
        Value::List(chars) => chars.iter().all(|c| matches!(c, Value::Char(_))),
        Value::Char(_) => true,
        _ => false,
    });

    if all_rows_are_strings {
        let lines = rows
            .iter()
            .map(|row| match row {
                Value::Char(c) => format!("\"{}\"", c),
                other => other.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        return Some(lines);
    }

    let has_nested = rows.iter().any(|row| {
        if let Value::List(cells) = row {
            cells.iter().any(|c| matches!(c, Value::List(_)))
        } else {
            false
        }
    });

    if has_nested {
        let lines = rows.iter().map(|row| {
            if let Value::List(cells) = row {
                cells.iter().map(|c| repr(c)).collect::<Vec<_>>().join(" ")
            } else {
                repr(row)
            }
        });
        return Some(lines.collect::<Vec<_>>().join("\n"));
    }

    //actual alignment
    let table: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            if let Value::List(cells) = row {
                cells.iter().map(|c| repr(c)).collect()
            } else {
                vec![repr(row)]
            }
        })
        .collect();

    let ncols = table.iter().map(Vec::len).max().unwrap_or(0);
    if ncols == 0 {
        return None;
    }

    let mut widths = vec![0; ncols];
    for row in &table {
        for (j, cell) in row.iter().enumerate() {
            widths[j] = widths[j].max(cell.len());
        }
    }

    let mut lines = Vec::with_capacity(table.len());
    for row in &table {
        let mut parts = Vec::with_capacity(ncols);
        for (j, &w) in widths.iter().enumerate() {
            let text = row.get(j).map(String::as_str).unwrap_or("");
            parts.push(format!("{:<width$}", text, width = w));
        }
        lines.push(parts.join(" ").trim_end().to_string());
    }

    Some(lines.join("\n"))
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
            Value::Symbol(s) => write!(f, "`{s}"),
            Value::Bool(b) => write!(f, "{}", if *b { "true" } else { "false" }),
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
                    return write!(f, "\"{}\"", s);
                }

                // 3. In boxed mode, try table formatting first
                if box_mode::is_boxed() {
                    if let Some(table) = format_table(items) {
                        return write!(f, "{}", table);
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
            None
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
}
