use crate::{astnode::BinaryOperator, value::Value};

use num_bigint::BigInt;
use num_traits::Zero;

#[inline]
pub fn fp_int_binary_op(op: BinaryOperator, left: &Value, right: &Value) -> Option<Value> {
    use BinaryOperator::*;
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => match op {
            Add => a.checked_add(*b).map(Value::Int),
            Subtract => a.checked_sub(*b).map(Value::Int),
            Multiply => a.checked_mul(*b).map(Value::Int),
            Divide => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Float(*a as f64 / *b as f64))
                }
            }
            DivideDot => Some(Value::Float(*a as f64 / *b as f64)),
            Modulo => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Int(*a % *b))
                }
            }
            ModuloDot => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Float(*a as f64 % *b as f64))
                }
            }
            LessThan => Some(Value::Bool(a < b)),
            LessThanOrEqual => Some(Value::Bool(a <= b)),
            GreaterThan => Some(Value::Bool(a > b)),
            GreaterThanOrEqual => Some(Value::Bool(a >= b)),
            Equal => Some(Value::Bool(a == b)),
            NotEqual => Some(Value::Bool(a != b)),
            _ => None,
        },
        _ => None,
    }
}

#[inline]
pub fn fp_floor_div(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => {
            if *b == 0 {
                return None;
            }
            // Handle the only overflowing division case: MIN / -1
            if *a == i64::MIN && *b == -1 {
                let qa = BigInt::from(*a);
                let qb = BigInt::from(*b);
                let q0 = &qa / &qb; // exact here
                return Some(Value::from_bigint(q0));
            }
            // Safe to use primitive ops
            let q0 = *a / *b; // trunc toward zero
            let r = *a % *b;
            if r == 0 || (a ^ b) >= 0 {
                Some(Value::Int(q0))
            } else {
                // floor requires subtracting 1; if it overflows, fall back to BigInt
                if let Some(qm1) = q0.checked_sub(1) {
                    Some(Value::Int(qm1))
                } else {
                    let qa = BigInt::from(*a);
                    let qb = BigInt::from(*b);
                    let q0b = &qa / &qb; // trunc toward zero
                    let rb = &qa % &qb;
                    let adjust = !rb.is_zero()
                        && ((qa.sign() == num_bigint::Sign::Minus)
                            ^ (qb.sign() == num_bigint::Sign::Minus));
                    let qb_floor = if adjust { q0b - 1 } else { q0b };
                    Some(Value::from_bigint(qb_floor))
                }
            }
        }
        _ => None,
    }
}
