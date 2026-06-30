use num_bigint::BigInt;

use crate::cas::limit::{LimitDirection, is_singular_substitution_value, limit_cas};
use crate::cas::{
    cas_add, cas_div, cas_err, cas_mul, cas_pow, cas_sub, contains_cas_var, eval_exact_numeric_div,
    eval_numeric_binary, extract_linear_coefficients, numeric_is_negative, numeric_is_zero,
    numeric_mul, poly_degree, poly_evaluate, poly_from_expr, rewrite_cas, simplify_cas_value,
    solve_cas, substitute_cas, var_name_from_value, with_cas_div_cache,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasConst, CasFunction, CasOp};
use crate::value::{Value, WqResult};

mod base;
mod byparts;
mod elliptic;
mod exp_poly;
mod irrational;
mod liouville;
mod polynomial;
pub(crate) mod rational;
mod substitution;
mod trig;

pub(crate) fn integrate_cas(expr: &Value, var: &Value) -> WqResult<Value> {
    with_cas_div_cache(|| {
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
    })
}

/// Evaluate a definite integral from lower to upper of expr d(var) via the
/// Fundamental Theorem of Calculus: F(upper) - F(lower), where F is the
/// antiderivative.
///
/// When one of the bounds is `inf` or `-inf`, or a bound lies at a singularity
/// of F, a one-sided limit is used to evaluate F at that bound.
pub(crate) fn definite_integrate_cas(
    expr: &Value,
    var: &Value,
    lower: &Value,
    upper: &Value,
) -> WqResult<Value> {
    let expr = simplify_cas_value(expr)?;
    let antideriv = integrate_cas(&expr, var)?;

    // If the antiderivative is itself an unevaluated integral, bail.
    if let Some((name, _)) = antideriv.cas_function_parts()
        && name == CasFunction::Integrate
    {
        return Ok(antideriv);
    }

    evaluate_definite_segments(&expr, &antideriv, var, lower, upper)
}

fn evaluate_definite_segments(
    expr: &Value,
    antideriv: &Value,
    var: &Value,
    lower: &Value,
    upper: &Value,
) -> WqResult<Value> {
    let Some(lower_key) = bound_order_key(lower) else {
        return evaluate_segment(antideriv, var, lower, upper);
    };
    let Some(upper_key) = bound_order_key(upper) else {
        return evaluate_segment(antideriv, var, lower, upper);
    };
    if lower_key == upper_key {
        return Ok(Value::Int(0));
    }

    let forward = lower_key < upper_key;
    let (start, end) = if forward {
        (lower.clone(), upper.clone())
    } else {
        (upper.clone(), lower.clone())
    };

    let mut bounds = Vec::new();
    bounds.push(start);
    bounds.extend(singularity_split_points(
        expr,
        var,
        lower_key.min(upper_key),
        lower_key.max(upper_key),
    )?);
    bounds.push(end);

    let mut segments = Vec::with_capacity(bounds.len().saturating_sub(1));
    for pair in bounds.windows(2) {
        segments.push(evaluate_segment(antideriv, var, &pair[0], &pair[1])?);
    }

    let total = combine_segment_values(segments)?;
    if forward {
        Ok(total)
    } else {
        negate_definite_value(total)
    }
}

fn evaluate_segment(
    antideriv: &Value,
    var: &Value,
    lower: &Value,
    upper: &Value,
) -> WqResult<Value> {
    let f_upper = evaluate_at_bound(antideriv, var, upper, endpoint_direction(false, upper))?;
    let f_lower = evaluate_at_bound(antideriv, var, lower, endpoint_direction(true, lower))?;
    if let Some(value) = subtract_endpoint_limits(&f_upper, &f_lower) {
        return Ok(value);
    }
    cas_sub(f_upper, f_lower)
}

fn subtract_endpoint_limits(upper: &Value, lower: &Value) -> Option<Value> {
    match (limit_value_sign(upper), limit_value_sign(lower)) {
        (Some(None), _) | (_, Some(None)) => Some(Value::from_cas_const(CasConst::Undefined)),
        (Some(Some(upper_sign)), Some(Some(lower_sign))) if upper_sign == lower_sign => {
            Some(Value::from_cas_const(CasConst::Undefined))
        }
        (Some(Some(upper_sign)), Some(Some(lower_sign))) => {
            Some(infinite_value(upper_sign - lower_sign))
        }
        (Some(Some(upper_sign)), None) => Some(infinite_value(upper_sign)),
        (None, Some(Some(lower_sign))) => Some(infinite_value(-lower_sign)),
        (None, None) => None,
    }
}

fn limit_value_sign(value: &Value) -> Option<Option<i32>> {
    match value.cas_const() {
        Some(CasConst::Infinity) => Some(Some(1)),
        Some(CasConst::NegInfinity) => Some(Some(-1)),
        Some(CasConst::Undefined) => Some(None),
        _ => None,
    }
}

fn endpoint_direction(is_lower: bool, bound: &Value) -> Option<LimitDirection> {
    if bound_order_key(bound).is_some_and(f64::is_finite) {
        Some(if is_lower {
            LimitDirection::Right
        } else {
            LimitDirection::Left
        })
    } else {
        None
    }
}

