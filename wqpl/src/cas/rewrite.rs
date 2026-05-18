use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive};

use crate::session::dbglog::DebugLogFlags;
use crate::value::{Value, WqResult};

use super::{
    cas_add, cas_div, cas_err, cas_mul, cas_neg, cas_pow, cas_sub, collect_single_poly_var,
    common_numeric_gcd, eval_numeric_binary, expand_expr, factor_expr, numeric_is_negative,
    numeric_is_one, poly_from_expr, rebuild_scaled_term, simplify_cas_value, split_add_term,
};

pub(super) fn push_flattened(out: &mut Vec<Value>, op: &str, value: Value) {
    if let Some((inner_op, inner_args)) = value.cas_op_parts()
        && inner_op == op
    {
        out.extend(inner_args.iter().cloned());
    } else {
        out.push(value);
    }
}

/// Build a product `Value` from a list of factors.
/// `[]` → 1, `[x]` → x, `[x, y, …]` → (* x y …).
pub(crate) fn cas_product(factors: Vec<Value>) -> Value {
    match factors.len() {
        0 => Value::Int(1),
        1 => factors.into_iter().next().unwrap(),
        _ => Value::from_cas_op("*", factors),
    }
}

fn extract_unit_negative(arg: &Value) -> Option<Value> {
    let ("*", args) = arg.cas_op_parts()? else {
        return None;
    };
    let (first, rest) = args.split_first()?;
    if first.is_cas_expr() || !first.exact_int_is(-1) {
        return None;
    }
    Some(match rest {
        [single] => single.clone(),
        _ => Value::from_cas_op("*", rest.to_vec()),
    })
}

fn take_additive_constant(arg: &Value, target: f64) -> Option<Value> {
    let ("+", args) = arg.cas_op_parts()? else {
        return None;
    };
    let mut rest = Vec::with_capacity(args.len().saturating_sub(1));
    let mut removed = false;
    for term in args {
        if !removed
            && !term.is_cas_expr()
            && term
                .as_f64()
                .is_some_and(|value| (value - target).abs() <= 1e-12)
        {
            removed = true;
        } else {
            rest.push(term.clone());
        }
    }
    if !removed {
        return None;
    }
    Some(match rest.len() {
        0 => Value::Int(0),
        1 => rest.into_iter().next().expect("single additive remainder"),
        _ => Value::from_cas_op("+", rest),
    })
}

fn contains_symbolic_var(value: &Value) -> bool {
    if value.cas_var_name().is_some() {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_symbolic_var);
    }
    if let Some((_, args)) = value.cas_call_parts() {
        return args.iter().any(contains_symbolic_var);
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_symbolic_var(lhs) || contains_symbolic_var(rhs);
    }
    false
}

fn contains_negative_power(value: &Value) -> bool {
    if let Some(("^", [_, exp])) = value.cas_op_parts()
        && exp.rational_parts().is_some_and(|(n, _)| n.is_negative())
    {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_negative_power);
    }
    if let Some((_, args)) = value.cas_call_parts() {
        return args.iter().any(contains_negative_power);
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_negative_power(lhs) || contains_negative_power(rhs);
    }
    false
}

fn contains_var_dependent_fractional_power(value: &Value) -> bool {
    if let Some(("^", [base, exp])) = value.cas_op_parts()
        && exp.rational_parts().is_some_and(|(_, d)| !d.is_one())
        && contains_symbolic_var(base)
    {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_var_dependent_fractional_power);
    }
    if let Some((_, args)) = value.cas_call_parts() {
        return args.iter().any(contains_var_dependent_fractional_power);
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_var_dependent_fractional_power(lhs)
            || contains_var_dependent_fractional_power(rhs);
    }
    false
}

