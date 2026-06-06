use num_bigint::BigInt;
use num_rational::Ratio;
use num_traits::{FromPrimitive, Num, One, Signed, Zero};

use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::{Excerpt, Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

const FRACTIONL_DENOM_LIMIT: i64 = 1_000_000;

pub(super) fn fraction(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Fraction, [1, 2], &args)?;
    match &*args {
        [value] => fraction_impl(value, BuiltinEnum::Fraction, None),
        [a, b] => match (a, b) {
            (Value::Int(n), Value::Int(d)) => {
                if *d == 0 {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BuiltinEnum::Fraction)
                        .msg("denominator cannot be zero"));
                }
                Ok(Value::from_fraction_parts(
                    BigInt::from(*n),
                    BigInt::from(*d),
                ))
            }
            (Value::BigInt(n), Value::Int(d)) => {
                if *d == 0 {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BuiltinEnum::Fraction)
                        .msg("denominator cannot be zero"));
                }
                Ok(Value::raw_from_fraction_parts_ref(
                    n.as_ref(),
                    &BigInt::from(*d),
                ))
            }
            (Value::Int(n), Value::BigInt(d)) => {
                if d.is_zero() {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BuiltinEnum::Fraction)
                        .msg("denominator cannot be zero"));
                }
                Ok(Value::raw_from_fraction_parts_ref(
                    &BigInt::from(*n),
                    d.as_ref(),
                ))
            }
            (Value::BigInt(n), Value::BigInt(d)) => {
                if d.is_zero() {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BuiltinEnum::Fraction)
                        .msg("denominator cannot be zero"));
                }
                Ok(Value::raw_from_fraction_parts_ref(n.as_ref(), d.as_ref()))
            }
            _ => {
                let limit = parse_denominator_limit(b, BuiltinEnum::Fraction)?;
                fraction_impl(a, BuiltinEnum::Fraction, Some(&limit))
            }
        },
        _ => unreachable!(),
    }
}

pub(super) fn fractionl(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Fractionl, [1], &args)?;
    let limit = BigInt::from(FRACTIONL_DENOM_LIMIT);
    fraction_impl(&args[0], BuiltinEnum::Fractionl, Some(&limit))
}

fn limit_denominator(numer: BigInt, denom: BigInt, max_denom: &BigInt) -> (BigInt, BigInt) {
    debug_assert!(!denom.is_zero());
    debug_assert!(*max_denom > BigInt::zero());

    if numer.is_zero() || denom <= *max_denom {
        return (numer, denom);
    }

    let sign = numer.signum();
    let mut n = numer.abs();
    let d = denom;
    let original_n = n.clone();
    let original_d = d.clone();
    let mut rem_d = d;

    let mut p0 = BigInt::zero();
    let mut q0 = BigInt::one();
    let mut p1 = BigInt::one();
    let mut q1 = BigInt::zero();

    loop {
        let a = &n / &rem_d;
        let q2 = &q0 + &a * &q1;
        if q2 > *max_denom {
            break;
        }

        let p2 = &p0 + &a * &p1;
        p0 = p1;
        q0 = q1;
        p1 = p2;
        q1 = q2;

        let next = &n - &a * &rem_d;
        if next.is_zero() {
            break;
        }
        n = rem_d;
        rem_d = next;
    }

    let k = if q1.is_zero() {
        BigInt::zero()
    } else {
        (max_denom - &q0) / &q1
    };

    let lower_n = &p0 + &k * &p1;
    let lower_d = &q0 + &k * &q1;
    let upper_n = p1;
    let upper_d = q1;

    let lower_err = (&original_n * &lower_d - &lower_n * &original_d).abs() * &upper_d;
    let upper_err = (&original_n * &upper_d - &upper_n * &original_d).abs() * &lower_d;

    let (best_n, best_d) = if upper_err <= lower_err {
        (upper_n, upper_d)
    } else {
        (lower_n, lower_d)
    };

    let r = Ratio::new(sign * best_n, best_d);
    (r.numer().clone(), r.denom().clone())
}

fn parse_denominator_limit(arg: &Value, builtin: BuiltinEnum) -> WqResult<BigInt> {
    match arg {
        Value::Int(n) if *n > 0 => Ok(BigInt::from(*n)),
        Value::BigInt(n) if n.is_positive() => Ok((**n).clone()),
        Value::Int(_) | Value::BigInt(_) => Err(WqError::new(WqErrorType::Domain)
            .src(builtin)
            .msg("expected positive int or bigint for denominator limit")
            .at_arg(0)),
        _ => Err(WqError::new(WqErrorType::Domain)
            .src(builtin)
            .msg("expected positive int or bigint for denominator limit")
            .at_arg(0)
            .got1(arg)),
    }
}

