use std::cmp::Ordering;

use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive};

use super::limit::LimitDirection;
use super::numeric::{
    numeric_abs, numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul,
};
use crate::value::cas::{CasConst, CasOp};
use crate::value::Value;

fn precedence(value: &Value) -> u8 {
    match value.cas_known_op_parts() {
        Some((CasOp::Add, _)) => 1,
        Some((CasOp::Multiply, _)) => 2,
        Some((CasOp::Power, _)) => 3,
        _ => 4,
    }
}

fn canonical_degree(value: &Value) -> u32 {
    if !value.is_cas_expr() {
        return 0;
    }
    if value.cas_var_name().is_some() {
        return 1;
    }
    if let Some((op, args)) = value.cas_known_op_parts() {
        return match (op, args) {
            (CasOp::Add, args) => args.iter().map(canonical_degree).max().unwrap_or(0),
            (CasOp::Multiply, args) => args
                .iter()
                .fold(0u32, |acc, arg| acc.saturating_add(canonical_degree(arg))),
            (CasOp::Power, [base, exp]) => match exp.exact_int().and_then(|n| n.to_u32()) {
                Some(n) => canonical_degree(base).saturating_mul(n),
                None => u32::MAX / 4,
            },
            _ => u32::MAX / 4,
        };
    }
    if let Some((_name, args)) = value.cas_function_parts() {
        return args
            .iter()
            .map(canonical_degree)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
    }
    if let Some((_name, args)) = value.cas_apply_parts() {
        return args
            .iter()
            .map(canonical_degree)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
    }
    0
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

fn canonical_cmp(lhs: &Value, rhs: &Value) -> Ordering {
    let degree_cmp = canonical_degree(lhs).cmp(&canonical_degree(rhs));
    if degree_cmp != Ordering::Equal {
        return degree_cmp;
    }
    let mut lhs_key = String::new();
    let mut rhs_key = String::new();
    push_canonical_key(lhs, &mut lhs_key);
    push_canonical_key(rhs, &mut rhs_key);
    lhs_key.cmp(&rhs_key)
}

pub(super) fn sort_canonical(values: &mut [Value]) {
    values.sort_by(canonical_cmp);
}

fn format_arg(value: &Value, parent_prec: u8) -> String {
    let rendered = format_expr(value, 0);
    let needs_parens = if value.is_cas_expr() {
        precedence(value) < parent_prec
    } else {
        rendered.contains(" + ") || rendered.contains(" - ")
    };
    if needs_parens {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn format_atom(value: &Value) -> String {
    if let Some((numer, denom)) = value.rational_parts()
        && !denom.is_one()
    {
        return format!("{numer}/{denom}");
    }
    value.to_string()
}

fn reciprocal_denominator(value: &Value) -> Option<Value> {
    if let Some((numer, denom)) = value.rational_parts()
        && numer == BigInt::one()
    {
        return Some(Value::from_bigint(denom));
    }
    let Value::Float(f) = value else {
        return None;
    };
    if !f.is_finite() || **f <= 0.0 {
        return None;
    }
    let inverse = 1.0 / **f;
    if (inverse - inverse.round()).abs() > f64::EPSILON {
        return None;
    }
    Some(Value::Int(inverse.round() as i64))
}

fn format_power(base: &Value, exp: &Value, parent_prec: u8) -> String {
    let prec: u8 = 3;
    let base_rendered = if base.cas_op_args(CasOp::Power).is_some() {
        format!("({})", format_expr(base, 0))
    } else if let Value::Algebraic(a) = base {
        let non_zero: Vec<usize> = a
            .coeffs
            .iter()
            .enumerate()
            .filter(|(_, c)| !numeric_is_zero(c))
            .map(|(i, _)| i)
            .collect();
        let needs_paren = non_zero.len() > 1 || non_zero.iter().any(|&i| i >= 2);
        if needs_paren {
            format!("({})", format_expr(base, 0))
        } else {
            format_arg(base, prec)
        }
    } else {
        format_arg(base, prec)
    };
    let exp_rendered = if exp
        .rational_parts()
        .is_some_and(|(_, denom)| !denom.is_one())
    {
        format!("({})", format_expr(exp, 0))
    } else {
        format_arg(exp, prec.saturating_add(1))
    };
    let rendered = format!("{}^{}", base_rendered, exp_rendered);
    if prec < parent_prec {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn split_denominator_factor(value: &Value) -> Option<Value> {
    let [base, exp] = value.cas_op_args(CasOp::Power)? else {
        return None;
    };
    let power = exp.exact_int()?;
    if !power.is_negative() {
        return None;
    }
    let abs_power = Value::from_bigint(-power);
    Some(if numeric_is_one(&abs_power) {
        base.clone()
    } else {
        Value::from_cas_known_op(CasOp::Power, vec![base.clone(), abs_power])
    })
}

fn display_factor_cmp(lhs: &Value, rhs: &Value) -> Ordering {
    let lhs_is_add = lhs.cas_op_args(CasOp::Add).is_some();
    let rhs_is_add = rhs.cas_op_args(CasOp::Add).is_some();
    lhs_is_add
        .cmp(&rhs_is_add)
        .then_with(|| canonical_degree(rhs).cmp(&canonical_degree(lhs)))
        .then_with(|| canonical_cmp(lhs, rhs))
}

fn format_product_parts(leading: Option<&Value>, rest: &[Value], parent_prec: u8) -> String {
    /// True when a CAS expression is a manifest constant (no variable
    /// dependency). Used only for display grouping — does not affect
    /// canonical sort order.
    fn is_constant_cas(value: &Value) -> bool {
        if let Some([base, _]) = value.cas_op_args(CasOp::Power) {
            return !base.is_cas_expr();
        }
        false
    }

    let prec = 2;
    let mut numeric_coeff: Option<Value> = leading.cloned();
    let mut numerators = Vec::new();
    let mut denominators = Vec::new();

    // Constant CAS factors (e.g. 3^(-1/4)) are pulled out so they display
    // before symbolic factors, but kept separate from the pure-numeric
    // coefficient to avoid creating a nested product that re-enters
    // format_product_parts and causes infinite recursion.
    let mut const_factors: Vec<Value> = Vec::new();

    for factor in rest {
        if let Some(denominator) = split_denominator_factor(factor) {
            denominators.push(denominator);
        } else if !factor.is_cas_expr() {
            numeric_coeff = Some(match numeric_coeff.take() {
                Some(acc) => numeric_mul(&acc, factor).expect("numeric display coefficient"),
                None => factor.clone(),
            });
        } else if is_constant_cas(factor) {
            const_factors.push(factor.clone());
        } else {
            numerators.push(factor.clone());
        }
    }

    numerators.sort_by(display_factor_cmp);
    denominators.sort_by(display_factor_cmp);

    // Prepend constant CAS factors at the front, before the leading numeric
    // coefficient.
    for cf in const_factors.into_iter().rev() {
        numerators.insert(0, cf);
    }

    if let Some(coeff) = numeric_coeff.take() {
        if let Some(denominator) = reciprocal_denominator(&coeff)
            && (!numerators.is_empty() || !denominators.is_empty())
        {
            denominators.insert(0, denominator);
        } else if !numeric_is_one(&coeff) || (numerators.is_empty() && denominators.is_empty()) {
            numerators.insert(0, coeff);
        }
    }

    let mut rendered = if numerators.is_empty() {
        String::from("1")
    } else {
        numerators
            .iter()
            .map(|factor| format_arg(factor, prec))
            .collect::<Vec<_>>()
            .join("*")
    };
    for denominator in denominators {
        rendered.push('/');
        rendered.push_str(&format_arg(&denominator, prec + 1));
    }
    if prec < parent_prec {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn format_term_with_sign(term: &Value, parent_prec: u8) -> (bool, String) {
    if !term.is_cas_expr() && numeric_is_negative(term) {
        return (true, format_arg(&numeric_abs(term), parent_prec));
    }

    if let Some(args) = term.cas_op_args(CasOp::Multiply)
        && let Some((first, rest)) = args.split_first()
        && !first.is_cas_expr()
        && numeric_is_negative(first)
    {
        let abs = numeric_abs(first);
        let rendered = if numeric_is_one(&abs) {
            format_product_parts(None, rest, parent_prec)
        } else {
            format_product_parts(Some(&abs), rest, parent_prec)
        };
        return (true, rendered);
    }

    (false, format_arg(term, parent_prec))
}

fn format_sum(args: &[Value], parent_prec: u8) -> String {
    let prec = 1;
    let mut rendered = String::new();
    for (idx, term) in args.iter().rev().enumerate() {
        let (is_negative, term_str) = format_term_with_sign(term, prec);
        if idx == 0 {
            if is_negative {
                rendered.push('-');
            }
            rendered.push_str(&term_str);
            continue;
        }
        rendered.push_str(if is_negative { " - " } else { " + " });
        rendered.push_str(&term_str);
    }
    if prec < parent_prec {
        format!("({rendered})")
    } else {
        rendered
    }
}

fn format_raw_op(value: &Value) -> String {
    let Some((op, args)) = value.cas_op_parts() else {
        return format_atom(value);
    };
    let rendered_args = args
        .iter()
        .map(|arg| format_expr(arg, 0))
        .collect::<Vec<_>>();
    format!("{}[{}]", op.symbol(), rendered_args.join(";"))
}

pub(super) fn format_expr(value: &Value, parent_prec: u8) -> String {
    if let Some(name) = value.cas_var_name() {
        return name.to_string();
    }
    if let Some(konst) = value.cas_const() {
        return match konst {
            CasConst::Infinity => "∞".to_string(),
            CasConst::NegInfinity => "-∞".to_string(),
            _ => konst.name().to_string(),
        };
    }
    if let Some((op, args)) = value.cas_known_op_parts() {
        return match (op, args) {
            (CasOp::Add, args) => format_sum(args, parent_prec),
            (CasOp::Multiply, args) => {
                let prec = 2;
                if let Some((first, rest)) = args.split_first()
                    && !first.is_cas_expr()
                    && numeric_is_negative(first)
                {
                    let abs = numeric_abs(first);
                    let rendered = if numeric_is_one(&abs) {
                        format!("-{}", format_product_parts(None, rest, prec))
                    } else {
                        format!("-{}", format_product_parts(Some(&abs), rest, prec))
                    };
                    if prec < parent_prec {
                        format!("({rendered})")
                    } else {
                        rendered
                    }
                } else {
                    format_product_parts(None, args, parent_prec)
                }
            }
            (CasOp::Power, [base, exp]) => format_power(base, exp, parent_prec),
            _ => format_raw_op(value),
        };
    }
    if let Some((expr, var, point, direction)) = value.cas_limit_parts() {
        let mut rendered = format!(
            "limit[{};{};{}",
            format_expr(expr, 0),
            format_expr(var, 0),
            format_expr(point, 0),
        );
        if let Some(dir) = direction {
            let tag = match dir {
                LimitDirection::Right => "+",
                LimitDirection::Left => "-",
            };
            rendered.push_str(&format!(";{tag}"));
        }
        rendered.push(']');
        return rendered;
    }
    if let Some((name, args)) = value.cas_function_parts() {
        let mut rendered_args = Vec::with_capacity(args.len());
        for arg in args {
            rendered_args.push(format_expr(arg, 0));
        }
        return format!("{}[{}]", name.name(), rendered_args.join(";"));
    }
    if let Some((name, args)) = value.cas_apply_parts() {
        let mut rendered_args = Vec::with_capacity(args.len());
        for arg in args {
            rendered_args.push(format_expr(arg, 0));
        }
        return format!("{}[{}]", name.as_str(), rendered_args.join(";"));
    }
    format_atom(value)
}
