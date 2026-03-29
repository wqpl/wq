use num_bigint::BigInt;
use num_traits::ToPrimitive;

use super::{
    cas_add, cas_err, cas_pow, contains_cas_var, eval_exact_numeric_div, eval_numeric_binary,
    numeric_is_negative, numeric_is_one, numeric_is_zero, rebuild_scaled_term, simplify_cas_value,
};
use crate::value::{Value, WqResult};

pub(crate) fn extract_linear_coefficients(expr: &Value, var: &str) -> Option<(Value, Value)> {
    if expr.cas_var_name() == Some(var) {
        return Some((Value::Int(1), Value::Int(0)));
    }

    if let Some(("*", args)) = expr.cas_op_parts() {
        let mut numeric = Value::Int(1);
        let mut symbolic = None;
        for arg in args {
            if !arg.is_cas_expr() {
                numeric = eval_numeric_binary("*", &numeric, arg).ok()?;
            } else if symbolic.is_some() {
                return None;
            } else {
                symbolic = Some(arg);
            }
        }
        let sym = symbolic?;
        if sym.cas_var_name() == Some(var) {
            return Some((numeric, Value::Int(0)));
        }
        let (a, b) = extract_linear_coefficients(sym, var)?;
        return Some((
            eval_numeric_binary("*", &numeric, &a).ok()?,
            eval_numeric_binary("*", &numeric, &b).ok()?,
        ));
    }

    if let Some(("+", args)) = expr.cas_op_parts() {
        let mut a = Value::Int(1);
        let mut b = Value::Int(0);
        let mut found_var = false;

        for arg in args {
            if arg.cas_var_name() == Some(var) {
                if found_var {
                    return None;
                }
                found_var = true;
            } else if let Some(("*", inner_args)) = arg.cas_op_parts() {
                let mut coeff = Value::Int(1);
                let mut has_var = false;
                for inner in inner_args {
                    if inner.cas_var_name() == Some(var) {
                        if has_var {
                            return None;
                        }
                        has_var = true;
                    } else if !inner.is_cas_expr() {
                        coeff = eval_numeric_binary("*", &coeff, inner).ok()?;
                    } else {
                        return None;
                    }
                }
                if has_var {
                    if found_var {
                        return None;
                    }
                    a = coeff;
                    found_var = true;
                } else {
                    b = eval_numeric_binary("+", &b, arg).ok()?;
                }
            } else if !arg.is_cas_expr() {
                b = eval_numeric_binary("+", &b, arg).ok()?;
            } else {
                return None;
            }
        }

        if found_var {
            return Some((a, b));
        }
    }

    None
}

pub(crate) fn poly_add(lhs: &[Value], rhs: &[Value]) -> WqResult<Vec<Value>> {
    let size = lhs.len().max(rhs.len());
    let mut out = vec![Value::Int(0); size];
    for (idx, slot) in out.iter_mut().enumerate() {
        let left = lhs.get(idx).unwrap_or(&Value::Int(0));
        let right = rhs.get(idx).unwrap_or(&Value::Int(0));
        *slot = eval_numeric_binary("+", left, right)?;
    }
    poly_trim(&mut out);
    Ok(out)
}

pub(crate) fn poly_mul(lhs: &[Value], rhs: &[Value]) -> WqResult<Vec<Value>> {
    let mut out = vec![Value::Int(0); lhs.len() + rhs.len().saturating_sub(1)];
    for i in 0..lhs.len() {
        for j in 0..rhs.len() {
            if numeric_is_zero(&lhs[i]) || numeric_is_zero(&rhs[j]) {
                continue;
            }
            let term = eval_numeric_binary("*", &lhs[i], &rhs[j])?;
            out[i + j] = eval_numeric_binary("+", &out[i + j], &term)?;
        }
    }
    poly_trim(&mut out);
    Ok(out)
}

pub(crate) fn poly_neg(coeffs: &[Value]) -> Vec<Value> {
    coeffs
        .iter()
        .map(|c| eval_numeric_binary("*", c, &Value::Int(-1)).expect("numeric neg"))
        .collect()
}

pub(crate) fn poly_scalar_mul(coeffs: &[Value], scalar: &Value) -> WqResult<Vec<Value>> {
    coeffs
        .iter()
        .map(|c| eval_numeric_binary("*", c, scalar))
        .collect()
}

pub(crate) fn poly_sub(lhs: &[Value], rhs: &[Value]) -> WqResult<Vec<Value>> {
    poly_add(lhs, &poly_neg(rhs))
}

