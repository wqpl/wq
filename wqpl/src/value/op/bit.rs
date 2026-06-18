use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::value::op::arith::{int_bigint_pair, intlist_map, intlist_zip_map};
use crate::value::{Value, WqResult, expected_integer1, expected_integer2};
use crate::wqerror::{WqError, WqErrorType};

fn invalid_shift(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("shift must be in 0..4_294_967_295")
        .got1(v)
}

fn band_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| Some(x & y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            intlist_map(a, |x| Some(x & *b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| Some(*a & y)).map(|v| Value::IntList(Arc::new(v)))
        }
        _ => None,
    }
}

fn bor_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| Some(x | y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            intlist_map(a, |x| Some(x | *b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| Some(*a | y)).map(|v| Value::IntList(Arc::new(v)))
        }
        _ => None,
    }
}

fn xor_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| Some(x ^ y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            intlist_map(a, |x| Some(x ^ *b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| Some(*a ^ y)).map(|v| Value::IntList(Arc::new(v)))
        }
        _ => None,
    }
}

fn not_intlist(v: &Value) -> Option<Value> {
    match v {
        Value::IntList(a) => intlist_map(a, |x| Some(!x)).map(|v| Value::IntList(Arc::new(v))),
        _ => None,
    }
}

fn shl_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::Int(b)) => {
            if *b < 0 {
                return None;
            }
            intlist_map(a, |x| Some(x.wrapping_shl(*b as u32))).map(|v| Value::IntList(Arc::new(v)))
        }
        _ => None,
    }
}

fn shr_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::Int(b)) => {
            if *b < 0 {
                return None;
            }
            intlist_map(a, |x| Some(x.wrapping_shr(*b as u32))).map(|v| Value::IntList(Arc::new(v)))
        }
        _ => None,
    }
}

fn band_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::BitAnd, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x & y)),
        (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x & &**y)),
        _ => {
            if let Some((x, y)) = int_bigint_pair(a, b) {
                return Ok(Value::from_bigint(BigInt::from(x) & y));
            }
            Err(expected_integer2(a, b))
        }
    }
}

fn bor_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::BitOr, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x | y)),
        (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x | &**y)),
        _ => {
            if let Some((x, y)) = int_bigint_pair(a, b) {
                return Ok(Value::from_bigint(BigInt::from(x) | y));
            }
            Err(expected_integer2(a, b))
        }
    }
}

fn xor_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::BitXor, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x ^ y)),
        (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x ^ &**y)),
        (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(x ^ y)),
        _ => {
            if let Some((x, y)) = int_bigint_pair(a, b) {
                return Ok(Value::from_bigint(BigInt::from(x) ^ y));
            }
            Err(expected_integer2(a, b))
        }
    }
}

fn not_atom(v: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_unary(UnaryOperator::Not, v) {
        return Ok(res);
    }
    match v {
        Value::Int(x) => Ok(Value::Int(!x)),
        Value::BigInt(x) => Ok(Value::from_bigint(!&**x)),
        Value::Bool(b) => Ok(Value::Bool(!b)),
        _ => Err(expected_integer1(v)),
    }
}

fn shl_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Shl, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(s)) => {
            if *s < 0 {
                Err(invalid_shift(b))
            } else {
                Ok(Value::Int(x.wrapping_shl(*s as u32)))
            }
        }
        (Value::BigInt(x), Value::Int(s)) if *s >= 0 => Ok(Value::from_bigint(&**x << (*s as u32))),
        (Value::Int(x), Value::BigInt(s)) => {
            let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
            Ok(Value::Int(x.wrapping_shl(shift)))
        }
        (Value::BigInt(x), Value::BigInt(s)) => {
            let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
            Ok(Value::from_bigint(&**x << shift))
        }
        _ => Err(expected_integer2(a, b)),
    }
}

fn shr_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Shr, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(s)) => {
            if *s < 0 {
                Err(invalid_shift(b))
            } else {
                Ok(Value::Int(x.wrapping_shr(*s as u32)))
            }
        }
        (Value::BigInt(x), Value::Int(s)) if *s >= 0 => Ok(Value::from_bigint(&**x >> (*s as u32))),
        (Value::Int(x), Value::BigInt(s)) => {
            let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
            Ok(Value::Int(x.wrapping_shr(shift)))
        }
        (Value::BigInt(x), Value::BigInt(s)) => {
            let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
            Ok(Value::from_bigint(&**x >> shift))
        }
        _ => Err(expected_integer2(a, b)),
    }
}

impl Value {
    pub(crate) fn band(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = band_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return band_atoms(self, other);
        }
        self.bc2(other, band_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn bor(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = bor_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return bor_atoms(self, other);
        }
        self.bc2(other, bor_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn xor(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = xor_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return xor_atoms(self, other);
        }
        self.bc2(other, xor_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn not(&self) -> WqResult<Value> {
        if let Some(res) = not_intlist(self) {
            return Ok(res);
        }
        if self.is_atom() {
            return not_atom(self);
        }
        self.bc1(not_atom).map_err(|e| e.into_wqerror())
    }

    // Shifts (reject negative shift counts)

    pub(crate) fn shl(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = shl_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return shl_atoms(self, other);
        }
        self.bc2(other, shl_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn shr(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = shr_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return shr_atoms(self, other);
        }
        self.bc2(other, shr_atoms).map_err(|e| e.into_wqerror())
    }
}
