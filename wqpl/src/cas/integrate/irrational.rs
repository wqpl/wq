//! Integration of irrational expressions involving sqrt(quadratic).
//!
//! Handles the standard forms:
//!   int sqrt(x^2 +/- a^2) dx
//!   int 1/sqrt(x^2 +/- a^2) dx
//!   int sqrt(a^2 - x^2) dx
//!   int 1/sqrt(a^2 - x^2) dx
//! and their linear-argument variants sqrt((kx+m)^2 +/- a^2).

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::rational::find_rational_root_value;
use super::trig::binomial_coeff;
use crate::cas::{
    cas_add, cas_div, cas_mul, cas_pow, cas_product, cas_sub, eval_exact_numeric_div, numeric_add,
    numeric_div, numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul, numeric_sub,
    poly_degree, poly_divide, poly_from_expr, poly_gcd, poly_is_zero, poly_mul, poly_to_expr,
    poly_trim, simplify_cas_value, substitute_expr,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

// Guard against infinite recursion in sqrt reduction.
// Each call to try_sqrt_reduction increments; if the counter exceeds
// MAX_SQRT_REDUCTION_DEPTH, reduction is skipped to break cycles.
thread_local! {
    static SQRT_REDUCTION_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
const MAX_SQRT_REDUCTION_DEPTH: usize = 5;

struct ResetDepthOnDrop;
impl Drop for ResetDepthOnDrop {
    fn drop(&mut self) {
        SQRT_REDUCTION_DEPTH.with(|c| c.set(c.get().saturating_sub(1)));
    }
}

/// Strategy entry point: integrate irrational expressions with sqrt(quadratic).
pub(super) fn integrate_irrational(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let simplified = simplify_cas_value(expr)?;
    try_irrational(&simplified, var)
}

fn try_irrational(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Case: sqrt(quad) = (quad)^(1/2)
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts() {
        let half_pow = exp.exact_half();
        let neg_half_pow = exp.exact_neg_half();

        if (half_pow || neg_half_pow)
            && let Some(quad_info) = classify_quadratic(base, var)
            && let Ok(result) = integrate_quadratic_root(&quad_info, half_pow, var)
        {
            return Ok(Some(result));
        }
        // Direct (linear)^(+/-1/2): use power rule via substitution
        if (half_pow || neg_half_pow)
            && let Some((a, b)) = classify_linear(base, var)
        {
            let one = Value::Int(1);
            if let Ok(result) = integrate_poly_sqrt_linear(&one, &a, &b, half_pow, var) {
                return Ok(Some(result));
            }
        }
        // Fall through to Euler if simple formula doesn't apply
    }

    // Case: product with a quadratic root factor
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        for (i, arg) in args.iter().enumerate() {
            if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts() {
                let half_pow = exp.exact_half();
                let neg_half_pow = exp.exact_neg_half();
                if (half_pow || neg_half_pow)
                    && let Some(quad_info) = classify_quadratic(base, var)
                {
                    let rest: Vec<Value> = args
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, v)| v.clone())
                        .collect();
                    let poly_expr = cas_product(rest);
                    if poly_from_expr(&poly_expr, var).is_ok()
                        && let Ok(result) =
                            integrate_poly_times_root(&poly_expr, &quad_info, half_pow, var)
                    {
                        return Ok(Some(result));
                    }
                    // Fall through to Euler if simple path fails
                }
                // Check for linear base: (a*x+b)^(+/-1/2)
                if (half_pow || neg_half_pow)
                    && let Some((a, b)) = classify_linear(base, var)
                {
                    let rest: Vec<Value> = args
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, v)| v.clone())
                        .collect();
                    let poly_expr = cas_product(rest);
                    if poly_from_expr(&poly_expr, var).is_ok()
                        && let Ok(result) =
                            integrate_poly_sqrt_linear(&poly_expr, &a, &b, half_pow, var)
                    {
                        return Ok(Some(result));
                    }
                }
            }
        }
    }

    // Quartic reciprocal reduction to the cubic elliptic path.
    if let Some(result) = try_quartic_inverse_reduction(expr, var)? {
        return Ok(Some(result));
    }

    // Try square factor extraction for higher-degree polynomials under sqrt
    if let Some(result) = try_sqrt_reduction(expr, var)? {
        return Ok(Some(result));
    }

    // Euler substitution for general sqrt(quadratic) cases.
    try_euler_substitution(expr, var)
}

/// Reduce int dx/sqrt(P4(x)) to a cubic-root integral when P4 has a rational
/// root r.  With x = r + 1/t, P4(x) = C3(t)/t^4 and dx/sqrt(P4(x)) =
/// -dt/sqrt(C3(t)).
fn try_quartic_inverse_reduction(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts() else {
        return Ok(None);
    };
    if !exp.exact_neg_half() {
        return Ok(None);
    }
    let poly = match poly_from_expr(base, var) {
        Ok(poly) if poly_degree(&poly) == 4 => poly,
        _ => return Ok(None),
    };
    let Some(root) = find_rational_root_value(&poly) else {
        return Ok(None);
    };

    let transformed_poly = reciprocal_quartic_transform(&poly, &root)?;
    if poly_degree(&transformed_poly) < 1 {
        return Ok(None);
    }

    let t_var = "--cas-quartic-t";
    let transformed_base = poly_to_expr(&transformed_poly, t_var)?;
    let transformed = cas_pow(
        transformed_base,
        Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
    )?;
    let integrated = super::integrate_expr_with_depth(&transformed, t_var, 0)?;
    let signed = cas_mul(vec![Value::Int(-1), integrated])?;

    let x_minus_root = cas_sub(Value::from_cas_var(var), root)?;
    let t_back = cas_div(Value::Int(1), x_minus_root)?;
    let back_subbed = substitute_expr(&signed, t_var, &t_back)?;
    simplify_cas_value(&back_subbed).map(Some)
}

fn reciprocal_quartic_transform(poly: &[Value], root: &Value) -> WqResult<Vec<Value>> {
    let mut out = vec![Value::Int(0); 4];
    for (i, coeff) in poly.iter().enumerate().take(5) {
        if numeric_is_zero(coeff) {
            continue;
        }
        for k in 1..=i {
            let power = 4 - k;
            let binom = Value::from_bigint(BigInt::from(binomial_coeff(i, k)));
            let root_power = pow_value(root, (i - k) as u32)?;
            let term = numeric_mul(&numeric_mul(coeff, &binom)?, &root_power)?;
            out[power] = numeric_add(&out[power], &term)?;
        }
    }
    poly_trim(&mut out);
    Ok(out)
}

/// Extract factors with multiplicity >= 2 from a polynomial under sqrt.
/// Returns `(outside_poly, inside_poly)` where:
/// - `outside` = product of factor^(mult // 2)
/// - `inside` = product of factor^(mult % 2) The original sqrt = |outside| *
///   sqrt(inside).
fn extract_square_factors(poly: &[Value], _var: &str) -> WqResult<(Vec<Value>, Vec<Value>)> {
    let sf_factors = crate::cas::square_free_factor(poly)?;
    let mut outside = vec![Value::Int(1)];
    let mut inside = vec![Value::Int(1)];
    for (factor, mult) in sf_factors {
        let out_pow = mult / 2;
        let in_pow = mult % 2;
        if out_pow > 0 {
            let factor_pow = super::rational::poly_pow(&factor, out_pow)?;
            outside = poly_mul(&outside, &factor_pow)?;
        }
        if in_pow > 0 {
            inside = poly_mul(&inside, &factor)?;
        }
    }
    poly_trim(&mut outside);
    poly_trim(&mut inside);
    Ok((outside, inside))
}

