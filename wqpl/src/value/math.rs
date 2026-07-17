use num_bigint::BigInt;
use num_complex::Complex64;
use num_traits::{One, Signed, Zero};

use crate::cas::cas_call_expr;
use crate::value::bc::{Bc1Stop, Bc2Stop};
use crate::value::cas::CasFunction;
use crate::value::{Value, WqResult, expected_numeric1, expected_numeric2};
use crate::wqerror::{WqError, WqErrorType};

#[inline]
fn guard_nan<F>(res: f64, err: F) -> Result<f64, WqError>
where
    F: FnOnce() -> WqError,
{
    if res.is_nan() { Err(err()) } else { Ok(res) }
}

#[inline]
fn math_nan_err1(op: &str, arg: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg(format!("'{op}' is not defined for the given value"))
        .attach_note("builtin math functions are defined over the real numbers")
        .got1(arg)
}

#[inline]
fn math_nan_err2(op: &str, lhs: &Value, rhs: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg(format!("'{op}' is not defined for the given values"))
        .attach_note("builtin math functions are defined over the real numbers")
        .got2(lhs, rhs)
}

#[inline]
fn unary_float_math<F>(op: &str, arg: &Value, func: F) -> Result<Value, WqError>
where
    F: FnOnce(f64) -> f64,
{
    let input = arg.as_f64().ok_or_else(|| expected_numeric1(arg))?;
    guard_nan(func(input), || math_nan_err1(op, arg)).map(Value::float)
}

#[inline]
fn unary_complex_math<FR, FC>(
    function: CasFunction,
    arg: &Value,
    real_func: FR,
    complex_func: FC,
) -> Result<Value, WqError>
where
    FR: FnOnce(f64) -> f64,
    FC: FnOnce(Complex64) -> Complex64,
{
    if arg.is_cas_expr() {
        return cas_call_expr(function, std::slice::from_ref(arg));
    }
    if arg.is_complex() {
        return arg
            .as_complex64()
            .map(complex_func)
            .map(Value::from_complex64)
            .ok_or_else(|| expected_numeric1(arg));
    }

    let op = function.name();
    let input = arg.as_f64().ok_or_else(|| expected_numeric1(arg))?;
    let real_res = real_func(input);
    if !real_res.is_nan() {
        return Ok(Value::float(real_res));
    }

    let complex_res = complex_func(Complex64::new(input, 0.0));
    if complex_res.re.is_nan() || complex_res.im.is_nan() {
        return Err(math_nan_err1(op, arg));
    }
    Ok(Value::from_complex64(complex_res))
}

#[inline]
fn unary_float_to_int<F>(op: &str, arg: &Value, func: F) -> Result<Value, WqError>
where
    F: FnOnce(f64) -> f64,
{
    unary_float_math(op, arg, func).map(|res| match res {
        Value::Float(f) => Value::Int(*f as i64),
        other => other,
    })
}

#[inline]
fn binary_float_math<F>(
    function: CasFunction,
    lhs: &Value,
    rhs: &Value,
    func: F,
) -> Result<Value, WqError>
where
    F: FnOnce(f64, f64) -> f64,
{
    if lhs.is_cas_expr() || rhs.is_cas_expr() {
        return cas_call_expr(function, &[lhs.clone(), rhs.clone()]);
    }
    let op = function.name();
    let left = match lhs.as_f64() {
        Some(v) => v,
        None => return Err(expected_numeric2(lhs, rhs)),
    };
    let right = match rhs.as_f64() {
        Some(v) => v,
        None => return Err(expected_numeric2(lhs, rhs)),
    };
    guard_nan(func(left, right), || math_nan_err2(op, lhs, rhs)).map(Value::float)
}

