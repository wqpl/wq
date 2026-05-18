//! Integration of elliptic integrals — sqrt(cubic) and sqrt(quartic)
//! that cannot be reduced to elementary form by square factor extraction.
//!
//! Handles:
//!   ∫ sqrt(x³ + a) dx  →  algebraic part + first-kind elliptic integral
//!   ∫ 1/sqrt(x³ + a) dx  →  first-kind elliptic integral

use crate::cas::{
    cas_add, cas_div, cas_mul, cas_pow, numeric_is_one, numeric_is_zero, poly_degree,
    poly_from_expr, simplify_cas_value,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::{Value, WqResult};

/// Strategy entry point: integrate elliptic integrals involving sqrt(cubic).
pub(super) fn integrate_elliptic(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let simplified = simplify_cas_value(expr)?;
    let expr_fmt = simplified
        .format_cas()
        .unwrap_or_else(|| simplified.to_string());
    cas_trace!(DebugLogFlags::CAS, "[cas] elliptic enter: {expr_fmt}");

    let result = try_elliptic(&simplified, var);

    if let Ok(Some(ref val)) = result {
        let val_fmt = val.format_cas().unwrap_or_else(|| val.to_string());
        cas_trace!(DebugLogFlags::CAS, "[cas] elliptic exit: {val_fmt}");
    } else {
        cas_trace!(DebugLogFlags::CAS, "[cas] elliptic exit (not_elliptic)");
    }

    result
}

fn try_elliptic(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Case: sqrt(cubic) = (cubic)^(1/2) or (cubic)^(-1/2)
    if let Some(("^", [base, exp])) = expr.cas_op_parts()
        && (exp.exact_half() || exp.exact_neg_half())
        && let Some(result) = try_cubic_reduction(base, var, exp.exact_half())?
    {
        return Ok(Some(result));
    }

    // Case: sqrt(cubic) as Call("sqrt", [cubic])
    if let Some(("sqrt", [arg])) = expr.cas_call_parts()
        && let Some(result) = try_cubic_reduction(arg, var, true)?
    {
        return Ok(Some(result));
    }

    Ok(None)
}

/// Check if `expr` is a cubic polynomial in `var`, and if so, reduce the
/// elliptic integral to standard forms.
///
/// `is_sqrt`: true for ∫ sqrt(cubic) dx, false for ∫ 1/sqrt(cubic) dx.
fn try_cubic_reduction(base: &Value, var: &str, is_sqrt: bool) -> WqResult<Option<Value>> {
    let poly = match poly_from_expr(base, var) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };

    let deg = poly_degree(&poly);
    // Only handle degree 3 for now (quartic can be added later).
    if deg != 3 {
        return Ok(None);
    }

    // Extract coefficients of c3·x³ + c2·x² + c1·x + c0.
    let (c3, c2, c1, c0) = cubic_coeffs(&poly);

    // Check if it's of the form x³ + a (monic, no x² term, no x term).
    let is_simple_cubic = numeric_is_one(&c3) && numeric_is_zero(&c2) && numeric_is_zero(&c1);
    if !is_simple_cubic {
        return Ok(None);
    }

    // We have y² = x³ + c0.
    let a = c0;

    if is_sqrt {
        integrate_sqrt_x3_plus_a(var, &a)
    } else {
        integrate_one_over_sqrt_x3_plus_a(var, &a)
    }
}

