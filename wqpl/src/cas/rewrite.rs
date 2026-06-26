use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::eqsat::rewrite_with_egg;
use super::{
    cas_add, cas_div, cas_err, cas_mul, cas_neg, cas_pow, cas_sub, collect_single_poly_var,
    common_numeric_gcd, eval_exact_numeric_div, eval_numeric_binary, expand_expr,
    extract_perfect_power_factor, factor_expr, numeric_add, numeric_is_negative, numeric_is_one,
    numeric_is_zero, numeric_mul, numeric_sub, poly_degree, poly_divide, poly_from_expr,
    poly_is_zero, rebuild_scaled_term, simplify_cas_value, split_add_term, with_cas_div_cache,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasConst, CasFunction, CasOp};
use crate::value::{Value, WqResult};

pub(super) fn push_flattened(out: &mut Vec<Value>, op: CasOp, value: Value) {
    if let Some((inner_op, inner_args)) = value.cas_op_parts()
        && inner_op == op
    {
        out.extend(inner_args.iter().cloned());
    } else {
        out.push(value);
    }
}

/// Build a product `Value` from a list of factors.
/// `[]` -> 1, `[x]` -> x, `[x, y, ...]` -> (* x y ...).
pub(crate) fn cas_product(factors: Vec<Value>) -> Value {
    match factors.len() {
        0 => Value::Int(1),
        1 => factors.into_iter().next().unwrap(),
        _ => Value::from_cas_op(CasOp::Multiply, factors),
    }
}

fn extract_unit_negative(arg: &Value) -> Option<Value> {
    let (CasOp::Multiply, args) = arg.cas_op_parts()? else {
        return None;
    };
    let (first, rest) = args.split_first()?;
    if first.is_cas_expr() || !first.exact_int_is(-1) {
        return None;
    }
    Some(match rest {
        [single] => single.clone(),
        _ => cas_product(rest.to_vec()),
    })
}

fn take_additive_constant(arg: &Value, target: f64) -> Option<Value> {
    let (CasOp::Add, args) = arg.cas_op_parts()? else {
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
        _ => Value::from_cas_op(CasOp::Add, rest),
    })
}

fn contains_symbolic_var(value: &Value) -> bool {
    if value.cas_var_name().is_some() {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_symbolic_var);
    }
    if let Some((_, args)) = value.cas_function_parts() {
        return args.iter().any(contains_symbolic_var);
    }
    if let Some((_, args)) = value.cas_apply_parts() {
        return args.iter().any(contains_symbolic_var);
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_symbolic_var(lhs) || contains_symbolic_var(rhs);
    }
    false
}

fn contains_negative_power(value: &Value) -> bool {
    if let Some((CasOp::Power, [_, exp])) = value.cas_op_parts()
        && exp.rational_parts().is_some_and(|(n, _)| n.is_negative())
    {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_negative_power);
    }
    if let Some((_, args)) = value.cas_function_parts() {
        return args.iter().any(contains_negative_power);
    }
    if let Some((_, args)) = value.cas_apply_parts() {
        return args.iter().any(contains_negative_power);
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_negative_power(lhs) || contains_negative_power(rhs);
    }
    false
}