#[inline]
fn binary_complex_math<FR, FC>(
    function: CasFunction,
    lhs: &Value,
    rhs: &Value,
    real_func: FR,
    complex_func: FC,
) -> Result<Value, WqError>
where
    FR: FnOnce(f64, f64) -> f64,
    FC: FnOnce(Complex64, Complex64) -> Complex64,
{
    if lhs.is_cas_expr() || rhs.is_cas_expr() {
        return cas_call_expr(function, &[lhs.clone(), rhs.clone()]);
    }
    let op = function.name();
    if lhs.is_complex() || rhs.is_complex() {
        let left = lhs
            .as_complex64()
            .ok_or_else(|| expected_numeric2(lhs, rhs))?;
        let right = rhs
            .as_complex64()
            .ok_or_else(|| expected_numeric2(lhs, rhs))?;
        return Ok(Value::from_complex64(complex_func(left, right)));
    }

    let left = match lhs.as_f64() {
        Some(v) => v,
        None => return Err(expected_numeric2(lhs, rhs)),
    };
    let right = match rhs.as_f64() {
        Some(v) => v,
        None => return Err(expected_numeric2(lhs, rhs)),
    };
    let real_res = real_func(left, right);
    if !real_res.is_nan() {
        return Ok(Value::float(real_res));
    }

    let complex_res = complex_func(Complex64::new(left, 0.0), Complex64::new(right, 0.0));
    if complex_res.re.is_nan() || complex_res.im.is_nan() {
        return Err(math_nan_err2(op, lhs, rhs));
    }
    Ok(Value::from_complex64(complex_res))
}

#[inline]
fn rational_floor_value(numer: &BigInt, denom: &BigInt) -> BigInt {
    let q = numer / denom;
    let r = numer % denom;
    if !r.is_zero() && numer.is_negative() {
        q - BigInt::one()
    } else {
        q
    }
}

#[inline]
fn rational_ceil_value(numer: &BigInt, denom: &BigInt) -> BigInt {
    let q = numer / denom;
    let r = numer % denom;
    if !r.is_zero() && numer.is_positive() {
        q + BigInt::one()
    } else {
        q
    }
}

#[inline]
fn rational_round_value(numer: &BigInt, denom: &BigInt) -> BigInt {
    let q = numer / denom;
    let r = numer % denom;
    if r.is_zero() {
        return q;
    }
    if (r.abs() * 2) < *denom {
        q
    } else if numer.is_negative() {
        q - BigInt::one()
    } else {
        q + BigInt::one()
    }
}

