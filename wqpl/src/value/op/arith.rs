use std::sync::Arc;

use num_bigint::BigInt;
use num_complex::Complex64;
use num_traits::{One, Signed, ToPrimitive, Zero};
use rayon::prelude::*;

use crate::ast::{BinaryOperator, UnaryOperator};
use crate::cas::{cas_binary_expr, cas_unary_expr};
use crate::value::cas::CasOp;
use crate::value::op::PAR_BC_THRESHOLD;
use crate::value::seq::IntRangeData;
use crate::value::{Value, WqResult, expected_numeric1, expected_numeric2};
use crate::wqerror::{WqError, WqErrorType};

fn algebraic_binary_op(op: &str, a: &Value, b: &Value) -> WqResult<Value> {
    use crate::value::algebraic;
    let Some((a, b)) = algebraic::coerce_to_common_field(a, b)? else {
        unreachable!()
    };
    let Value::Algebraic(aa) = &a else {
        unreachable!()
    };
    let Value::Algebraic(ab) = &b else {
        unreachable!()
    };
    match op {
        "+" => algebraic::algebraic_add(aa, ab),
        "-" => algebraic::algebraic_sub(aa, ab),
        "*" => algebraic::algebraic_mul(aa, ab),
        "/" => algebraic::algebraic_div(aa, ab),
        _ => unreachable!(),
    }
}

fn zero_div_err(msg: Option<&'static str>) -> WqError {
    let msg = msg.unwrap_or("cannot divide by 0").to_string();
    WqError::new(WqErrorType::ZeroDiv).msg(msg)
}

fn zero_to_negative_power_err() -> WqError {
    zero_div_err(Some("0 cannot be raised to a negative power"))
}

fn bigint_too_big_for_float() -> WqError {
    WqError::new(WqErrorType::Domain).msg("provided bigint is too big to convert to float")
}

fn is_zero(v: &Value) -> bool {
    match v {
        Value::Int(0) => true,
        Value::Float(f) if **f == 0.0 => true,
        Value::BigInt(n) if n.is_zero() => true,
        _ => false,
    }
}

fn complex_operand(v: &Value) -> WqResult<Complex64> {
    match v {
        Value::BigInt(n) => n
            .to_f64()
            .map(|re| Complex64::new(re, 0.0))
            .ok_or_else(bigint_too_big_for_float),
        _ => v.try_as_complex64(),
    }
}

fn complex_operands(a: &Value, b: &Value) -> WqResult<(Complex64, Complex64)> {
    Ok((complex_operand(a)?, complex_operand(b)?))
}

fn rational_operands(a: &Value, b: &Value) -> Option<((BigInt, BigInt), (BigInt, BigInt))> {
    Some((a.rational_parts()?, b.rational_parts()?))
}

fn rational_exponent(v: &Value) -> Option<BigInt> {
    let (numer, denom) = v.rational_parts()?;
    if denom.is_one() { Some(numer) } else { None }
}

fn exact_nth_root_bigint(n: &BigInt, q: u32) -> Option<BigInt> {
    if q == 0 {
        return None;
    }
    if n.is_zero() || n.is_one() {
        return Some(n.clone());
    }
    if n.is_negative() && q.is_multiple_of(2) {
        return None;
    }

    let abs_n = n.abs();
    let root_f = abs_n.to_f64()?.powf(1.0 / q as f64);
    if !root_f.is_finite() || root_f > i64::MAX as f64 {
        return None;
    }

    let candidate = root_f.round() as i64;
    for c in [candidate - 1, candidate, candidate + 1] {
        if c < 0 {
            continue;
        }
        let root = BigInt::from(c);
        if root.pow(q) == abs_n {
            return Some(if n.is_negative() { -root } else { root });
        }
    }
    None
}

fn exact_rational_fractional_pow(
    base_n: &BigInt,
    base_d: &BigInt,
    exp_n: &BigInt,
    exp_d: &BigInt,
) -> Option<Value> {
    let q = exp_d.to_u32()?;
    let root_n = exact_nth_root_bigint(base_n, q)?;
    let root_d = exact_nth_root_bigint(base_d, q)?;
    let p = exp_n.abs().to_u32()?;

    if exp_n.is_negative() {
        Some(Value::from_fraction_parts(root_d.pow(p), root_n.pow(p)))
    } else {
        Some(Value::from_fraction_parts(root_n.pow(p), root_d.pow(p)))
    }
}

fn int_float_pair(a: &Value, b: &Value) -> Option<(i64, f64)> {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) => Some((*x, **y)),
        (Value::Float(x), Value::Int(y)) => Some((*y, **x)),
        _ => None,
    }
}

pub(crate) fn int_bigint_pair<'a>(a: &'a Value, b: &'a Value) -> Option<(i64, &'a BigInt)> {
    match (a, b) {
        (Value::Int(x), Value::BigInt(y)) => Some((*x, y)),
        (Value::BigInt(x), Value::Int(y)) => Some((*y, x)),
        _ => None,
    }
}

fn float_bigint_pair<'a>(a: &'a Value, b: &'a Value) -> Option<(f64, &'a BigInt)> {
    match (a, b) {
        (Value::Float(x), Value::BigInt(y)) => Some((**x, y)),
        (Value::BigInt(x), Value::Float(y)) => Some((**y, x)),
        _ => None,
    }
}

fn int_float_ordered(a: &Value, b: &Value) -> Option<(f64, f64)> {
    match (a, b) {
        (Value::Int(x), Value::Float(y)) => Some((*x as f64, **y)),
        (Value::Float(x), Value::Int(y)) => Some((**x, *y as f64)),
        _ => None,
    }
}

