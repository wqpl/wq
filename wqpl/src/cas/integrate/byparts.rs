use std::cell::{Cell, RefCell};
use std::collections::HashSet;

use num_bigint::BigInt;

use super::{MAX_DEPTH, integrate_expr_with_depth, split_off_numeric};
use crate::cas::diff::diff_expr;
use crate::cas::{
    cas_add, cas_div, cas_mul, cas_pow, cas_product, cas_sub, contains_cas_var,
    extract_linear_coefficients, numeric_is_one, numeric_is_zero, poly_derivative, poly_from_expr,
    poly_is_zero, poly_to_expr, simplify_cas_value,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

/// Maximum number of nested by-parts calls in a single chain before
/// we bail out to avoid infinite ping-pong (e.g. `int e^x*sin x dx` without
/// the dedicated formula).
const MAX_BYPARTS_CHAIN: usize = 12;

thread_local! {
    static BYPARTS_ACTIVE: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// Global nesting counter -- prevents infinite chains where each step
    /// changes the expression (e.g. 1/(x-a)^n -> 1/(x-a)^(n+1)) but the
    /// per-key cycle guard never triggers because the key changes.
    static BYPARTS_NESTING: Cell<usize> = const { Cell::new(0) };
}

struct ByPartsGuard {
    key: String,
}

impl Drop for ByPartsGuard {
    fn drop(&mut self) {
        BYPARTS_ACTIVE.with(|set| {
            set.borrow_mut().remove(&self.key);
        });
        BYPARTS_NESTING.with(|n| n.set(n.get().saturating_sub(1)));
    }
}

fn enter_byparts(key: String) -> Option<ByPartsGuard> {
    let nesting_ok = BYPARTS_NESTING.with(|n| {
        let depth = n.get();
        if depth >= MAX_BYPARTS_CHAIN {
            false
        } else {
            n.set(depth + 1);
            true
        }
    });
    if !nesting_ok {
        return None;
    }
    BYPARTS_ACTIVE.with(|set| {
        let mut s = set.borrow_mut();
        if s.len() >= MAX_BYPARTS_CHAIN || !s.insert(key.clone()) {
            None
        } else {
            Some(ByPartsGuard { key })
        }
    })
}

fn canonical_key(expr: &Value) -> String {
    let mut out = String::new();
    push_canonical_key(expr, &mut out);
    out
}

fn push_canonical_key(value: &Value, out: &mut String) {
    if let Some(name) = value.cas_var_name() {
        out.push_str("v:");
        out.push_str(name);
        return;
    }
    if let Some((op, args)) = value.cas_op_parts() {
        out.push_str("o:");
        out.push_str(op.symbol());
        out.push('(');
        for arg in args {
            push_canonical_key(arg, out);
            out.push(',');
        }
        out.push(')');
        return;
    }
    if let Some((name, args)) = value.cas_function_parts() {
        out.push_str("c:");
        out.push_str(name.name());
        out.push('(');
        for arg in args {
            push_canonical_key(arg, out);
            out.push(',');
        }
        out.push(')');
        return;
    }
    if let Some((name, args)) = value.cas_apply_parts() {
        out.push_str("a:");
        out.push_str(name.as_str());
        out.push('(');
        for arg in args {
            push_canonical_key(arg, out);
            out.push(',');
        }
        out.push(')');
        return;
    }
    out.push_str("n:");
    out.push_str(&value.to_string());
}

pub(super) fn integrate_by_parts(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] byparts enter: {}",
        expr.format_cas().unwrap_or_else(|| expr.to_string())
    );

    // 1. Try direct formula for exp*sin / exp*cos
    if let Some(result) = try_exp_trig_product(expr, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] byparts exit (exp_trig): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }

    // 2. Try tabular integration (polynomial * cyclic function)
    if let Some(result) = try_tabular(expr, var)? {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] byparts exit (tabular): {}",
            result.format_cas().unwrap_or_else(|| result.to_string())
        );
        return Ok(Some(result));
    }

    let Some((CasOp::Multiply, args)) = expr.cas_op_parts() else {
        cas_trace!(DebugLogFlags::CAS, "[cas] byparts exit (not_product)");
        return Ok(None);
    };
    let (_, symbolic) = split_off_numeric(args);
    if symbolic.len() < 2 {
        cas_trace!(DebugLogFlags::CAS, "[cas] byparts exit (too_few_symbolic)");
        return Ok(None);
    }
    if has_trig_over_non_polynomial_factor(&symbolic, var) {
        cas_trace!(
            DebugLogFlags::CAS,
            "[cas] byparts exit (trig_non_polynomial_mix)"
        );
        return Ok(None);
    }

    // 3. Cycle guard for ordinary by-parts
    let key = format!("{}|{}", canonical_key(expr), var);
    let _guard = match enter_byparts(key) {
        Some(g) => g,
        None => {
            let nesting = BYPARTS_NESTING.with(|n| n.get());
            cas_trace!(
                DebugLogFlags::CAS,
                "[cas] byparts exit (cycle_guard blocked) nesting={nesting}"
            );
            return Ok(None);
        }
    };

    // Collect candidate (u, dv) pairs, LIATE-preferred first
    let mut candidates: Vec<(Value, Value)> = Vec::new();
    for split_idx in 1..symbolic.len() {
        let a = cas_product(symbolic[..split_idx].to_vec());
        let b = cas_product(symbolic[split_idx..].to_vec());
        if liate_rank(&a) >= liate_rank(&b) {
            candidates.push((a.clone(), b.clone()));
            candidates.push((b, a));
        } else {
            candidates.push((b.clone(), a.clone()));
            candidates.push((a, b));
        }
    }

    for (u, dv) in candidates {
        let nesting = BYPARTS_NESTING.with(|n| n.get());
        let depth = nesting + 1; // at least 1, increases with nesting
        if let Ok(Some(result)) = try_parts(&u, &dv, var, depth) {
            cas_trace!(
                DebugLogFlags::CAS,
                "[cas] byparts exit: {}",
                result.format_cas().unwrap_or_else(|| result.to_string())
            );
            return Ok(Some(result));
        }
    }
    cas_trace!(DebugLogFlags::CAS, "[cas] byparts exit (no_candidate)");
    Ok(None)
}