/// Evaluate F(bound), falling back to a one-sided limit when substitution fails
/// (e.g. singularity or infinity bound).
fn evaluate_at_bound(
    antideriv: &Value,
    var: &Value,
    bound: &Value,
    direction: Option<LimitDirection>,
) -> WqResult<Value> {
    // For infinity bounds, skip substitution.
    // Substituting inf produces expressions like inf^(-1) that aren't meaningful.
    let is_inf = matches!(
        bound.cas_const(),
        Some(CasConst::Infinity | CasConst::NegInfinity)
    );
    if !is_inf {
        match substitute_cas(antideriv, var, bound) {
            Ok(v) if !v.is_cas_expr() => return Ok(v),
            Ok(v) => {
                let var_name = var.cas_var_name().unwrap_or("");
                if !contains_cas_var(&v, var_name) && !is_singular_substitution_value(&v) {
                    return Ok(v);
                }
            }
            Err(_) => {}
        }
    }
    limit_cas(antideriv, var, bound, direction)
}

fn bound_order_key(bound: &Value) -> Option<f64> {
    match bound.cas_const() {
        Some(CasConst::Infinity) => Some(f64::INFINITY),
        Some(CasConst::NegInfinity) => Some(f64::NEG_INFINITY),
        _ => bound.as_f64().filter(|value| !value.is_nan()),
    }
}

fn singularity_split_points(
    expr: &Value,
    var: &Value,
    lower_key: f64,
    upper_key: f64,
) -> WqResult<Vec<Value>> {
    let var_name = var_name_from_value(var)?;
    let mut candidates = Vec::new();
    collect_denominator_candidates(expr, &var_name, &mut candidates);

    let mut roots = Vec::new();
    for candidate in candidates {
        let Ok(solved) = solve_cas(&candidate, var) else {
            continue;
        };
        let Value::List(items) = solved else {
            continue;
        };
        for root in items.iter() {
            let Some(key) = root.as_f64() else {
                continue;
            };
            if key.is_finite() && lower_key < key && key < upper_key {
                roots.push((key, root.clone()));
            }
        }
    }

    roots.sort_by(|left, right| left.0.total_cmp(&right.0));
    roots.dedup_by(|left, right| root_keys_equal(left.0, right.0));
    Ok(roots.into_iter().map(|(_, root)| root).collect())
}

fn root_keys_equal(left: f64, right: f64) -> bool {
    let scale = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= scale * 1e-10
}

fn collect_denominator_candidates(expr: &Value, var_name: &str, out: &mut Vec<Value>) {
    if !contains_cas_var(expr, var_name) {
        return;
    }

    if let Some((op, args)) = expr.cas_op_parts() {
        match (op, args) {
            (CasOp::Divide, [num, den]) => {
                collect_denominator_zero_candidates(den, var_name, out);
                collect_denominator_candidates(num, var_name, out);
                collect_denominator_candidates(den, var_name, out);
            }
            (CasOp::Power, [base, exp]) if numeric_is_negative(exp) => {
                collect_denominator_zero_candidates(base, var_name, out);
                collect_denominator_candidates(base, var_name, out);
            }
            _ => {
                for arg in args {
                    collect_denominator_candidates(arg, var_name, out);
                }
            }
        }
    } else if let Some((_, args)) = expr.cas_function_parts() {
        for arg in args {
            collect_denominator_candidates(arg, var_name, out);
        }
    } else if let Some((_, args)) = expr.cas_apply_parts() {
        for arg in args {
            collect_denominator_candidates(arg, var_name, out);
        }
    } else if let Some((_, value)) = expr.cas_named_arg_parts() {
        collect_denominator_candidates(value, var_name, out);
    } else if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        collect_denominator_candidates(lhs, var_name, out);
        collect_denominator_candidates(rhs, var_name, out);
    }
}

fn collect_denominator_zero_candidates(expr: &Value, var_name: &str, out: &mut Vec<Value>) {
    if !contains_cas_var(expr, var_name) {
        return;
    }

    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        for arg in args {
            collect_denominator_zero_candidates(arg, var_name, out);
        }
    } else if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && !numeric_is_negative(exp)
        && !numeric_is_zero(exp)
    {
        collect_denominator_zero_candidates(base, var_name, out);
    } else {
        out.push(expr.clone());
    }
}

fn combine_segment_values(segments: Vec<Value>) -> WqResult<Value> {
    let mut finite_terms = Vec::new();
    let mut infinite_sign = None;

    for segment in segments {
        match segment.cas_const() {
            Some(CasConst::Undefined) => return Ok(Value::from_cas_const(CasConst::Undefined)),
            Some(CasConst::Infinity) => {
                if !merge_infinite_sign(&mut infinite_sign, 1) {
                    return Ok(Value::from_cas_const(CasConst::Undefined));
                }
            }
            Some(CasConst::NegInfinity) => {
                if !merge_infinite_sign(&mut infinite_sign, -1) {
                    return Ok(Value::from_cas_const(CasConst::Undefined));
                }
            }
            _ => finite_terms.push(segment),
        }
    }

    if let Some(sign) = infinite_sign {
        return Ok(infinite_value(sign));
    }

    match finite_terms.len() {
        0 => Ok(Value::Int(0)),
        1 => Ok(finite_terms.into_iter().next().expect("one finite term")),
        _ => cas_add(finite_terms),
    }
}