fn bigint_float_ordered<'a>(a: &'a Value, b: &'a Value) -> Option<(&'a BigInt, f64)> {
    match (a, b) {
        (Value::BigInt(x), Value::Float(y)) => Some((x, **y)),
        _ => None,
    }
}

fn float_bigint_ordered<'a>(a: &'a Value, b: &'a Value) -> Option<(f64, &'a BigInt)> {
    match (a, b) {
        (Value::Float(x), Value::BigInt(y)) => Some((**x, y)),
        _ => None,
    }
}

fn neg_atom(v: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_unary(UnaryOperator::Negate, v) {
        return Ok(res);
    }
    match v {
        Value::Int(n) => Ok(n
            .checked_neg()
            .map(Value::Int)
            .unwrap_or_else(|| Value::BigInt(Arc::new(-BigInt::from(*n))))),
        Value::Float(f) => Ok(Value::float(-f)),
        Value::BigInt(n) => Ok(Value::BigInt(Arc::new(-&**n))),
        _ if v.is_cas_expr() => cas_unary_expr(CasOp::Subtract, v),
        _ if v.is_complex() => Ok(Value::from_complex64(-complex_operand(v)?)),
        _ if v.is_fraction() => {
            let (numer, denom) = v.dict_fraction_parts().unwrap();
            Ok(Value::from_fraction_parts(-numer, denom))
        }
        _ if v.is_algebraic_number() => {
            if let Value::Algebraic(a) = v {
                Ok(crate::value::algebraic::algebraic_neg(a))
            } else {
                unreachable!()
            }
        }
        _ => Err(expected_numeric1(v)),
    }
}

fn exponent_too_large_err() -> WqError {
    WqError::new(WqErrorType::Domain).msg("exponent cannot exceed 4_294_967_295")
}

fn add_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Add, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x
            .checked_add(*y)
            .map(Value::Int)
            .unwrap_or_else(|| Value::from_bigint(BigInt::from(*x) + BigInt::from(*y)))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::float(x + y)),
        _ => {
            if let Some((x, y)) = int_float_pair(a, b) {
                return Ok(Value::float(x as f64 + y));
            }
            if let Some((x, y)) = int_bigint_pair(a, b) {
                return Ok(Value::from_bigint(BigInt::from(x) + y));
            }
            if let Some((x, y)) = float_bigint_pair(a, b) {
                return y
                    .to_f64()
                    .map(|yf| Value::float(x + yf))
                    .ok_or_else(bigint_too_big_for_float);
            }
            match (a, b) {
                (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x + &**y)),
                _ if a.is_cas_expr() || b.is_cas_expr() => cas_binary_expr(CasOp::Add, a, b),
                _ if a.is_complex() || b.is_complex() => {
                    let (za, zb) = complex_operands(a, b)?;
                    Ok(Value::from_complex64(za + zb))
                }
                _ if a.is_algebraic_number() || b.is_algebraic_number() => {
                    algebraic_binary_op("+", a, b)
                }
                _ if a.is_fraction() || b.is_fraction() => {
                    if let Some(((an, ad), (bn, bd))) = rational_operands(a, b) {
                        Ok(Value::from_fraction_parts(an * &bd + bn * &ad, ad * bd))
                    } else {
                        Ok(Value::float(
                            a.as_f64().ok_or_else(|| expected_numeric2(a, b))?
                                + b.as_f64().ok_or_else(|| expected_numeric2(a, b))?,
                        ))
                    }
                }
                _ => Err(expected_numeric2(a, b)),
            }
        }
    }
}

pub(crate) fn intlist_map<F>(items: &[i64], f: F) -> Option<Vec<i64>>
where
    F: Fn(i64) -> Option<i64> + Sync + Send,
{
    if items.len() > PAR_BC_THRESHOLD {
        items.par_iter().map(|&x| f(x)).collect()
    } else {
        items.iter().map(|&x| f(x)).collect()
    }
}

pub(crate) fn intlist_zip_map<F>(a: &[i64], b: &[i64], f: F) -> Option<Vec<i64>>
where
    F: Fn(i64, i64) -> Option<i64> + Sync + Send,
{
    if a.len() != b.len() {
        return None;
    }
    if a.len() > PAR_BC_THRESHOLD {
        a.par_iter()
            .zip(b.par_iter())
            .map(|(&x, &y)| f(x, y))
            .collect()
    } else {
        a.iter().zip(b.iter()).map(|(&x, &y)| f(x, y)).collect()
    }
}

fn affine_int_range(range: &IntRangeData, scale: i64, offset: i64) -> Option<Value> {
    let len = range.len();
    if len == 0 {
        return Some(Value::IntRange(Arc::new(IntRangeData::new(0, 1, 0))));
    }

    let step = range.step().checked_mul(scale)?;
    if step == 0 {
        return None;
    }
    let start = range.start().checked_mul(scale)?.checked_add(offset)?;
    let _last = range
        .last_value()?
        .checked_mul(scale)?
        .checked_add(offset)?;

    Some(Value::IntRange(Arc::new(IntRangeData::new(
        start, step, len,
    ))))
}