fn has_trig_over_non_polynomial_factor(symbolic: &[Value], var: &str) -> bool {
    symbolic.iter().any(is_trig_like_factor)
        && symbolic.iter().any(|factor| {
            !is_trig_like_factor(factor)
                && contains_cas_var(factor, var)
                && poly_from_expr(factor, var).is_err()
        })
}

fn is_trig_like_factor(expr: &Value) -> bool {
    expr.cas_function_parts().is_some_and(|(name, args)| {
        args.len() == 1
            && matches!(
                name,
                CasFunction::Sin
                    | CasFunction::Cos
                    | CasFunction::Tan
                    | CasFunction::Sec
                    | CasFunction::Csc
                    | CasFunction::Cot
                    | CasFunction::Sinh
                    | CasFunction::Cosh
                    | CasFunction::Tanh
            )
    })
}

/// Extract the argument from an exponential factor.
/// Matches: `exp[g(x)]` (Call node) and `e^(g(x))` (Pow node with Const("e")).
pub(super) fn try_extract_exp_arg(factor: &Value) -> Option<Value> {
    if let Some((name, inner)) = factor.cas_function_parts()
        && name == CasFunction::Exp
        && inner.len() == 1
    {
        return Some(inner[0].clone());
    }
    if let Some((CasOp::Power, args)) = factor.cas_op_parts()
        && args.len() == 2
        && args[0].cas_const_name() == Some("e")
    {
        return Some(args[1].clone());
    }
    None
}

