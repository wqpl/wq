use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::{
    CasDebug, cas_add, cas_internal_err, cas_mul, cas_pow, eval_exact_numeric_div, numeric_add,
    numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul, numeric_sub,
    simplify_cas_value, split_mul_factor,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp, CasSymbol};
use crate::value::{Value, WqResult};

fn expand_mul_into_terms(terms: Vec<Value>, factor: Value) -> WqResult<Vec<Value>> {
    let factor_terms = if let Some(args) = factor.cas_op_args(CasOp::Add) {
        args.to_vec()
    } else {
        vec![factor]
    };
    let next_cap = terms.len().saturating_mul(factor_terms.len());
    let mut next = Vec::with_capacity(next_cap);
    for term in terms {
        for factor_term in &factor_terms {
            next.push(cas_mul(vec![term.clone(), factor_term.clone()])?);
        }
    }
    Ok(next)
}

/// Stack frame for iterative expand.
enum ExpandFrame {
    /// Node that still needs its children processed.
    Expr(Value),
    /// Combine the top `n` results into a `+` node.
    Add(usize),
    /// Combine the top `n` results into a `*` expansion.
    Mul(usize),
    /// Combine the top `n` results into a `*` without expanding `+` nodes.
    /// Used by negative-power distribution so that (a*b)^(-1) stays
    /// a^(-1)*b^(-1) rather than being fully expanded into a sum.
    MulNoExpand(usize),
    /// Combine the top result (expanded base) with the saved exponent.
    Pow { exp: Value, power: Option<usize> },
    /// Reassemble a builtin-function call from the top `n` results.
    Function { function: CasFunction, n: usize },
    /// Re-assemble an uninterpreted symbolic application from the top `n`
    /// results.
    Apply { name: CasSymbol, n: usize },
    /// Re-assemble an equation from the top 2 results.
    Eq,
}