/// Try to reduce a sqrt(cubic) or sqrt(quartic) by extracting square factors.
/// If the polynomial under sqrt can be simplified, rebuild the expression
/// and recurse into the integration pipeline.
fn try_sqrt_reduction(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Guard against infinite recursion
    let depth = SQRT_REDUCTION_DEPTH.get();
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        depth,
        "[cas-v] sqrt_reduction enter depth={depth} expr={}",
        expr.format_cas().unwrap_or_else(|| expr.to_string())
    );
    if depth >= MAX_SQRT_REDUCTION_DEPTH {
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] sqrt_reduction exit depth={depth} -> max_depth_exceeded"
        );
        return Ok(None);
    }
    SQRT_REDUCTION_DEPTH.set(depth + 1);
    let _guard = ResetDepthOnDrop;

    let (base, is_sqrt) = match find_sqrt_factor(expr, var) {
        Some((b, s)) => (b, s),
        None => return Ok(None),
    };

    let base_poly = match poly_from_expr(&base, var) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let deg = poly_degree(&base_poly);
    if deg < 3 {
        return Ok(None);
    }

    let (out_poly, in_poly) = extract_square_factors(&base_poly, var)?;
    let in_deg = poly_degree(&in_poly);

    // If no reduction happened, don't recurse
    if in_deg == deg {
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] sqrt_reduction exit depth={depth} -> no_reduction"
        );
        return Ok(None);
    }

    // Build the simplified expression: extract outside factor, keep inside sqrt
    let out_expr = poly_to_expr(&out_poly, var)?;
    let in_expr = poly_to_expr(&in_poly, var)?;
    let half = Value::from_fraction_parts(1u64.into(), 2u64.into());
    let neg_half = Value::from_fraction_parts((-1i64).into(), 2u64.into());

    // Build the replacement expression: out * (in)^(+/-1/2)
    let simplified = {
        let in_pow = Value::from_cas_op(
            CasOp::Power,
            vec![in_expr, if is_sqrt { half } else { neg_half }],
        );
        if poly_degree(&out_poly) == 0 && numeric_is_one(&out_poly[0]) {
            in_pow
        } else {
            cas_mul(vec![out_expr, in_pow])?
        }
    };
    let simplified = simplify_cas_value(&simplified)?;

    // Recurse into the integration pipeline
    let result = super::integrate_expr_with_depth(&simplified, var, 0)?;
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        depth,
        "[cas-v] sqrt_reduction exit depth={depth} -> {}",
        result.format_cas().unwrap_or_else(|| result.to_string())
    );
    Ok(Some(result))
}

// ---------------------------------------------------------------------------
// Euler substitution #1 (a > 0): sqrt(ax^2+bx+c) = sqrt(a)*x + t
// ---------------------------------------------------------------------------

fn try_euler_substitution(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] euler enter: {}",
        expr.format_cas().unwrap_or_else(|| expr.to_string())
    );
    let root_expr = find_sqrt_factor(expr, var);
    let (quad_base, _is_sqrt) = match root_expr {
        Some(q) => q,
        None => return Ok(None),
    };

    let Some(q) = classify_quadratic(&quad_base, var) else {
        return Ok(None);
    };

    let (a, b, c) = extract_abc(&q);

    // Euler #1: a > 0
    if !numeric_is_negative(&a)
        && !numeric_is_zero(&a)
        && let Some(s) = sqrt_value(&a)
    {
        return euler1_integrate(expr, &q, &a, &b, &c, &s, var).map(Some);
    }

    // Euler #2: c > 0
    if !numeric_is_negative(&c)
        && !numeric_is_zero(&c)
        && let Some(s) = sqrt_value(&c)
    {
        return euler2_integrate(expr, &q, &a, &b, &c, &s, var).map(Some);
    }

    // Euler #3: real roots (discriminant b^2-4ac > 0)
    let disc = numeric_sub(
        &numeric_mul(&b, &b).unwrap_or(Value::Int(0)),
        &numeric_mul(
            &numeric_mul(&Value::Int(4), &a).unwrap_or(Value::Int(1)),
            &c,
        )
        .unwrap_or(Value::Int(0)),
    )
    .unwrap_or(Value::Int(-1));
    if !numeric_is_negative(&disc)
        && !numeric_is_zero(&disc)
        && !numeric_is_zero(&a)
        && let Some(s) = sqrt_value(&disc)
    {
        return euler3_integrate(expr, &q, &a, &b, &c, &s, var).map(Some);
    }

    cas_trace!(DebugLogFlags::CAS, "[cas] euler exit (no_match)");
    Ok(None)
}

// ---------------------------------------------------------------------------
// Euler substitution helpers (moved from below)
// ---------------------------------------------------------------------------

/// Extract the standard-form coefficients from QuadInfo.
fn extract_abc(q: &QuadInfo) -> (Value, Value, Value) {
    let a = q.a.clone();
    let neg_two = Value::Int(-2);
    let b = numeric_mul(
        &numeric_mul(&a, &q.shift).unwrap_or(Value::Int(0)),
        &neg_two,
    )
    .unwrap_or(Value::Int(0));
    let shift_sq = numeric_mul(&q.shift, &q.shift).unwrap_or(Value::Int(0));
    let c = numeric_add(&numeric_mul(&a, &shift_sq).unwrap_or(Value::Int(0)), &q.k)
        .unwrap_or(Value::Int(0));
    (a, b, c)
}

/// Find the sqrt(poly) or 1/sqrt(poly) factor in expr, returning (poly,
/// is_sqrt).
fn find_sqrt_factor(expr: &Value, _var: &str) -> Option<(Value, bool)> {
    // Direct: expr = (poly)^(1/2) or (poly)^(-1/2)
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts() {
        if exp.exact_half() {
            return Some((base.clone(), true));
        }
        if exp.exact_neg_half() {
            return Some((base.clone(), false));
        }
        // Recurse into exponent -1: (product * sqrt)^(-1) -> product * sqrt
        if exp.exact_int().is_some_and(|k| k == BigInt::from(-1)) {
            return find_sqrt_factor(base, _var);
        }
    }
    // Product: look for a sqrt factor
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        for arg in args {
            if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts() {
                if exp.exact_half() {
                    return Some((base.clone(), true));
                }
                if exp.exact_neg_half() {
                    return Some((base.clone(), false));
                }
            }
        }
    }
    None
}