// ---------------------------------------------------------------------------
// Direct formula: int e^{ax+b} * sin(cx+d) dx  and  int e^{ax+b} * cos(cx+d) dx
// ---------------------------------------------------------------------------
fn try_exp_trig_product(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some((CasOp::Multiply, args)) = expr.cas_op_parts() else {
        return Ok(None);
    };
    let (coeff, symbolic) = split_off_numeric(args);
    if symbolic.len() != 2 {
        return Ok(None);
    }

    let mut exp_arg = None;
    let mut trig = None;
    for factor in &symbolic {
        if let Some(arg) = try_extract_exp_arg(factor) {
            exp_arg = Some(arg);
        } else if let Some((name @ (CasFunction::Sin | CasFunction::Cos), [arg])) =
            factor.cas_function_parts()
        {
            trig = Some((name, arg.clone()));
        }
    }

    let exp_arg_val = match exp_arg {
        Some(arg) => arg,
        None => return Ok(None),
    };
    let (exp_a, _exp_b) = match extract_linear_coefficients(&exp_arg_val, var) {
        Some(c) => c,
        None => return Ok(None),
    };
    let (trig_name, trig_arg_val) = match trig {
        Some(t) => t,
        None => return Ok(None),
    };
    let (trig_a, _trig_b) = match extract_linear_coefficients(&trig_arg_val, var) {
        Some(c) => c,
        None => return Ok(None),
    };

    if numeric_is_zero(&exp_a) || numeric_is_zero(&trig_a) {
        return Ok(None);
    }

    // denominator = a^2 + c^2
    let a2 = cas_pow(exp_a.clone(), Value::from_bigint(BigInt::from(2)))?;
    let c2 = cas_pow(trig_a.clone(), Value::from_bigint(BigInt::from(2)))?;
    let denom = cas_add(vec![a2, c2])?;

    let exp_expr = Value::from_cas_function(CasFunction::Exp, vec![exp_arg_val.clone()]);
    let trig_expr = Value::from_cas_function(trig_name, vec![trig_arg_val.clone()]);

    let numerator = if trig_name == CasFunction::Sin {
        // a*sin(cx+d) - c*cos(cx+d)
        let a_sin = cas_mul(vec![exp_a.clone(), trig_expr])?;
        let c_cos = cas_mul(vec![
            trig_a.clone(),
            Value::from_cas_function(CasFunction::Cos, vec![trig_arg_val.clone()]),
        ])?;
        cas_sub(a_sin, c_cos)?
    } else {
        // a*cos(cx+d) + c*sin(cx+d)
        let a_cos = cas_mul(vec![exp_a.clone(), trig_expr])?;
        let c_sin = cas_mul(vec![
            trig_a.clone(),
            Value::from_cas_function(CasFunction::Sin, vec![trig_arg_val.clone()]),
        ])?;
        cas_add(vec![a_cos, c_sin])?
    };

    let mut result = cas_mul(vec![exp_expr, cas_div(numerator, denom)?])?;
    if !numeric_is_one(&coeff) {
        result = cas_mul(vec![coeff, result])?;
    }
    simplify_cas_value(&result).map(Some)
}

// ---------------------------------------------------------------------------
// Tabular integration: polynomial * cyclic function
// ---------------------------------------------------------------------------
fn try_tabular(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let Some((CasOp::Multiply, args)) = expr.cas_op_parts() else {
        return Ok(None);
    };
    let (coeff, symbolic) = split_off_numeric(args);
    if symbolic.len() != 2 {
        return Ok(None);
    }

    // Identify which factor is the polynomial and which is cyclic.
    let (poly_factor_idx, cyclic_factor_idx) = if poly_from_expr(&symbolic[0], var).is_ok() {
        (0, 1)
    } else if poly_from_expr(&symbolic[1], var).is_ok() {
        (1, 0)
    } else {
        return Ok(None);
    };

    let poly = poly_from_expr(&symbolic[poly_factor_idx], var)?;
    let cyclic = &symbolic[cyclic_factor_idx];

    let (cyclic_name, cyclic_arg) = match cyclic.cas_function_parts() {
        Some((name, [arg])) if is_tabular_cyclic(name) => (name, arg),
        _ => return Ok(None),
    };

    // The argument must be linear in the integration variable.
    let (k, _b) = match extract_linear_coefficients(cyclic_arg, var) {
        Some(c) => c,
        None => return Ok(None),
    };
    if numeric_is_zero(&k) {
        return Ok(None);
    }

    // Build derivative rows until the polynomial vanishes.
    let mut derivatives = vec![poly];
    loop {
        let last = derivatives.last().expect("derivatives should be non-empty");
        let d = poly_derivative(last);
        if poly_is_zero(&d) {
            break;
        }
        derivatives.push(d);
    }

    // Sum with alternating signs: sum (-1)^i * P^(i) * int^(i+1) g
    let mut terms = Vec::with_capacity(derivatives.len());
    for (i, deriv_coeffs) in derivatives.iter().enumerate() {
        let sign = if i % 2 == 0 {
            Value::Int(1)
        } else {
            Value::Int(-1)
        };
        let poly_expr = poly_to_expr(deriv_coeffs, var)?;
        let integral_expr = compute_cyclic_integral(cyclic_name, cyclic_arg, i + 1, &k)?;
        let term = cas_mul(vec![sign, poly_expr, integral_expr])?;
        terms.push(term);
    }

    let mut result = cas_add(terms)?;
    if !numeric_is_one(&coeff) {
        result = cas_mul(vec![coeff, result])?;
    }
    simplify_cas_value(&result).map(Some)
}

fn is_tabular_cyclic(name: CasFunction) -> bool {
    matches!(
        name,
        CasFunction::Exp
            | CasFunction::Sin
            | CasFunction::Cos
            | CasFunction::Sinh
            | CasFunction::Cosh
    )
}