/// Extract `numer/denom` from a term represented as `numer * denom^(-1)`.
/// Returns `(numer, denom)` when there is exactly one reciprocal factor.
fn extract_reciprocal_term(term: &Value) -> Option<(Value, Value)> {
    if let Some(("^", [base, exp])) = term.cas_op_parts()
        && let Some((numer, denom)) = exp.rational_parts()
        && numer.is_negative()
    {
        let abs_exp = Value::from_fraction_parts(-numer, denom);
        let recip_base = if numeric_is_one(&abs_exp) {
            base.clone()
        } else {
            Value::from_cas_op("^", vec![base.clone(), abs_exp])
        };
        return Some((Value::Int(1), recip_base));
    }
    let Some(("*", args)) = term.cas_op_parts() else {
        return None;
    };
    let mut reciprocal_base = None;
    let mut reciprocal_count = 0usize;
    let mut numer_factors = Vec::with_capacity(args.len());
    for arg in args {
        if let Some(("^", [base, exp])) = arg.cas_op_parts()
            && let Some((numer, denom)) = exp.rational_parts()
            && numer.is_negative()
        {
            reciprocal_count += 1;
            let abs_exp = Value::from_fraction_parts(-numer, denom);
            reciprocal_base = Some(if numeric_is_one(&abs_exp) {
                base.clone()
            } else {
                Value::from_cas_op("^", vec![base.clone(), abs_exp])
            });
        } else {
            numer_factors.push(arg.clone());
        }
    }
    if reciprocal_count != 1 {
        return None;
    }
    Some((cas_product(numer_factors), reciprocal_base?))
}

/// Rewrite `N/K + C` (or `C + N/K`) into `(N + C*K)/K` when `K` is
/// variable-free. This removes nested constant divisors from larger
/// denominators and helps downstream rational-term combination.
fn try_combine_var_free_denominator_sum(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() != 2 {
        return Ok(None);
    }
    for (frac_i, other_i) in [(0, 1), (1, 0)] {
        let Some((numer, denom)) = extract_reciprocal_term(&args[frac_i]) else {
            continue;
        };
        if contains_symbolic_var(&denom) {
            continue;
        }
        let scaled_other = cas_mul(vec![args[other_i].clone(), denom.clone()])?;
        let rewritten_numer = cas_add(vec![numer, scaled_other])?;
        let rewritten = cas_mul(vec![
            rewritten_numer,
            Value::from_cas_op("^", vec![denom, Value::Int(-1)]),
        ])?;
        return Ok(Some(rewritten));
    }
    Ok(None)
}

/// Rewrite `N/K ± 1` (or `±1 + N/K`) into `(N ± K)/K`.
///
/// This is a focused form of fraction addition that avoids relying on
/// polynomial coefficient extraction and helps simplify nested square-root
/// chains generated by inverse-trig compositions.
fn try_combine_unit_with_fraction_sum(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() != 2 {
        return Ok(None);
    }
    for (frac_i, unit_i) in [(0, 1), (1, 0)] {
        let Some((numer, denom)) = extract_reciprocal_term(&args[frac_i]) else {
            continue;
        };
        if contains_negative_power(&numer) {
            continue;
        }
        if contains_var_dependent_fractional_power(&denom) {
            continue;
        }
        let unit = &args[unit_i];
        if !unit.exact_int_is(1) && !unit.exact_int_is(-1) {
            continue;
        }
        let scaled = cas_mul(vec![unit.clone(), denom.clone()])?;
        let rewritten_numer = cas_add(vec![numer, scaled])?;
        return Ok(Some(cas_div(rewritten_numer, denom)?));
    }
    Ok(None)
}

/// Match `A/B + C` (or `C + A/B`) where `A/B` is represented as
/// `A * B^(-1)`. Returns `(A, B, C)`.
fn match_affine_over_sum(expr: &Value) -> Option<(Value, Value, Value)> {
    let (op, args) = expr.cas_op_parts()?;
    if op != "+" {
        return None;
    }
    if args.len() != 2 {
        return None;
    }
    for (frac_i, other_i) in [(0, 1), (1, 0)] {
        if let Some((numer, denom)) = extract_reciprocal_term(&args[frac_i]) {
            return Some((numer, denom, args[other_i].clone()));
        }
    }
    None
}

