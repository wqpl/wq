use super::{
    CasExprContext, CasNamedArg, cas_call_expr, cas_err, close_cas_scope, ensure_cas_math_expr,
    ensure_expr_arg, parse_integral_spec, parse_limit_spec,
};
use crate::value::cas::{CasFunction, CasPredicate};
use crate::value::{Value, WqResult};

pub(crate) fn cas_special_call_name(name: &str) -> bool {
    matches!(
        name,
        "limit"
            | "integrate"
            | "root"
            | "zero"
            | "nonzero"
            | "positive"
            | "negative"
            | "nonnegative"
            | "real"
            | "integer"
    )
}

fn cas_predicate_expr(
    name: &str,
    args: &[Value],
    named: &[CasNamedArg],
) -> WqResult<Option<Value>> {
    let constructor: fn(Value) -> CasPredicate = match name {
        "zero" => CasPredicate::Zero,
        "nonzero" => CasPredicate::NonZero,
        "positive" => CasPredicate::Positive,
        "negative" => CasPredicate::Negative,
        "nonnegative" => CasPredicate::NonNegative,
        "real" => CasPredicate::Real,
        "integer" => CasPredicate::Integer,
        _ => return Ok(None),
    };
    if !named.is_empty() {
        return Err(cas_err(format!("'{name}' does not accept named arguments")));
    }
    let [arg] = args else {
        return Err(cas_err(format!("'{name}' expects exactly 1 argument")));
    };
    let quoted_name = format!("'{name}'");
    ensure_expr_arg(arg, CasExprContext::Builtin(&quoted_name))?;
    ensure_cas_math_expr(name, arg)?;
    Ok(Some(Value::from_cas_predicate(constructor(arg.clone()))))
}

fn cas_limit_expr(args: &[Value], named: &[CasNamedArg]) -> WqResult<Value> {
    let spec = parse_limit_spec(args, named)?;
    let mut result = spec.expr;
    for step in spec.steps {
        let name = step
            .var
            .cas_var_name()
            .expect("validated symbolic limit variable");
        result = Value::from_cas_limit(close_cas_scope(&result, name), step.point, step.direction);
    }
    Ok(result)
}

fn cas_integral_expr(args: &[Value], named: &[CasNamedArg]) -> WqResult<Value> {
    if !named.is_empty() {
        return Err(cas_err("'integrate' does not accept named arguments"));
    }
    let spec = parse_integral_spec(args)?;
    let name = spec
        .var
        .cas_var_name()
        .expect("validated symbolic integration variable");
    Ok(Value::from_cas_integral(
        close_cas_scope(&spec.expr, name),
        spec.bounds,
    ))
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
    if name == "integrate" {
        return cas_integral_expr(args, named);
    }
    if name == "root" {
        return super::root::cas_root_expr(args, named);
    }
    if let Some(predicate) = cas_predicate_expr(name, args, named)? {
        return Ok(predicate);
    }
    if let Some(function) = CasFunction::from_name(name) {
        if !named.is_empty() {
            return Err(cas_err(format!("'{name}' does not accept named arguments")));
        }
        if !function.accepts_arity(args.len()) {
            return Err(cas_err(format!(
                "'{name}' expects {}",
                function.arity_description()
            )));
        }
        return cas_call_expr(function, args);
    }
    let args = with_named_args(args, named);
    for arg in &args {
        ensure_expr_arg(arg, CasExprContext::Application(name))?;
    }
    Ok(Value::from_cas_apply(name, args))
}
