//! Integration of elliptic integrals -- sqrt(cubic) and sqrt(quartic)
//! that cannot be reduced to elementary form by square factor extraction.
//!
//! Handles:
//!   int sqrt(x^3 + a) dx  ->  algebraic part + first-kind elliptic integral
//!   int 1/sqrt(x^3 + a) dx  ->  first-kind elliptic integral

use crate::cas::{
    cas_add, cas_div, cas_mul, cas_pow, expand_expr, numeric_div, numeric_is_one, numeric_is_zero,
    numeric_mul, numeric_sub, poly_degree, poly_from_expr, simplify_cas_value,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

/// Strategy entry point: integrate elliptic integrals involving sqrt(cubic).
pub(super) fn integrate_elliptic(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let simplified = simplify_cas_value(expr)?;
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] elliptic enter: {}",
        simplified
            .format_cas()
            .unwrap_or_else(|| simplified.to_string())
    );

    let result = try_elliptic(&simplified, var);

    if let Ok(Some(ref val)) = result {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] elliptic exit: {}",
            val.format_cas().unwrap_or_else(|| val.to_string())
        );
    } else {
        cas_trace!(DebugLogFlags::CAS, "[cas] elliptic exit (not_elliptic)");
    }

    result
}

fn try_elliptic(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Case: sqrt(cubic) = (cubic)^(1/2) or (cubic)^(-1/2)
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && (exp.exact_half() || exp.exact_neg_half())
        && let Some(result) = try_cubic_reduction(base, var, exp.exact_half())?
    {
        return Ok(Some(result));
    }

    // Case: sqrt(cubic) as Call("sqrt", [cubic])
    if let Some((CasFunction::Sqrt, [arg])) = expr.cas_function_parts()
        && let Some(result) = try_cubic_reduction(arg, var, true)?
    {
        return Ok(Some(result));
    }

    Ok(None)
}

/// Check if `expr` is a cubic polynomial in `var`, and if so, reduce the
/// elliptic integral to standard forms.
///
/// `is_sqrt`: true for int sqrt(cubic) dx, false for int 1/sqrt(cubic) dx.
fn try_cubic_reduction(base: &Value, var: &str, is_sqrt: bool) -> WqResult<Option<Value>> {
    let expanded = expand_expr(base).unwrap_or_else(|_| base.clone());
    let poly = match poly_from_expr(&expanded, var) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };

    let deg = poly_degree(&poly);
    // Only handle degree 3 for now (quartic can be added later).
    if deg != 3 {
        return Ok(None);
    }

    // Extract coefficients of c3*x^3 + c2*x^2 + c1*x + c0.
    let (c3, c2, c1, c0) = cubic_coeffs(&poly);

    let Some(normalized) = shifted_binomial_cubic(var, &c3, &c2, &c1, &c0) else {
        return Ok(None);
    };

    let base_integral = if is_sqrt {
        integrate_sqrt_u3_plus_a(&normalized.u, &normalized.a)?
    } else {
        integrate_one_over_sqrt_u3_plus_a(&normalized.u, &normalized.a)?
    };
    let scale = if numeric_is_one(&normalized.leading) {
        Value::Int(1)
    } else {
        let exp = if is_sqrt {
            Value::from_fraction_parts(1u64.into(), 2u64.into())
        } else {
            Value::from_fraction_parts((-1i64).into(), 2u64.into())
        };
        cas_pow(normalized.leading, exp)?
    };

    simplify_cas_value(&cas_mul(vec![scale, base_integral])?).map(Some)
}

struct ShiftedBinomialCubic {
    /// `u` in `c3 * (u^3 + a)`, where `u = x + s`.
    u: Value,
    /// Constant term inside the normalized binomial cubic.
    a: Value,
    /// Leading scale `c3`.
    leading: Value,
}

fn numeric_values_equal(lhs: &Value, rhs: &Value) -> bool {
    numeric_sub(lhs, rhs).is_ok_and(|diff| numeric_is_zero(&diff))
}

/// Recognize `c3*x^3 + c2*x^2 + c1*x + c0` as `c3*((x+s)^3 + a)`.
///
/// This is intentionally conservative: if coefficient arithmetic is not exact
/// numeric arithmetic, the shape is rejected and the integrator falls through.
fn shifted_binomial_cubic(
    var: &str,
    c3: &Value,
    c2: &Value,
    c1: &Value,
    c0: &Value,
) -> Option<ShiftedBinomialCubic> {
    if numeric_is_zero(c3) {
        return None;
    }

    let three = Value::Int(3);
    let twenty_seven = Value::Int(27);
    let c2_sq = numeric_mul(c2, c2).ok()?;
    let three_c3 = numeric_mul(&three, c3).ok()?;
    let expected_c1 = numeric_div(&c2_sq, &three_c3).ok()?;
    if !numeric_values_equal(c1, &expected_c1) {
        return None;
    }

    let c2_cubed = numeric_mul(&c2_sq, c2).ok()?;
    let c3_sq = numeric_mul(c3, c3).ok()?;
    let denom = numeric_mul(&twenty_seven, &c3_sq).ok()?;
    let shifted_constant = numeric_div(&c2_cubed, &denom).ok()?;
    let q = numeric_sub(c0, &shifted_constant).ok()?;
    let a = numeric_div(&q, c3).ok()?;

    let x = Value::from_cas_var(var);
    let shift = numeric_div(c2, &three_c3).ok()?;
    let u = if numeric_is_zero(&shift) {
        x
    } else {
        cas_add(vec![x, shift]).ok()?
    };

    Some(ShiftedBinomialCubic {
        u,
        a,
        leading: c3.clone(),
    })
}

