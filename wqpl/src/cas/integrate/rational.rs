use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::{One, ToPrimitive, Zero};

use crate::cas::{
    cas_add, cas_div, cas_err, cas_mul, cas_pow, cas_product, cas_sub, eval_exact_numeric_div,
    numeric_add, numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul, numeric_sub,
    poly_add, poly_const_mul, poly_degree, poly_derivative, poly_divide, poly_evaluate,
    poly_from_expr, poly_gcd, poly_interpolate, poly_is_zero, poly_mul, poly_neg, poly_resultant,
    poly_sub, poly_to_expr, poly_trim, simplify_cas_value,
};
use crate::value::algebraic::{AlgebraicData, AlgebraicField};
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

/// Try to extract (numerator_poly, denominator_poly) from an expression as a
/// rational function in `var`.
///
/// Returns `None` if the expression is not a rational function in `var`
/// (e.g., contains transcendental functions, non-polynomial subexpressions,
/// etc.).
fn extract_rational(expr: &Value, var: &str) -> WqResult<Option<(Vec<Value>, Vec<Value>)>> {
    // Case: pure polynomial, no negative powers present
    if !contains_var_negative_power(expr, var) {
        match poly_from_expr(expr, var) {
            Ok(num) => return Ok(Some((num, vec![Value::Int(1)]))),
            Err(_) => return Ok(None),
        }
    }

    // Case: base^(-k) where k > 0
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && let Some(k) = exp.exact_int()
        && k < BigInt::zero()
    {
        let k_abs = (-k).to_usize().unwrap_or(0);
        if k_abs == 0 {
            return Ok(None);
        }
        let base_poly = match poly_from_expr(base, var) {
            Ok(p) => p,
            Err(_) => return Ok(None),
        };
        let denom = poly_pow(&base_poly, k_abs)?;
        return Ok(Some((vec![Value::Int(1)], denom)));
    }

    // Case: product containing negative-power factors and possibly polynomial
    // factors
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        return extract_rational_product(args, var);
    }

    // Not a recognized rational function form
    Ok(None)
}

/// Check if expression contains any negative power of `var`.
fn contains_var_negative_power(expr: &Value, var: &str) -> bool {
    if expr.cas_var_name().is_some() {
        return false;
    }
    if let Some((op, args)) = expr.cas_op_parts() {
        if op == CasOp::Power
            && args.len() == 2
            && let Some((CasOp::Power, [base, exp])) = args[0].cas_op_parts()
            && let Some(k) = exp.exact_int()
            && k < BigInt::zero()
            && contains_var(base, var)
        {
            return true;
        }
        if op == CasOp::Power
            && args.len() == 2
            && args[1].exact_int().is_some_and(|k| k < BigInt::zero())
            && contains_var(&args[0], var)
        {
            return true;
        }
        for arg in args {
            if contains_var_negative_power(arg, var) {
                return true;
            }
        }
    }
    if let Some((_, args)) = expr.cas_function_parts() {
        for arg in args {
            if contains_var_negative_power(arg, var) {
                return true;
            }
        }
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        for arg in args {
            if contains_var_negative_power(arg, var) {
                return true;
            }
        }
    }
    false
}

/// Check if expression contains the variable `var`.
fn contains_var(expr: &Value, var: &str) -> bool {
    if expr.cas_var_name() == Some(var) {
        return true;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        for arg in args {
            if contains_var(arg, var) {
                return true;
            }
        }
    }
    if let Some((_, args)) = expr.cas_function_parts() {
        for arg in args {
            if contains_var(arg, var) {
                return true;
            }
        }
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        for arg in args {
            if contains_var(arg, var) {
                return true;
            }
        }
    }
    false
}

/// Extract rational function from a product expression.
fn extract_rational_product(
    args: &[Value],
    var: &str,
) -> WqResult<Option<(Vec<Value>, Vec<Value>)>> {
    let mut num_factors: Vec<Value> = Vec::new();
    let mut denom_factors: Vec<Value> = Vec::new();

    for arg in args {
        // Check for negative power factor: base^(-k)
        if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && let Some(k) = exp.exact_int()
            && k < BigInt::zero()
        {
            let k_abs = (-k).to_usize().unwrap_or(0);
            if k_abs == 0 {
                continue;
            }
            let base_poly = match poly_from_expr(base, var) {
                Ok(p) => p,
                Err(_) => return Ok(None),
            };
            let powered = poly_pow(&base_poly, k_abs)?;
            denom_factors.push(poly_to_expr(&powered, var)?);
            continue;
        }

        // Check if it's a polynomial factor
        if is_polynomial_in_var(arg, var) {
            num_factors.push(arg.clone());
            continue;
        }

        // Numeric constant
        if !arg.is_cas_expr() {
            num_factors.push(arg.clone());
            continue;
        }

        // Can't recognize
        return Ok(None);
    }

    let num = if num_factors.is_empty() {
        vec![Value::Int(1)]
    } else {
        let num_expr = cas_product(num_factors.to_vec());
        poly_from_expr(&num_expr, var).unwrap_or_else(|_| vec![Value::Int(1)])
    };

    let denom = if denom_factors.is_empty() {
        vec![Value::Int(1)]
    } else {
        let denom_expr = cas_product(denom_factors.to_vec());
        poly_from_expr(&denom_expr, var).unwrap_or_else(|_| vec![Value::Int(1)])
    };

    Ok(Some((num, denom)))
}

/// Check if an expression is a polynomial in `var`.
fn is_polynomial_in_var(expr: &Value, var: &str) -> bool {
    if !expr.is_cas_expr() {
        return true;
    }
    poly_from_expr(expr, var).is_ok()
}

/// Compute poly^k using repeated squaring on coefficient vectors.
pub(crate) fn poly_pow(poly: &[Value], k: usize) -> WqResult<Vec<Value>> {
    match k {
        0 => Ok(vec![Value::Int(1)]),
        1 => Ok(poly.to_vec()),
        _ => {
            let mut result = vec![Value::Int(1)];
            let mut base = poly.to_vec();
            let mut exp = k;
            while exp > 0 {
                if exp & 1 == 1 {
                    result = poly_mul(&result, &base)?;
                }
                exp >>= 1;
                if exp > 0 {
                    base = poly_mul(&base, &base)?;
                }
            }
            Ok(result)
        }
    }
}

/// Computes the square-free factorization of a polynomial using Yun's
/// algorithm.
///
/// Returns a list of `(factor_poly, multiplicity)` where each `factor_poly` is
/// square-free and pairwise coprime, and the original polynomial is product
/// factor_i^i.
pub(super) fn square_free_factor(poly: &[Value], _var: &str) -> WqResult<Vec<(Vec<Value>, usize)>> {
    crate::cas::square_free_factor(poly)
}

// ---------------------------------------------------------------------------
// Integration entry point and helpers
// ---------------------------------------------------------------------------

/// Strategy entry point: integrate a rational function in `var`.
///
/// Returns `Some(result)` on success, `None` if the expression is not a
/// rational function or the integration cannot be completed.
pub(super) fn integrate_by_rational(
    expr: &Value,
    var: &str,
    _debug: crate::cas::CasDebug<'_>,
) -> WqResult<Option<Value>> {
    let (mut numer, mut denom) = match extract_rational(expr, var)? {
        Some(pair) => pair,
        None => return Ok(None),
    };

    // Ensure denominator has positive leading coefficient for normalization
    if !denom.is_empty() && numeric_is_negative(denom.last().expect("non-empty denom")) {
        numer = poly_neg(&numer);
        denom = poly_neg(&denom);
    }

    // Case: denominator is constant -> integrate polynomial
    if poly_degree(&denom) == 0 && denom[0] != Value::Int(0) {
        let recip = eval_exact_numeric_div(&Value::Int(1), &denom[0])?;
        let scaled = poly_const_mul(&numer, &recip)?;
        return integrate_polynomial(&scaled, var).map(Some);
    }

    // Reduce improper fraction: polynomial division
    let (quotient, remainder) = poly_divide(&numer, &denom)?;
    let poly_result = if poly_is_zero(&quotient) {
        Value::Int(0)
    } else {
        integrate_polynomial(&quotient, var)?
    };

    if poly_is_zero(&remainder) {
        return Ok(Some(poly_result));
    }

    // Try binomial formula x^n +/- a before general proper rational integration
    if poly_is_zero(&quotient)
        && let Some(result) = try_integrate_binomial(&remainder, &denom, var)?
    {
        return Ok(Some(result));
    }

    // Proper rational function integration
    let factors = square_free_factor(&denom, var)?;
    let rational_result = integrate_proper_rational(&remainder, &denom, &factors, var)?;

    if poly_result == Value::Int(0) {
        return Ok(Some(rational_result));
    }

    simplify_cas_value(&cas_add(vec![poly_result, rational_result])?).map(Some)
}

/// Integrate each term of a polynomial via the power rule.
fn integrate_polynomial(coeffs: &[Value], var: &str) -> WqResult<Value> {
    let mut terms = Vec::new();
    for (deg, coeff) in coeffs.iter().enumerate() {
        if numeric_is_zero(coeff) {
            continue;
        }
        let new_deg = deg + 1;
        let new_exp = Value::from_bigint(BigInt::from(new_deg));
        let divided = eval_exact_numeric_div(coeff, &new_exp)?;
        if new_deg == 1 {
            terms.push(cas_mul(vec![divided, Value::from_cas_var(var)])?);
        } else {
            let power = cas_pow(Value::from_cas_var(var), new_exp)?;
            terms.push(cas_mul(vec![divided, power])?);
        }
    }
    if terms.is_empty() {
        Ok(Value::Int(0))
    } else if terms.len() == 1 {
        Ok(terms.into_iter().next().expect("single term"))
    } else {
        cas_add(terms)
    }
}

/// Try to integrate 1/(x^n - a) for odd n using the known algebraic formula.
/// Returns Some(result) on success, None if the expression doesn't match.
fn try_integrate_binomial(numer: &[Value], denom: &[Value], var: &str) -> WqResult<Option<Value>> {
    let n = poly_degree(denom);
    // Only odd n >= 3
    // Only odd n in [5] currently supported (trig values for n=3 are
    // handled by the existing factor_polynomial / RT code)
    if n < 5 || n.is_multiple_of(2) || n > 5 {
        return Ok(None);
    }

    // Numerator must be constant 1
    if numer.len() != 1 || !numeric_is_one(&numer[0]) {
        return Ok(None);
    }

    // Check binomial pattern: only leading and constant coefficients are non-zero
    let lead = &denom[n];
    if !numeric_is_one(lead) {
        return Ok(None);
    }
    for c in denom.iter().take(n).skip(1) {
        if !numeric_is_zero(c) {
            return Ok(None);
        }
    }

    let const_term = &denom[0];
    // Handle x^n - a (constant negative): real root a^(1/n)
    // Handle x^n + a (constant positive): real root -a^(1/n)
    let (a, sign) = if numeric_is_negative(const_term) {
        (numeric_mul(const_term, &Value::Int(-1))?, Value::Int(1))
    } else if !numeric_is_zero(const_term) && !numeric_is_negative(const_term) {
        (const_term.clone(), Value::Int(-1))
    } else {
        return Ok(None);
    };

    // Build polynomial x^n - a (for root finding)
    let mut root_poly = vec![Value::Int(0); n + 1];
    root_poly[0] = numeric_mul(&a, &Value::Int(-1))?; // -a
    root_poly[n] = Value::Int(1);
    let p = match find_real_algebraic_root(&root_poly) {
        Some(v) => v,
        None => return Ok(None),
    };

    // For x^n + a: root is -p (i.e., p from x^n - (-a) gives root of x^n = a)
    let root = if sign == Value::Int(-1) {
        numeric_mul(&p, &Value::Int(-1))?
    } else {
        p.clone()
    };

    // Denominator factor: n * a^(1-1/n) = n * root^(n-1) for x^n - a
    // For x^n + a: n * a * root^(n-1) ... wait, let me recompute.
    // Actually: 1/(x^n - a) has residue 1/(n*a^(1-1/n)) at real root a^(1/n)
    // For x^n + a with odd n: 1/(x^n + a) has residue 1/(n*a^(1-1/n)) at root
    // -a^(1/n) Wait no: d/dx (x^n +/- a) at x = root = n * root^(n-1)
    // For x^n - a at x=a^(1/n): derivative = n * a^(1-1/n)
    // For x^n + a at x=-a^(1/n): derivative = n * (-a^(1/n))^(n-1) = n * a^(1-1/n)
    // (since n-1 is even for odd n) So both have residue 1/(n * a^(1-1/n)) at
    // the real root.
    let root_n1 = {
        let exp = Value::from_bigint(BigInt::from(n as i64 - 1));
        // root^(n-1) as a CAS expression
        // But root is Algebraic, and cas_pow may or may not simplify
        // Use cas_pow for the symbolic power
        cas_pow(root.clone(), exp)?
    };
    let n_val = Value::from_bigint(BigInt::from(n as i64));
    let denom_factor = simplify_cas_value(&cas_mul(vec![n_val, root_n1])?)?;

    // Real root term: (1/denom_factor) * ln|x - root|
    let x_val = Value::from_cas_var(var);
    let x_minus_root = cas_sub(x_val.clone(), root.clone())?;
    let ln_abs = Value::from_cas_function(
        CasFunction::Ln,
        vec![Value::from_cas_function(
            CasFunction::Abs,
            vec![x_minus_root],
        )],
    );
    let one_val = Value::Int(1);
    let real_coeff = simplify_cas_value(&cas_div(one_val.clone(), denom_factor.clone())?)?;
    let mut result_terms = vec![cas_mul(vec![real_coeff, ln_abs])?];

    // Complex conjugate pairs for k = 1..(n-1)/2
    let p_sq = simplify_cas_value(&cas_pow(root.clone(), Value::Int(2))?)?;
    let n_u32 = match u32::try_from(n) {
        Ok(n) => n,
        Err(_) => return Ok(None),
    };
    for k in 1..=(n - 1) / 2 {
        let k_u32 = u32::try_from(k).expect("k is bounded by n_u32");
        let (cos_val, sin_val) = match get_trig_values(k_u32, n_u32) {
            Some(v) => v,
            None => return Ok(None),
        };

        // Quadratic factor: x^2 - 2*root*cos_val*x + root^2
        let two_root = simplify_cas_value(&cas_mul(vec![Value::Int(2), root.clone()])?)?;
        let b_coeff = simplify_cas_value(&cas_mul(vec![two_root, cos_val.clone()])?)?;
        // b = -2*root*cos (for x^2 + bx + c form)
        let neg_b = numeric_mul(&b_coeff, &Value::Int(-1))?;
        let quad = simplify_cas_value(&cas_add(vec![
            cas_pow(x_val.clone(), Value::Int(2))?,
            cas_mul(vec![neg_b, x_val.clone()])?,
            p_sq.clone(),
        ])?)?;

        // ln term: (cos_val / denom_factor) * ln|quad|
        let ln_coeff = simplify_cas_value(&cas_div(cos_val.clone(), denom_factor.clone())?)?;
        let ln_abs_quad = Value::from_cas_function(
            CasFunction::Ln,
            vec![Value::from_cas_function(CasFunction::Abs, vec![quad])],
        );
        result_terms.push(cas_mul(vec![ln_coeff, ln_abs_quad])?);

        // arctan term: (-2*sin_val / denom_factor) * arctan((x -
        // root*cos_val)/(root*sin_val))
        let root_cos = simplify_cas_value(&cas_mul(vec![root.clone(), cos_val])?)?;
        let root_sin = simplify_cas_value(&cas_mul(vec![root.clone(), sin_val.clone()])?)?;
        let arctan_inner =
            simplify_cas_value(&cas_div(cas_sub(x_val.clone(), root_cos)?, root_sin)?)?;
        let arctan_call = Value::from_cas_function(CasFunction::ArcTan, vec![arctan_inner]);
        let neg_two = Value::Int(-2);
        let neg_two_sin = simplify_cas_value(&cas_mul(vec![neg_two, sin_val])?)?;
        let arctan_coeff = simplify_cas_value(&cas_div(neg_two_sin, denom_factor.clone())?)?;
        result_terms.push(cas_mul(vec![arctan_coeff, arctan_call])?);
    }

    let result = simplify_cas_value(&cas_add(result_terms)?)?;
    Ok(Some(result))
}