fn add_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| x.checked_add(y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            intlist_map(a, |x| x.checked_add(*b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| a.checked_add(y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntRange(a), Value::Int(b)) | (Value::Int(b), Value::IntRange(a)) => {
            affine_int_range(a, 1, *b)
        }
        _ => None,
    }
}

fn sub_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Subtract, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x
            .checked_sub(*y)
            .map(Value::Int)
            .unwrap_or_else(|| Value::from_bigint(BigInt::from(*x) - BigInt::from(*y)))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::float(x - y)),
        _ => {
            if let Some((x, y)) = int_float_ordered(a, b) {
                return Ok(Value::float(x - y));
            }
            match (a, b) {
                (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x - &**y)),
                (Value::Int(x), Value::BigInt(y)) => {
                    Ok(Value::from_bigint(BigInt::from(*x) - &**y))
                }
                (Value::BigInt(x), Value::Int(y)) => {
                    Ok(Value::from_bigint(&**x - BigInt::from(*y)))
                }
                _ => {
                    if let Some((x, y)) = bigint_float_ordered(a, b) {
                        return x
                            .to_f64()
                            .map(|xf| Value::float(xf - y))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if let Some((x, y)) = float_bigint_ordered(a, b) {
                        return y
                            .to_f64()
                            .map(|yf| Value::float(x - yf))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if a.is_cas_expr() || b.is_cas_expr() {
                        cas_binary_expr(CasOp::Subtract, a, b)
                    } else if a.is_complex() || b.is_complex() {
                        let (za, zb) = complex_operands(a, b)?;
                        Ok(Value::from_complex64(za - zb))
                    } else if a.is_algebraic_number() || b.is_algebraic_number() {
                        algebraic_binary_op("-", a, b)
                    } else if a.is_fraction() || b.is_fraction() {
                        if let Some(((an, ad), (bn, bd))) = rational_operands(a, b) {
                            Ok(Value::from_fraction_parts(an * &bd - bn * &ad, ad * bd))
                        } else {
                            Ok(Value::float(
                                a.as_f64().ok_or_else(|| expected_numeric2(a, b))?
                                    - b.as_f64().ok_or_else(|| expected_numeric2(a, b))?,
                            ))
                        }
                    } else {
                        Err(expected_numeric2(a, b))
                    }
                }
            }
        }
    }
}

fn sub_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| x.checked_sub(y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            intlist_map(a, |x| x.checked_sub(*b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| a.checked_sub(y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntRange(a), Value::Int(b)) => affine_int_range(a, 1, b.checked_neg()?),
        (Value::Int(a), Value::IntRange(b)) => affine_int_range(b, -1, *a),
        _ => None,
    }
}

fn mul_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Multiply, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(x
            .checked_mul(*y)
            .map(Value::Int)
            .unwrap_or_else(|| Value::from_bigint(BigInt::from(*x) * BigInt::from(*y)))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::float(x * y)),
        _ => {
            if let Some((x, y)) = int_float_pair(a, b) {
                return Ok(Value::float(x as f64 * y));
            }
            if let Some((x, y)) = int_bigint_pair(a, b) {
                return Ok(Value::from_bigint(BigInt::from(x) * y));
            }
            if let Some((x, y)) = float_bigint_pair(a, b) {
                return y
                    .to_f64()
                    .map(|yf| Value::float(x * yf))
                    .ok_or_else(bigint_too_big_for_float);
            }
            match (a, b) {
                (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x * &**y)),
                _ if a.is_cas_expr() || b.is_cas_expr() => cas_binary_expr(CasOp::Multiply, a, b),
                _ if a.is_complex() || b.is_complex() => {
                    let (za, zb) = complex_operands(a, b)?;
                    Ok(Value::from_complex64(za * zb))
                }
                _ if a.is_algebraic_number() || b.is_algebraic_number() => {
                    algebraic_binary_op("*", a, b)
                }
                _ if a.is_fraction() || b.is_fraction() => {
                    if let Some(((an, ad), (bn, bd))) = rational_operands(a, b) {
                        Ok(Value::from_fraction_parts(an * bn, ad * bd))
                    } else {
                        Ok(Value::float(
                            a.as_f64().ok_or_else(|| expected_numeric2(a, b))?
                                * b.as_f64().ok_or_else(|| expected_numeric2(a, b))?,
                        ))
                    }
                }
                _ => Err(expected_numeric2(a, b)),
            }
        }
    }
}

fn mul_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| x.checked_mul(y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            intlist_map(a, |x| x.checked_mul(*b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| a.checked_mul(y)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntRange(a), Value::Int(b)) | (Value::Int(b), Value::IntRange(a)) => {
            affine_int_range(a, *b, 0)
        }
        _ => None,
    }
}

