use std::cmp::Ordering;

use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

impl Value {
    #[inline]
    fn cmp_atom(a: &Value, b: &Value) -> Option<Ordering> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
            (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
            (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
            (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
            (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
            (Value::Symbol(x), Value::Symbol(y)) => Some(x.cmp(y)),
            (Value::Null, Value::Null) => Some(Ordering::Equal),
            _ => None,
        }
    }

    #[inline]
    fn cmp_pred<F>(&self, other: &Value, pred: F) -> WqResult<Value>
    where
        F: Fn(Ordering) -> bool + Copy,
    {
        self.bc2(other, |a, b| {
            let ord = Self::cmp_atom(a, b).ok_or(WqError::DomainError(format!(
                "Cannot compare {} of type {} and {} of type {}",
                a,
                a.type_name_verbose(),
                b,
                b.type_name_verbose()
            )))?;
            Ok(Value::Bool(pred(ord)))
        })
    }

    pub fn eq(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| Ok(Value::Bool(a == b)))
    }

    pub fn neq(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |a, b| Ok(Value::Bool(a != b)))
    }

    pub fn lt(&self, other: &Value) -> WqResult<Value> {
        self.cmp_pred(other, |o| o == Ordering::Less)
    }

    pub fn leq(&self, other: &Value) -> WqResult<Value> {
        self.cmp_pred(other, |o| o != Ordering::Greater)
    }

    pub fn gt(&self, other: &Value) -> WqResult<Value> {
        self.cmp_pred(other, |o| o == Ordering::Greater)
    }

    pub fn geq(&self, other: &Value) -> WqResult<Value> {
        self.cmp_pred(other, |o| o != Ordering::Less)
    }
}