fn contains_var_dependent_fractional_power(value: &Value) -> bool {
    if let Some((CasOp::Power, [base, exp])) = value.cas_op_parts()
        && exp.rational_parts().is_some_and(|(_, d)| !d.is_one())
        && contains_symbolic_var(base)
    {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_var_dependent_fractional_power);
    }
    if let Some((_, args)) = value.cas_function_parts() {
        return args.iter().any(contains_var_dependent_fractional_power);
    }
    if let Some((_, args)) = value.cas_apply_parts() {
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
    if let Some((CasOp::Power, [base, exp])) = term.cas_op_parts()
        && let Some((numer, denom)) = exp.rational_parts()
        && numer.is_negative()
    {
        let abs_exp = Value::from_fraction_parts(-numer, denom);
        let recip_base = if numeric_is_one(&abs_exp) {
            base.clone()
        } else {
            Value::from_cas_op(CasOp::Power, vec![base.clone(), abs_exp])
        };
        return Some((Value::Int(1), recip_base));
    }
    let Some((CasOp::Multiply, args)) = term.cas_op_parts() else {
        return None;
    };
    let mut reciprocal_base = None;
    let mut reciprocal_count = 0usize;
    let mut numer_factors = Vec::with_capacity(args.len());
    for arg in args {
        if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && let Some((numer, denom)) = exp.rational_parts()
            && numer.is_negative()
        {
            reciprocal_count += 1;
            let abs_exp = Value::from_fraction_parts(-numer, denom);
            reciprocal_base = Some(if numeric_is_one(&abs_exp) {
                base.clone()
            } else {
                Value::from_cas_op(CasOp::Power, vec![base.clone(), abs_exp])
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
            Value::from_cas_op(CasOp::Power, vec![denom, Value::Int(-1)]),
        ])?;
        return Ok(Some(rewritten));
    }
    Ok(None)
}

/// Rewrite `N/K +/- 1` (or `+/-1 + N/K`) into `(N +/- K)/K`.
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
    if op != CasOp::Add {
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
    let mut rhs_factors = if let Some((CasOp::Multiply, args)) = rhs_factored.cas_op_parts() {
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
        let Some((CasOp::Power, [rhs, exp])) = args[rhs_i].cas_op_parts() else {
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

fn remove_common_product_factors(lhs: &Value, rhs: &Value) -> Option<(Vec<Value>, Value, Value)> {
    let mut lhs_factors = if let Some((CasOp::Multiply, args)) = lhs.cas_op_parts() {
        args.to_vec()
    } else {
        vec![lhs.clone()]
    };
    let mut rhs_factors = if let Some((CasOp::Multiply, args)) = rhs.cas_op_parts() {
        args.to_vec()
    } else {
        vec![rhs.clone()]
    };

    let mut common = Vec::new();
    let mut i = 0;
    while i < lhs_factors.len() {
        if let Some(j) = rhs_factors
            .iter()
            .position(|factor| factor == &lhs_factors[i])
        {
            common.push(lhs_factors.remove(i));
            rhs_factors.remove(j);
        } else {
            i += 1;
        }
    }
    let has_inverse_sqrt_factor = common.iter().any(|factor| {
        matches!(
            factor.cas_op_parts(),
            Some((CasOp::Power, [base, exp])) if exp.exact_neg_half() && single_poly_degree(base) == Some(3)
        )
    });
    if common.is_empty() || !has_inverse_sqrt_factor {
        return None;
    }

    Some((common, cas_product(lhs_factors), cas_product(rhs_factors)))
}

fn try_factor_common_sum_pair(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() < 2 {
        return Ok(None);
    }
    for i in 0..args.len() {
        for j in (i + 1)..args.len() {
            let Some((common, lhs_rest, rhs_rest)) =
                remove_common_product_factors(&args[i], &args[j])
            else {
                continue;
            };
            let factored_pair = cas_mul(vec![
                cas_product(common),
                cas_add(vec![lhs_rest, rhs_rest])?,
            ])?;
            let mut new_args = Vec::with_capacity(args.len() - 1);
            for (idx, arg) in args.iter().enumerate() {
                if idx == i {
                    new_args.push(factored_pair.clone());
                } else if idx != j {
                    new_args.push(arg.clone());
                }
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

fn try_distribute_scaled_sum_for_like_term(args: &[Value]) -> WqResult<Option<Value>> {
    for (idx, arg) in args.iter().enumerate() {
        let (coeff, core) = split_add_term(arg);
        if numeric_is_one(&coeff) {
            continue;
        }
        let Some(core) = core else {
            continue;
        };
        let Some((CasOp::Add, inner_terms)) = core.cas_op_parts() else {
            continue;
        };
        let has_like_outer_term = inner_terms.iter().any(|inner| {
            args.iter()
                .enumerate()
                .any(|(other_idx, other)| idx != other_idx && terms_have_same_core(inner, other))
        });
        if !has_like_outer_term {
            continue;
        }

        let mut new_args = Vec::with_capacity(args.len() + inner_terms.len() - 1);
        for (arg_idx, original) in args.iter().enumerate() {
            if arg_idx == idx {
                for inner in inner_terms {
                    new_args.push(rebuild_scaled_term(coeff.clone(), Some(inner.clone()))?);
                }
            } else {
                new_args.push(original.clone());
            }
        }
        return Ok(Some(cas_add(new_args)?));
    }
    Ok(None)
}

fn terms_have_same_core(lhs: &Value, rhs: &Value) -> bool {
    let (_, lhs_core) = split_add_term(lhs);
    let (_, rhs_core) = split_add_term(rhs);
    lhs_core.is_some() && lhs_core == rhs_core
}

fn combine_logs_in_sum(args: &[Value]) -> WqResult<Option<Value>> {
    let mut other_terms = Vec::with_capacity(args.len());
    let mut log_args = Vec::new();
    for term in args {
        if let Some((CasFunction::Ln, [arg])) = term.cas_function_parts() {
            log_args.push(arg.clone());
        } else {
            other_terms.push(term.clone());
        }
    }
    if log_args.len() < 2 {
        return Ok(None);
    }
    other_terms.push(Value::from_cas_function(
        CasFunction::Ln,
        vec![cas_mul(log_args)?],
    ));
    Ok(Some(cas_add(other_terms)?))
}

fn rewrite_sgn_abs_product(args: &[Value]) -> WqResult<Option<Value>> {
    let mut sgn_arg = None;
    let mut abs_arg = None;
    let mut abs_power = None;
    for arg in args {
        if let Some((CasFunction::Sgn, [s])) = arg.cas_function_parts() {
            sgn_arg = Some(s.clone());
        } else if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts() {
            if let Some((CasFunction::Abs, [a])) = base.cas_function_parts() {
                abs_arg = Some(a.clone());
                abs_power = Some(exp.clone());
            }
        } else if let Some((CasFunction::Abs, [a])) = arg.cas_function_parts() {
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
                .cas_function_parts()
                .is_some_and(|(n, a2)| n == CasFunction::Sgn && a2.len() == 1 && a2[0] == *s);
        let is_abs = !removed_abs
            && arg
                .cas_function_parts()
                .is_some_and(|(n, a2)| n == CasFunction::Abs && a2.len() == 1 && a2[0] == *a);
        let is_abs_inv = !removed_abs
            && arg.cas_op_parts().is_some_and(|(op, a2)| {
                op == CasOp::Power
                    && a2.len() == 2
                    && a2[1].exact_int_is(-1)
                    && a2[0].cas_function_parts().is_some_and(|(n, a3)| {
                        n == CasFunction::Abs && a3.len() == 1 && a3[0] == *a
                    })
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
    // Quadratic a*x^2 + b*x + c with a > 0 and disc < 0 -> always > 0
    let Some((CasOp::Add, _)) = expr.cas_op_parts() else {
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
    // disc = b^2 - 4ac must be negative
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
    // The Algebraic value c0 + c1*alpha + ... has sign = sign(ck) when alpha > 0 and
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

#[derive(Clone)]
struct ShiftedCubicRootBase {
    leading: Value,
    monic_base: Value,
    shift: Value,
    constant: Value,
    var: String,
}

struct CubicRootTerm {
    scale: Value,
    numerator: Value,
    base: Value,
    exp: Value,
}

fn numeric_values_equal(lhs: &Value, rhs: &Value) -> bool {
    numeric_sub(lhs, rhs).is_ok_and(|diff| numeric_is_zero(&diff))
}

fn shifted_binomial_cubic_root_base(base: &Value) -> WqResult<Option<ShiftedCubicRootBase>> {
    let expanded = simplify_cas_value(&expand_expr(base)?)?;
    let mut found_var = None;
    if !collect_single_poly_var(&expanded, &mut found_var) {
        return Ok(None);
    }
    let Some(var) = found_var else {
        return Ok(None);
    };
    let coeffs = match poly_from_expr(&expanded, &var) {
        Ok(coeffs) if poly_degree(&coeffs) == 3 => coeffs,
        _ => return Ok(None),
    };

    let zero = Value::Int(0);
    let c0 = coeffs.first().cloned().unwrap_or_else(|| zero.clone());
    let c1 = coeffs.get(1).cloned().unwrap_or_else(|| zero.clone());
    let c2 = coeffs.get(2).cloned().unwrap_or_else(|| zero.clone());
    let c3 = coeffs.get(3).cloned().unwrap_or(zero);
    if numeric_is_zero(&c3) {
        return Ok(None);
    }

    let three = Value::Int(3);
    let twenty_seven = Value::Int(27);
    let c2_sq = numeric_mul(&c2, &c2)?;
    let three_c3 = numeric_mul(&three, &c3)?;
    let expected_c1 = match eval_exact_numeric_div(&c2_sq, &three_c3) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    if !numeric_values_equal(&c1, &expected_c1) {
        return Ok(None);
    }

    let c2_cubed = numeric_mul(&c2_sq, &c2)?;
    let c3_sq = numeric_mul(&c3, &c3)?;
    let denom = numeric_mul(&twenty_seven, &c3_sq)?;
    let shifted_constant = match eval_exact_numeric_div(&c2_cubed, &denom) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let q = numeric_sub(&c0, &shifted_constant)?;
    let constant = match eval_exact_numeric_div(&q, &c3) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let shift = match eval_exact_numeric_div(&c2, &three_c3) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };

    let x = Value::from_cas_var(var.clone());
    let u = if numeric_is_zero(&shift) {
        x
    } else {
        cas_add(vec![x, shift.clone()])?
    };
    let monic_base = cas_add(vec![cas_pow(u, Value::Int(3))?, constant.clone()])?;

    Ok(Some(ShiftedCubicRootBase {
        leading: c3,
        monic_base,
        shift,
        constant,
        var,
    }))
}

fn push_power_factor(
    powers: &mut Vec<(Value, Value)>,
    base: Value,
    exp: Value,
) -> WqResult<()> {
    for (existing_base, existing_exp) in powers.iter_mut() {
        if *existing_base == base {
            *existing_exp = numeric_add(existing_exp, &exp)?;
            return Ok(());
        }
    }
    powers.push((base, exp));
    Ok(())
}

fn reduce_rational_power_base(base: &Value, exp: &Value) -> Option<(Value, Value)> {
    let (bn, bd) = base.rational_parts()?;
    let (en, ed) = exp.rational_parts()?;
    if ed.is_one() {
        return None;
    }
    let max_q = ed.to_u32()?.min(12);
    for q in (2..=max_q).rev() {
        let Some(root_n) = perfect_power_root(&bn, q) else {
            continue;
        };
        let Some(root_d) = perfect_power_root(&bd, q) else {
            continue;
        };
        let root = Value::from_fraction_parts(root_n, root_d);
        if root == *base {
            continue;
        }
        let new_exp = Value::from_fraction_parts(en.clone() * BigInt::from(q), ed.clone());
        return Some((root, new_exp));
    }
    None
}

fn simplify_var_free_product(factors: Vec<Value>) -> WqResult<Value> {
    let mut numeric = Value::Int(1);
    let mut powers = Vec::new();
    let mut pending = factors;
    while let Some(factor) = pending.pop() {
        let factor = simplify_cas_value(&factor)?;
        if numeric_is_one(&factor) {
            continue;
        }
        if let Some((CasOp::Multiply, args)) = factor.cas_op_parts()
            && !contains_symbolic_var(&factor)
        {
            pending.extend(args.iter().cloned());
            continue;
        }
        if !factor.is_cas_expr() {
            numeric = numeric_mul(&numeric, &factor)?;
            continue;
        }
        if contains_symbolic_var(&factor) {
            return cas_mul(vec![numeric, factor]);
        }
        if let Some((CasOp::Power, [base, exp])) = factor.cas_op_parts()
            && exp.rational_parts().is_some()
            && !contains_symbolic_var(base)
        {
            let (base, exp) = reduce_rational_power_base(base, exp)
                .unwrap_or_else(|| (base.clone(), exp.clone()));
            push_power_factor(&mut powers, base, exp)?;
        } else {
            push_power_factor(&mut powers, factor, Value::Int(1))?;
        }
    }

    let mut out = Vec::new();
    for (base, exp) in powers {
        if numeric_is_zero(&exp) {
            continue;
        }
        let powered = pow_constant_factor(&base, &exp)?;
        if !powered.is_cas_expr() {
            numeric = numeric_mul(&numeric, &powered)?;
        } else {
            out.push(powered);
        }
    }
    if !numeric_is_one(&numeric) || out.is_empty() {
        out.push(numeric);
    }
    cas_mul(out)
}

fn display_half_power_parts(base: &Value, exp: &Value) -> WqResult<(Value, Value)> {
    if let Some((display_base, scale)) = integer_affine_cubic_display(base)? {
        let scale_exp = numeric_mul(exp, &Value::Int(-1))?;
        Ok((pow_constant_factor(&scale, &scale_exp)?, display_base))
    } else {
        Ok((Value::Int(1), base.clone()))
    }
}

fn normalize_cubic_root_term(term: &Value) -> WqResult<Option<CubicRootTerm>> {
    let (coeff, core) = split_add_term(term);
    let Some(core) = core else {
        return Ok(None);
    };

    let mut root: Option<(ShiftedCubicRootBase, Value)> = None;
    let mut scale_factors = vec![coeff];
    let mut numerator_factors = Vec::new();
    for factor in product_factors(&core) {
        if root.is_none()
            && let Some((CasOp::Power, [base, exp])) = factor.cas_op_parts()
            && (exp.exact_half() || exp.exact_neg_half())
            && let Some(root_base) = shifted_binomial_cubic_root_base(base)?
        {
            root = Some((root_base, exp.clone()));
            continue;
        }
        if contains_symbolic_var(&factor) {
            numerator_factors.push(factor);
        } else {
            scale_factors.push(factor);
        }
    }
    let Some((root_base, exp)) = root else {
        return Ok(None);
    };
    scale_factors.push(pow_constant_factor(&root_base.leading, &exp)?);
    let scale = simplify_var_free_product(scale_factors)?;
    let numerator = simplify_cas_value(&cas_product(numerator_factors))?;

    Ok(Some(CubicRootTerm {
        scale,
        numerator,
        base: root_base.monic_base,
        exp,
    }))
}

fn constant_poly_quotient(numer: &Value, denom: &Value) -> WqResult<Option<Value>> {
    let mut found_var = None;
    if !collect_single_poly_var(denom, &mut found_var) {
        return Ok(None);
    }
    let Some(var) = found_var else {
        return Ok(None);
    };
    let numer = simplify_cas_value(&expand_expr(numer)?)?;
    let denom = simplify_cas_value(&expand_expr(denom)?)?;
    let n_poly = match poly_from_expr(&numer, &var) {
        Ok(poly) => poly,
        Err(_) => return Ok(None),
    };
    let d_poly = match poly_from_expr(&denom, &var) {
        Ok(poly) => poly,
        Err(_) => return Ok(None),
    };
    let (quotient, remainder) = poly_divide(&n_poly, &d_poly)?;
    if !poly_is_zero(&remainder) || poly_degree(&quotient) != 0 {
        return Ok(None);
    }
    Ok(quotient.first().cloned())
}

fn integer_affine_cubic_display(base: &Value) -> WqResult<Option<(Value, Value)>> {
    let Some(parts) = shifted_binomial_cubic_root_base(base)? else {
        return Ok(None);
    };
    if !numeric_is_one(&parts.leading) {
        return Ok(None);
    }
    let Some((_, shift_denom)) = parts.shift.rational_parts() else {
        return Ok(None);
    };
    if shift_denom.is_one() {
        return Ok(None);
    }

    let divisor = shift_denom.pow(3);
    let scale = Value::from_bigint(divisor);
    let d = Value::from_bigint(shift_denom);
    let x = Value::from_cas_var(parts.var);
    let affine_x = cas_mul(vec![d.clone(), x])?;
    let affine_const = numeric_mul(&d, &parts.shift)?;
    let affine = cas_add(vec![affine_x, affine_const])?;
    let display_const = numeric_mul(&scale, &parts.constant)?;
    let display_base = cas_add(vec![cas_pow(affine, Value::Int(3))?, display_const])?;

    Ok(Some((display_base, scale)))
}

fn display_scaled_half_power(base: &Value, exp: &Value) -> WqResult<Value> {
    let (scale, display_base) = display_half_power_parts(base, exp)?;
    let root = cas_mul(vec![scale, cas_pow(display_base, exp.clone())?])?;
    simplify_cas_value(&root)
}

fn try_collapse_cubic_root_sum(args: &[Value]) -> WqResult<Option<Value>> {
    if args.len() < 2 {
        return Ok(None);
    }

    let mut terms = Vec::with_capacity(args.len());
    for arg in args {
        let Some(term) = normalize_cubic_root_term(arg)? else {
            return Ok(None);
        };
        if !term.exp.exact_half() && !term.exp.exact_neg_half() {
            return Ok(None);
        }
        terms.push(term);
    }

    let base = terms[0].base.clone();
    if terms.iter().any(|term| term.base != base) {
        return Ok(None);
    }

    let mut half_terms = Vec::new();
    let mut inv_half_terms = Vec::new();
    for term in terms {
        let scaled = cas_mul(vec![term.scale, term.numerator])?;
        if term.exp.exact_half() {
            half_terms.push(scaled);
        } else {
            inv_half_terms.push(scaled);
        }
    }
    if half_terms.is_empty() || inv_half_terms.is_empty() {
        return Ok(None);
    }

    let half_numer = cas_add(half_terms)?;
    let inv_half_numer = cas_add(inv_half_terms)?;
    let combined_numer = cas_add(vec![cas_mul(vec![half_numer, base.clone()])?, inv_half_numer])?;
    let Some(quotient) = constant_poly_quotient(&combined_numer, &base)? else {
        return Ok(None);
    };
    let root = display_scaled_half_power(
        &base,
        &Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )?;
    Ok(Some(cas_mul(vec![quotient, root])?))
}

fn try_normalize_standalone_cubic_root_product(
    value: &Value,
    args: &[Value],
) -> WqResult<Option<Value>> {
    if args
        .iter()
        .any(|arg| !arg.is_cas_expr() && !numeric_is_one(arg))
    {
        return Ok(None);
    }
    let Some(term) = normalize_cubic_root_term(value)? else {
        return Ok(None);
    };
    if !numeric_is_one(&term.numerator) {
        return Ok(None);
    }
    let (display_scale, display_base) = display_half_power_parts(&term.base, &term.exp)?;
    let scale = simplify_var_free_product(vec![term.scale, display_scale])?;
    let normalized = cas_mul(vec![scale, cas_pow(display_base, term.exp)?])?;
    if normalized == *value {
        Ok(None)
    } else {
        Ok(Some(normalized))
    }
}

/// (+ (* A B) (* (* -1 A) C)) -> (* A (+ B (* -1 C))).
/// Also handles non-unit negative coefficients for function/app common factors,
/// such as exponential terms produced by differentiation.
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
        if numeric_is_negative(&coeff_pos) || !numeric_is_negative(&coeff_neg) {
            continue;
        }
        let coeff_neg_is_unit = coeff_neg.exact_int_is(-1) || exact_int_value_is(&coeff_neg, -1);
        // Both cores should be products sharing a common factor
        let factors_pos: Vec<Value> = match core_pos.cas_op_parts() {
            Some((CasOp::Multiply, f)) => f.to_vec(),
            _ => vec![core_pos.clone()],
        };
        let factors_neg: Vec<Value> = match &core_neg {
            Some(c) if matches!(c.cas_op_parts(), Some((CasOp::Multiply, _))) => {
                if let Some((CasOp::Multiply, f)) = c.cas_op_parts() {
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
                if !coeff_neg_is_unit && !allows_non_unit_negative_factor(&common) {
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
                let term_neg = rebuild_scaled_term(coeff_neg.clone(), new_core_neg)?;
                let inner = cas_add(vec![term_pos, term_neg])?;
                let inner = if coeff_neg_is_unit {
                    inner
                } else {
                    simplify_cas_value(&expand_expr(&inner)?)?
                };
                return Ok(Some(cas_mul(vec![common, inner])?));
            }
        }
    }
    Ok(None)
}

fn allows_non_unit_negative_factor(common: &Value) -> bool {
    common.cas_function_parts().is_some()
        || common.cas_apply_parts().is_some()
        || matches!(
            common.cas_op_parts(),
            Some((CasOp::Power, [base, exp]))
                if base.cas_const() == Some(CasConst::E) && exp.is_cas_expr()
        )
}

/// (* ... (^ D1 -1) ... (^ D2 -1) ...) -> replace both with (^ (D1*D2) -1)
/// when D1*D2 expands to something simpler.
/// Only applies when both D1, D2 are simple sums (<= 3 terms) to avoid large
/// expansions.
fn try_combine_inv_denoms(args: &[Value]) -> WqResult<Option<Value>> {
    let mut inv_info = Vec::new();
    for (i, arg) in args.iter().enumerate() {
        if let Some((CasOp::Power, [base, e])) = arg.cas_op_parts()
            && e.exact_int_is(-1)
            && let Some((CasOp::Add, sum_terms)) = base.cas_op_parts()
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
            let product = if let Some((CasOp::Add, sum_args)) = product.cas_op_parts()
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
            let combined = Value::from_cas_op(CasOp::Power, vec![product, Value::Int(-1)]);
            let mut new_args: Vec<Value> = Vec::with_capacity(args.len() - 1);
            for (k, arg) in args.iter().enumerate() {
                if k == i || k == j {
                    continue;
                }
                new_args.push(arg.clone());
            }
            new_args.push(combined);
            return Ok(Some(simplify_cas_value(&cas_product(new_args))?));
        }
    }
    Ok(None)
}

fn negative_integer_power(exp: &Value) -> Option<Value> {
    let power = exp.exact_int()?;
    if !power.is_negative() {
        return None;
    }
    Some(Value::from_bigint(-power))
}

fn single_poly_degree(expr: &Value) -> Option<usize> {
    let mut var = None;
    if !collect_single_poly_var(expr, &mut var) {
        return None;
    }
    let var = var?;
    let coeffs = poly_from_expr(expr, &var).ok()?;
    Some(poly_degree(&coeffs))
}

fn product_factors(value: &Value) -> Vec<Value> {
    if let Some((CasOp::Multiply, args)) = value.cas_op_parts() {
        args.to_vec()
    } else {
        vec![value.clone()]
    }
}

fn split_fraction_product(value: &Value) -> WqResult<Option<(Value, Value)>> {
    let mut numerator = Vec::new();
    let mut denominator = Vec::new();
    for factor in product_factors(value) {
        if let Some((CasOp::Power, [base, exp])) = factor.cas_op_parts()
            && let Some(abs_power) = negative_integer_power(exp)
        {
            denominator.push(cas_pow(base.clone(), abs_power)?);
        } else {
            numerator.push(factor);
        }
    }
    if denominator.is_empty() {
        return Ok(None);
    }
    Ok(Some((cas_product(numerator), cas_mul(denominator)?)))
}

fn product_negative_integer_denominator(
    args: &[Value],
    skip: usize,
) -> WqResult<Option<(Value, Vec<usize>)>> {
    let mut denominator = Vec::new();
    let mut indices = Vec::new();
    for (idx, arg) in args.iter().enumerate() {
        if idx == skip {
            continue;
        }
        if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && let Some(abs_power) = negative_integer_power(exp)
        {
            denominator.push(cas_pow(base.clone(), abs_power)?);
            indices.push(idx);
        }
    }
    if denominator.is_empty() {
        return Ok(None);
    }
    Ok(Some((cas_mul(denominator)?, indices)))
}

fn expand_equivalent(lhs: &Value, rhs: &Value) -> WqResult<bool> {
    let lhs = simplify_cas_value(&expand_expr(lhs)?)?;
    let rhs = simplify_cas_value(&expand_expr(rhs)?)?;
    Ok(lhs == rhs || lhs.to_string() == rhs.to_string())
}

fn try_cancel_inverse_sqrt_denominator(args: &[Value]) -> WqResult<Option<Value>> {
    for (sqrt_idx, factor) in args.iter().enumerate() {
        let Some((CasOp::Power, [base, exp])) = factor.cas_op_parts() else {
            continue;
        };
        if !exp.exact_neg_half() {
            continue;
        }
        let Some((numerator, inner_denominator)) = split_fraction_product(base)? else {
            continue;
        };
        let Some((outer_denominator, outer_indices)) =
            product_negative_integer_denominator(args, sqrt_idx)?
        else {
            continue;
        };
        let outer_square = cas_pow(outer_denominator, Value::Int(2))?;
        if !expand_equivalent(&outer_square, &inner_denominator)? {
            continue;
        }

        let numerator = factor_expr(&simplify_cas_value(&expand_expr(&numerator)?)?)?;
        if single_poly_degree(&numerator) != Some(3) {
            continue;
        }
        let replacement = cas_pow(numerator, exp.clone())?;
        let mut new_args = Vec::with_capacity(args.len() - outer_indices.len());
        for (idx, arg) in args.iter().enumerate() {
            if idx == sqrt_idx {
                new_args.push(replacement.clone());
            } else if !outer_indices.contains(&idx) {
                new_args.push(arg.clone());
            }
        }
        return Ok(Some(cas_mul(new_args)?));
    }
    Ok(None)
}

fn perfect_power_root(n: &BigInt, q: u32) -> Option<BigInt> {
    if n.is_zero() || n.is_one() {
        return Some(n.clone());
    }
    if n.is_negative() {
        if q.is_multiple_of(2) {
            return None;
        }
        return perfect_power_root(&(-n), q).map(|root| -root);
    }
    let (root, rem) = extract_perfect_power_factor(n, q);
    if rem.is_one() { Some(root) } else { None }
}

fn exact_rational_fractional_power(base: &Value, exp: &Value) -> Option<Value> {
    let (bn, bd) = base.rational_parts()?;
    let (en, ed) = exp.rational_parts()?;
    if ed.is_one() {
        return None;
    }
    let q = ed.to_u32()?;
    let root_n = perfect_power_root(&bn, q)?;
    let root_d = perfect_power_root(&bd, q)?;
    let power = en.abs().to_u32()?;
    let numer = root_n.pow(power);
    let denom = root_d.pow(power);
    if en.is_negative() {
        if numer.is_zero() {
            return None;
        }
        Some(Value::from_fraction_parts(denom, numer))
    } else {
        Some(Value::from_fraction_parts(numer, denom))
    }
}

fn pow_constant_factor(factor: &Value, exp: &Value) -> WqResult<Value> {
    if let Some(exact) = exact_rational_fractional_power(factor, exp) {
        return Ok(exact);
    }
    if let Some((CasOp::Power, [base, inner_exp])) = factor.cas_op_parts()
        && !contains_symbolic_var(base)
        && inner_exp.rational_parts().is_some()
        && exp.rational_parts().is_some()
    {
        return cas_pow(base.clone(), eval_numeric_binary("*", inner_exp, exp)?);
    }
    cas_pow(factor.clone(), exp.clone())
}

fn try_split_var_free_product_power(base: &Value, exp: &Value) -> WqResult<Option<Value>> {
    if !exp.exact_half() && !exp.exact_neg_half() {
        return Ok(None);
    }
    let Some((CasOp::Multiply, args)) = base.cas_op_parts() else {
        return Ok(None);
    };

    let mut factors = Vec::with_capacity(args.len());
    let mut symbolic = Vec::new();
    let mut split_any = false;
    for arg in args {
        if contains_symbolic_var(arg) {
            symbolic.push(arg.clone());
        } else {
            split_any = true;
            factors.push(pow_constant_factor(arg, exp)?);
        }
    }
    if !split_any {
        return Ok(None);
    }
    if symbolic.is_empty() {
        return Ok(None);
    }
    let symbolic_base = cas_mul(symbolic)?;
    if single_poly_degree(&symbolic_base) != Some(3) {
        return Ok(None);
    }
    factors.push(cas_pow(symbolic_base, exp.clone())?);
    Ok(Some(cas_mul(factors)?))
}

fn apply_tree_rewrite(value: &Value) -> WqResult<Option<Value>> {
    if let Some((CasOp::Add, args)) = value.cas_op_parts() {
        if let Some(result) = try_combine_unit_with_fraction_sum(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_combine_var_free_denominator_sum(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_collapse_cubic_root_sum(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_factor_var_free_binary_sum(value, args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_distribute_scaled_sum_for_like_term(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_merge_var_free_sum_pair(args)? {
            return Ok(Some(result));
        }
        if let Some(result) = try_factor_common_sum_pair(args)? {
            return Ok(Some(result));
        }
        // Factor common product from sum: (+ (* A B) (* (* -1 A) C)) -> (* A (+ B (* -1
        // C)))
        if let Some(result) = try_factor_binary_product(args)? {
            return Ok(Some(result));
        }
        return combine_logs_in_sum(args);
    }

    // Distribute -1 over sum: (* -1 (+ a b ...)) -> (+ (* -1 a) (* -1 b) ...)
    if let Some((CasOp::Multiply, [a, b])) = value.cas_op_parts() {
        if let (Some((CasOp::Add, sum_args)), true) = (b.cas_op_parts(), a.exact_int_is(-1)) {
            let new_args: Vec<Value> = sum_args
                .iter()
                .map(|arg| cas_neg(arg.clone()))
                .collect::<WqResult<_>>()?;
            return Ok(Some(cas_add(new_args)?));
        }
        if let (Some((CasOp::Add, sum_args)), true) = (a.cas_op_parts(), b.exact_int_is(-1)) {
            let new_args: Vec<Value> = sum_args
                .iter()
                .map(|arg| cas_neg(arg.clone()))
                .collect::<WqResult<_>>()?;
            return Ok(Some(cas_add(new_args)?));
        }
    }

    if let Some((CasOp::Multiply, args)) = value.cas_op_parts()
        && let Some(result) = try_normalize_standalone_cubic_root_product(value, args)?
    {
        return Ok(Some(result));
    }

    if let Some((CasOp::Multiply, args)) = value.cas_op_parts()
        && let Some(result) = try_cancel_affine_over_product(args)?
    {
        return Ok(Some(result));
    }

    if let Some((CasOp::Multiply, args)) = value.cas_op_parts()
        && let Some(result) = try_cancel_inverse_sqrt_denominator(args)?
    {
        return Ok(Some(result));
    }

    if let Some((CasOp::Multiply, args)) = value.cas_op_parts()
        && let Some(result) = rewrite_sgn_abs_product(args)?
    {
        return Ok(Some(result));
    }

    // Combine (^ D1 -1) * (^ D2 -1) -> (^ (D1*D2) -1) when D1*D2 expands usefully
    if let Some((CasOp::Multiply, args)) = value.cas_op_parts()
        && let Some(result) = try_combine_inv_denoms(args)?
    {
        return Ok(Some(result));
    }

    if let Some((CasOp::Power, [base, exp])) = value.cas_op_parts() {
        if let Some(result) = try_split_var_free_product_power(base, exp)? {
            return Ok(Some(result));
        }
        if exp.exact_half()
            && let Some((CasOp::Power, [inner_base, inner_exp])) = base.cas_op_parts()
            && inner_exp.exact_int_is(2)
        {
            return Ok(Some(Value::from_cas_function(
                CasFunction::Abs,
                vec![inner_base.clone()],
            )));
        }
        if exp.exact_int_is(2)
            && let Some((CasFunction::Abs, [arg])) = base.cas_function_parts()
        {
            return Ok(Some(cas_pow(arg.clone(), Value::Int(2))?));
        }
    }

    let Some((name, args)) = value.cas_function_parts() else {
        return Ok(None);
    };
    let rewritten = match (name, args) {
        (CasFunction::Ln, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::Exp, [_]))) =>
        {
            let Some((_, [inner])) = arg.cas_function_parts() else {
                unreachable!("matched exp call")
            };
            Some(inner.clone())
        }
        (CasFunction::Exp, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::Ln, [_]))) =>
        {
            let Some((_, [inner])) = arg.cas_function_parts() else {
                unreachable!("matched ln call")
            };
            Some(inner.clone())
        }
        (CasFunction::Ln, [arg]) if matches!(arg.cas_op_parts(), Some((CasOp::Power, [_, _]))) => {
            let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts() else {
                unreachable!("matched power")
            };
            Some(cas_mul(vec![
                exp.clone(),
                Value::from_cas_function(CasFunction::Ln, vec![base.clone()]),
            ])?)
        }
        (CasFunction::Sin, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::ArcSin, [_]))) =>
        {
            // sin(arcsin(t)) = t
            let Some((CasFunction::ArcSin, [inner])) = arg.cas_function_parts() else {
                unreachable!()
            };
            Some(inner.clone())
        }
        (CasFunction::Cos, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::ArcCos, [_]))) =>
        {
            // cos(arccos(t)) = t
            let Some((CasFunction::ArcCos, [inner])) = arg.cas_function_parts() else {
                unreachable!()
            };
            Some(inner.clone())
        }
        (CasFunction::Tan, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::ArcTan, [_]))) =>
        {
            // tan(arctan(t)) = t
            let Some((CasFunction::ArcTan, [inner])) = arg.cas_function_parts() else {
                unreachable!()
            };
            Some(inner.clone())
        }
        (CasFunction::Sin, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::ArcCos, [_]))) =>
        {
            // sin(arccos(t)) = sqrt(1 - t^2)
            let Some((CasFunction::ArcCos, [inner])) = arg.cas_function_parts() else {
                unreachable!()
            };
            Some(cas_pow(
                cas_sub(Value::Int(1), cas_pow(inner.clone(), Value::Int(2))?)?,
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            )?)
        }
        (CasFunction::Cos, [arg])
            if matches!(arg.cas_function_parts(), Some((CasFunction::ArcSin, [_]))) =>
        {
            // cos(arcsin(t)) = sqrt(1 - t^2)
            let Some((CasFunction::ArcSin, [inner])) = arg.cas_function_parts() else {
                unreachable!()
            };
            Some(cas_pow(
                cas_sub(Value::Int(1), cas_pow(inner.clone(), Value::Int(2))?)?,
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            )?)
        }
        (CasFunction::Sin, [arg]) => {
            if let Some(inner) = extract_unit_negative(arg) {
                Some(cas_neg(Value::from_cas_function(
                    CasFunction::Sin,
                    vec![inner],
                ))?)
            } else if let Some(shifted) = take_additive_constant(arg, std::f64::consts::FRAC_PI_2) {
                Some(Value::from_cas_function(CasFunction::Cos, vec![shifted]))
            } else {
                let (coeff, core) = split_add_term(arg);
                if coeff.exact_int_is(2) {
                    core.map(|inner| {
                        cas_mul(vec![
                            Value::Int(2),
                            Value::from_cas_function(CasFunction::Sin, vec![inner.clone()]),
                            Value::from_cas_function(CasFunction::Cos, vec![inner]),
                        ])
                    })
                    .transpose()?
                } else {
                    None
                }
            }
        }
        (CasFunction::Cos, [arg]) => extract_unit_negative(arg)
            .map(|inner| Value::from_cas_function(CasFunction::Cos, vec![inner])),
        (CasFunction::Abs, [arg]) => {
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
    } else if let Some((name, args)) = value.cas_function_parts() {
        let mut rewritten_args = Vec::with_capacity(args.len());
        for arg in args {
            rewritten_args.push(rewrite_expr(arg)?);
        }
        simplify_cas_value(&Value::from_cas_function(name, rewritten_args))?
    } else if let Some((name, args)) = value.cas_apply_parts() {
        let mut rewritten_args = Vec::with_capacity(args.len());
        for arg in args {
            rewritten_args.push(rewrite_expr(arg)?);
        }
        simplify_cas_value(&Value::from_cas_apply(name.as_str(), rewritten_args))?
    } else {
        value.clone()
    };

    match apply_tree_rewrite(&rewritten)? {
        Some(next) => simplify_cas_value(&next),
        None => Ok(rewritten),
    }
}

pub(crate) fn rewrite_cas(expr: &Value) -> WqResult<Value> {
    with_cas_div_cache(|| {
        let mut current = simplify_cas_value(expr)?;
        rewrite_loop(&mut current)?;
        if let Some(next) = rewrite_with_egg(&current)? {
            current = next;
            rewrite_loop(&mut current)?;
        }
        Ok(current)
    })
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
        cas_trace!(
            DebugLogFlags::CAS_VERBOSE,
            "[cas-v] rewrite_loop iteration={i} -> {}",
            next.format_cas().unwrap_or_else(|| next.to_string())
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
    if let Some((_, args)) = expr.cas_function_parts() {
        return args.iter().any(|a| contains_cas_var(a, var));
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        return args.iter().any(|a| contains_cas_var(a, var));
    }
    if let Some((inner, limit_var, point, _)) = expr.cas_limit_parts() {
        return contains_cas_var(inner, var)
            || contains_cas_var(limit_var, var)
            || contains_cas_var(point, var);
    }
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        return contains_cas_var(lhs, var) || contains_cas_var(rhs, var);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_binary_product_handles_non_unit_exp_common_factor() {
        let x = Value::from_cas_var("x");
        let x2 = cas_pow(x.clone(), Value::Int(2)).expect("x^2");
        let x3 = cas_pow(x.clone(), Value::Int(3)).expect("x^3");
        let exp = Value::from_cas_function(
            CasFunction::Exp,
            vec![cas_div(x3, Value::Int(3)).expect("x^3/3")],
        );
        let term1 = cas_mul(vec![
            exp.clone(),
            x2.clone(),
            cas_add(vec![x2, Value::Int(1)]).expect("x^2 + 1"),
        ])
        .expect("term1");
        let term2 = cas_mul(vec![Value::Int(-2), exp.clone(), x]).expect("term2");
        let simplified_exp = simplify_cas_value(&exp).expect("simplified exp");

        let result = try_factor_binary_product(&[term1, term2])
            .expect("factor")
            .expect("expected factored result");
        let Some((CasOp::Multiply, factors)) = result.cas_op_parts() else {
            panic!("expected product, got {result}");
        };
        assert!(
            factors.iter().any(|factor| factor == &simplified_exp),
            "expected exp common factor in {result}"
        );
        let text = result.format_cas().unwrap_or_else(|| result.to_string());
        assert!(
            text.contains("x^4 + x^2 - 2*x"),
            "expected expanded inner sum in {text}"
        );
    }

    #[test]
    fn factor_binary_product_skips_non_unit_plain_var_common_factor() {
        let x = Value::from_cas_var("x");
        let term1 =
            cas_mul(vec![Value::Int(2), x.clone(), Value::from_cas_var("y")]).expect("term1");
        let term2 = cas_mul(vec![Value::Int(-3), x, Value::from_cas_var("z")]).expect("term2");

        let result = try_factor_binary_product(&[term1, term2]).expect("factor");
        assert!(
            result.is_none(),
            "plain variable common factor should stay conservative: {result:?}"
        );
    }

    #[test]
    fn rewrite_distributes_scaled_sum_to_combine_like_term() {
        let x = Value::from_cas_var("x");
        let x2 = cas_pow(x, Value::Int(2)).expect("x^2");
        let scaled_sum = cas_div(
            cas_add(vec![x2.clone(), Value::Int(1)]).expect("x^2 + 1"),
            Value::Int(2),
        )
        .expect("(x^2 + 1)/2");
        let expr = cas_add(vec![scaled_sum, cas_neg(x2).expect("-x^2")]).expect("sum");

        let result = rewrite_cas(&expr).expect("rewrite");
        let text = result.format_cas().unwrap_or_else(|| result.to_string());
        assert_eq!(text, "-x^2/2 + 1/2");
    }
}