/// Euler #1 (a > 0): sqrt(ax^2+bx+c) = sqrt(a)*x + t
fn euler1_integrate(
    expr: &Value,
    _q: &QuadInfo,
    a: &Value,
    b: &Value,
    c: &Value,
    s: &Value,
    var: &str,
) -> WqResult<Value> {
    // s = sqrt(a)
    // x = (t^2 - c) / (b - 2s*t)
    // sqrt = s*x + t
    // dx/dt = 2*sqrt / (b - 2s*t)
    let t = Value::from_cas_var("--cas-euler-t");
    let two = Value::Int(2);
    let two_s = numeric_mul(&two, s)?;
    let denom = cas_sub(b.clone(), cas_mul(vec![two_s, t.clone()])?)?;

    let x_t = simplify_cas_value(&cas_div(
        cas_sub(cas_pow(t.clone(), Value::Int(2))?, c.clone())?,
        denom.clone(),
    )?)?;

    let sqrt_t = simplify_cas_value(&cas_add(vec![
        cas_mul(vec![s.clone(), x_t.clone()])?,
        t.clone(),
    ])?)?;

    let dx_dt = simplify_cas_value(&cas_div(cas_mul(vec![two, sqrt_t.clone()])?, denom)?)?;

    // Build original sqrt for back-substitution
    let orig_sqrt = build_sqrt_expr(a, b, c, var)?;
    let t_back = simplify_cas_value(&cas_sub(
        orig_sqrt.clone(),
        cas_mul(vec![s.clone(), Value::from_cas_var(var)])?,
    )?)?;

    euler_integrate_core(expr, &orig_sqrt, &x_t, &sqrt_t, &dx_dt, &t_back, var)
}

/// Euler #2 (c > 0): sqrt(ax^2+bx+c) = x*t + sqrt(c)
fn euler2_integrate(
    expr: &Value,
    _q: &QuadInfo,
    a: &Value,
    b: &Value,
    c: &Value,
    s: &Value,
    var: &str,
) -> WqResult<Value> {
    // s = sqrt(c)
    // x = (2s*t - b) / (a - t^2)
    // sqrt = x*t + s
    // dx/dt = 2*sqrt / (a - t^2)
    let t = Value::from_cas_var("--cas-euler-t");
    let two = Value::Int(2);
    let t_sq = cas_pow(t.clone(), Value::Int(2))?;
    let denom = cas_sub(a.clone(), t_sq)?;

    let two_s = numeric_mul(&two, s)?;
    let x_t = simplify_cas_value(&cas_div(
        cas_sub(cas_mul(vec![two_s, t.clone()])?, b.clone())?,
        denom.clone(),
    )?)?;

    let sqrt_t = simplify_cas_value(&cas_add(vec![
        cas_mul(vec![x_t.clone(), t.clone()])?,
        s.clone(),
    ])?)?;

    let dx_dt = simplify_cas_value(&cas_div(cas_mul(vec![two, sqrt_t.clone()])?, denom)?)?;

    let orig_sqrt = build_sqrt_expr(a, b, c, var)?;
    // t = (sqrt(quad) - sqrt(c)) / x
    let t_back = simplify_cas_value(&cas_div(
        cas_sub(orig_sqrt.clone(), s.clone())?,
        Value::from_cas_var(var),
    )?)?;

    euler_integrate_core(expr, &orig_sqrt, &x_t, &sqrt_t, &dx_dt, &t_back, var)
}

/// Euler #3 (real roots alpha < beta): sqrt(a(x-alpha)(x-beta)) = t*(x-alpha)
fn euler3_integrate(
    expr: &Value,
    _q: &QuadInfo,
    a: &Value,
    b: &Value,
    c: &Value,
    sqrt_disc: &Value,
    var: &str,
) -> WqResult<Value> {
    // Discriminant delta = b^2-4ac > 0.  Roots: alpha = (-b-sqrt(delta))/(2a), beta = (-b+sqrt(delta))/(2a)
    let two = Value::Int(2);
    let neg_b = numeric_mul(b, &Value::Int(-1))?;
    let two_a = numeric_mul(&two, a)?;
    let alpha = numeric_div(&numeric_sub(&neg_b, sqrt_disc)?, &two_a)?;
    let beta = numeric_div(&numeric_add(&neg_b, sqrt_disc)?, &two_a)?;

    // sqrt(a(x-alpha)(x-beta)) = t*(x-alpha)
    // x = (a*beta - t^2*alpha) / (a - t^2)
    // dx/dt = 2a*t*(beta-alpha) / (a - t^2)^2
    let t = Value::from_cas_var("--cas-euler-t");
    let t_sq = cas_pow(t.clone(), Value::Int(2))?;
    let denom = cas_sub(a.clone(), t_sq.clone())?;

    let x_t = simplify_cas_value(&cas_div(
        cas_sub(
            cas_mul(vec![a.clone(), beta.clone()])?,
            cas_mul(vec![t_sq, alpha.clone()])?,
        )?,
        denom.clone(),
    )?)?;

    let sqrt_t = simplify_cas_value(&cas_mul(vec![
        t.clone(),
        cas_sub(x_t.clone(), alpha.clone())?,
    ])?)?;

    let beta_minus_alpha = numeric_sub(&beta, &alpha)?;
    let dx_dt = simplify_cas_value(&cas_div(
        cas_mul(vec![
            cas_mul(vec![two, a.clone()])?,
            t.clone(),
            beta_minus_alpha,
        ])?,
        cas_pow(denom.clone(), Value::Int(2))?,
    )?)?;

    let orig_sqrt = build_sqrt_expr(a, b, c, var)?;
    // t = sqrt(quad) / (x - alpha)
    let t_back = simplify_cas_value(&cas_div(
        orig_sqrt.clone(),
        cas_sub(Value::from_cas_var(var), alpha)?,
    )?)?;

    euler_integrate_core(expr, &orig_sqrt, &x_t, &sqrt_t, &dx_dt, &t_back, var)
}

/// Build sqrt(ax^2+bx+c) as a CAS expression.
fn build_sqrt_expr(a: &Value, b: &Value, c: &Value, var: &str) -> WqResult<Value> {
    let x = Value::from_cas_var(var);
    let x_sq = cas_pow(x.clone(), Value::Int(2))?;
    let inner = cas_add(vec![
        cas_mul(vec![a.clone(), x_sq])?,
        cas_mul(vec![b.clone(), x])?,
        c.clone(),
    ])?;
    simplify_cas_value(&cas_pow(
        inner,
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )?)
}

/// Core Euler integration: substitute, rationalize, integrate, back-substitute.
fn euler_integrate_core(
    expr: &Value,
    orig_sqrt: &Value,
    x_t: &Value,
    sqrt_t: &Value,
    dx_dt: &Value,
    t_back: &Value,
    var: &str,
) -> WqResult<Value> {
    cas_trace!(DebugLogFlags::CAS, "[euler] expr={}", expr);
    cas_trace!(DebugLogFlags::CAS, "[euler] orig_sqrt={}", orig_sqrt);
    cas_trace!(DebugLogFlags::CAS, "[euler] x_t={}", x_t);
    cas_trace!(DebugLogFlags::CAS, "[euler] sqrt_t={}", sqrt_t);
    cas_trace!(DebugLogFlags::CAS, "[euler] dx_dt={}", dx_dt);

    // Replace sqrt BEFORE variable substitution: after x->x_t the sqrt
    // argument changes and no longer matches orig_sqrt exactly.
    let integrand_t = replace_sqrt_in_expr(expr, orig_sqrt, sqrt_t);
    cas_trace!(
        DebugLogFlags::CAS,
        "[euler] after replace_sqrt: {}",
        integrand_t
    );
    let integrand_t = substitute_expr(&integrand_t, var, x_t)?;
    cas_trace!(DebugLogFlags::CAS, "[euler] after sub x: {}", integrand_t);

    // Substitute sqrt_t with a plain variable _s so that powers cancel
    // naturally in cas_mul.  Also expand to distribute negative integer
    // powers across products: (a*b)^(-k) -> a^(-k)*b^(-k).  Together
    // these let (x*sqrt)^(-1)*sqrt cancel to x^(-1), producing a rational
    // function in _t that the rational module can integrate.
    let s_var = Value::from_cas_var("--cas-sqrt-s");
    let integrand_s = replace_sqrt_in_expr(&integrand_t, sqrt_t, &s_var);
    let dx_dt_s = replace_sqrt_in_expr(dx_dt, sqrt_t, &s_var);
    cas_trace!(DebugLogFlags::CAS, "[euler] integrand_s={}", integrand_s);
    cas_trace!(DebugLogFlags::CAS, "[euler] dx_dt_s={}", dx_dt_s);
    let integrand_t = simplify_cas_value(&cas_mul(vec![integrand_s, dx_dt_s])?)?;
    cas_trace!(
        DebugLogFlags::CAS,
        "[euler] after mul+simplify: {}",
        integrand_t
    );
    let integrand_t = crate::cas::expand_cas(&integrand_t)?;
    cas_trace!(
        DebugLogFlags::CAS,
        "[euler] after expand_cas: {}",
        integrand_t
    );

    // GCD-based cancellation for common polynomial factors
    let integrand_t = cancel_rational_gcd(&integrand_t, "--cas-euler-t")?;

    let integrated = super::rational::integrate_by_rational(&integrand_t, "--cas-euler-t")?;
    let integrated = integrated
        .ok_or_else(|| crate::cas::cas_err("Euler substitution produced non-rational integrand"))?;

    simplify_cas_value(&substitute_expr(&integrated, "--cas-euler-t", t_back)?)
}

