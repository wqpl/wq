use std::cmp::Ordering;

use crate::{
    value::{Excerpt, Value, bc::BcResult},
    wqerr::{WqErr, WqErrType},
};

use num_bigint::BigInt;
use num_traits::ToPrimitive;

#[inline]
pub fn cmp_atom(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::BigInt(x), Value::BigInt(y)) => Some(x.cmp(y)),
        (Value::BigInt(x), Value::Int(y)) => {
            let rhs = BigInt::from(*y);
            Some(x.cmp(&rhs))
        }
        (Value::Int(x), Value::BigInt(y)) => {
            let lhs = BigInt::from(*x);
            Some(lhs.cmp(y))
        }
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::BigInt(x), Value::Float(y)) => x.to_f64().and_then(|xf| xf.partial_cmp(y)),
        (Value::Float(x), Value::BigInt(y)) => y.to_f64().and_then(|yf| x.partial_cmp(&yf)),
        // (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
        (Value::Symbol(x), Value::Symbol(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

impl Value {
    #[inline]
    fn cmp_pred<F>(&self, other: &Value, pred: F) -> BcResult<Value>
    where
        F: Fn(Ordering) -> bool + Copy,
    {
        self.bc2(other, |a, b| {
            let ord = cmp_atom(a, b).ok_or(
                WqErr::new(WqErrType::Domain)
                    .msg(format!(
                        "cannot compare {} and {}",
                        a.type_name(),
                        b.type_name()
                    ))
                    .attach_note(format!("lhs value excerpt is {}", a.excerpt()))
                    .attach_note(format!("rhs value excerpt is {}", b.excerpt())),
            )?;
            Ok(Value::Bool(pred(ord)))
        })
    }

    pub fn eq(&self, other: &Value) -> Value {
        Value::Bool(self == other)
    }

    pub fn neq(&self, other: &Value) -> Value {
        Value::Bool(self != other)
    }

    pub fn lt(&self, other: &Value) -> BcResult<Value> {
        self.cmp_pred(other, |o| o == Ordering::Less)
    }

    pub fn leq(&self, other: &Value) -> BcResult<Value> {
        self.cmp_pred(other, |o| o != Ordering::Greater)
    }

    pub fn gt(&self, other: &Value) -> BcResult<Value> {
        self.cmp_pred(other, |o| o == Ordering::Greater)
    }

    pub fn geq(&self, other: &Value) -> BcResult<Value> {
        self.cmp_pred(other, |o| o != Ordering::Less)
    }
}