/// Get cos(2*pi*k/n) and sin(2*pi*k/n) as CAS Value expressions for n=3,5.
fn get_trig_values(k: u32, n: u32) -> Option<(Value, Value)> {
    let result: WqResult<Option<(Value, Value)>> = (|| {
        match (n, k) {
            // n=3: cos(120 deg) = -1/2, sin(120 deg) = sqrt(3)/2
            (3, 1) => {
                let neg_half = Value::from_fraction_parts((-1i64).into(), 2u64.into());
                let sqrt3 = Value::from_cas_function(CasFunction::Sqrt, vec![Value::Int(3)]);
                let sin_val = simplify_cas_value(&cas_div(sqrt3, Value::Int(2))?)?;
                Ok(Some((neg_half, sin_val)))
            }
            // n=5: cos(72 deg) = (sqrt(5)-1)/4, sin(72 deg) = sqrt(10+2*sqrt(5))/4
            (5, 1) => {
                let sqrt5 = Value::from_cas_function(CasFunction::Sqrt, vec![Value::Int(5)]);
                let cos_val = simplify_cas_value(&cas_div(
                    cas_sub(sqrt5.clone(), Value::Int(1))?,
                    Value::Int(4),
                )?)?;
                let inner = cas_add(vec![Value::Int(10), cas_mul(vec![Value::Int(2), sqrt5])?])?;
                let sin_val = simplify_cas_value(&cas_div(
                    Value::from_cas_function(CasFunction::Sqrt, vec![inner]),
                    Value::Int(4),
                )?)?;
                Ok(Some((cos_val, sin_val)))
            }
            // n=5: cos(144 deg) = -(sqrt(5)+1)/4, sin(144 deg) = sqrt(10-2*sqrt(5))/4
            (5, 2) => {
                let sqrt5 = Value::from_cas_function(CasFunction::Sqrt, vec![Value::Int(5)]);
                let numer = numeric_add(&numeric_mul(&sqrt5, &Value::Int(-1))?, &Value::Int(-1))?;
                // numer = -sqrt(5) - 1
                let cos_val = simplify_cas_value(&cas_div(numer, Value::Int(4))?)?;
                let inner = cas_sub(Value::Int(10), cas_mul(vec![Value::Int(2), sqrt5])?)?;
                let sin_val = simplify_cas_value(&cas_div(
                    Value::from_cas_function(CasFunction::Sqrt, vec![inner]),
                    Value::Int(4),
                )?)?;
                Ok(Some((cos_val, sin_val)))
            }
            _ => Ok(None),
        }
    })();
    result.ok().flatten()
}

/// Dispatch proper rational function N/D based on square-free factorisation of
/// D.
fn integrate_proper_rational(
    numer: &[Value],
    denom: &[Value],
    factors: &[(Vec<Value>, usize)],
    var: &str,
) -> WqResult<Value> {
    // Repeated quadratic Hermite reduction rewrites the whole rational
    // function, including all coprime denominator factors.  Handle one such
    // factor before the per-factor partial fraction loop so the remaining
    // factors are not integrated twice.
    if let Some((factor, mult)) = factors
        .iter()
        .find(|(factor, mult)| *mult > 1 && poly_degree(factor) == 2)
    {
        let b = factor.get(1).cloned().unwrap_or(Value::Int(0));
        let c = factor.first().cloned().unwrap_or(Value::Int(0));
        return integrate_repeated_quadratic(numer, denom, factor, &b, &c, *mult, var);
    }

    let mut result_terms: Vec<Value> = Vec::new();

    for (factor, mult) in factors {
        let factor_deg = poly_degree(factor);
        match factor_deg {
            1 => {
                let terms = integrate_linear_factor_all(numer, denom, factor, *mult, var)?;
                result_terms.push(terms);
            }
            2 => {
                let terms = integrate_quadratic_factor_all(numer, denom, factor, *mult, var)?;
                result_terms.push(terms);
            }
            _ => {
                // Try to split higher-degree factor using rational root finding
                let sub_factors = factor_polynomial(factor)?;
                if sub_factors.len() <= 1 {
                    // Fall back to Rothstein-Trager for the whole proper rational function
                    let rt_result = integrate_rothstein_trager(numer, denom, var)?;
                    result_terms.push(rt_result);
                    continue;
                }
                // Build new factor list with correct multiplicities and recurse
                let mut sub_with_mult: Vec<(Vec<Value>, usize)> = Vec::new();
                for sf in &sub_factors {
                    sub_with_mult.push((sf.clone(), *mult));
                }
                // Rebuild denominator with split factors
                let mut new_denom = vec![Value::Int(1)];
                for (sf, m) in &sub_with_mult {
                    let pow = poly_pow(sf, *m)?;
                    new_denom = poly_mul(&new_denom, &pow)?;
                }
                let terms = integrate_proper_rational(numer, &new_denom, &sub_with_mult, var)?;
                result_terms.push(terms);
            }
        }
    }

    if result_terms.is_empty() {
        return Ok(Value::Int(0));
    }
    if result_terms.len() == 1 {
        return Ok(result_terms.into_iter().next().expect("single term"));
    }
    simplify_cas_value(&cas_add(result_terms)?)
}

// ---------------------------------------------------------------------------
// Rothstein-Trager method for square-free denominator factors of any degree
// ---------------------------------------------------------------------------

/// Integrate a proper rational function N/D where D is square-free using the
/// Rothstein-Trager method.
///
/// Computes R(z) = resultant_x(D(x), N(x) - z*D'(x)), finds its rational
/// and real algebraic roots alpha, and returns sum alpha*ln(gcd(D, N -
/// alpha*D')).
fn integrate_rothstein_trager(numer: &[Value], denom: &[Value], var: &str) -> WqResult<Value> {
    let deg_d = poly_degree(denom);
    if deg_d == 0 {
        return Ok(Value::Int(0));
    }

    let d_deriv = poly_derivative(denom);

    // Determine degree of N - z*D': max(deg(N), deg(D'))
    let n_minus_z_d_deg = poly_degree(numer).max(poly_degree(&d_deriv));

    // Evaluate resultant R(z) = resultant_x(D, N - z*D') at deg(D) + 1 points
    let num_points = deg_d + 1;
    let mut points: Vec<(Value, Value)> = Vec::with_capacity(num_points);

    for i in 0..num_points {
        let zi = Value::from_bigint(BigInt::from(i));

        // Build N - zi*D' coefficient vector
        let mut nz = vec![Value::Int(0); n_minus_z_d_deg + 1];
        for (j, c) in numer.iter().enumerate() {
            nz[j] = c.clone();
        }
        for (j, dc) in d_deriv.iter().enumerate() {
            let term = numeric_mul(&zi, dc)?;
            nz[j] = numeric_sub(&nz[j], &term)?;
        }
        poly_trim(&mut nz);

        let r_i = poly_resultant(denom, &nz)?;
        points.push((zi, r_i));
    }

    // Interpolate R(z)
    let r_coeffs = poly_interpolate(&points)?;

    // Find all real roots of R(z): both rational and algebraic
    let (rational_roots, algebraic_roots) = find_real_roots_poly(&r_coeffs);

    // For each root alpha, compute V_alpha = gcd(D, N - alpha*D')
    let mut terms = Vec::new();
    let mut accumulated_gcd = vec![Value::Int(1)];
    let mut v_alpha_pairs: Vec<(Value, Vec<Value>)> = Vec::new();

    let mut all_roots: Vec<Value> = rational_roots;
    all_roots.extend(algebraic_roots);

    for alpha in &all_roots {
        // Build N - alpha*D'
        let mut n_alpha = vec![Value::Int(0); n_minus_z_d_deg + 1];
        for (j, c) in numer.iter().enumerate() {
            n_alpha[j] = c.clone();
        }
        for (j, dc) in d_deriv.iter().enumerate() {
            let term = numeric_mul(alpha, dc)?;
            n_alpha[j] = numeric_sub(&n_alpha[j], &term)?;
        }
        poly_trim(&mut n_alpha);

        let v_alpha = poly_gcd(denom, &n_alpha)?;
        if poly_degree(&v_alpha) > 0 {
            v_alpha_pairs.push((alpha.clone(), v_alpha.clone()));
            // Multiply accumulated gcd by v_alpha
            accumulated_gcd = poly_mul(&accumulated_gcd, &v_alpha)?;

            // Build logarithmic term: alpha * ln(V_alpha(x))
            let v_expr = poly_to_expr(&v_alpha, var)?;
            let ln_term = Value::from_cas_function(
                CasFunction::Ln,
                vec![Value::from_cas_function(CasFunction::Abs, vec![v_expr])],
            );
            let term = if alpha == &Value::Int(1) {
                ln_term
            } else {
                cas_mul(vec![alpha.clone(), ln_term])?
            };
            terms.push(term);
        }
    }

    if terms.is_empty() {
        return Err(cas_err("Rothstein-Trager: no roots found for resultant"));
    }

    // Check if there's an unaccounted factor in denom
    let (remaining, _rem_check) = poly_divide(denom, &accumulated_gcd)?;
    if poly_degree(&remaining) > 0 {
        // Compute the proper numerator for the remaining factor by subtracting
        // the extracted log-term contributions from the original numerator.
        let rem_numer = compute_remaining_numer(numer, denom, &accumulated_gcd, &v_alpha_pairs)?;
        if let Some(term) = integrate_remaining_factor(&remaining, &rem_numer, var)? {
            terms.push(term);
        } else {
            return Err(cas_err(format!(
                "Rothstein-Trager: resultant has {} unaccounted root(s); cannot complete integration",
                poly_degree(&remaining)
            )));
        }
    }

    if terms.len() == 1 {
        return Ok(terms.into_iter().next().expect("single term"));
    }
    simplify_cas_value(&cas_add(terms)?)
}