/// Try cancelling `(A + B*C)` when dividing `(A/B + C)` by a denominator that
/// contains `(A + B*C)` as a factor:
///
///   (A/B + C) / (R*(A + B*C)) -> 1/(B*R)
pub(super) fn try_cancel_affine_over_factor(lhs: &Value, rhs: &Value) -> WqResult<Option<Value>> {
    let Some((a, b, c)) = match_affine_over_sum(lhs) else {
        return Ok(None);
    };

    let affine_factor = simplify_cas_value(&cas_add(vec![a, cas_mul(vec![b.clone(), c])?])?)?;
    let rhs_factored = factor_expr(rhs)?;
    let mut rhs_factors = if let Some(("*", args)) = rhs_factored.cas_op_parts() {
        args.to_vec()
    } else {
        vec![rhs_factored.clone()]
    };

    let mut sign_flip = false;
    let idx = if let Some(i) = rhs_factors.iter().position(|f| *f == affine_factor) {
        Some(i)
    } else {
        let neg_affine = cas_neg(affine_factor.clone())?;
        if let Some(i) = rhs_factors.iter().position(|f| *f == neg_affine) {
            sign_flip = true;
            Some(i)
        } else {
            None
        }
    };
    let Some(idx) = idx else {
        return Ok(None);
    };
    rhs_factors.remove(idx);

    let mut denom_factors = vec![b];
    let remaining = cas_product(rhs_factors);
    if !numeric_is_one(&remaining) {
        denom_factors.push(remaining);
    }
    if sign_flip {
        denom_factors.push(Value::Int(-1));
    }
    Ok(Some(cas_div(Value::Int(1), cas_mul(denom_factors)?)?))
}

/// Try cancelling affine-over factors in `lhs * rhs^(-1)` form.
fn try_cancel_affine_over_product(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() != 2 {
        return Ok(None);
    }
    for (lhs_i, rhs_i) in [(0, 1), (1, 0)] {
        let Some(("^", [rhs, exp])) = args[rhs_i].cas_op_parts() else {
            continue;
        };
        if !exp.exact_int_is(-1) {
            continue;
        }
        if let Some(cancelled) = try_cancel_affine_over_factor(&args[lhs_i], rhs)? {
            return Ok(Some(cancelled));
        }
    }
    Ok(None)
}

/// In larger sums, merge two variable-free terms when factoring the pair
/// produces a simpler shared-factor form.
fn try_merge_var_free_sum_pair(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() < 3 {
        return Ok(None);
    }
    for i in 0..args.len() {
        if contains_symbolic_var(&args[i]) {
            continue;
        }
        for j in (i + 1)..args.len() {
            if contains_symbolic_var(&args[j]) {
                continue;
            }
            let pair = cas_add(vec![args[i].clone(), args[j].clone()])?;
            let merged = factor_expr(&pair)?;
            if merged == pair {
                continue;
            }
            let mut new_args = Vec::with_capacity(args.len() - 1);
            for (idx, term) in args.iter().enumerate() {
                if idx == i {
                    new_args.push(merged.clone());
                    continue;
                }
                if idx == j {
                    continue;
                }
                new_args.push(term.clone());
            }
            return Ok(Some(cas_add(new_args)?));
        }
    }
    Ok(None)
}

/// For a binary variable-free sum, allow common-factor extraction.
fn try_factor_var_free_binary_sum(value: &Value, args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() != 2 {
        return Ok(None);
    }
    if args.iter().any(contains_symbolic_var) {
        return Ok(None);
    }
    let factored = factor_expr(value)?;
    if factored == *value {
        Ok(None)
    } else {
        Ok(Some(factored))
    }
}