pub(super) fn expand_expr(expr: &Value) -> WqResult<Value> {
    // Fast path: atom or non-CAS value.
    if !expr.is_cas_expr() || expr.cas_var_name().is_some() {
        return Ok(expr.clone());
    }

    let mut stack = vec![ExpandFrame::Expr(expr.clone())];
    let mut results: Vec<Value> = Vec::new();

    while let Some(frame) = stack.pop() {
        match frame {
            ExpandFrame::Expr(expr) => {
                if !expr.is_cas_expr() || expr.cas_var_name().is_some() {
                    results.push(expr);
                    continue;
                }

                if let Some((lhs, rhs)) = expr.cas_eq_parts() {
                    stack.push(ExpandFrame::Eq);
                    stack.push(ExpandFrame::Expr(rhs.clone()));
                    stack.push(ExpandFrame::Expr(lhs.clone()));
                    continue;
                }

                if let Some((op, args)) = expr.cas_known_op_parts() {
                    match (op, args) {
                        (CasOp::Add, args) => {
                            let n = args.len();
                            if n == 0 {
                                results.push(cas_add(Vec::new())?);
                                continue;
                            }
                            stack.push(ExpandFrame::Add(n));
                            for arg in args.iter().rev() {
                                stack.push(ExpandFrame::Expr(arg.clone()));
                            }
                        }
                        (CasOp::Multiply, args) => {
                            let n = args.len();
                            if n == 0 {
                                results.push(cas_mul(vec![Value::Int(1)])?);
                                continue;
                            }
                            stack.push(ExpandFrame::Mul(n));
                            for arg in args.iter().rev() {
                                stack.push(ExpandFrame::Expr(arg.clone()));
                            }
                        }
                        (CasOp::Power, [base, exp]) => {
                            match exp.exact_int().and_then(|n| n.to_usize()) {
                                Some(0) => {
                                    results.push(Value::Int(1));
                                }
                                Some(power) => {
                                    stack.push(ExpandFrame::Pow {
                                        exp: exp.clone(),
                                        power: Some(power),
                                    });
                                    stack.push(ExpandFrame::Expr(base.clone()));
                                }
                                None => {
                                    // Negative-power distribution must happen BEFORE
                                    // base is expanded, otherwise a product like
                                    // (_s*(_t^2-1)*(-2*_t)^-1)^-1 gets expanded into
                                    // a sum first and the * check below fails.
                                    if let Some(k) = exp.exact_int()
                                        && k < BigInt::zero()
                                        && let Some(base_args) = base.cas_op_args(CasOp::Multiply)
                                    {
                                        let n = base_args.len();
                                        stack.push(ExpandFrame::MulNoExpand(n));
                                        for arg in base_args.iter().rev() {
                                            stack.push(ExpandFrame::Expr(cas_pow(
                                                arg.clone(),
                                                exp.clone(),
                                            )?));
                                        }
                                    } else {
                                        stack.push(ExpandFrame::Pow {
                                            exp: exp.clone(),
                                            power: None,
                                        });
                                        stack.push(ExpandFrame::Expr(base.clone()));
                                    }
                                }
                            }
                        }
                        _ => {
                            // Unknown op: do not expand children.
                            results.push(expr.clone());
                        }
                    }
                    continue;
                }

                if let Some((function, args)) = expr.cas_function_parts() {
                    let n = args.len();
                    stack.push(ExpandFrame::Function { function, n });
                    for arg in args.iter().rev() {
                        stack.push(ExpandFrame::Expr(arg.clone()));
                    }
                    continue;
                }

                if let Some((name, args)) = expr.cas_apply_parts() {
                    let n = args.len();
                    stack.push(ExpandFrame::Apply {
                        name: name.clone(),
                        n,
                    });
                    for arg in args.iter().rev() {
                        stack.push(ExpandFrame::Expr(arg.clone()));
                    }
                    continue;
                }

                results.push(expr.clone());
            }
            ExpandFrame::Add(n) => {
                let children = split_off_results(&mut results, n)?;
                results.push(cas_add(children)?);
            }
            ExpandFrame::Mul(n) => {
                let children = split_off_results(&mut results, n)?;
                let mut terms = vec![Value::Int(1)];
                for arg in children {
                    terms = expand_mul_into_terms(terms, arg)?;
                }
                let result = cas_add(terms)?;

                results.push(result);
            }
            ExpandFrame::MulNoExpand(n) => {
                let children = split_off_results(&mut results, n)?;
                let result = cas_mul(children)?;

                results.push(result);
            }
            ExpandFrame::Pow { exp, power } => {
                let base = results
                    .pop()
                    .ok_or_else(|| cas_internal_err("expanding a symbolic expression"))?;
                match power {
                    Some(power) => {
                        let mut terms = vec![Value::Int(1)];
                        for _ in 0..power {
                            terms = expand_mul_into_terms(terms, base.clone())?;
                        }
                        results.push(cas_add(terms)?);
                    }
                    None => {
                        // Distribute negative integer power across product:
                        // (a*b)^(-k) -> a^(-k) * b^(-k).
                        // Re-queue each factor as an Expr so nested products
                        // like ((c*d)^(-1)*e)^(-1) also get expanded.
                        if let Some(k) = exp.exact_int()
                            && k < BigInt::zero()
                            && let Some(base_args) = base.cas_op_args(CasOp::Multiply)
                        {
                            let n = base_args.len();
                            stack.push(ExpandFrame::MulNoExpand(n));
                            for arg in base_args.iter().rev() {
                                stack.push(ExpandFrame::Expr(cas_pow(arg.clone(), exp.clone())?));
                            }
                        } else {
                            results.push(cas_pow(base, simplify_cas_value(&exp)?)?);
                        }
                    }
                }
            }
            ExpandFrame::Function { function, n } => {
                let args = split_off_results(&mut results, n)?;
                results.push(simplify_cas_value(&Value::from_cas_function(
                    function, args,
                ))?);
            }
            ExpandFrame::Apply { name, n } => {
                let args = split_off_results(&mut results, n)?;
                results.push(simplify_cas_value(&Value::from_cas_apply(
                    name.as_str(),
                    args,
                ))?);
            }
            ExpandFrame::Eq => {
                let rhs = results
                    .pop()
                    .ok_or_else(|| cas_internal_err("expanding a symbolic expression"))?;
                let lhs = results
                    .pop()
                    .ok_or_else(|| cas_internal_err("expanding a symbolic expression"))?;
                results.push(Value::from_cas_eq(lhs, rhs));
            }
        }
    }

    let result = results
        .pop()
        .ok_or_else(|| cas_internal_err("expanding a symbolic expression"))?;
    Ok(result)
}

pub(super) fn split_off_results(results: &mut Vec<Value>, n: usize) -> WqResult<Vec<Value>> {
    if results.len() < n {
        return Err(cas_internal_err("processing a symbolic expression"));
    }
    Ok(results.split_off(results.len().saturating_sub(n)))
}

#[cfg(test)]
pub(crate) fn expand_cas(expr: &Value) -> WqResult<Value> {
    expand_cas_with_debug(expr, CasDebug::disabled())
}