fn div_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Divide, a, b) {
        return Ok(res);
    }
    if is_zero(b) {
        return Err(zero_div_err(None));
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                Err(zero_div_err(None))
            } else {
                Ok(Value::float(*x as f64 / *y as f64))
            }
        }
        (Value::Float(x), Value::Float(y)) => Ok(Value::float(x / y)),
        _ => {
            if let Some((x, y)) = int_float_ordered(a, b) {
                return Ok(Value::float(x / y));
            }
            match (a, b) {
                (Value::BigInt(x), Value::BigInt(y)) => {
                    let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    Ok(Value::float(xf / yf))
                }
                (Value::BigInt(x), Value::Int(y)) => {
                    let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    Ok(Value::float(xf / *y as f64))
                }
                (Value::Int(x), Value::BigInt(y)) => {
                    let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    Ok(Value::float(*x as f64 / yf))
                }
                _ => {
                    if let Some((x, y)) = bigint_float_ordered(a, b) {
                        return x
                            .to_f64()
                            .map(|xf| Value::float(xf / y))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if let Some((x, y)) = float_bigint_ordered(a, b) {
                        return y
                            .to_f64()
                            .map(|yf| Value::float(x / yf))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if a.is_cas_expr() || b.is_cas_expr() {
                        cas_binary_expr(CasOp::Divide, a, b)
                    } else if a.is_complex() || b.is_complex() {
                        let (za, zb) = complex_operands(a, b)?;
                        if zb == Complex64::new(0.0, 0.0) {
                            Err(zero_div_err(None))
                        } else {
                            Ok(Value::from_complex64(za / zb))
                        }
                    } else if a.is_algebraic_number() || b.is_algebraic_number() {
                        algebraic_binary_op("/", a, b)
                    } else if a.is_fraction() || b.is_fraction() {
                        if let Some(((an, ad), (bn, bd))) = rational_operands(a, b) {
                            if bn.is_zero() {
                                Err(zero_div_err(None))
                            } else {
                                Ok(Value::from_fraction_parts(an * bd, ad * bn))
                            }
                        } else {
                            let left = a.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                            let right = b.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                            if right == 0.0 {
                                Err(zero_div_err(None))
                            } else {
                                Ok(Value::float(left / right))
                            }
                        }
                    } else {
                        Err(expected_numeric2(a, b))
                    }
                }
            }
        }
    }
}

fn div_dot_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::DivideDot, a, b) {
        return Ok(res);
    }
    if is_zero(b) {
        return Err(zero_div_err(None));
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                Err(zero_div_err(None))
            } else if x % y == 0 {
                Ok(Value::Int(x / y))
            } else {
                Ok(Value::from_fraction_parts(
                    BigInt::from(*x),
                    BigInt::from(*y),
                ))
            }
        }
        (Value::Float(x), Value::Float(y)) => Ok(Value::float(x / y)),
        _ => {
            if let Some((x, y)) = int_float_ordered(a, b) {
                return Ok(Value::float(x / y));
            }
            match (a, b) {
                (Value::BigInt(x), Value::BigInt(y)) => {
                    let q = &**x / &**y;
                    let r = &**x % &**y;
                    if r.is_zero() {
                        Ok(Value::from_bigint(q))
                    } else {
                        Ok(Value::from_fraction_parts((**x).clone(), (**y).clone()))
                    }
                }
                (Value::BigInt(x), Value::Int(y)) => {
                    if *y == 0 {
                        Err(zero_div_err(None))
                    } else {
                        let yb = BigInt::from(*y);
                        let q = &**x / &yb;
                        let r = &**x % &yb;
                        if r.is_zero() {
                            Ok(Value::from_bigint(q))
                        } else {
                            Ok(Value::from_fraction_parts((**x).clone(), yb))
                        }
                    }
                }
                (Value::Int(x), Value::BigInt(y)) => {
                    let xb = BigInt::from(*x);
                    let q = &xb / &**y;
                    let r = &xb % &**y;
                    if r.is_zero() {
                        Ok(Value::from_bigint(q))
                    } else {
                        Ok(Value::from_fraction_parts(xb, (**y).clone()))
                    }
                }
                _ => {
                    if let Some((x, y)) = bigint_float_ordered(a, b) {
                        return x
                            .to_f64()
                            .map(|xf| Value::float(xf / y))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if let Some((x, y)) = float_bigint_ordered(a, b) {
                        return y
                            .to_f64()
                            .map(|yf| Value::float(x / yf))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if a.is_cas_expr() || b.is_cas_expr() {
                        cas_binary_expr(CasOp::Divide, a, b)
                    } else if a.is_complex() || b.is_complex() {
                        let (za, zb) = complex_operands(a, b)?;
                        if zb == Complex64::new(0.0, 0.0) {
                            Err(zero_div_err(None))
                        } else {
                            Ok(Value::from_complex64(za / zb))
                        }
                    } else if a.is_algebraic_number() || b.is_algebraic_number() {
                        algebraic_binary_op("/", a, b)
                    } else if a.is_fraction() || b.is_fraction() {
                        if let Some(((an, ad), (bn, bd))) = rational_operands(a, b)
                            && !bn.is_zero()
                        {
                            let numer = an * bd;
                            let denom = ad * bn;
                            if denom.is_zero() {
                                Err(zero_div_err(None))
                            } else {
                                let q = &numer / &denom;
                                let r = &numer % &denom;
                                if r.is_zero() {
                                    Ok(Value::from_bigint(q))
                                } else {
                                    Ok(Value::from_fraction_parts(numer, denom))
                                }
                            }
                        } else {
                            let left = a.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                            let right = b.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                            if right == 0.0 {
                                Err(zero_div_err(None))
                            } else {
                                Ok(Value::float(left / right))
                            }
                        }
                    } else {
                        Err(expected_numeric2(a, b))
                    }
                }
            }
        }
    }
}

fn mod_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Modulo, a, b) {
        return Ok(res);
    }
    if is_zero(b) {
        return Err(zero_div_err(None));
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                Err(zero_div_err(None))
            } else {
                Ok(Value::Int(x % y))
            }
        }
        (Value::Float(x), Value::Float(y)) => Ok(Value::float(x % y)),
        _ => {
            if let Some((x, y)) = int_float_ordered(a, b) {
                return Ok(Value::float(x % y));
            }
            match (a, b) {
                (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(&**x % &**y)),
                (Value::BigInt(x), Value::Int(y)) => {
                    Ok(Value::from_bigint(&**x % BigInt::from(*y)))
                }
                (Value::Int(x), Value::BigInt(y)) => {
                    Ok(Value::from_bigint(BigInt::from(*x) % &**y))
                }
                _ => {
                    if let Some((x, y)) = bigint_float_ordered(a, b) {
                        return x
                            .to_f64()
                            .map(|xf| Value::float(xf % y))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    if let Some((x, y)) = float_bigint_ordered(a, b) {
                        return y
                            .to_f64()
                            .map(|yf| Value::float(x % yf))
                            .ok_or_else(bigint_too_big_for_float);
                    }
                    Err(expected_numeric2(a, b))
                }
            }
        }
    }
}

