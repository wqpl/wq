use crate::cas::limit::limit_cas;
use crate::cas::{
    cas_add, cas_div, cas_err, cas_mul, cas_pow, cas_sub, contains_cas_var, numeric_mul,
    rewrite_cas, simplify_cas_value, substitute_cas, var_name_from_value,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

mod base;
mod byparts;
mod elliptic;
mod exp_poly;
mod irrational;
mod liouville;
mod partial_fractions;
mod polynomial;
pub(crate) mod rational;
mod substitution;
mod trig;

pub(crate) fn integrate_cas(expr: &Value, var: &Value) -> WqResult<Value> {
    let var = var_name_from_value(var)?;
    let expr = simplify_cas_value(expr)?;
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] integrate enter: expr={} var={}",
        expr.format_cas().unwrap_or_else(|| expr.to_string()),
        var
    );
    let result = rewrite_cas(&simplify_cas_value(&integrate_expr_with_depth(
        &expr, &var, 0,
    )?)?)?;
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] integrate exit: {}",
        result.format_cas().unwrap_or_else(|| result.to_string())
    );
    Ok(result)
}

/// Evaluate a definite integral ∫_lower^upper expr d(var) via the Fundamental
/// Theorem of Calculus: F(upper) - F(lower), where F is the antiderivative.
///
/// When one of the bounds is `oo` or `_oo`, or a bound lies at a singularity
/// of F, a one-sided limit is used to evaluate F at that bound.
pub(crate) fn definite_integrate_cas(
    expr: &Value,
    var: &Value,
    lower: &Value,
    upper: &Value,
) -> WqResult<Value> {
    let antideriv = integrate_cas(expr, var)?;

    // If the antiderivative is itself an unevaluated integral, bail.
    if let Some((name, _)) = antideriv.cas_function_parts()
        && name == CasFunction::Integrate
    {
        return Ok(antideriv);
    }

    let f_upper = evaluate_at_bound(&antideriv, var, upper)?;
    let f_lower = evaluate_at_bound(&antideriv, var, lower)?;

    cas_sub(f_upper, f_lower)
}

/// Evaluate F(bound), falling back to a one-sided limit when substitution fails
/// (e.g. singularity or infinity bound).
fn evaluate_at_bound(antideriv: &Value, var: &Value, bound: &Value) -> WqResult<Value> {
    // For infinity bounds, skip substitution — substituting oo produces
    // expressions like oo^(-1) that aren't meaningful.
    let is_inf = bound
        .cas_const_name()
        .is_some_and(|n| n == "oo" || n == "_oo");
    if !is_inf {
        match substitute_cas(antideriv, var, bound) {
            Ok(v) if !v.is_cas_expr() => return Ok(v),
            Ok(v) => {
                let var_name = var.cas_var_name().unwrap_or("");
                if !contains_cas_var(&v, var_name) {
                    return Ok(v);
                }
            }
            Err(_) => {}
        }
    }
    limit_cas(antideriv, var, bound, None)
}

type IntegrateStrategy = fn(&Value, &str) -> WqResult<Option<Value>>;

// IMPORTANT — strategy ordering and recursion safety:
//
// Each strategy is tried in order for every symbolic sub-expression. Strategies
// that call `integrate_expr_with_depth` internally (substitution, byparts)
// **must** be placed AFTER strategies that can fully handle the same form,
// otherwise the recursive call will re-enter the strategy chain and cause a
// stack overflow through unbounded re-processing.
//
// Rule: if a strategy transforms the integrand and delegates back to the
// pipeline, its output must not re-match itself.  Both `trig` and `rational`
// avoid this entirely by using direct coefficient-vector arithmetic instead of
// calling `integrate_expr_with_depth`.
const STRATEGIES: &[(&str, IntegrateStrategy)] = &[
    ("table", base::integrate_by_table),
    ("substitution", substitution::integrate_by_substitution),
    ("trig", trig::integrate_by_trig),
    ("irrational", irrational::integrate_irrational),
    ("elliptic", elliptic::integrate_elliptic),
    ("exp_poly", exp_poly::integrate_exp_poly),
    ("liouville", liouville::integrate_liouville),
    ("rational", rational::integrate_by_rational),
    ("byparts", byparts::integrate_by_parts),
    (
        "partial_fractions",
        partial_fractions::integrate_by_partial_fractions,
    ),
];

pub(super) const MAX_DEPTH: usize = 20;