pub(crate) fn expand_cas_with_debug(expr: &Value, debug: CasDebug<'_>) -> WqResult<Value> {
    cas_trace!(debug, DebugLogFlags::CAS, "[expand_cas] in: {}", expr);
    let expr = simplify_cas_value(expr)?;
    let expanded = expand_expr(&expr)?;
    let result = simplify_cas_value(&expanded)?;
    cas_trace!(debug, DebugLogFlags::CAS, "[expand_cas] out: {}", result);
    Ok(result)
}

fn push_factor(out: &mut Vec<(Value, Value)>, base: Value, power: Value) {
    for (existing_base, existing_power) in out.iter_mut() {
        if *existing_base == base {
            *existing_power = numeric_add(existing_power, &power).expect("numeric power addition");
            return;
        }
    }
    out.push((base, power));
}

fn factor_term(term: &Value) -> (Value, Vec<(Value, Value)>) {
    if !term.is_cas_expr() {
        return (term.clone(), Vec::new());
    }
    if let Some(args) = term.cas_op_args(CasOp::Multiply) {
        let mut coeff = Value::Int(1);
        let mut factors = Vec::new();
        for arg in args {
            if !arg.is_cas_expr() {
                coeff = numeric_mul(&coeff, arg).expect("numeric coefficient multiply");
            } else {
                let (base, power) = split_mul_factor(arg);
                push_factor(&mut factors, base, power);
            }
        }
        return (coeff, factors);
    }
    let (base, power) = split_mul_factor(term);
    (Value::Int(1), vec![(base, power)])
}

/// Extract the rational content (GCD of all coefficients) from an algebraic
/// value.
pub(super) fn extract_algebraic_content(a: &crate::value::algebraic::AlgebraicData) -> Value {
    match algebraic_coeff_gcd(&a.coeffs) {
        Some(c) if c > BigInt::one() => Value::from_bigint(c),
        _ => Value::Int(1),
    }
}

/// Compute the GCD of the rational integer parts of algebraic coefficients.
/// Returns `None` if any coefficient has a non-trivial denominator.
fn algebraic_coeff_gcd(coeffs: &[Value]) -> Option<BigInt> {
    let mut g: Option<BigInt> = None;
    for c in coeffs {
        if numeric_is_zero(c) {
            continue;
        }
        let (n, d) = c.rational_parts()?;
        if !d.is_one() {
            return None;
        }
        g = Some(match g.take() {
            None => n.abs(),
            Some(prev) => bigint_abs_gcd(prev, n.abs()),
        });
    }
    g
}

/// GCD of two numeric values, handling Algebraic by extracting rational
/// content.
pub(super) fn eval_numeric_binary_gcd(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    let extract_int = |v: &Value| -> Option<BigInt> {
        if let Value::Algebraic(a) = v {
            algebraic_coeff_gcd(&a.coeffs)
        } else {
            let (n, d) = v.rational_parts()?;
            if !d.is_one() { None } else { Some(n.abs()) }
        }
    };
    let ln = extract_int(lhs).unwrap_or(BigInt::one());
    let rn = extract_int(rhs).unwrap_or(BigInt::one());
    Ok(Value::from_bigint(bigint_abs_gcd(ln, rn)))
}

fn bigint_abs_gcd(lhs: BigInt, rhs: BigInt) -> BigInt {
    let mut a = lhs.abs();
    let mut b = rhs.abs();
    while !b.is_zero() {
        let next = &a % &b;
        a = b;
        b = next;
    }
    a
}

fn common_numeric_factor(coeffs: &[Value]) -> Option<Value> {
    if let Some(first) = coeffs.first()
        && !numeric_is_zero(first)
        && !numeric_is_one(first)
        && coeffs.iter().all(|coeff| coeff == first)
    {
        return Some(first.clone());
    }

    fn bigint_lcm(lhs: &BigInt, rhs: &BigInt) -> BigInt {
        if lhs.is_zero() || rhs.is_zero() {
            return BigInt::zero();
        }
        (lhs / bigint_abs_gcd(lhs.clone(), rhs.clone())) * rhs
    }

    if let Some((first_n, first_d)) = coeffs.first().and_then(Value::rational_parts) {
        let mut gcd_num = first_n.abs();
        let mut lcm_den = first_d;
        let mut all_rational = true;
        for coeff in coeffs.iter().skip(1) {
            if let Some((n, d)) = coeff.rational_parts() {
                gcd_num = bigint_abs_gcd(gcd_num, n.abs());
                lcm_den = bigint_lcm(&lcm_den, &d);
            } else {
                all_rational = false;
                break;
            }
        }
        if all_rational {
            let all_negative = coeffs.iter().all(numeric_is_negative);
            let numer = if all_negative { -gcd_num } else { gcd_num };
            let factor = Value::from_fraction_parts(numer, lcm_den);
            if !numeric_is_zero(&factor) && !numeric_is_one(&factor) {
                return Some(factor);
            }
        }
    }

    fn extract_int(c: &Value) -> Option<BigInt> {
        if let Value::Algebraic(a) = c {
            algebraic_coeff_gcd(&a.coeffs)
        } else {
            c.exact_int().map(|n| n.abs())
        }
    }
    let mut iter = coeffs.iter().map(extract_int);
    let first = iter.next()??;
    let mut gcd = first;
    for coeff in iter {
        gcd = bigint_abs_gcd(gcd, coeff?);
    }
    if gcd.is_zero() || gcd.is_one() {
        return None;
    }
    let all_negative = coeffs.iter().all(numeric_is_negative);
    Some(Value::from_bigint(if all_negative { -gcd } else { gcd }))
}

