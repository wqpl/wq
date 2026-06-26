use super::limit::parse_limit_direction;
use super::{cas_call_expr, cas_err, ensure_expr_arg, infer_single_cas_var};
use crate::value::cas::CasFunction;
use crate::value::{Value, WqResult};

fn cas_limit_expr(args: &[Value]) -> WqResult<Value> {
    if !(2..=4).contains(&args.len()) {
        return Err(cas_err("limit expects 2, 3, or 4 symbolic arguments"));
    }
    let direction = |arg: &Value| {
        parse_limit_direction(arg).ok_or_else(|| cas_err("limit direction must be symbolic + or -"))
    };
    let infer_var = || {
        infer_single_cas_var(&args[0])
            .map(Value::from_cas_var)
            .map_err(|_| cas_err("limit could not infer one target symbol"))
    };

    match args {
        [expr, point] => Ok(Value::from_cas_limit(
            expr.clone(),
            infer_var()?,
            point.clone(),
            None,
        )),
        [expr, var, point] => Ok(Value::from_cas_limit(
            expr.clone(),
            var.clone(),
            point.clone(),
            None,
        )),
        [expr, var, point, dir] => Ok(Value::from_cas_limit(
            expr.clone(),
            var.clone(),
            point.clone(),
            Some(direction(dir)?),
        )),
        _ => unreachable!("limit arity checked"),
    }
}

pub(crate) fn cas_symbolic_call_expr(name: &str, args: &[Value]) -> WqResult<Value> {
    if name == "limit" {
        return cas_limit_expr(args);
    }
    if let Some(function) = CasFunction::from_name(name) {
        return cas_call_expr(function, args);
    }
    for arg in args {
        ensure_expr_arg(arg, name)?;
    }
    Ok(Value::from_cas_apply(name, args.to_vec()))
}