/// Simplify a rational function in `var` by cancelling common factors between
/// numerator and denominator polynomials.
fn cancel_rational_gcd(expr: &Value, var: &str) -> WqResult<Value> {
    // Try to extract numerator/denominator from product form
    let (num_expr, denom_expr) = match extract_rational_parts(expr) {
        Some(p) => p,
        None => return Ok(expr.clone()),
    };

    let num_poly = match poly_from_expr(&num_expr, var) {
        Ok(p) => p,
        Err(_) => return Ok(expr.clone()),
    };
    let denom_poly = match poly_from_expr(&denom_expr, var) {
        Ok(p) => p,
        Err(_) => return Ok(expr.clone()),
    };

    let gcd = match poly_gcd(&num_poly, &denom_poly) {
        Ok(g) => g,
        Err(_) => return Ok(expr.clone()),
    };
    if poly_is_zero(&gcd) || (poly_degree(&gcd) == 0 && gcd[0] == Value::Int(1)) {
        return Ok(expr.clone());
    }

    let (num_reduced, _) = poly_divide(&num_poly, &gcd)?;
    let (denom_reduced, _) = poly_divide(&denom_poly, &gcd)?;

    let num_expr_reduced = poly_to_expr(&num_reduced, var)?;
    let denom_expr_reduced = poly_to_expr(&denom_reduced, var)?;

    simplify_cas_value(&cas_div(num_expr_reduced, denom_expr_reduced)?)
}

/// Extract (numerator, denominator) from an expression in product form.
/// Recognizes `(* num_factors... denom_factor^(-1)...)`.
fn extract_rational_parts(expr: &Value) -> Option<(Value, Value)> {
    let mut num_factors: Vec<Value> = Vec::new();
    let mut denom_factors: Vec<Value> = Vec::new();

    let args: &[Value] = if let Some((CasOp::Multiply, a)) = expr.cas_op_parts() {
        a
    } else if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && exp.exact_int_is(-1)
    {
        return Some((Value::Int(1), base.clone()));
    } else {
        return Some((expr.clone(), Value::Int(1)));
    };

    for arg in args {
        if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && exp.exact_int_is(-1)
        {
            denom_factors.push(base.clone());
        } else if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && let Some(p) = exp.exact_int()
            && p < BigInt::from(0)
        {
            // base^(-k) for k > 1
            let k = (-p).to_usize()?;
            for _ in 0..k {
                denom_factors.push(base.clone());
            }
        } else {
            num_factors.push(arg.clone());
        }
    }

    if denom_factors.is_empty() {
        return None;
    }

    let num = cas_product(num_factors.to_vec());
    let denom = cas_product(denom_factors.to_vec());
    Some((num, denom))
}

/// Replace sqrt_expr with replacement everywhere in expr.
fn replace_sqrt_in_expr(expr: &Value, sqrt_expr: &Value, replacement: &Value) -> Value {
    if expr == sqrt_expr {
        return replacement.clone();
    }
    // Also match 1/sqrt form: expr^(-1) where expr == sqrt
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && exp.exact_int_is(-1)
        && base == sqrt_expr
    {
        return cas_pow(replacement.clone(), Value::Int(-1))
            .and_then(|v| simplify_cas_value(&v))
            .unwrap_or_else(|_| expr.clone());
    }
    // Match expr = base^p where sqrt_expr = base^(1/2):
    // base^p = sqrt_expr^(2p) -> replacement^(2p)
    // Handles cases like 1/sqrt: base^(-1/2) -> replacement^(-1)
    if let Some((CasOp::Power, [expr_base, expr_exp])) = expr.cas_op_parts()
        && let Some((CasOp::Power, [sqrt_base, sqrt_exp])) = sqrt_expr.cas_op_parts()
        && sqrt_exp.exact_half()
        && expr_base == sqrt_base
    {
        let two = Value::Int(2);
        let new_exp = numeric_mul(expr_exp, &two).unwrap_or_else(|_| expr_exp.clone());
        return cas_pow(replacement.clone(), new_exp)
            .and_then(|v| simplify_cas_value(&v))
            .unwrap_or_else(|_| expr.clone());
    }
    if let Some((op, args)) = expr.cas_op_parts() {
        let new_args: Vec<Value> = args
            .iter()
            .map(|a| replace_sqrt_in_expr(a, sqrt_expr, replacement))
            .collect();
        return Value::from_cas_op(op, new_args);
    }
    if let Some((name, args)) = expr.cas_function_parts() {
        let new_args: Vec<Value> = args
            .iter()
            .map(|a| replace_sqrt_in_expr(a, sqrt_expr, replacement))
            .collect();
        return Value::from_cas_function(name, new_args);
    }
    if let Some((name, args)) = expr.cas_apply_parts() {
        let new_args: Vec<Value> = args
            .iter()
            .map(|a| replace_sqrt_in_expr(a, sqrt_expr, replacement))
            .collect();
        return Value::from_cas_apply(name.as_str(), new_args);
    }
    expr.clone()
}

/// Classification of a quadratic expression ax^2+bx+c.
struct QuadInfo {
    /// Coefficient of x^2
    a: Value,
    /// Completed-square shift s such that expr = a*(x - s)^2 + k
    shift: Value,
    /// Remaining constant k
    k: Value,
}

