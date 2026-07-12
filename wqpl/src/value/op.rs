pub mod arith;
pub mod bit;
pub mod char;
pub mod container;

use crate::ast::{BinaryOperator, BoolOperator, UnaryOperator};
use crate::value::convert::IntoWqValue;
use crate::value::{Value, WqResult, expected_bool1, expected_bool2};

/// Minimum length for intlist fast-path operations to switch to parallel
/// iteration.
pub(crate) const PAR_BC_THRESHOLD: usize = 4096;

#[inline]
pub(crate) fn eval_unary(op: &UnaryOperator, val: &Value) -> WqResult<Value> {
    use UnaryOperator::*;

    macro_rules! up {
        ($s:literal) => {
            concat!("unary operator ", $s)
        };
    }

    match op {
        Negate => val.neg().map_err(|e| e.src(up!("-"))),
        Count => Ok(val.len().into_wq_value()),
        Not => val.not().map_err(|e| e.src(up!("~"))),
    }
}

#[inline]
pub(crate) fn eval_binary(op: &BinaryOperator, left: &Value, right: &Value) -> WqResult<Value> {
    use BinaryOperator::*;

    macro_rules! bp {
        ($s:literal) => {
            concat!("binary operator ", $s)
        };
    }

    match op {
        Add => left.add(right).map_err(|e| e.src(bp!("+"))),
        Subtract => left.subtract(right).map_err(|e| e.src(bp!("-"))),
        Multiply => left.multiply(right).map_err(|e| e.src(bp!("*"))),
        Power => left.power(right).map_err(|e| e.src(bp!("^"))),
        PowerDot => left.power_dot(right).map_err(|e| e.src(bp!("^."))),
        Divide => left.divide(right).map_err(|e| e.src(bp!("/"))),
        DivideDot => left.divide_dot(right).map_err(|e| e.src(bp!("/."))),
        Modulo => left.modulo(right).map_err(|e| e.src(bp!("%"))),
        Matmul => left.mm(right).map_err(|e| e.src(bp!("**"))),

        Equal => Ok(left.eq(right)),
        EqualDot => left.eq_bc(right).map_err(|e| e.src(bp!("=."))),
        NotEqual => Ok(left.neq(right)),
        NotEqualDot => left.neq_bc(right).map_err(|e| e.src(bp!("~."))),
        Lt => left.lt(right).map_err(|e| e.src(bp!("<"))),
        Lte => left.leq(right).map_err(|e| e.src(bp!("<="))),
        Gt => left.gt(right).map_err(|e| e.src(bp!(">"))),
        Gte => left.geq(right).map_err(|e| e.src(bp!(">="))),
        Cat => Ok(left.clone().cat(right.clone())),
        BitAnd => left.band(right).map_err(|e| e.src("band")),
        BitOr => left.bor(right).map_err(|e| e.src("bor")),
        BitXor => left.bxor(right).map_err(|e| e.src("bxor")),
        Shl => left.shl(right).map_err(|e| e.src("shl")),
        Shr => left.shr(right).map_err(|e| e.src("shr")),
        FloorDiv => left.floor_div(right).map_err(|e| e.src(bp!("/%"))),
    }
}

#[inline]
pub(crate) fn eval_bool_op(op: BoolOperator, left: &Value, right: &Value) -> WqResult<Value> {
    match op {
        BoolOperator::And => left.bool_and(right).map_err(|e| e.src("A/and")),
        BoolOperator::Or => left.bool_or(right).map_err(|e| e.src("O/or")),
    }
}

impl Value {
    pub(crate) fn bool_and(&self, right: &Value) -> WqResult<Value> {
        if let Some(false) = self.try_to_rust_bool() {
            return Ok(Value::Bool(false));
        }
        if let Some(true) = self.try_to_rust_bool() {
            return right
                .bc1(|v| {
                    if let Some(y) = v.try_to_rust_bool() {
                        Ok(Value::Bool(y))
                    } else {
                        Err(expected_bool1(v))
                    }
                })
                .map_err(|e| e.into_wqerror());
        }
        self.bc2(right, |a, b| {
            if let Some(false) = a.try_to_rust_bool() {
                return Ok(Value::Bool(false));
            }
            if let Some(true) = a.try_to_rust_bool() {
                if let Some(y) = b.try_to_rust_bool() {
                    return Ok(Value::Bool(y));
                }
                return Err(expected_bool1(b));
            }
            Err(expected_bool2(a, b))
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn bool_or(&self, right: &Value) -> WqResult<Value> {
        if let Some(true) = self.try_to_rust_bool() {
            return Ok(Value::Bool(true));
        }
        if let Some(false) = self.try_to_rust_bool() {
            return right
                .bc1(|v| {
                    if let Some(y) = v.try_to_rust_bool() {
                        Ok(Value::Bool(y))
                    } else {
                        Err(expected_bool1(v))
                    }
                })
                .map_err(|e| e.into_wqerror());
        }
        self.bc2(right, |a, b| {
            if let Some(true) = a.try_to_rust_bool() {
                return Ok(Value::Bool(true));
            }
            if let Some(false) = a.try_to_rust_bool() {
                if let Some(y) = b.try_to_rust_bool() {
                    return Ok(Value::Bool(y));
                }
                return Err(expected_bool1(b));
            }
            Err(expected_bool2(a, b))
        })
        .map_err(|e| e.into_wqerror())
    }
}
