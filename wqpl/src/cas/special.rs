use super::limit::{LimitDirection, parse_limit_direction};
use super::{cas_err, infer_single_cas_var};
use crate::value::{Value, WqResult};

pub(crate) type CasNamedArg = (String, Value);

pub(crate) fn ensure_cas_math_expr(construct: &str, value: &Value) -> WqResult<()> {
    if value.is_cas_expr()
        || value.rational_parts().is_some()
        || matches!(
            value,
            Value::Float(_) | Value::Complex(_) | Value::Algebraic(_)
        )
    {
        Ok(())
    } else {
        Err(cas_err(format!(
            "'{construct}' expects a numeric or symbolic expression"
        ))
        .got1(value))
    }
}

pub(crate) struct IntegralSpec {
    pub(crate) expr: Value,
    pub(crate) var: Value,
    pub(crate) bounds: Option<(Value, Value)>,
}

pub(crate) fn parse_integral_spec(args: &[Value]) -> WqResult<IntegralSpec> {
    let (expr, var, bounds) = match args {
        [expr] => {
            ensure_cas_math_expr("integrate", expr)?;
            (
                expr.clone(),
                Value::from_cas_var(infer_single_cas_var(expr).map_err(|_| {
                    cas_err("'integrate' could not infer exactly one symbolic variable")
                })?),
                None,
            )
        }
        [expr, var] => {
            ensure_cas_math_expr("integrate", expr)?;
            (expr.clone(), required_var("integrate", var)?, None)
        }
        [expr, var, lower, upper] => {
            ensure_cas_math_expr("integrate", expr)?;
            (
                expr.clone(),
                required_var("integrate", var)?,
                Some((lower.clone(), upper.clone())),
            )
        }
        _ => {
            return Err(cas_err(
                "'integrate' expects 'integrate[expr]', 'integrate[expr;var]', or 'integrate[expr;var;lower;upper]'",
            ));
        }
    };
    if let Some((lower, upper)) = &bounds {
        ensure_cas_math_expr("integrate", lower)?;
        ensure_cas_math_expr("integrate", upper)?;
    }
    Ok(IntegralSpec { expr, var, bounds })
}

#[derive(Debug)]
pub(crate) struct LimitStep {
    pub(crate) var: Value,
    pub(crate) point: Value,
    pub(crate) direction: Option<LimitDirection>,
}

#[derive(Debug)]
pub(crate) struct LimitSpec {
    pub(crate) expr: Value,
    pub(crate) steps: Vec<LimitStep>,
}

pub(crate) fn parse_limit_spec(args: &[Value], named: &[CasNamedArg]) -> WqResult<LimitSpec> {
    let direction = limit_direction(named)?;
    if args.len() < 2 {
        return Err(cas_err(
            "'limit' expects at least 2 arguments: 'limit[expr;point]'",
        ));
    }

    if let [expr, point] = args {
        ensure_cas_math_expr("limit", expr)?;
        ensure_cas_math_expr("limit", point)?;
        return Ok(LimitSpec {
            expr: expr.clone(),
            steps: vec![LimitStep {
                var: Value::from_cas_var(infer_single_cas_var(expr).map_err(|_| {
                    cas_err("'limit' could not infer exactly one symbolic variable")
                })?),
                point: point.clone(),
                direction,
            }],
        });
    }

    let n = args.len() - 1;
    if !n.is_multiple_of(2) {
        return Err(cas_err(
            "'limit' expects 'limit[expr;point]' or 'limit[expr;var;point]', optionally followed by additional 'var;point' pairs",
        ));
    }

    let n_pairs = n / 2;
    ensure_cas_math_expr("limit", &args[0])?;
    let mut steps = Vec::with_capacity(n_pairs);
    for i in 0..n_pairs {
        let idx = 1 + i * 2;
        ensure_cas_math_expr("limit", &args[idx + 1])?;
        steps.push(LimitStep {
            var: required_var("limit", &args[idx])?,
            point: args[idx + 1].clone(),
            direction: if i == n_pairs - 1 { direction } else { None },
        });
    }
    Ok(LimitSpec {
        expr: args[0].clone(),
        steps,
    })
}

fn required_var(construct: &str, value: &Value) -> WqResult<Value> {
    if value.cas_var_name().is_some() && parse_limit_direction(value).is_none() {
        Ok(value.clone())
    } else {
        Err(cas_err(format!("'{construct}' target must be a symbolic variable")).got1(value))
    }
}

fn limit_direction(named: &[CasNamedArg]) -> WqResult<Option<LimitDirection>> {
    let mut direction = None;
    for (name, value) in named {
        if name != "direction" {
            return Err(cas_err(format!("unknown named argument '{name}'")));
        }
        if direction.is_some() {
            return Err(cas_err("duplicate named argument 'direction'"));
        }
        direction = Some(
            parse_limit_direction(value)
                .ok_or_else(|| cas_err("'limit' direction must be '@s+' or '@s-'"))?,
        );
    }
    Ok(direction)
}
