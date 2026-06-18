//! Trigonometric integration via reduction formulas and direct polynomial
//! arithmetic.
//!
//! # Stack-overflow prevention
//!
//! Early versions of this module called `integrate_expr_with_depth` on the
//! algebraically reduced integrand (e.g. after substituting u = cos x).  That
//! produced unbounded recursion because the pipeline would re-enter the trig
//! strategy on cos / sin terms in the reduced form.
//!
//! **Rule:** this module must **never** call `integrate_expr_with_depth`.
//! Instead every reduction is carried to completion with coefficient-vector
//! arithmetic (`expand_binomial_poly` + `integrate_poly_coeffs` for odd-power
//! substitutions, recurrence relations for even-power and product reductions).
//!
//! When adding new patterns, follow the same discipline — expand the integrand
//! into monomials, integrate each term by hand, and substitute back.

use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::cas::{
    cas_add, cas_div, cas_mul, cas_neg, cas_pow, cas_product, cas_sub, eval_exact_numeric_div,
    numeric_add, numeric_is_zero, numeric_mul, numeric_sub, poly_trim, simplify_cas_value,
    substitute_expr,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

/// Strategy entry point: integrate trigonometric expressions.
pub(super) fn integrate_by_trig(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    if !contains_trig(expr) {
        return Ok(None);
    }
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] trig enter: {}",
        expr.format_cas().unwrap_or_else(|| expr.to_string())
    );
    let simplified = simplify_cas_value(expr)?;
    if let Some(result) = try_single_fn_power(&simplified, CasFunction::Sin, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (sin_power): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_single_fn_power(&simplified, CasFunction::Cos, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (cos_power): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_single_fn_power(&simplified, CasFunction::Tan, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (tan_power): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_single_fn_power(&simplified, CasFunction::Sec, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (sec_power): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_single_fn_power(&simplified, CasFunction::Csc, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (csc_power): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_single_fn_power(&simplified, CasFunction::Cot, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (cot_power): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_sin_cos_product(&simplified, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (sin_cos_product): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    if let Some(result) = try_product_to_sum(&simplified, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] trig exit (product_to_sum): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }
    cas_trace!(DebugLogFlags::CAS, "[cas] trig exit (no_match)");
    Ok(None)
}

/// Check if expression contains any trigonometric function call.
fn contains_trig(expr: &Value) -> bool {
    if let Some((name, _)) = expr.cas_function_parts()
        && matches!(
            name,
            CasFunction::Sin
                | CasFunction::Cos
                | CasFunction::Tan
                | CasFunction::Sec
                | CasFunction::Csc
                | CasFunction::Cot
                | CasFunction::Sinh
                | CasFunction::Cosh
                | CasFunction::Tanh
        )
    {
        return true;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        for arg in args {
            if contains_trig(arg) {
                return true;
            }
        }
    }
    if let Some((_, args)) = expr.cas_function_parts() {
        for arg in args {
            if contains_trig(arg) {
                return true;
            }
        }
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        for arg in args {
            if contains_trig(arg) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Result of matching fn(arg)^n: (power_n, coeff_a, offset_b)
/// where arg = a*var + b.  For fn(var) → (n, 1, 0).
type TrigMatch = (usize, Value, Value);

/// Try to match `fn_name(arg)^n` or `fn_name(arg)`.  Returns `(n, a, b)` such
/// that arg = a*var + b, or `None` if the expression doesn't match.
fn match_fn_power(expr: &Value, fn_name: CasFunction, var: &str) -> Option<TrigMatch> {
    // Helper: extract linear coefficients from an argument
    let extract_ab = |arg: &Value| -> Option<(Value, Value)> {
        if arg.cas_var_name() == Some(var) {
            return Some((Value::Int(1), Value::Int(0)));
        }
        // a*var + b
        if let Some((CasOp::Add, sum_args)) = arg.cas_op_parts() {
            let mut a: Option<Value> = None;
            let mut b = Value::Int(0);
            for sa in sum_args {
                if let Some(ca) = as_linear_monomial(sa, var) {
                    if a.is_some() {
                        return None;
                    }
                    a = Some(ca);
                } else if !sa.is_cas_expr() {
                    b = numeric_add(&b, sa).ok()?;
                } else {
                    return None;
                }
            }
            return Some((a.unwrap_or(Value::Int(1)), b));
        }
        // a*var (no offset)
        if let Some(ca) = as_linear_monomial(arg, var) {
            return Some((ca, Value::Int(0)));
        }
        None
    };

    // Case: fn(arg)
    if let Some((name, args)) = expr.cas_function_parts()
        && name == fn_name
        && args.len() == 1
    {
        let (a, b) = extract_ab(&args[0])?;
        return Some((1, a, b));
    }

    // Case: fn(arg)^n
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && let Some((name, args)) = base.cas_function_parts()
        && name == fn_name
        && args.len() == 1
        && let Some(n) = exp.exact_int()
        && let Some(n_usize) = n.to_usize()
    {
        let (a, b) = extract_ab(&args[0])?;
        return Some((n_usize, a, b));
    }

    None
}

/// If `expr` is `c * var` (exactly one variable factor, possibly with a
/// numeric coefficient), return `c`.  Otherwise `None`.
fn as_linear_monomial(expr: &Value, var: &str) -> Option<Value> {
    if expr.cas_var_name() == Some(var) {
        return Some(Value::Int(1));
    }
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let mut coeff = Value::Int(1);
        let mut found_var = false;
        for arg in args {
            if arg.cas_var_name() == Some(var) {
                if found_var {
                    return None;
                }
                found_var = true;
            } else if !arg.is_cas_expr() {
                coeff = numeric_mul(&coeff, arg).ok()?;
            } else {
                return None;
            }
        }
        if found_var {
            return Some(coeff);
        }
    }
    None
}

/// Build the argument expression `a*var + b` as a CAS Value.
fn build_linear_arg(a: &Value, b: &Value, var: &str) -> Value {
    if numeric_is_zero(b) {
        if a == &Value::Int(1) {
            return Value::from_cas_var(var);
        }
        return cas_mul(vec![a.clone(), Value::from_cas_var(var)])
            .unwrap_or_else(|_| Value::from_cas_var(var));
    }
    let var_part = if a == &Value::Int(1) {
        Value::from_cas_var(var)
    } else {
        cas_mul(vec![a.clone(), Value::from_cas_var(var)])
            .unwrap_or_else(|_| Value::from_cas_var(var))
    };
    cas_add(vec![var_part, b.clone()]).unwrap_or_else(|_| Value::from_cas_var(var))
}

/// Build a function call: fn_name(a*var + b).
fn build_fn_call(fn_name: CasFunction, a: &Value, b: &Value, var: &str) -> Value {
    Value::from_cas_function(fn_name, vec![build_linear_arg(a, b, var)])
}

fn cas_ln_abs(arg: Value) -> Value {
    Value::from_cas_function(
        CasFunction::Ln,
        vec![Value::from_cas_function(CasFunction::Abs, vec![arg])],
    )
}

// ---------------------------------------------------------------------------
// Single-function power dispatch
// ---------------------------------------------------------------------------

fn try_single_fn_power(expr: &Value, fn_name: CasFunction, var: &str) -> WqResult<Option<Value>> {
    let (n, a, b) = match match_fn_power(expr, fn_name, var) {
        Some(p) => p,
        None => return Ok(None),
    };
    if n == 0 {
        return Ok(Some(Value::from_cas_var(var)));
    }
    match fn_name {
        CasFunction::Sin => {
            if n % 2 == 1 {
                integrate_sin_odd(n, &a, &b, var).map(Some)
            } else {
                integrate_sin_reduction(n, &a, &b, var).map(Some)
            }
        }
        CasFunction::Cos => {
            if n % 2 == 1 {
                integrate_cos_odd(n, &a, &b, var).map(Some)
            } else {
                integrate_cos_reduction(n, &a, &b, var).map(Some)
            }
        }
        CasFunction::Tan => integrate_tan_power(n, &a, &b, var).map(Some),
        CasFunction::Sec => integrate_sec_power(n, &a, &b, var).map(Some),
        CasFunction::Csc => integrate_csc_power(n, &a, &b, var).map(Some),
        CasFunction::Cot => integrate_cot_power(n, &a, &b, var).map(Some),
        _ => Ok(None),
    }
}

// ---------------------------------------------------------------------------
// Binomial expansion helpers
// ---------------------------------------------------------------------------

pub(super) fn binomial_coeff(n: usize, k: usize) -> i64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    let mut result = 1i64;
    for i in 0..k {
        result = result * (n - i) as i64 / (i + 1) as i64;
    }
    result
}

/// Expand (1 - u²)^k with an overall sign factor.
/// Returns coefficient vector: Σ sign * (-1)^j * C(k,j) * u^(2j)
fn expand_binomial_poly(k: usize, sign: i64) -> Vec<Value> {
    let mut result = vec![Value::Int(0); 2 * k + 1];
    for j in 0..=k {
        let mut coeff = binomial_coeff(k, j);
        if j % 2 == 1 {
            coeff = -coeff;
        }
        if sign == -1 {
            coeff = -coeff;
        }
        result[2 * j] = Value::from_bigint(BigInt::from(coeff));
    }
    poly_trim(&mut result);
    result
}

/// Integrate a coefficient vector (in dummy variable `_u`) and substitute
/// `_u → fn_name(a*var + b)`.  The whole result is divided by `div_a`.
fn integrate_poly_coeffs_with_sub(
    coeffs: &[Value],
    replace_with: CasFunction,
    a: &Value,
    b: &Value,
    var: &str,
    div_a: bool,
) -> WqResult<Value> {
    let mut terms = Vec::new();
    for (deg, coeff) in coeffs.iter().enumerate() {
        if numeric_is_zero(coeff) {
            continue;
        }
        let new_deg = deg + 1;
        let denom = Value::from_bigint(BigInt::from(new_deg));
        let mut c = eval_exact_numeric_div(coeff, &denom)?;
        // Divide by a if this came from a du = a·... substitution
        if div_a {
            c = eval_exact_numeric_div(&c, a)?;
        }
        let monomial = if new_deg == 1 {
            Value::from_cas_var("--cas-trig-u")
        } else {
            cas_pow(
                Value::from_cas_var("--cas-trig-u"),
                Value::from_bigint(BigInt::from(new_deg)),
            )?
        };
        let term = if c == Value::Int(1) || c == Value::BigInt(Arc::new(BigInt::from(1))) {
            monomial
        } else {
            cas_mul(vec![c, monomial])?
        };
        let sub_target = build_fn_call(replace_with, a, b, var);
        let term = substitute_expr(&term, "--cas-trig-u", &sub_target)?;
        terms.push(term);
    }
    if terms.is_empty() {
        return Ok(Value::Int(0));
    }
    if terms.len() == 1 {
        return Ok(terms.into_iter().next().expect("single term"));
    }
    simplify_cas_value(&cas_add(terms)?)
}

// ---------------------------------------------------------------------------
// sin^n(a·x + b)
// ---------------------------------------------------------------------------

/// ∫ sin^n(ax+b) dx where n is odd.  Substitute u = cos(ax+b), du =
/// -a·sin(ax+b)dx.
fn integrate_sin_odd(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    let k = (n - 1) / 2;
    // ∫ sin^n dx = -(1/a)·∫ (1-u²)^k du   with u = cos(ax+b)
    let coeffs = expand_binomial_poly(k, -1);
    integrate_poly_coeffs_with_sub(&coeffs, CasFunction::Cos, a, b, var, true)
}

/// ∫ sin^n(ax+b) dx using reduction (works for both even and odd n):
///   ∫ sin^n = -cos·sin^(n-1)/(a·n) + (n-1)/n·∫ sin^(n-2)
fn integrate_sin_reduction(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if n == 0 {
        return Ok(Value::from_cas_var(var));
    }
    if n == 1 {
        let cos = build_fn_call(CasFunction::Cos, a, b, var);
        return simplify_cas_value(&cas_div(cas_neg(cos)?, a.clone())?);
    }
    let sin = build_fn_call(CasFunction::Sin, a, b, var);
    let cos = build_fn_call(CasFunction::Cos, a, b, var);
    let mut result = if n.is_multiple_of(2) {
        Value::from_cas_var(var)
    } else {
        simplify_cas_value(&cas_div(cas_neg(cos.clone())?, a.clone())?)?
    };
    let start = if n.is_multiple_of(2) { 2 } else { 3 };
    for m in (start..=n).step_by(2) {
        let sin_m1 = if m == 2 {
            sin.clone()
        } else {
            cas_pow(sin.clone(), Value::from_bigint(BigInt::from(m - 1)))?
        };
        let m_val = Value::from_bigint(BigInt::from(m));
        let a_m = numeric_mul(a, &m_val)?;
        let first = simplify_cas_value(&cas_div(cas_mul(vec![cos.clone(), sin_m1])?, a_m)?)?;
        let ratio = Value::from_fraction_parts(BigInt::from(m - 1), BigInt::from(m));
        result = simplify_cas_value(&cas_sub(cas_mul(vec![ratio, result])?, first)?)?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// cos^n(a·x + b)
// ---------------------------------------------------------------------------

/// ∫ cos^n(ax+b) dx where n is odd.  Substitute u = sin(ax+b), du =
/// a·cos(ax+b)dx.
fn integrate_cos_odd(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    let k = (n - 1) / 2;
    // ∫ cos^n dx = (1/a)·∫ (1-u²)^k du   with u = sin(ax+b)
    let coeffs = expand_binomial_poly(k, 1);
    integrate_poly_coeffs_with_sub(&coeffs, CasFunction::Sin, a, b, var, true)
}

/// ∫ cos^n(ax+b) dx using reduction:
///   ∫ cos^n = sin·cos^(n-1)/(a·n) + (n-1)/n·∫ cos^(n-2)
fn integrate_cos_reduction(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if n == 0 {
        return Ok(Value::from_cas_var(var));
    }
    if n == 1 {
        let sin = build_fn_call(CasFunction::Sin, a, b, var);
        return simplify_cas_value(&cas_div(sin, a.clone())?);
    }
    let sin = build_fn_call(CasFunction::Sin, a, b, var);
    let cos = build_fn_call(CasFunction::Cos, a, b, var);
    let mut result = if n.is_multiple_of(2) {
        Value::from_cas_var(var)
    } else {
        simplify_cas_value(&cas_div(sin.clone(), a.clone())?)?
    };
    let start = if n.is_multiple_of(2) { 2 } else { 3 };
    for m in (start..=n).step_by(2) {
        let cos_m1 = if m == 2 {
            cos.clone()
        } else {
            cas_pow(cos.clone(), Value::from_bigint(BigInt::from(m - 1)))?
        };
        let m_val = Value::from_bigint(BigInt::from(m));
        let a_m = numeric_mul(a, &m_val)?;
        let first = simplify_cas_value(&cas_div(cas_mul(vec![sin.clone(), cos_m1])?, a_m)?)?;
        let ratio = Value::from_fraction_parts(BigInt::from(m - 1), BigInt::from(m));
        result = simplify_cas_value(&cas_add(vec![first, cas_mul(vec![ratio, result])?])?)?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// tan^n(a·x + b)
// ---------------------------------------------------------------------------

/// ∫ tan^n(ax+b) dx using reduction: tan^(n-1)/(a·(n-1)) - ∫ tan^(n-2)
fn integrate_tan_power(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if n == 0 {
        return Ok(Value::from_cas_var(var));
    }
    if n == 1 {
        let cos = build_fn_call(CasFunction::Cos, a, b, var);
        let ln = cas_ln_abs(cos);
        return simplify_cas_value(&cas_div(cas_neg(ln)?, a.clone())?);
    }
    let tan = build_fn_call(CasFunction::Tan, a, b, var);
    let mut result = if n.is_multiple_of(2) {
        Value::from_cas_var(var)
    } else {
        let cos = build_fn_call(CasFunction::Cos, a, b, var);
        let ln = cas_ln_abs(cos);
        simplify_cas_value(&cas_div(cas_neg(ln)?, a.clone())?)?
    };
    let start = if n.is_multiple_of(2) { 2 } else { 3 };
    for m in (start..=n).step_by(2) {
        let tan_pow = cas_pow(tan.clone(), Value::from_bigint(BigInt::from(m - 1)))?;
        let denom = numeric_mul(a, &Value::from_bigint(BigInt::from(m - 1)))?;
        let term = cas_div(tan_pow, denom)?;
        result = simplify_cas_value(&cas_sub(term, result)?)?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// sec^n(a·x + b)
// ---------------------------------------------------------------------------

/// ∫ sec^n(ax+b) dx
fn integrate_sec_power(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if n == 0 {
        return Ok(Value::from_cas_var(var));
    }
    if n == 1 {
        let sec = build_fn_call(CasFunction::Sec, a, b, var);
        let tan = build_fn_call(CasFunction::Tan, a, b, var);
        let sum = cas_add(vec![sec, tan])?;
        let ln = cas_ln_abs(sum);
        return simplify_cas_value(&cas_div(ln, a.clone())?);
    }
    if n == 2 {
        let tan = build_fn_call(CasFunction::Tan, a, b, var);
        return simplify_cas_value(&cas_div(tan, a.clone())?);
    }
    let sec = build_fn_call(CasFunction::Sec, a, b, var);
    let tan = build_fn_call(CasFunction::Tan, a, b, var);
    let mut result = if n.is_multiple_of(2) {
        let tan = build_fn_call(CasFunction::Tan, a, b, var);
        simplify_cas_value(&cas_div(tan, a.clone())?)?
    } else {
        let sec = build_fn_call(CasFunction::Sec, a, b, var);
        let tan = build_fn_call(CasFunction::Tan, a, b, var);
        let sum = cas_add(vec![sec, tan])?;
        let ln = cas_ln_abs(sum);
        simplify_cas_value(&cas_div(ln, a.clone())?)?
    };
    let start = if n.is_multiple_of(2) { 4 } else { 3 };
    for m in (start..=n).step_by(2) {
        let sec_pow = cas_pow(sec.clone(), Value::from_bigint(BigInt::from(m - 2)))?;
        let num = cas_mul(vec![sec_pow, tan.clone()])?;
        let a_n1 = numeric_mul(a, &Value::from_bigint(BigInt::from(m - 1)))?;
        let first = cas_div(num, a_n1)?;
        let ratio = Value::from_fraction_parts(BigInt::from(m - 2), BigInt::from(m - 1));
        result = simplify_cas_value(&cas_add(vec![first, cas_mul(vec![ratio, result])?])?)?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// csc^n(a·x + b)
// ---------------------------------------------------------------------------

/// ∫ csc^n(ax+b) dx — mirror of sec^n with sign adjustments.
fn integrate_csc_power(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if n == 0 {
        return Ok(Value::from_cas_var(var));
    }
    if n == 1 {
        let csc = build_fn_call(CasFunction::Csc, a, b, var);
        let cot = build_fn_call(CasFunction::Cot, a, b, var);
        let sum = cas_add(vec![csc, cot])?;
        let ln = cas_ln_abs(sum);
        return simplify_cas_value(&cas_div(cas_neg(ln)?, a.clone())?);
    }
    if n == 2 {
        let cot = build_fn_call(CasFunction::Cot, a, b, var);
        return simplify_cas_value(&cas_div(cas_neg(cot)?, a.clone())?);
    }
    let csc = build_fn_call(CasFunction::Csc, a, b, var);
    let cot = build_fn_call(CasFunction::Cot, a, b, var);
    let mut result = if n.is_multiple_of(2) {
        let cot = build_fn_call(CasFunction::Cot, a, b, var);
        simplify_cas_value(&cas_div(cas_neg(cot)?, a.clone())?)?
    } else {
        let csc = build_fn_call(CasFunction::Csc, a, b, var);
        let cot = build_fn_call(CasFunction::Cot, a, b, var);
        let sum = cas_add(vec![csc, cot])?;
        let ln = cas_ln_abs(sum);
        simplify_cas_value(&cas_div(cas_neg(ln)?, a.clone())?)?
    };
    let start = if n.is_multiple_of(2) { 4 } else { 3 };
    for m in (start..=n).step_by(2) {
        let csc_pow = cas_pow(csc.clone(), Value::from_bigint(BigInt::from(m - 2)))?;
        let num = cas_mul(vec![csc_pow, cot.clone()])?;
        let a_n1 = numeric_mul(a, &Value::from_bigint(BigInt::from(m - 1)))?;
        let first = cas_div(cas_neg(num)?, a_n1)?;
        let ratio = Value::from_fraction_parts(BigInt::from(m - 2), BigInt::from(m - 1));
        result = simplify_cas_value(&cas_add(vec![first, cas_mul(vec![ratio, result])?])?)?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// cot^n(a·x + b)
// ---------------------------------------------------------------------------

/// ∫ cot^n(ax+b) dx using reduction: -cot^(n-1)/(a·(n-1)) - ∫ cot^(n-2)
fn integrate_cot_power(n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if n == 0 {
        return Ok(Value::from_cas_var(var));
    }
    if n == 1 {
        let sin = build_fn_call(CasFunction::Sin, a, b, var);
        let ln = cas_ln_abs(sin);
        return simplify_cas_value(&cas_div(ln, a.clone())?);
    }
    let cot = build_fn_call(CasFunction::Cot, a, b, var);
    let mut result = if n.is_multiple_of(2) {
        Value::from_cas_var(var)
    } else {
        let sin = build_fn_call(CasFunction::Sin, a, b, var);
        let ln = cas_ln_abs(sin);
        simplify_cas_value(&cas_div(ln, a.clone())?)?
    };
    let start = if n.is_multiple_of(2) { 2 } else { 3 };
    for m in (start..=n).step_by(2) {
        let cot_pow = cas_pow(cot.clone(), Value::from_bigint(BigInt::from(m - 1)))?;
        let denom = numeric_mul(a, &Value::from_bigint(BigInt::from(m - 1)))?;
        let term = cas_div(cot_pow, denom)?;
        result = simplify_cas_value(&cas_sub(cas_neg(term)?, result)?)?;
    }
    Ok(result)
}

// ---------------------------------------------------------------------------
// sin^m(ax+b) · cos^n(ax+b)
// ---------------------------------------------------------------------------

fn try_sin_cos_product(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some((CasOp::Multiply, args)) = expr.cas_op_parts() else {
        return Ok(None);
    };

    let mut sin_match: Option<TrigMatch> = None;
    let mut cos_match: Option<TrigMatch> = None;
    let mut other_factors: Vec<Value> = Vec::new();

    for arg in args {
        if sin_match.is_none()
            && let Some(m) = match_fn_power(arg, CasFunction::Sin, var)
        {
            sin_match = Some(m);
            continue;
        }
        if cos_match.is_none()
            && let Some(m) = match_fn_power(arg, CasFunction::Cos, var)
        {
            cos_match = Some(m);
            continue;
        }
        if !arg.is_cas_expr() {
            other_factors.push(arg.clone());
        } else {
            return Ok(None);
        }
    }

    let (sm, sa, sb) = match sin_match {
        Some(m) => m,
        None => return Ok(None),
    };
    let (cn, ca, cb) = match cos_match {
        Some(m) => m,
        None => return Ok(None),
    };

    // Both sin and cos must have the same argument
    if sa != ca || sb != cb {
        return Ok(None);
    }

    let result = integrate_sin_cos(sm, cn, &sa, &sb, var)?;
    if other_factors.is_empty() {
        return Ok(Some(result));
    }
    let coeff = cas_product(other_factors.to_vec());
    Ok(Some(simplify_cas_value(&cas_mul(vec![coeff, result])?)?))
}

fn integrate_sin_cos(m: usize, n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    if m % 2 == 1 {
        return integrate_sin_odd_cos(m, n, a, b, var);
    }
    if n % 2 == 1 {
        return integrate_sin_cos_odd(m, n, a, b, var);
    }
    integrate_sin_cos_both_even(m, n, a, b, var)
}

/// ∫ sin^m cos^n dx where m is odd. u = cos(ax+b), du = -a·sin(ax+b)dx.
fn integrate_sin_odd_cos(m: usize, n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    let k = (m - 1) / 2;
    let binom = expand_binomial_poly(k, -1); // - sign for sin→cos substitution
    // Multiply by u^n
    let mut coeffs = vec![Value::Int(0); binom.len() + n];
    for (i, c) in binom.iter().enumerate() {
        coeffs[i + n] = c.clone();
    }
    poly_trim(&mut coeffs);
    integrate_poly_coeffs_with_sub(&coeffs, CasFunction::Cos, a, b, var, true)
}

/// ∫ sin^m cos^n dx where n is odd. u = sin(ax+b), du = a·cos(ax+b)dx.
fn integrate_sin_cos_odd(m: usize, n: usize, a: &Value, b: &Value, var: &str) -> WqResult<Value> {
    let k = (n - 1) / 2;
    let binom = expand_binomial_poly(k, 1);
    let mut coeffs = vec![Value::Int(0); binom.len() + m];
    for (i, c) in binom.iter().enumerate() {
        coeffs[i + m] = c.clone();
    }
    poly_trim(&mut coeffs);
    integrate_poly_coeffs_with_sub(&coeffs, CasFunction::Sin, a, b, var, true)
}

/// ∫ sin^m cos^n dx where both even. Reduction decreases m by 2 each step.
fn integrate_sin_cos_both_even(
    m: usize,
    n: usize,
    a: &Value,
    b: &Value,
    var: &str,
) -> WqResult<Value> {
    if m == 0 {
        return integrate_cos_reduction(n, a, b, var);
    }
    if n == 0 {
        return integrate_sin_reduction(m, a, b, var);
    }

    let sin = build_fn_call(CasFunction::Sin, a, b, var);
    let cos = build_fn_call(CasFunction::Cos, a, b, var);
    let mut result = integrate_cos_reduction(n, a, b, var)?;

    for mm in (2..=m).step_by(2) {
        let sin_m1 = if mm == 2 {
            sin.clone()
        } else {
            cas_pow(sin.clone(), Value::from_bigint(BigInt::from(mm - 1)))?
        };
        let cos_n1 = if n == 0 {
            Value::Int(1)
        } else {
            cas_pow(cos.clone(), Value::from_bigint(BigInt::from(n + 1)))?
        };
        let sum = Value::from_bigint(BigInt::from(mm + n));
        let a_sum = numeric_mul(a, &sum)?;
        let first = cas_div(cas_mul(vec![sin_m1, cos_n1])?, a_sum)?;

        let ratio = Value::from_fraction_parts(BigInt::from(mm - 1), BigInt::from(mm + n));
        result = simplify_cas_value(&cas_sub(cas_mul(vec![ratio, result])?, first)?)?;
    }

    Ok(result)
}

// ---------------------------------------------------------------------------
// Product-to-sum
// ---------------------------------------------------------------------------

fn try_product_to_sum(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some((CasOp::Multiply, args)) = expr.cas_op_parts() else {
        return Ok(None);
    };
    let mut fn_args: Vec<(CasFunction, &Value)> = Vec::new();
    let mut other_factors: Vec<Value> = Vec::new();
    for arg in args {
        if let Some((name, inner)) = arg.cas_function_parts()
            && inner.len() == 1
            && matches!(name, CasFunction::Sin | CasFunction::Cos)
        {
            fn_args.push((name, &inner[0]));
        } else if !arg.is_cas_expr() {
            other_factors.push(arg.clone());
        } else {
            return Ok(None);
        }
    }
    if fn_args.len() != 2 {
        return Ok(None);
    }
    let (fn1, arg1) = fn_args[0];
    let (fn2, arg2) = fn_args[1];
    let result = match (fn1, fn2) {
        (CasFunction::Sin, CasFunction::Cos) => product_sin_cos(arg1, arg2, var)?,
        (CasFunction::Cos, CasFunction::Sin) => product_sin_cos(arg2, arg1, var)?,
        (CasFunction::Sin, CasFunction::Sin) => product_sin_sin(arg1, arg2, var)?,
        (CasFunction::Cos, CasFunction::Cos) => product_cos_cos(arg1, arg2, var)?,
        _ => return Ok(None),
    };
    if other_factors.is_empty() {
        return Ok(Some(result));
    }
    Ok(Some(simplify_cas_value(&cas_mul(vec![
        cas_product(other_factors.to_vec()),
        result,
    ])?)?))
}

fn product_sin_cos(arg1: &Value, arg2: &Value, var: &str) -> WqResult<Value> {
    let a = extract_coeff(arg1, var);
    let b = extract_coeff(arg2, var);
    let half = Value::from_fraction_parts(BigInt::from(1), BigInt::from(2));
    let a_plus_b = numeric_add(&a, &b)?;
    let a_minus_b = numeric_sub(&a, &b)?;
    let t1 = Value::from_cas_function(
        CasFunction::Sin,
        vec![cas_mul(vec![a_plus_b, Value::from_cas_var(var)])?],
    );
    let t2 = Value::from_cas_function(
        CasFunction::Sin,
        vec![cas_mul(vec![a_minus_b, Value::from_cas_var(var)])?],
    );
    integrate_simple_linear_trig(&cas_mul(vec![half, cas_add(vec![t1, t2])?])?, var)
}

fn product_sin_sin(arg1: &Value, arg2: &Value, var: &str) -> WqResult<Value> {
    let a = extract_coeff(arg1, var);
    let b = extract_coeff(arg2, var);
    let half = Value::from_fraction_parts(BigInt::from(1), BigInt::from(2));
    let a_plus_b = numeric_add(&a, &b)?;
    let a_minus_b = numeric_sub(&a, &b)?;
    let t1 = Value::from_cas_function(
        CasFunction::Cos,
        vec![cas_mul(vec![a_minus_b, Value::from_cas_var(var)])?],
    );
    let t2 = Value::from_cas_function(
        CasFunction::Cos,
        vec![cas_mul(vec![a_plus_b, Value::from_cas_var(var)])?],
    );
    integrate_simple_linear_trig(&cas_mul(vec![half, cas_sub(t1, t2)?])?, var)
}

fn product_cos_cos(arg1: &Value, arg2: &Value, var: &str) -> WqResult<Value> {
    let a = extract_coeff(arg1, var);
    let b = extract_coeff(arg2, var);
    let half = Value::from_fraction_parts(BigInt::from(1), BigInt::from(2));
    let a_plus_b = numeric_add(&a, &b)?;
    let a_minus_b = numeric_sub(&a, &b)?;
    let t1 = Value::from_cas_function(
        CasFunction::Cos,
        vec![cas_mul(vec![a_plus_b, Value::from_cas_var(var)])?],
    );
    let t2 = Value::from_cas_function(
        CasFunction::Cos,
        vec![cas_mul(vec![a_minus_b, Value::from_cas_var(var)])?],
    );
    integrate_simple_linear_trig(&cas_mul(vec![half, cas_add(vec![t1, t2])?])?, var)
}

fn integrate_simple_linear_trig(expr: &Value, var: &str) -> WqResult<Value> {
    if let Some((CasOp::Add, args)) = expr.cas_op_parts() {
        let mut terms = Vec::new();
        for arg in args {
            terms.push(integrate_simple_linear_trig(arg, var)?);
        }
        return simplify_cas_value(&cas_add(terms)?);
    }
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let mut coeff = Value::Int(1);
        let mut fn_part = None;
        for arg in args {
            if !arg.is_cas_expr() {
                coeff = numeric_mul(&coeff, arg)?;
            } else {
                fn_part = Some(arg);
            }
        }
        if let Some(fp) = fn_part {
            let integrated = integrate_simple_linear_trig(fp, var)?;
            return simplify_cas_value(&cas_mul(vec![coeff, integrated])?);
        }
    }
    if let Some((name, args)) = expr.cas_function_parts()
        && args.len() == 1
        && matches!(name, CasFunction::Sin | CasFunction::Cos)
    {
        let arg = &args[0];
        let a = extract_coeff(arg, var);
        let result = match name {
            CasFunction::Sin => cas_neg(Value::from_cas_function(
                CasFunction::Cos,
                vec![arg.clone()],
            ))?,
            CasFunction::Cos => Value::from_cas_function(CasFunction::Sin, vec![arg.clone()]),
            _ => unreachable!(),
        };
        if a == Value::Int(1) {
            return Ok(result);
        }
        return simplify_cas_value(&cas_div(result, a)?);
    }
    Ok(expr.clone())
}

fn extract_coeff(expr: &Value, var: &str) -> Value {
    if expr.cas_var_name() == Some(var) {
        return Value::Int(1);
    }
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        for arg in args {
            if !arg.is_cas_expr() {
                return arg.clone();
            }
        }
    }
    Value::Int(1)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn op(op: CasOp, args: Vec<Value>) -> Value {
        Value::from_cas_op(op, args)
    }

    fn call(function: CasFunction, args: Vec<Value>) -> Value {
        Value::from_cas_function(function, args)
    }

    #[test]
    fn test_match_fn_power_simple() {
        let expr = call(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        let (n, a, b) = match_fn_power(&expr, CasFunction::Sin, "x").unwrap();
        assert_eq!(n, 1);
        assert_eq!(a, Value::Int(1));
        assert_eq!(b, Value::Int(0));
    }

    #[test]
    fn test_match_fn_power_power() {
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
                Value::Int(3),
            ],
        );
        let (n, _, _) = match_fn_power(&expr, CasFunction::Sin, "x").unwrap();
        assert_eq!(n, 3);
    }

    #[test]
    fn test_match_fn_power_linear() {
        // sin(2*x)
        let arg = op(
            CasOp::Multiply,
            vec![Value::Int(2), Value::from_cas_var("x")],
        );
        let expr = call(CasFunction::Sin, vec![arg]);
        let (n, a, b) = match_fn_power(&expr, CasFunction::Sin, "x").unwrap();
        assert_eq!(n, 1);
        assert_eq!(a, Value::Int(2));
        assert_eq!(b, Value::Int(0));
    }

    #[test]
    fn test_match_fn_power_linear_offset() {
        // sin(2*x + 1)
        let arg = op(
            CasOp::Add,
            vec![
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
                Value::Int(1),
            ],
        );
        let expr = call(CasFunction::Sin, vec![arg]);
        let (n, a, b) = match_fn_power(&expr, CasFunction::Sin, "x").unwrap();
        assert_eq!(n, 1);
        assert_eq!(a, Value::Int(2));
        assert_eq!(b, Value::Int(1));
    }

    #[test]
    fn test_match_fn_power_rejects_nonlinear() {
        // sin(x^2) — not a*x+b
        let arg = op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]);
        let expr = call(CasFunction::Sin, vec![arg]);
        assert!(match_fn_power(&expr, CasFunction::Sin, "x").is_none());
    }

    #[test]
    fn test_match_fn_power_sec() {
        let expr = op(
            CasOp::Power,
            vec![
                call(CasFunction::Sec, vec![Value::from_cas_var("x")]),
                Value::Int(2),
            ],
        );
        let (n, a, b) = match_fn_power(&expr, CasFunction::Sec, "x").unwrap();
        assert_eq!(n, 2);
        assert_eq!(a, Value::Int(1));
        assert_eq!(b, Value::Int(0));
    }

    #[test]
    fn test_integrate_sin_odd_direct() {
        let result = integrate_sin_odd(3, &Value::Int(1), &Value::Int(0), "x").unwrap();
        let s = result.to_string();
        assert!(s.contains("cos"), "expected cos: {s}");
    }

    #[test]
    fn test_integrate_cos_odd_direct() {
        let result = integrate_cos_odd(3, &Value::Int(1), &Value::Int(0), "x").unwrap();
        let s = result.to_string();
        assert!(s.contains("sin"), "expected sin: {s}");
    }

    #[test]
    fn test_integrate_sec_squared() {
        // ∫ sec²(x) dx = tan(x)
        let result = integrate_sec_power(2, &Value::Int(1), &Value::Int(0), "x").unwrap();
        let s = result.to_string();
        assert!(s.contains("tan[x]"), "expected tan[x]: {s}");
    }

    #[test]
    fn test_integrate_csc_squared() {
        // ∫ csc²(x) dx = -cot(x)
        let result = integrate_csc_power(2, &Value::Int(1), &Value::Int(0), "x").unwrap();
        let s = result.to_string();
        assert!(s.contains("cot[x]"), "expected cot[x]: {s}");
    }

    #[test]
    fn test_integrate_cot() {
        // ∫ cot(x) dx = ln|sin(x)|
        let result = integrate_cot_power(1, &Value::Int(1), &Value::Int(0), "x").unwrap();
        let s = result.to_string();
        assert!(s.contains("ln"), "expected ln: {s}");
        assert!(s.contains("sin"), "expected sin: {s}");
    }

    #[test]
    fn test_integrate_sec_cubed() {
        // ∫ sec³(x) dx via reduction
        let result = integrate_sec_power(3, &Value::Int(1), &Value::Int(0), "x").unwrap();
        let s = result.to_string();
        assert!(
            s.contains("tan") || s.contains("sec"),
            "expected tan/sec: {s}"
        );
    }
}
