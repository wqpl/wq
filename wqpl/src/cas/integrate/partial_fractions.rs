use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::split_off_numeric;
use crate::cas::{cas_add, cas_div, cas_mul, cas_sub, numeric_is_one, simplify_cas_value};
use crate::value::{Value, WqResult};

pub(super) fn integrate_by_partial_fractions(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Check for direct ^(-1) form: (denom)^(-1)
    if let Some(("^", [base, exp])) = expr.cas_op_parts()
        && let Some(power) = exp.exact_int()
        && power == BigInt::from(-1)
        && let Some(result) = try_quadratic_denominator(base, var)?
    {
        return Ok(Some(result));
    }

    // Check for * form with (denom)^(-1) as a factor
    if let Some(("*", args)) = expr.cas_op_parts() {
        for arg in args {
            if let Some(("^", [base, exp])) = arg.cas_op_parts()
                && let Some(power) = exp.exact_int()
                && power == BigInt::from(-1)
                && let Some(result) = try_quadratic_denominator(base, var)?
            {
                return Ok(Some(result));
            }
        }
    }

    Ok(None)
}

fn try_quadratic_denominator(denom: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some(("+", args)) = denom.cas_op_parts() else {
        return Ok(None);
    };

    let mut x_sq_coeff: Option<Value> = None;
    let mut const_term: Option<Value> = None;

    for arg in args {
        if let Some(("^", [base, exp])) = arg.cas_op_parts()
            && base.cas_var_name() == Some(var)
            && exp.exact_int().is_some_and(|n| n == 2.into())
        {
            x_sq_coeff = Some(Value::Int(1));
            continue;
        }
        if let Some(("*", inner_args)) = arg.cas_op_parts() {
            let (coeff, rest) = split_off_numeric(inner_args);
            if rest.len() == 1
                && let Some(("^", [base, exp])) = rest[0].cas_op_parts()
                && base.cas_var_name() == Some(var)
                && exp.exact_int().is_some_and(|n| n == 2.into())
            {
                x_sq_coeff = Some(coeff);
                continue;
            }
        }
        if !arg.is_cas_expr() {
            const_term = Some(arg.clone());
            continue;
        }
        return Ok(None);
    }

    let Some(x_sq_coeff) = x_sq_coeff else {
        return Ok(None);
    };
    if !numeric_is_one(&x_sq_coeff) {
        return Ok(None);
    }

    let c = match const_term {
        Some(c) => c,
        None => return Ok(None),
    };

    // 1/(x^2 + c)
    if let Some(a_sq) = try_negate(&c) {
        // 1/(x^2 - a^2), a_sq = a^2
        let Some(a) = sqrt_of_value(&a_sq) else {
            return Ok(None);
        };
        let two_a = cas_mul(vec![Value::Int(2), a.clone()])?;
        let result = cas_mul(vec![
            cas_div(Value::Int(1), two_a)?,
            Value::from_cas_call(
                "ln",
                vec![Value::from_cas_call(
                    "abs",
                    vec![cas_div(
                        cas_sub(Value::from_cas_var(var), a.clone())?,
                        cas_add(vec![Value::from_cas_var(var), a])?,
                    )?],
                )],
            ),
        ])?;
        return Ok(Some(simplify_cas_value(&result)?));
    }

    // 1/(x^2 + a^2)
    let Some(a) = sqrt_of_value(&c) else {
        return Ok(None);
    };
    let result = cas_mul(vec![
        cas_div(Value::Int(1), a.clone())?,
        Value::from_cas_call("arctan", vec![cas_div(Value::from_cas_var(var), a)?]),
    ])?;
    Ok(Some(simplify_cas_value(&result)?))
}

fn try_negate(value: &Value) -> Option<Value> {
    match value {
        Value::Int(n) if *n < 0 => Some(Value::Int(-n)),
        Value::Float(f) if **f < 0.0 => Some(Value::float(-f)),
        Value::BigInt(n) if n.to_i64().is_some_and(|n| n < 0) => {
            Some(Value::from_bigint(-n.as_ref().clone()))
        }
        _ => {
            if let Some(("*", args)) = value.cas_op_parts()
                && args.len() == 2
                && args[0] == Value::Int(-1)
            {
                return Some(args[1].clone());
            }
            None
        }
    }
}

fn sqrt_of_value(value: &Value) -> Option<Value> {
    match value {
        Value::Int(n) => {
            let f = (*n as f64).sqrt();
            if (f - f.round()).abs() < 1e-12 {
                Some(Value::Int(f.round() as i64))
            } else {
                None
            }
        }
        Value::Float(f) => {
            let sqrt = f.sqrt();
            if sqrt.is_finite() {
                Some(Value::float(sqrt))
            } else {
                None
            }
        }
        Value::BigInt(n) => {
            let f = n.to_f64()?.sqrt();
            if (f - f.round()).abs() < 1e-12 {
                Some(Value::Int(f.round() as i64))
            } else {
                Some(Value::float(f))
            }
        }
        _ => None,
    }
}
