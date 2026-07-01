use super::limit::parse_limit_direction;
use super::{cas_call_expr, cas_err, ensure_expr_arg, infer_single_cas_var};
use crate::value::cas::CasFunction;
use crate::value::{Value, WqResult};

pub(crate) type CasNamedArg = (String, Value);

fn limit_direction(named: &[CasNamedArg]) -> WqResult<Option<super::limit::LimitDirection>> {
    let mut direction = None;
    for (name, value) in named {
        if name != "d" {
            return Err(cas_err(format!("unknown named argument '{name}'")));
        }
        if direction.is_some() {
            return Err(cas_err("duplicate named argument 'd'"));
        }
        direction = Some(
            parse_limit_direction(value)
                .ok_or_else(|| cas_err("limit direction must be symbolic + or -"))?,
        );
    }
    Ok(direction)
}

fn inferred_limit_var(expr: &Value) -> WqResult<Value> {
    infer_single_cas_var(expr)
        .map(Value::from_cas_var)
        .map_err(|_| cas_err("limit could not infer one target symbol"))
}

fn required_limit_var(value: &Value) -> WqResult<Value> {
    if value.cas_var_name().is_some() && parse_limit_direction(value).is_none() {
        Ok(value.clone())
    } else {
        Err(cas_err("limit target must be a symbolic variable").got1(value))
    }
}

fn cas_limit_expr(args: &[Value], named: &[CasNamedArg]) -> WqResult<Value> {
    let direction = limit_direction(named)?;
    if args.len() < 2 {
        return Err(cas_err("limit expects at least 2 args: expr, point"));
    }

    if let [expr, point] = args {
        return Ok(Value::from_cas_limit(
            expr.clone(),
            inferred_limit_var(expr)?,
            point.clone(),
            direction,
        ));
    }

    let n = args.len() - 1;
    if !n.is_multiple_of(2) {
        return Err(cas_err(
            "limit expects expr;point or expr followed by var;point pairs",
        ));
    }

    let n_pairs = n / 2;
    let mut result = args[0].clone();
    for i in 0..n_pairs {
        let idx = 1 + i * 2;
        let var = required_limit_var(&args[idx])?;
        let point = args[idx + 1].clone();
        let dir = if i == n_pairs - 1 { direction } else { None };
        result = Value::from_cas_limit(result, var, point, dir);
    }
    Ok(result)
}

fn with_named_args(args: &[Value], named: &[CasNamedArg]) -> Vec<Value> {
    let mut out = Vec::with_capacity(args.len() + named.len());
    out.extend(args.iter().cloned());
    out.extend(
        named
            .iter()
            .map(|(name, value)| Value::from_cas_named_arg(name.clone(), value.clone())),
    );
    out
}

pub(crate) fn cas_symbolic_call_expr(
    name: &str,
    args: &[Value],
    named: &[CasNamedArg],
) -> WqResult<Value> {
    if name == "limit" {
        return cas_limit_expr(args, named);
    }
    if name == "root" {
        return super::root::cas_root_expr(args, named);
    }
    let args = with_named_args(args, named);
    if let Some(function) = CasFunction::from_name(name) {
        return cas_call_expr(function, &args);
    }
    for arg in &args {
        ensure_expr_arg(arg, name)?;
    }
    Ok(Value::from_cas_apply(name, args))
}