fn mod_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            intlist_zip_map(a, b, |x, y| if y == 0 { None } else { Some(x % y) })
                .map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::IntList(a), Value::Int(b)) => {
            if *b == 0 {
                return None;
            }
            intlist_map(a, |x| Some(x % *b)).map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => {
            intlist_map(b, |y| if y == 0 { None } else { Some(*a % y) })
                .map(|v| Value::IntList(Arc::new(v)))
        }
        _ => None,
    }
}

fn neg_intlist(v: &Value) -> Option<Value> {
    match v {
        Value::IntList(a) => {
            intlist_map(a, |x| x.checked_neg()).map(|v| Value::IntList(Arc::new(v)))
        }
        Value::IntRange(a) => affine_int_range(a, -1, 0),
        _ => None,
    }
}

fn floor_div_intlist(left: &Value, right: &Value) -> Option<Value> {
    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => intlist_zip_map(a, b, |x, y| {
            if y == 0 {
                return None;
            }
            if x == i64::MIN && y == -1 {
                return None;
            }
            let q0 = x / y;
            let r = x % y;
            Some(if r == 0 || (x ^ y) >= 0 { q0 } else { q0 - 1 })
        })
        .map(|v| Value::IntList(Arc::new(v))),
        (Value::IntList(a), Value::Int(b)) => {
            if *b == 0 {
                return None;
            }
            intlist_map(a, |x| {
                if x == i64::MIN && *b == -1 {
                    return None;
                }
                let q0 = x / *b;
                let r = x % *b;
                Some(if r == 0 || (x ^ *b) >= 0 { q0 } else { q0 - 1 })
            })
            .map(|v| Value::IntList(Arc::new(v)))
        }
        (Value::Int(a), Value::IntList(b)) => intlist_map(b, |y| {
            if y == 0 {
                return None;
            }
            if *a == i64::MIN && y == -1 {
                return None;
            }
            let q0 = *a / y;
            let r = *a % y;
            Some(if r == 0 || (*a ^ y) >= 0 { q0 } else { q0 - 1 })
        })
        .map(|v| Value::IntList(Arc::new(v))),
        _ => None,
    }
}

fn floor_div_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::FloorDiv, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => {
            if *y == 0 {
                return Err(zero_div_err(None));
            }
            if *x == i64::MIN && *y == -1 {
                let qa = num_bigint::BigInt::from(*x);
                let qb = num_bigint::BigInt::from(*y);
                let q0 = &qa / &qb;
                return Ok(Value::from_bigint(q0));
            }
            let q0 = x / y;
            let r = x % y;
            if r == 0 || (x ^ y) >= 0 {
                Ok(Value::Int(q0))
            } else {
                Ok(Value::Int(q0 - 1))
            }
        }
        _ => {
            let div = div_atoms(a, b)?;
            div.floor()
        }
    }
}

fn pow_dot_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::PowerDot, a, b) {
        return Ok(res);
    }
    match (a, b) {
        // Exact versions of integer negative exponents: produce fractions
        (Value::Int(x), Value::Int(y)) if *y < 0 => {
            if *x == 0 {
                Err(zero_to_negative_power_err())
            } else {
                let abs_y = y.unsigned_abs();
                if let Ok(uy) = u32::try_from(abs_y) {
                    if let Some(pow_val) = x.checked_pow(uy) {
                        Ok(Value::from_fraction_parts(
                            BigInt::one(),
                            BigInt::from(pow_val),
                        ))
                    } else {
                        let big_x = BigInt::from(*x);
                        let big_pow = big_x.pow(uy);
                        Ok(Value::from_fraction_parts(BigInt::one(), big_pow))
                    }
                } else {
                    Err(exponent_too_large_err())
                }
            }
        }
        (Value::BigInt(base), Value::Int(exp)) => {
            if *exp < 0 {
                if base.is_zero() {
                    Err(zero_to_negative_power_err())
                } else {
                    let exp_u32 =
                        u32::try_from(exp.unsigned_abs()).map_err(|_| exponent_too_large_err())?;
                    let big_pow = base.clone().pow(exp_u32);
                    Ok(Value::from_fraction_parts(BigInt::one(), big_pow))
                }
            } else {
                let exp_u32 = u32::try_from(*exp).map_err(|_| exponent_too_large_err())?;
                Ok(Value::from_bigint(base.clone().pow(exp_u32)))
            }
        }
        (Value::BigInt(base), Value::BigInt(exp)) => {
            if exp.is_negative() {
                if base.is_zero() {
                    Err(zero_to_negative_power_err())
                } else {
                    let exp_u32 = exp.abs().to_u32().ok_or_else(exponent_too_large_err)?;
                    let big_pow = base.clone().pow(exp_u32);
                    Ok(Value::from_fraction_parts(BigInt::one(), big_pow))
                }
            } else {
                let exp_u32 = exp.to_u32().ok_or_else(exponent_too_large_err)?;
                Ok(Value::from_bigint(base.clone().pow(exp_u32)))
            }
        }
        (Value::Int(base), Value::BigInt(exp)) => {
            if exp.is_negative() {
                if *base == 0 {
                    Err(zero_to_negative_power_err())
                } else {
                    let exp_u32 = exp.abs().to_u32().ok_or_else(exponent_too_large_err)?;
                    let big_pow = BigInt::from(*base).pow(exp_u32);
                    Ok(Value::from_fraction_parts(BigInt::one(), big_pow))
                }
            } else {
                let exp_u32 = exp.to_u32().ok_or_else(exponent_too_large_err)?;
                Ok(Value::from_bigint(BigInt::from(*base).pow(exp_u32)))
            }
        }
        _ if a.is_fraction() || b.is_fraction() => {
            if let (Some((base_n, base_d)), Some((exp_n, exp_d))) =
                (a.rational_parts(), b.rational_parts())
                && !exp_d.is_one()
            {
                if base_n.is_zero() && exp_n.is_negative() {
                    return Err(zero_to_negative_power_err());
                }
                if let Some(exact) = exact_rational_fractional_pow(&base_n, &base_d, &exp_n, &exp_d)
                {
                    return Ok(exact);
                }
            }
            pow_atoms(a, b)
        }
        // Everything else delegates to pow_atoms
        _ => pow_atoms(a, b),
    }
}

