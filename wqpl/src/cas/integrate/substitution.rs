use super::{integrate_expr_with_depth, split_off_numeric};
use crate::cas::diff::diff_expr;
use crate::cas::{cas_div, cas_mul, cas_product, numeric_is_one, simplify_cas_value};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

pub(super) fn integrate_by_substitution(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let expr_fmt = expr.format_cas().unwrap_or_else(|| expr.to_string());
    cas_trace!(DebugLogFlags::CAS, "[cas] substitution enter: {expr_fmt}");
    let Some((CasOp::Multiply, args)) = expr.cas_op_parts() else {
        cas_trace!(DebugLogFlags::CAS, "[cas] substitution exit (not_product)");
        return Ok(None);
    };
    let (coeff, symbolic) = split_off_numeric(args);

    if symbolic.is_empty() {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] substitution exit (empty_symbolic)"
        );
        return Ok(None);
    }

    for (gi, f_of_g) in symbolic.iter().enumerate() {
        let (fname_opt, inner_opt, is_half_pow): (Option<CasFunction>, Option<&Value>, bool) =
            // f(g(x)) — Call node like sin[x²], exp[x³], sqrt[x+1]
            if let Some((name, fargs)) = f_of_g.cas_function_parts()
                && fargs.len() == 1
            {
                (Some(name), Some(&fargs[0]), false)
            }
            // (g(x))^(1/2) or (g(x))^(-1/2) — half-power Op node
            else if let Some((CasOp::Power, [base, exp])) = f_of_g.cas_op_parts()
                && (exp.exact_half() || exp.exact_neg_half())
            {
                (Some(CasFunction::Sqrt), Some(base), true)
            } else {
                (None, None, false)
            };

        let (Some(fname), Some(inner)) = (fname_opt, inner_opt) else {
            continue;
        };
        if !inner.is_cas_expr() || inner.cas_var_name() == Some(var) {
            continue;
        }

        // Candidate u = g(x) = inner
        let u_expr = inner;

        // Build effective du from remaining factors
        let remaining_factors: Vec<Value> = symbolic
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != gi)
            .map(|(_, f)| f.clone())
            .collect();

        let du = diff_expr(u_expr, var)?;

        let remaining_product = cas_product(remaining_factors.to_vec());
        let effective_du = if numeric_is_one(&coeff) {
            remaining_product
        } else {
            cas_mul(vec![coeff.clone(), remaining_product.clone()]).unwrap_or(remaining_product)
        };

        // Check if effective_du equals du (up to a constant)
        let scale = match_du_scale(&effective_du, &du)?;
        if let Some(scale) = scale {
            let integrated = if is_half_pow {
                // ∫ u^(1/2) du = 2/3 · u^(3/2)  (direct power rule)
                let u_var = Value::from_cas_var("--cas-sub-u");
                let two_thirds = Value::from_fraction_parts(2u64.into(), 3u64.into());
                cas_mul(vec![
                    two_thirds,
                    Value::from_cas_op(
                        "^",
                        vec![u_var, Value::from_fraction_parts(3u64.into(), 2u64.into())],
                    ),
                ])?
            } else {
                let f_of_u = Value::from_cas_call(fname, vec![Value::from_cas_var("--cas-sub-u")]);
                integrate_expr_with_depth(&f_of_u, "--cas-sub-u", 0)?
            };
            // Substitute u back: F(u) → F(g(x))
            let result = substitute_into_call(&integrated, "--cas-sub-u", u_expr)?;
            let result = if numeric_is_one(&scale) {
                result
            } else {
                cas_div(result, scale)?
            };
            return Ok(Some(simplify_cas_value(&result)?));
        }
    }

    cas_trace!(DebugLogFlags::CAS, "[cas] substitution exit (no_match)");
    Ok(None)
}

fn match_du_scale(effective_du: &Value, du: &Value) -> WqResult<Option<Value>> {
    if values_equal(effective_du, du) {
        return Ok(Some(Value::Int(1)));
    }
    // If du = c * effective_du, then effective_du = du / c
    // The integrand f(g(x)) * (g'(x)/c) dx = (1/c) * f(u) du
    // Scale = c (we divide the result by c)
    let quotient = simplify_cas_value(&cas_div(du.clone(), effective_du.clone())?)?;
    if !quotient.is_cas_expr() {
        return Ok(Some(quotient));
    }
    Ok(None)
}

fn values_equal(a: &Value, b: &Value) -> bool {
    if let Ok(a) = simplify_cas_value(a)
        && let Ok(b) = simplify_cas_value(b)
    {
        return a == b;
    }
    false
}

fn substitute_into_call(expr: &Value, var: &str, replacement: &Value) -> WqResult<Value> {
    if expr.cas_var_name() == Some(var) {
        return Ok(replacement.clone());
    }
    if let Some((op, args)) = expr.cas_op_parts() {
        let mut new_args = Vec::with_capacity(args.len());
        for arg in args {
            new_args.push(substitute_into_call(arg, var, replacement)?);
        }
        return simplify_cas_value(&Value::from_cas_op(op, new_args));
    }
    if let Some((name, args)) = expr.cas_function_parts() {
        let mut new_args = Vec::with_capacity(args.len());
        for arg in args {
            new_args.push(substitute_into_call(arg, var, replacement)?);
        }
        return simplify_cas_value(&Value::from_cas_call(name, new_args));
    }
    if let Some((name, args)) = expr.cas_apply_parts() {
        let mut new_args = Vec::with_capacity(args.len());
        for arg in args {
            new_args.push(substitute_into_call(arg, var, replacement)?);
        }
        return simplify_cas_value(&Value::from_cas_apply(name.as_str(), new_args));
    }
    Ok(expr.clone())
}
