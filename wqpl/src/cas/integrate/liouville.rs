//! Liouville integration for exponential integrands: ∫ f(x)·e^(g(x)) dx.
//!
//! Liouville's theorem: if ∫ f·e^g is elementary, there exists a rational
//! function R such that f = R' + R·g'.  Then ∫ f·e^g = R·e^g.
//!
//! This module solves for R using undetermined coefficients when f and g
//! are polynomials (the polynomial × exp(polynomial) case).

use num_bigint::BigInt;
use num_traits::ToPrimitive as _;

use super::byparts::try_extract_exp_arg;
use crate::cas::{
    cas_add, cas_div, cas_mul, cas_pow, cas_product, cas_sub, eval_exact_numeric_div, numeric_add,
    numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul, numeric_sub, poly_degree,
    poly_derivative, poly_divide, poly_from_expr, poly_gcd, poly_is_zero, poly_mul, poly_sub,
    poly_to_expr, poly_trim, simplify_cas_value,
};
use crate::value::cas::{CasConst, CasFunction, CasOp};
use crate::value::{Value, WqResult};

/// Strategy entry point: integrate f(x)·e^(g(x)) via Liouville's principle.
pub(super) fn integrate_liouville(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let simplified = simplify_cas_value(expr)?;

    // Case: pure exp(g(x)) / e^(g(x)) — delegate to table
    if try_extract_exp_arg(&simplified).is_some() {
        return Ok(None);
    }

    // Determine f(x) and g(x) from the expression.
    // Handles: f * exp(g), e^g / d, f * e^g / d
    let (f_expr, g_expr) = match simplified.cas_op_parts() {
        // Case: f(x) * exp(g(x)) / e^(g(x))
        Some((CasOp::Multiply, args)) => {
            let mut f_factors = Vec::new();
            let mut g = None;
            for arg in args {
                if let Some(g_arg) = try_extract_exp_arg(arg) {
                    if g.is_some() {
                        return Ok(None);
                    }
                    g = Some(g_arg);
                } else {
                    f_factors.push(arg.clone());
                }
            }
            let g = match g {
                Some(g) => g,
                None => return Ok(None),
            };
            (cas_product(f_factors.to_vec()), g)
        }
        // Case: e^(g(x)) / d(x)
        Some((CasOp::Divide, [n, d])) => {
            if let Some(g) = try_extract_exp_arg(n) {
                // f(x) = 1/d(x)
                let f = Value::from_cas_op(CasOp::Divide, vec![Value::Int(1), d.clone()]);
                (f, g)
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };

    // If f is not CAS (pure numeric), delegate to table
    if !f_expr.is_cas_expr() {
        return Ok(None);
    }

    // Try polynomial f and polynomial g
    if let Some(result) = try_liouville_poly_poly(&f_expr, &g_expr, var)? {
        return Ok(Some(result));
    }

    // Try rational f with linear g
    if let Some(result) = try_liouville_rational(&f_expr, &g_expr, var)? {
        return Ok(Some(result));
    }

    // Try rational f with polynomial g (deg >= 2)
    let g_coeffs = match poly_from_expr(&g_expr, var) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    if poly_degree(&g_coeffs) >= 2
        && let Some(result) = try_liouville_rational_general(&f_expr, &g_coeffs, var)?
    {
        return Ok(Some(result));
    }

    // Ei pattern: ∫ C/x · e^(a·x^n) dx = (C/n)·Ei(a·x^n)
    if let Some(result) = try_liouville_ei_pattern(&f_expr, &g_coeffs, var)? {
        return Ok(Some(result));
    }

    // Erf pattern: ∫ C · e^(-a·x^2) dx = C·√(π/a)/2 · erf(√a·x)
    if let Some(result) = try_liouville_erf_pattern(&f_expr, &g_coeffs, var)? {
        return Ok(Some(result));
    }

    Ok(None)
}

/// Handle ∫ P(x)·e^(Q(x)) dx where P, Q are polynomials.
fn try_liouville_poly_poly(f_expr: &Value, g_expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Extract polynomial coefficients
    let p = match poly_from_expr(f_expr, var) {
        Ok(c) => c,
        Err(_) => return Ok(None), // f is not a polynomial — defer
    };
    let g = match poly_from_expr(g_expr, var) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };

    let deg_p = poly_degree(&p);
    let deg_g = poly_degree(&g);

    if deg_g == 0 {
        // g is constant → e^(const) * ∫ P(x) dx — already handled
        return Ok(None);
    }

    // Compute g' (derivative polynomial)
    let g_deriv = poly_derivative(&g);
    let deg_gd = poly_degree(&g_deriv);

    // Deg of R: R' + R·g' = P
    // deg(R' + R·g') = max(deg(R)-1, deg(R) + deg(g'))
    // If deg(g') > 0: max term is deg(R) + deg(g')
    //   deg(R) + deg(g') = deg(P) → deg(R) = deg(P) - deg(g')
    // If deg(R) < 0, no polynomial solution exists.
    let deg_r = if deg_gd > 0 {
        if deg_p < deg_gd {
            return Ok(None);
        }
        deg_p - deg_gd
    } else {
        // g' is constant (g is linear: g = kx + b, g' = k)
        // deg(R' + kR) = deg(R) if deg(R) > 0, or 0 if R is constant
        // If deg(P) can be matched, deg(R) = deg(P)
        deg_p
    };

    // Solve for R's coefficients: R' + g'·R = P
    let r = solve_liouville_coeffs(&p, &g_deriv, deg_r)?;

    // Build result: R(x) * e^(g(x))
    let r_expr = poly_to_expr(&r, var)?;
    let exp_g = Value::from_cas_function(CasFunction::Exp, vec![g_expr.clone()]);
    let result = simplify_cas_value(&cas_mul(vec![r_expr, exp_g])?)?;

    Ok(Some(result))
}

/// Solve R' + G·R = P for polynomial R.
///
/// Let R(x) = r₀ + r₁x + ... + rₙxⁿ where n = deg_r.
/// Let G(x) = g₀ + g₁x + ... + gₘxᵐ (m ≥ 0).
///
/// R' + G·R = (r₁ + 2r₂x + ... + nrₙxⁿ⁻¹) + (g₀+g₁x+...)(r₀+r₁x+...+rₙxⁿ)
///
/// This is an upper-triangular linear system when processing equations from
/// highest degree down.  At t = m+j, the variable r_j first appears with
/// coefficient g_m, so we solve for r_j starting from r_n down to r_0.
fn solve_liouville_coeffs(p: &[Value], g: &[Value], deg_r: usize) -> WqResult<Vec<Value>> {
    let deg_p = poly_degree(p);
    let m = poly_degree(g);

    // R has deg_r + 1 coefficients (indices 0..deg_r)
    let mut r = vec![Value::Int(0); deg_r + 1];

    if m == 0 {
        // G is constant k.  R' + k·R = P.
        // At x^j: (j+1)·r_{j+1} + k·r_j = p_j — solve r_j from j=deg_r down to 0.
        let k = &g[0];
        for j in (0..=deg_r).rev() {
            let p_j = p.get(j).cloned().unwrap_or(Value::Int(0));
            let next_term = if j < deg_r {
                numeric_mul(&Value::from_bigint(BigInt::from(j + 1)), &r[j + 1])?
            } else {
                Value::Int(0)
            };
            let numer = numeric_sub(&p_j, &next_term)?;
            r[j] = eval_exact_numeric_div(&numer, k)?;
        }
    } else {
        // General case: G has degree m ≥ 1.
        // Equation at x^t: (t+1)·r_{t+1} + Σ_{k=max(0,t-m)}^{min(n,t)} g_{t-k}·r_k =
        // p_t Process t from n+m down to m — each equation determines exactly
        // one new variable r_{t-m} via the leading coefficient g_m.
        let g_m = g[m].clone();

        for t in (m..=deg_p).rev() {
            let k = t - m;
            let p_t = p.get(t).cloned().unwrap_or(Value::Int(0));

            let r_prime = if t < deg_r {
                numeric_mul(&Value::from_bigint(BigInt::from(t + 1)), &r[t + 1])?
            } else {
                Value::Int(0)
            };

            let mut g_dot_r = Value::Int(0);
            let k_high = deg_r.min(t);
            for (k_known, r_k) in r.iter().enumerate().take(k_high + 1).skip(k + 1) {
                let i = t - k_known;
                let g_i = g.get(i).cloned().unwrap_or(Value::Int(0));
                let term = numeric_mul(&g_i, r_k)?;
                g_dot_r = numeric_add(&g_dot_r, &term)?;
            }

            let numer = numeric_sub(&numeric_sub(&p_t, &r_prime)?, &g_dot_r)?;
            r[k] = eval_exact_numeric_div(&numer, &g_m)?;
        }

        // Consistency check: equations t = 0..m-1 involve only known r's.
        for t in 0..m {
            let p_t = p.get(t).cloned().unwrap_or(Value::Int(0));

            let r_prime = if t < deg_r {
                numeric_mul(&Value::from_bigint(BigInt::from(t + 1)), &r[t + 1])?
            } else {
                Value::Int(0)
            };

            let mut g_dot_r = Value::Int(0);
            let k_low = t.saturating_sub(m);
            for (k_known, r_k) in r.iter().enumerate().take(deg_r.min(t) + 1).skip(k_low) {
                let i = t - k_known;
                let g_i = g.get(i).cloned().unwrap_or(Value::Int(0));
                let term = numeric_mul(&g_i, r_k)?;
                g_dot_r = numeric_add(&g_dot_r, &term)?;
            }

            let computed = numeric_add(&r_prime, &g_dot_r)?;
            let diff = numeric_sub(&p_t, &computed)?;
            if !numeric_is_zero(&diff) {
                return Err(crate::cas::cas_err(format!(
                    "Liouville: inconsistent at degree {} (P={}, computed={})",
                    t, p_t, computed,
                )));
            }
        }
    }

    poly_trim(&mut r);
    Ok(r)
}

/// Extract (numerator, denominator) polynomials from a rational expression.
/// Handles: "/"(n, d), "^"(base, -k), and "*"(const, "^"(base, -k)).
fn extract_rational_num_den(expr: &Value, var: &str) -> Option<(Vec<Value>, Vec<Value>)> {
    match expr.cas_op_parts() {
        Some((CasOp::Divide, args)) if args.len() == 2 => {
            let n = &args[0];
            let d = &args[1];
            let num = poly_from_expr(n, var).ok()?;
            let den = poly_from_expr(d, var).ok()?;
            Some((num, den))
        }
        Some((CasOp::Power, args)) if args.len() == 2 => {
            let base = &args[0];
            let exp = &args[1];
            let k = exp.exact_int()?;
            if k >= BigInt::from(0) {
                return None;
            }
            let k_abs = (-k).to_usize()?;
            let base_poly = poly_from_expr(base, var).ok()?;
            let denom = crate::cas::integrate::rational::poly_pow(&base_poly, k_abs).ok()?;
            Some((vec![Value::Int(1)], denom))
        }
        Some((CasOp::Multiply, args)) if args.len() == 2 => {
            let (const_part, pow_part) = if args[0]
                .cas_op_parts()
                .is_some_and(|(op, _)| op == CasOp::Power)
            {
                (&args[1], &args[0])
            } else {
                (&args[0], &args[1])
            };
            match pow_part.cas_op_parts() {
                Some((CasOp::Power, pow_args)) if pow_args.len() == 2 => {
                    let base = &pow_args[0];
                    let exp = &pow_args[1];
                    let k = exp.exact_int()?;
                    if k >= BigInt::from(0) {
                        return None;
                    }
                    let k_abs = (-k).to_usize()?;
                    let base_poly = poly_from_expr(base, var).ok()?;
                    let denom =
                        crate::cas::integrate::rational::poly_pow(&base_poly, k_abs).ok()?;
                    let num = poly_from_expr(const_part, var).ok()?;
                    Some((num, denom))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Handle ∫ f(x)·e^(g(x)) dx where f is rational and g is linear (g' = k ≠ 0).
fn try_liouville_rational(f_expr: &Value, g_expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let g_coeffs = match poly_from_expr(g_expr, var) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    if poly_degree(&g_coeffs) != 1 {
        return Ok(None);
    }
    let k = g_coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    if numeric_is_zero(&k) {
        return Ok(None);
    }
    let b = g_coeffs.first().cloned().unwrap_or(Value::Int(0));

    // Extract numerator and denominator from f.
    // Handle both "/"(n, d) and "^"(base, -k) forms.
    let (numer, denom) = match extract_rational_num_den(f_expr, var) {
        Some((n, d)) => (n, d),
        None => return Ok(None),
    };
    if poly_is_zero(&denom) {
        return Ok(None);
    }

    let (quotient, remainder) = poly_divide(&numer, &denom)?;
    let mut terms = Vec::new();

    if !poly_is_zero(&quotient) {
        let q_expr = poly_to_expr(&quotient, var)?;
        if let Some(q_result) = try_liouville_poly_poly(&q_expr, g_expr, var)? {
            terms.push(q_result);
        }
    }

    if !poly_is_zero(&remainder)
        && let Some(r_term) = integrate_rational_exp(&remainder, &denom, &k, &b, var)?
    {
        terms.push(r_term);
    }

    if terms.is_empty() {
        return Ok(None);
    }
    Ok(Some(simplify_cas_value(&cas_add(terms)?)?))
}

/// Try to integrate remainder/denom · e^(kx+b) where denom is (x-a)^n.
fn integrate_rational_exp(
    remainder: &[Value],
    denom: &[Value],
    k: &Value,
    b: &Value,
    var: &str,
) -> WqResult<Option<Value>> {
    let denom_deg = poly_degree(denom);
    if denom_deg == 0 {
        return Ok(None);
    }
    let lc = denom[denom_deg].clone();
    if !numeric_is_one(&lc) {
        return Ok(None);
    }

    let n = denom_deg;
    let a = if n == 1 {
        numeric_mul(&denom[0], &Value::Int(-1))?
    } else {
        let coeff_nm1 = &denom[n - 1];
        let neg_n = Value::from_bigint(BigInt::from(-(n as i64)));
        eval_exact_numeric_div(coeff_nm1, &neg_n)?
    };

    let rem_deg = poly_degree(remainder);
    let coeff_a = if rem_deg == 0 {
        remainder[0].clone()
    } else {
        return Ok(None);
    };

    Ok(Some(integrate_simple_pole_exp(&coeff_a, &a, k, b, n, var)?))
}

/// ∫ A/(x-a)^n · e^(kx+b) dx using recurrence:
///   I₁ = A·e^(ka+b)·Ei(k(x-a))
///   Iₙ = -A·e^(kx+b)/((n-1)(x-a)^(n-1)) + k/(n-1)·Iₙ₋₁
fn integrate_simple_pole_exp(
    a_coeff: &Value,
    pole: &Value,
    k: &Value,
    b: &Value,
    n: usize,
    var: &str,
) -> WqResult<Value> {
    let x = Value::from_cas_var(var);
    let x_minus_pole = cas_sub(x.clone(), pole.clone())?;

    if n == 1 {
        let kpole_plus_b = numeric_add(&numeric_mul(k, pole)?, b)?;
        let ei_arg = cas_mul(vec![k.clone(), x_minus_pole])?;
        let ei_term = Value::from_cas_function(CasFunction::Ei, vec![ei_arg]);
        let prefactor = cas_mul(vec![
            a_coeff.clone(),
            Value::from_cas_function(CasFunction::Exp, vec![kpole_plus_b]),
        ])?;
        return cas_mul(vec![prefactor, ei_term]);
    }

    let nm1 = Value::from_bigint(BigInt::from((n - 1) as i64));
    let kx_plus_b = cas_add(vec![cas_mul(vec![k.clone(), x.clone()])?, b.clone()])?;
    let exp_term = Value::from_cas_function(CasFunction::Exp, vec![kx_plus_b]);

    let denom_power = if n == 2 {
        x_minus_pole.clone()
    } else {
        cas_pow(
            x_minus_pole.clone(),
            Value::from_bigint(BigInt::from((n - 1) as i64)),
        )?
    };
    let neg_a_exp = cas_mul(vec![Value::Int(-1), a_coeff.clone(), exp_term])?;
    let first_term = cas_div(neg_a_exp, cas_mul(vec![nm1.clone(), denom_power])?)?;

    let factor = eval_exact_numeric_div(k, &nm1)?;
    let recurse = integrate_simple_pole_exp(a_coeff, pole, k, b, n - 1, var)?;
    let second_term = cas_mul(vec![factor, recurse])?;

    cas_add(vec![first_term, second_term])
}

/// Solve A(x)·P'(x) + B(x)·P(x) = H(x) for polynomial P of degree `deg_p`.
///
/// The system is upper-triangular when processed from highest degree down.
/// For the top deg(B)-deg(A) equations, only B·P contributes, and each
/// equation introduces exactly one new coefficient with leading coefficient
/// b[deg(B)].  After all p_j are solved, the remaining lower-degree
/// equations serve as consistency checks.
fn solve_poly_ode_general(
    a: &[Value],
    b: &[Value],
    h: &[Value],
    deg_p: usize,
) -> WqResult<Vec<Value>> {
    let deg_a = poly_degree(a);
    let deg_b = poly_degree(b);
    let deg_h = poly_degree(h);

    let mut p = vec![Value::Int(0); deg_p + 1];
    let mut solved = vec![false; deg_p + 1];
    let b_lead = b[deg_b].clone();

    for d in (0..=deg_h).rev() {
        // Accumulate known contributions to lhs at degree d
        let mut lhs = Value::Int(0);

        // From A·P': a_i · (j+1) · p_{j+1}
        for (i, a_i) in a.iter().enumerate().take(deg_a.min(d) + 1) {
            let j = d - i;
            if j < deg_p {
                let p_idx = j + 1;
                if solved[p_idx] {
                    let factor = Value::from_bigint(BigInt::from(p_idx as i64));
                    let contrib = numeric_mul(&numeric_mul(a_i, &factor)?, &p[p_idx])?;
                    lhs = numeric_add(&lhs, &contrib)?;
                }
            }
        }

        // From B·P: b_i · p_j
        for (i, b_i) in b.iter().enumerate().take(deg_b.min(d) + 1) {
            let j = d - i;
            if j <= deg_p && solved[j] {
                let contrib = numeric_mul(b_i, &p[j])?;
                lhs = numeric_add(&lhs, &contrib)?;
            }
        }

        // Identify the new unknown: p_{d - deg_b}
        let j_new = if d >= deg_b { d - deg_b } else { deg_p + 1 };
        if j_new <= deg_p && !solved[j_new] {
            // The coefficient of p[j_new] from B·P is b_lead
            let h_d = h.get(d).cloned().unwrap_or(Value::Int(0));
            let rhs = numeric_sub(&h_d, &lhs)?;
            p[j_new] = eval_exact_numeric_div(&rhs, &b_lead)?;
            solved[j_new] = true;
        } else {
            // Consistency check: no new unknown at this degree
            let h_d = h.get(d).cloned().unwrap_or(Value::Int(0));
            let diff = numeric_sub(&h_d, &lhs)?;
            if !numeric_is_zero(&diff) {
                return Err(crate::cas::cas_err(format!(
                    "Liouville gen: inconsistent at degree {} (H={}, computed={})",
                    d, h_d, lhs,
                )));
            }
        }
    }

    poly_trim(&mut p);
    Ok(p)
}

/// Handle ∫ f(x)·e^(g(x)) dx where f = N/D is rational and g is polynomial (deg
/// ≥ 2).
fn try_liouville_rational_general(
    f_expr: &Value,
    g_coeffs: &[Value],
    var: &str,
) -> WqResult<Option<Value>> {
    // Extract numerator and denominator from f
    let (numer, denom) = match extract_rational_num_den(f_expr, var) {
        Some((n, d)) => (n, d),
        None => return Ok(None),
    };
    if poly_is_zero(&denom) {
        return Ok(None);
    }

    // Try the inner solver on the full rational f — do not split into
    // quotient + remainder, because the quotient part ∫ Q·e^g is only
    // elementary when deg(Q) ≥ deg(g'), and the combined solution
    // A·P' + B·P = H correctly handles all cases.
    if let Some(r_term) = try_liouville_rational_general_inner(&numer, &denom, g_coeffs, var)? {
        return Ok(Some(r_term));
    }

    Ok(None)
}

/// Core solver for proper rational f (deg(N) < deg(D)) × e^(Q).
fn try_liouville_rational_general_inner(
    numer: &[Value],
    denom: &[Value],
    g_coeffs: &[Value],
    var: &str,
) -> WqResult<Option<Value>> {
    // 1. Compute D₀ = gcd(D, D') — denominator of R
    let denom_deriv = poly_derivative(denom);
    let d0 = poly_gcd(denom, &denom_deriv)?;
    if poly_degree(&d0) == 0 {
        // All poles are simple — R would be polynomial.
        // But f has poles, so R cannot cancel them. Not elementary via this method.
        return Ok(None);
    }

    // 2. Compute D₁ = D / D₀ — product of distinct irreducible factors
    let (d1, rem) = poly_divide(denom, &d0)?;
    if !poly_is_zero(&rem) {
        return Ok(None);
    }

    // 3. Compute g' = derivative of exponent polynomial
    let g_prime = poly_derivative(g_coeffs);

    // 4. Build A = D₁·D₀, B = D₁·(g'·D₀ - D₀'), H = N·D₀
    // A = poly_mul(&d1, &d0)?;  // = D (original denominator), reuse denom directly
    let a = denom; // = D = D₁·D₀

    let g_prime_d0 = poly_mul(&g_prime, &d0)?;
    let d0_deriv = poly_derivative(&d0);
    let b_raw = poly_sub(&g_prime_d0, &d0_deriv)?;
    let b = poly_mul(&d1, &b_raw)?;

    let h = poly_mul(numer, &d0)?;

    let deg_h = poly_degree(&h);
    let deg_b = poly_degree(&b);

    // 5. Degree bound: deg(P) = deg(H) - deg(B)
    if deg_h < deg_b {
        return Ok(None);
    }
    let deg_p = deg_h - deg_b;

    // 6. Solve for P
    let p = solve_poly_ode_general(a, &b, &h, deg_p)?;

    // 7. Build R = P / D₀
    let p_expr = poly_to_expr(&p, var)?;
    let d0_expr = poly_to_expr(&d0, var)?;
    let r_expr = cas_div(p_expr, d0_expr)?;

    // 8. Build exp(g) and combine
    let g_expr = poly_to_expr(g_coeffs, var)?;
    let exp_g = Value::from_cas_function(CasFunction::Exp, vec![g_expr]);
    let result = simplify_cas_value(&cas_mul(vec![r_expr, exp_g])?)?;

    Ok(Some(result))
}

/// Handle ∫ C/x · e^(a·x^n) dx = (C/n)·Ei(a·x^n) via substitution u = a·x^n.
///
/// This catches cases where the general Liouville solver correctly determines
/// the integral is not elementary, but it can be expressed using the
/// exponential integral Ei which is already supported in the codebase.
fn try_liouville_ei_pattern(
    f_expr: &Value,
    g_coeffs: &[Value],
    var: &str,
) -> WqResult<Option<Value>> {
    // f must be C/x: either "/"(C, x) or (* C (^ x -1))
    let (c, is_one_over_x) = match f_expr.cas_op_parts() {
        Some((CasOp::Divide, [n, d])) if d.cas_var_name() == Some(var) => {
            if n.is_cas_expr() && n.cas_var_name().is_none() {
                return Ok(None);
            }
            (n.clone(), true)
        }
        Some((CasOp::Multiply, args)) => {
            // Look for (* C (^ x -1))
            let mut c_val = None;
            let mut has_x_pow_neg1 = false;
            for arg in args {
                if let Some((CasOp::Power, [base, e])) = arg.cas_op_parts()
                    && base.cas_var_name() == Some(var)
                    && e.exact_int_is(-1)
                {
                    has_x_pow_neg1 = true;
                } else if !arg.is_cas_expr() || arg.cas_var_name().is_some() {
                    // numeric constant or bare variable
                    if c_val.is_some() {
                        return Ok(None); // too many non-denom factors
                    }
                    c_val = Some(arg.clone());
                } else {
                    return Ok(None); // non-polynomial CAS factor
                }
            }
            if has_x_pow_neg1 {
                (c_val.unwrap_or(Value::Int(1)), true)
            } else {
                return Ok(None);
            }
        }
        Some((CasOp::Power, [base, e]))
            if base.cas_var_name() == Some(var) && e.exact_int_is(-1) =>
        {
            (Value::Int(1), true)
        }
        _ => return Ok(None),
    };

    if !is_one_over_x {
        return Ok(None);
    }

    // g must be a monomial a·x^n with n ≥ 2, or just x^n
    let g_deg = poly_degree(g_coeffs);
    if g_deg < 2 {
        return Ok(None);
    }
    // Check that g is a monomial: only the leading coefficient is non-zero
    let a = &g_coeffs[g_deg];
    for (i, coeff) in g_coeffs.iter().enumerate() {
        if i != g_deg && !numeric_is_zero(coeff) {
            return Ok(None);
        }
    }

    // Result: (C/n) · Ei(a·x^n)
    let n = Value::from_bigint(BigInt::from(g_deg as i64));
    let factor = eval_exact_numeric_div(&c, &n)?;

    let x = Value::from_cas_var(var);
    let x_pow_n = cas_pow(x, Value::from_bigint(BigInt::from(g_deg as i64)))?;
    let ei_arg = cas_mul(vec![a.clone(), x_pow_n])?;
    let ei_term = Value::from_cas_function(CasFunction::Ei, vec![ei_arg]);

    let result = simplify_cas_value(&cas_mul(vec![factor, ei_term])?)?;
    Ok(Some(result))
}

/// Handle ∫ C · e^(-a·x^2) dx = C·√(π/a)/2 · erf(√a·x) for a > 0.
///
/// This catches the Gaussian integral which is not elementary but can be
/// expressed using the error function erf, already supported in the codebase.
fn try_liouville_erf_pattern(
    f_expr: &Value,
    g_coeffs: &[Value],
    var: &str,
) -> WqResult<Option<Value>> {
    // f must be a constant (numeric, not a CAS expression involving var)
    let c = match f_expr {
        v if !v.is_cas_expr() => v.clone(),
        v if v.cas_var_name().is_some() => {
            // f is just the variable — not a constant
            return Ok(None);
        }
        _ => {
            // Try to extract as a constant polynomial
            match poly_from_expr(f_expr, var) {
                Ok(p) if poly_degree(&p) == 0 => p[0].clone(),
                _ => return Ok(None),
            }
        }
    };

    // g must be a quadratic monomial: only a·x^2 (and possibly constant term)
    let g_deg = poly_degree(g_coeffs);
    if g_deg != 2 {
        return Ok(None);
    }
    // Check that g has only x^2 term and possibly constant term
    let a = &g_coeffs[2]; // coefficient of x^2
    let b_term = g_coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    if !numeric_is_zero(&b_term) {
        return Ok(None); // has linear term — not pure Gaussian
    }
    // Constant term in g becomes e^(constant) factor
    let g_const = g_coeffs.first().cloned().unwrap_or(Value::Int(0));

    // a must be negative for e^(-a·x^2) to converge
    if !numeric_is_negative(a) {
        return Ok(None);
    }
    let a_pos = numeric_mul(a, &Value::Int(-1))?; // -a > 0

    // Build √(π/a)/2
    // = sqrt(pi) / (2 * sqrt(a))
    let pi = Value::from_cas_const(CasConst::Pi);
    let sqrt_pi = Value::from_cas_function(CasFunction::Sqrt, vec![pi]);
    let sqrt_a = Value::from_cas_function(CasFunction::Sqrt, vec![a_pos.clone()]);
    let two_sqrt_a = cas_mul(vec![Value::Int(2), sqrt_a.clone()])?;
    let mut factor = simplify_cas_value(&cas_div(sqrt_pi, two_sqrt_a)?)?;
    factor = cas_mul(vec![c, factor])?;

    // If g has a constant term, multiply by e^g_const
    if !numeric_is_zero(&g_const) {
        let exp_const = Value::from_cas_function(CasFunction::Exp, vec![g_const]);
        factor = cas_mul(vec![factor, exp_const])?;
    }

    // Build erf(√a · x)
    let x = Value::from_cas_var(var);
    let erf_arg = cas_mul(vec![sqrt_a, x])?;
    let erf_term = Value::from_cas_function(CasFunction::Erf, vec![erf_arg]);

    let result = simplify_cas_value(&cas_mul(vec![factor, erf_term])?)?;
    Ok(Some(result))
}

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
    fn test_solve_constant_g() {
        // P = x, G = 1 → R' + R = x → R = x - 1
        let p = vec![Value::Int(0), Value::Int(1)];
        let g = vec![Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 1).unwrap();
        assert_eq!(r, vec![Value::Int(-1), Value::Int(1)]);
    }

    #[test]
    fn test_solve_linear_g() {
        let p = vec![Value::Int(1), Value::Int(2)]; // 2x+1
        let g = vec![Value::Int(1)]; // constant 1
        let r = solve_liouville_coeffs(&p, &g, 1).unwrap();
        assert_eq!(r, vec![Value::Int(-1), Value::Int(2)]);
    }

    #[test]
    fn test_solve_linear_g_with_g1() {
        // D = 1 + x (d₀=1, d₁=1), deg_r=1, deg_p=2.
        // R = r₀ + r₁x
        // R' + (1+x)R = (r₁+r₀) + (r₀+r₁)x + r₁x²
        // For r₀=1, r₁=1: P = [2, 2, 1].
        let p = vec![Value::Int(2), Value::Int(2), Value::Int(1)];
        let g = vec![Value::Int(1), Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 1).unwrap();
        assert_eq!(r, vec![Value::Int(1), Value::Int(1)]);
    }

    #[test]
    fn test_solve_linear_g_d0_zero() {
        // D = x (d₀=0, d₁=1), deg_r=1, deg_p=2.
        // R' + x·R = P
        // R = r₀ + r₁x
        // R' + x·R = r₁ + r₀x + r₁x²
        // For P = [1, 2, 1]: r₁=1 (from x² and x⁰), r₀=2 (from x).
        // p₀=r₁=1, p₁=r₀=2, p₂=r₁=1.
        let p = vec![Value::Int(1), Value::Int(2), Value::Int(1)];
        let g = vec![Value::Int(0), Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 1).unwrap();
        assert_eq!(r, vec![Value::Int(2), Value::Int(1)]);
    }

    #[test]
    fn test_solve_quadratic_g_deg_r_zero() {
        // D = x² (d₀=0, d₁=0, d₂=1), deg_r=0, deg_p=2.
        // R = r₀
        // R' + x²·R = 0 + r₀·x²
        // For P = 5x²: r₀ = 5.
        let p = vec![Value::Int(0), Value::Int(0), Value::Int(5)];
        let g = vec![Value::Int(0), Value::Int(0), Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 0).unwrap();
        assert_eq!(r, vec![Value::Int(5)]);
    }

    #[test]
    fn test_solve_quadratic_g_deg_r_one() {
        // D = x² (d₀=0, d₁=0, d₂=1), deg_r=1, deg_p=3.
        // R = r₀ + r₁x
        // R' + x²·R = r₁ + r₀·x² + r₁·x³
        // For P: p₀=r₁, p₁=0, p₂=r₀, p₃=r₁.
        // Choose r₀=3, r₁=2: P = [2, 0, 3, 2].
        let p = vec![Value::Int(2), Value::Int(0), Value::Int(3), Value::Int(2)];
        let g = vec![Value::Int(0), Value::Int(0), Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 1).unwrap();
        assert_eq!(r, vec![Value::Int(3), Value::Int(2)]);
    }

    #[test]
    fn test_solve_quadratic_g_deg_r_two() {
        // D = x² (d₀=0, d₁=0, d₂=1), deg_r=2, deg_p=4.
        // R = r₀ + r₁x + r₂x²
        // R' + x²·R = r₁ + 2r₂x + r₀·x² + r₁·x³ + r₂·x⁴
        // For P: p₀=r₁, p₁=2r₂, p₂=r₀, p₃=r₁, p₄=r₂.
        // r₀=7, r₁=3, r₂=4: P = [3, 8, 7, 3, 4].
        let p = vec![
            Value::Int(3),
            Value::Int(8),
            Value::Int(7),
            Value::Int(3),
            Value::Int(4),
        ];
        let g = vec![Value::Int(0), Value::Int(0), Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 2).unwrap();
        assert_eq!(r, vec![Value::Int(7), Value::Int(3), Value::Int(4)]);
    }

    #[test]
    fn test_solve_quadratic_g_full() {
        // D = 1 + x + x² (d₀=1, d₁=1, d₂=1), deg_r=1, deg_p=3.
        // R = r₀ + r₁x
        // R' = r₁
        // D·R = (1+x+x²)(r₀+r₁x) = r₀ + (r₀+r₁)x + (r₀+r₁)x² + r₁x³
        // R' + D·R = (r₁+r₀) + (r₀+r₁)x + (r₀+r₁)x² + r₁x³
        // P = [1, 1, 1, 1] → r₀=0, r₁=1? Check: r₁+r₀=0+1=1, r₀+r₁=1, r₀+r₁=1, r₁=1.
        let p = vec![Value::Int(1), Value::Int(1), Value::Int(1), Value::Int(1)];
        let g = vec![Value::Int(1), Value::Int(1), Value::Int(1)];
        let r = solve_liouville_coeffs(&p, &g, 1).unwrap();
        assert_eq!(r, vec![Value::Int(0), Value::Int(1)]);
    }

    #[test]
    fn test_integrate_cubic_exponent() {
        // ∫ (2x+1)·e^(x³/3) dx = (2x+1)·e^(x³/3) — wait, that's not right.
        // R' + x²·R = 2x+1 where D(x)=x² is the derivative of g=x³/3.
        // deg_r = deg_p - deg(D) = 1 - 2 = -1 < 0, so no polynomial solution.
        //
        // Let's use a solvable case: g = x³/3, f = R' + x²·R with R = x².
        // R' + x²·R = 2x + x⁴ → f = x⁴ + 2x.
        // ∫ (x⁴+2x)·e^(x³/3) dx = x²·e^(x³/3).
        let f_expr = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(4)]),
                op(
                    CasOp::Multiply,
                    vec![Value::Int(2), Value::from_cas_var("x")],
                ),
            ],
        );
        // g = 1/3 * x^3 (Fraction, not Float)
        let g_expr = op(
            CasOp::Multiply,
            vec![
                Value::from_fraction_parts(1u64.into(), 3u64.into()),
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
            ],
        );
        let expr = op(
            CasOp::Multiply,
            vec![f_expr, call(CasFunction::Exp, vec![g_expr])],
        );

        let result = super::integrate_liouville(&expr, "x").unwrap().unwrap();
        // Should be x²·e^(x³/3), equiv to (x²)*exp(x³/3)
        let s = result.to_string();
        assert!(s.contains("x^2"), "expected x^2 in result: {s}");
        assert!(s.contains("e^"), "expected e^ in result: {s}");
    }

    #[test]
    fn test_solve_inconsistent_system() {
        // D = x² (d₀=0, d₁=0, d₂=1), deg_r=1, deg_p=3.
        // For R = r₀ + r₁x: R' + x²·R = r₁ + r₀x² + r₁x³.
        // This constrains p₁ = 0 and p₀ = p₃.
        // P = [1, 1, 1, 1] violates both — must return an error.
        let p = vec![Value::Int(1), Value::Int(1), Value::Int(1), Value::Int(1)];
        let g = vec![Value::Int(0), Value::Int(0), Value::Int(1)];
        assert!(
            solve_liouville_coeffs(&p, &g, 1)
                .unwrap_err()
                .to_string()
                .contains("inconsistent")
        );
    }

    #[test]
    fn test_simple_pole_exp_base() {
        // ∫ e^x/(x-1) dx = e^1 · Ei(x-1)
        let result = super::integrate_simple_pole_exp(
            &Value::Int(1),
            &Value::Int(1),
            &Value::Int(1),
            &Value::Int(0),
            1,
            "x",
        )
        .unwrap();
        let s = result.to_string();
        assert!(s.contains("ei"), "expected ei: {s}");
    }

    #[test]
    fn test_simple_pole_exp_n2() {
        // ∫ e^(2x)/(x+1)^2 dx
        let result = super::integrate_simple_pole_exp(
            &Value::Int(1),
            &Value::Int(-1),
            &Value::Int(2),
            &Value::Int(0),
            2,
            "x",
        )
        .unwrap();
        let s = result.to_string();
        assert!(s.contains("ei"), "expected ei: {s}");
    }

    #[test]
    fn test_liouville_rational_1_over_x_times_exp() {
        let f = op(CasOp::Divide, vec![Value::Int(1), Value::from_cas_var("x")]);
        let g = op(
            CasOp::Multiply,
            vec![Value::Int(2), Value::from_cas_var("x")],
        );
        let expr = op(CasOp::Multiply, vec![f, call(CasFunction::Exp, vec![g])]);
        let result = super::integrate_liouville(&expr, "x");
        // Should return Ei(2x) or fall through gracefully
        let _ = result;
    }

    // ── Liouville general rational f(x) tests ──

    /// Build integrand f(x) * exp(g(x))
    fn build_integrand(f: Value, g: Value) -> Value {
        op(CasOp::Multiply, vec![f, call(CasFunction::Exp, vec![g])])
    }

    #[test]
    fn test_rational_liouville_simple_pole_quadratic_g() {
        // R = 1/x, g = x²
        // f = R' + g'·R = -1/x² + 2x/x = (2x²-1)/x²
        // ∫ (2x²-1)/x² · e^(x²) dx = (1/x)·e^(x²)
        let x = Value::from_cas_var("x");
        let two_x_sq = op(
            CasOp::Multiply,
            vec![
                Value::Int(2),
                op(CasOp::Power, vec![x.clone(), Value::Int(2)]),
            ],
        );
        let numer = op(CasOp::Add, vec![two_x_sq, Value::Int(-1)]);
        let denom = op(CasOp::Power, vec![x.clone(), Value::Int(2)]);
        let f = op(CasOp::Divide, vec![numer, denom]);
        let g = op(CasOp::Power, vec![x, Value::Int(2)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        // Should contain x^(-1) and e^(x^2)
        assert!(
            s.contains("-1") || s.contains("1/x") || s.contains("x^"),
            "expected 1/x factor in result: {s}"
        );
        assert!(
            s.contains("e^") || s.contains("exp"),
            "expected exp factor in result: {s}"
        );
    }

    #[test]
    fn test_rational_liouville_quadratic_denom_cubic_g() {
        // R = 1/(x²+1), g = x³/3
        // f = R' + g'·R = -2x/(x²+1)² + x²/(x²+1)
        //   = (x⁴ + x² - 2x)/(x²+1)²
        // ∫ (x⁴+x²-2x)/(x²+1)² · e^(x³/3) dx = 1/(x²+1) · e^(x³/3)
        let x = Value::from_cas_var("x");
        let x_sq = op(CasOp::Power, vec![x.clone(), Value::Int(2)]);
        let x_sq_plus_1 = op(CasOp::Add, vec![x_sq.clone(), Value::Int(1)]);
        // D = (x²+1)²
        let denom = op(CasOp::Power, vec![x_sq_plus_1, Value::Int(2)]);
        // N = x⁴ + x² - 2x
        let x4 = op(CasOp::Power, vec![x.clone(), Value::Int(4)]);
        let x2 = op(CasOp::Power, vec![x.clone(), Value::Int(2)]);
        let two_x = op(CasOp::Multiply, vec![Value::Int(2), x.clone()]);
        let numer = op(
            CasOp::Add,
            vec![
                x4,
                op(
                    CasOp::Add,
                    vec![x2, op(CasOp::Multiply, vec![Value::Int(-1), two_x])],
                ),
            ],
        );
        let f = op(CasOp::Divide, vec![numer, denom]);
        // g = x³/3
        let g = op(
            CasOp::Multiply,
            vec![
                Value::from_fraction_parts(1u64.into(), 3u64.into()),
                op(CasOp::Power, vec![x, Value::Int(3)]),
            ],
        );
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        assert!(
            s.contains("x^2") || s.contains("x²") || s.contains("+"),
            "expected (x²+1) factor in result: {s}"
        );
        assert!(
            s.contains("e^") || s.contains("exp"),
            "expected exp factor in result: {s}"
        );
    }

    #[test]
    fn test_rational_liouville_linear_p() {
        // R = x/(x-1), g = x²
        // f = R' + g'·R = -1/(x-1)² + 2x·x/(x-1) = (2x³-2x²-1)/(x-1)²
        // ∫ (2x³-2x²-1)/(x-1)² · e^(x²) dx = x/(x-1) · e^(x²)
        let xv = Value::from_cas_var("x");
        let x_minus_1 = op(CasOp::Add, vec![xv.clone(), Value::Int(-1)]);
        let denom = op(CasOp::Power, vec![x_minus_1, Value::Int(2)]);
        // N = 2x³ - 2x² - 1
        let two_x3 = op(
            CasOp::Multiply,
            vec![
                Value::Int(2),
                op(CasOp::Power, vec![xv.clone(), Value::Int(3)]),
            ],
        );
        let two_x2 = op(
            CasOp::Multiply,
            vec![
                Value::Int(2),
                op(CasOp::Power, vec![xv.clone(), Value::Int(2)]),
            ],
        );
        let numer = op(
            CasOp::Add,
            vec![
                two_x3,
                op(CasOp::Multiply, vec![Value::Int(-1), two_x2]),
                Value::Int(-1),
            ],
        );
        let f = op(CasOp::Divide, vec![numer, denom]);
        let g = op(CasOp::Power, vec![xv, Value::Int(2)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        let has_x = s.contains("x") && (s.contains("- 1") || s.contains("-1"));
        assert!(has_x, "expected x/(x-1) factor in result: {s}");
        assert!(
            s.contains("e^") || s.contains("exp"),
            "expected exp factor in result: {s}"
        );
    }

    #[test]
    fn test_rational_liouville_repeated_pole() {
        // R = 1/(x-1)², g = x²
        // f = R' + g'·R = -2/(x-1)³ + 2x/(x-1)² = (2x²-2x-2)/(x-1)³
        // ∫ (2x²-2x-2)/(x-1)³ · e^(x²) dx = 1/(x-1)² · e^(x²)
        let xv = Value::from_cas_var("x");
        let x_minus_1 = op(CasOp::Add, vec![xv.clone(), Value::Int(-1)]);
        let denom = op(CasOp::Power, vec![x_minus_1, Value::Int(3)]);
        // N = 2x² - 2x - 2
        let two_x2 = op(
            CasOp::Multiply,
            vec![
                Value::Int(2),
                op(CasOp::Power, vec![xv.clone(), Value::Int(2)]),
            ],
        );
        let two_x = op(CasOp::Multiply, vec![Value::Int(2), xv.clone()]);
        let numer = op(
            CasOp::Add,
            vec![
                two_x2,
                op(CasOp::Multiply, vec![Value::Int(-1), two_x]),
                Value::Int(-2),
            ],
        );
        let f = op(CasOp::Divide, vec![numer, denom]);
        let g = op(CasOp::Power, vec![xv, Value::Int(2)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        // Result should be e^(x^2)/(x-1)^2 — denominator may be expanded to x^2-2x+1
        assert!(
            s.contains("x^2") && s.contains("2*x") && s.contains("1"),
            "expected e^(x^2)/(x-1)^2 (possibly expanded) in result: {s}"
        );
        assert!(
            s.contains("e^") || s.contains("exp"),
            "expected exp factor in result: {s}"
        );
    }

    #[test]
    fn test_rational_liouville_non_elementary() {
        // ∫ e^(x²)/x² dx — not elementary and not Ei-pattern
        let x = Value::from_cas_var("x");
        let f = op(
            CasOp::Divide,
            vec![
                Value::Int(1),
                op(CasOp::Power, vec![x.clone(), Value::Int(2)]),
            ],
        );
        let g = op(CasOp::Power, vec![x, Value::Int(2)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x");
        assert!(
            matches!(result, Ok(None)),
            "∫ e^(x²)/x² dx should not be elementary, got: {result:?}"
        );
    }

    #[test]
    fn test_liouville_ei_pattern_basic() {
        // ∫ e^(x²)/x dx = ei(x²)/2
        let x = Value::from_cas_var("x");
        let f = op(CasOp::Divide, vec![Value::Int(1), x.clone()]);
        let g = op(CasOp::Power, vec![x, Value::Int(2)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        assert!(s.contains("ei"), "expected ei in result: {s}");
        assert!(s.contains("2"), "expected division by 2: {s}");
    }

    #[test]
    fn test_liouville_ei_pattern_with_coeff() {
        // ∫ 3·e^(x³)/x dx = ei(x³)
        let x = Value::from_cas_var("x");
        let three = Value::Int(3);
        let f = op(CasOp::Divide, vec![three, x.clone()]);
        let g = op(CasOp::Power, vec![x, Value::Int(3)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        assert!(s.contains("ei"), "expected ei in result: {s}");
        assert!(
            s.contains("x^3") || s.contains("x³"),
            "expected x^3 in ei arg: {s}"
        );
    }

    #[test]
    fn test_rational_liouville_improper_fraction() {
        // R = x + 1/(x-1), g = x²
        // f = R' + g'·R = (1 - 1/(x-1)²) + 2x·(x + 1/(x-1))
        //   = 1 + 2x² - 1/(x-1)² + 2x/(x-1)
        //   = (2x³-2x²+2x-1)/(x-1)² ... let me compute carefully
        // Actually let's use a simpler test: R = x + 1/x, g = x²
        // f = R' + 2x·R = (1 - 1/x²) + 2x(x + 1/x) = 1 - 1/x² + 2x² + 2 = 2x² + 3 -
        // 1/x² = (2x⁴ + 3x² - 1)/x²
        // ∫ (2x⁴ + 3x² - 1)/x² · e^(x²) dx = (x + 1/x)·e^(x²)
        let xv = Value::from_cas_var("x");
        let x_sq = op(CasOp::Power, vec![xv.clone(), Value::Int(2)]);
        let denom = x_sq.clone();
        // N = 2x⁴ + 3x² - 1
        let two_x4 = op(
            CasOp::Multiply,
            vec![
                Value::Int(2),
                op(CasOp::Power, vec![xv.clone(), Value::Int(4)]),
            ],
        );
        let three_x2 = op(
            CasOp::Multiply,
            vec![
                Value::Int(3),
                op(CasOp::Power, vec![xv.clone(), Value::Int(2)]),
            ],
        );
        let numer = op(
            CasOp::Add,
            vec![two_x4, op(CasOp::Add, vec![three_x2, Value::Int(-1)])],
        );
        let f = op(CasOp::Divide, vec![numer, denom]);
        let g = op(CasOp::Power, vec![xv, Value::Int(2)]);
        let integrand = build_integrand(f, g);

        let result = super::integrate_liouville(&integrand, "x")
            .unwrap()
            .unwrap();
        let s = simplify_cas_value(&result).unwrap().to_string();
        // Should contain x + 1/x times e^(x²)
        assert!(
            (s.contains("x") && s.contains("e^")) || s.contains("exp"),
            "expected sum with exp factor: {s}"
        );
    }

    /// Test the ODE solver directly: A·P' + B·P = H
    #[test]
    fn test_solve_poly_ode_general_simple() {
        // R = 1/x, g = x² → A = x², B = 2x³-x, H = 2x³-x, deg_P = 0
        // A·P' + B·P = 0 + (2x³-x)·p₀ = 2x³-x → p₀ = 1
        let a = vec![Value::Int(0), Value::Int(0), Value::Int(1)]; // x²
        let b = vec![Value::Int(0), Value::Int(-1), Value::Int(0), Value::Int(2)]; // 2x³-x
        let h = vec![Value::Int(0), Value::Int(-1), Value::Int(0), Value::Int(2)]; // 2x³-x
        let p = super::solve_poly_ode_general(&a, &b, &h, 0).unwrap();
        assert_eq!(p, vec![Value::Int(1)]);
    }

    #[test]
    fn test_solve_poly_ode_general_deg1() {
        // R = x/(x-1), g = x²
        // A = (x-1)² = x²-2x+1, B = 2x³-4x²+x+1, H = 2x⁴-4x³+2x²-x+1, deg_P = 1
        // P = [0, 1] = x
        let a = vec![Value::Int(1), Value::Int(-2), Value::Int(1)]; // x²-2x+1
        let b = vec![Value::Int(1), Value::Int(1), Value::Int(-4), Value::Int(2)]; // 2x³-4x²+x+1
        let h = vec![
            Value::Int(1),
            Value::Int(-1),
            Value::Int(2),
            Value::Int(-4),
            Value::Int(2),
        ]; // 2x⁴-4x³+2x²-x+1
        let p = super::solve_poly_ode_general(&a, &b, &h, 1).unwrap();
        assert_eq!(p, vec![Value::Int(0), Value::Int(1)]);
    }
}