fn combine_logs_in_sum(args: &[Value]) -> WqResult<Option<Value>> {
    let mut other_terms = Vec::with_capacity(args.len());
    let mut log_args = Vec::new();
    for term in args {
        if let Some(("ln", [arg])) = term.cas_call_parts() {
            log_args.push(arg.clone());
        } else {
            other_terms.push(term.clone());
        }
    }
    if log_args.len() < 2 {
        return Ok(None);
    }
    other_terms.push(Value::from_cas_call("ln", vec![cas_mul(log_args)?]));
    Ok(Some(cas_add(other_terms)?))
}

fn rewrite_sgn_abs_product(args: &[Value]) -> WqResult<Option<Value>> {
    let mut sgn_arg = None;
    let mut abs_arg = None;
    let mut abs_power = None;
    for arg in args {
        if let Some(("sgn", [s])) = arg.cas_call_parts() {
            sgn_arg = Some(s.clone());
        } else if let Some(("^", [base, exp])) = arg.cas_op_parts() {
            if let Some(("abs", [a])) = base.cas_call_parts() {
                abs_arg = Some(a.clone());
                abs_power = Some(exp.clone());
            }
        } else if let Some(("abs", [a])) = arg.cas_call_parts() {
            abs_arg = Some(a.clone());
            abs_power = Some(Value::Int(1));
        }
    }
    let (Some(s), Some(a), Some(p)) = (&sgn_arg, &abs_arg, &abs_power) else {
        return Ok(None);
    };
    if s != a {
        return Ok(None);
    }
    let replacement = if p.exact_int_is(1) {
        s.clone()
    } else if p.exact_int_is(-1) {
        cas_pow(s.clone(), Value::Int(-1))?
    } else {
        return Ok(None);
    };

    let mut new_args = Vec::new();
    let mut removed_sgn = false;
    let mut removed_abs = false;
    for arg in args {
        let is_sgn = !removed_sgn
            && arg
                .cas_call_parts()
                .is_some_and(|(n, a2)| n == "sgn" && a2.len() == 1 && a2[0] == *s);
        let is_abs = !removed_abs
            && arg
                .cas_call_parts()
                .is_some_and(|(n, a2)| n == "abs" && a2.len() == 1 && a2[0] == *a);
        let is_abs_inv = !removed_abs
            && arg.cas_op_parts().is_some_and(|(op, a2)| {
                op == "^"
                    && a2.len() == 2
                    && a2[1].exact_int_is(-1)
                    && a2[0]
                        .cas_call_parts()
                        .is_some_and(|(n, a3)| n == "abs" && a3.len() == 1 && a3[0] == *a)
            });

        if is_sgn {
            removed_sgn = true;
            continue;
        }
        if is_abs || is_abs_inv {
            removed_abs = true;
            continue;
        }
        new_args.push(arg.clone());
    }
    new_args.push(replacement);
    Ok(Some(cas_mul(new_args)?))
}

/// Check whether `expr` is provably positive for all real values of the
/// variable it contains.  Used to drop unnecessary `abs` wrappers.
fn is_provably_positive(expr: &Value) -> bool {
    // Positive numeric constant
    if !expr.is_cas_expr() {
        return expr.as_f64().is_some_and(|f| f > 0.0);
    }
    // Quadratic a·x² + b·x + c with a > 0 and disc < 0 → always > 0
    let Some(("+", _)) = expr.cas_op_parts() else {
        return false;
    };
    let mut found_var = None;
    if !collect_single_poly_var(expr, &mut found_var) {
        return false;
    }
    let Some(var) = found_var else {
        return false;
    };
    let coeffs = match poly_from_expr(expr, &var) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if coeffs.len() != 3 {
        return false;
    }
    // a = coeffs[2], b = coeffs[1], c = coeffs[0]
    let a = &coeffs[2];
    let b = &coeffs[1];
    let c = &coeffs[0];
    // a must be positive
    if numeric_is_negative(a) || !a.as_f64().is_some_and(|f| f > 0.0) {
        return false;
    }
    // disc = b² - 4ac must be negative
    let Ok(b_sq) = eval_numeric_binary("*", b, b) else {
        return false;
    };
    let Ok(ac) = eval_numeric_binary("*", a, c) else {
        return false;
    };
    let Ok(four_ac) = eval_numeric_binary("*", &Value::Int(4), &ac) else {
        return false;
    };
    let Ok(disc) = eval_numeric_binary("-", &b_sq, &four_ac) else {
        return false;
    };
    // For Algebraic values, use is_negative (checks coeffs with generator sign).
    // The Algebraic value c0 + c1·α + ... has sign = sign(ck) when α > 0 and
    // only one non-zero coeff.  For the general case, trust as_f64.
    if let Value::Algebraic(da) = &disc {
        da.is_negative()
    } else {
        disc.as_f64().is_some_and(|f| f < 0.0)
    }
}