/// int sqrt(u^3 + a) du
///
/// Reduction: int y du = (2/5)*u*y + (3a/5)*int du/y
/// where y = sqrt(u^3 + a).
fn integrate_sqrt_u3_plus_a(u: &Value, a: &Value) -> WqResult<Value> {
    let two = Value::Int(2);
    let three = Value::Int(3);
    let five = Value::Int(5);

    if numeric_is_zero(a) {
        return cas_mul(vec![
            cas_div(two, five)?,
            cas_pow(
                u.clone(),
                Value::from_fraction_parts(5u64.into(), 2u64.into()),
            )?,
        ]);
    }

    // y = (u^3 + a)^(1/2)
    let u3 = cas_pow(u.clone(), Value::Int(3))?;
    let cubic = cas_add(vec![u3, a.clone()])?;
    let y = cas_pow(
        cubic.clone(),
        Value::from_fraction_parts(1u64.into(), 2u64.into()),
    )?;

    // Algebraic part: (2/5)*u*y
    let alg_part = cas_mul(vec![
        cas_div(two.clone(), five.clone())?,
        u.clone(),
        y.clone(),
    ])?;

    // Elliptic part: (3a/5)*int du/y
    let first_kind = integrate_one_over_sqrt_u3_plus_a(u, a)?;
    let coeff = cas_mul(vec![cas_div(three, five)?, a.clone(), first_kind])?;
    simplify_cas_value(&cas_add(vec![alg_part, coeff])?)
}

/// int du/sqrt(u^3 + a)
///
/// For a = 1, using the Legendre reduction for a cubic with one real root:
///
///   int dx/sqrt(x^3+1) = 3^(-1/4) * F(arccos((sqrt(3)-1-x)/(sqrt(3)+1+x)), (2+sqrt(3))/4)
///
/// For general a, substitute u = a^(1/3)*z:
///   int du/sqrt(u^3+a) = a^(-1/6) * int dz/sqrt(z^3+1)
///
/// So: int du/sqrt(u^3+a) = 3^(-1/4)*a^(-1/6) * F(arccos((sqrt(3)-1-z)/(sqrt(3)+1+z)), k^2)
/// where z = u/a^(1/3), k^2 = (2+sqrt(3))/4.
fn integrate_one_over_sqrt_u3_plus_a(u: &Value, a: &Value) -> WqResult<Value> {
    let one = Value::Int(1);
    let two = Value::Int(2);
    let three = Value::Int(3);
    let four = Value::Int(4);

    if numeric_is_zero(a) {
        return cas_mul(vec![
            Value::Int(-2),
            cas_pow(
                u.clone(),
                Value::from_fraction_parts((-1i64).into(), 2u64.into()),
            )?,
        ]);
    }

    let normalized_u = if numeric_is_one(a) {
        u.clone()
    } else {
        let scale = cas_pow(
            a.clone(),
            Value::from_fraction_parts(1u64.into(), 3u64.into()),
        )?;
        cas_div(u.clone(), scale)?
    };

    // Build sqrt(3) = 3^(1/2)
    let sqrt3 = cas_pow(
        three.clone(),
        Value::from_fraction_parts(1u64.into(), 2u64.into()),
    )?;

    // Build k^2 = (2 + sqrt(3)) / 4
    let k2 = cas_div(cas_add(vec![two.clone(), sqrt3.clone()])?, four.clone())?;

    // cos phi = (sqrt(3) - 1 - z) / (sqrt(3) + 1 + z)
    let neg_x = cas_mul(vec![Value::Int(-1), normalized_u.clone()])?;
    let cos_phi = cas_div(
        cas_add(vec![sqrt3.clone(), Value::Int(-1), neg_x])?,
        cas_add(vec![sqrt3.clone(), one, normalized_u])?,
    )?;

    // phi = arccos(cos_phi)
    let phi = Value::from_cas_function(CasFunction::ArcCos, vec![cos_phi]);

    // First-kind elliptic integral: ellik[phi; k^2]
    let ellik_part = Value::from_cas_function(CasFunction::EllIk, vec![phi, k2]);

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

    simplify_cas_value(&result)
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
