use std::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::ast::BinaryOperator;
use crate::value::{Excerpt, Value, WqResult, eval_binary};
use crate::wqerror::{WqError, WqErrorType};

fn cmp_err(a: &Value, b: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg(format!(
            "cannot compare {} and {}",
            a.type_name(),
            b.type_name()
        ))
        .attach_note(format!("lhs value excerpt is {}", a.excerpt()))
        .attach_note(format!("rhs value excerpt is {}", b.excerpt()))
}

#[inline]
pub(crate) fn cmp_atom(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::BigInt(x), Value::BigInt(y)) => Some((**x).cmp(&**y)),
        (Value::BigInt(x), Value::Int(y)) => {
            let rhs = BigInt::from(*y);
            Some((**x).cmp(&rhs))
        }
        (Value::Int(x), Value::BigInt(y)) => {
            let lhs = BigInt::from(*x);
            Some(lhs.cmp(&**y))
        }
        (lhs, rhs) if lhs.is_fraction() || rhs.is_fraction() => {
            if let (Some((ln, ld)), Some((rn, rd))) = (lhs.rational_parts(), rhs.rational_parts()) {
                Some((ln * rd).cmp(&(rn * ld)))
            } else {
                lhs.as_f64()?.partial_cmp(&rhs.as_f64()?)
            }
        }
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(&**y),
        (Value::Float(x), Value::Int(y)) => (**x).partial_cmp(&(*y as f64)),
        (Value::BigInt(x), Value::Float(y)) => x.to_f64().and_then(|xf| xf.partial_cmp(&**y)),
        (Value::Float(x), Value::BigInt(y)) => y.to_f64().and_then(|yf| (**x).partial_cmp(&yf)),
        // (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
        (Value::Tag(x), Value::Tag(y)) => Some(x.cmp(y)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::String(a), Value::List(b)) => {
            if b.iter().any(|v| !matches!(v, Value::Char(_))) {
                return None;
            }
            Some(a.chars().cmp(b.iter().filter_map(|v| {
                if let Value::Char(c) = v {
                    Some(*c)
                } else {
                    None
                }
            })))
        }
        (Value::List(a), Value::String(b)) => {
            if a.iter().any(|v| !matches!(v, Value::Char(_))) {
                return None;
            }
            Some(
                a.iter()
                    .filter_map(|v| {
                        if let Value::Char(c) = v {
                            Some(*c)
                        } else {
                            None
                        }
                    })
                    .cmp(b.chars()),
            )
        }
        _ => None,
    }
}

macro_rules! cmp_intlist {
    ($left:expr, $right:expr, $pred:expr) => {{
        match ($left, $right) {
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() != b.len() {
                    None
                } else {
                    let mut out = Vec::with_capacity(a.len());
                    for (&x, &y) in a.iter().zip(b.iter()) {
                        out.push(Value::Bool($pred(x.cmp(&y))));
                    }
                    Some(Value::from_items(out))
                }
            }
            (Value::IntList(a), Value::Int(b)) => {
                let mut out = Vec::with_capacity(a.len());
                for &x in a.iter() {
                    out.push(Value::Bool($pred(x.cmp(b))));
                }
                Some(Value::from_items(out))
            }
            (Value::Int(a), Value::IntList(b)) => {
                let mut out = Vec::with_capacity(b.len());
                for &y in b.iter() {
                    out.push(Value::Bool($pred(a.cmp(&y))));
                }
                Some(Value::from_items(out))
            }
            _ => None,
        }
    }};
}

#[inline]
fn eq_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            if a.len() != b.len() {
                return None;
            }
            let mut out = Vec::with_capacity(a.len());
            for (&x, &y) in a.iter().zip(b.iter()) {
                out.push(Value::Bool(x == y));
            }
            Some(Value::from_items(out))
        }
        (Value::IntList(a), Value::Int(b)) => {
            let mut out = Vec::with_capacity(a.len());
            for &x in a.iter() {
                out.push(Value::Bool(x == *b));
            }
            Some(Value::from_items(out))
        }
        (Value::Int(a), Value::IntList(b)) => {
            let mut out = Vec::with_capacity(b.len());
            for &y in b.iter() {
                out.push(Value::Bool(*a == y));
            }
            Some(Value::from_items(out))
        }
        _ => None,
    }
}

#[inline]
fn neq_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            if a.len() != b.len() {
                return None;
            }
            let mut out = Vec::with_capacity(a.len());
            for (&x, &y) in a.iter().zip(b.iter()) {
                out.push(Value::Bool(x != y));
            }
            Some(Value::from_items(out))
        }
        (Value::IntList(a), Value::Int(b)) => {
            let mut out = Vec::with_capacity(a.len());
            for &x in a.iter() {
                out.push(Value::Bool(x != *b));
            }
            Some(Value::from_items(out))
        }
        (Value::Int(a), Value::IntList(b)) => {
            let mut out = Vec::with_capacity(b.len());
            for &y in b.iter() {
                out.push(Value::Bool(*a != y));
            }
            Some(Value::from_items(out))
        }
        _ => None,
    }
}

