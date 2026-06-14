//! Integration of polynomial × exponential: ∫ P(x)·e^(k·x) dx via
//! undetermined coefficients.
//!
//! The ansatz ∫ P(x)·e^(k·x) dx = e^(k·x) · R(x)  with deg(R) = deg(P)
//! leads to the differential equation  R'(x) + k·R(x) = P(x).
//!
//! Expanding both sides as coefficient vectors and matching from the
//! highest degree down gives a simple back-substitution (no linear-system
//! solve needed), so this is fast and exact.

use num_bigint::BigInt;

use crate::cas::{
    cas_mul, cas_product, eval_exact_numeric_div, numeric_mul, numeric_sub, poly_degree,
    poly_from_expr, poly_to_expr, poly_trim, simplify_cas_value,
};
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

/// Strategy entry point: integrate P(x)·e^(k·x).
///
/// Returns `Some(result)` on success, `None` if the expression is not a
/// polynomial × exponential form.
pub(super) fn integrate_exp_poly(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let simplified = simplify_cas_value(expr)?;

    // Case 1: pure exp(k·x) — delegate to table strategy (already handled)
    if let Some((name, _)) = simplified.cas_function_parts()
        && name == CasFunction::Exp
    {
        return Ok(None);
    }

    // Case 2: P(x) * exp(k·x)
    let Some((CasOp::Multiply, args)) = simplified.cas_op_parts() else {
        return Ok(None);
    };

    // Separate polynomial factor and exp factor
    let mut poly_factors: Vec<Value> = Vec::new();
    let mut exp_arg: Option<Value> = None;
    let mut numeric_coeff: Option<Value> = None;

    for arg in args {
        if let Some((name, inner)) = arg.cas_function_parts()
            && name == CasFunction::Exp
            && inner.len() == 1
        {
            if exp_arg.is_some() {
                return Ok(None); // two exp factors — not this pattern
            }
            exp_arg = Some(inner[0].clone());
        } else if arg.is_cas_expr() {
            // Check if it's a polynomial in var
            if poly_from_expr(arg, var).is_ok() {
                poly_factors.push(arg.clone());
            } else {
                return Ok(None); // non-polynomial, non-exp factor
            }
        } else {
            // Numeric constant
            numeric_coeff = Some(match numeric_coeff.take() {
                Some(acc) => numeric_mul(&acc, arg)?,
                None => arg.clone(),
            });
        }
    }

    let exp_arg = match exp_arg {
        Some(a) => a,
        None => return Ok(None),
    };

    // Build polynomial and extract its coefficients
    let poly_expr = cas_product(poly_factors);

    let poly_coeffs = match poly_from_expr(&poly_expr, var) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    // Degenerate case: constant polynomial — delegate to table
    let deg = poly_degree(&poly_coeffs);
    if deg == 0 && poly_coeffs[0] == Value::Int(0) {
        return Ok(None);
    }
    if deg == 0 {
        // Constant × exp(kx) → handled by table; just integrate exp(kx)
        return Ok(None);
    }

    // Extract the exponential coefficient k from exp_arg = k·x
    // exp_arg should be a*x (linear, no constant term)
    let Some(k) = extract_linear_coeff(&exp_arg, var) else {
        return Ok(None);
    };

    // Solve R'(x) + k·R(x) = P(x) by back-substitution
    let r = solve_undetermined_coeffs(&poly_coeffs, &k)?;

    // Build result: coeff * e^(k·x) * R(x)
    let exp_factor = Value::from_cas_function(CasFunction::Exp, vec![exp_arg]);
    let r_expr = poly_to_expr(&r, var)?;

    // Multiply by numeric coefficient if present
    let mut result = cas_mul(vec![exp_factor, r_expr])?;
    if let Some(c) = numeric_coeff {
        result = cas_mul(vec![c, result])?;
    }

    simplify_cas_value(&result).map(Some)
}