fn pow_atoms(a: &Value, b: &Value) -> WqResult<Value> {
    if let Some(res) = Value::lift_callable_binary(BinaryOperator::Power, a, b) {
        return Ok(res);
    }
    match (a, b) {
        (Value::Int(x), Value::Int(y)) if *y >= 0 => {
            let uy = u32::try_from(*y).map_err(|_| exponent_too_large_err())?;
            Ok(x.checked_pow(uy)
                .map(Value::Int)
                .unwrap_or_else(|| Value::from_bigint(BigInt::from(*x).pow(uy))))
        }
        (Value::Int(0), Value::Int(y)) if *y < 0 => Err(zero_to_negative_power_err()),
        (Value::Int(x), Value::Int(y)) if *y < 0 => Ok(Value::float((*x as f64).powf(*y as f64))),
        (Value::Float(f), Value::Float(y)) if **f == 0.0 && **y < 0.0 => {
            Err(zero_to_negative_power_err())
        }
        (Value::Float(x), Value::Float(y)) => {
            let r = x.powf(**y);
            if r.is_nan() && **x < 0.0 && y.is_finite() {
                Ok(Value::from_complex64(
                    Complex64::new(**x, 0.0).powc(Complex64::new(**y, 0.0)),
                ))
            } else {
                Ok(Value::float(r))
            }
        }
        (Value::Int(0), Value::Float(y)) if **y < 0.0 => Err(zero_to_negative_power_err()),
        (Value::Int(x), Value::Float(y)) => {
            let xb = *x as f64;
            let r = xb.powf(**y);
            if r.is_nan() && xb < 0.0 && y.is_finite() {
                Ok(Value::from_complex64(
                    Complex64::new(xb, 0.0).powc(Complex64::new(**y, 0.0)),
                ))
            } else {
                Ok(Value::float(r))
            }
        }
        (Value::Float(f), Value::Int(y)) if **f == 0.0 && *y < 0 => {
            Err(zero_to_negative_power_err())
        }
        (Value::Float(x), Value::Int(y)) => Ok(Value::float(x.powf(*y as f64))),

        (Value::BigInt(base), Value::BigInt(exp)) => {
            if exp.is_negative() {
                if base.is_zero() {
                    Err(zero_to_negative_power_err())
                } else {
                    let xb = base.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    let yb = exp.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    Ok(Value::float(xb.powf(yb)))
                }
            } else {
                let exp_u32 = exp.to_u32().ok_or_else(exponent_too_large_err)?;
                Ok(Value::from_bigint(base.clone().pow(exp_u32)))
            }
        }
        (Value::BigInt(base), Value::Int(exp)) => {
            if *exp < 0 {
                if base.is_zero() {
                    Err(zero_to_negative_power_err())
                } else {
                    let xb = base.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    Ok(Value::float(xb.powf(*exp as f64)))
                }
            } else {
                let exp_u32 = u32::try_from(*exp).map_err(|_| exponent_too_large_err())?;
                Ok(Value::from_bigint(base.clone().pow(exp_u32)))
            }
        }
        (Value::Int(base), Value::BigInt(exp)) => {
            if exp.is_negative() {
                if *base == 0 {
                    Err(zero_to_negative_power_err())
                } else {
                    let yb = exp.to_f64().ok_or_else(bigint_too_big_for_float)?;
                    Ok(Value::float((*base as f64).powf(yb)))
                }
            } else {
                let exp_u32 = exp.to_u32().ok_or_else(exponent_too_large_err)?;
                Ok(Value::from_bigint(BigInt::from(*base).pow(exp_u32)))
            }
        }
        (Value::BigInt(base), Value::Float(exp)) if base.is_zero() && **exp < 0.0 => {
            Err(zero_to_negative_power_err())
        }
        (Value::BigInt(base), Value::Float(exp)) => {
            let xb = base.to_f64().ok_or_else(bigint_too_big_for_float)?;
            let r = xb.powf(**exp);
            if r.is_nan() && base.is_negative() && exp.is_finite() {
                Ok(Value::from_complex64(
                    Complex64::new(xb, 0.0).powc(Complex64::new(**exp, 0.0)),
                ))
            } else {
                Ok(Value::float(r))
            }
        }
        (Value::Float(f), Value::BigInt(exp)) if **f == 0.0 && exp.is_negative() => {
            Err(zero_to_negative_power_err())
        }
        (Value::Float(x), Value::BigInt(exp)) => {
            let yb = exp.to_f64().ok_or_else(bigint_too_big_for_float)?;
            Ok(Value::float(x.powf(yb)))
        }

        _ if a.is_cas_expr() || b.is_cas_expr() => cas_binary_expr(CasOp::Power, a, b),
        _ if a.is_complex() || b.is_complex() => {
            let (base, exp) = complex_operands(a, b)?;
            if base == Complex64::new(0.0, 0.0) && exp.im == 0.0 && exp.re < 0.0 {
                Err(zero_to_negative_power_err())
            } else {
                Ok(Value::from_complex64(base.powc(exp)))
            }
        }
        _ if a.is_algebraic_number() => {
            if let Value::Algebraic(aa) = a {
                if let Some(n) = b.as_i64() {
                    return crate::value::algebraic::algebraic_pow(aa, n);
                }
                if let Some((num, den)) = b.rational_parts()
                    && let Ok(result) =
                        crate::value::algebraic::algebraic_rational_pow(aa, &num, &den)
                {
                    return Ok(result);
                }
            }
            cas_binary_expr(CasOp::Power, a, b)
        }
        _ if a.is_fraction() || b.is_fraction() => {
            if let (Some((base_n, base_d)), Some(exp)) = (a.rational_parts(), rational_exponent(b))
            {
                if exp.is_negative() && base_n.is_zero() {
                    Err(zero_to_negative_power_err())
                } else {
                    let exp_u32 = exp.abs().to_u32().ok_or_else(exponent_too_large_err)?;
                    let (numer, denom) = if exp.is_negative() {
                        (base_d.pow(exp_u32), base_n.pow(exp_u32))
                    } else {
                        (base_n.pow(exp_u32), base_d.pow(exp_u32))
                    };
                    Ok(Value::from_fraction_parts(numer, denom))
                }
            } else if let (Some((base_n, base_d)), Some((_exp_n, exp_d))) =
                (a.rational_parts(), b.rational_parts())
            {
                if !exp_d.is_one()
                    && let Some(exp_d_u32) = exp_d.to_u32()
                {
                    let is_exact = exact_nth_root_bigint(&base_n, exp_d_u32).is_some()
                        && exact_nth_root_bigint(&base_d, exp_d_u32).is_some();
                    if !is_exact {
                        return cas_binary_expr(CasOp::Power, a, b);
                    }
                }
                let base = a.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                let exp = b.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                if base == 0.0 && exp < 0.0 {
                    Err(zero_to_negative_power_err())
                } else {
                    let r = base.powf(exp);
                    if r.is_nan() && base < 0.0 && exp.is_finite() {
                        Ok(Value::from_complex64(
                            Complex64::new(base, 0.0).powc(Complex64::new(exp, 0.0)),
                        ))
                    } else {
                        Ok(Value::float(r))
                    }
                }
            } else {
                let base = a.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                let exp = b.as_f64().ok_or_else(|| expected_numeric2(a, b))?;
                if base == 0.0 && exp < 0.0 {
                    Err(zero_to_negative_power_err())
                } else {
                    let r = base.powf(exp);
                    if r.is_nan() && base < 0.0 && exp.is_finite() {
                        Ok(Value::from_complex64(
                            Complex64::new(base, 0.0).powc(Complex64::new(exp, 0.0)),
                        ))
                    } else {
                        Ok(Value::float(r))
                    }
                }
            }
        }
        _ => Err(expected_numeric2(a, b)),
    }
}