/// Try to classify expr as a quadratic in var: a*var^2 + b*var + c.
fn classify_quadratic(expr: &Value, var: &str) -> Option<QuadInfo> {
    // Must be a polynomial of degree 2
    let coeffs = poly_from_expr(expr, var).ok()?;
    if poly_degree(&coeffs) != 2 {
        // Check deg 0 or 1 -- not a quadratic sqrt
        return None;
    }

    let a = coeffs[2].clone();
    let b = coeffs[1].clone();
    let c = coeffs[0].clone();

    if numeric_is_zero(&a) {
        return None;
    }

    // Complete the square: a*x^2 + b*x + c = a*(x + b/(2a))^2 + (c - b^2/(4a))
    let two = Value::Int(2);
    let four = Value::Int(4);
    let two_a = numeric_mul(&two, &a).ok()?;
    let shift_num = numeric_mul(&b, &Value::Int(-1)).ok()?;
    // Use exact division so coefficients stay Int/Fraction, not Float
    let shift = eval_exact_numeric_div(&shift_num, &two_a).ok()?;

    let b_sq = numeric_mul(&b, &b).ok()?;
    let four_a = numeric_mul(&four, &a).ok()?;
    let b_sq_over_4a = eval_exact_numeric_div(&b_sq, &four_a).ok()?;
    let k = numeric_sub(&c, &b_sq_over_4a).ok()?;

    Some(QuadInfo { a, shift, k })
}

/// Integrate sqrt(ax^2+bx+c)^(+/-1/2).
fn integrate_quadratic_root(q: &QuadInfo, is_sqrt: bool, var: &str) -> WqResult<Value> {
    if is_sqrt {
        integrate_sqrt_quadratic(q, var)
    } else {
        integrate_one_over_sqrt_quadratic(q, var)
    }
}

/// int sqrt(a*x^2 + b*x + c) dx
fn integrate_sqrt_quadratic(q: &QuadInfo, var: &str) -> WqResult<Value> {
    let a = &q.a;
    let k = &q.k;

    if numeric_is_zero(&q.shift) && numeric_is_one(a) {
        // Simple form: sqrt(x^2 + k)
        if numeric_is_negative(k) {
            // sqrt(x^2 - d^2) where d^2 = -k
            let d_sq = numeric_mul(k, &Value::Int(-1))?;
            sqrt_value(&d_sq)
                .ok_or_else(|| crate::cas::cas_err("expected perfect square under sqrt"))?;
            // x/2*sqrt(x^2-d^2) - d^2/2*ln|x + sqrt(x^2-d^2)|
            let x = Value::from_cas_var(var);
            let sqrt_expr = Value::from_cas_function(
                CasFunction::Sqrt,
                vec![cas_sub(cas_pow(x.clone(), Value::Int(2))?, d_sq.clone())?],
            );
            let first = cas_mul(vec![
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
                x.clone(),
                sqrt_expr.clone(),
            ])?;
            let half_d_sq = cas_div(d_sq, Value::Int(2))?;
            let inner = cas_add(vec![x, sqrt_expr])?;
            let ln = Value::from_cas_function(
                CasFunction::Ln,
                vec![Value::from_cas_function(CasFunction::Abs, vec![inner])],
            );
            let second = cas_mul(vec![half_d_sq, ln])?;
            return simplify_cas_value(&cas_sub(first, second)?);
        } else {
            // sqrt(x^2 + d^2) where d^2 = k
            let d = sqrt_value(k)
                .ok_or_else(|| crate::cas::cas_err("expected perfect square under sqrt"))?;
            // x/2*sqrt(x^2+d^2) + d^2/2*arcsinh(x/d)
            let x = Value::from_cas_var(var);
            let sqrt_expr = Value::from_cas_function(
                CasFunction::Sqrt,
                vec![cas_add(vec![
                    cas_pow(x.clone(), Value::Int(2))?,
                    k.clone(),
                ])?],
            );
            let first = cas_mul(vec![
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
                x.clone(),
                sqrt_expr,
            ])?;
            let half_k = cas_div(k.clone(), Value::Int(2))?;
            let arg = cas_div(x, d)?;
            let arcsinh = Value::from_cas_function(CasFunction::ArcSinh, vec![arg]);
            let second = cas_mul(vec![half_k, arcsinh])?;
            return simplify_cas_value(&cas_add(vec![first, second])?);
        }
    }

    // General case: sqrt(a*(x-s)^2 + k)
    // Substitute u = x - s, then solve sqrt(a*u^2 + k)
    if !numeric_is_zero(&q.shift) {
        let u_var = "--cas-shift-u";
        let simple_q = QuadInfo {
            a: a.clone(),
            shift: Value::Int(0),
            k: k.clone(),
        };
        let integrated = integrate_sqrt_quadratic(&simple_q, u_var)?;
        simplify_cas_value(&substitute_expr(
            &integrated,
            u_var,
            &cas_sub(Value::from_cas_var(var), q.shift.clone())?,
        )?)
    } else if !numeric_is_one(a) {
        // sqrt(a*x^2 + k): factor out sqrt(a)
        // sqrt(a*x^2 + k) = sqrt(a)*sqrt(x^2 + k/a)
        let sqrt_a = Value::from_cas_function(CasFunction::Sqrt, vec![a.clone()]);
        let simple_q = QuadInfo {
            a: Value::Int(1),
            shift: Value::Int(0),
            k: numeric_div(k, a)?,
        };
        let inner = integrate_sqrt_quadratic(&simple_q, var)?;
        simplify_cas_value(&cas_mul(vec![sqrt_a, inner])?)
    } else {
        // k = 0: sqrt(x^2) = |x| -- already handled by simpler rules
        Err(crate::cas::cas_err("degenerate quadratic under sqrt"))
    }
}

/// int 1/sqrt(a*x^2 + b*x + c) dx
fn integrate_one_over_sqrt_quadratic(q: &QuadInfo, var: &str) -> WqResult<Value> {
    let a = &q.a;
    let k = &q.k;

    if numeric_is_zero(&q.shift) && numeric_is_one(a) {
        if numeric_is_negative(k) {
            // 1/sqrt(x^2 - d^2) -> arccosh(x/d)
            let d_sq = numeric_mul(k, &Value::Int(-1))?;
            let d = sqrt_value(&d_sq)
                .ok_or_else(|| crate::cas::cas_err("expected perfect square under sqrt"))?;
            let arg = cas_div(Value::from_cas_var(var), d)?;
            return Ok(Value::from_cas_function(CasFunction::ArcCosh, vec![arg]));
        } else {
            // 1/sqrt(x^2 + d^2) -> arcsinh(x/d)
            let d = sqrt_value(k)
                .ok_or_else(|| crate::cas::cas_err("expected perfect square under sqrt"))?;
            let arg = cas_div(Value::from_cas_var(var), d)?;
            return Ok(Value::from_cas_function(CasFunction::ArcSinh, vec![arg]));
        }
    }

    // sqrt(a^2 - x^2) case: a is negative of x^2
    // a*x^2 + k with a < 0, k > 0 -> sqrt(k - |a|*x^2)
    if numeric_is_negative(a) && numeric_is_zero(&q.shift) {
        let neg_a = numeric_mul(a, &Value::Int(-1))?;
        if numeric_is_one(&neg_a) && !numeric_is_negative(k) {
            // 1/sqrt(k - x^2) -> arcsin(x/sqrt(k))  if k > 0
            let d = sqrt_value(k)
                .ok_or_else(|| crate::cas::cas_err("expected perfect square under sqrt"))?;
            let arg = cas_div(Value::from_cas_var(var), d)?;
            return Ok(Value::from_cas_function(CasFunction::ArcSin, vec![arg]));
        }
    }

    // General case with shift: substitute u = x - s
    if !numeric_is_zero(&q.shift) {
        let u_var = "--cas-shift-u";
        let simple_q = QuadInfo {
            a: a.clone(),
            shift: Value::Int(0),
            k: k.clone(),
        };
        let integrated = integrate_one_over_sqrt_quadratic(&simple_q, u_var)?;
        return simplify_cas_value(&substitute_expr(
            &integrated,
            u_var,
            &cas_sub(Value::from_cas_var(var), q.shift.clone())?,
        )?);
    }

    Err(crate::cas::cas_err("unsupported irrational form"))
}