/// Solve for R such that R' + k·R = P using back-substitution from highest
/// degree.
///
/// For deg(P) = n, let R = r₀ + r₁x + ... + rₙxⁿ.
/// R' + k·R = (kr₀ + r₁) + (kr₁ + 2r₂)x + ... + (kr_{n-1} + nrₙ)xⁿ⁻¹ + (krₙ)xⁿ
///
/// Match with P from xⁿ down:
///   rₙ = pₙ/k
///   r_{n-1} = (p_{n-1} - n·rₙ)/k
///   ...
///   rⱼ = (pⱼ - (j+1)·r_{j+1})/k
fn solve_undetermined_coeffs(p: &[Value], k: &Value) -> WqResult<Vec<Value>> {
    let n = poly_degree(p);
    let mut r = vec![Value::Int(0); n + 1];

    for j in (0..=n).rev() {
        let p_j = p.get(j).cloned().unwrap_or(Value::Int(0));

        let correction = if j < n {
            numeric_mul(&Value::from_bigint(BigInt::from(j + 1)), &r[j + 1])?
        } else {
            Value::Int(0)
        };

        // rⱼ = (pⱼ - (j+1)·r_{j+1}) / k
        let numer = numeric_sub(&p_j, &correction)?;
        r[j] = eval_exact_numeric_div(&numer, k)?;
    }

    poly_trim(&mut r);
    Ok(r)
}

/// Check if expr is a simple linear expression k·x (no constant term).
/// Returns Some(k) if so.
fn extract_linear_coeff(expr: &Value, var: &str) -> Option<Value> {
    // Case: just x → k = 1
    if expr.cas_var_name() == Some(var) {
        return Some(Value::Int(1));
    }

    // Case: k * x
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let mut coeff = Value::Int(1);
        let mut has_var = false;
        for arg in args {
            if arg.cas_var_name() == Some(var) {
                if has_var {
                    return None; // x appears twice
                }
                has_var = true;
            } else if !arg.is_cas_expr() {
                coeff = numeric_mul(&coeff, arg).ok()?;
            } else {
                return None; // non-linear sub-expression
            }
        }
        if has_var {
            return Some(coeff);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(op: CasOp, args: Vec<Value>) -> Value {
        Value::from_cas_op(op, args)
    }

    #[test]
    fn test_extract_linear_coeff_simple() {
        let expr = Value::from_cas_var("x");
        assert_eq!(extract_linear_coeff(&expr, "x"), Some(Value::Int(1)));

        let expr = op(
            CasOp::Multiply,
            vec![Value::Int(3), Value::from_cas_var("x")],
        );
        assert_eq!(extract_linear_coeff(&expr, "x"), Some(Value::Int(3)));
    }

    #[test]
    fn test_extract_linear_coeff_rejects_offset() {
        let expr = op(
            CasOp::Add,
            vec![
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
                Value::Int(1),
            ],
        );
        assert!(extract_linear_coeff(&expr, "x").is_none());
    }

    #[test]
    fn test_solve_linear() {
        // P(x) = x, k = 1 → R(x) = x - 1
        let p = vec![Value::Int(0), Value::Int(1)];
        let r = solve_undetermined_coeffs(&p, &Value::Int(1)).unwrap();
        assert_eq!(r, vec![Value::Int(-1), Value::Int(1)]);

        // P(x) = 1, k = 2 → R(x) = 1/2
        let p = vec![Value::Int(1)];
        let r = solve_undetermined_coeffs(&p, &Value::Int(2)).unwrap();
        assert_eq!(
            r,
            vec![Value::from_fraction_parts(BigInt::from(1), BigInt::from(2))]
        );
    }

    #[test]
    fn test_solve_quadratic() {
        // P(x) = x², k = 1 → R(x) = x² - 2x + 2
        let p = vec![Value::Int(0), Value::Int(0), Value::Int(1)];
        let r = solve_undetermined_coeffs(&p, &Value::Int(1)).unwrap();
        assert_eq!(r, vec![Value::Int(2), Value::Int(-2), Value::Int(1)]);
    }
}