fn fraction_impl(value: &Value, builtin: BuiltinEnum, limit: Option<&BigInt>) -> WqResult<Value> {
    let (numer, denom) = exact_fraction_from_value(value, builtin)?;
    let (numer, denom) = match limit {
        Some(max_denom) => limit_denominator(numer, denom, max_denom),
        None => (numer, denom),
    };
    Ok(Value::from_fraction_parts(numer, denom))
}

fn exact_fraction_from_f64(value: f64) -> (BigInt, BigInt) {
    let r: Ratio<BigInt> = Ratio::from_f64(value).expect("finite f64");
    (r.numer().clone(), r.denom().clone())
}

fn exact_fraction_from_value(value: &Value, builtin: BuiltinEnum) -> WqResult<(BigInt, BigInt)> {
    if let Some(parts) = value.rational_parts() {
        return Ok(parts);
    }

    match value {
        Value::Float(f) if f.is_finite() => Ok(exact_fraction_from_f64(**f)),
        Value::Float(_) => Err(WqError::new(WqErrorType::Domain)
            .msg(format!("{} is not defined for given value", builtin.name()))
            .attach_note("builtin fraction functions require finite numbers")
            .attach_note(format!("got {}", value.excerpt()))),
        Value::String(s) => parse_fraction_string(s, builtin),
        Value::Char(c) => parse_fraction_string(&c.to_string(), builtin),
        Value::IntList(l) if value.len() == 2 => Ok((l[0].into(), l[1].into())),
        Value::List(l) if value.len() == 2 => match (&l[0], &l[1]) {
            (Value::Int(n), Value::Int(d)) => Ok((BigInt::from(*n), BigInt::from(*d))),
            (Value::BigInt(n), Value::Int(d)) => Ok(((**n).clone(), BigInt::from(*d))),
            (Value::Int(n), Value::BigInt(d)) => Ok((BigInt::from(*n), (**d).clone())),
            (Value::BigInt(n), Value::BigInt(d)) => Ok(((**n).clone(), (**d).clone())),
            _ => Err(WqError::new(WqErrorType::Domain)
                .msg("expected int, bigint, float, string or fraction")
                .got1(value)),
        },
        _ => Err(WqError::new(WqErrorType::Domain)
            .msg("expected int, bigint, float, string or fraction")
            .got1(value)),
    }
}

/// Parse an integer literal using the same semantics as the lexer:
/// supports `0x` / `0b` / `0o` prefixes and `_` digit separators.
fn parse_integer_literal(s: &str) -> Option<BigInt> {
    let s = s.replace('_', "");
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        BigInt::from_str_radix(rest, 16).ok()
    } else if let Some(rest) = s.strip_prefix("0b").or_else(|| s.strip_prefix("0B")) {
        BigInt::from_str_radix(rest, 2).ok()
    } else if let Some(rest) = s.strip_prefix("0o").or_else(|| s.strip_prefix("0O")) {
        BigInt::from_str_radix(rest, 8).ok()
    } else {
        s.parse::<BigInt>().ok()
    }
}

fn parse_decimal_fraction(s: &str) -> Option<(BigInt, BigInt)> {
    let s = s.trim();
    let (sign, rest) = match s.strip_prefix('-') {
        Some(r) => (BigInt::from(-1), r),
        None => (BigInt::from(1), s.strip_prefix('+').unwrap_or(s)),
    };

    let dot_idx = rest.find('.')?;
    let int_part = &rest[..dot_idx];
    let frac_part = &rest[dot_idx + 1..];

    // Reject scientific notation (e.g. "1.2e3") – fall back to f64 path.
    if rest.contains('e') || rest.contains('E') {
        return None;
    }

    let digits = format!("{}{}", int_part, frac_part);
    if digits.is_empty() {
        return None;
    }
    let numer = digits.parse::<BigInt>().ok()? * sign;
    let exp = frac_part.len().try_into().ok()?;
    let denom = BigInt::from(10).pow(exp);
    Some((numer, denom))
}