/// ∫ √(x³ + a) dx
///
/// Reduction: ∫ y dx = (2/5)·x·y + (3a/5)·∫ dx/y
/// where y = √(x³ + a).
fn integrate_sqrt_x3_plus_a(var: &str, a: &Value) -> WqResult<Option<Value>> {
    let x = Value::from_cas_var(var);
    let two = Value::Int(2);
    let three = Value::Int(3);
    let five = Value::Int(5);

    // y = (x³ + a)^(1/2)
    let x3 = cas_pow(x.clone(), Value::Int(3))?;
    let cubic = cas_add(vec![x3, a.clone()])?;
    let y = cas_pow(
        cubic.clone(),
        Value::from_fraction_parts(1u64.into(), 2u64.into()),
    )?;

    // Algebraic part: (2/5)·x·y
    let alg_part = cas_mul(vec![
        cas_div(two.clone(), five.clone())?,
        x.clone(),
        y.clone(),
    ])?;

    // Elliptic part: (3a/5)·∫ dx/y
    let first_kind = integrate_one_over_sqrt_x3_plus_a(var, a)?;

    match first_kind {
        Some(ik) => {
            let coeff = cas_mul(vec![cas_div(three, five)?, a.clone(), ik])?;
            Ok(Some(simplify_cas_value(&cas_add(vec![alg_part, coeff])?)?))
        }
        None => {
            // If we can't do the first-kind integral, return the algebraic part.
            Ok(Some(alg_part))
        }
    }
}

/// ∫ dx/√(x³ + a)
///
/// For a = 1, using the Legendre reduction for a cubic with one real root:
///
///   ∫ dx/√(x³+1) = 3^(-1/4) · F(arccos((√3-1-x)/(√3+1+x)), (2+√3)/4)
///
/// For general a, substitute x = a^(1/3)·u:
///   ∫ dx/√(x³+a) = a^(-1/6) · ∫ du/√(u³+1)
///
/// So: ∫ dx/√(x³+a) = 3^(-1/4)·a^(-1/6) · F(arccos((√3-1-u)/(√3+1+u)), k²)
/// where u = x/a^(1/3), k² = (2+√3)/4.
fn integrate_one_over_sqrt_x3_plus_a(var: &str, a: &Value) -> WqResult<Option<Value>> {
    let x = Value::from_cas_var(var);
    let one = Value::Int(1);
    let two = Value::Int(2);
    let three = Value::Int(3);
    let four = Value::Int(4);

    // Build √3 = 3^(1/2)
    let sqrt3 = cas_pow(
        three.clone(),
        Value::from_fraction_parts(1u64.into(), 2u64.into()),
    )?;

    // Build k² = (2 + √3) / 4
    let k2 = cas_div(cas_add(vec![two.clone(), sqrt3.clone()])?, four.clone())?;

    // cos φ = (√3 - 1 - x) / (√3 + 1 + x)
    let neg_x = Value::from_cas_op("*", vec![Value::Int(-1), x.clone()]);
    let cos_phi = cas_div(
        cas_add(vec![sqrt3.clone(), Value::Int(-1), neg_x])?,
        cas_add(vec![sqrt3.clone(), one, x.clone()])?,
    )?;

    // φ = arccos(cos_phi)
    let phi = Value::from_cas_call("arccos", vec![cos_phi]);

    // First-kind elliptic integral: ellik[φ; k²]
    let ellik_part = Value::from_cas_call("ellik", vec![phi, k2]);

    // Scale factor: 3^(-1/4)
    let scale_3 = cas_pow(
        three,
        Value::from_fraction_parts((-1i64).into(), 4u64.into()),
    )?;

    // For general a: additional scale factor a^(-1/6)
    let result = if numeric_is_one(a) {
        cas_mul(vec![scale_3, ellik_part])?
    } else {
        let scale_a = cas_pow(
            a.clone(),
            Value::from_fraction_parts((-1i64).into(), 6u64.into()),
        )?;
        cas_mul(vec![scale_3, scale_a, ellik_part])?
    };

    Ok(Some(simplify_cas_value(&result)?))
}

/// Returns (c3, c2, c1, c0) where coeff[i] = coefficient of x^i.
fn cubic_coeffs(poly: &[Value]) -> (Value, Value, Value, Value) {
    let zero = Value::Int(0);
    let c3 = poly.get(3).cloned().unwrap_or_else(|| zero.clone());
    let c2 = poly.get(2).cloned().unwrap_or_else(|| zero.clone());
    let c1 = poly.get(1).cloned().unwrap_or_else(|| zero.clone());
    let c0 = poly.first().cloned().unwrap_or_else(|| zero.clone());
    (c3, c2, c1, c0)
}