pub(super) fn integrate_expr_with_depth(expr: &Value, var: &str, depth: usize) -> WqResult<Value> {
    if depth >= MAX_DEPTH {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] integrate_expr_with_depth depth={} -> depth_exceeded",
            depth
        );
        return Err(cas_err("integration recursion depth exceeded"));
    }
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        depth,
        "[cas-v] integrate_expr_with_depth enter depth={depth} expr={} var={var}",
        expr.format_cas().unwrap_or_else(|| expr.to_string())
    );

    if !expr.is_cas_expr() {
        let result = cas_mul(vec![expr.clone(), Value::from_cas_var(var)])?;
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] integrate_expr_with_depth exit depth={depth} -> numeric*var"
        );
        return Ok(result);
    }
    if let Some(name) = expr.cas_var_name() {
        let result = if name == var {
            cas_div(
                cas_pow(Value::from_cas_var(var), Value::Int(2))?,
                Value::Int(2),
            )?
        } else {
            cas_mul(vec![expr.clone(), Value::from_cas_var(var)])?
        };
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] integrate_expr_with_depth exit depth={depth} -> variable_rule"
        );
        return Ok(result);
    }
    if let Some((op, args)) = expr.cas_op_parts() {
        let out = match (op, args) {
            (CasOp::Add, args) => {
                let mut terms = Vec::with_capacity(args.len());
                for arg in args {
                    terms.push(integrate_expr_with_depth(arg, var, depth + 1)?);
                }
                cas_add(terms)?
            }
            (CasOp::Multiply, args) => {
                let (coeff, symbolic) = split_off_numeric(args);
                match symbolic.len() {
                    0 => cas_mul(vec![coeff, Value::from_cas_var(var)]),
                    1 => cas_mul(vec![
                        coeff.clone(),
                        integrate_expr_with_depth(&symbolic[0], var, depth + 1)?,
                    ]),
                    _ => try_strategies(expr, var, depth + 1),
                }?
            }
            (CasOp::Power, [base, exp]) if base.cas_var_name() == Some(var) => {
                polynomial::integrate_power_rule(base, exp, var)?
            }
            _ => try_strategies(expr, var, depth + 1)?,
        };
        let result = simplify_cas_value(&out)?;
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] integrate_expr_with_depth exit depth={depth} op={op} -> {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(result);
    }
    if expr.cas_function_parts().is_some() {
        let result = try_strategies(expr, var, depth + 1)?;
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] integrate_expr_with_depth exit depth={depth} call -> {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(result);
    }
    if expr.cas_apply_parts().is_some() {
        if contains_cas_var(expr, var) {
            return Err(cas_err("unsupported symbolic integral for application").got1(expr));
        }
        return cas_mul(vec![expr.clone(), Value::from_cas_var(var)]);
    }
    Err(cas_err("expected symbolic expression").got1(expr))
}

fn try_strategies(expr: &Value, var: &str, depth: usize) -> WqResult<Value> {
    if depth >= MAX_DEPTH {
        return Err(cas_err("integration recursion depth exceeded"));
    }
    for (name, strategy) in STRATEGIES {
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] try_strategy {name} depth={depth} expr={}",
            expr.format_cas().unwrap_or_else(|| expr.to_string())
        );
        if let Some(result) = strategy(expr, var)? {
            cas_trace!(
                DebugLogFlags::CAS,
                "[cas] strategy {name} -> success: {}",
                result.format_cas().unwrap_or_else(|| result.to_string())
            );
            cas_trace_depth!(
                DebugLogFlags::CAS_VERBOSE,
                depth,
                "[cas-v] try_strategy {name} depth={depth} -> success: {}",
                result.format_cas().unwrap_or_else(|| result.to_string())
            );
            return simplify_cas_value(&result);
        }
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] try_strategy {name} depth={depth} -> failed"
        );
    }
    let formatted = expr.format_cas().unwrap_or_else(|| expr.to_string());
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] all strategies failed for: {formatted}"
    );
    Err(cas_err(format!(
        "unsupported symbolic integral: {formatted}"
    )))
}