fn parse_fraction_string(s: &str, builtin: BuiltinEnum) -> WqResult<(BigInt, BigInt)> {
    let s = s.trim();
    if let Some(idx) = s.find('/') {
        let numer_str = s[..idx].trim();
        let denom_str = s[idx + 1..].trim();
        let numer = parse_integer_literal(numer_str).ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(builtin)
                .msg("invalid numerator in fraction string")
        })?;
        let denom = parse_integer_literal(denom_str).ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(builtin)
                .msg("invalid denominator in fraction string")
        })?;
        if denom.is_zero() {
            return Err(WqError::new(WqErrorType::Domain)
                .src(builtin)
                .msg("denominator cannot be zero"));
        }
        Ok((numer, denom))
    } else if let Some(n) = parse_integer_literal(s) {
        Ok((n, BigInt::one()))
    } else if let Some((n, d)) = parse_decimal_fraction(s) {
        Ok((n, d))
    } else if let Ok(f) = s.parse::<f64>() {
        if f.is_finite() {
            Ok(exact_fraction_from_f64(f))
        } else {
            Err(WqError::new(WqErrorType::Domain)
                .src(builtin)
                .msg("builtin fraction functions require finite numbers"))
        }
    } else {
        Err(WqError::new(WqErrorType::Domain)
            .src(builtin)
            .msg("expected fraction string like \"1/2\" or numeric literal"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use smallvec::smallvec;

    use super::*;
    use crate::value::into_wq_string;

    #[test]
    fn fraction_returns_exact_ratio_for_simple_float() {
        let result = fraction(BuiltinFnArgs::from(Value::float(0.5))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2))
        );
    }

    #[test]
    fn fraction_applies_denominator_limit() {
        let result = fraction(BuiltinFnArgs::from(smallvec![
            Value::float(1.0 / 3.0),
            Value::Int(10)
        ]))
        .unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(3))
        );
    }

    #[test]
    fn fractionl_uses_default_limit() {
        let result = fractionl(BuiltinFnArgs::from(Value::float(0.1))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(10))
        );
    }

    #[test]
    fn fraction_promotes_bigint_output_when_needed() {
        let big = BigInt::from(i64::MAX) + BigInt::from(1);
        let result = fraction(BuiltinFnArgs::from(Value::BigInt(Arc::new(big.clone())))).unwrap();
        assert_eq!(result, Value::from_fraction_parts(big, BigInt::one()));
    }

    #[test]
    fn fraction_accepts_fraction_like_input() {
        let input = Value::from_fraction_parts(BigInt::from(6), BigInt::from(8));
        let result = fraction(BuiltinFnArgs::from(input)).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(3), BigInt::from(4))
        );
    }

    #[test]
    fn fraction_rejects_non_positive_limit() {
        let result = fraction(BuiltinFnArgs::from(smallvec![
            Value::Int(0),
            Value::float(0.5)
        ]));
        assert!(result.is_err());
    }

    #[test]
    fn fraction_from_two_ints() {
        let result =
            fraction(BuiltinFnArgs::from(smallvec![Value::Int(1), Value::Int(2)])).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2))
        );
    }

    #[test]
    fn fraction_from_string() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("3/4"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(3), BigInt::from(4))
        );
    }

    #[test]
    fn fraction_from_string_integer() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("42"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(42), BigInt::from(1))
        );
    }

    #[test]
    fn fraction_from_string_float() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("0.5"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2))
        );
    }

    #[test]
    fn fraction_from_string_decimal() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("0.3"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(3), BigInt::from(10))
        );
    }

    #[test]
    fn fraction_from_char() {
        let result = fraction(BuiltinFnArgs::from(Value::Char('2'))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(2), BigInt::from(1))
        );
    }

    #[test]
    fn fraction_from_hex_string() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("0x10"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(16), BigInt::from(1))
        );
    }

    #[test]
    fn fraction_from_binary_string() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("0b101"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(5), BigInt::from(1))
        );
    }

    #[test]
    fn fraction_from_underscore_string() {
        let result = fraction(BuiltinFnArgs::from(into_wq_string("1_000"))).unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(1000), BigInt::from(1))
        );
    }

    #[test]
    fn fraction_from_two_bigints() {
        let big = BigInt::from(i64::MAX) + BigInt::from(1);
        let result = fraction(BuiltinFnArgs::from(smallvec![
            Value::BigInt(Arc::new(big.clone())),
            Value::Int(3)
        ]))
        .unwrap();
        assert_eq!(result, Value::from_fraction_parts(big, BigInt::from(3)));
    }
}
