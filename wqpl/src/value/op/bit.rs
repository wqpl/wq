use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::ast::{BinaryOperator, UnaryOperator};
use crate::value::op::arith::{int_bigint_pair, intlist_map, intlist_zip_map};
use crate::value::{Value, WqResult, expected_integer1, expected_integer2};
use crate::wqerror::{Bound, Requirement, WqError, WqErrorType};

fn invalid_shift(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .expected(Requirement::int_range(
            Bound::Included(0),
            Bound::Included(i128::from(u32::MAX)),
        ))
        .attach_note("for a shift count")
        .got1(v)
}

fn shift_count_i64(n: i64, original: &Value) -> WqResult<u32> {
    u32::try_from(n).map_err(|_| invalid_shift(original))
}

fn shl_i64_exact(x: i64, shift: u32) -> Value {
    if x == 0 {
        return Value::Int(0);
    }
    if let Some(factor) = 1_i64.checked_shl(shift)
        && factor > 0
        && let Some(result) = x.checked_mul(factor)
    {
        return Value::Int(result);
    }
    Value::from_bigint(BigInt::from(x) << shift)
}

fn shr_i64_exact(x: i64, shift: u32) -> Value {
    if shift >= i64::BITS {
        Value::Int(if x < 0 { -1 } else { 0 })
    } else {
        Value::Int(x >> shift)
    }
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
            let shift = u32::try_from(*b).ok()?;
            Some(Value::from_items(
                a.iter().map(|&x| shl_i64_exact(x, shift)).collect(),
            ))
        }
        _ => None,
    }
}

fn shr_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::Int(b)) => {
            let shift = u32::try_from(*b).ok()?;
            Some(Value::from_items(
                a.iter().map(|&x| shr_i64_exact(x, shift)).collect(),
            ))
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
        (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(*x & *y)),
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
        (Value::Bool(x), Value::Bool(y)) => Ok(Value::Bool(*x | *y)),
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
            let shift = shift_count_i64(*s, b)?;
            Ok(shl_i64_exact(*x, shift))
        }
        (Value::BigInt(x), Value::Int(s)) => {
            let shift = shift_count_i64(*s, b)?;
            Ok(Value::from_bigint(&**x << shift))
        }
        (Value::Int(x), Value::BigInt(s)) => {
            let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
            Ok(shl_i64_exact(*x, shift))
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
            let shift = shift_count_i64(*s, b)?;
            Ok(shr_i64_exact(*x, shift))
        }
        (Value::BigInt(x), Value::Int(s)) => {
            let shift = shift_count_i64(*s, b)?;
            Ok(Value::from_bigint(&**x >> shift))
        }
        (Value::Int(x), Value::BigInt(s)) => {
            let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
            Ok(shr_i64_exact(*x, shift))
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

    pub(crate) fn bxor(&self, other: &Value) -> WqResult<Value> {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn shifts_reject_counts_that_do_not_fit_u32() {
        let count = Value::Int(i64::from(u32::MAX) + 1);

        let error = Value::Int(1)
            .shl(&count)
            .expect_err("oversized shift should fail");
        assert_eq!(
            error.msg.as_deref(),
            Some("expected int from 0 through 4294967295")
        );
        assert_eq!(
            error.notes.as_slice(),
            ["for a shift count", "got 4294967296 (int)"]
        );
        assert!(Value::Int(8).shr(&count).is_err());
    }

    #[test]
    fn int_list_shifts_reject_counts_that_do_not_fit_u32() {
        let values = Value::IntList(Arc::new(vec![1, 2]));
        let count = Value::Int(i64::from(u32::MAX) + 1);

        assert!(values.shl(&count).is_err());
        assert!(values.shr(&count).is_err());
    }

    #[test]
    fn int_left_shifts_are_exact_and_promote() {
        assert_eq!(
            Value::Int(1)
                .shl(&Value::Int(63))
                .expect("shift should succeed"),
            Value::from_bigint(BigInt::from(1) << 63_u32)
        );
        assert_eq!(
            Value::Int(i64::MIN)
                .shl(&Value::Int(1))
                .expect("shift should succeed"),
            Value::from_bigint(BigInt::from(i64::MIN) << 1_u32)
        );
    }

    #[test]
    fn int_right_shifts_are_exact_arithmetic_shifts() {
        assert_eq!(
            Value::Int(8)
                .shr(&Value::Int(64))
                .expect("shift should succeed"),
            Value::Int(0)
        );
        assert_eq!(
            Value::Int(-3)
                .shr(&Value::Int(1))
                .expect("shift should succeed"),
            Value::Int(-2)
        );
        assert_eq!(
            Value::Int(-1)
                .shr(&Value::Int(128))
                .expect("shift should succeed"),
            Value::Int(-1)
        );
    }

    #[test]
    fn int_list_left_shift_widens_when_needed() {
        let values = Value::IntList(Arc::new(vec![1, 2]));

        assert_eq!(
            values.shl(&Value::Int(2)).expect("shift should succeed"),
            Value::IntList(Arc::new(vec![4, 8]))
        );
        assert_eq!(
            values.shl(&Value::Int(63)).expect("shift should succeed"),
            Value::from_items(vec![
                Value::from_bigint(BigInt::from(1) << 63_u32),
                Value::from_bigint(BigInt::from(2) << 63_u32),
            ])
        );
    }

    #[test]
    fn int_list_right_shift_stays_packed() {
        let values = Value::IntList(Arc::new(vec![8, -3, -1]));

        assert_eq!(
            values.shr(&Value::Int(1)).expect("shift should succeed"),
            Value::IntList(Arc::new(vec![4, -2, -1]))
        );
        assert_eq!(
            values.shr(&Value::Int(64)).expect("shift should succeed"),
            Value::IntList(Arc::new(vec![0, -1, -1]))
        );
    }

    #[test]
    fn bitwise_logical_ops_accept_bool_pairs() {
        assert_eq!(
            Value::Bool(true)
                .band(&Value::Bool(false))
                .expect("bool band should succeed"),
            Value::Bool(false)
        );
        assert_eq!(
            Value::Bool(true)
                .bor(&Value::Bool(false))
                .expect("bool bor should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            Value::Bool(true)
                .bxor(&Value::Bool(false))
                .expect("bool xor should succeed"),
            Value::Bool(true)
        );
    }
}