fn exact_int_value_is(value: &Value, expected: i64) -> bool {
    value
        .exact_int()
        .and_then(|int| int.to_i64())
        .is_some_and(|int| int == expected)
}

/// (+ (* A B) (* (* -1 A) C)) → (* A (+ B (* -1 C))).
/// The second term must have coefficient -1 so that after A is factored,
/// the inner sum is B - C.
fn try_factor_binary_product(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() != 2 {
        return Ok(None);
    }
    // Try each ordering: term i has the positive factor, term j has the negative
    for (pos_i, neg_i) in [(0, 1), (1, 0)] {
        let (coeff_pos, core_pos) = split_add_term(&args[pos_i]);
        let Some(ref core_pos) = core_pos else {
            continue;
        };
        let (coeff_neg, core_neg) = split_add_term(&args[neg_i]);
        let coeff_neg_is_plain_int = coeff_neg.exact_int_is(-1);
        if !coeff_neg_is_plain_int && !exact_int_value_is(&coeff_neg, -1) {
            continue;
        }
        // Both cores should be products sharing a common factor
        let factors_pos: Vec<Value> = match core_pos.cas_op_parts() {
            Some(("*", f)) => f.to_vec(),
            _ => vec![core_pos.clone()],
        };
        let factors_neg: Vec<Value> = match &core_neg {
            Some(c) if matches!(c.cas_op_parts(), Some(("*", _))) => {
                if let Some(("*", f)) = c.cas_op_parts() {
                    f.to_vec()
                } else {
                    continue;
                }
            }
            Some(c) => vec![c.clone()],
            None => continue,
        };
        // Find a common factor A that appears in both
        for fa in &factors_pos {
            if !fa.is_cas_expr() {
                continue;
            }
            if factors_neg.contains(fa) {
                let common = fa.clone();
                if !coeff_neg_is_plain_int && common.cas_call_parts().is_none() {
                    continue;
                }
                let rem_pos: Vec<Value> = factors_pos
                    .iter()
                    .filter(|f| *f != &common)
                    .cloned()
                    .collect();
                let rem_neg: Vec<Value> = factors_neg
                    .iter()
                    .filter(|f| *f != &common)
                    .cloned()
                    .collect();
                let new_core_pos = match rem_pos.len() {
                    0 => None,
                    1 => Some(rem_pos[0].clone()),
                    _ => Some(cas_product(rem_pos)),
                };
                let new_core_neg = match rem_neg.len() {
                    0 => None,
                    1 => Some(rem_neg[0].clone()),
                    _ => Some(cas_product(rem_neg)),
                };
                let term_pos = rebuild_scaled_term(coeff_pos.clone(), new_core_pos)?;
                let term_neg = rebuild_scaled_term(Value::Int(-1), new_core_neg)?;
                let inner = cas_add(vec![term_pos, term_neg])?;
                return Ok(Some(cas_mul(vec![common, inner])?));
            }
        }
    }
    Ok(None)
}