impl Value {
    pub(crate) fn abs(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| match v {
            _ if v.is_cas_expr() => cas_call_expr(CasFunction::Abs, std::slice::from_ref(v)),
            Value::Int(n) => Ok(match n.checked_abs() {
                Some(m) => Value::Int(m),
                None => Value::from_bigint(BigInt::from(*n).abs()),
            }),
            Value::BigInt(n) => Ok(Value::from_bigint(n.abs())),
            Value::Float(_) => unary_float_math("abs", v, |x| x.abs()),
            _ if v.is_complex() => Ok(Value::float(
                v.as_complex64().ok_or_else(|| expected_numeric1(v))?.norm(),
            )),
            _ if v.is_algebraic_number() => {
                if let Value::Algebraic(a) = v {
                    match a.sign() {
                        crate::value::algebraic::NumericSign::Negative => v.neg(),
                        crate::value::algebraic::NumericSign::Zero
                        | crate::value::algebraic::NumericSign::Positive => Ok(v.clone()),
                        crate::value::algebraic::NumericSign::Unknown => Err(expected_numeric1(v)),
                    }
                } else {
                    unreachable!("algebraic branch only handles algebraic values")
                }
            }
            _ if v.is_fraction() => {
                let (numer, denom) = v.dict_fraction_parts().unwrap();
                Ok(Value::from_fraction_parts(numer.abs(), denom))
            }
            _ => Err(expected_numeric1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn sgn(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| match v {
            _ if v.is_cas_expr() => cas_call_expr(CasFunction::Sgn, std::slice::from_ref(v)),
            Value::Int(n) => Ok(Value::Int(n.signum())),
            Value::BigInt(n) => Ok(Value::Int(if n.is_zero() {
                0
            } else if n.is_positive() {
                1
            } else {
                -1
            })),
            Value::Float(_) => unary_float_math("sgn", v, |x| x.signum()),
            _ if v.is_fraction() => {
                let (numer, _) = v.dict_fraction_parts().unwrap();
                Ok(Value::Int(if numer.is_zero() {
                    0
                } else if numer.is_positive() {
                    1
                } else {
                    -1
                }))
            }
            _ => Err(expected_numeric1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn sqrt(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Sqrt, v, |x| x.sqrt(), |z| z.sqrt())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn exp(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Exp, v, |x| x.exp(), |z| z.exp())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ln(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Ln, v, |x| x.ln(), |z| z.ln())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn log2(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(
                CasFunction::Log2,
                v,
                |x| x.log2(),
                |z| z.ln() / Complex64::new(2.0_f64.ln(), 0.0),
            )
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn log10(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(
                CasFunction::Log10,
                v,
                |x| x.log10(),
                |z| z.ln() / Complex64::new(10.0_f64.ln(), 0.0),
            )
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn log(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |v1, v2| {
            binary_complex_math(
                CasFunction::Log,
                v1,
                v2,
                |x, y| x.log(y),
                |x, y| x.ln() / y.ln(),
            )
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arctan2(&self, other: &Value) -> WqResult<Value> {
        self.bc2(other, |v1, v2| {
            binary_float_math(CasFunction::ArcTan2, v1, v2, |x, y| x.atan2(y))
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn floor(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            _ if v.is_cas_expr() => cas_call_expr(CasFunction::Floor, std::slice::from_ref(v)),
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            // cast to i64
            Value::Float(_) => unary_float_to_int("floor", v, |x| x.floor()),
            _ if v.is_fraction() => {
                let (numer, denom) = v.dict_fraction_parts().unwrap();
                Ok(Value::from_bigint(rational_floor_value(&numer, &denom)))
            }
            _ => Err(expected_numeric1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ceil(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            _ if v.is_cas_expr() => cas_call_expr(CasFunction::Ceil, std::slice::from_ref(v)),
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            Value::Float(_) => unary_float_to_int("ceil", v, |x| x.ceil()),
            _ if v.is_fraction() => {
                let (numer, denom) = v.dict_fraction_parts().unwrap();
                Ok(Value::from_bigint(rational_ceil_value(&numer, &denom)))
            }
            _ => Err(expected_numeric1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn round(&self) -> WqResult<Value> {
        self.bc1(|v| match v {
            _ if v.is_cas_expr() => cas_call_expr(CasFunction::Round, std::slice::from_ref(v)),
            Value::Int(n) => Ok(Value::Int(*n)),
            Value::BigInt(n) => Ok(Value::BigInt(n.clone())),
            Value::Float(_) => unary_float_to_int("round", v, |x| x.round()),
            _ if v.is_fraction() => {
                let (numer, denom) = v.dict_fraction_parts().unwrap();
                Ok(Value::from_bigint(rational_round_value(&numer, &denom)))
            }
            _ => Err(expected_numeric1(v)),
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn sin(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Sin, v, |x| x.sin(), |z| z.sin())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn cos(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Cos, v, |x| x.cos(), |z| z.cos())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn tan(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Tan, v, |x| x.tan(), |z| z.tan())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn sec(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(
                CasFunction::Sec,
                v,
                |x| 1.0 / x.cos(),
                |z| Complex64::new(1.0, 0.0) / z.cos(),
            )
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn csc(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(
                CasFunction::Csc,
                v,
                |x| 1.0 / x.sin(),
                |z| Complex64::new(1.0, 0.0) / z.sin(),
            )
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn cot(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(
                CasFunction::Cot,
                v,
                |x| 1.0 / x.tan(),
                |z| Complex64::new(1.0, 0.0) / z.tan(),
            )
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn erf(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| unary_float_math("erf", v, libm::erf))
            .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn erfc(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| unary_float_math("erfc", v, libm::erfc))
            .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn gamma(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_float_math("gamma", v, libm::tgamma)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn lngamma(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_float_math("lngamma", v, libm::lgamma)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn si(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            if v.is_cas_expr() {
                return cas_call_expr(CasFunction::Si, std::slice::from_ref(v));
            }
            let x = v.as_f64().ok_or_else(|| expected_numeric1(v))?;
            let res = crate::cephes::si(x);
            guard_nan(res, || math_nan_err1("si", v)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ci(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            if v.is_cas_expr() {
                return cas_call_expr(CasFunction::Ci, std::slice::from_ref(v));
            }
            let x = v.as_f64().ok_or_else(|| expected_numeric1(v))?;
            let res = crate::cephes::ci(x);
            guard_nan(res, || math_nan_err1("ci", v)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ei(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            if v.is_cas_expr() {
                return cas_call_expr(CasFunction::Ei, std::slice::from_ref(v));
            }
            let x = v.as_f64().ok_or_else(|| expected_numeric1(v))?;
            let res = crate::cephes::ei(x);
            guard_nan(res, || math_nan_err1("ei", v)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn en(&self, other: &Value) -> WqResult<Value> {
        self.bc2_until(other, Bc2Stop::BothAtom, |v1, v2| {
            if v1.is_cas_expr() || v2.is_cas_expr() {
                return cas_call_expr(CasFunction::En, &[v1.clone(), v2.clone()]);
            }
            let n = v1.as_f64().ok_or_else(|| expected_numeric2(v1, v2))?;
            let x = v2.as_f64().ok_or_else(|| expected_numeric2(v1, v2))?;
            let res = crate::cephes::en(n as i32, x);
            guard_nan(res, || math_nan_err2("en", v1, v2)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ellpk(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            if v.is_cas_expr() {
                return cas_call_expr(CasFunction::EllPk, std::slice::from_ref(v));
            }
            let x = v.as_f64().ok_or_else(|| expected_numeric1(v))?;
            let res = crate::cephes::ellpk(x);
            guard_nan(res, || math_nan_err1("ellpk", v)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ellpe(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            if v.is_cas_expr() {
                return cas_call_expr(CasFunction::EllPe, std::slice::from_ref(v));
            }
            let x = v.as_f64().ok_or_else(|| expected_numeric1(v))?;
            let res = crate::cephes::ellpe(x);
            guard_nan(res, || math_nan_err1("ellpe", v)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ellik(&self, other: &Value) -> WqResult<Value> {
        self.bc2_until(other, Bc2Stop::BothAtom, |v1, v2| {
            if v1.is_cas_expr() || v2.is_cas_expr() {
                return cas_call_expr(CasFunction::EllIk, &[v1.clone(), v2.clone()]);
            }
            let phi = v1.as_f64().ok_or_else(|| expected_numeric2(v1, v2))?;
            let m = v2.as_f64().ok_or_else(|| expected_numeric2(v1, v2))?;
            let res = crate::cephes::ellik(phi, m);
            guard_nan(res, || math_nan_err2("ellik", v1, v2)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn ellie(&self, other: &Value) -> WqResult<Value> {
        self.bc2_until(other, Bc2Stop::BothAtom, |v1, v2| {
            if v1.is_cas_expr() || v2.is_cas_expr() {
                return cas_call_expr(CasFunction::EllIe, &[v1.clone(), v2.clone()]);
            }
            let phi = v1.as_f64().ok_or_else(|| expected_numeric2(v1, v2))?;
            let m = v2.as_f64().ok_or_else(|| expected_numeric2(v1, v2))?;
            let res = crate::cephes::ellie(phi, m);
            guard_nan(res, || math_nan_err2("ellie", v1, v2)).map(Value::float)
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn heaviside(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            if v.is_cas_expr() {
                return cas_call_expr(CasFunction::Heaviside, std::slice::from_ref(v));
            }
            let input = v.as_f64().ok_or_else(|| expected_numeric1(v))?;
            let result = if input < 0.0 {
                0.0
            } else if input > 0.0 {
                1.0
            } else {
                0.5
            };
            Ok(Value::float(result))
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn sinh(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Sinh, v, |x| x.sinh(), |z| z.sinh())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn cosh(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Cosh, v, |x| x.cosh(), |z| z.cosh())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn tanh(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::Tanh, v, |x| x.tanh(), |z| z.tanh())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arcsin(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::ArcSin, v, |x| x.asin(), |z| z.asin())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arccos(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::ArcCos, v, |x| x.acos(), |z| z.acos())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arctan(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::ArcTan, v, |x| x.atan(), |z| z.atan())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arcsinh(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::ArcSinh, v, |x| x.asinh(), |z| z.asinh())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arccosh(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::ArcCosh, v, |x| x.acosh(), |z| z.acosh())
        })
        .map_err(|e| e.into_wqerror())
    }

    pub(crate) fn arctanh(&self) -> WqResult<Value> {
        self.bc1_until(Bc1Stop::Atom, |v| {
            unary_complex_math(CasFunction::ArcTanh, v, |x| x.atanh(), |z| z.atanh())
        })
        .map_err(|e| e.into_wqerror())
    }
}