/// Integrate a remaining monic quadratic factor with possibly algebraic
/// coefficients: int 1/(x^2 + px + q) dx = (2/r)*arctan((2x+p)/r) where
/// r = sqrt(4q-p^2) when 4q-p^2 > 0 (complex conjugate roots -> arctan).
/// Compute the numerator for the remaining denominator factor after extracting
/// Rothstein-Trager log terms. Subtracts the derivative of sum
/// alpha*ln(V_alpha) from the original rational function N/D, then divides by
/// accumulated_gcd.
fn compute_remaining_numer(
    numer: &[Value],
    denom: &[Value],
    accumulated_gcd: &[Value],
    v_alpha_pairs: &[(Value, Vec<Value>)],
) -> WqResult<Vec<Value>> {
    // extracted_numer = sum alpha * V_alpha' * (D / V_alpha)
    let mut extracted = vec![Value::Int(0)];
    for (alpha, v) in v_alpha_pairs {
        let v_deriv = poly_derivative(v);
        let (d_without_v, _) = poly_divide(denom, v)?;
        let term = poly_mul(&v_deriv, &d_without_v)?;
        let term = poly_const_mul(&term, alpha)?;
        extracted = poly_add(&extracted, &term)?;
    }
    let rem_numer = poly_sub(numer, &extracted)?;
    let (n_q, remainder) = poly_divide(&rem_numer, accumulated_gcd)?;
    if poly_degree(&remainder) > 0 {
        return Err(cas_err(
            "Rothstein-Trager: non-zero remainder when computing remaining numerator",
        ));
    }
    Ok(n_q)
}

fn integrate_remaining_factor(
    factor: &[Value],
    numer: &[Value],
    var: &str,
) -> WqResult<Option<Value>> {
    if poly_degree(factor) != 2 {
        return Ok(None);
    }
    // Monic quadratic: x^2 + p*x + q
    let p = factor.get(1).cloned().unwrap_or(Value::Int(0));
    let q = factor.first().cloned().unwrap_or(Value::Int(0));

    // Numerator: A*x + B
    let a = numer.get(1).cloned().unwrap_or(Value::Int(0));
    let b = numer.first().cloned().unwrap_or(Value::Int(0));

    let half = Value::from_fraction_parts(BigInt::one(), BigInt::from(2));

    let mut result_terms: Vec<Value> = Vec::new();

    // Log term: (A/2)*ln(x^2+px+q)
    let a_half = numeric_mul(&a, &half)?;
    if !numeric_is_zero(&a_half) {
        let quad_expr = poly_to_expr(factor, var)?;
        let log_term = Value::from_cas_function(
            CasFunction::Ln,
            vec![Value::from_cas_function(CasFunction::Abs, vec![quad_expr])],
        );
        result_terms.push(cas_mul(vec![a_half, log_term])?);
    }

    // Arctan term: (B - A*p/2)*int dx/(x^2+px+q)
    let ap_half = numeric_mul(&numeric_mul(&a, &p)?, &half)?;
    let c_prime = numeric_sub(&b, &ap_half)?;
    if !numeric_is_zero(&c_prime) {
        // Discriminant: 4q - p^2
        let four_q = numeric_mul(&Value::Int(4), &q)?;
        let p_sq = numeric_mul(&p, &p)?;
        let disc = numeric_sub(&four_q, &p_sq)?;

        if numeric_is_negative(&disc) || numeric_is_zero(&disc) {
            return Ok(None);
        }

        let sqrt_disc = Value::from_cas_function(CasFunction::Sqrt, vec![disc]);
        let x = Value::from_cas_var(var);
        let two_x_plus_p = cas_add(vec![cas_mul(vec![Value::Int(2), x])?, p.clone()])?;
        let arg = cas_div(two_x_plus_p, sqrt_disc.clone())?;
        let arctan_term = Value::from_cas_function(CasFunction::ArcTan, vec![arg]);
        let two_c = numeric_mul(&Value::Int(2), &c_prime)?;
        let prefactor = cas_div(two_c, sqrt_disc)?;
        result_terms.push(cas_mul(vec![prefactor, arctan_term])?);
    }

    if result_terms.is_empty() {
        return Ok(Some(Value::Int(0)));
    }
    if result_terms.len() == 1 {
        return Ok(Some(result_terms.into_iter().next().expect("single term")));
    }
    simplify_cas_value(&cas_add(result_terms)?).map(Some)
}

/// Extract (numer, denom) from a Value if it represents a rational number.
pub(super) fn rational_parts_value(value: &Value) -> Option<(BigInt, BigInt)> {
    match value {
        Value::Int(i) => Some((BigInt::from(*i), BigInt::one())),
        Value::BigInt(b) => Some((b.as_ref().clone(), BigInt::one())),
        Value::Fraction(f) => Some((f.numer().clone(), f.denom().clone())),
        _ => None,
    }
}

/// Compute LCM of two BigInts using Euclidean GCD.
fn num_integer_lcm(a: &BigInt, b: &BigInt) -> BigInt {
    if a.is_zero() || b.is_zero() {
        return BigInt::one();
    }
    let gcd = bigint_gcd(a, b);
    (a / &gcd) * b
}

fn bigint_gcd(a: &BigInt, b: &BigInt) -> BigInt {
    let mut a = a.clone();
    let mut b = b.clone();
    while b != BigInt::zero() {
        let r = &a % &b;
        a = b;
        b = r;
    }
    a
}

/// For an irreducible polynomial over Q of degree >= 2, find a real root as an
/// Algebraic number. Returns None if no real root can be isolated.
fn find_real_algebraic_root(poly: &[Value]) -> Option<Value> {
    let deg = poly_degree(poly);
    if deg < 2 {
        return None;
    }

    // Clear denominators to get an integer polynomial
    let mut lcm = BigInt::one();
    for c in poly.iter().take(deg + 1) {
        if let Some((_, d)) = rational_parts_value(c) {
            lcm = num_integer_lcm(&lcm, &d);
        }
    }
    let lcm_val = Value::from_bigint(lcm.clone());

    // Convert to integer coefficients by clearing denominators
    let scaled: Vec<BigInt> = poly[..=deg]
        .iter()
        .map(|c| {
            let s = numeric_mul(c, &lcm_val).unwrap_or_else(|_| c.clone());
            match rational_parts_value(&s) {
                Some((n, _)) => n,
                None => BigInt::zero(),
            }
        })
        .collect();

    // Use the scaled integer polynomial for root isolation; the field
    // constructor normalizes it before storing identity.
    let poly_arc: Arc<[BigInt]> = Arc::from(scaled.clone());

    let interval = isolate_root_interval(&poly_arc)?;

    // Build generator alpha with proper basis: [0, 1, 0, ..., 0] with length = deg
    let deg = poly_arc.len().saturating_sub(1);
    if deg == 1 {
        // Degree 1 polynomial
        return None;
    }
    let field = AlgebraicField::new_real_root(scaled, interval).ok()?;
    let Value::Algebraic(alg) = AlgebraicData::generator(field).ok()? else {
        return None;
    };
    let alg = (*alg).clone();
    // Normalize pure-power fields (e.g. Q(cbrt(1/108)) -> Q(cbrt(2)))
    if let Some(normalized) = crate::value::algebraic::normalize_algebraic_field(&alg) {
        return Some(Value::Algebraic(Arc::new(normalized)));
    }
    Some(Value::Algebraic(Arc::new(alg)))
}

/// Find an isolating interval for a real root of a polynomial.
///
/// Uses sign-change scanning outward from 0 to bracket the root, then
/// bisection to narrow, followed by Newton refinement for a tight bound.
fn isolate_root_interval(poly: &[BigInt]) -> Option<(f64, f64)> {
    let deg = poly.len().saturating_sub(1);
    if deg == 0 {
        return None;
    }

    let eval = |x: f64| -> Option<f64> {
        // Horner's method for better numerical stability
        let mut result = 0.0_f64;
        for c in poly.iter().rev() {
            result = result * x + c.to_f64()?;
        }
        Some(result)
    };

    let eval_deriv = |x: f64| -> Option<f64> {
        let mut result = 0.0_f64;
        for (i, c) in poly.iter().enumerate().skip(1) {
            result = result * x + c.to_f64()? * (i as f64);
        }
        // Horner is messy for derivative; use standard eval
        let mut r = 0.0_f64;
        for (i, c) in poly.iter().enumerate().skip(1) {
            r += c.to_f64()? * (i as f64) * x.powi(i as i32 - 1);
        }
        Some(r)
    };

    let f0 = eval(0.0)?;
    // Root near 0: use Newton starting from 0
    if f0.abs() < 1e-12 {
        return newton_refine_interval(&eval, &eval_deriv, 0.0);
    }

    // Estimate the maximum root magnitude (Cauchy bound)
    let max_root = cauchy_bound(poly);

    // Scan outward from 0 with exponential steps to find a sign change
    let mut bound = 1.0_f64;
    for _ in 0..40 {
        if bound > max_root * 2.0 {
            break;
        }
        let f_pos = eval(bound)?;
        if f0 * f_pos <= 0.0 {
            return bisect_and_refine(&eval, &eval_deriv, 0.0, bound);
        }
        let f_neg = eval(-bound)?;
        if f0 * f_neg <= 0.0 {
            return bisect_and_refine(&eval, &eval_deriv, -bound, 0.0);
        }
        bound *= 1.6;
    }

    // If no sign change found within max_root, scan the full range
    bound = max_root;
    if bound > 1.5 {
        let f_pos = eval(bound)?;
        if f0 * f_pos <= 0.0 {
            return bisect_and_refine(&eval, &eval_deriv, 0.0, bound);
        }
        let f_neg = eval(-bound)?;
        if f0 * f_neg <= 0.0 {
            return bisect_and_refine(&eval, &eval_deriv, -bound, 0.0);
        }
    }
    None
}

/// Cauchy's bound: all real roots of a0 + ... + an*x^n lie in (-M, M)
/// where M = 1 + max_{0<=k<n} |a_k/a_n|.
fn cauchy_bound(poly: &[BigInt]) -> f64 {
    let n = poly.len().saturating_sub(1);
    if n == 0 {
        return 0.0;
    }
    let an = poly[n].to_f64().unwrap_or(1.0).abs();
    if an < 1e-15 {
        return 1e6;
    }
    let mut max_ratio = 0.0_f64;
    for c in poly.iter().take(n) {
        let cf = c.to_f64().unwrap_or(0.0).abs();
        let ratio = cf / an;
        if ratio > max_ratio {
            max_ratio = ratio;
        }
    }
    1.0 + max_ratio
}

/// Bisection to narrow the bracket, then Newton refinement for tight bounds.
fn bisect_and_refine(
    eval: &dyn Fn(f64) -> Option<f64>,
    eval_deriv: &dyn Fn(f64) -> Option<f64>,
    mut lo: f64,
    mut hi: f64,
) -> Option<(f64, f64)> {
    if lo > hi {
        std::mem::swap(&mut lo, &mut hi);
    }

    // Bisection: narrow to ~1e-7 precision
    for _ in 0..55 {
        let mid = (lo + hi) * 0.5;
        let fmid = eval(mid)?;
        if fmid.abs() < 1e-15 {
            // Exact hit
            let eps = mid.abs().max(1.0) * 1e-12 + 1e-10;
            return Some((mid - eps, mid + eps));
        }
        let flo = eval(lo)?;
        if flo * fmid <= 0.0 {
            hi = mid;
        } else {
            lo = mid;
        }
    }

    // Newton refinement from the bisection midpoint
    newton_refine_interval(eval, eval_deriv, (lo + hi) * 0.5)
}

/// Apply Newton's method from an initial guess to find a tight root
/// approximation, then expand by a small epsilon to form an isolating interval.
fn newton_refine_interval(
    eval: &dyn Fn(f64) -> Option<f64>,
    eval_deriv: &dyn Fn(f64) -> Option<f64>,
    mut x: f64,
) -> Option<(f64, f64)> {
    // Newton iteration
    for _ in 0..15 {
        let fx = eval(x)?;
        let fpx = eval_deriv(x)?;
        if fpx.abs() < 1e-15 {
            break;
        }
        let dx = fx / fpx;
        x -= dx;
        if dx.abs() < 1e-14 {
            break;
        }
    }

    // Ensure the returned interval actually brackets the root:
    // expand until we get a sign change or hit a reasonable epsilon.
    let mut eps = x.abs().max(1.0) * 1e-12 + 1e-10;
    for _ in 0..8 {
        let lo = x - eps;
        let hi = x + eps;
        let flo = eval(lo)?;
        let fhi = eval(hi)?;
        if flo * fhi <= 0.0 {
            return Some((lo, hi));
        }
        eps *= 10.0;
    }
    None
}