fn merge_infinite_sign(current: &mut Option<i32>, sign: i32) -> bool {
    match current {
        Some(existing) if *existing != sign => false,
        Some(_) => true,
        None => {
            *current = Some(sign);
            true
        }
    }
}

fn infinite_value(sign: i32) -> Value {
    if sign >= 0 {
        Value::from_cas_const(CasConst::Infinity)
    } else {
        Value::from_cas_const(CasConst::NegInfinity)
    }
}

fn negate_definite_value(value: Value) -> WqResult<Value> {
    match value.cas_const() {
        Some(CasConst::Infinity) => Ok(Value::from_cas_const(CasConst::NegInfinity)),
        Some(CasConst::NegInfinity) => Ok(Value::from_cas_const(CasConst::Infinity)),
        _ => cas_sub(Value::Int(0), value),
    }
}

type IntegrateStrategy = fn(&Value, &str) -> WqResult<Option<Value>>;

// # Strategy ordering and recursion safety
//
// Each strategy is tried in order for every symbolic sub-expression. Strategies
// that call `integrate_expr_with_depth` internally (substitution, byparts)
// must be placed after strategies that can fully handle the same form,
// otherwise the recursive call will re-enter the strategy chain and cause a
// stack overflow through unbounded re-processing.
//
// If a strategy transforms the integrand and delegates back to the
// pipeline, its output must not re-match itself.  Both `trig` and `rational`
// avoid this entirely by using direct coefficient-vector arithmetic instead of
// calling `integrate_expr_with_depth`.
const STRATEGIES: &[(&str, IntegrateStrategy)] = &[
    ("table", base::integrate_by_table),
    ("abs_poly", integrate_abs_polynomial),
    ("abs_affine_factor", integrate_abs_affine_factor),
    ("substitution", substitution::integrate_by_substitution),
    ("trig", trig::integrate_by_trig),
    ("irrational", irrational::integrate_irrational),
    ("elliptic", elliptic::integrate_elliptic),
    ("exp_poly", exp_poly::integrate_exp_poly),
    ("liouville", liouville::integrate_liouville),
    ("rational", rational::integrate_by_rational),
    ("byparts", byparts::integrate_by_parts),
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
    if !contains_cas_var(expr, var) {
        let result = cas_mul(vec![expr.clone(), Value::from_cas_var(var)])?;
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] integrate_expr_with_depth exit depth={depth} -> constant_wrt_var"
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
                let (coeff, symbolic) = split_off_constant_factors(args, var)?;
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

fn integrate_abs_polynomial(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some((CasFunction::Abs, [arg])) = expr.cas_function_parts() else {
        return Ok(None);
    };
    let coeffs = match poly_from_expr(arg, var) {
        Ok(coeffs) if poly_degree(&coeffs) >= 1 => coeffs,
        _ => return Ok(None),
    };

    if poly_degree(&coeffs) == 1 {
        return Ok(None);
    }

    let antideriv = polynomial_antiderivative_expr(&coeffs, var)?;
    let roots = real_roots_of_polynomial(&coeffs, arg, var)?;
    let signs = polynomial_interval_signs(&coeffs, &roots)?;
    if signs.is_empty() {
        return Ok(None);
    }

    let mut terms = vec![signed_expr(antideriv.clone(), signs[0])?];
    let var_value = Value::from_cas_var(var);
    for (idx, (_, root)) in roots.iter().enumerate() {
        let left_sign = signs[idx];
        let right_sign = signs[idx + 1];
        if left_sign == right_sign {
            continue;
        }
        let root_antideriv = simplify_cas_value(&substitute_cas(&antideriv, &var_value, root)?)?;
        let shifted_antideriv = cas_sub(antideriv.clone(), root_antideriv)?;
        let step_arg = cas_sub(var_value.clone(), root.clone())?;
        let step = Value::from_cas_function(CasFunction::Heaviside, vec![step_arg]);
        terms.push(cas_mul(vec![
            Value::Int(i64::from(right_sign - left_sign)),
            step,
            shifted_antideriv,
        ])?);
    }

    Ok(Some(simplify_cas_value(&cas_add(terms)?)?))
}

fn integrate_abs_affine_factor(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some(factors) = expr.cas_op_args(CasOp::Multiply) else {
        return Ok(None);
    };

    let mut abs_match = None;
    for (idx, factor) in factors.iter().enumerate() {
        let Some((CasFunction::Abs, [arg])) = factor.cas_function_parts() else {
            continue;
        };
        if abs_match.is_some() {
            return Ok(None);
        }
        let Some((root, root_key, coeff_sign)) = linear_root_and_sign(arg, var)? else {
            return Ok(None);
        };
        abs_match = Some((idx, arg.clone(), root, root_key, coeff_sign));
    }

    let Some((abs_idx, abs_arg, root, root_key, coeff_sign)) = abs_match else {
        return Ok(None);
    };

    let mut signed_factors = Vec::with_capacity(factors.len());
    for (idx, factor) in factors.iter().enumerate() {
        if idx == abs_idx {
            signed_factors.push(abs_arg.clone());
        } else {
            signed_factors.push(factor.clone());
        }
    }
    let signed_integrand = cas_mul(signed_factors)?;
    let Ok(antideriv) = integrate_expr_with_depth(&signed_integrand, var, 0) else {
        return Ok(None);
    };

    if let Some((lower, upper)) = principal_sqrt_linear_domain(factors, var)?
        && !root_inside_domain(root_key, lower, upper)
    {
        let Some(sample) = sample_in_domain(lower, upper) else {
            return Ok(None);
        };
        let Some(sign) = linear_sign_from_root(coeff_sign, root_key, sample) else {
            return Ok(None);
        };
        return Ok(Some(simplify_cas_value(&signed_expr(antideriv, sign)?)?));
    }

    let left_sign = -coeff_sign;
    let right_sign = coeff_sign;
    let var_value = Value::from_cas_var(var);
    let root_antideriv = simplify_cas_value(&substitute_cas(&antideriv, &var_value, &root)?)?;
    let shifted_antideriv = cas_sub(antideriv.clone(), root_antideriv)?;
    let step_arg = cas_sub(var_value, root)?;
    let step = Value::from_cas_function(CasFunction::Heaviside, vec![step_arg]);
    let result = cas_add(vec![
        signed_expr(antideriv, left_sign)?,
        cas_mul(vec![
            Value::Int(i64::from(right_sign - left_sign)),
            step,
            shifted_antideriv,
        ])?,
    ])?;
    Ok(Some(simplify_cas_value(&result)?))
}

fn linear_root_and_sign(expr: &Value, var: &str) -> WqResult<Option<(Value, f64, i32)>> {
    let Some((a, b)) = extract_linear_coefficients(expr, var) else {
        return Ok(None);
    };
    let Some(coeff_sign) = numeric_sign_i32(&a) else {
        return Ok(None);
    };
    if coeff_sign == 0 {
        return Ok(None);
    }
    let neg_b = eval_numeric_binary("*", &b, &Value::Int(-1))?;
    let root = eval_exact_numeric_div(&neg_b, &a)?;
    let Some(root_key) = root.as_f64() else {
        return Ok(None);
    };
    if !root_key.is_finite() {
        return Ok(None);
    }
    Ok(Some((root, root_key, coeff_sign)))
}

fn principal_sqrt_linear_domain(
    factors: &[Value],
    var: &str,
) -> WqResult<Option<(Option<f64>, Option<f64>)>> {
    let mut lower: Option<f64> = None;
    let mut upper: Option<f64> = None;
    let mut found = false;

    for factor in factors {
        let base = if let Some((CasOp::Power, [base, exp])) = factor.cas_op_parts() {
            if !exp.exact_half() {
                continue;
            }
            base
        } else if let Some((CasFunction::Sqrt, [base])) = factor.cas_function_parts() {
            base
        } else {
            continue;
        };
        let Some((_, root_key, coeff_sign)) = linear_root_and_sign(base, var)? else {
            continue;
        };
        found = true;
        if coeff_sign > 0 {
            lower = Some(lower.map_or(root_key, |current| current.max(root_key)));
        } else {
            upper = Some(upper.map_or(root_key, |current| current.min(root_key)));
        }
    }

    if found {
        Ok(Some((lower, upper)))
    } else {
        Ok(None)
    }
}

fn root_inside_domain(root: f64, lower: Option<f64>, upper: Option<f64>) -> bool {
    lower.is_none_or(|value| root > value) && upper.is_none_or(|value| root < value)
}

fn sample_in_domain(lower: Option<f64>, upper: Option<f64>) -> Option<f64> {
    match (lower, upper) {
        (Some(l), Some(u)) if l < u => Some((l + u) / 2.0),
        (Some(l), None) => Some(l + l.abs().max(1.0)),
        (None, Some(u)) => Some(u - u.abs().max(1.0)),
        (None, None) => Some(0.0),
        _ => None,
    }
}

fn linear_sign_from_root(coeff_sign: i32, root: f64, sample: f64) -> Option<i32> {
    if sample > root {
        Some(coeff_sign)
    } else if sample < root {
        Some(-coeff_sign)
    } else {
        None
    }
}

fn polynomial_antiderivative_expr(coeffs: &[Value], var: &str) -> WqResult<Value> {
    let mut terms = Vec::new();
    for (degree, coeff) in coeffs.iter().enumerate() {
        if numeric_is_zero(coeff) {
            continue;
        }
        let new_degree = degree + 1;
        let divided = eval_exact_numeric_div(coeff, &Value::from_bigint(BigInt::from(new_degree)))?;
        let monomial = if new_degree == 1 {
            Value::from_cas_var(var)
        } else {
            let exp = i64::try_from(new_degree).expect("polynomial degree should fit in i64");
            cas_pow(Value::from_cas_var(var), Value::Int(exp))?
        };
        terms.push(cas_mul(vec![divided, monomial])?);
    }
    cas_add(terms)
}

fn real_roots_of_polynomial(
    coeffs: &[Value],
    expr: &Value,
    var: &str,
) -> WqResult<Vec<(f64, Value)>> {
    if let Some(roots) = exact_rational_roots(coeffs)? {
        return Ok(sort_real_roots(roots));
    }

    let solved = solve_cas(expr, &Value::from_cas_var(var))?;
    let Value::List(items) = solved else {
        return Ok(Vec::new());
    };
    Ok(sort_real_roots(items.iter().cloned().collect()))
}

fn sort_real_roots(values: Vec<Value>) -> Vec<(f64, Value)> {
    let mut roots = Vec::new();
    for root in values {
        let Some(key) = root.as_f64() else {
            continue;
        };
        if key.is_finite() {
            roots.push((key, root));
        }
    }
    roots.sort_by(|left, right| left.0.total_cmp(&right.0));
    roots.dedup_by(|left, right| root_keys_equal(left.0, right.0));
    roots
}

fn exact_rational_roots(coeffs: &[Value]) -> WqResult<Option<Vec<Value>>> {
    match poly_degree(coeffs) {
        0 => Ok(Some(Vec::new())),
        1 => {
            let root = eval_exact_numeric_div(
                &eval_numeric_binary("*", &coeffs[0], &Value::Int(-1))?,
                &coeffs[1],
            )?;
            Ok(Some(vec![root]))
        }
        2 => exact_quadratic_rational_roots(coeffs),
        _ => Ok(None),
    }
}

fn exact_quadratic_rational_roots(coeffs: &[Value]) -> WqResult<Option<Vec<Value>>> {
    let a = coeffs.get(2).cloned().unwrap_or(Value::Int(0));
    let b = coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    let c = coeffs.first().cloned().unwrap_or(Value::Int(0));

    let b_sq = eval_numeric_binary("*", &b, &b)?;
    let four_ac = eval_numeric_binary("*", &Value::Int(4), &eval_numeric_binary("*", &a, &c)?)?;
    let disc = eval_numeric_binary("-", &b_sq, &four_ac)?;
    let Some((disc_num, disc_den)) = disc.rational_parts() else {
        return Ok(None);
    };
    if disc_num < BigInt::from(0) {
        return Ok(Some(Vec::new()));
    }

    let sqrt_num = disc_num.sqrt();
    let sqrt_den = disc_den.sqrt();
    if &sqrt_num * &sqrt_num != disc_num || &sqrt_den * &sqrt_den != disc_den {
        return Ok(None);
    }

    let sqrt_disc = Value::from_fraction_parts(sqrt_num, sqrt_den);
    let neg_b = eval_numeric_binary("*", &b, &Value::Int(-1))?;
    let two_a = eval_numeric_binary("*", &Value::Int(2), &a)?;
    let root1 = eval_exact_numeric_div(&eval_numeric_binary("+", &neg_b, &sqrt_disc)?, &two_a)?;
    let root2 = eval_exact_numeric_div(&eval_numeric_binary("-", &neg_b, &sqrt_disc)?, &two_a)?;
    Ok(Some(vec![root1, root2]))
}

fn polynomial_interval_signs(coeffs: &[Value], roots: &[(f64, Value)]) -> WqResult<Vec<i32>> {
    if roots.is_empty() {
        return polynomial_sign_at(coeffs, 0.0).map(|sign| sign.into_iter().collect());
    }

    let mut signs = Vec::with_capacity(roots.len() + 1);
    for idx in 0..=roots.len() {
        let sample = if idx == 0 {
            roots[0].0 - roots[0].0.abs().max(1.0)
        } else if idx == roots.len() {
            let last = roots[roots.len() - 1].0;
            last + last.abs().max(1.0)
        } else {
            (roots[idx - 1].0 + roots[idx].0) / 2.0
        };
        let Some(sign) = polynomial_sign_at(coeffs, sample)? else {
            return Ok(Vec::new());
        };
        signs.push(sign);
    }
    Ok(signs)
}

fn polynomial_sign_at(coeffs: &[Value], sample: f64) -> WqResult<Option<i32>> {
    let value = poly_evaluate(coeffs, &Value::float(sample))?;
    if numeric_is_zero(&value) {
        Ok(None)
    } else if numeric_is_negative(&value) {
        Ok(Some(-1))
    } else if value.as_f64().is_some_and(|f| f > 0.0) || value.rational_parts().is_some() {
        Ok(Some(1))
    } else {
        Ok(None)
    }
}

fn numeric_sign_i32(value: &Value) -> Option<i32> {
    if numeric_is_zero(value) {
        Some(0)
    } else if numeric_is_negative(value) {
        Some(-1)
    } else if value.as_f64().is_some_and(|f| f > 0.0) || value.rational_parts().is_some() {
        Some(1)
    } else {
        None
    }
}

fn signed_expr(expr: Value, sign: i32) -> WqResult<Value> {
    if sign >= 0 {
        Ok(expr)
    } else {
        cas_mul(vec![Value::Int(-1), expr])
    }
}

pub(super) fn split_off_constant_factors(
    args: &[Value],
    var: &str,
) -> WqResult<(Value, Vec<Value>)> {
    let mut coeff_factors = Vec::new();
    let mut symbolic = Vec::new();
    for arg in args {
        if !arg.is_cas_expr() || !contains_cas_var(arg, var) {
            coeff_factors.push(arg.clone());
        } else {
            symbolic.push(arg.clone());
        }
    }
    let coeff = match coeff_factors.len() {
        0 => Value::Int(1),
        1 => coeff_factors
            .into_iter()
            .next()
            .expect("single constant factor"),
        _ => cas_mul(coeff_factors)?,
    };
    Ok((coeff, symbolic))
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
    fn integrate_one_over_x2_plus_symbolic_square() {
        let expr = op(
            CasOp::Power,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                        op(CasOp::Power, vec![Value::from_cas_var("a"), Value::Int(2)]),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x"))
            .expect("symbolic square denominator should integrate");
        assert_eq!(result.to_string(), "arctan[x/a]/a");
    }

    #[test]
    fn integrate_one_over_x2_minus_symbolic_square() {
        let a_sq = op(CasOp::Power, vec![Value::from_cas_var("a"), Value::Int(2)]);
        let expr = op(
            CasOp::Power,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                        op(CasOp::Multiply, vec![Value::Int(-1), a_sq]),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x"))
            .expect("symbolic square difference should integrate");
        assert_eq!(result.to_string(), "ln[abs[(x - a)/(x + a)]]/2/a");
    }

    #[test]
    fn integrate_unknown_factor_over_quadratic_is_unsupported() {
        let reciprocal = op(
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
        let expr = op(
            CasOp::Multiply,
            vec![
                Value::from_cas_apply("f", vec![Value::from_cas_var("x")]),
                reciprocal,
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x"));
        assert!(
            result.is_err(),
            "expected unsupported integral, got {result:?}"
        );
    }

    #[test]
    fn integrate_trig_over_quadratic_is_unsupported() {
        let reciprocal = op(
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
        let expr = op(
            CasOp::Multiply,
            vec![
                call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
                reciprocal,
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x"));
        assert!(
            result.is_err(),
            "expected unsupported integral, got {result:?}"
        );
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
        // int 1/(x+1) dx = ln|x+1|
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
        // int 1/(x+1)^2 dx = -1/(x+1)
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
        // int x/(x^2+1) dx = ln|x^2+1|/2
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
        // int (x^2+1)/(x+1) dx = x^2/2 - x + 2*ln|x+1|
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
        // int 1/(x^2+x+1) dx -> arctan form
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
        // int (x+1)/(x-1)^3 dx
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
        // int 1/(x^3+x) dx = int 1/(x(x^2+1)) dx
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
        // int (x^3 + 2x) dx = x^4/4 + x^2
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
        // int (3x^2 + 2x)/(x+1) dx
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
        // int 1/(x^2-4) dx = 1/4 * ln|(x-2)/(x+2)|
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
        // int sin^3(x) dx = -cos(x) + cos^3(x)/3
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
        // int sin^2(x) dx = x/2 - sin(2x)/4
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
        // int cos^3(x) dx = sin(x) - sin^3(x)/3
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
        // int cos^2(x) dx = x/2 + sin(2x)/4
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
        // int sin^3(x)*(x) dx... actually test sin^2(x)*cos^3(x)
        // Wait, sin^3*cos^2: m=3 odd, n=2 -> use u=cos(x) substitution
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
        // int tan^3(x) dx = tan^2(x)/2 + ln|cos(x)|
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
        // int tan^2(x) dx = tan(x) - x
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
        // int sin(2x)*cos(3x) dx = 1/2 int (sin(5x) + sin(-x)) dx
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

    // -- exponential * polynomial integration tests --

    #[test]
    fn integrate_exp_poly_x_times_exp_x() {
        // Verify exp_poly handles the classic x*e^x case
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
        // int x^2*e^x dx = e^x*(x^2 - 2x + 2)
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
        // int (x^2+1)*e^(2x) dx
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
        // int x*e^(3x) dx = e^(3x)*(x/3 - 1/9)
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
        // int x^3*e^x dx = e^x*(x^3 - 3x^2 + 6x - 6)
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
        // int sin(2x) dx = -cos(2x)/2  (already handled by table, but verify)
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
        // int sin^3(2x) dx
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
        // int cos^2(3x) dx
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
        // int sin(2x+1) dx = -cos(2x+1)/2
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
        // int sec^2(x) dx = tan(x)
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
        // int csc^2(x) dx = -cot(x)
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
        // int cot^2(x) dx = -cot(x) - x
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
        // int sec^3(x) dx
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
        // int cot(x) dx = ln|sin(x)| -- verify reduction path
        let expr = call(CasFunction::Cot, vec![Value::from_cas_var("x")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln"), "expected ln: {s}");
    }

    // -- Rothstein-Trager / higher-degree denominator tests --

    #[test]
    fn integrate_one_over_x3_plus_x_plus_1() {
        // int 1/(x^3+x+1) dx -- irreducible cubic, now handled via Cardano's formula.
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
        // int 1/(x^3-2) dx -- denominator has one real and two complex roots.
        // Partial fractions: A/(x-cbrt(2)) + (Bx+C)/(x^2+cbrt(2)*x+cbrt(4))
        // Result should have:
        //   - a ln term from the linear factor (x-cbrt(2))
        //   - a ln term from the quadratic factor (x^2+cbrt(2)*x+cbrt(4)) because B !=
        //     0
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
        // int (3x^2+1)/(x^3+x+1) dx -- N = D', the derivative case, int D'/D = ln|D|
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
        // int 1/sqrt(x^2+1) dx = arcsinh(x)
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
        // int sqrt(x^2+1) dx = x/2*sqrt(x^2+1) + 1/2*arcsinh(x)
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
        // int 1/sqrt(x^2-1) dx = arccosh(x)
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
        // int 1/sqrt(1-x^2) dx = arcsin(x)
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
        // int x*sqrt(x^2+1) dx = (x^2+1)^(3/2)/3
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
        // int sqrt(2x^2+1) dx
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
        // int sqrt(x^2+2x+5) dx -- shifted quadratic
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
        // int x^2*sqrt(x^2+1) dx -- degree-2 polynomial times sqrt, triggers Euler
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
        // int 1/(x*sqrt(x^2+1)) dx -- Euler #2 (c=1 > 0)
        // Substitution reduces to a rational integral in t, then back-substitutes
        // the logarithmic antiderivative.
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
        let result =
            integrate_cas(&expr, &Value::from_cas_var("x")).expect("Euler integral should succeed");
        let s = result.to_string();
        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ln[abs["), "expected logarithmic result: {s}");
        assert!(
            s.contains("(x^2 + 1)^(1/2)"),
            "expected sqrt(x^2+1) back-substitution: {s}"
        );
    }

    // -- Liouville / exponential-polynomial tests --

    #[test]
    fn integrate_poly_times_exp_quadratic() {
        // int (2x+1)*e^(x^2+x) dx = e^(x^2+x)
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

    // -- Tabular integration & exp*trig direct formula tests --

    #[test]
    fn integrate_exp_sin_direct() {
        // int e^x*sin x dx = e^x*(sin x - cos x)/2
        let expr = cas_mul(vec![
            call(CasFunction::Exp, vec![Value::from_cas_var("x")]),
            call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
        ])
        .unwrap();
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^x*(-cos[x] + sin[x])/2");
    }

    #[test]
    fn integrate_variable_free_function_as_constant() {
        let expr = call(CasFunction::Sin, vec![Value::from_cas_var("y")]);
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let derivative = crate::cas::diff::diff_cas(&result, &Value::from_cas_var("x")).unwrap();
        assert_eq!(derivative.to_string(), "sin[y]");
    }

    #[test]
    fn integrate_sin_with_symbolic_linear_coeff() {
        let x = Value::from_cas_var("x");
        let a_x = cas_mul(vec![Value::from_cas_var("a"), x.clone()]).expect("a*x");
        let expr = call(CasFunction::Sin, vec![a_x]);
        let result = integrate_cas(&expr, &x).expect("parameterized sine integral");
        let derivative =
            crate::cas::diff::diff_cas(&result, &x).expect("differentiate integral result");
        assert_eq!(
            simplify_cas_value(&derivative).expect("simplified derivative"),
            simplify_cas_value(&expr).expect("simplified integrand")
        );
    }

    #[test]
    fn integrate_exp_with_symbolic_linear_coeff() {
        let x = Value::from_cas_var("x");
        let a_x = cas_mul(vec![Value::from_cas_var("a"), x.clone()]).expect("a*x");
        let expr = call(CasFunction::Exp, vec![a_x]);
        let result = integrate_cas(&expr, &x).expect("parameterized exp integral");
        let derivative =
            crate::cas::diff::diff_cas(&result, &x).expect("differentiate integral result");
        assert_eq!(
            simplify_cas_value(&derivative).expect("simplified derivative"),
            simplify_cas_value(&expr).expect("simplified integrand")
        );
    }

    #[test]
    fn integrate_inverse_shifted_by_symbolic_parameter() {
        let x = Value::from_cas_var("x");
        let base = cas_add(vec![x.clone(), Value::from_cas_var("a")]).expect("x+a affine base");
        let expr = cas_pow(base, Value::Int(-1)).expect("inverse affine base");
        let result = integrate_cas(&expr, &x).expect("parameterized inverse affine integral");
        assert_eq!(result.to_string(), "ln[abs[x + a]]");
        let derivative =
            crate::cas::diff::diff_cas(&result, &x).expect("differentiate integral result");
        assert_eq!(
            simplify_cas_value(&derivative).expect("simplified derivative"),
            simplify_cas_value(&expr).expect("simplified integrand")
        );
    }

    #[test]
    fn integrate_inverse_affine_with_symbolic_coefficients() {
        let x = Value::from_cas_var("x");
        let base = cas_add(vec![
            cas_mul(vec![Value::from_cas_var("a"), x.clone()]).expect("a*x"),
            Value::from_cas_var("b"),
        ])
        .expect("a*x+b affine base");
        let expr = cas_pow(base, Value::Int(-1)).expect("inverse affine base");
        let result = integrate_cas(&expr, &x).expect("parameterized inverse affine integral");
        let derivative =
            crate::cas::diff::diff_cas(&result, &x).expect("differentiate integral result");
        assert_eq!(
            simplify_cas_value(&derivative).expect("simplified derivative"),
            simplify_cas_value(&expr).expect("simplified integrand")
        );
    }

    #[test]
    fn integrate_fractional_affine_power_with_symbolic_coefficients() {
        let x = Value::from_cas_var("x");
        let base = cas_add(vec![
            cas_mul(vec![Value::from_cas_var("a"), x.clone()]).expect("a*x"),
            Value::from_cas_var("b"),
        ])
        .expect("a*x+b affine base");
        let expr = cas_pow(
            base,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        )
        .expect("affine square root");
        let result = integrate_cas(&expr, &x).expect("parameterized affine power integral");
        let derivative =
            crate::cas::diff::diff_cas(&result, &x).expect("differentiate integral result");
        assert_eq!(
            simplify_cas_value(&derivative).expect("simplified derivative"),
            simplify_cas_value(&expr).expect("simplified integrand")
        );
    }

    #[test]
    fn integrate_expanded_affine_square_with_symbolic_coefficients() {
        let x = Value::from_cas_var("x");
        let base = cas_add(vec![
            cas_mul(vec![Value::from_cas_var("a"), x.clone()]).expect("a*x"),
            Value::from_cas_var("b"),
        ])
        .expect("a*x+b affine base");
        let expr = cas_pow(base, Value::Int(2)).expect("affine square");
        let result = integrate_cas(&expr, &x).expect("parameterized affine square integral");
        let derivative =
            crate::cas::diff::diff_cas(&result, &x).expect("differentiate integral result");
        assert_eq!(
            simplify_cas_value(&derivative).expect("simplified derivative"),
            simplify_cas_value(&expr).expect("simplified integrand")
        );
    }

    #[test]
    fn integrate_exp_cos_direct() {
        // int e^x*cos x dx = e^x*(sin x + cos x)/2
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
        // int x*e^x dx = e^x*(x - 1)
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
        // int x^2*sin x dx = -x^2*cos x + 2x*sin x + 2*cos x
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
        // int x*cos x dx = x*sin x + cos x
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
        // int e^(2x)*sin(3x) dx = e^(2x)*(2*sin(3x) - 3*cos(3x))/13
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

    // -- definite integrals --

    #[test]
    fn definite_polynomial() {
        // int_0^1 x dx = 1/2
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
        // int_0^2 x^2 dx = 8/3
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
        // int_0^pi sin(x) dx = 2
        let expr = call(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        // Use float pi approximation for the bound
        let pi = Value::float(std::f64::consts::PI);
        let result =
            definite_integrate_cas(&expr, &Value::from_cas_var("x"), &Value::Int(0), &pi).unwrap();
        // cos(pi) = -1, cos(0) = 1, so -(cos(pi) - cos(0)) = -(-1 - 1) = 2
        // But the result is Float due to float bound
        let f = result.as_f64().unwrap();
        assert!((f - 2.0).abs() < 1e-10, "expected ~2, got {f}");
    }

    #[test]
    fn definite_one_over_x() {
        // int_1^2 1/x dx = ln(2)
        let expr = op(CasOp::Divide, vec![Value::Int(1), Value::from_cas_var("x")]);
        let result = definite_integrate_cas(
            &expr,
            &Value::from_cas_var("x"),
            &Value::Int(1),
            &Value::Int(2),
        )
        .unwrap();
        // ln stays symbolic now -- result is ln[2]
        assert_eq!(result.to_string(), "ln[2]");
    }

    #[test]
    fn definite_exp() {
        // int_0^1 e^x dx = e - 1
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
        // int sqrt(x^3+1) dx -> algebraic part + ellik
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
        // int 1/sqrt(x^3+1) dx -> ellik
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
    fn integrate_one_over_sqrt_shifted_scaled_cubic() {
        // int dx/sqrt((2x+1)^3+1) reduces through u = x + 1/2.
        let x = Value::from_cas_var("x");
        let two_x = cas_mul(vec![Value::Int(2), x]).expect("2*x");
        let affine = cas_add(vec![two_x, Value::Int(1)]).expect("affine expression");
        let affine_cubed = cas_pow(affine, Value::Int(3)).expect("affine cube");
        let inner = cas_add(vec![affine_cubed, Value::Int(1)]).expect("cubic expression");
        let expr = cas_pow(
            inner,
            Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
        )
        .expect("inverse square root");

        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();

        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ellik"), "expected ellik in result: {s}");
        assert!(s.contains("arccos"), "expected arccos in result: {s}");
    }

    #[test]
    fn integrate_sqrt_shifted_scaled_cubic() {
        // int sqrt((2x+1)^3+1) dx keeps the algebraic part and elliptic correction.
        let x = Value::from_cas_var("x");
        let two_x = cas_mul(vec![Value::Int(2), x]).expect("2*x");
        let affine = cas_add(vec![two_x, Value::Int(1)]).expect("affine expression");
        let affine_cubed = cas_pow(affine, Value::Int(3)).expect("affine cube");
        let inner = cas_add(vec![affine_cubed, Value::Int(1)]).expect("cubic expression");
        let expr = cas_pow(
            inner,
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        )
        .expect("square root");

        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();

        assert!(!s.contains("unsupported"), "got unsupported: {s}");
        assert!(s.contains("ellik"), "expected ellik in result: {s}");
        assert!(
            s.contains("((x + 1/2)^3 + 1/8)^(1/2)") && s.contains("x + 1/2"),
            "expected algebraic radical in result: {s}"
        );
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