/// (* ... (^ D1 -1) ... (^ D2 -1) ...) → replace both with (^ (D1*D2) -1)
/// when D1*D2 expands to something simpler.
/// Only applies when both D1, D2 are simple sums (≤ 3 terms) to avoid large
/// expansions.
fn try_combine_inv_denoms(args: &[Value]) -> WqResult<Option<Value>> {
    let mut inv_info = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if let Some(("^", [base, e])) = arg.cas_op_parts()
            && e.exact_int_is(-1)
            && let Some(("+", sum_terms)) = base.cas_op_parts()
            && sum_terms.len() <= 3
        {
            inv_info.push((i, base.clone()));
        }
    }
    if inv_info.len() < 2 {
        return Ok(None);
    }
    for a in 0..inv_info.len() {
        for b in (a + 1)..inv_info.len() {
            let (i, ref base_i) = inv_info[a];
            let (j, ref base_j) = inv_info[b];
            let product = expand_expr(&cas_mul(vec![base_i.clone(), base_j.clone()])?)?;
            let raw = cas_mul(vec![base_i.clone(), base_j.clone()])?;
            if product == raw {
                continue;
            }
            // Factor out common numeric gcd to expose structural cancellation
            let product = if let Some(("+", sum_args)) = product.cas_op_parts()
                && let Some(gcd) = common_numeric_gcd(sum_args)
            {
                let mut factored = Vec::with_capacity(sum_args.len());
                for arg in sum_args {
                    factored.push(cas_div(arg.clone(), gcd.clone())?);
                }
                cas_mul(vec![gcd, cas_add(factored)?])?
            } else {
                product
            };
            let combined = Value::from_cas_op("^", vec![product, Value::Int(-1)]);
            let mut new_args: Vec<Value> = Vec::with_capacity(args.len() - 1);
            for (k, arg) in args.iter().enumerate() {
                if k == i || k == j {
                    continue;
                }
                new_args.push(arg.clone());
            }
            new_args.push(combined);
            return Ok(Some(simplify_cas_value(&Value::from_cas_op(
                "*", new_args,
            ))?));
        }
    }
    Ok(None)
}