/// Find all real roots of a polynomial, including both rational and algebraic
/// roots. Returns (rational_roots, algebraic_roots).
fn find_real_roots_poly(poly: &[Value]) -> (Vec<Value>, Vec<Value>) {
    let mut rational = Vec::new();
    let mut algebraic = Vec::new();

    let mut remaining = poly.to_vec();
    poly_trim(&mut remaining);

    // Extract z=0 roots
    loop {
        if !remaining.first().is_some_and(numeric_is_zero) {
            break;
        }
        if !rational.contains(&Value::Int(0)) {
            rational.push(Value::Int(0));
        }
        let new_len = remaining.len().saturating_sub(1);
        if new_len == 0 {
            remaining = vec![];
            break;
        }
        let mut new_coeffs = vec![Value::Int(0); new_len];
        new_coeffs.clone_from_slice(&remaining[1..=new_len]);
        remaining = new_coeffs;
        poly_trim(&mut remaining);
    }

    if poly_is_zero(&remaining) {
        return (rational, algebraic);
    }

    // Try factoring via radical formulas first (degree 3 or 4)
    let deg = poly_degree(&remaining);
    if matches!(deg, 3 | 4)
        && let Ok(Some(factors)) = factor_by_radical_formula(&remaining)
    {
        for factor in factors {
            collect_real_roots_from_factor(&factor, &mut rational, &mut algebraic);
        }
        return (rational, algebraic);
    }

    // General case: try rational roots
    let mut current = remaining.clone();
    let mut cur_deg = poly_degree(&current);

    while cur_deg >= 1 {
        if let Some(linear_factor) = find_rational_root(&current) {
            if let Ok((quotient, _)) = poly_divide(&current, &linear_factor) {
                if let Ok(root) = numeric_mul(&linear_factor[0], &Value::Int(-1)) {
                    let root = eval_exact_numeric_div(&root, &linear_factor[1]).unwrap_or(root);
                    if !rational.contains(&root) {
                        rational.push(root);
                    }
                }
                current = quotient;
                poly_trim(&mut current);
                cur_deg = poly_degree(&current);
            } else {
                break;
            }
        } else {
            break;
        }
    }

    // For the remaining irreducible factor, try to find real algebraic roots
    cur_deg = poly_degree(&current);
    if cur_deg >= 2 {
        // Try radical factorization for the remaining factor
        if matches!(cur_deg, 3 | 4)
            && let Ok(Some(factors)) = factor_by_radical_formula(&current)
        {
            for factor in factors {
                collect_real_roots_from_factor(&factor, &mut rational, &mut algebraic);
            }
            return (rational, algebraic);
        }

        // For any remaining irreducible factor, try to find a real algebraic root.
        // After finding one, also check its negation
        if let Some(alg_root) = find_real_algebraic_root(&current) {
            if !algebraic.contains(&alg_root) {
                algebraic.push(alg_root.clone());
            }
            let neg_root = numeric_mul(&alg_root, &Value::Int(-1)).ok();
            if let Some(neg) = neg_root
                && !algebraic.contains(&neg)
                && let Ok(val) = poly_evaluate(&current, &neg)
                && numeric_is_zero(&val)
            {
                algebraic.push(neg);
            }
        }
    }

    (rational, algebraic)
}

fn collect_real_roots_from_factor(
    factor: &[Value],
    rational: &mut Vec<Value>,
    algebraic: &mut Vec<Value>,
) {
    match poly_degree(factor) {
        0 => {}
        1 => {
            let root = numeric_mul(&factor[0], &Value::Int(-1))
                .map(|r| eval_exact_numeric_div(&r, &factor[1]).unwrap_or(r))
                .unwrap_or_else(|_| factor[0].clone());
            push_real_root(root, rational, algebraic);
        }
        2 => {
            if let Ok(Some(roots)) = real_quadratic_roots(factor) {
                for root in roots {
                    push_real_root(root, rational, algebraic);
                }
                return;
            }
            push_algebraic_root_and_checked_negation(factor, rational, algebraic);
        }
        _ => push_algebraic_root_and_checked_negation(factor, rational, algebraic),
    }
}

fn real_quadratic_roots(poly: &[Value]) -> WqResult<Option<Vec<Value>>> {
    if poly_degree(poly) != 2 {
        return Ok(None);
    }

    let c = poly.first().cloned().unwrap_or(Value::Int(0));
    let b = poly.get(1).cloned().unwrap_or(Value::Int(0));
    let a = poly.get(2).cloned().unwrap_or(Value::Int(0));
    if numeric_is_zero(&a) {
        return Ok(None);
    }

    let four_ac = numeric_mul(&Value::Int(4), &numeric_mul(&a, &c)?)?;
    let disc = numeric_sub(&numeric_mul(&b, &b)?, &four_ac)?;
    if numeric_is_negative(&disc) {
        return Ok(Some(Vec::new()));
    }

    let neg_b = numeric_mul(&b, &Value::Int(-1))?;
    let two_a = numeric_mul(&Value::Int(2), &a)?;
    if numeric_is_zero(&disc) {
        return Ok(Some(vec![eval_exact_numeric_div(&neg_b, &two_a)?]));
    }

    let Some(sqrt_disc) = algebraic_sqrt_of_rational(&disc) else {
        return Ok(None);
    };
    let r1 = eval_exact_numeric_div(&numeric_add(&neg_b, &sqrt_disc)?, &two_a)?;
    let r2 = eval_exact_numeric_div(&numeric_sub(&neg_b, &sqrt_disc)?, &two_a)?;
    Ok(Some(vec![r1, r2]))
}

fn push_algebraic_root_and_checked_negation(
    factor: &[Value],
    rational: &mut Vec<Value>,
    algebraic: &mut Vec<Value>,
) {
    let Some(alg_root) = find_real_algebraic_root(factor) else {
        return;
    };
    push_real_root(alg_root.clone(), rational, algebraic);

    let neg_root = numeric_mul(&alg_root, &Value::Int(-1)).ok();
    if let Some(neg) = neg_root
        && let Ok(val) = poly_evaluate(factor, &neg)
        && numeric_is_zero(&val)
    {
        push_real_root(neg, rational, algebraic);
    }
}

fn push_real_root(root: Value, rational: &mut Vec<Value>, algebraic: &mut Vec<Value>) {
    let roots = if root.is_algebraic_number() {
        algebraic
    } else {
        rational
    };
    if !roots.contains(&root) {
        roots.push(root);
    }
}

/// Factor a square-free polynomial into linear and quadratic irreducible
/// factors by finding rational roots.
pub(crate) fn factor_polynomial(poly: &[Value]) -> WqResult<Vec<Vec<Value>>> {
    let mut remaining = poly.to_vec();
    poly_trim(&mut remaining);
    let mut factors: Vec<Vec<Value>> = Vec::new();

    let deg = poly_degree(&remaining);

    // Try cubic / quartic radical formulas for unfactored degree 3 or 4
    let radicals = if matches!(deg, 3 | 4) {
        factor_by_radical_formula(&remaining)?
    } else {
        None
    };
    if let Some(radical_factors) = radicals {
        return Ok(radical_factors);
    }

    // Find rational roots using Rational Root Theorem
    while poly_degree(&remaining) >= 3 {
        if let Some(linear_factor) = find_rational_root(&remaining) {
            let (quotient, _) = poly_divide(&remaining, &linear_factor)?;
            factors.push(linear_factor);
            remaining = quotient;
            poly_trim(&mut remaining);
        } else {
            break;
        }
    }

    if poly_degree(&remaining) > 0 {
        factors.push(remaining);
    }

    Ok(factors)
}

/// Factor a monic cubic by finding rational roots. Returns all linear factors
/// (up to 3) if fully reducible over Q, or a linear + quadratic factor if
/// partially reducible, or None if irreducible over Q.
fn solve_cubic_by_rational_root(poly: &[Value]) -> WqResult<Option<Vec<Vec<Value>>>> {
    if poly_degree(poly) != 3 {
        return Ok(None);
    }
    let mut factors = Vec::new();
    let mut remaining = poly.to_vec();
    while poly_degree(&remaining) >= 1 {
        if let Some(linear) = find_rational_root(&remaining) {
            let (quotient, _) = poly_divide(&remaining, &linear)?;
            factors.push(linear);
            remaining = quotient;
            poly_trim(&mut remaining);
        } else {
            break;
        }
    }
    if factors.is_empty() {
        Ok(None)
    } else {
        if poly_degree(&remaining) >= 1 {
            factors.push(remaining);
        }
        Ok(Some(factors))
    }
}

/// Factor a quartic into two quadratics via exact Ferrari formula.
///
/// Uses exact Value arithmetic. Handles the case where the resolvent cubic has
/// a rational root m with `2m - p > 0`. For irreducible resolvent cubics,
/// returns `Ok(None)` (fallback to Rothstein-Trager).
fn solve_quartic_exact(coeffs: &[Value]) -> WqResult<Option<Vec<Vec<Value>>>> {
    if poly_degree(coeffs) != 4 {
        return Ok(None);
    }

    let a = coeffs.get(4).cloned().unwrap_or(Value::Int(0));
    let b = coeffs.get(3).cloned().unwrap_or(Value::Int(0));
    let c = coeffs.get(2).cloned().unwrap_or(Value::Int(0));
    let d = coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    let e = coeffs.first().cloned().unwrap_or(Value::Int(0));

    if numeric_is_zero(&a) {
        return Ok(None);
    }

    let add = numeric_add;
    let sub = numeric_sub;
    let mul = numeric_mul;
    let div = |l: &Value, r: &Value| eval_exact_numeric_div(l, r);

    // 1. Normalise: make monic
    let b_n = div(&b, &a)?;
    let c_n = div(&c, &a)?;
    let d_n = div(&d, &a)?;
    let e_n = div(&e, &a)?;

    // 2. Depress: x = t - b_n/4  ->  t^4 + p*t^2 + q*t + r = 0
    let four = Value::Int(4);
    let b_over_4 = div(&b_n, &four)?;
    let shift = b_over_4; // t = x + shift

    let three = Value::Int(3);
    let eight = Value::Int(8);
    let two = Value::Int(2);

    // b_n_sq = b_n^2
    let b_n_sq = mul(&b_n, &b_n)?;
    // p = c_n - 3*b_n^2/8
    let p = sub(&c_n, &div(&mul(&three, &b_n_sq)?, &eight)?)?;
    // q = d_n - b_n*c_n/2 + b_n^3/8
    let b_n_cu = mul(&b_n_sq, &b_n)?;
    let q = add(
        &sub(&d_n, &div(&mul(&b_n, &c_n)?, &two)?)?,
        &div(&b_n_cu, &eight)?,
    )?;
    // r = e_n - b_n*d_n/4 + b_n^2*c_n/16 - 3*b_n^4/256
    let b_n_qu = mul(&b_n_cu, &b_n)?;
    let sixteen = Value::Int(16);
    let r = {
        let t1 = sub(&e_n, &div(&mul(&b_n, &d_n)?, &four)?)?;
        let t2 = add(&t1, &div(&mul(&b_n_sq, &c_n)?, &sixteen)?)?;
        sub(&t2, &div(&mul(&three, &b_n_qu)?, &Value::Int(256))?)?
    };

    // 3. Build resolvent cubic: m^3 - (p/2)*m^2 - r*m + (pr/2 - q^2/8) = 0
    let half = Value::from_fraction_parts(BigInt::one(), BigInt::from(2));
    let p_half = mul(&p, &half)?;
    let q_sq = mul(&q, &q)?;
    let q_sq_over_8 = div(&q_sq, &eight)?;
    let pr_half = mul(&mul(&p, &r)?, &half)?;
    let cubic_const = sub(&pr_half, &q_sq_over_8)?; // pr/2 - q^2/8
    let cubic_neg_r = mul(&r, &Value::Int(-1))?;
    let cubic_neg_p_half = mul(&p_half, &Value::Int(-1))?;

    let cubic_coeffs = vec![
        cubic_const.clone(),
        cubic_neg_r,
        cubic_neg_p_half,
        Value::Int(1),
    ];

    // 4. Find a real root m of the resolvent cubic with 2m - p >= 0
    let Some(m) = find_good_resolvent_root(&cubic_coeffs, &p)? else {
        return Ok(None);
    };

    // 5. Compute sqrt term s = sqrt(2m - p)
    let two_m = mul(&two, &m)?;
    let s_sq = sub(&two_m, &p)?;

    if numeric_is_negative(&s_sq) {
        return Ok(None);
    }
    if numeric_is_zero(&s_sq) {
        // s_sq = 0 means 2m = p. This degenerate case requires q = 0 and
        // m^2 >= r. Since find_good_resolvent_root now prefers s_sq > 0, this
        // case should rarely be reached. Fall back to RT for simplicity.
        return Ok(None);
    }

    let s = match algebraic_sqrt_of_rational(&s_sq) {
        Some(v) => v,
        None => return Ok(None),
    };

    // 6. Build two quadratics in t: t^2 +/- s*t + (m -/+ q/(2s))
    let two_s = mul(&two, &s)?;
    let q_over_2s = div(&q, &two_s)?;

    let k1 = sub(&m, &q_over_2s)?;
    let k2 = add(&m, &q_over_2s)?;

    let neg_s = mul(&s, &Value::Int(-1))?;

    // quad1: t^2 + s*t + k1, coefficients: [k1, s, 1]
    // quad2: t^2 - s*t + k2, coefficients: [k2, -s, 1]
    let mut quad1_t = vec![k1, s.clone(), Value::Int(1)];
    let mut quad2_t = vec![k2, neg_s, Value::Int(1)];
    poly_trim(&mut quad1_t);
    poly_trim(&mut quad2_t);

    // 7. Undepress: substitute t = x + shift
    // For t^2 + A*t + B -> x^2 + (2*shift + A)*x + (shift^2 + A*shift + B)
    let undepress = |quad: &[Value]| -> WqResult<Vec<Value>> {
        let a_t = quad.get(1).cloned().unwrap_or(Value::Int(0));
        let b_t = quad.first().cloned().unwrap_or(Value::Int(0));
        let two_shift = mul(&two, &shift)?;
        let new_a = add(&two_shift, &a_t)?; // 2*shift + A
        let shift_sq = mul(&shift, &shift)?;
        let a_shift = mul(&a_t, &shift)?;
        let new_b = add(&add(&shift_sq, &a_shift)?, &b_t)?; // shift^2 + A*shift + B
        let mut result = vec![new_b, new_a, Value::Int(1)];
        poly_trim(&mut result);
        Ok(result)
    };

    let quad1 = undepress(&quad1_t)?;
    let quad2 = undepress(&quad2_t)?;

    Ok(Some(vec![quad1, quad2]))
}