pub(crate) fn poly_derivative(coeffs: &[Value]) -> Vec<Value> {
    if coeffs.len() <= 1 {
        return vec![Value::Int(0)];
    }
    let mut deriv = Vec::with_capacity(coeffs.len() - 1);
    for (i, coeff) in coeffs.iter().enumerate().skip(1) {
        deriv.push(
            eval_numeric_binary("*", coeff, &Value::from_bigint(BigInt::from(i)))
                .expect("numeric derivative coeff"),
        );
    }
    poly_trim(&mut deriv);
    deriv
}

pub(crate) fn poly_evaluate(coeffs: &[Value], x: &Value) -> WqResult<Value> {
    let mut result = Value::Int(0);
    for coeff in coeffs.iter().rev() {
        result = eval_numeric_binary("*", &result, x)?;
        result = eval_numeric_binary("+", &result, coeff)?;
    }
    Ok(result)
}

pub(crate) fn poly_gcd(a: &[Value], b: &[Value]) -> WqResult<Vec<Value>> {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    poly_trim(&mut a);
    poly_trim(&mut b);
    if poly_is_zero(&a) {
        return Ok(b);
    }
    if poly_is_zero(&b) {
        return Ok(a);
    }
    while !poly_is_zero(&b) {
        let (_, r) = poly_divide(&a, &b)?;
        a = b;
        b = r;
        poly_trim(&mut b);
    }
    poly_trim(&mut a);
    if !a.is_empty() {
        let lc = a.last().expect("non-empty poly").clone();
        if numeric_is_negative(&lc) {
            a = poly_neg(&a);
        }
        let lc = a.last().expect("non-empty poly").clone();
        if !numeric_is_one(&lc) {
            let scale = eval_exact_numeric_div(&Value::Int(1), &lc)?;
            a = poly_scalar_mul(&a, &scale)?;
        }
    }
    Ok(a)
}

/// Compute the resultant of two polynomials using the subresultant PRS
/// algorithm.
///
/// The resultant is a scalar that vanishes iff the two polynomials share a
/// common root.  Coefficients must be numeric; symbolic coefficients are not
/// supported.
pub(crate) fn poly_resultant(a: &[Value], b: &[Value]) -> WqResult<Value> {
    let mut a = a.to_vec();
    let mut b = b.to_vec();
    poly_trim(&mut a);
    poly_trim(&mut b);

    if poly_is_zero(&a) || poly_is_zero(&b) {
        return Ok(Value::Int(0));
    }

    let deg_a = poly_degree(&a);
    let deg_b = poly_degree(&b);

    // Ensure deg(a) >= deg(b)
    if deg_a < deg_b {
        let sign = if (deg_a * deg_b) % 2 == 1 {
            -1i64
        } else {
            1i64
        };
        let r = poly_resultant(&b, &a)?;
        return eval_numeric_binary("*", &Value::Int(sign), &r);
    }

    let lc_b = b[deg_b].clone();

    // Base case: b is constant
    if deg_b == 0 {
        // resultant(a, c) = c^deg(a)
        if deg_a == 0 {
            return Ok(Value::Int(1));
        }
        let mut result = lc_b.clone();
        for _ in 1..deg_a {
            result = eval_numeric_binary("*", &result, &lc_b)?;
        }
        return Ok(result);
    }

    let (_, rem) = poly_divide(&a, &b)?;
    let deg_rem = poly_degree(&rem);

    // resultant(a, b) = (-1)^(deg_a·deg_b) · lc_b^(deg_a - deg_rem) · resultant(b,
    // rem)
    let sign = if (deg_a * deg_b) % 2 == 1 {
        -1i64
    } else {
        1i64
    };
    let mut factor = Value::Int(sign);

    let exp_diff = deg_a - deg_rem;
    for _ in 0..exp_diff {
        factor = eval_numeric_binary("*", &factor, &lc_b)?;
    }

    let inner = poly_resultant(&b, &rem)?;
    eval_numeric_binary("*", &factor, &inner)
}

/// Lagrange interpolation: given points (x_i, y_i), return the polynomial
/// P(z) of degree ≤ n-1 such that P(x_i) = y_i for all i.
///
/// Points are (x, y) pairs.  Returns coefficients [c0, c1, ..., cn] where
/// c0 + c1·z + ... + cn·z^n is the interpolating polynomial.
pub(crate) fn poly_interpolate(points: &[(Value, Value)]) -> WqResult<Vec<Value>> {
    if points.is_empty() {
        return Ok(vec![Value::Int(0)]);
    }
    // Build Lagrange basis: P(z) = Σ y_i · Π_{j≠i} (z - x_j) / (x_i - x_j)
    let mut result = vec![Value::Int(0)];
    for (i, (xi, yi)) in points.iter().enumerate() {
        // Build numerator: Π_{j≠i} (z - x_j)
        let mut numer = vec![Value::Int(1)]; // degree-0 polynomial = 1
        for (j, (xj, _)) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            // Multiply numer by (z - xj) = [-xj, 1]
            let factor = vec![
                eval_numeric_binary("*", xj, &Value::Int(-1))?,
                Value::Int(1),
            ];
            numer = poly_mul(&numer, &factor)?;
        }
        // Compute denominator: Π_{j≠i} (x_i - x_j)
        let mut denom = Value::Int(1);
        for (j, (xj, _)) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            denom = eval_numeric_binary("*", &denom, &eval_numeric_binary("-", xi, xj)?)?;
        }
        // Scale numerator by y_i / denom
        let scale = eval_exact_numeric_div(yi, &denom)?;
        let term = poly_scalar_mul(&numer, &scale)?;
        result = poly_add(&result, &term)?;
    }
    poly_trim(&mut result);
    Ok(result)
}