/// Integrate P(x) * sqrt(quadratic)^(+/-1/2), where P is a polynomial.
fn integrate_poly_times_root(
    poly_expr: &Value,
    q: &QuadInfo,
    is_sqrt: bool,
    var: &str,
) -> WqResult<Value> {
    let coeffs = poly_from_expr(poly_expr, var)
        .map_err(|_| crate::cas::cas_err("expected polynomial factor in irrational integrand"))?;
    let deg = poly_degree(&coeffs);

    if deg == 0 {
        // Constant factor
        let c = &coeffs[0];
        let inner = integrate_quadratic_root(q, is_sqrt, var)?;
        if numeric_is_one(c) {
            return Ok(inner);
        }
        return simplify_cas_value(&cas_mul(vec![c.clone(), inner])?);
    }

    if deg == 1 && numeric_is_zero(&q.shift) && numeric_is_one(&q.a) {
        // int x*sqrt(x^2+k)^(+/-1/2) dx -- power rule
        let k = &q.k;
        if is_sqrt {
            // int x*(x^2+k)^(1/2) dx = (x^2+k)^(3/2)/3
            let inner = cas_add(vec![
                cas_pow(Value::from_cas_var(var), Value::Int(2))?,
                k.clone(),
            ])?;
            let pow = cas_pow(
                inner,
                Value::from_fraction_parts(BigInt::from(3), BigInt::from(2)),
            )?;
            return simplify_cas_value(&cas_div(pow, Value::Int(3))?);
        } else {
            // int x*(x^2+k)^(-1/2) dx = (x^2+k)^(1/2)
            let inner = cas_add(vec![
                cas_pow(Value::from_cas_var(var), Value::Int(2))?,
                k.clone(),
            ])?;
            return Ok(Value::from_cas_function(CasFunction::Sqrt, vec![inner]));
        }
    }

    // For higher degree: use reduction formula (pure x^n only)
    // int x^n*(x^2+k)^(+/-1/2) dx
    if deg >= 2 && numeric_is_zero(&q.shift) && numeric_is_one(&q.a) && is_monomial(&coeffs) {
        let c = &coeffs[deg];
        let result = integrate_xn_sqrt_reduction(deg, &q.k, is_sqrt, var)?;
        return if numeric_is_one(c) {
            Ok(result)
        } else {
            simplify_cas_value(&cas_mul(vec![c.clone(), result])?)
        };
    }

    Err(crate::cas::cas_err(format!(
        "irrational integral with degree-{} polynomial factor not yet supported",
        deg
    )))
}

/// int P(x)*(a*x+b)^(+/-1/2) dx via substitution u = a*x+b, direct power rule.
///
/// Does NOT call integrate_expr_with_depth (avoids re-entering the strategy
/// chain and causing infinite recursion).  Instead, converts P(x) to a
/// polynomial Q(u) via x = (u-b)/a, then integrates each term of
/// Q(u)*u^(+/-1/2)/a using the power rule.
fn integrate_poly_sqrt_linear(
    poly_expr: &Value,
    a: &Value,
    b: &Value,
    is_sqrt: bool,
    var: &str,
) -> WqResult<Value> {
    let coeffs = poly_from_expr(poly_expr, var)
        .map_err(|_| crate::cas::cas_err("expected polynomial factor in irrational integrand"))?;
    let deg = poly_degree(&coeffs);
    let u_var_val = Value::from_cas_var("--cas-lin-u");

    // Convert P(x) to Q(u) where u = a*x+b, x = (u-b)/a.
    // Q(u) = sum p_i * ((u - b)/a)^i
    // We compute coefficients of Q(u) as a Vec<Value> [q0, q1, ..., q_deg].
    let mut q = vec![Value::Int(0); deg + 1];
    // Precompute (u-b)^0 through (u-b)^deg as coefficient vectors
    // (u-b)^i = sum_{j=0}^{i} C(i,j) * (-b)^(i-j) * u^j
    let inv_a = eval_exact_numeric_div(&Value::Int(1), a)?;
    let mut a_pows = vec![Value::Int(1); deg + 1]; // a_pows[i] = 1/a^i
    for i in 1..=deg {
        a_pows[i] = numeric_mul(&a_pows[i - 1], &inv_a)?;
    }

    // Compute Q(u) term by term
    for i in 0..=deg {
        let p_i = &coeffs[i];
        if numeric_is_zero(p_i) {
            continue;
        }
        // Compute (u - b)^i coefficients
        // coeff of u^j in (u-b)^i: C(i, j) * (-b)^(i-j)
        let m1_b = numeric_mul(b, &Value::Int(-1))?; // -b
        for (j, q_j) in q.iter_mut().enumerate().take(i + 1) {
            let binom = Value::from_bigint(BigInt::from(binomial_coeff(i, j)));
            let neg_b_pow = pow_value(&m1_b, (i - j) as u32)?;
            let term_coeff = numeric_mul(&numeric_mul(&binom, &neg_b_pow)?, p_i)?;
            let term_coeff = numeric_mul(&term_coeff, &a_pows[i])?;
            *q_j = numeric_add(q_j, &term_coeff)?;
        }
    }

    // Now integrate: int Q(u) * u^(+/-1/2) / a du
    // Each term: q_j/a * int u^(j +/- 1/2) du = q_j/a * u^(j +/- 1/2 + 1) / (j +/- 1/2 + 1)
    let mut result_terms: Vec<Value> = Vec::new();
    let half = Value::from_fraction_parts(1u64.into(), 2u64.into());
    let one = Value::Int(1);
    let exponent_delta: Value = if is_sqrt {
        half.clone()
    } else {
        numeric_mul(&half, &Value::Int(-1))?
    };

    for (j, q_j) in q.iter().enumerate() {
        if numeric_is_zero(q_j) {
            continue;
        }
        // New exponent: j + delta + 1
        let new_exp = numeric_add(&numeric_add(&Value::Int(j as i64), &exponent_delta)?, &one)?;
        // Coefficient: q_j / (a * new_exp)
        let denom = numeric_mul(a, &new_exp)?;
        let coeff = eval_exact_numeric_div(q_j, &denom)?;

        let term = cas_mul(vec![coeff, cas_pow(u_var_val.clone(), new_exp)?])?;
        result_terms.push(term);
    }

    if result_terms.is_empty() {
        return Ok(Value::Int(0));
    }
    let result_u = simplify_cas_value(&cas_add(result_terms)?)?;

    // Back-substitute u = a*x + b
    let u_back = cas_add(vec![
        cas_mul(vec![a.clone(), Value::from_cas_var(var)])?,
        b.clone(),
    ])?;
    simplify_cas_value(&substitute_expr(&result_u, "--cas-lin-u", &u_back)?)
}

fn pow_value(base: &Value, exp: u32) -> WqResult<Value> {
    if exp == 0 {
        return Ok(Value::Int(1));
    }
    if exp == 1 {
        return Ok(base.clone());
    }
    let mut result = Value::Int(1);
    let mut b = base.clone();
    let mut e = exp;
    while e > 0 {
        if e & 1 == 1 {
            result = numeric_mul(&result, &b)?;
        }
        e >>= 1;
        if e > 0 {
            b = numeric_mul(&b, &b)?;
        }
    }
    Ok(result)
}