/// Find a real root m of the resolvent cubic such that 2m - p > 0.
/// Tries rational roots first, then falls back to algebraic root isolation.
/// Returns None if no root satisfies the strictly-positive condition.
fn find_good_resolvent_root(cubic_coeffs: &[Value], p: &Value) -> WqResult<Option<Value>> {
    let two = Value::Int(2);

    // Helper: check if a candidate m gives 2m - p > 0
    let is_good = |m: &Value| -> WqResult<bool> {
        let two_m = numeric_mul(&two, m)?;
        let s_sq = numeric_sub(&two_m, p)?;
        Ok(!numeric_is_negative(&s_sq) && !numeric_is_zero(&s_sq))
    };

    // Try rational roots
    if let Some(linear) = find_rational_root(cubic_coeffs) {
        let root = numeric_mul(&linear[0], &Value::Int(-1))?;
        if is_good(&root)? {
            return Ok(Some(root));
        }
        // Try other rational roots by dividing and continuing
        if let Ok((remaining, _)) = poly_divide(cubic_coeffs, &linear)
            && poly_degree(&remaining) == 2
        {
            let c0 = remaining.first().cloned().unwrap_or(Value::Int(0));
            let c1 = remaining.get(1).cloned().unwrap_or(Value::Int(0));
            let disc = numeric_sub(&numeric_mul(&c1, &c1)?, &numeric_mul(&Value::Int(4), &c0)?)?;
            if !numeric_is_negative(&disc)
                && let Some(sqrt_disc) = algebraic_sqrt_of_rational(&disc)
            {
                let neg_c1 = numeric_mul(&c1, &Value::Int(-1))?;
                let two_v = Value::Int(2);
                let r1 = eval_exact_numeric_div(&numeric_add(&neg_c1, &sqrt_disc)?, &two_v)?;
                if is_good(&r1)? {
                    return Ok(Some(r1));
                }
                let r2 = eval_exact_numeric_div(&numeric_sub(&neg_c1, &sqrt_disc)?, &two_v)?;
                if is_good(&r2)? {
                    return Ok(Some(r2));
                }
            }
        }
    }

    // Fallback: try algebraic root isolation
    if let Some(alg_root) = find_real_algebraic_root(cubic_coeffs)
        && is_good(&alg_root)?
    {
        return Ok(Some(alg_root));
    }

    Ok(None)
}

/// Try to factor a cubic or quartic using exact radical formulas.
///
/// Uses exact Value arithmetic (Cardano/Ferrari). Reducible cubics/quartics
/// with rational roots are also handled by `find_rational_root`.
/// Irreducible cubics without rational resolvent roots fall through
/// to Rothstein-Trager.
fn factor_by_radical_formula(poly: &[Value]) -> WqResult<Option<Vec<Vec<Value>>>> {
    let deg = poly_degree(poly);
    match deg {
        4 => solve_quartic_exact(poly),
        3 => solve_cubic_by_rational_root(poly),
        _ => Ok(None),
    }
}

/// Find a rational root value of a polynomial using the Rational Root Theorem.
pub(crate) fn find_rational_root_value(poly: &[Value]) -> Option<Value> {
    // Special case: if constant term is 0, x=0 is a root
    if poly.first().is_some_and(numeric_is_zero) {
        return Some(Value::Int(0));
    }

    // Get constant term (c0) and leading coefficient (cn)
    let c0 = poly.first()?.clone();
    let lead_idx = poly_degree(poly);
    let cn = poly[lead_idx].clone();

    // Get integer divisors of c0 and cn
    let c0_divs = integer_divisors(&c0);
    let cn_divs = integer_divisors(&cn);

    for p in &c0_divs {
        for q in &cn_divs {
            for sign in [1, -1].iter() {
                let sign_val = BigInt::from(*sign);
                let numer = p * &sign_val;
                let denom = q.clone();
                if denom == BigInt::zero() {
                    continue;
                }

                // Build root value
                let root_val = if denom == BigInt::one() {
                    Value::from_bigint(numer)
                } else {
                    Value::from_fraction_parts(numer, denom)
                };

                // Evaluate polynomial at root
                let result = poly_evaluate(poly, &root_val).ok()?;
                if numeric_is_zero(&result) {
                    return Some(root_val);
                }
            }
        }
    }

    None
}

/// Find a rational root of a polynomial using the Rational Root Theorem.
/// Returns Some([c, 1] = x - root) if found.
fn find_rational_root(poly: &[Value]) -> Option<Vec<Value>> {
    let root = find_rational_root_value(poly)?;
    let c = numeric_mul(&root, &Value::Int(-1)).ok()?;
    Some(vec![c, Value::Int(1)])
}

/// Get all positive integer divisors of a Value (if it represents an integer).
fn integer_divisors(value: &Value) -> Vec<BigInt> {
    use num_traits::ToPrimitive;

    let n: Option<BigInt> = match value {
        Value::Int(i) => Some(BigInt::from(i.unsigned_abs())),
        Value::BigInt(b) => b.to_i64().map(|i| BigInt::from(i.unsigned_abs())),
        Value::Fraction(f) => {
            if f.denom().is_one() {
                f.numer().to_i64().map(|i| BigInt::from(i.unsigned_abs()))
            } else {
                None
            }
        }
        _ => None,
    };

    let n = match n {
        Some(n) => n,
        None => return vec![BigInt::one()],
    };

    let mut divs = Vec::new();
    let n_u64 = n.to_u64();
    if let Some(limit) = n_u64 {
        let limit = integer_sqrt_u64(limit);
        for i in 1..=limit {
            let i_big = BigInt::from(i);
            if &n % &i_big == BigInt::zero() {
                divs.push(i_big.clone());
                let other = &n / &i_big;
                if other != i_big {
                    divs.push(other);
                }
            }
        }
    } else {
        // For very large numbers, try small divisors
        for i in 1..=100u64 {
            let i_big = BigInt::from(i);
            if &n % &i_big == BigInt::zero() {
                divs.push(i_big.clone());
                let other = &n / &i_big;
                if other != i_big {
                    divs.push(other);
                }
            }
        }
    }

    if divs.is_empty() {
        divs.push(BigInt::one());
    }
    divs
}

