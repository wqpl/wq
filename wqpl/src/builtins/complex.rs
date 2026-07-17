use num_complex::Complex64;

use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::bc::Bc1Stop;
use crate::value::{Value, WqResult};
use crate::wqerror::{Requirement, WqError, WqErrorType};

pub(super) fn complex(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Complex, [2], &args)?;
    let re = args[0].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Complex)
            .expected(Requirement::REAL_NUMBER)
            .at_arg(0)
            .got1(&args[0])
    })?;
    let im = args[1].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Complex)
            .expected(Requirement::REAL_NUMBER)
            .at_arg(1)
            .got1(&args[1])
    })?;
    Ok(Value::from_complex64(Complex64::new(re, im)))
}

pub(super) fn real(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Re, [1], &args)?;
    // Stop broadcast descent at Complex
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
                    .expected(Requirement::one_of([
                        Requirement::REAL_NUMBER,
                        Requirement::COMPLEX,
                    ]))
                    .got1(v))
            }
        })
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Re))
}

pub(super) fn imag(args: BuiltinFnArgs) -> WqResult<Value> {
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
                    .expected(Requirement::one_of([
                        Requirement::REAL_NUMBER,
                        Requirement::COMPLEX,
                    ]))
                    .got1(v))
            }
        })
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Im))
}

pub(super) fn conj(args: BuiltinFnArgs) -> WqResult<Value> {
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
                    .expected(Requirement::one_of([
                        Requirement::REAL_NUMBER,
                        Requirement::COMPLEX,
                    ]))
                    .got1(v))
            }
        })
        .map_err(|e| e.into_wqerror().src(BuiltinEnum::Conj))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn complex_reports_real_number_arguments_consistently() {
        let error = complex(BuiltinFnArgs::from(vec![
            Value::String(Arc::new("one".to_string())),
            Value::Int(0),
        ]))
        .expect_err("string real component should fail");

        assert_eq!(error.msg.as_deref(), Some("expected real number"));
        assert_eq!(
            error.notes.as_ref(),
            &["at argument 1", "got \"one\" (list)"]
        );
    }
}