/// Check if expr is a linear polynomial in var: degree exactly 1.
fn classify_linear(expr: &Value, var: &str) -> Option<(Value, Value)> {
    let coeffs = poly_from_expr(expr, var).ok()?;
    if poly_degree(&coeffs) != 1 {
        return None;
    }
    Some((coeffs[1].clone(), coeffs[0].clone())) // (a, b) for ax+b
}

fn is_monomial(coeffs: &[Value]) -> bool {
    let deg = poly_degree(coeffs);
    coeffs.iter().take(deg).all(numeric_is_zero) && !numeric_is_zero(&coeffs[deg])
}

/// Recursive reduction for int x^n*(x^2+k)^(+/-1/2) dx.
fn integrate_xn_sqrt_reduction(n: usize, k: &Value, is_sqrt: bool, var: &str) -> WqResult<Value> {
    let x = Value::from_cas_var(var);
    let x_sq = cas_pow(x.clone(), Value::Int(2))?;
    let inner = cas_add(vec![x_sq, k.clone()])?;

    if n == 0 {
        let q = QuadInfo {
            a: Value::Int(1),
            shift: Value::Int(0),
            k: k.clone(),
        };
        return integrate_quadratic_root(&q, is_sqrt, var);
    }

    if n == 1 {
        return if is_sqrt {
            let pow = cas_pow(
                inner.clone(),
                Value::from_fraction_parts(BigInt::from(3), BigInt::from(2)),
            )?;
            simplify_cas_value(&cas_div(pow, Value::Int(3))?)
        } else {
            Ok(Value::from_cas_function(
                CasFunction::Sqrt,
                vec![inner.clone()],
            ))
        };
    }

    // Iterative reduction from base case up to n.
    let base = if n.is_multiple_of(2) {
        let q = QuadInfo {
            a: Value::Int(1),
            shift: Value::Int(0),
            k: k.clone(),
        };
        integrate_quadratic_root(&q, is_sqrt, var)?
    } else {
        if is_sqrt {
            let pow = cas_pow(
                inner.clone(),
                Value::from_fraction_parts(BigInt::from(3), BigInt::from(2)),
            )?;
            simplify_cas_value(&cas_div(pow, Value::Int(3))?)?
        } else {
            Value::from_cas_function(CasFunction::Sqrt, vec![inner.clone()])
        }
    };

    let pow_3_2 = cas_pow(
        inner.clone(),
        Value::from_fraction_parts(BigInt::from(3), BigInt::from(2)),
    )?;
    let sqrt_expr = Value::from_cas_function(CasFunction::Sqrt, vec![inner.clone()]);

    let start = if n.is_multiple_of(2) { 2 } else { 3 };
    let mut result = base;

    for m in (start..=n).step_by(2) {
        if is_sqrt {
            let x_n1 = cas_pow(x.clone(), Value::from_bigint(BigInt::from(m - 1)))?;
            let n_plus_2 = Value::from_bigint(BigInt::from(m + 2));
            let first = cas_div(cas_mul(vec![x_n1, pow_3_2.clone()])?, n_plus_2.clone())?;

            let ratio = Value::from_fraction_parts(BigInt::from(m - 1), BigInt::from(m + 2));
            let k_times_result = cas_mul(vec![k.clone(), result])?;
            let second = cas_mul(vec![ratio, k_times_result])?;

            result = simplify_cas_value(&cas_sub(first, second)?)?;
        } else {
            let x_n1 = cas_pow(x.clone(), Value::from_bigint(BigInt::from(m - 1)))?;
            let n_val = Value::from_bigint(BigInt::from(m));
            let first = cas_div(cas_mul(vec![x_n1, sqrt_expr.clone()])?, n_val.clone())?;

            let ratio = Value::from_fraction_parts(BigInt::from(m - 1), BigInt::from(m));
            let k_times_result = cas_mul(vec![k.clone(), result])?;
            let second = cas_mul(vec![ratio, k_times_result])?;

            result = simplify_cas_value(&cas_sub(first, second)?)?;
        }
    }

    Ok(result)
}