pub(crate) fn poly_trim(coeffs: &mut Vec<Value>) {
    while coeffs.len() > 1 && coeffs.last().is_some_and(numeric_is_zero) {
        coeffs.pop();
    }
}

pub(crate) fn poly_degree(coeffs: &[Value]) -> usize {
    coeffs
        .iter()
        .rposition(|coeff| !numeric_is_zero(coeff))
        .unwrap_or(0)
}

pub(crate) fn poly_is_zero(coeffs: &[Value]) -> bool {
    coeffs.iter().all(numeric_is_zero)
}

/// Yun's square-free factorization.
/// Returns `(factor, multiplicity)` pairs where each factor is square-free
/// and pairwise coprime, and the original polynomial = ∏ factor_i^i.
pub(crate) fn square_free_factor(poly: &[Value]) -> WqResult<Vec<(Vec<Value>, usize)>> {
    if poly_is_zero(poly) {
        return Ok(Vec::new());
    }
    let mut poly = poly.to_vec();
    poly_trim(&mut poly);

    let mut deriv = poly_derivative(&poly);
    poly_trim(&mut deriv);

    let g = poly_gcd(&poly, &deriv)?;
    if poly_is_zero(&g) {
        return Err(cas_err("square_free_factor: unexpected zero gcd"));
    }

    let (mut s, _) = poly_divide(&poly, &g)?;
    let (mut t, _) = poly_divide(&deriv, &g)?;

    let mut result = Vec::new();
    let mut i: usize = 1;

    while poly_degree(&s) > 0 {
        let s_deriv = poly_derivative(&s);
        let h = poly_sub(&t, &s_deriv)?;
        let g = poly_gcd(&s, &h)?;
        if poly_degree(&g) > 0 {
            result.push((g.clone(), i));
        }
        let (next_s, _) = poly_divide(&s, &g)?;
        let (next_t, _) = poly_divide(&h, &g)?;
        s = next_s;
        t = next_t;
        i += 1;
    }

    Ok(result)
}

pub(crate) fn poly_to_expr(coeffs: &[Value], var: &str) -> WqResult<Value> {
    let mut terms = Vec::new();
    for (degree, coeff) in coeffs.iter().enumerate() {
        if numeric_is_zero(coeff) {
            continue;
        }
        let core = match degree {
            0 => None,
            1 => Some(Value::from_cas_var(var)),
            _ => Some(cas_pow(
                Value::from_cas_var(var),
                Value::from_bigint(BigInt::from(degree)),
            )?),
        };
        terms.push(rebuild_scaled_term(coeff.clone(), core)?);
    }
    cas_add(terms)
}

pub(crate) fn poly_divide(numer: &[Value], denom: &[Value]) -> WqResult<(Vec<Value>, Vec<Value>)> {
    let mut numer = numer.to_vec();
    let mut denom = denom.to_vec();
    poly_trim(&mut numer);
    poly_trim(&mut denom);
    if poly_is_zero(&denom) {
        return Err(cas_err("cannot divide by the zero polynomial"));
    }
    if poly_degree(&numer) < poly_degree(&denom) {
        return Ok((vec![Value::Int(0)], numer));
    }

    let denom_degree = poly_degree(&denom);
    let denom_lead = denom[denom_degree].clone();
    let mut remainder = numer;
    let mut quotient = vec![Value::Int(0); poly_degree(&remainder) - denom_degree + 1];

    while !poly_is_zero(&remainder) && poly_degree(&remainder) >= denom_degree {
        let rem_degree = poly_degree(&remainder);
        let shift = rem_degree - denom_degree;
        let coeff = eval_exact_numeric_div(&remainder[rem_degree], &denom_lead)?;
        quotient[shift] = eval_numeric_binary("+", &quotient[shift], &coeff)?;
        for (idx, denom_coeff) in denom.iter().enumerate().take(denom_degree + 1) {
            let term = eval_numeric_binary("*", &coeff, denom_coeff)?;
            remainder[idx + shift] = eval_numeric_binary("-", &remainder[idx + shift], &term)?;
        }
        poly_trim(&mut remainder);
    }

    poly_trim(&mut quotient);
    poly_trim(&mut remainder);
    Ok((quotient, remainder))
}

