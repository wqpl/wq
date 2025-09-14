use crate::{
    value::{
        Excerpt, Value,
        bc::BcResult,
        wqerr_ext::{expected_numeric1, expected_numeric2},
    },
    wqerr::{WqErr, WqErrType},
};

use num_bigint::BigInt;
use num_traits::{Signed, Zero};

#[inline]
fn guard_nan<F>(res: f64, err: F) -> Result<f64, WqErr>
where
    F: FnOnce() -> WqErr,
{
    if res.is_nan() { Err(err()) } else { Ok(res) }
}

#[inline]
fn math_nan_err1(op: &str, arg: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg(format!("{op} is not defined for given value"))
        .attach_note("builtin math functions are defined on real set")
        .attach_note(format!("got {}", arg.excerpt()))
}

#[inline]
fn math_nan_err2(op: &str, lhs: &Value, rhs: &Value) -> WqErr {
    WqErr::new(WqErrType::Domain)
        .msg(format!("{op} is not defined for given values"))
        .attach_note("builtin math functions are defined on real set")
        .attach_note(format!("got {} for lhs", lhs.excerpt()))
        .attach_note(format!("got {} for rhs", rhs.excerpt()))
}

#[inline]
fn unary_float_math<F>(op: &str, arg: &Value, func: F) -> Result<Value, WqErr>
where
    F: FnOnce(f64) -> f64,
{
    let input = arg.as_f64().ok_or_else(|| expected_numeric1(arg))?;
    guard_nan(func(input), || math_nan_err1(op, arg)).map(Value::Float)
}

#[inline]
fn unary_float_to_int<F>(op: &str, arg: &Value, func: F) -> Result<Value, WqErr>
where
    F: FnOnce(f64) -> f64,
{
    unary_float_math(op, arg, func).map(|res| match res {
        Value::Float(f) => Value::Int(f as i64),
        other => other,
    })
}

#[inline]
fn binary_float_math<F>(op: &str, lhs: &Value, rhs: &Value, func: F) -> Result<Value, WqErr>
where
    F: FnOnce(f64, f64) -> f64,
{
    let left = match lhs.as_f64() {
        Some(v) => v,
        None => return Err(expected_numeric2(lhs, rhs)),
    };
    let right = match rhs.as_f64() {
        Some(v) => v,
        None => return Err(expected_numeric2(lhs, rhs)),
    };
    guard_nan(func(left, right), || math_nan_err2(op, lhs, rhs)).map(Value::Float)
}

impl Value {
    pub fn abs(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(match n.checked_abs() {
                Some(m) => Value::Int(m),
                None => Value::from_bigint(BigInt::from(*n).abs()),
            }),
            Value::BigInt(n) => Ok(Value::from_bigint(n.abs())),
            Value::Float(_) => unary_float_math("abs", v, |x| x.abs()),
            _ => Err(expected_numeric1(v)),
        })
    }

    pub fn sgn(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(n.signum())),
            Value::BigInt(n) => Ok(Value::Int(if n.is_zero() {
                0
            } else if n.is_positive() {
                1
            } else {
                -1
            })),
            Value::Float(_) => unary_float_math("sgn", v, |x| x.signum()),
            _ => Err(expected_numeric1(v)),
        })
    }

    pub fn sqrt(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("sqrt", v, |x| x.sqrt()))
    }

    pub fn exp(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("exp", v, |x| x.exp()))
    }

    pub fn ln(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("ln", v, |x| x.ln()))
    }

    pub fn log2(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("log2", v, |x| x.log2()))
    }

    pub fn log10(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("log10", v, |x| x.log10()))
    }

    pub fn log(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |v1, v2| {
            binary_float_math("log", v1, v2, |x, y| x.log(y))
        })
    }

    pub fn arctan2(&self, other: &Value) -> BcResult<Value> {
        self.bc2(other, |v1, v2| {
            binary_float_math("arctan2", v1, v2, |x, y| x.atan2(y))
        })
    }

    pub fn floor(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            // cast to i64
            Value::Float(_) => unary_float_to_int("floor", v, |x| x.floor()),
            _ => Err(expected_numeric1(v)),
        })
    }

    pub fn ceil(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            Value::Float(_) => unary_float_to_int("ceil", v, |x| x.ceil()),
            _ => Err(expected_numeric1(v)),
        })
    }

    pub fn round(&self) -> BcResult<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            Value::Float(_) => unary_float_to_int("round", v, |x| x.round()),
            _ => Err(expected_numeric1(v)),
        })
    }

    pub fn sin(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("sin", v, |x| x.sin()))
    }

    pub fn cos(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("cos", v, |x| x.cos()))
    }

    pub fn tan(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("tan", v, |x| x.tan()))
    }

    pub fn sinh(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("sinh", v, |x| x.sinh()))
    }

    pub fn cosh(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("cosh", v, |x| x.cosh()))
    }

    pub fn tanh(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("tanh", v, |x| x.tanh()))
    }

    pub fn arcsin(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("arcsin", v, |x| x.asin()))
    }

    pub fn arccos(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("arccos", v, |x| x.acos()))
    }

    pub fn arctan(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("arctan", v, |x| x.atan()))
    }

    pub fn arcsinh(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("arcsinh", v, |x| x.asinh()))
    }

    pub fn arccosh(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("arccosh", v, |x| x.acosh()))
    }

    pub fn arctanh(&self) -> BcResult<Value> {
        self.bc1(|v| unary_float_math("arctanh", v, |x| x.atanh()))
    }
}