fn apply_tree_rewrite(value: &Value) -> WqResult<Option<Value>> {
    if let Some(("+", args)) = value.cas_op_parts() {
        if let Some(result) = try_combine_unit_with_fraction_sum(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_combine_var_free_denominator_sum(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_factor_var_free_binary_sum(value, args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_merge_var_free_sum_pair(args)? {
            return Ok(Some(result));
        }
        // Factor common product from sum: (+ (* A B) (* (* -1 A) C)) → (* A (+ B (* -1
        // C)))
        if let Some(result) = try_factor_binary_product(args)? {
            return Ok(Some(result));
        }
        return combine_logs_in_sum(args);
    }

    // Distribute -1 over sum: (* -1 (+ a b …)) → (+ (* -1 a) (* -1 b) …)
    if let Some(("*", [a, b])) = value.cas_op_parts() {
        if let (Some(("+", sum_args)), true) = (b.cas_op_parts(), a.exact_int_is(-1)) {
            let new_args: Vec<Value> = sum_args
                .iter()
                .map(|arg| cas_neg(arg.clone()))
                .collect::<WqResult<_>>()?;
            return Ok(Some(cas_add(new_args)?));
        }
        if let (Some(("+", sum_args)), true) = (a.cas_op_parts(), b.exact_int_is(-1)) {
            let new_args: Vec<Value> = sum_args
                .iter()
                .map(|arg| cas_neg(arg.clone()))
                .collect::<WqResult<_>>()?;
            return Ok(Some(cas_add(new_args)?));
        }
    }

    if let Some(("*", args)) = value.cas_op_parts()
        && let Some(result) = try_cancel_affine_over_product(args)?
    {
        return Ok(Some(result));
    }

    if let Some(("*", args)) = value.cas_op_parts()
        && let Some(result) = rewrite_sgn_abs_product(args)?
    {
        return Ok(Some(result));
    }

    // Combine (^ D1 -1) * (^ D2 -1) → (^ (D1*D2) -1) when D1*D2 expands usefully
    if let Some(("*", args)) = value.cas_op_parts()
        && let Some(result) = try_combine_inv_denoms(args)?
    {
        return Ok(Some(result));
    }

    if let Some(("^", [base, exp])) = value.cas_op_parts() {
        if exp.exact_half()
            && let Some(("^", [inner_base, inner_exp])) = base.cas_op_parts()
            && inner_exp.exact_int_is(2)
        {
            return Ok(Some(Value::from_cas_call("abs", vec![inner_base.clone()])));
        }
        if exp.exact_int_is(2)
            && let Some(("abs", [arg])) = base.cas_call_parts()
        {
            return Ok(Some(cas_pow(arg.clone(), Value::Int(2))?));
        }
    }

    let Some((name, args)) = value.cas_call_parts() else {
        return Ok(None);
    };
    let rewritten = match (name, args) {
        ("ln", [arg]) if matches!(arg.cas_call_parts(), Some(("exp", [_]))) => {
            let Some((_, [inner])) = arg.cas_call_parts() else {
                unreachable!("matched exp call")
            };
            Some(inner.clone())
        }
        ("exp", [arg]) if matches!(arg.cas_call_parts(), Some(("ln", [_]))) => {
            let Some((_, [inner])) = arg.cas_call_parts() else {
                unreachable!("matched ln call")
            };
            Some(inner.clone())
        }
        ("ln", [arg]) if matches!(arg.cas_op_parts(), Some(("^", [_, _]))) => {
            let Some(("^", [base, exp])) = arg.cas_op_parts() else {
                unreachable!("matched power")
            };
            Some(cas_mul(vec![
                exp.clone(),
                Value::from_cas_call("ln", vec![base.clone()]),
            ])?)
        }
        ("sin", [arg]) if matches!(arg.cas_call_parts(), Some(("arcsin", [_]))) => {
            // sin(arcsin(t)) = t
            let Some(("arcsin", [inner])) = arg.cas_call_parts() else {
                unreachable!()
            };
            Some(inner.clone())
        }
        ("cos", [arg]) if matches!(arg.cas_call_parts(), Some(("arccos", [_]))) => {
            // cos(arccos(t)) = t
            let Some(("arccos", [inner])) = arg.cas_call_parts() else {
                unreachable!()
            };
            Some(inner.clone())
        }
        ("tan", [arg]) if matches!(arg.cas_call_parts(), Some(("arctan", [_]))) => {
            // tan(arctan(t)) = t
            let Some(("arctan", [inner])) = arg.cas_call_parts() else {
                unreachable!()
            };
            Some(inner.clone())
        }
        ("sin", [arg]) if matches!(arg.cas_call_parts(), Some(("arccos", [_]))) => {
            // sin(arccos(t)) = sqrt(1 - t^2)
            let Some(("arccos", [inner])) = arg.cas_call_parts() else {
                unreachable!()
            };
            Some(cas_pow(
                cas_sub(Value::Int(1), cas_pow(inner.clone(), Value::Int(2))?)?,
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            )?)
        }
        ("cos", [arg]) if matches!(arg.cas_call_parts(), Some(("arcsin", [_]))) => {
            // cos(arcsin(t)) = sqrt(1 - t^2)
            let Some(("arcsin", [inner])) = arg.cas_call_parts() else {
                unreachable!()
            };
            Some(cas_pow(
                cas_sub(Value::Int(1), cas_pow(inner.clone(), Value::Int(2))?)?,
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            )?)
        }
        ("sin", [arg]) => {
            if let Some(inner) = extract_unit_negative(arg) {
                Some(cas_neg(Value::from_cas_call("sin", vec![inner]))?)
            } else if let Some(shifted) = take_additive_constant(arg, std::f64::consts::FRAC_PI_2) {
                Some(Value::from_cas_call("cos", vec![shifted]))
            } else {
                let (coeff, core) = split_add_term(arg);
                if coeff.exact_int_is(2) {
                    core.map(|inner| {
                        cas_mul(vec![
                            Value::Int(2),
                            Value::from_cas_call("sin", vec![inner.clone()]),
                            Value::from_cas_call("cos", vec![inner]),
                        ])
                    })
                    .transpose()?
                } else {
                    None
                }
            }
        }
        ("cos", [arg]) => {
            extract_unit_negative(arg).map(|inner| Value::from_cas_call("cos", vec![inner]))
        }
        ("abs", [arg]) => {
            if is_provably_positive(arg) {
                Some(arg.clone())
            } else {
                None
            }
        }
        _ => None,
    };
    Ok(rewritten)
}

pub(super) fn rewrite_expr(value: &Value) -> WqResult<Value> {
    let rewritten = if let Some((lhs, rhs)) = value.cas_eq_parts() {
        Value::from_cas_eq(rewrite_expr(lhs)?, rewrite_expr(rhs)?)
    } else if let Some((op, args)) = value.cas_op_parts() {
        let mut rewritten_args = Vec::with_capacity(args.len());
        for arg in args {
            rewritten_args.push(rewrite_expr(arg)?);
        }
        simplify_cas_value(&Value::from_cas_op(op, rewritten_args))?
    } else if let Some((name, args)) = value.cas_call_parts() {
        let mut rewritten_args = Vec::with_capacity(args.len());
        for arg in args {
            rewritten_args.push(rewrite_expr(arg)?);
        }
        simplify_cas_value(&Value::from_cas_call(name, rewritten_args))?
    } else {
        value.clone()
    };

    match apply_tree_rewrite(&rewritten)? {
        Some(next) => simplify_cas_value(&next),
        None => Ok(rewritten),
    }
}

pub(crate) fn rewrite_cas(expr: &Value) -> WqResult<Value> {
    let mut current = simplify_cas_value(expr)?;
    rewrite_loop(&mut current)?;
    Ok(current)
}

/// Apply tree rewrites in a loop without an initial simplify pass.
/// This allows callers to do rewrites first and simplify afterward, so that
/// rational-term combination sees the already-rewritten expression.
pub(crate) fn rewrite_loop(current: &mut Value) -> WqResult<()> {
    for i in 0..32 {
        let next = rewrite_expr(current)?;
        if next == *current {
            cas_trace!(
                DebugLogFlags::CAS_VERBOSE,
                "[cas-v] rewrite_loop converged at iteration={i}"
            );
            return Ok(());
        }
        let next_fmt = next.format_cas().unwrap_or_else(|| next.to_string());
        cas_trace!(
            DebugLogFlags::CAS_VERBOSE,
            "[cas-v] rewrite_loop iteration={i} -> {next_fmt}"
        );
        *current = next;
    }
    cas_trace!(
        DebugLogFlags::CAS_VERBOSE,
        "[cas-v] rewrite_loop reached max iterations (32) without convergence"
    );
    Ok(())
}

pub(crate) fn normalize_root_objective_cas(input: &Value) -> WqResult<Value> {
    if let Some((lhs, rhs)) = input.cas_eq_parts() {
        cas_sub(lhs.clone(), rhs.clone())
    } else {
        simplify_cas_value(input)
    }
}

pub(crate) fn infer_single_cas_var(expr: &Value) -> WqResult<String> {
    let mut var = None;
    if !collect_single_poly_var(expr, &mut var) {
        return Err(cas_err(
            "expected an expression with exactly one symbolic variable",
        ));
    }
    var.ok_or_else(|| cas_err("expected an expression with exactly one symbolic variable"))
}

/// Check whether a CAS expression still contains the given variable.
pub(crate) fn contains_cas_var(expr: &Value, var: &str) -> bool {
    if expr.cas_var_name() == Some(var) {
        return true;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        return args.iter().any(|a| contains_cas_var(a, var));
    }
    if let Some((_, args)) = expr.cas_call_parts() {
        return args.iter().any(|a| contains_cas_var(a, var));
    }
    false
}