fn sqrt_value(value: &Value) -> Option<Value> {
    match value {
        Value::Int(n) if *n >= 0 => {
            let f = (*n as f64).sqrt();
            if (f - f.round()).abs() < 1e-12 {
                Some(Value::Int(f.round() as i64))
            } else if f.is_finite() {
                Some(Value::float(f))
            } else {
                None
            }
        }
        Value::Float(f) if **f >= 0.0 => {
            let sqrt = f.sqrt();
            if sqrt.is_finite() {
                Some(Value::float(sqrt))
            } else {
                None
            }
        }
        Value::BigInt(n) => {
            if let Some(i) = n.to_i64() {
                if i >= 0 {
                    let f = (i as f64).sqrt();
                    if (f - f.round()).abs() < 1e-12 {
                        Some(Value::Int(f.round() as i64))
                    } else if f.is_finite() {
                        Some(Value::float(f))
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        Value::Fraction(fr) => {
            let numer = fr.numer().to_f64()?;
            let denom = fr.denom().to_f64()?;
            if numer >= 0.0 && denom > 0.0 {
                let f = (numer / denom).sqrt();
                if (f - f.round()).abs() < 1e-12 {
                    Some(Value::Int(f.round() as i64))
                } else if f.is_finite() {
                    Some(Value::float(f))
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(op: CasOp, args: Vec<Value>) -> Value {
        Value::from_cas_op(op, args)
    }

    #[test]
    fn test_classify_x2_plus_1() {
        let x = Value::from_cas_var("x");
        let expr = cas_add(vec![cas_pow(x, Value::Int(2)).unwrap(), Value::Int(1)]).unwrap();
        let q = classify_quadratic(&expr, "x").unwrap();
        assert!(numeric_is_one(&q.a), "a should be 1, got {:?}", q.a);
        assert!(
            numeric_is_zero(&q.shift),
            "shift should be 0, got {:?}",
            q.shift
        );
        assert!(numeric_is_one(&q.k), "k should be 1, got {:?}", q.k);
    }

    #[test]
    fn test_classify_x2_minus_4() {
        let x = Value::from_cas_var("x");
        let expr = cas_sub(cas_pow(x, Value::Int(2)).unwrap(), Value::Int(4)).unwrap();
        let q = classify_quadratic(&expr, "x").unwrap();
        assert!(
            numeric_is_negative(&q.k),
            "k should be negative, got {:?}",
            q.k
        );
    }

    #[test]
    fn test_classify_general_quadratic() {
        // x^2 + 2x + 5 = (x+1)^2 + 4
        let x = Value::from_cas_var("x");
        let expr = cas_add(vec![
            cas_pow(x.clone(), Value::Int(2)).unwrap(),
            cas_mul(vec![Value::Int(2), x]).unwrap(),
            Value::Int(5),
        ])
        .unwrap();
        let q = classify_quadratic(&expr, "x").unwrap();
        assert!(
            numeric_is_negative(&q.shift),
            "shift should be -1, got {:?}",
            q.shift
        );
        // (x+1)^2 + 4: shift = -1, k = 4
        assert!(
            numeric_is_zero(&numeric_sub(&q.k, &Value::Int(4)).unwrap_or(Value::Int(1))),
            "k should be 4, got {:?}",
            q.k
        );
    }

    #[test]
    fn test_euler_sqrt_cancellation() {
        // Simulate exactly what euler_integrate_core does for 1/(x*sqrt(x^2+1))
        let var = "x";
        let a = Value::Int(1);
        let b = Value::Int(0);
        let c = Value::Int(1);
        let s = Value::Int(1);

        // Build the integrand: (x * sqrt(x^2+1))^(-1)
        let x = Value::from_cas_var(var);
        let orig_sqrt = build_sqrt_expr(&a, &b, &c, var).unwrap();
        let expr = op(
            CasOp::Power,
            vec![
                op(CasOp::Multiply, vec![x, orig_sqrt.clone()]),
                Value::Int(-1),
            ],
        );

        // Euler #1: x_t, sqrt_t, dx_dt, denom
        let t = Value::from_cas_var("--cas-euler-t");
        let two = Value::Int(2);
        let two_s = numeric_mul(&two, &s).unwrap();
        let denom = cas_sub(b.clone(), cas_mul(vec![two_s, t.clone()]).unwrap()).unwrap();
        let x_t = simplify_cas_value(
            &cas_div(
                cas_sub(cas_pow(t.clone(), Value::Int(2)).unwrap(), c.clone()).unwrap(),
                denom.clone(),
            )
            .unwrap(),
        )
        .unwrap();
        let sqrt_t = simplify_cas_value(
            &cas_add(vec![cas_mul(vec![s, x_t.clone()]).unwrap(), t.clone()]).unwrap(),
        )
        .unwrap();
        let dx_dt = simplify_cas_value(
            &cas_div(cas_mul(vec![two, sqrt_t.clone()]).unwrap(), denom.clone()).unwrap(),
        )
        .unwrap();

        // Step 1: replace sqrt
        let integrand_t = replace_sqrt_in_expr(&expr, &orig_sqrt, &sqrt_t);
        // Step 2: substitute x
        let integrand_t = substitute_expr(&integrand_t, var, &x_t).unwrap();
        // Step 3: replace sqrt_t with _s
        let s_var = Value::from_cas_var("--cas-sqrt-s");
        let integrand_s = replace_sqrt_in_expr(&integrand_t, &sqrt_t, &s_var);
        let dx_dt_s = replace_sqrt_in_expr(&dx_dt, &sqrt_t, &s_var);
        // Step 4: multiply and expand
        let _integrand_t =
            simplify_cas_value(&cas_mul(vec![integrand_s, dx_dt_s]).unwrap()).unwrap();
        let _integrand_t = crate::cas::expand_cas(&_integrand_t).unwrap();

        // Debug intermediate steps
        let step1 = replace_sqrt_in_expr(&expr, &orig_sqrt, &sqrt_t);
        let step1_str = step1.to_string();
        let step2 = substitute_expr(&step1, var, &x_t).unwrap();
        let step2_str = step2.to_string();
        let step3 = replace_sqrt_in_expr(&step2, &sqrt_t, &s_var);
        let step3_str = step3.to_string();
        let dx_dt_s = replace_sqrt_in_expr(&dx_dt, &sqrt_t, &s_var);
        let step4 =
            simplify_cas_value(&cas_mul(vec![step3.clone(), dx_dt_s.clone()]).unwrap()).unwrap();
        let step4_str = step4.to_string();
        let step5 = crate::cas::expand_cas(&step4).unwrap();
        let step5_str = step5.to_string();

        assert!(
            !step5_str.contains("--cas-sqrt-s"),
            "step1 (replace sqrt): {step1_str}\n\
             step2 (sub x): {step2_str}\n\
             step3 (replace sqrt_t with _s): {step3_str}\n\
             dx_dt_s: {dx_dt_s}\n\
             step4 (multiply): {step4_str}\n\
             step5 (expand): {step5_str}\n\
             _s should have cancelled"
        );
    }

    #[test]
    fn test_exact_half() {
        assert!(Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)).exact_half());
        assert!(!Value::Int(1).exact_half());
        assert!(Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)).exact_neg_half());
    }

    #[test]
    fn test_reciprocal_quartic_transform_nonzero_root() {
        // x^4 - x^3 has root 1. With x = 1 + 1/t, P4(x) = (t+1)^3/t^4.
        let poly = vec![
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(-1),
            Value::Int(1),
        ];
        let transformed = super::reciprocal_quartic_transform(&poly, &Value::Int(1))
            .expect("nonzero-root reciprocal transform should succeed");
        assert_eq!(
            transformed,
            vec![Value::Int(1), Value::Int(3), Value::Int(3), Value::Int(1)]
        );
    }

    // -- Square factor extraction tests --

    #[test]
    fn test_extract_square_factors_cubic_double_root() {
        // x^3+2x^2+x = x(x+1)^2 -> out=(x+1), in=x
        let poly = vec![Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(1)];
        let (out, inn) = super::extract_square_factors(&poly, "x").unwrap();
        // out = x+1 = [1, 1]
        assert_eq!(out, vec![Value::Int(1), Value::Int(1)]);
        // in = x = [0, 1]
        assert_eq!(inn, vec![Value::Int(0), Value::Int(1)]);
    }

    #[test]
    fn test_extract_square_factors_quartic_double_square() {
        // x^4+2x^3+x^2 = x^2(x+1)^2 -> out=x(x+1)=x^2+x=[0,1,1], in=1
        let poly = vec![
            Value::Int(0),
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(1),
        ];
        let (out, inn) = super::extract_square_factors(&poly, "x").unwrap();
        assert_eq!(out, vec![Value::Int(0), Value::Int(1), Value::Int(1)]);
        assert_eq!(inn, vec![Value::Int(1)]);
    }

    #[test]
    fn test_extract_square_factors_perfect_square_quartic() {
        // x^4+2x^2+1 = (x^2+1)^2 -> out=x^2+1=[1,0,1], in=1
        let poly = vec![
            Value::Int(1),
            Value::Int(0),
            Value::Int(2),
            Value::Int(0),
            Value::Int(1),
        ];
        let (out, inn) = super::extract_square_factors(&poly, "x").unwrap();
        assert_eq!(out, vec![Value::Int(1), Value::Int(0), Value::Int(1)]);
        assert_eq!(inn, vec![Value::Int(1)]);
    }

    #[test]
    fn test_extract_square_factors_cubic_no_square() {
        // x^3+1 -- no repeated factors -> out=1, in=x^3+1
        let poly = vec![Value::Int(1), Value::Int(0), Value::Int(0), Value::Int(1)];
        let (out, inn) = super::extract_square_factors(&poly, "x").unwrap();
        assert_eq!(out, vec![Value::Int(1)]);
        assert_eq!(inn, poly);
    }

    #[test]
    fn test_extract_square_factors_cubic_triple_root() {
        // x^3+3x^2+3x+1 = (x+1)^3 -> out=(x+1), in=(x+1)
        let poly = vec![Value::Int(1), Value::Int(3), Value::Int(3), Value::Int(1)];
        let (out, inn) = super::extract_square_factors(&poly, "x").unwrap();
        // square_free_factor: (x+1) with mult 3 -> out_pow=1, in_pow=1
        assert_eq!(out, vec![Value::Int(1), Value::Int(1)]); // x+1
        assert_eq!(inn, vec![Value::Int(1), Value::Int(1)]); // x+1
    }
}