impl Value {
    pub(crate) fn eq(&self, other: &Value) -> Value {
        Value::Bool(self == other)
    }

    pub(crate) fn eq_bc(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = eq_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return Ok(Value::Bool(self == other));
        }
        self.bc2(other, |a, b| Ok(Value::Bool(a == b)))
            .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn neq(&self, other: &Value) -> Value {
        Value::Bool(self != other)
    }

    pub(crate) fn neq_bc(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = neq_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return Ok(Value::Bool(self != other));
        }
        self.bc2(other, |a, b| Ok(Value::Bool(a != b)))
            .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn lt(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = cmp_intlist!(self, other, |ord: Ordering| ord == Ordering::Less) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            let ord = cmp_atom(self, other).ok_or_else(|| cmp_err(self, other))?;
            return Ok(Value::Bool(ord == Ordering::Less));
        }
        self.bc2(other, |a, b| {
            let ord = cmp_atom(a, b).ok_or_else(|| cmp_err(a, b))?;
            Ok(Value::Bool(ord == Ordering::Less))
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn leq(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = cmp_intlist!(self, other, |ord: Ordering| ord != Ordering::Greater) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            let ord = cmp_atom(self, other).ok_or_else(|| cmp_err(self, other))?;
            return Ok(Value::Bool(ord != Ordering::Greater));
        }
        self.bc2(other, |a, b| {
            let ord = cmp_atom(a, b).ok_or_else(|| cmp_err(a, b))?;
            Ok(Value::Bool(ord != Ordering::Greater))
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn gt(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = cmp_intlist!(self, other, |ord: Ordering| ord == Ordering::Greater) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            let ord = cmp_atom(self, other).ok_or_else(|| cmp_err(self, other))?;
            return Ok(Value::Bool(ord == Ordering::Greater));
        }
        self.bc2(other, |a, b| {
            let ord = cmp_atom(a, b).ok_or_else(|| cmp_err(a, b))?;
            Ok(Value::Bool(ord == Ordering::Greater))
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn geq(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = cmp_intlist!(self, other, |ord: Ordering| ord != Ordering::Less) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            let ord = cmp_atom(self, other).ok_or_else(|| cmp_err(self, other))?;
            return Ok(Value::Bool(ord != Ordering::Less));
        }
        self.bc2(other, |a, b| {
            let ord = cmp_atom(a, b).ok_or_else(|| cmp_err(a, b))?;
            Ok(Value::Bool(ord != Ordering::Less))
        })
        .map_err(|e| e.into_wqerror())
    }
}

#[inline]
pub(crate) fn eval_cmp_chain(ops: &[BinaryOperator], values: &[Value]) -> WqResult<Value> {
    debug_assert_eq!(ops.len() + 1, values.len());

    if ops.is_empty() || values.is_empty() {
        return Ok(Value::Bool(true));
    }

    let mut result = Value::Bool(true);
    let mut left = &values[0];

    for (idx, op) in ops.iter().enumerate() {
        let right = &values[idx + 1];
        let cmp = eval_binary(op, left, right)?;
        result = result.bool_and(&cmp).map_err(|e| e.src("cmp-chain"))?;
        if matches!(result, Value::Bool(false)) {
            break;
        }
        left = right;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ast::BinaryOperator;

    #[test]
    fn chain_all_true_scalar() {
        let ops = [BinaryOperator::Lt, BinaryOperator::Lte];
        let values = [Value::Int(1), Value::Int(2), Value::Int(2)];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn chain_false_scalar() {
        let ops = [BinaryOperator::Lt, BinaryOperator::Lt];
        let values = [Value::Int(2), Value::Int(1), Value::Int(3)];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn chain_list_broadcast() {
        let ops = [BinaryOperator::Lt, BinaryOperator::Lt];
        let values = [
            Value::from_items(vec![Value::Int(1), Value::Int(3)]),
            Value::from_items(vec![Value::Int(2), Value::Int(2)]),
            Value::from_items(vec![Value::Int(3), Value::Int(4)]),
        ];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::Bool(true), Value::Bool(false)]))
        );
    }

    #[test]
    fn chain_scalar_with_list_broadcast() {
        let ops = [BinaryOperator::Lt, BinaryOperator::Lt];
        let values = [
            Value::Int(0),
            Value::from_items(vec![Value::Int(1), Value::Int(2)]),
            Value::Int(3),
        ];
        let result = eval_cmp_chain(&ops, &values).expect("cmp chain result");
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::Bool(true), Value::Bool(true)]))
        );
    }
}