pub(super) fn collect_single_poly_var(expr: &Value, found: &mut Option<String>) -> bool {
    if let Some(name) = expr.cas_var_name() {
        match found {
            Some(existing) if existing != name => return false,
            Some(_) => {}
            None => *found = Some(name.to_string()),
        }
        return true;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        for arg in args {
            if !collect_single_poly_var(arg, found) {
                return false;
            }
        }
    }
    if let Some((_, args)) = expr.cas_call_parts() {
        for arg in args {
            if !collect_single_poly_var(arg, found) {
                return false;
            }
        }
    }
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        if !collect_single_poly_var(lhs, found) {
            return false;
        }
        if !collect_single_poly_var(rhs, found) {
            return false;
        }
    }
    true
}

pub(super) fn try_exact_polynomial_division(lhs: &Value, rhs: &Value) -> WqResult<Option<Value>> {
    let mut var = None;
    if !collect_single_poly_var(lhs, &mut var) || !collect_single_poly_var(rhs, &mut var) {
        return Ok(None);
    }
    let Some(var) = var else {
        return Ok(None);
    };
    let numer = match poly_from_expr(lhs, &var) {
        Ok(coeffs) => coeffs,
        Err(_) => return Ok(None),
    };
    let denom = match poly_from_expr(rhs, &var) {
        Ok(coeffs) => coeffs,
        Err(_) => return Ok(None),
    };
    if poly_degree(&denom) == 0 {
        return Ok(None);
    }
    let (quotient, remainder) = poly_divide(&numer, &denom)?;
    if poly_is_zero(&remainder) {
        Ok(Some(poly_to_expr(&quotient, &var)?))
    } else {
        Ok(None)
    }
}

pub(crate) fn poly_from_expr(expr: &Value, var: &str) -> WqResult<Vec<Value>> {
    if let Some(name) = expr.cas_var_name() {
        if name == var {
            return Ok(vec![Value::Int(0), Value::Int(1)]);
        }
        return Err(cas_err(format!(
            "solve currently supports a single variable '{var}' only"
        )));
    }
    if !expr.is_cas_expr() {
        return Ok(vec![expr.clone()]);
    }
    if let Some((op, args)) = expr.cas_op_parts() {
        return match (op, args) {
            ("+", args) => {
                let mut acc = vec![Value::Int(0)];
                for arg in args {
                    acc = poly_add(&acc, &poly_from_expr(arg, var)?)?;
                }
                Ok(acc)
            }
            ("*", args) => {
                let mut acc = vec![Value::Int(1)];
                for arg in args {
                    acc = poly_mul(&acc, &poly_from_expr(arg, var)?)?;
                }
                Ok(acc)
            }
            ("^", [base, exp]) => {
                if base.cas_var_name() == Some(var) {
                    let n = exp.exact_int().and_then(|n| n.to_usize()).ok_or_else(|| {
                        cas_err("solve currently supports non-negative integer powers only")
                    })?;
                    let mut coeffs = vec![Value::Int(0); n + 1];
                    coeffs[n] = Value::Int(1);
                    Ok(coeffs)
                } else if !contains_cas_var(base, var) {
                    let Some(n) = exp.exact_int() else {
                        // Fractional power of constant base — not a polynomial.
                        return Err(cas_err(
                            "solve currently supports polynomial expressions with exact numeric coefficients",
                        ));
                    };
                    // Try to evaluate the constant power to a numeric or
                    // algebraic value so that polynomial arithmetic below
                    // (poly_mul/poly_add/poly_gcd) works with clean coeffs.
                    let base_val = simplify_cas_value(base)?;
                    if !base_val.is_cas_expr() {
                        let n_val = Value::from_bigint(n.clone());
                        if let Ok(val) = eval_numeric_binary("^", &base_val, &n_val) {
                            return Ok(vec![val]);
                        }
                    }
                    // Base or pow couldn't be reduced — keep as CAS expr.
                    Ok(vec![expr.clone()])
                } else {
                    Err(cas_err(
                        "solve currently supports polynomial expressions with exact numeric coefficients",
                    ))
                }
            }
            _ => Err(cas_err(
                "solve currently supports polynomial expressions with exact numeric coefficients",
            )),
        };
    }
    Err(cas_err("solve expected a symbolic polynomial expression").got1(expr))
}