/// Compute the j-th repeated integral of a cyclic function whose argument
/// is `k*var + b`.
///
/// For `exp`:  always `exp(arg) / k^j`.
/// For `sin`/`cos`: cycle with period 4.
/// For `sinh`/`cosh`: cycle with period 2.
fn compute_cyclic_integral(name: CasFunction, arg: &Value, j: usize, k: &Value) -> WqResult<Value> {
    let kj = cas_pow(k.clone(), Value::from_bigint(BigInt::from(j)))?;

    let (fn_name, sign) = match name {
        CasFunction::Exp => (CasFunction::Exp, Value::Int(1)),
        CasFunction::Sin => match j % 4 {
            1 => (CasFunction::Cos, Value::Int(-1)), //  int sin = -cos
            2 => (CasFunction::Sin, Value::Int(-1)), //  int^2 sin = -sin
            3 => (CasFunction::Cos, Value::Int(1)),  //  int^3 sin = cos
            0 => (CasFunction::Sin, Value::Int(1)),  //  int^4 sin = sin
            _ => unreachable!(),
        },
        CasFunction::Cos => match j % 4 {
            1 => (CasFunction::Sin, Value::Int(1)),  //  int cos = sin
            2 => (CasFunction::Cos, Value::Int(-1)), //  int^2 cos = -cos
            3 => (CasFunction::Sin, Value::Int(-1)), //  int^3 cos = -sin
            0 => (CasFunction::Cos, Value::Int(1)),  //  int^4 cos = cos
            _ => unreachable!(),
        },
        CasFunction::Sinh => match j % 2 {
            1 => (CasFunction::Cosh, Value::Int(1)),
            0 => (CasFunction::Sinh, Value::Int(1)),
            _ => unreachable!(),
        },
        CasFunction::Cosh => match j % 2 {
            1 => (CasFunction::Sinh, Value::Int(1)),
            0 => (CasFunction::Cosh, Value::Int(1)),
            _ => unreachable!(),
        },
        _ => return Ok(Value::from_cas_function(name, vec![arg.clone()])),
    };

    let call = Value::from_cas_function(fn_name, vec![arg.clone()]);
    let div = cas_div(call, kj)?;
    cas_mul(vec![sign, div])
}

// ---------------------------------------------------------------------------
// Ordinary LIATE by-parts
// ---------------------------------------------------------------------------
fn liate_rank(expr: &Value) -> i32 {
    if let Some((name, _)) = expr.cas_function_parts() {
        match name {
            CasFunction::Ln | CasFunction::Log2 | CasFunction::Log10 => return 5,
            CasFunction::ArcSin
            | CasFunction::ArcCos
            | CasFunction::ArcTan
            | CasFunction::ArcSinh
            | CasFunction::ArcCosh
            | CasFunction::ArcTanh => return 4,
            CasFunction::Sin
            | CasFunction::Cos
            | CasFunction::Tan
            | CasFunction::Sec
            | CasFunction::Csc
            | CasFunction::Cot => return 2,
            CasFunction::Sinh | CasFunction::Cosh | CasFunction::Tanh => return 2,
            CasFunction::Exp => return 1,
            _ => return 0,
        }
    }
    if expr.cas_var_name().is_some() || expr.cas_op_parts().is_some() {
        return 3; // algebraic
    }
    0
}

fn try_parts(u: &Value, dv: &Value, var: &str, depth: usize) -> WqResult<Option<Value>> {
    if depth >= MAX_DEPTH {
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            depth,
            "[cas-v] try_parts depth={depth} -> max_depth_exceeded",
        );
        return Ok(None);
    }
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        depth,
        "[cas-v] try_parts enter depth={depth} u={} dv={}",
        u.format_cas().unwrap_or_else(|| u.to_string()),
        dv.format_cas().unwrap_or_else(|| dv.to_string())
    );
    let v = match integrate_expr_with_depth(dv, var, depth + 1) {
        Ok(v) => v,
        Err(_) => {
            cas_trace_depth!(
                DebugLogFlags::CAS_VERBOSE,
                depth,
                "[cas-v] try_parts depth={depth} -> dv_integration_failed",
            );
            return Ok(None);
        }
    };
    let du = diff_expr(u, var)?;

    let vdu = cas_mul(vec![v.clone(), du])?;
    let rest = match integrate_expr_with_depth(&vdu, var, depth + 1) {
        Ok(r) => r,
        Err(_) => {
            cas_trace_depth!(
                DebugLogFlags::CAS_VERBOSE,
                depth,
                "[cas-v] try_parts depth={depth} -> vdu_integration_failed",
            );
            return Ok(None);
        }
    };

    let uv = cas_mul(vec![u.clone(), v])?;
    let result = simplify_cas_value(&cas_sub(uv, rest)?)?;
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        depth,
        "[cas-v] try_parts exit depth={depth} -> {}",
        result.format_cas().unwrap_or_else(|| result.to_string())
    );
    Ok(Some(result))
}
