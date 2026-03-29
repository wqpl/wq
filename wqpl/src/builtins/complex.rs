use num_complex::Complex64;

use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::bc::Bc1Stop;
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn complex(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Complex, [2], &args)?;
    let re = args[0].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Complex)
            .msg("expected real")
            .at_arg(0)
    })?;
    let im = args[1].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Complex)
            .msg("expected real")
            .at_arg(1)
    })?;
    Ok(Value::from_complex64(Complex64::new(re, im)))
}

pub(super) fn real(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Re, [1], &args)?;
    // Stop broadcast descent at Complex — we want the real part of the
    // whole complex, not to descend into re/im components.
    args[0]
        .bc1_until(Bc1Stop::Atom, |v| {
            if v.is_complex() {
                Ok(Value::float(v.as_complex64().unwrap().re))
            } else if matches!(v, Value::Int(_) | Value::BigInt(_) | Value::Float(_))
                || v.is_fraction()
            {
                Ok(v.clone())
            } else {
                Err(WqError::new(WqErrorType::Domain)
                    .msg("expected real or complex")
                    .got1(v))
            }
        })
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Re))
}

pub(super) fn imag(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Im, [1], &args)?;
    args[0]
        .bc1_until(Bc1Stop::Atom, |v| {
            if v.is_complex() {
                Ok(Value::float(v.as_complex64().unwrap().im))
            } else if matches!(v, Value::Int(_) | Value::BigInt(_) | Value::Float(_))
                || v.is_fraction()
            {
                Ok(Value::Int(0))
            } else {
                Err(WqError::new(WqErrorType::Domain)
                    .msg("expected real or complex")
                    .got1(v))
            }
        })
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Im))
}

pub(super) fn conj(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Conj, [1], &args)?;
    args[0]
        .bc1_until(Bc1Stop::Atom, |v| {
            if v.is_complex() {
                Ok(Value::from_complex64(v.as_complex64().unwrap().conj()))
            } else if matches!(v, Value::Int(_) | Value::BigInt(_) | Value::Float(_))
                || v.is_fraction()
            {
                Ok(v.clone())
            } else {
                Err(WqError::new(WqErrorType::Domain)
                    .msg("expected real or complex")
                    .got1(v))
            }
        })
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Conj))
}
