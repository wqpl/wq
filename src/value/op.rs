use crate::{
    value::{
        Value, WqResult,
        bc::BcResult,
        wqerror_helper::{
            expected_bool1, expected_bool2, expected_integer1, expected_integer2,
            expected_numeric1, expected_numeric2,
        },
    },
    wqerror::{WqError, WqErrorType},
};

use indexmap::indexmap;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

fn zero_div_err(msg: Option<&'static str>) -> WqError {
    let msg = msg.unwrap_or("cannot divide by 0").to_string();
    WqError::new(WqErrorType::ZeroDiv).msg(msg)
}

fn bigint_too_big_for_float() -> WqError {
    WqError::new(WqErrorType::NumericOverflow).msg("provided bigint cannot be converted to float")
}

fn invalid_shift(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("shift must be in 0..4_294_967_295")
        .got1(v)
}

fn invalid_unicode(v: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("invalid Unicode scalar value")
        .attach_note("valid Unicode code points are 0x0000..=0xD7FF, 0xE000..=0x10FFFF")
        .got1(v)
}

impl Value {
    pub fn neg(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::BigInt(n) => Ok(Value::BigInt(-n.clone())),
            Value::Int(n) => Ok(match n.checked_neg() {
                Some(v) => Value::Int(v),
                None => Value::BigInt(-BigInt::from(*n)),
            }),
            Value::Float(f) => Ok(Value::Float(-f)),
            _ => Err(expected_numeric1(v)),
        })
    }

    pub fn add(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x + y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x + BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) + y)),
            (Value::Int(x), Value::Int(y)) => Ok(match x.checked_add(*y) {
                Some(sum) => Value::Int(sum),
                None => Value::from_bigint(BigInt::from(*x) + BigInt::from(*y)),
            }),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x + y)),
            (Value::BigInt(x), Value::Float(y)) => x
                .to_f64()
                .map(|xf| Value::Float(xf + *y))
                .ok_or_else(|| expected_numeric2(a, b)),
            (Value::Float(x), Value::BigInt(y)) => y
                .to_f64()
                .map(|yf| Value::Float(*x + yf))
                .ok_or_else(bigint_too_big_for_float),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 + *y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(*x + *y as f64)),
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn subtract(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x - y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x - BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) - y)),
            (Value::Int(x), Value::Int(y)) => Ok(match x.checked_sub(*y) {
                Some(diff) => Value::Int(diff),
                None => Value::from_bigint(BigInt::from(*x) - BigInt::from(*y)),
            }),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x - y)),
            (Value::BigInt(x), Value::Float(y)) => x
                .to_f64()
                .map(|xf| Value::Float(xf - *y))
                .ok_or_else(bigint_too_big_for_float),
            (Value::Float(x), Value::BigInt(y)) => y
                .to_f64()
                .map(|yf| Value::Float(*x - yf))
                .ok_or_else(bigint_too_big_for_float),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 - *y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(*x - *y as f64)),
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn multiply(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x * y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x * BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) * y)),
            (Value::Int(x), Value::Int(y)) => Ok(match x.checked_mul(*y) {
                Some(prod) => Value::Int(prod),
                None => Value::from_bigint(BigInt::from(*x) * BigInt::from(*y)),
            }),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x * y)),
            (Value::BigInt(x), Value::Float(y)) => x
                .to_f64()
                .map(|xf| Value::Float(xf * *y))
                .ok_or_else(bigint_too_big_for_float),
            (Value::Float(x), Value::BigInt(y)) => y
                .to_f64()
                .map(|yf| Value::Float(*x * yf))
                .ok_or_else(bigint_too_big_for_float),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 * *y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(*x * *y as f64)),
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn divide(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(_), Value::Int(0))
            | (Value::Float(_), Value::Int(0))
            | (Value::Float(_), Value::Float(0.0)) => Err(zero_div_err(None)),
            (Value::BigInt(_), Value::Int(0)) => Err(zero_div_err(None)),
            (Value::BigInt(_), Value::BigInt(y)) if y.is_zero() => Err(zero_div_err(None)),
            (Value::Int(_), Value::BigInt(y)) if y.is_zero() => Err(zero_div_err(None)),
            (Value::Int(x), Value::Int(y)) => Ok(Value::Float(*x as f64 / *y as f64)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 / y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x / *y as f64)),
            (Value::BigInt(x), Value::BigInt(y)) => {
                let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(xf / yf))
            }
            (Value::BigInt(x), Value::Int(y)) => {
                let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(xf / *y as f64))
            }
            (Value::Int(x), Value::BigInt(y)) => {
                let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(*x as f64 / yf))
            }
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn divide_dot(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            // (Value::Int(_), Value::Int(0)) => None,
            // allow y==0.0`
            // (Value::Float(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,
            (Value::Int(x), Value::Int(y)) => Ok(Value::Float(*x as f64 / *y as f64)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x / y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 / y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x / *y as f64)),
            (Value::BigInt(x), Value::BigInt(y)) => {
                let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(xf / yf))
            }
            (Value::BigInt(x), Value::Int(y)) => {
                let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(xf / *y as f64))
            }
            (Value::Int(x), Value::BigInt(y)) => {
                let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(*x as f64 / yf))
            }
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn modulo(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(_), Value::Int(0))
            | (Value::Float(_), Value::Int(0))
            | (Value::Float(_), Value::Float(0.0)) => Err(zero_div_err(None)),
            (Value::BigInt(_), Value::Int(0)) => Err(zero_div_err(None)),
            (Value::BigInt(_), Value::BigInt(y)) if y.is_zero() => Err(zero_div_err(None)),
            (Value::Int(_), Value::BigInt(y)) if y.is_zero() => Err(zero_div_err(None)),
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x % y)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x % y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 % y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x % *y as f64)),
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x % y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x % BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) % y)),
            (Value::BigInt(x), Value::Float(y)) => x
                .to_f64()
                .map(|xf| Value::Float(xf % *y))
                .ok_or_else(bigint_too_big_for_float),
            (Value::Float(x), Value::BigInt(y)) => y
                .to_f64()
                .map(|yf| Value::Float(*x % yf))
                .ok_or_else(bigint_too_big_for_float),
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn modulo_dot(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            // (Value::Int(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,
            (Value::Int(x), Value::Int(y)) => Ok(Value::Float(*x as f64 % *y as f64)),
            (Value::Float(x), Value::Float(y)) => Ok(Value::Float(x % y)),
            (Value::Int(x), Value::Float(y)) => Ok(Value::Float(*x as f64 % y)),
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x % *y as f64)),
            (Value::BigInt(x), Value::BigInt(y)) => {
                let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(xf % yf))
            }
            (Value::BigInt(x), Value::Int(y)) => {
                let xf = x.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(xf % *y as f64))
            }
            (Value::Int(x), Value::BigInt(y)) => {
                let yf = y.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(*x as f64 % yf))
            }
            _ => Err(expected_numeric2(a, b)),
        })
    }

    pub fn power(&self, other: &Value) -> BcResult<Value> {
        use std::f64::consts::PI;

        fn zero_neg_pow_err() -> WqResult<Value> {
            Err(zero_div_err(Some("0 cannot be raised to a negative power")))
        }

        fn exponent_too_large_err() -> WqError {
            WqError::new(WqErrorType::Domain).msg("exponent cannot exceed 4_294_967_295")
        }

        // implicit 0^0=1
        self.bc2(other, |a, b| match (a, b) {
            (Value::BigInt(base), Value::BigInt(exp)) => {
                if exp.is_negative() {
                    if base.is_zero() {
                        zero_neg_pow_err()
                    } else {
                        let xb = base.to_f64().ok_or_else(bigint_too_big_for_float)?;
                        let yb = exp.to_f64().ok_or_else(bigint_too_big_for_float)?;
                        Ok(Value::Float(xb.powf(yb)))
                    }
                } else {
                    let exp_u32 = exp.to_u32().ok_or_else(exponent_too_large_err)?;
                    Ok(Value::from_bigint(base.clone().pow(exp_u32)))
                }
            }
            (Value::BigInt(base), Value::Int(exp)) => {
                if *exp < 0 {
                    if base.is_zero() {
                        zero_neg_pow_err()
                    } else {
                        let xb = base.to_f64().ok_or_else(bigint_too_big_for_float)?;
                        Ok(Value::Float(xb.powf(*exp as f64)))
                    }
                } else {
                    let exp_u32 = u32::try_from(*exp).map_err(|_| exponent_too_large_err())?;
                    Ok(Value::from_bigint(base.clone().pow(exp_u32)))
                }
            }
            (Value::Int(base), Value::BigInt(exp)) => {
                if exp.is_negative() {
                    if *base == 0 {
                        zero_neg_pow_err()
                    } else {
                        let yb = exp.to_f64().ok_or_else(bigint_too_big_for_float)?;
                        Ok(Value::Float((*base as f64).powf(yb)))
                    }
                } else {
                    let exp_u32 = exp.to_u32().ok_or_else(exponent_too_large_err)?;
                    Ok(Value::from_bigint(BigInt::from(*base).pow(exp_u32)))
                }
            }
            (Value::BigInt(base), Value::Float(exp)) if base.is_zero() && *exp < 0.0 => {
                zero_neg_pow_err()
            }
            (Value::BigInt(base), Value::Float(exp)) => {
                let xb = base.to_f64().ok_or_else(bigint_too_big_for_float)?;
                let r = xb.powf(*exp);
                if r.is_nan() && base.is_negative() && exp.is_finite() {
                    let a_mag = (-xb).powf(*exp);
                    let theta = PI * *exp;
                    let re = a_mag * theta.cos();
                    let im = a_mag * theta.sin();
                    Ok(Value::Dict(indexmap! {
                        "re".into() => Value::Float(re),
                        "im".into() => Value::Float(im)
                    }))
                } else {
                    Ok(Value::Float(r))
                }
            }
            (Value::Float(0.0), Value::BigInt(exp)) if exp.is_negative() => zero_neg_pow_err(),
            (Value::Float(x), Value::BigInt(exp)) => {
                let yb = exp.to_f64().ok_or_else(bigint_too_big_for_float)?;
                Ok(Value::Float(x.powf(yb)))
            }
            (Value::Int(x), Value::Int(y)) if *y >= 0 => {
                let uy = u32::try_from(*y).map_err(|_| exponent_too_large_err())?;
                Ok(match x.checked_pow(uy) {
                    Some(v) => Value::Int(v),
                    None => Value::from_bigint(BigInt::from(*x).pow(uy)),
                })
            }
            (Value::Int(0), Value::Int(y)) if *y < 0 => zero_neg_pow_err(),
            (Value::Int(x), Value::Int(y)) if *y < 0 => {
                Ok(Value::Float((*x as f64).powf(*y as f64)))
            }
            (Value::Float(0.0), Value::Float(y)) if *y < 0.0 => zero_neg_pow_err(),
            // Float^Float: if powf() is NaN due to a negative base and non-integer exponent,
            // return principal complex result
            (Value::Float(x), Value::Float(y)) => {
                let r = x.powf(*y);
                if r.is_nan() && *x < 0.0 && y.is_finite() {
                    let a = -*x;
                    let mag = a.powf(*y);
                    let theta = PI * *y;
                    let re = mag * theta.cos();
                    let im = mag * theta.sin();
                    Ok(Value::Dict(indexmap! {
                        "re".into() => Value::Float(re),
                        "im".into() => Value::Float(im)
                    }))
                } else {
                    Ok(Value::Float(r))
                }
            }
            (Value::Int(0), Value::Float(y)) if *y < 0.0 => zero_neg_pow_err(),
            // Int^Float: same NaN->complex handling as above
            (Value::Int(x), Value::Float(y)) => {
                let xb = *x as f64;
                let r = xb.powf(*y);
                if r.is_nan() && xb < 0.0 && y.is_finite() {
                    let a = -xb;
                    let mag = a.powf(*y);
                    let theta = PI * *y;
                    let re = mag * theta.cos();
                    let im = mag * theta.sin();
                    Ok(Value::Dict(indexmap! {
                        "re".into() => Value::Float(re),
                        "im".into() => Value::Float(im)
                    }))
                } else {
                    Ok(Value::Float(r))
                }
            }
            (Value::Float(0.0), Value::Int(y)) if *y < 0 => zero_neg_pow_err(),
            // Float^Int stays real; powf won't NaN here.
            (Value::Float(x), Value::Int(y)) => Ok(Value::Float(x.powf(*y as f64))),
            _ => Err(expected_numeric2(a, b)),
        })
    }

    // bitwise ops ===================================================================================================

    pub fn band(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x & y)),
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x & y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x & BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) & y)),
            _ => Err(expected_integer2(a, b)),
        })
    }

    pub fn bor(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x | y)),
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x | y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x | BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) | y)),
            _ => Err(expected_integer2(a, b)),
        })
    }

    pub fn bxor(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Ok(Value::Int(x ^ y)),
            (Value::BigInt(x), Value::BigInt(y)) => Ok(Value::from_bigint(x ^ y)),
            (Value::BigInt(x), Value::Int(y)) => Ok(Value::from_bigint(x ^ BigInt::from(*y))),
            (Value::Int(x), Value::BigInt(y)) => Ok(Value::from_bigint(BigInt::from(*x) ^ y)),
            _ => Err(expected_integer2(a, b)),
        })
    }

    pub fn bnot(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(x) => Ok(Value::Int(!x)),
            Value::BigInt(x) => Ok(Value::from_bigint(!x)),
            _ => Err(expected_integer1(v)),
        })
    }

    // Shifts (reject negative shift counts)

    pub fn shl(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(s)) if *s >= 0 => Ok(Value::Int(x.wrapping_shl(*s as u32))),
            (Value::BigInt(x), Value::Int(s)) if *s >= 0 => {
                Ok(Value::from_bigint(x << (*s as u32)))
            }
            (Value::Int(x), Value::BigInt(s)) => {
                let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
                Ok(Value::Int(x.wrapping_shl(shift)))
            }
            (Value::BigInt(x), Value::BigInt(s)) => {
                let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
                Ok(Value::from_bigint(x << shift))
            }
            _ => Err(expected_integer2(a, b)),
        })
    }

    pub fn shr(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(s)) if *s >= 0 => Ok(Value::Int(x.wrapping_shr(*s as u32))),
            (Value::BigInt(x), Value::Int(s)) if *s >= 0 => {
                Ok(Value::from_bigint(x >> (*s as u32)))
            }
            (Value::Int(x), Value::BigInt(s)) => {
                let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
                Ok(Value::Int(x.wrapping_shr(shift)))
            }
            (Value::BigInt(x), Value::BigInt(s)) => {
                let shift = s.to_u32().ok_or_else(|| invalid_shift(b))?;
                Ok(Value::from_bigint(x >> shift))
            }
            _ => Err(expected_integer2(a, b)),
        })
    }

    // Logical ==============================================================================================================

    pub fn and_bool(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
            _ => Err(expected_bool2(a, b)),
        })
    }

    pub fn or_bool(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
            _ => Err(expected_bool2(a, b)),
        })
    }

    pub fn xor_bool(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a ^ *b)),
            _ => Err(expected_bool2(a, b)),
        })
    }

    pub fn not_bool(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Bool(b) => Ok(Value::Bool(!b)),
            _ => Err(expected_bool1(v)),
        })
    }

    pub fn chr(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(i) => {
                let ch = u32::try_from(*i) // reject negatives/overflow
                    .ok()
                    .and_then(char::from_u32) // reject > 0x10FFFF and surrogates
                    .ok_or_else(|| invalid_unicode(v))?;
                Ok(Value::Char(ch))
            }
            Value::BigInt(n) => {
                let ch = n
                    .to_u32()
                    .and_then(char::from_u32)
                    .ok_or_else(|| invalid_unicode(v))?;
                Ok(Value::Char(ch))
            }
            _ => Err(expected_integer1(v)),
        })
    }

    pub fn ord(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Char(c) => Ok(Value::Int(i64::from(u32::from(*c)))),
            // Value::Symbol(s) => {
            //     let mut codes = Vec::with_capacity(s.chars().count());
            //     codes.extend(s.chars().map(|c| i64::from(u32::from(c))));
            //     Ok(Value::IntList(codes))
            // }
            _ => Err(WqError::new(WqErrorType::Domain)
                .msg("expected char or list<char>")
                .got1(v)),
        })
    }

    pub fn to_hex_repr(&self, with_prefix: bool) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 16, with_prefix, "0x");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            Value::BigInt(n) => {
                let s = to_bigint_radix_string(n, 16, with_prefix, "0x");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            _ => Err(expected_integer1(v)),
        })
    }

    pub fn to_bin_repr(&self, with_prefix: bool) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 2, with_prefix, "0b");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            Value::BigInt(n) => {
                let s = to_bigint_radix_string(n, 2, with_prefix, "0b");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            _ => Err(expected_integer1(v)),
        })
    }

    pub fn to_oct_repr(&self, with_prefix: bool) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => {
                let s = to_radix_string(*n, 8, with_prefix, "0o");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            Value::BigInt(n) => {
                let s = to_bigint_radix_string(n, 8, with_prefix, "0o");
                Ok(Value::List(s.chars().map(Value::Char).collect()))
            }
            _ => Err(expected_integer1(v)),
        })
    }
}

