pub mod arith;
pub mod bit;
pub mod container;
pub mod logic;
pub mod set;

use crate::astnode::{BinaryOperator, UnaryOperator};
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
        Not => val.bnot().map_err(|e| e.src(up!("~"))),
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
        BoolAnd => eval_bool_and(left, right).map_err(|e| e.src(bp!("&|"))),
        BoolOr => eval_bool_or(left, right).map_err(|e| e.src(bp!(r"\|"))),
        BitAnd => left.band(right).map_err(|e| e.src(bp!("&"))),
        BitOr => left.bor(right).map_err(|e| e.src(bp!(r"\"))),
        BitXor => left.bxor(right).map_err(|e| e.src(bp!(r"^\"))),
        Shl => left.shl(right).map_err(|e| e.src(bp!("<<"))),
        Shr => left.shr(right).map_err(|e| e.src(bp!(">>"))),
        FloorDiv => left.floor_div(right).map_err(|e| e.src(bp!("/%"))),
        SetIntersection => left.set_intersection(right).map_err(|e| e.src(bp!(".&"))),
        SetUnion => left.set_union(right).map_err(|e| e.src(bp!(r".\"))),
        SetSymDiff => left.set_sym_diff(right).map_err(|e| e.src(bp!(".^"))),
        SetDifference => left.set_difference(right).map_err(|e| e.src(bp!(".-"))),
        SetSubset => left.set_subset(right).map_err(|e| e.src(bp!(".<"))),
        SetSubsetEq => left.set_subset_eq(right).map_err(|e| e.src(bp!(".<="))),
        SetSuperset => left.set_superset(right).map_err(|e| e.src(bp!(".>"))),
        SetSupersetEq => left.set_superset_eq(right).map_err(|e| e.src(bp!(".>="))),
    }
}

pub(crate) fn eval_bool_and(left: &Value, right: &Value) -> WqResult<Value> {
    if let Some(false) = left.try_to_rust_bool() {
        return Ok(Value::Bool(false));
    }
    if let Some(true) = left.try_to_rust_bool() {
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
    left.bc2(right, |a, b| {
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

pub(crate) fn eval_bool_or(left: &Value, right: &Value) -> WqResult<Value> {
    if let Some(true) = left.try_to_rust_bool() {
        return Ok(Value::Bool(true));
    }
    if let Some(false) = left.try_to_rust_bool() {
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
    left.bc2(right, |a, b| {
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