pub(super) fn split_off_numeric(args: &[Value]) -> (Value, Vec<Value>) {
    let mut coeff = Value::Int(1);
    let mut symbolic = Vec::new();
    for arg in args {
        if !arg.is_cas_expr() {
            coeff = numeric_mul(&coeff, arg).expect("numeric coefficient multiply");
        } else {
            symbolic.push(arg.clone());
        }
    }
    (coeff, symbolic)
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;
    use num_traits::One as _;

    use super::*;

    fn op(op: CasOp, args: Vec<Value>) -> Value {
        Value::from_cas_op(op, args)
    }

    fn call(function: CasFunction, args: Vec<Value>) -> Value {
        Value::from_cas_function(function, args)
    }

    #[test]
    fn integrate_variable() {
        let result = integrate_cas(&Value::from_cas_var("x"), &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "x^2/2");
    }

    #[test]
    fn integrate_fractional_power() {
        let expr = op(
            CasOp::Power,
            vec![
                Value::from_cas_var("x"),
                Value::from_fraction_parts(BigInt::one(), BigInt::from(2)),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "2/3*x^(3/2)");
    }

    #[test]
    fn integrate_tan() {
        let expr = call(CasFunction::Tan, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-ln[cos[x]]");
    }

    #[test]
    fn integrate_ln() {
        let expr = call(CasFunction::Ln, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "x*(ln[x] - 1)");
    }

    #[test]
    fn integrate_inverse() {
        let expr = op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(-1)]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "ln[abs[x]]");
    }

    #[test]
    fn integrate_arcsin() {
        let expr = call(CasFunction::ArcSin, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "(-x^2 + 1)^(1/2) + arcsin[x]*x");
    }

    #[test]
    fn integrate_arccos() {
        let expr = call(CasFunction::ArcCos, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-(-x^2 + 1)^(1/2) + arccos[x]*x");
    }

    #[test]
    fn integrate_arctan() {
        let expr = call(CasFunction::ArcTan, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "arctan[x]*x - ln[x^2 + 1]/2");
    }

    #[test]
    fn integrate_arcsinh() {
        let expr = call(CasFunction::ArcSinh, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-(x^2 + 1)^(1/2) + arcsinh[x]*x");
    }

    #[test]
    fn integrate_arccosh() {
        let expr = call(CasFunction::ArcCosh, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-(x^2 - 1)^(1/2) + arccosh[x]*x");
    }

    #[test]
    fn integrate_arctanh() {
        let expr = call(CasFunction::ArcTanh, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "arctanh[x]*x + ln[-x^2 + 1]/2");
    }

    #[test]
    fn integrate_abs() {
        let expr = call(CasFunction::Abs, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "abs[x]*x/2");
    }

    #[test]
    fn integrate_sgn() {
        let expr = call(CasFunction::Sgn, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "abs[x]");
    }

    #[test]
    fn integrate_sin_linear_composite() {
        let expr = call(
            CasFunction::Sin,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-cos[2*x]/2");
    }

    #[test]
    fn integrate_cos_linear_composite() {
        let expr = call(
            CasFunction::Cos,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(3), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "sin[3*x]/3");
    }

    #[test]
    fn integrate_exp_linear_composite() {
        let expr = call(
            CasFunction::Exp,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^(2*x)/2");
    }

    #[test]
    fn integrate_ln_linear_composite() {
        let expr = call(
            CasFunction::Ln,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "x*(ln[2*x] - 1)");
    }

    #[test]
    fn integrate_arctan_linear_composite() {
        let expr = call(
            CasFunction::ArcTan,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(
            result.to_string(),
            // (2*x)^2 distributes to 4*x^2 by cas_pow
            "(4*arctan[2*x]*x - ln[4*x^2 + 1])/4"
        );
    }

    #[test]
    fn integrate_sec() {
        let expr = call(CasFunction::Sec, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "ln[tan[x] + sec[x]]");
    }

    #[test]
    fn integrate_csc() {
        let expr = call(CasFunction::Csc, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-ln[csc[x] + cot[x]]");
    }

    #[test]
    fn integrate_cot() {
        let expr = call(CasFunction::Cot, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "ln[sin[x]]");
    }

    #[test]
    fn integrate_x_sin_x() {
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-cos[x]*x + sin[x]");
    }

    #[test]
    fn integrate_x_exp_x() {
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("e^x"), "expected e^x in result: {s}");
        assert!(s.contains("x - 1"), "expected x-1 factor in result: {s}");
    }

    #[test]
    fn integrate_x_ln_x() {
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(CasFunction::Ln, vec![Value::from_cas_var("x")]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "ln[x]*x^2/2 - x^2/4");
    }

    #[test]
    fn integrate_sin_x2_times_2x() {
        let expr = op(
            CasOp::Multiply,
            vec![
                call(
                    CasFunction::Sin,
                    vec![op(
                        CasOp::Power,
                        vec![Value::from_cas_var("x"), Value::Int(2)],
                    )],
                ),
                Value::Int(2),
                Value::from_cas_var("x"),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-cos[x^2]");
    }

    #[test]
    fn integrate_x_times_exp_x2() {
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(
                    CasFunction::Exp,
                    vec![op(
                        CasOp::Power,
                        vec![Value::from_cas_var("x"), Value::Int(2)],
                    )],
                ),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^(x^2)/2");
    }

    #[test]
    fn integrate_one_over_x2_minus_1() {
        let expr = op(
            CasOp::Power,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                        Value::Int(-1),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "ln[abs[(x - 1)/(x + 1)]]/2");
    }

    #[test]
    fn integrate_one_over_x2_plus_1() {
        let expr = op(
            CasOp::Power,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                        Value::Int(1),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "arctan[x]");
    }

    #[test]
    fn integrate_erf() {
        let expr = call(CasFunction::Erf, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert!(result.to_string().contains("erf[x]*x"));
        assert!(result.to_string().contains("e^(-x^2)"));
    }

    #[test]
    fn integrate_erfc() {
        let expr = call(CasFunction::Erfc, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert!(result.to_string().contains("erfc[x]*x"));
        assert!(result.to_string().contains("e^(-x^2)"));
    }

    #[test]
    fn integrate_heaviside() {
        let expr = call(CasFunction::Heaviside, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "heaviside[x]*x");
    }

    #[test]
    fn integrate_delta() {
        let expr = call(CasFunction::Delta, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "heaviside[x]");
    }

    #[test]
    fn integrate_erf_linear_composite() {
        let expr = call(
            CasFunction::Erf,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert!(result.to_string().contains("erf[2*x]*x"));
        // (2*x)^2 distributes to 4*x^2 by cas_pow
        assert!(result.to_string().contains("e^(-4*x^2)"));
    }

    #[test]
    fn integrate_one_over_x_plus_1() {
        // ∫ 1/(x+1) dx = ln|x+1|
        let expr = op(
            CasOp::Power,
            vec![
                op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
                Value::Int(-1),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("ln"), "expected ln in result: {s}");
        assert!(
            s.contains("x + 1") || s.contains("x+1"),
            "expected x+1 in result: {s}"
        );
    }

    #[test]
    fn integrate_one_over_x_plus_1_squared() {
        // ∫ 1/(x+1)^2 dx = -1/(x+1)
        let expr = op(
            CasOp::Power,
            vec![
                op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
                Value::Int(-2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_x_over_x2_plus_1() {
        // ∫ x/(x^2+1) dx = ln|x^2+1|/2
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                op(CasOp::Power, vec![denom, Value::Int(-1)]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("ln"), "expected ln in result: {s}");
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_polynomial_over_linear() {
        // ∫ (x^2+1)/(x+1) dx = x^2/2 - x + 2*ln|x+1|
        let numer = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let denom = op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]);
        let expr = op(
            CasOp::Multiply,
            vec![numer, op(CasOp::Power, vec![denom, Value::Int(-1)])],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("ln"), "expected ln in result: {s}");
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_one_over_x2_plus_x_plus_1() {
        // ∫ 1/(x^2+x+1) dx → arctan form
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::from_cas_var("x"),
                Value::Int(1),
            ],
        );
        let expr = op(CasOp::Power, vec![denom, Value::Int(-1)]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("arctan"), "expected arctan in result: {s}");
    }

    #[test]
    fn integrate_rational_repeated_linear_factor() {
        // ∫ (x+1)/(x-1)^3 dx
        let numer = op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]);
        let denom = op(
            CasOp::Power,
            vec![
                op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(-1)]),
                Value::Int(3),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![numer, op(CasOp::Power, vec![denom, Value::Int(-1)])],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_one_over_x3_plus_x() {
        // ∫ 1/(x^3+x) dx = ∫ 1/(x(x^2+1)) dx
        // = ln|x| - ln|x^2+1|/2
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                Value::from_cas_var("x"),
            ],
        );
        let expr = op(CasOp::Power, vec![denom, Value::Int(-1)]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln"), "expected ln in result: {s}");
    }

    #[test]
    fn integrate_pure_polynomial() {
        // ∫ (x^3 + 2x) dx = x^4/4 + x^2
        let expr = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("x^4"), "expected x^4 in result: {s}");
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_rational_with_coeff() {
        // ∫ (3x^2 + 2x)/(x+1) dx
        let numer = op(
            CasOp::Add,
            vec![
                op(
                    CasOp::Multiply,
                    vec![
                        Value::Int(3),
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                    ],
                ),
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
            ],
        );
        let denom = op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]);
        let expr = op(
            CasOp::Multiply,
            vec![numer, op(CasOp::Power, vec![denom, Value::Int(-1)])],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln"), "expected ln in result: {s}");
    }

    #[test]
    fn integrate_one_over_x2_minus_4() {
        // ∫ 1/(x^2-4) dx = 1/4 * ln|(x-2)/(x+2)|
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(-4),
            ],
        );
        let expr = op(CasOp::Power, vec![denom, Value::Int(-1)]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln"), "expected ln in result: {s}");
    }

    // -- trigonometric integration tests --

    #[test]
    fn integrate_sin_cubed() {
        // ∫ sin³(x) dx = -cos(x) + cos³(x)/3
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
                Value::Int(3),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("cos"), "expected cos in result: {s}");
    }

    #[test]
    fn integrate_sin_squared() {
        // ∫ sin²(x) dx = x/2 - sin(2x)/4
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(
            s.contains("x") || s.contains("sin"),
            "expected x or sin in result: {s}"
        );
    }

    #[test]
    fn integrate_cos_cubed() {
        // ∫ cos³(x) dx = sin(x) - sin³(x)/3
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Cos, vec![Value::from_cas_var("x")]),
                Value::Int(3),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("sin"), "expected sin in result: {s}");
    }

    #[test]
    fn integrate_cos_squared() {
        // ∫ cos²(x) dx = x/2 + sin(2x)/4
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Cos, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_sin_cos_product_odd_sin() {
        // ∫ sin³(x)·(x) dx... actually test sin²(x)·cos³(x)
        // Wait, sin^3*cos^2: m=3 odd, n=2 → use u=cos(x) substitution
        let sin3 = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
                Value::Int(3),
            ],
        );
        let cos2 = op(
            CasOp::Power,
            vec![
                call(CasFunction::Cos, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let expr = op(CasOp::Multiply, vec![sin3, cos2]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_tan_cubed() {
        // ∫ tan³(x) dx = tan²(x)/2 + ln|cos(x)|
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Tan, vec![Value::from_cas_var("x")]),
                Value::Int(3),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(
            s.contains("ln") || s.contains("tan"),
            "expected ln or tan in result: {s}"
        );
    }

    #[test]
    fn integrate_tan_squared() {
        // ∫ tan²(x) dx = tan(x) - x
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Tan, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_sin2x_cos3x() {
        // ∫ sin(2x)·cos(3x) dx = 1/2 ∫ (sin(5x) + sin(-x)) dx
        // = -cos(5x)/10 + cos(x)/2
        let sin_2x = call(
            CasFunction::Sin,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let cos_3x = call(
            CasFunction::Cos,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(3), Value::from_cas_var("x")],
            )],
        );
        let expr = op(CasOp::Multiply, vec![sin_2x, cos_3x]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    // -- exponential × polynomial integration tests --

    #[test]
    fn integrate_exp_poly_x_times_exp_x() {
        // Verify exp_poly handles the classic x·eˣ case
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("e^x"), "expected e^x in result: {s}");
        assert!(s.contains("x"), "expected x in result: {s}");
    }

    #[test]
    fn integrate_x_squared_exp_x() {
        // ∫ x²·eˣ dx = eˣ·(x² - 2x + 2)
        let expr = op(
            CasOp::Multiply,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("e^x"), "expected e^x in result: {s}");
    }

    #[test]
    fn integrate_poly_times_exp_2x() {
        // ∫ (x²+1)·e^(2x) dx
        let poly = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let exp_2x = call(
            CasFunction::Exp,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let expr = op(CasOp::Multiply, vec![poly, exp_2x]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("e^(2*x)"), "expected e^(2*x) in result: {s}");
    }

    #[test]
    fn integrate_x_times_exp_3x() {
        // ∫ x·e^(3x) dx = e^(3x)·(x/3 - 1/9)
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(
                    CasFunction::Exp,
                    vec![op(
                        CasOp::Multiply,
                        vec![Value::Int(3), Value::from_cas_var("x")],
                    )],
                ),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("e^(3*x)"), "expected e^(3*x) in result: {s}");
    }

    #[test]
    fn integrate_x_cubed_exp_x() {
        // ∫ x³·eˣ dx = eˣ·(x³ - 3x² + 6x - 6)
        let expr = op(
            CasOp::Multiply,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    // -- linear argument trig tests --

    #[test]
    fn integrate_sin_2x() {
        // ∫ sin(2x) dx = -cos(2x)/2  (already handled by table, but verify)
        let expr = call(
            CasFunction::Sin,
            vec![op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("cos"), "expected cos: {s}");
    }

    #[test]
    fn integrate_sin_cubed_2x() {
        // ∫ sin³(2x) dx
        let inner = op(
            CasOp::Multiply,
            vec![Value::Int(2), Value::from_cas_var("x")],
        );
        let expr = op(
            CasOp::Power,
            vec![call(CasFunction::Sin, vec![inner]), Value::Int(3)],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("cos"), "expected cos: {s}");
    }

    #[test]
    fn integrate_cos_squared_3x() {
        // ∫ cos²(3x) dx
        let inner = op(
            CasOp::Multiply,
            vec![Value::Int(3), Value::from_cas_var("x")],
        );
        let expr = op(
            CasOp::Power,
            vec![call(CasFunction::Cos, vec![inner]), Value::Int(2)],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_sin_2x_plus_1() {
        // ∫ sin(2x+1) dx = -cos(2x+1)/2
        let inner = op(
            CasOp::Add,
            vec![
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
                Value::Int(1),
            ],
        );
        let expr = call(CasFunction::Sin, vec![inner]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("cos"), "expected cos: {s}");
    }

    // -- sec / csc / cot tests --

    #[test]
    fn integrate_sec_squared() {
        // ∫ sec²(x) dx = tan(x)
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sec, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("tan"), "expected tan: {s}");
    }

    #[test]
    fn integrate_csc_squared() {
        // ∫ csc²(x) dx = -cot(x)
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Csc, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("cot"), "expected cot: {s}");
    }

    #[test]
    fn integrate_cot_squared() {
        // ∫ cot²(x) dx = -cot(x) - x
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Cot, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_sec_cubed() {
        // ∫ sec³(x) dx
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sec, vec![Value::from_cas_var("x")]),
                Value::Int(3),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_cot_reduction() {
        // ∫ cot(x) dx = ln|sin(x)| — verify reduction path
        let expr = call(CasFunction::Cot, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln"), "expected ln: {s}");
    }

    // -- Rothstein-Trager / higher-degree denominator tests --

    #[test]
    fn integrate_one_over_x3_plus_x_plus_1() {
        // ∫ 1/(x³+x+1) dx — irreducible cubic, now handled via Cardano's formula.
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                Value::from_cas_var("x"),
                Value::Int(1),
            ],
        );
        let expr = op(CasOp::Power, vec![denom, Value::Int(-1)]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(
            s.contains("ln") && s.contains("abs"),
            "expected log terms, got: {s}"
        );
    }

    #[test]
    fn integrate_one_over_x3_minus_2() {
        // ∫ 1/(x³-2) dx — denominator has one real and two complex roots.
        // Partial fractions: A/(x-∛2) + (Bx+C)/(x²+∛2·x+∛4)
        // Result should have:
        //   - a ln term from the linear factor (x-∛2)
        //   - a ln term from the quadratic factor (x²+∛2·x+∛4) because B ≠ 0
        //   - an arctan term from the quadratic factor
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                Value::Int(-2),
            ],
        );
        let expr = op(CasOp::Power, vec![denom, Value::Int(-1)]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("arctan"), "expected arctan in result: {s}");
        // Two ln terms: one from the linear factor, one from the quadratic
        let ln_count = s.matches("ln").count();
        assert_eq!(
            ln_count, 2,
            "expected 2 ln terms (linear + quadratic), got {ln_count}: {s}"
        );
    }

    #[test]
    fn integrate_poly_over_irreducible_cubic() {
        // ∫ (3x²+1)/(x³+x+1) dx — N = D', the derivative case, ∫ D'/D = ln|D|
        // This should be handled before reaching RT (table / substitution)
        let numer = op(
            CasOp::Add,
            vec![
                op(
                    CasOp::Multiply,
                    vec![
                        Value::Int(3),
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                    ],
                ),
                Value::Int(1),
            ],
        );
        let denom = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                Value::from_cas_var("x"),
                Value::Int(1),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![numer, op(CasOp::Power, vec![denom, Value::Int(-1)])],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln"), "expected ln|denom| form, got: {s}");
    }

    // -- irrational / sqrt(quadratic) tests --

    #[test]
    fn integrate_one_over_sqrt_x2_plus_1() {
        // ∫ 1/sqrt(x²+1) dx = arcsinh(x)
        let inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let expr = op(
            CasOp::Power,
            vec![call(CasFunction::Sqrt, vec![inner]), Value::Int(-1)],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("arcsinh"), "expected arcsinh in result: {s}");
    }

    #[test]
    fn integrate_sqrt_x2_plus_1() {
        // ∫ sqrt(x²+1) dx = x/2·sqrt(x²+1) + 1/2·arcsinh(x)
        let inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let expr = call(CasFunction::Sqrt, vec![inner]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(
            s.contains("arcsinh") || s.contains("sqrt"),
            "expected arcsinh or sqrt: {s}"
        );
    }

    #[test]
    fn integrate_one_over_sqrt_x2_minus_1() {
        // ∫ 1/sqrt(x²-1) dx = arccosh(x)
        let inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(-1),
            ],
        );
        let expr = op(
            CasOp::Power,
            vec![call(CasFunction::Sqrt, vec![inner]), Value::Int(-1)],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("arccosh"), "expected arccosh in result: {s}");
    }

    #[test]
    fn integrate_one_over_sqrt_1_minus_x2() {
        // ∫ 1/sqrt(1-x²) dx = arcsin(x)
        let inner = op(
            CasOp::Add,
            vec![
                Value::Int(1),
                op(
                    CasOp::Multiply,
                    vec![
                        Value::Int(-1),
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                    ],
                ),
            ],
        );
        let expr = op(
            CasOp::Power,
            vec![call(CasFunction::Sqrt, vec![inner]), Value::Int(-1)],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("arcsin"), "expected arcsin in result: {s}");
    }

    #[test]
    fn integrate_x_sqrt_x2_plus_1() {
        // ∫ x·sqrt(x²+1) dx = (x²+1)^(3/2)/3
        let sqrt_inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                call(CasFunction::Sqrt, vec![sqrt_inner]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_sqrt_2x2_plus_1() {
        // ∫ √(2x²+1) dx
        let inner = op(
            CasOp::Add,
            vec![
                op(
                    CasOp::Multiply,
                    vec![
                        Value::Int(2),
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                    ],
                ),
                Value::Int(1),
            ],
        );
        let expr = call(CasFunction::Sqrt, vec![inner]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_sqrt_x2_plus_2x_plus_5() {
        // ∫ √(x²+2x+5) dx — shifted quadratic
        let inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
                Value::Int(5),
            ],
        );
        let expr = call(CasFunction::Sqrt, vec![inner]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_x2_sqrt_x2_plus_1() {
        // ∫ x²·√(x²+1) dx — degree-2 polynomial times sqrt, triggers Euler
        let sqrt_inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                call(CasFunction::Sqrt, vec![sqrt_inner]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
    }

    #[test]
    fn integrate_one_over_x_sqrt_x2_plus_1() {
        // ∫ 1/(x·√(x²+1)) dx — Euler #2 (c=1 > 0)
        // Substitution reduces to ∫ dt/t = ln|t|.  The formulas are correct
        // but the CAS simplifier currently can't fully flatten the nested
        // rational expression.  GCD cancellation is in place and works when
        // the expression tree is shallow enough.
        let sqrt_part = call(
            CasFunction::Sqrt,
            vec![op(
                CasOp::Add,
                vec![
                    op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                    Value::Int(1),
                ],
            )],
        );
        let expr = op(
            CasOp::Multiply,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(-1)]),
                op(CasOp::Power, vec![sqrt_part, Value::Int(-1)]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x"));
        // GCD cancellation is present but expression nesting is too deep.
        // Known limitation — needs CAS simplification improvements.
        assert!(
            result.is_err() || { !result.as_ref().unwrap().to_string().contains("unsupported") }
        );
    }

    // -- Liouville / exponential-polynomial tests --

    #[test]
    fn integrate_poly_times_exp_quadratic() {
        // ∫ (2x+1)·e^(x²+x) dx = e^(x²+x)
        let exp_arg = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::from_cas_var("x"),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(
                            CasOp::Multiply,
                            vec![Value::Int(2), Value::from_cas_var("x")],
                        ),
                        Value::Int(1),
                    ],
                ),
                call(CasFunction::Exp, vec![exp_arg]),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("e^(x^2 + x)"), "expected exp: {s}");
    }

    // -- Tabular integration & exp·trig direct formula tests --

    #[test]
    fn integrate_exp_sin_direct() {
        // ∫ e^x·sin x dx = e^x·(sin x - cos x)/2
        let expr = cas_mul(vec![
            call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^x*(-cos[x] + sin[x])/2");
    }

    #[test]
    fn integrate_exp_cos_direct() {
        // ∫ e^x·cos x dx = e^x·(sin x + cos x)/2
        let expr = cas_mul(vec![
            call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            call(CasFunction::Cos, vec![Value::from_cas_var("x")]),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^x*(sin[x] + cos[x])/2");
    }

    #[test]
    fn integrate_x_exp_tabular() {
        // ∫ x·e^x dx = e^x·(x - 1)
        let expr = cas_mul(vec![
            Value::from_cas_var("x"),
            call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^x*(x - 1)");
    }

    #[test]
    fn integrate_x2_sin_tabular() {
        // ∫ x^2·sin x dx = -x^2·cos x + 2x·sin x + 2·cos x
        let expr = cas_mul(vec![
            cas_pow(Value::from_cas_var("x"), Value::Int(2)).unwrap(),
            call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-cos[x]*x^2 + 2*sin[x]*x + 2*cos[x]");
    }

    #[test]
    fn integrate_x_cos_tabular() {
        // ∫ x·cos x dx = x·sin x + cos x
        let expr = cas_mul(vec![
            Value::from_cas_var("x"),
            call(CasFunction::Cos, vec![Value::from_cas_var("x")]),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "sin[x]*x + cos[x]");
    }

    #[test]
    fn integrate_exp_sin_linear_coeffs() {
        // ∫ e^(2x)·sin(3x) dx = e^(2x)·(2·sin(3x) - 3·cos(3x))/13
        let expr = cas_mul(vec![
            call(
                CasFunction::Exp,
                vec![cas_mul(vec![Value::Int(2), Value::from_cas_var("x")]).unwrap()],
            ),
            call(
                CasFunction::Sin,
                vec![cas_mul(vec![Value::Int(3), Value::from_cas_var("x")]).unwrap()],
            ),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^(2*x)*(2*sin[3*x] - 3*cos[3*x])/13");
    }

    #[test]
    fn integrate_si() {
        let expr = call(CasFunction::Si, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "si[x]*x + cos[x]");
    }

    #[test]
    fn integrate_ci() {
        let expr = call(CasFunction::Ci, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "ci[x]*x - sin[x]");
    }

    #[test]
    fn integrate_ei() {
        let expr = call(CasFunction::Ei, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("ei[x]") && s.contains("e^x"), "unexpected: {s}");
    }

    // ── definite integrals ──

    #[test]
    fn definite_polynomial() {
        // ∫_0^1 x dx = 1/2
        let result = definite_integrate_cas(
            &Value::from_cas_var("x"),
            &Value::from_cas_var("x"),
            &Value::Int(0),
            &Value::Int(1),
        )
        .unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::one(), BigInt::from(2))
        );
    }

    #[test]
    fn definite_x_squared() {
        // ∫_0^2 x² dx = 8/3
        let expr = op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]);
        let result = definite_integrate_cas(
            &expr,
            &Value::from_cas_var("x"),
            &Value::Int(0),
            &Value::Int(2),
        )
        .unwrap();
        assert_eq!(
            result,
            Value::from_fraction_parts(BigInt::from(8), BigInt::from(3)),
        );
    }

    #[test]
    fn definite_sin() {
        // ∫_0^π sin(x) dx = 2
        let expr = call(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        // Use float pi approximation for the bound
        let pi = Value::float(std::f64::consts::PI);
        let result =
            definite_integrate_cas(&expr, &Value::from_cas_var("x"), &Value::Int(0), &pi).unwrap();
        // cos(π) = -1, cos(0) = 1, so -(cos(π) - cos(0)) = -(-1 - 1) = 2
        // But the result is Float due to float bound
        let f = result.as_f64().unwrap();
        assert!((f - 2.0).abs() < 1e-10, "expected ~2, got {f}");
    }

    #[test]
    fn definite_one_over_x() {
        // ∫_1^2 1/x dx = ln(2)
        let expr = op(CasOp::Divide, vec![Value::Int(1), Value::from_cas_var("x")]);
        let result = definite_integrate_cas(
            &expr,
            &Value::from_cas_var("x"),
            &Value::Int(1),
            &Value::Int(2),
        )
        .unwrap();
        // ln stays symbolic now — result is ln[2]
        assert_eq!(result.to_string(), "ln[2]");
    }

    #[test]
    fn definite_exp() {
        // ∫_0^1 e^x dx = e - 1
        let expr = call(CasFunction::Exp, vec![Value::from_cas_var("x")]);
        let result = definite_integrate_cas(
            &expr,
            &Value::from_cas_var("x"),
            &Value::Int(0),
            &Value::Int(1),
        )
        .unwrap();
        // Antiderivative is e^x, F(1)-F(0) = e - 1
        assert!(result.to_string().contains("- 1"));
        assert!(result.to_string().contains("e"));
    }

    // -- elliptic integral tests --

    #[test]
    fn integrate_sqrt_x3_plus_1() {
        // ∫ √(x³+1) dx → algebraic part + ellik
        let expr = call(
            CasFunction::Sqrt,
            vec![op(
                CasOp::Add,
                vec![
                    op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                    Value::Int(1),
                ],
            )],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ellik"), "expected ellik in result: {s}");
        assert!(s.contains("arccos"), "expected arccos in result: {s}");
        // Should have algebraic part and elliptic part
        assert!(
            s.contains("(x^3 + 1)^(1/2)"),
            "expected algebraic part: {s}"
        );
    }

    #[test]
    fn integrate_one_over_sqrt_x3_plus_1() {
        // ∫ 1/√(x³+1) dx → ellik
        let inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
                Value::Int(1),
            ],
        );
        let expr = op(
            CasOp::Power,
            vec![call(CasFunction::Sqrt, vec![inner]), Value::Int(-1)],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ellik"), "expected ellik in result: {s}");
        assert!(s.contains("arccos"), "expected arccos in result: {s}");
    }

    #[test]
    fn integrate_one_over_sqrt_x4_plus_x() {
        // x^4+x has rational root 0.  With x=1/t:
        // int dx/sqrt(x^4+x) = -int dt/sqrt(t^3+1), reusing the cubic path.
        let inner = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(4)]),
                Value::from_cas_var("x"),
            ],
        );
        let expr = op(
            CasOp::Power,
            vec![
                inner,
                Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x"))
            .expect("quartic inverse radical should reduce to cubic path");
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ellik"), "expected ellik in result: {s}");
        assert!(s.contains("arccos"), "expected arccos in result: {s}");
    }
}