/// Build a signed string in the given radix with optional prefix.
/// Sign always precedes the prefix.
fn to_radix_string(n: i64, base: u32, with_prefix: bool, prefix: &str) -> String {
    let neg = n < 0;
    // Use i128 to avoid overflow on i64::MIN when taking the absolute value.
    let mag_i128: i128 = if neg { -(n as i128) } else { n as i128 };
    let digits = match base {
        16 => format!("{mag_i128:x}",),
        8 => format!("{mag_i128:o}",),
        2 => format!("{mag_i128:b}",),
        _ => unreachable!("to_radix_string only used for base 2, 8 and 16"),
    };
    match (neg, with_prefix) {
        (true, true) => format!("-{prefix}{digits}"),
        (true, false) => format!("-{digits}"),
        (false, true) => format!("{prefix}{digits}"),
        (false, false) => digits,
    }
}

fn to_bigint_radix_string(n: &BigInt, base: u32, with_prefix: bool, prefix: &str) -> String {
    let neg = n.is_negative();
    let mag = n.abs().to_str_radix(base);
    match (neg, with_prefix) {
        (true, true) => format!("-{prefix}{mag}"),
        (true, false) => format!("-{mag}"),
        (false, true) => format!("{prefix}{mag}"),
        (false, false) => mag,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    #[test]
    fn chr_valid() {
        assert_eq!(Value::Int(65).chr().unwrap(), Value::Char('A'));
        assert_eq!(Value::Int(0x1F600).chr().unwrap(), Value::Char('😀'));
    }

    #[test]
    fn chr_invalid() {
        assert!(Value::Int(-1).chr().is_err());
        assert!(Value::Int(0x110000).chr().is_err()); // > Unicode max
    }

    #[test]
    fn add_overflow_promotes_to_bigint() {
        let a = Value::Int(i64::MAX);
        let b = Value::Int(1);
        let result = a.add(&b).unwrap();
        match result {
            Value::BigInt(ref n) => {
                let expected = BigInt::from(i64::MAX) + BigInt::from(1);
                assert_eq!(*n, expected);
            }
            other => panic!("expected bigint result, got {other:?}"),
        }
    }

    // #[test]
    // fn ord_char_and_symbol() {
    //     assert_eq!(Value::Char('A').ord().unwrap(), Value::Int(65));
    //     assert_eq!(
    //         Value::Symbol("A😀".into()).ord().unwrap(),
    //         Value::IntList(vec![65, 0x1F600])
    //     );
    // }
}