fn intersect_common_factors(common: &mut Vec<(Value, Value)>, factors: &[(Value, Value)]) {
    let mut idx = 0;
    while idx < common.len() {
        if let Some((_, power)) = factors.iter().find(|(base, _)| *base == common[idx].0) {
            if let Ok(cmp) = numeric_sub(power, &common[idx].1)
                && numeric_is_negative(&cmp)
            {
                common[idx].1 = power.clone();
            }
            idx += 1;
        } else {
            common.remove(idx);
        }
    }
}

fn build_from_coeff_and_factors(coeff: Value, factors: Vec<(Value, Value)>) -> WqResult<Value> {
    let mut out = Vec::with_capacity(factors.len() + 1);
    if !numeric_is_one(&coeff) || factors.is_empty() {
        out.push(coeff);
    }
    for (base, power) in factors {
        if numeric_is_zero(&power) {
            continue;
        }
        if numeric_is_one(&power) {
            out.push(base);
        } else {
            out.push(cas_pow(base, power)?);
        }
    }
    cas_mul(out)
}

pub(super) fn factor_expr(expr: &Value) -> WqResult<Value> {
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        return Ok(Value::from_cas_eq(factor_expr(lhs)?, factor_expr(rhs)?));
    }
    let expr = simplify_cas_value(expr)?;
    let Some(args) = expr.cas_op_args(CasOp::Add) else {
        return Ok(expr);
    };
    if args.len() < 2 {
        return Ok(expr);
    }

    let mut term_coeffs = Vec::with_capacity(args.len());
    let mut term_factors = Vec::with_capacity(args.len());
    for term in args {
        let (coeff, factors) = factor_term(term);
        term_coeffs.push(coeff);
        term_factors.push(factors);
    }

    let mut common_factors = term_factors[0].clone();
    for factors in &term_factors[1..] {
        intersect_common_factors(&mut common_factors, factors);
    }
    common_factors.retain(|(_, power)| !numeric_is_zero(power));
    let common_numeric = common_numeric_factor(&term_coeffs);
    if common_factors.is_empty() && common_numeric.is_none() {
        return Ok(expr);
    }

    let common_coeff = common_numeric.clone().unwrap_or(Value::Int(1));
    let common = build_from_coeff_and_factors(common_coeff.clone(), common_factors.clone())?;

    let mut reduced_terms = Vec::with_capacity(args.len());
    for (coeff, factors) in term_coeffs.into_iter().zip(term_factors) {
        let reduced_coeff = if let Some(common_numeric) = &common_numeric {
            eval_exact_numeric_div(&coeff, common_numeric)?
        } else {
            coeff
        };
        let mut remaining = factors;
        for (common_base, common_power) in &common_factors {
            if let Some((_, power)) = remaining.iter_mut().find(|(base, _)| *base == *common_base) {
                *power = numeric_sub(power, common_power).expect("numeric power subtraction");
            }
        }
        remaining.retain(|(_, power)| !numeric_is_zero(power));
        reduced_terms.push(build_from_coeff_and_factors(reduced_coeff, remaining)?);
    }

    cas_mul(vec![common, cas_add(reduced_terms)?])
}

pub(crate) fn factor_cas(expr: &Value) -> WqResult<Value> {
    factor_expr(&simplify_cas_value(expr)?)
}

#[cfg(test)]
mod internal_error_tests {
    use super::*;
    use crate::wqerror::WqErrorType;

    #[test]
    fn traversal_underflow_is_an_internal_cas_error() {
        let err = split_off_results(&mut Vec::new(), 1)
            .expect_err("missing traversal result should fail");

        assert_eq!(err.err_type, WqErrorType::Vm);
        assert_eq!(
            err.msg.as_deref(),
            Some("internal CAS error while processing a symbolic expression")
        );
    }
}