impl Value {
    pub(crate) fn neg(&self) -> WqResult<Value> {
        if let Some(res) = neg_intlist(self) {
            return Ok(res);
        }
        if self.is_atom() {
            return neg_atom(self);
        }
        self.bc1(neg_atom).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn add(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = add_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return add_atoms(self, other);
        }
        self.bc2(other, add_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn subtract(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = sub_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return sub_atoms(self, other);
        }
        self.bc2(other, sub_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn multiply(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = mul_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return mul_atoms(self, other);
        }
        self.bc2(other, mul_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn divide(&self, other: &Value) -> WqResult<Value> {
        if self.is_atom() && other.is_atom() {
            return div_atoms(self, other);
        }
        self.bc2(other, div_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn divide_dot(&self, other: &Value) -> WqResult<Value> {
        if self.is_atom() && other.is_atom() {
            return div_dot_atoms(self, other);
        }
        self.bc2(other, div_dot_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn modulo(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = mod_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return mod_atoms(self, other);
        }
        self.bc2(other, mod_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn power(&self, other: &Value) -> WqResult<Value> {
        if self.is_atom() && other.is_atom() {
            return pow_atoms(self, other);
        }
        self.bc2(other, pow_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn power_dot(&self, other: &Value) -> WqResult<Value> {
        if self.is_atom() && other.is_atom() {
            return pow_dot_atoms(self, other);
        }
        self.bc2(other, pow_dot_atoms).map_err(|e| e.into_wqerror())
    }

    pub(crate) fn floor_div(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = floor_div_intlist(self, other) {
            return Ok(res);
        }
        if self.is_atom() && other.is_atom() {
            return floor_div_atoms(self, other);
        }
        self.bc2(other, floor_div_atoms)
            .map_err(|e| e.into_wqerror())
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;

    #[test]
    fn add_overflow_promotes_to_bigint() {
        let a = Value::Int(i64::MAX);
        let b = Value::Int(1);
        let result = a.add(&b).unwrap();
        match result {
            Value::BigInt(ref n) => {
                let expected = BigInt::from(i64::MAX) + BigInt::from(1);
                assert_eq!(&**n, &expected);
            }
            other => panic!("expected bigint result, got {other:?}"),
        }
    }

    #[test]
    fn affine_int_range_arithmetic_preserves_virtual_storage() {
        let range = Value::IntRange(Arc::new(IntRangeData::new(0, 1, 4)));
        let doubled = Value::Int(2).multiply(&range).expect("multiply succeeds");
        assert!(matches!(
            &doubled,
            Value::IntRange(range) if range.start() == 0 && range.step() == 2 && range.len() == 4
        ));
        assert_eq!(doubled, Value::IntList(Arc::new(vec![0, 2, 4, 6])));

        let shifted = Value::Int(1).add(&doubled).expect("add succeeds");
        assert!(matches!(
            &shifted,
            Value::IntRange(range) if range.start() == 1 && range.step() == 2 && range.len() == 4
        ));
        assert_eq!(shifted, Value::IntList(Arc::new(vec![1, 3, 5, 7])));

        let negated = shifted.neg().expect("negate succeeds");
        assert!(matches!(
            &negated,
            Value::IntRange(range) if range.start() == -1 && range.step() == -2 && range.len() == 4
        ));
        assert_eq!(negated, Value::IntList(Arc::new(vec![-1, -3, -5, -7])));
    }

    #[test]
    fn affine_int_range_overflow_falls_back_to_value_broadcast() {
        let range = Value::IntRange(Arc::new(IntRangeData::new(i64::MAX - 1, 1, 2)));
        let shifted = range.add(&Value::Int(1)).expect("add succeeds");
        assert_eq!(
            shifted,
            Value::List(Arc::new(vec![
                Value::Int(i64::MAX),
                Value::from_bigint(BigInt::from(i64::MAX) + BigInt::from(1))
            ]))
        );
    }

    #[test]
    fn fraction_like_add_preserves_fraction_output() {
        let a = Value::from_fraction_parts(BigInt::from(1), BigInt::from(2));
        let b = Value::from_fraction_parts(BigInt::from(1), BigInt::from(4));
        assert_eq!(
            a.add(&b).unwrap(),
            Value::from_fraction_parts(BigInt::from(3), BigInt::from(4))
        );
    }

    #[test]
    fn fraction_like_divide_by_int_stays_fractional() {
        let a = Value::from_fraction_parts(BigInt::from(3), BigInt::from(2));
        assert_eq!(
            a.divide(&Value::Int(3)).unwrap(),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2))
        );
    }

    #[test]
    fn classic_int_division_returns_float() {
        assert_eq!(
            Value::Int(1).divide(&Value::Int(3)).unwrap(),
            Value::float(1.0 / 3.0)
        );
    }

    #[test]
    fn exact_int_division_returns_fraction() {
        assert_eq!(
            Value::Int(1).divide_dot(&Value::Int(3)).unwrap(),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(3))
        );
    }

    #[test]
    fn classic_negative_integer_power_returns_float() {
        assert_eq!(
            Value::Int(2).power(&Value::Int(-3)).unwrap(),
            Value::float(0.125)
        );
    }

    #[test]
    fn exact_negative_integer_power_returns_fraction() {
        assert_eq!(
            Value::Int(2).power_dot(&Value::Int(-3)).unwrap(),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(8))
        );
    }

    #[test]
    fn perfect_rational_fractional_power_stays_exact() {
        let base = Value::from_fraction_parts(BigInt::from(8), BigInt::from(27));
        let exp = Value::from_fraction_parts(BigInt::from(1), BigInt::from(3));

        assert_eq!(
            base.power_dot(&exp).unwrap(),
            Value::from_fraction_parts(BigInt::from(2), BigInt::from(3))
        );
    }

    #[test]
    fn classic_rational_fractional_power_stays_float() {
        let base = Value::from_fraction_parts(BigInt::from(8), BigInt::from(27));
        let exp = Value::from_fraction_parts(BigInt::from(1), BigInt::from(3));

        let result = base.power(&exp).unwrap();
        assert!(matches!(result, Value::Float(_)));
        let f = result.as_f64().expect("classic power returns a float");
        assert!((f - 2.0 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn rational_fractional_power_with_integer_numerator_stays_exact() {
        let base = Value::from_fraction_parts(BigInt::from(27), BigInt::from(8));
        let exp = Value::from_fraction_parts(BigInt::from(2), BigInt::from(3));

        assert_eq!(
            base.power_dot(&exp).unwrap(),
            Value::from_fraction_parts(BigInt::from(9), BigInt::from(4))
        );
    }

    #[test]
    fn negative_rational_fractional_power_stays_exact() {
        let base = Value::from_fraction_parts(BigInt::from(8), BigInt::from(27));
        let exp = Value::from_fraction_parts(BigInt::from(-1), BigInt::from(3));

        assert_eq!(
            base.power_dot(&exp).unwrap(),
            Value::from_fraction_parts(BigInt::from(3), BigInt::from(2))
        );
    }

    #[test]
    fn odd_root_of_negative_rational_stays_exact() {
        let base = Value::from_fraction_parts(BigInt::from(-8), BigInt::from(27));
        let exp = Value::from_fraction_parts(BigInt::from(1), BigInt::from(3));

        assert_eq!(
            base.power_dot(&exp).unwrap(),
            Value::from_fraction_parts(BigInt::from(-2), BigInt::from(3))
        );
    }
}