fn integer_sqrt_u64(n: u64) -> u64 {
    let mut lo = 0u64;
    let mut hi = 1u64 << 32;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if mid <= n / mid {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}

// ---------------------------------------------------------------------------
// Linear denominator factor: (x - r)^m
// ---------------------------------------------------------------------------

/// Compute the partial fraction contributions from a linear factor (x - r)^m.
fn integrate_linear_factor_all(
    numer: &[Value],
    denom: &[Value],
    factor: &[Value],
    mult: usize,
    var: &str,
) -> WqResult<Value> {
    // factor = [c, 1]  ->  x + c = 0  =>  r = -c
    let r = numeric_mul(&factor[0], &Value::Int(-1))?;

    // Compute D_without = D / (x - r)^m as a polynomial
    let factor_pow = poly_pow(factor, mult)?;
    let (d_without, _) = poly_divide(denom, &factor_pow)?;

    // Compute A_1, ..., A_m using the cover-up / Taylor method.
    // A_{m-k} = g^{(k)}(r) / k!  for k = 0..m-1, where g(x) = N(x) / D_without(x)
    let mut coeffs = Vec::with_capacity(mult);
    for k in 0..mult {
        let deriv_order = k;
        let factorial = factorial_bigint(deriv_order);

        // Evaluate the k-th derivative of g(x) = N(x) / D_without(x) at x = r
        let deriv_val = eval_rational_derivative(numer, &d_without, deriv_order, &r)?;

        // A_{m-k} = g^{(k)}(r) / k!
        let a = eval_exact_numeric_div(&deriv_val, &Value::from_bigint(factorial))?;
        coeffs.push(a);
    }
    // coeffs[0] = A_m, coeffs[1] = A_{m-1}, ..., coeffs[m-1] = A_1

    let mut terms = Vec::new();
    for (idx, a) in coeffs.iter().enumerate() {
        let power = mult - idx; // 1, 2, ..., m
        let term = integrate_linear_power_term(a, power, &r, var)?;
        terms.push(term);
    }

    if terms.len() == 1 {
        Ok(terms.into_iter().next().expect("single term"))
    } else {
        simplify_cas_value(&cas_add(terms)?)
    }
}

/// Integrate A / (x - r)^power.
fn integrate_linear_power_term(a: &Value, power: usize, r: &Value, var: &str) -> WqResult<Value> {
    let x_minus_r = cas_sub(Value::from_cas_var(var), r.clone())?;
    if power == 1 {
        // A * ln|x - r|
        let log = Value::from_cas_function(
            CasFunction::Ln,
            vec![Value::from_cas_function(CasFunction::Abs, vec![x_minus_r])],
        );
        if numeric_is_one(a) {
            return Ok(log);
        }
        return cas_mul(vec![a.clone(), log]);
    }

    // A * (x - r)^(1-power) / (1-power)
    let numerator = cas_pow(
        x_minus_r,
        Value::from_bigint(BigInt::from(1 - power as i64)),
    )?;
    let denominator = Value::Int(1 - (power as i64));
    let term = cas_div(numerator, denominator)?;
    if numeric_is_one(a) {
        Ok(term)
    } else {
        cas_mul(vec![a.clone(), term])
    }
}

/// Compute the k-th derivative of N(x)/D(x) evaluated at x = r.
fn eval_rational_derivative(
    numer: &[Value],
    denom: &[Value],
    k: usize,
    x: &Value,
) -> WqResult<Value> {
    if k == 0 {
        let num_val = poly_evaluate(numer, x)?;
        let den_val = poly_evaluate(denom, x)?;
        return eval_exact_numeric_div(&num_val, &den_val);
    }

    let (deriv_num, _) = rational_deriv_numer_denom(numer, denom, k);

    let denom_pow = poly_pow(denom, k + 1)?;
    let num_val = poly_evaluate(&deriv_num, x)?;
    let den_val = poly_evaluate(&denom_pow, x)?;
    eval_exact_numeric_div(&num_val, &den_val)
}

/// Compute the k-th derivative numerator and denominator power for g(x) = N/D.
/// Returns (numerator_poly, denominator_power) such that g^{(k)}(x) = P /
/// D^{k+1}.
fn rational_deriv_numer_denom(numer: &[Value], denom: &[Value], k: usize) -> (Vec<Value>, usize) {
    let mut p = numer.to_vec();
    let mut q = denom.to_vec();

    for _order in 0..k {
        // p_new = p' * q - p * q'
        let p_deriv = poly_derivative(&p);
        let q_deriv = poly_derivative(&q);

        let term1 = poly_mul(&p_deriv, &q).unwrap_or_else(|_| vec![Value::Int(0)]);
        let term2 = poly_mul(&p, &q_deriv).unwrap_or_else(|_| vec![Value::Int(0)]);

        p = poly_sub(&term1, &term2).unwrap_or_else(|_| vec![Value::Int(0)]);
        q = poly_mul(&q, denom).unwrap_or_else(|_| vec![Value::Int(1)]);
    }

    (p, k + 1)
}

// ---------------------------------------------------------------------------
// Quadratic denominator factor: (x^2 + bx + c)^m
// ---------------------------------------------------------------------------

/// Compute partial fraction contributions from a quadratic factor (x^2 + bx +
/// c)^m.
fn integrate_quadratic_factor_all(
    numer: &[Value],
    denom: &[Value],
    factor: &[Value],
    mult: usize,
    var: &str,
) -> WqResult<Value> {
    // factor = [c, b, 1]  ->  x^2 + bx + c
    let (b, c) = match factor.len() {
        3 => (factor[1].clone(), factor[0].clone()),
        _ => return Err(cas_err("expected quadratic factor [c, b, 1]")),
    };

    if mult == 1 {
        return integrate_simple_quadratic(numer, denom, factor, &b, &c, var);
    }

    // For mult > 1, use Hermite reduction.
    integrate_repeated_quadratic(numer, denom, factor, &b, &c, mult, var)
}

/// Integrate (A*x + B) / (x^2 + bx + c)
fn integrate_simple_quadratic(
    numer: &[Value],
    denom: &[Value],
    factor: &[Value],
    b: &Value,
    c: &Value,
    var: &str,
) -> WqResult<Value> {
    // Find A, B such that N(x)/D_without == (A*x + B) (mod x^2+bx+c)
    let factor_pow = poly_pow(factor, 1)?;
    let (d_without, _) = poly_divide(denom, &factor_pow)?;

    // We need numerator == (A*x + B) * D_without (mod factor)
    // A*x + B are the unknown coefficients of the partial fraction for this factor.
    //
    // Solve N == (A*x + B) * D_without (mod factor) where deg(N) < deg(D)
    // Since deg(factor) = 2, we can solve by polynomial remainder.

    // Compute N mod factor and D_without mod factor
    let n_mod = poly_remainder(numer, factor)?;
    let d_mod = poly_remainder(&d_without, factor)?;

    // n_mod = a_n*x + b_n  (coefficient vector [b_n, a_n])
    // d_mod = a_d*x + b_d  (coefficient vector [b_d, a_d])
    // Solve: (A*x + B) * (a_d*x + b_d) == a_n*x + b_n  (mod x^2+bx+c)

    let (a, b_val) = solve_linear_coeffs_mod_quadratic(&n_mod, &d_mod, factor, b, c)?;

    integrate_quadratic_log_arctan_term(&a, &b_val, b, c, var)
}

/// Solve for (A, B) such that (A*x + B) * D_mod == N_mod (mod x^2+bx+c).
fn solve_linear_coeffs_mod_quadratic(
    n_mod: &[Value],
    d_mod: &[Value],
    _factor: &[Value],
    b: &Value,
    c: &Value,
) -> WqResult<(Value, Value)> {
    // n_mod = [b_n, a_n], d_mod = [b_d, a_d]
    let a_n = n_mod.get(1).cloned().unwrap_or(Value::Int(0));
    let b_n = n_mod.first().cloned().unwrap_or(Value::Int(0));
    let a_d = d_mod.get(1).cloned().unwrap_or(Value::Int(0));
    let b_d = d_mod.first().cloned().unwrap_or(Value::Int(0));

    // (A*x + B) * (a_d*x + b_d) = A*a_d*x^2 + (A*b_d + B*a_d)*x + B*b_d
    // Reduce mod x^2+bx+c: x^2 == -b*x - c
    // = A*a_d*(-b*x - c) + (A*b_d + B*a_d)*x + B*b_d
    // = (-A*a_d*b + A*b_d + B*a_d)*x + (-A*a_d*c + B*b_d)
    // == a_n*x + b_n

    // So:
    // x coeff: A*(-a_d*b + b_d) + B*a_d = a_n
    // const:   A*(-a_d*c) + B*b_d = b_n

    // Solve 2x2 linear system:
    // | -a_d*b + b_d    a_d | | A |   | a_n |
    // | -a_d*c          b_d | | B | = | b_n |

    let m11 = numeric_mul(
        &numeric_mul(&a_d, &numeric_mul(b, &Value::Int(-1))?)?,
        &Value::Int(1),
    )?;
    let m11 = numeric_add(&m11, &b_d)?; // -a_d*b + b_d
    let m12 = a_d.clone();
    let m21 = numeric_mul(&a_d, &numeric_mul(c, &Value::Int(-1))?)?; // -a_d*c
    let m22 = b_d.clone();

    // Determinant: m11*m22 - m12*m21
    let det = numeric_sub(&numeric_mul(&m11, &m22)?, &numeric_mul(&m12, &m21)?)?;

    if numeric_is_zero(&det) {
        return Err(cas_err("singular system in quadratic partial fraction"));
    }

    // A = (a_n*m22 - b_n*m12) / det
    let a = eval_exact_numeric_div(
        &numeric_sub(&numeric_mul(&a_n, &m22)?, &numeric_mul(&b_n, &m12)?)?,
        &det,
    )?;

    // B = (m11*b_n - m21*a_n) / det
    let b_val = eval_exact_numeric_div(
        &numeric_sub(&numeric_mul(&m11, &b_n)?, &numeric_mul(&m21, &a_n)?)?,
        &det,
    )?;

    Ok((a, b_val))
}

/// Compute poly mod factor (returns coefficients representing remainder).
fn poly_remainder(poly: &[Value], factor: &[Value]) -> WqResult<Vec<Value>> {
    let (_, rem) = poly_divide(poly, factor)?;
    Ok(rem)
}

/// Integrate (A*x + B) / (x^2 + bx + c).
fn integrate_quadratic_log_arctan_term(
    a: &Value,
    b_val: &Value,
    bb: &Value,
    c: &Value,
    var: &str,
) -> WqResult<Value> {
    // Split: A*x + B = (A/2)*(2x + b) + (B - A*b/2)
    let half = Value::from_fraction_parts(BigInt::one(), BigInt::from(2));
    let a_half = numeric_mul(a, &half)?;

    // Part 1: (A/2) * int (2x+b)/(x^2+bx+c) dx = (A/2) * ln|x^2+bx+c|
    let quad_str = poly_to_expr(&[c.clone(), bb.clone(), Value::Int(1)], var)?;
    let log_part = if !numeric_is_zero(&a_half) {
        let log_inside = simplify_cas_value(&quad_str)?;
        let log = Value::from_cas_function(
            CasFunction::Ln,
            vec![Value::from_cas_function(CasFunction::Abs, vec![log_inside])],
        );
        cas_mul(vec![a_half.clone(), log])?
    } else {
        Value::Int(0)
    };

    // Part 2: (B - A*b/2) * int 1/(x^2+bx+c) dx
    let b_prime = numeric_sub(b_val, &numeric_mul(a, &numeric_mul(bb, &half)?)?)?;

    if numeric_is_zero(&b_prime) {
        return Ok(log_part);
    }

    let arctan_or_log = integrate_one_over_quadratic(&b_prime, bb, c, var)?;

    if log_part == Value::Int(0) {
        Ok(arctan_or_log)
    } else {
        simplify_cas_value(&cas_add(vec![log_part, arctan_or_log])?)
    }
}

/// Integrate C / (x^2 + bx + c)
/// If `value` is a constant Algebraic (all coeffs[1..] zero), unwrap it
/// to the underlying constant value. Otherwise return the original value.
fn unwrap_constant_algebraic(value: Value) -> Value {
    if let Value::Algebraic(a) = &value
        && !a.coeffs.is_empty()
        && a.coeffs[1..].iter().all(numeric_is_zero)
    {
        return a.coeffs[0].clone();
    }
    value
}

fn integrate_one_over_quadratic(c_val: &Value, b: &Value, c: &Value, var: &str) -> WqResult<Value> {
    // Complete square: x^2 + bx + c = (x + b/2)^2 + (c - b^2/4)
    let half = Value::from_fraction_parts(BigInt::one(), BigInt::from(2));
    let b_half = numeric_mul(b, &half)?; // b/2

    let b_sq_div_4 = numeric_mul(
        &numeric_mul(b, b)?,
        &Value::from_fraction_parts(BigInt::one(), BigInt::from(4)),
    )?;
    let k_sq = numeric_sub(c, &b_sq_div_4)?; // c - b^2/4

    // If k_sq is a constant Algebraic (just a rational embedded in an extension),
    // unwrap to the plain value so sqrt_of_value can handle it.
    let k_sq = unwrap_constant_algebraic(k_sq);

    // x + b/2
    let x_plus_shift = cas_add(vec![Value::from_cas_var(var), b_half.clone()])?;

    if let Some(a_sq) = negated_value(&k_sq) {
        // k_sq = -a^2, so denominator is (x+b/2)^2 - a^2 = (x+b/2 - a)(x+b/2 + a)
        let a_sq = unwrap_constant_algebraic(a_sq);
        let a = sqrt_of_quadratic_constant(&a_sq).ok_or_else(|| {
            cas_err(format!(
                "cannot compute sqrt of {}",
                a_sq.format_cas().unwrap_or_default()
            ))
        })?;
        let a = simplify_cas_value(&a)?;

        let two_a = cas_mul(vec![Value::Int(2), a.clone()])?;
        let inner = cas_div(
            cas_sub(x_plus_shift.clone(), a.clone())?,
            cas_add(vec![x_plus_shift, a])?,
        )?;
        let log = Value::from_cas_function(
            CasFunction::Ln,
            vec![Value::from_cas_function(CasFunction::Abs, vec![inner])],
        );
        let result = cas_mul(vec![cas_div(c_val.clone(), two_a)?, log])?;
        simplify_cas_value(&result)
    } else {
        // k_sq = a^2, denominator is (x+b/2)^2 + a^2
        let a = sqrt_of_quadratic_constant(&k_sq).ok_or_else(|| {
            cas_err(format!(
                "cannot compute sqrt of {}",
                k_sq.format_cas().unwrap_or_default()
            ))
        })?;
        let a = simplify_cas_value(&a)?;

        let arctan_arg = simplify_cas_value(&cas_div(x_plus_shift, a.clone())?)?;
        let arctan = Value::from_cas_function(CasFunction::ArcTan, vec![arctan_arg]);
        let result = cas_mul(vec![cas_div(c_val.clone(), a)?, arctan])?;
        simplify_cas_value(&result)
    }
}

fn negated_value(value: &Value) -> Option<Value> {
    if let Some((numer, denom)) = value.rational_parts()
        && numer < BigInt::zero()
    {
        return Some(Value::from_fraction_parts(-numer, denom));
    }
    if let Value::Float(f) = value
        && **f < 0.0
    {
        return Some(Value::float(-**f));
    }
    if let Some((CasOp::Multiply, args)) = value.cas_op_parts() {
        let mut positive = Vec::with_capacity(args.len());
        let mut stripped_sign = false;
        for arg in args {
            if !stripped_sign && let Some(positive_arg) = negated_value(arg) {
                positive.push(positive_arg);
                stripped_sign = true;
            } else {
                positive.push(arg.clone());
            }
        }
        if !stripped_sign {
            return None;
        }
        return match positive.len() {
            0 => Some(Value::Int(1)),
            1 => positive.pop(),
            _ => cas_mul(positive).ok(),
        };
    }
    None
}

fn sqrt_of_quadratic_constant(value: &Value) -> Option<Value> {
    if let Some(root) = algebraic_sqrt_of_rational(value) {
        return Some(root);
    }
    if let Some(root) = sqrt_of_value(value) {
        return Some(root);
    }
    if let Some((CasOp::Power, [base, exp])) = value.cas_op_parts()
        && exp.exact_int().is_some_and(|n| n == BigInt::from(2))
    {
        return Some(base.clone());
    }
    if let Some((CasOp::Multiply, args)) = value.cas_op_parts() {
        let mut roots = Vec::with_capacity(args.len());
        for arg in args {
            roots.push(sqrt_of_quadratic_constant(arg)?);
        }
        return cas_mul(roots).ok();
    }
    None
}

/// Create a Value representing the positive square root of a positive rational.
///
/// If the rational is a perfect square, returns an exact Int or Fraction.
/// Otherwise, returns `Value::Algebraic` with minimal polynomial `x^2 - (n*d)`
/// representing `sqrt(n/d)` in the field `Q(sqrt(n*d))`.
fn algebraic_sqrt_of_rational(value: &Value) -> Option<Value> {
    let (n, d) = rational_parts_value(value)?;
    if n.is_zero() || n < BigInt::zero() {
        return None;
    }
    let c = &n * &d;
    // Check if c is a perfect square
    let c_sqrt = if let Some(cf) = c.to_f64() {
        let sf = cf.sqrt();
        let r = sf.round();
        if (sf - r).abs() < 1e-12 {
            let s = BigInt::from(r as i64);
            if &s * &s == c { Some(s) } else { None }
        } else {
            None
        }
    } else {
        None
    };
    if let Some(s) = c_sqrt {
        // sqrt(n/d) = sqrt(c) / d = s / d
        if s.is_zero() {
            return Some(Value::Int(0));
        }
        if d.is_one() {
            return Some(if s == BigInt::one() {
                Value::Int(1)
            } else {
                Value::from_bigint(s)
            });
        }
        return Some(Value::from_fraction_parts(s, d));
    }
    // Not a perfect square; create Algebraic number for sqrt(n/d) = sqrt(c) / d
    let poly = vec![-c.clone(), BigInt::zero(), BigInt::one()];
    let poly_arc: Arc<[BigInt]> = Arc::from(poly.clone());
    let interval = isolate_root_interval(&poly_arc)?;
    let coeff_alpha = if d.is_one() {
        Value::Int(1)
    } else {
        Value::from_fraction_parts(BigInt::one(), d)
    };
    let field = AlgebraicField::new_real_root(poly, interval).ok()?;
    AlgebraicData::value(field, vec![Value::Int(0), coeff_alpha]).ok()
}

fn sqrt_of_value(value: &Value) -> Option<Value> {
    use num_traits::ToPrimitive;

    match value {
        Value::Int(n) => {
            let f = (*n as f64).sqrt();
            if (f - f.round()).abs() < 1e-12 {
                Some(Value::Int(f.round() as i64))
            } else if f.is_finite() {
                Some(Value::from_cas_function(
                    CasFunction::Sqrt,
                    vec![value.clone()],
                ))
            } else {
                None
            }
        }
        Value::Float(f) => {
            let sqrt = f.sqrt();
            if sqrt.is_finite() {
                Some(Value::float(sqrt))
            } else {
                None
            }
        }
        Value::BigInt(n) => {
            let f = n.to_f64()?.sqrt();
            if (f - f.round()).abs() < 1e-12 {
                Some(Value::Int(f.round() as i64))
            } else if f.is_finite() {
                Some(Value::from_cas_function(
                    CasFunction::Sqrt,
                    vec![value.clone()],
                ))
            } else {
                None
            }
        }
        Value::Fraction(fr) => {
            let numer = fr.numer().to_f64()?;
            let denom = fr.denom().to_f64()?;
            let f = (numer / denom).sqrt();
            if (f - f.round()).abs() < 1e-12 {
                Some(Value::Int(f.round() as i64))
            } else if f.is_finite() {
                Some(Value::from_cas_function(
                    CasFunction::Sqrt,
                    vec![value.clone()],
                ))
            } else {
                None
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Repeated quadratic factor: Hermite reduction
// ---------------------------------------------------------------------------

/// Integrate rational function with repeated quadratic factor (x^2+bx+c)^m, m >
/// 1.
fn integrate_repeated_quadratic(
    numer: &[Value],
    denom: &[Value],
    factor: &[Value],
    _b: &Value,
    _c: &Value,
    mult: usize,
    var: &str,
) -> WqResult<Value> {
    // Hermite reduction: find P(x) with deg(P) < deg(factor) = 2 such that
    //   int N/F^m = P/F^(m-1) + int Q/F^(m-1)
    // where Q is a new numerator of degree < deg(F) * (m-1).

    let (rational_part_val, new_numer, reduced_pow) =
        hermite_reduce_one_step(numer, denom, factor, mult, var)?;

    // Now integrate new_numer / F^(m-1)
    let mut reduced_factors = vec![(factor.to_vec(), reduced_pow)];

    // Also include any remaining factors from the original denominator
    let factor_pow = poly_pow(factor, mult)?;
    let (rest_denom, _) = poly_divide(denom, &factor_pow)?;
    if poly_degree(&rest_denom) > 0 {
        let rest_factors = square_free_factor(&rest_denom, var)?;
        reduced_factors.extend(rest_factors);
    }

    // Build the reduced denominator: F^(m-1) * rest_denom
    let f_pow_reduced = poly_pow(factor, reduced_pow)?;
    let reduced_denom = poly_mul(&f_pow_reduced, &rest_denom)?;

    let rest_result = integrate_proper_rational(&new_numer, &reduced_denom, &reduced_factors, var)?;

    if rational_part_val == Value::Int(0) {
        Ok(rest_result)
    } else {
        simplify_cas_value(&cas_add(vec![rational_part_val, rest_result])?)
    }
}

/// Hermite reduction step: reduces int N / (F^m * rest) to P / F^(m-1) + int Q
/// / (F^(m-1) * rest)
///
/// Returns (rational_part_P_F^{m-1}, new_numer_Q, new_mult)
fn hermite_reduce_one_step(
    numer: &[Value],
    denom: &[Value],
    factor: &[Value],
    mult: usize,
    var: &str,
) -> WqResult<(Value, Vec<Value>, usize)> {
    if mult <= 1 {
        return Ok((Value::Int(0), numer.to_vec(), mult));
    }

    let factor_pow = poly_pow(factor, mult)?;
    let (rest, _) = poly_divide(denom, &factor_pow)?;

    // For deg(F) = 2, F = [c, b, 1], F' = [b, 2, 0] -> [b, 2].
    //
    // With D = F^m * R, choose S with deg(S) < deg(F) so that the derivative
    // of S / F^(m-1) cancels one F from the original denominator:
    //
    //   N - R * (S' * F - (m - 1) * S * F') == 0 mod F
    //
    let f_deriv = poly_derivative(factor);
    let m_minus_1 = Value::from_bigint(BigInt::from(mult - 1));
    let n_mod = poly_remainder(numer, factor)?;

    let hermite_remainder = |p_coeffs: Vec<Value>| -> WqResult<Vec<Value>> {
        let p_deriv = poly_derivative(&p_coeffs);
        let term1 = poly_mul(&p_deriv, factor)?;
        let term2_inner = poly_mul(&p_coeffs, &f_deriv)?;
        let term2 = poly_const_mul(&term2_inner, &m_minus_1)?;
        let deriv_numer = poly_sub(&term1, &term2)?;
        let with_rest = poly_mul(&rest, &deriv_numer)?;
        poly_remainder(&with_rest, factor)
    };

    let r_p0 = hermite_remainder(vec![Value::Int(1), Value::Int(0)])?;
    let r_p1 = hermite_remainder(vec![Value::Int(0), Value::Int(1)])?;

    // Solve [r_p0  r_p1] [p0; p1] = n_mod (2x2 via Cramer).
    let (r00, r10) = (
        r_p0.first().cloned().unwrap_or(Value::Int(0)),
        r_p0.get(1).cloned().unwrap_or(Value::Int(0)),
    );
    let (r01, r11) = (
        r_p1.first().cloned().unwrap_or(Value::Int(0)),
        r_p1.get(1).cloned().unwrap_or(Value::Int(0)),
    );
    let (n0, n1) = (
        n_mod.first().cloned().unwrap_or(Value::Int(0)),
        n_mod.get(1).cloned().unwrap_or(Value::Int(0)),
    );

    let det = numeric_sub(&numeric_mul(&r00, &r11)?, &numeric_mul(&r01, &r10)?)?;
    if numeric_is_zero(&det) {
        return Err(cas_err("Hermite reduction: singular system for S"));
    }
    let p0 = eval_exact_numeric_div(
        &numeric_sub(&numeric_mul(&n0, &r11)?, &numeric_mul(&n1, &r01)?)?,
        &det,
    )?;
    let p1 = eval_exact_numeric_div(
        &numeric_sub(&numeric_mul(&r00, &n1)?, &numeric_mul(&r10, &n0)?)?,
        &det,
    )?;
    let mut p_coeffs = vec![p0, p1];
    poly_trim(&mut p_coeffs);

    // Compute d/dx(S / F^(m-1)):
    // d/dx [num / F^(m-1)] = (num' * F^(m-1) - num * (m-1) * F^(m-2) * F') /
    // F^(2m-2)                       = (num' * F - num * (m-1) * F') / F^m

    let p_deriv = poly_derivative(&p_coeffs);
    let term1 = poly_mul(&p_deriv, factor)?; // num' * F

    let term2_inner = poly_mul(&p_coeffs, &f_deriv)?; // num * F'
    let term2 = poly_const_mul(&term2_inner, &m_minus_1)?; // (m-1) * num * F'

    let deriv_numer = poly_sub(&term1, &term2)?; // num' * F - (m-1) * num * F'

    // The derivative gives: deriv_numer / F^m
    // We want N/(F^m*R) - deriv_numer/F^m, so use common denominator F^m*R:
    // (N - R*deriv_numer)/(F^m*R), which should simplify to
    // Q_new/(F^(m-1)*R).
    //
    // new_numer = (N - R*deriv_numer) / F, new_denom = F^(m-1)*R.

    let deriv_numer_with_rest = poly_mul(&rest, &deriv_numer)?;
    let diff = poly_sub(numer, &deriv_numer_with_rest)?; // N - R*deriv_numer
    let (q_new, rem) = poly_divide(&diff, factor)?; // (N - R*deriv_numer) / F

    if !poly_is_zero(&rem) {
        return Err(cas_err(
            "Hermite reduction failed to cancel a repeated quadratic factor",
        ));
    }

    // Rational part: S / F^(m-1)
    let f_pow_m1 = poly_pow(factor, mult - 1)?;
    let rational_part = if poly_is_zero(&p_coeffs) {
        Value::Int(0)
    } else {
        let num_expr = poly_to_expr(&p_coeffs, var)?;
        let denom_expr = poly_to_expr(&f_pow_m1, var)?;
        cas_div(num_expr, denom_expr)?
    };

    Ok((rational_part, q_new, mult - 1))
}

fn factorial_bigint(n: usize) -> BigInt {
    let mut result = BigInt::one();
    for i in 2..=n {
        result *= BigInt::from(i);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cas::poly_resultant;

    fn op(op: CasOp, args: Vec<Value>) -> Value {
        Value::from_cas_op(op, args)
    }

    #[test]
    fn test_poly_resultant_quadratic() {
        // resultant(x^2-3x+2, x-1) = 0 (common root x=1)
        let a = vec![Value::Int(2), Value::Int(-3), Value::Int(1)]; // x^2-3x+2
        let b = vec![Value::Int(-1), Value::Int(1)]; // x-1
        let r = poly_resultant(&a, &b).unwrap();
        assert_eq!(r, Value::Int(0));

        // resultant(x^2-3x+2, x-3) = (3-1)(3-2) = 2
        let b = vec![Value::Int(-3), Value::Int(1)]; // x-3
        let r = poly_resultant(&a, &b).unwrap();
        assert_eq!(r, Value::Int(2));
    }

    #[test]
    fn test_poly_interpolate_linear() {
        // P(0)=1, P(1)=3 -> P(z)=2z+1 = [1, 2]
        let points = vec![
            (Value::Int(0), Value::Int(1)),
            (Value::Int(1), Value::Int(3)),
        ];
        let r = poly_interpolate(&points).unwrap();
        assert_eq!(poly_degree(&r), 1);
        assert!(numeric_is_zero(
            &numeric_sub(&poly_evaluate(&r, &Value::Int(0)).unwrap(), &Value::Int(1)).unwrap()
        ));
        assert!(numeric_is_zero(
            &numeric_sub(&poly_evaluate(&r, &Value::Int(1)).unwrap(), &Value::Int(3)).unwrap()
        ));
    }

    #[test]
    fn test_poly_interpolate_quadratic() {
        // P(0)=1, P(1)=2, P(2)=5 -> P(z)=z^2+1
        let points = vec![
            (Value::Int(0), Value::Int(1)),
            (Value::Int(1), Value::Int(2)),
            (Value::Int(2), Value::Int(5)),
        ];
        let r = poly_interpolate(&points).unwrap();
        assert_eq!(poly_degree(&r), 2);
        assert!(numeric_is_zero(
            &numeric_sub(&poly_evaluate(&r, &Value::Int(0)).unwrap(), &Value::Int(1)).unwrap()
        ));
        assert!(numeric_is_zero(
            &numeric_sub(&poly_evaluate(&r, &Value::Int(1)).unwrap(), &Value::Int(2)).unwrap()
        ));
        assert!(numeric_is_zero(
            &numeric_sub(&poly_evaluate(&r, &Value::Int(2)).unwrap(), &Value::Int(5)).unwrap()
        ));
    }

    #[test]
    fn test_poly_resultant_cubic() {
        // resultant(x^2+1, x^2-1) = (1^2+1)((-1)^2+1) = 2*2 = 4
        let a = vec![Value::Int(1), Value::Int(0), Value::Int(1)]; // x^2+1
        let b = vec![Value::Int(-1), Value::Int(0), Value::Int(1)]; // x^2-1
        let r = poly_resultant(&a, &b).unwrap();
        assert_eq!(r, Value::Int(4));
    }

    #[test]
    fn test_extract_polynomial() {
        let expr = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let (num, denom) = extract_rational(&expr, "x").unwrap().unwrap();
        assert_eq!(denom, vec![Value::Int(1)]);
        // num should be [1, 0, 1] (1 + x^2)
        assert_eq!(num.len(), 3);
        assert_eq!(num[0], Value::Int(1));
        assert_eq!(num[2], Value::Int(1));
    }

    #[test]
    fn test_extract_simple_fraction() {
        let expr = op(
            CasOp::Power,
            vec![
                op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
                Value::Int(-1),
            ],
        );
        let (num, denom) = extract_rational(&expr, "x").unwrap().unwrap();
        assert_eq!(num, vec![Value::Int(1)]);
        // denom should be [1, 1] (1 + x)
        assert_eq!(denom.len(), 2);
        assert_eq!(denom[0], Value::Int(1));
        assert_eq!(denom[1], Value::Int(1));
    }

    #[test]
    fn test_square_free_perfect_square() {
        // (x+1)^2 = x^2 + 2x + 1 = [1, 2, 1]
        let poly = vec![Value::Int(1), Value::Int(2), Value::Int(1)];
        let factors = square_free_factor(&poly, "x").unwrap();
        // Should get (x+1) with multiplicity 2
        assert_eq!(factors.len(), 1);
        assert_eq!(factors[0].1, 2);
        // factor should be x+1 = [1, 1]
        assert_eq!(factors[0].0.len(), 2);
        assert_eq!(factors[0].0[0], Value::Int(1));
        assert_eq!(factors[0].0[1], Value::Int(1));
    }

    #[test]
    fn test_square_free_cubic() {
        // (x+1)^3 = x^3 + 3x^2 + 3x + 1 = [1, 3, 3, 1]
        let poly = vec![Value::Int(1), Value::Int(3), Value::Int(3), Value::Int(1)];
        let factors = square_free_factor(&poly, "x").unwrap();
        // Should get (x+1) with multiplicity 3
        assert_eq!(factors.len(), 1);
        assert_eq!(factors[0].1, 3);
    }

    #[test]
    fn test_square_free_two_distinct_factors() {
        // (x+1)*(x-1)^2 = (x+1)(x^2 - 2x + 1) = x^3 - x^2 - x + 1 = [1, -1, -1, 1]
        let poly = vec![Value::Int(1), Value::Int(-1), Value::Int(-1), Value::Int(1)];
        let factors = square_free_factor(&poly, "x").unwrap();
        // Should get two factors: (x-1) with mult 2 and something with mult 1
        assert!(!factors.is_empty());
        let total_deg: usize = factors.iter().map(|(f, m)| (poly_degree(f)) * m).sum();
        assert_eq!(total_deg, 3);
    }

    #[test]
    fn test_square_free_square_free_input() {
        // x^2 + x + 1 = [1, 1, 1], irreducible over reals, square-free
        let poly = vec![Value::Int(1), Value::Int(1), Value::Int(1)];
        let factors = square_free_factor(&poly, "x").unwrap();
        // Should be a single factor with multiplicity 1
        assert_eq!(factors.len(), 1);
        assert_eq!(factors[0].1, 1);
        assert_eq!(poly_degree(&factors[0].0), 2);
    }

    #[test]
    fn test_square_free_x2_plus_4() {
        // x^2 + 4 = [4, 0, 1], square-free
        let poly = vec![Value::Int(4), Value::Int(0), Value::Int(1)];
        let factors = square_free_factor(&poly, "x").unwrap();
        assert_eq!(factors.len(), 1, "x^2+4 should be square-free");
        assert_eq!(factors[0].1, 1);
        assert_eq!(
            factors[0].0,
            vec![Value::Int(4), Value::Int(0), Value::Int(1)]
        );
    }

    #[test]
    fn test_integrate_one_over_x2_plus_4() {
        // int 1/(x^2+4) dx = arctan[x/2]/2
        let result = integrate_one_over_quadratic(
            &Value::Int(1), // c_val = b_prime
            &Value::Int(0), // b
            &Value::Int(4), // c
            "x",
        )
        .unwrap();
        assert_eq!(result.to_string(), "arctan[x/2]/2");
    }

    #[test]
    fn test_integrate_one_over_x2_minus_4() {
        // int 1/(x^2-4) dx = ln[abs[(x-2)/(x+2)]]/4
        let result = integrate_one_over_quadratic(
            &Value::Int(1),  // c_val = b_prime
            &Value::Int(0),  // b
            &Value::Int(-4), // c
            "x",
        )
        .unwrap();
        assert_eq!(result.to_string(), "ln[abs[(x - 2)/(x + 2)]]/4");
    }

    #[test]
    fn test_integrate_simple_quadratic_x2_plus_4() {
        // Numer = [1], Denom = [4, 0, 1], factor = [4, 0, 1], mult = 1
        let result = integrate_simple_quadratic(
            &[Value::Int(1)],
            &[Value::Int(4), Value::Int(0), Value::Int(1)],
            &[Value::Int(4), Value::Int(0), Value::Int(1)],
            &Value::Int(0),
            &Value::Int(4),
            "x",
        )
        .unwrap();
        assert_eq!(result.to_string(), "arctan[x/2]/2");
    }

    #[test]
    fn test_extract_and_integrate_x2_plus_4() {
        // Full pipeline: extract_rational -> integrate_by_rational for 1/(x^2+4)
        let expr = op(
            CasOp::Power,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                        Value::Int(4),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let result = integrate_by_rational(&expr, "x", crate::cas::CasDebug::disabled())
            .unwrap()
            .unwrap();
        assert_eq!(result.to_string(), "arctan[x/2]/2");
    }

    #[test]
    fn test_hermite_reduces_repeated_quadratic_with_rest() {
        // 1 / ((x^2+1)^2 * (x+1)) should reduce to:
        //   (x+1)/(4*(x^2+1)) + int (x+3)/(4*(x^2+1)*(x+1)) dx
        let factor = vec![Value::Int(1), Value::Int(0), Value::Int(1)];
        let rest = vec![Value::Int(1), Value::Int(1)];
        let denom = poly_mul(&poly_pow(&factor, 2).unwrap(), &rest).unwrap();

        let (rational_part, new_numer, reduced_pow) =
            hermite_reduce_one_step(&[Value::Int(1)], &denom, &factor, 2, "x").unwrap();

        assert_eq!(reduced_pow, 1);
        assert_eq!(
            new_numer,
            vec![
                Value::from_fraction_parts(BigInt::from(3), BigInt::from(4)),
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(4)),
            ]
        );
        assert_eq!(rational_part.to_string(), "(x/4 + 1/4)/(x^2 + 1)");
    }

    #[test]
    fn test_integrate_repeated_quadratic_with_rest() {
        use crate::cas::diff::diff_cas;
        use crate::cas::integrate::integrate_cas;
        use crate::cas::{eval_numeric_cas, substitute_cas};

        let x = Value::from_cas_var("x");
        let x2_plus_1 = op(
            CasOp::Add,
            vec![
                op(CasOp::Power, vec![x.clone(), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let x_plus_1 = op(CasOp::Add, vec![x.clone(), Value::Int(1)]);
        let denom = op(
            CasOp::Multiply,
            vec![op(CasOp::Power, vec![x2_plus_1, Value::Int(2)]), x_plus_1],
        );
        let expr = op(CasOp::Power, vec![denom, Value::Int(-1)]);

        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(
            s.contains("ln[abs[x + 1]]/4"),
            "expected single linear-factor contribution, got: {s}"
        );

        let derivative = diff_cas(&result, &Value::from_cas_var("x")).unwrap();
        let difference = cas_sub(derivative, expr).unwrap();
        let at_two =
            substitute_cas(&difference, &Value::from_cas_var("x"), &Value::Int(2)).unwrap();
        let numeric = eval_numeric_cas(&at_two).unwrap();
        let f = numeric
            .as_f64()
            .expect("numeric derivative check should produce a float");
        assert!(f.abs() < 1e-12, "expected derivative difference 0, got {f}");
    }

    #[test]
    fn test_find_real_algebraic_root_quadratic() {
        // x^2 - 2 = 0 -> root sqrt(2) approx 1.414
        let poly = vec![Value::Int(-2), Value::Int(0), Value::Int(1)];
        let root = find_real_algebraic_root(&poly).unwrap();
        if let Value::Algebraic(a) = &root {
            // Interval should contain sqrt(2)
            let interval = a.interval();
            let ok = interval.0 < 1.42 && interval.1 > 1.41;
            assert!(
                ok,
                "interval ({}, {}) does not contain sqrt(2)",
                interval.0, interval.1
            );
            // Generator alpha: coeffs [0, 1]
            assert_eq!(a.coeffs[0], Value::Int(0));
            assert_eq!(a.coeffs[1], Value::Int(1));
        } else {
            panic!("expected algebraic");
        }
    }

    #[test]
    fn test_find_real_algebraic_root_cubic() {
        // x^3 - 2 = 0 -> root cbrt(2) approx 1.26
        let poly = vec![Value::Int(-2), Value::Int(0), Value::Int(0), Value::Int(1)];
        let root = find_real_algebraic_root(&poly);
        assert!(root.is_some());
        if let Some(Value::Algebraic(a)) = &root {
            assert_eq!(poly_degree(&a.coeffs), 1);
        }
    }

    #[test]
    fn test_find_real_roots_rational_only() {
        // (z-1)(z-2)(z-3) = z^3 - 6z^2 + 11z - 6
        let poly = vec![
            Value::Int(-6),
            Value::Int(11),
            Value::Int(-6),
            Value::Int(1),
        ];
        let (rational, algebraic) = find_real_roots_poly(&poly);
        assert_eq!(rational.len(), 3); // 1, 2, 3
        assert_eq!(algebraic.len(), 0);
    }

    #[test]
    fn test_find_real_roots_mixed() {
        // (z-1)(z^2-2) = z^3 - z^2 - 2z + 2 = 0
        // Roots: 1, sqrt(2), -sqrt(2)
        let poly = vec![Value::Int(2), Value::Int(-2), Value::Int(-1), Value::Int(1)];
        let (rational, algebraic) = find_real_roots_poly(&poly);
        assert_eq!(rational, vec![Value::Int(1)]);
        assert_eq!(
            algebraic.len(),
            2,
            "expected +/-sqrt(2), got rational={:?} algebraic={:?}",
            rational,
            algebraic
        );
        assert!(algebraic.iter().any(numeric_is_negative));
        assert!(algebraic.iter().any(|root| !numeric_is_negative(root)));
        for root in &algebraic {
            let value = poly_evaluate(&poly, root).unwrap();
            assert!(numeric_is_zero(&value), "root {root} leaves value {value}");
        }
    }

    #[test]
    fn test_rt_algebraic_cubic_denom() {
        // int 1/(x^3-2) dx via RT with algebraic roots
        let numer = vec![Value::Int(1)];
        let denom = vec![Value::Int(-2), Value::Int(0), Value::Int(0), Value::Int(1)];
        let result = integrate_rothstein_trager(&numer, &denom, "x");
        match result {
            Ok(val) => {
                let s = val.to_string();
                assert!(s.contains("ln"), "expected log terms: {s}");
            }
            Err(e) => {
                panic!("RT failed: {e:?}");
            }
        }
    }

    #[test]
    fn test_algebraic_sqrt_of_rational_sqrt2() {
        let result = algebraic_sqrt_of_rational(&Value::Int(2)).unwrap();
        match &result {
            Value::Algebraic(a) => {
                assert_eq!(a.poly(), &[BigInt::from(-2), BigInt::zero(), BigInt::one()]);
                assert_eq!(a.coeffs[0], Value::Int(0));
                assert_eq!(a.coeffs[1], Value::Int(1));
            }
            _ => panic!("expected Algebraic, got {result:?}"),
        }
    }

    #[test]
    fn test_algebraic_sqrt_of_rational_perfect_square() {
        let result = algebraic_sqrt_of_rational(&Value::Int(4)).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn test_algebraic_sqrt_of_rational_fraction() {
        let result = algebraic_sqrt_of_rational(&Value::from_fraction_parts(
            BigInt::from(1),
            BigInt::from(2),
        ))
        .unwrap();
        match &result {
            Value::Algebraic(a) => {
                assert_eq!(a.coeffs[0], Value::Int(0));
                // sqrt(1/2) = sqrt(2)/2, so coeffs = [0, 1/2]
                match &a.coeffs[1] {
                    Value::Fraction(f) => {
                        assert_eq!(*f.numer(), BigInt::one());
                        assert_eq!(*f.denom(), BigInt::from(2));
                    }
                    _ => panic!("expected Fraction, got {:?}", a.coeffs[1]),
                }
            }
            _ => panic!("expected Algebraic, got {result:?}"),
        }
    }

    #[test]
    fn test_solve_quartic_exact_x4_plus_1() {
        let poly = vec![
            Value::Int(1),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(1),
        ];
        let factors = solve_quartic_exact(&poly).unwrap().unwrap();
        assert_eq!(factors.len(), 2);
        for f in &factors {
            assert_eq!(poly_degree(f), 2);
            // quadratic should be monic
            assert_eq!(f.last(), Some(&Value::Int(1)));
        }
        // Multiply factors to get original
        let mut product = poly_mul(&factors[0], &factors[1]).unwrap();
        poly_trim(&mut product);
        assert_eq!(poly_degree(&product), 4);
        assert_eq!(product.len(), poly.len());
        for (actual, expected) in product.iter().zip(&poly) {
            let diff = numeric_sub(actual, expected).unwrap();
            assert!(
                numeric_is_zero(&diff),
                "expected product coefficient {expected}, got {actual}"
            );
        }
    }

    #[test]
    fn test_solve_cubic_by_rational_root_fully_reducible() {
        // (z-1)(z-2)(z-3) = z^3 - 6z^2 + 11z - 6
        let poly = vec![
            Value::Int(-6),
            Value::Int(11),
            Value::Int(-6),
            Value::Int(1),
        ];
        let factors = solve_cubic_by_rational_root(&poly).unwrap().unwrap();
        // Should find all 3 linear factors
        let linear_count = factors.iter().filter(|f| poly_degree(f) == 1).count();
        assert_eq!(
            linear_count, 3,
            "expected 3 linear factors, got {:?}",
            factors
        );
    }

    #[test]
    fn test_solve_cubic_by_rational_root_irreducible() {
        // x^3 - 2 (irreducible over Q)
        let poly = vec![Value::Int(-2), Value::Int(0), Value::Int(0), Value::Int(1)];
        assert!(solve_cubic_by_rational_root(&poly).unwrap().is_none());
    }

    #[test]
    fn test_integrate_one_over_x4_plus_1() {
        use crate::cas::integrate::integrate_cas;
        let expr = op(
            CasOp::Power,
            vec![
                op(
                    CasOp::Add,
                    vec![
                        op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(4)]),
                        Value::Int(1),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let result = integrate_cas(&expr, &Value::from_cas_var("x")).unwrap();
        let s = result.to_string();
        assert!(s.contains("arctan"), "expected arctan, got: {s}");
        assert!(s.contains("2^(1/2)"), "expected 2^(1/2), got: {s}");
        assert!(
            !s.contains("1/2^(-1/2)"),
            "expected reciprocal radical to simplify, got: {s}"
        );
    }
}
