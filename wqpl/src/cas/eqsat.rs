use egg::{AstSize, Extractor, Id, RecExpr, Rewrite, Runner, Symbol, define_language};
use num_traits::{One, ToPrimitive};

use super::{cas_err, cas_product, expand_expr, simplify_cas_value};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasFunction, CasOp};
use crate::value::{Value, WqResult};

define_language! {
    enum EqSatLang {
        Num(i64),
        Sym(Symbol),
        "+" = Add([Id; 2]),
        "*" = Mul([Id; 2]),
        "^" = Pow([Id; 2]),
        "ln" = Ln(Id),
        "abs" = Abs(Id),
        "sin" = Sin(Id),
        "cos" = Cos(Id),
        "tan" = Tan(Id),
        "arcsin" = ArcSin(Id),
        "arccos" = ArcCos(Id),
        "arctan" = ArcTan(Id),
    }
}

#[derive(Default)]
struct ConvertCtx {
    literals: Vec<Value>,
}

impl ConvertCtx {
    fn literal_id(&mut self, value: &Value) -> usize {
        if let Some(idx) = self.literals.iter().position(|existing| existing == value) {
            idx
        } else {
            let idx = self.literals.len();
            self.literals.push(value.clone());
            idx
        }
    }
}

const MAX_EGG_INPUT_NODES: usize = 80;

pub(super) fn rewrite_with_egg(value: &Value) -> WqResult<Option<Value>> {
    if !egg_rewrite_may_help(value) {
        return Ok(None);
    }
    let mut ctx = ConvertCtx::default();
    let mut expr = RecExpr::default();
    let Some(root) = value_to_recexpr(value, &mut expr, &mut ctx)? else {
        return Ok(None);
    };
    if expr.as_ref().len() > MAX_EGG_INPUT_NODES {
        return Ok(None);
    }

    let runner = Runner::default()
        .with_expr(&expr)
        .with_iter_limit(8)
        .with_node_limit(10_000)
        .run(&rules());
    let extractor = Extractor::new(&runner.egraph, AstSize);
    let (cost, best) = extractor.find_best(runner.roots[0]);
    let rewritten = recexpr_to_value(&best, &ctx)?;
    let rewritten = simplify_cas_value(&rewritten)?;
    let rewritten = normalize_common_factorization(value, rewritten)?;

    if !should_accept_rewrite(value, &rewritten) {
        cas_trace!(
            DebugLogFlags::CAS_VERBOSE,
            "[cas-v] egg rewrite rejected root={root:?} cost={cost:?}: {} -> {}",
            value.format_cas().unwrap_or_else(|| value.to_string()),
            rewritten
                .format_cas()
                .unwrap_or_else(|| rewritten.to_string())
        );
        return Ok(None);
    }

    cas_trace!(
        DebugLogFlags::CAS_VERBOSE,
        "[cas-v] egg rewrite root={root:?} cost={cost:?}: {} -> {}",
        value.format_cas().unwrap_or_else(|| value.to_string()),
        rewritten
            .format_cas()
            .unwrap_or_else(|| rewritten.to_string())
    );
    Ok(Some(rewritten))
}

fn egg_rewrite_may_help(value: &Value) -> bool {
    if egg_rewrite_rule_may_apply(value) {
        return true;
    }
    if let Some((_op, args)) = value.cas_op_parts() {
        return args.iter().any(egg_rewrite_may_help);
    }
    value
        .cas_function_parts()
        .is_some_and(|(_name, args)| args.iter().any(egg_rewrite_may_help))
        || value
            .cas_apply_parts()
            .is_some_and(|(_name, args)| args.iter().any(egg_rewrite_may_help))
        || value
            .cas_eq_parts()
            .is_some_and(|(lhs, rhs)| egg_rewrite_may_help(lhs) || egg_rewrite_may_help(rhs))
}

fn egg_rewrite_rule_may_apply(value: &Value) -> bool {
    if let Some((op, args)) = value.cas_op_parts() {
        return match op {
            CasOp::Add => {
                args.iter().any(|arg| arg.exact_int_is(0))
                    || add_terms_have_common_factor(args)
                    || args
                        .iter()
                        .filter(|arg| {
                            arg.cas_function_parts()
                                .is_some_and(|(name, _)| name == CasFunction::Ln)
                        })
                        .take(2)
                        .count()
                        >= 2
            }
            CasOp::Multiply => {
                args.iter()
                    .any(|arg| arg.exact_int_is(0) || arg.exact_int_is(1))
                    || multiply_add_distribution_may_shorten(args)
                    || multiply_terms_have_inverse_pair(args)
            }
            CasOp::Power => matches!(args, [base, exp] if exp.exact_int_is(0)
                || exp.exact_int_is(1)
                || (exp.exact_int_is(2)
                    && base
                        .cas_function_parts()
                        .is_some_and(|(name, _)| name == CasFunction::Abs))),
            _ => false,
        };
    }
    if let Some((name, args)) = value.cas_function_parts() {
        return matches!(
            (name, args),
            (CasFunction::Sin, [arg])
                if arg.cas_function_parts().is_some_and(|(inner, _)| inner == CasFunction::ArcSin)
        ) || matches!(
            (name, args),
            (CasFunction::Cos, [arg])
                if arg.cas_function_parts().is_some_and(|(inner, _)| inner == CasFunction::ArcCos)
        ) || matches!(
            (name, args),
            (CasFunction::Tan, [arg])
                if arg.cas_function_parts().is_some_and(|(inner, _)| inner == CasFunction::ArcTan)
        );
    }
    false
}

fn add_terms_have_common_factor(terms: &[Value]) -> bool {
    terms.iter().enumerate().any(|(idx, lhs)| {
        terms
            .iter()
            .skip(idx + 1)
            .any(|rhs| terms_share_factor(lhs, rhs))
    })
}

fn terms_share_factor(lhs: &Value, rhs: &Value) -> bool {
    match (
        lhs.cas_op_args(CasOp::Multiply),
        rhs.cas_op_args(CasOp::Multiply),
    ) {
        (Some(lhs_factors), Some(rhs_factors)) => lhs_factors.iter().any(|lhs_factor| {
            is_factorable_common_factor(lhs_factor)
                && rhs_factors
                    .iter()
                    .any(|rhs_factor| lhs_factor == rhs_factor)
        }),
        (Some(lhs_factors), None) => lhs_factors
            .iter()
            .any(|lhs_factor| lhs_factor == rhs && is_factorable_common_factor(rhs)),
        (None, Some(rhs_factors)) => rhs_factors
            .iter()
            .any(|rhs_factor| lhs == rhs_factor && is_factorable_common_factor(lhs)),
        (None, None) => false,
    }
}

fn is_factorable_common_factor(value: &Value) -> bool {
    !value.exact_int_is(1) && !value.exact_int_is(-1)
}

fn multiply_terms_have_inverse_pair(factors: &[Value]) -> bool {
    factors.iter().enumerate().any(|(idx, factor)| {
        let Some([base, exp]) = factor.cas_op_args(CasOp::Power) else {
            return false;
        };
        exp.exact_int_is(-1)
            && factors
                .iter()
                .enumerate()
                .any(|(other_idx, other)| idx != other_idx && other == base)
    })
}

fn multiply_add_distribution_may_shorten(factors: &[Value]) -> bool {
    factors.iter().enumerate().any(|(idx, factor)| {
        let Some(add_terms) = factor.cas_op_args(CasOp::Add) else {
            return false;
        };
        factors
            .iter()
            .enumerate()
            .filter(|(other_idx, _)| idx != *other_idx)
            .any(|(_, other)| {
                add_terms
                    .iter()
                    .any(|term| term_has_factor_cancelling(term, other))
            })
    })
}

fn term_has_factor_cancelling(term: &Value, factor: &Value) -> bool {
    if let Some(term_factors) = term.cas_op_args(CasOp::Multiply) {
        term_factors
            .iter()
            .any(|term_factor| factors_cancel(term_factor, factor))
    } else {
        factors_cancel(term, factor)
    }
}

fn factors_cancel(lhs: &Value, rhs: &Value) -> bool {
    is_inverse_of(lhs, rhs) || is_inverse_of(rhs, lhs)
}

fn is_inverse_of(value: &Value, base: &Value) -> bool {
    matches!(value.cas_op_args(CasOp::Power), Some([pow_base, exp]) if exp.exact_int_is(-1) && pow_base == base)
}

fn should_accept_rewrite(original: &Value, rewritten: &Value) -> bool {
    if rewritten == original {
        return false;
    }

    if let Some(inner_terms) = common_factorization_inner_terms(original, rewritten) {
        return !inner_terms.iter().any(contains_fractional_numeric);
    }

    let original_text = original
        .format_cas()
        .unwrap_or_else(|| original.to_string());
    let rewritten_text = rewritten
        .format_cas()
        .unwrap_or_else(|| rewritten.to_string());
    rewritten_text.len().saturating_add(4) < original_text.len()
}

fn normalize_common_factorization(original: &Value, rewritten: Value) -> WqResult<Value> {
    if common_factorization_inner_terms(original, &rewritten).is_none() {
        return Ok(rewritten);
    }
    let Some((CasOp::Multiply, factors)) = rewritten.cas_op_parts() else {
        return Ok(rewritten);
    };
    let mut changed = false;
    let mut normalized = Vec::with_capacity(factors.len());
    for factor in factors {
        if matches!(factor.cas_op_parts(), Some((CasOp::Add, _))) {
            let expanded = simplify_cas_value(&expand_expr(factor)?)?;
            changed |= expanded != *factor;
            normalized.push(expanded);
        } else {
            normalized.push(factor.clone());
        }
    }
    if changed {
        simplify_cas_value(&cas_product(normalized))
    } else {
        Ok(rewritten)
    }
}

fn common_factorization_inner_terms<'a>(
    original: &Value,
    rewritten: &'a Value,
) -> Option<&'a [Value]> {
    let Some((CasOp::Add, _)) = original.cas_op_parts() else {
        return None;
    };
    let Some((CasOp::Multiply, factors)) = rewritten.cas_op_parts() else {
        return None;
    };
    let inner_terms = factors.iter().find_map(|factor| {
        if let Some((CasOp::Add, terms)) = factor.cas_op_parts() {
            Some(terms)
        } else {
            None
        }
    })?;
    if !factors.iter().any(|factor| {
        factor.cas_var_name().is_some()
            || factor.cas_function_parts().is_some()
            || factor.cas_apply_parts().is_some()
            || matches!(factor.cas_op_parts(), Some((CasOp::Power, _)))
    }) {
        return None;
    }
    Some(inner_terms)
}

fn contains_fractional_numeric(value: &Value) -> bool {
    if let Some((_, denom)) = value.rational_parts()
        && !denom.is_one()
    {
        return true;
    }
    if matches!(value, Value::Float(_)) {
        return true;
    }
    if let Some((_, args)) = value.cas_op_parts() {
        return args.iter().any(contains_fractional_numeric);
    }
    if let Some((_, args)) = value.cas_function_parts() {
        return args.iter().any(contains_fractional_numeric);
    }
    if let Some((_, args)) = value.cas_apply_parts() {
        return args.iter().any(contains_fractional_numeric);
    }
    false
}

fn rules() -> Vec<Rewrite<EqSatLang, ()>> {
    vec![
        egg::rewrite!("add-0-r"; "(+ ?a 0)" => "?a"),
        egg::rewrite!("add-0-l"; "(+ 0 ?a)" => "?a"),
        egg::rewrite!("mul-1-r"; "(* ?a 1)" => "?a"),
        egg::rewrite!("mul-1-l"; "(* 1 ?a)" => "?a"),
        egg::rewrite!("mul-0-r"; "(* ?a 0)" => "0"),
        egg::rewrite!("mul-0-l"; "(* 0 ?a)" => "0"),
        egg::rewrite!("pow-1"; "(^ ?a 1)" => "?a"),
        egg::rewrite!("pow-0"; "(^ ?a 0)" => "1"),
        egg::rewrite!("factor-left"; "(+ (* ?a ?b) (* ?a ?c))" => "(* ?a (+ ?b ?c))"),
        egg::rewrite!("factor-right"; "(+ (* ?b ?a) (* ?c ?a))" => "(* ?a (+ ?b ?c))"),
        egg::rewrite!("factor-mixed-left"; "(+ (* ?a ?b) (* ?c ?a))" => "(* ?a (+ ?b ?c))"),
        egg::rewrite!("factor-mixed-right"; "(+ (* ?b ?a) (* ?a ?c))" => "(* ?a (+ ?b ?c))"),
        egg::rewrite!("factor-left-unit"; "(+ (* ?a ?b) ?a)" => "(* ?a (+ ?b 1))"),
        egg::rewrite!("factor-right-unit"; "(+ ?a (* ?a ?b))" => "(* ?a (+ 1 ?b))"),
        egg::rewrite!("factor-mixed-left-unit"; "(+ (* ?b ?a) ?a)" => "(* ?a (+ ?b 1))"),
        egg::rewrite!("factor-mixed-right-unit"; "(+ ?a (* ?b ?a))" => "(* ?a (+ 1 ?b))"),
        egg::rewrite!("distribute-left"; "(* ?a (+ ?b ?c))" => "(+ (* ?a ?b) (* ?a ?c))"),
        egg::rewrite!("distribute-right"; "(* (+ ?b ?c) ?a)" => "(+ (* ?b ?a) (* ?c ?a))"),
        egg::rewrite!("cancel-inv-r"; "(* ?a (^ ?a -1))" => "1"),
        egg::rewrite!("cancel-inv-l"; "(* (^ ?a -1) ?a)" => "1"),
        egg::rewrite!("ln-mul"; "(+ (ln ?a) (ln ?b))" => "(ln (* ?a ?b))"),
        egg::rewrite!("sin-arcsin"; "(sin (arcsin ?a))" => "?a"),
        egg::rewrite!("cos-arccos"; "(cos (arccos ?a))" => "?a"),
        egg::rewrite!("tan-arctan"; "(tan (arctan ?a))" => "?a"),
        egg::rewrite!("abs-square"; "(^ (abs ?a) 2)" => "(^ ?a 2)"),
    ]
}

fn value_to_recexpr(
    value: &Value,
    expr: &mut RecExpr<EqSatLang>,
    ctx: &mut ConvertCtx,
) -> WqResult<Option<Id>> {
    if let Some(name) = value.cas_var_name() {
        return Ok(Some(expr.add(EqSatLang::Sym(format!("v:{name}").into()))));
    }
    if let Some(name) = value.cas_const_name() {
        return Ok(Some(expr.add(EqSatLang::Sym(format!("c:{name}").into()))));
    }
    if let Some((op, args)) = value.cas_op_parts() {
        return match (op, args) {
            (CasOp::Add, args) => fold_binary(args, EqSatLang::Num(0), EqSatLang::Add, expr, ctx),
            (CasOp::Multiply, args) => {
                fold_binary(args, EqSatLang::Num(1), EqSatLang::Mul, expr, ctx)
            }
            (CasOp::Power, [base, exp]) => {
                let Some(base) = value_to_recexpr(base, expr, ctx)? else {
                    return Ok(None);
                };
                let Some(exp) = value_to_recexpr(exp, expr, ctx)? else {
                    return Ok(None);
                };
                Ok(Some(expr.add(EqSatLang::Pow([base, exp]))))
            }
            _ => Ok(None),
        };
    }
    if let Some((name, args)) = value.cas_function_parts() {
        let [arg] = args else {
            return Ok(None);
        };
        let Some(arg) = value_to_recexpr(arg, expr, ctx)? else {
            return Ok(None);
        };
        let node = match name {
            CasFunction::Ln => EqSatLang::Ln(arg),
            CasFunction::Abs => EqSatLang::Abs(arg),
            CasFunction::Sin => EqSatLang::Sin(arg),
            CasFunction::Cos => EqSatLang::Cos(arg),
            CasFunction::Tan => EqSatLang::Tan(arg),
            CasFunction::ArcSin => EqSatLang::ArcSin(arg),
            CasFunction::ArcCos => EqSatLang::ArcCos(arg),
            CasFunction::ArcTan => EqSatLang::ArcTan(arg),
            _ => return Ok(None),
        };
        return Ok(Some(expr.add(node)));
    }
    if value.cas_eq_parts().is_some() {
        return Ok(None);
    }
    if let Some(n) = value.exact_int().and_then(|n| n.to_i64()) {
        return Ok(Some(expr.add(EqSatLang::Num(n))));
    }

    let idx = ctx.literal_id(value);
    Ok(Some(expr.add(EqSatLang::Sym(format!("lit:{idx}").into()))))
}

fn fold_binary(
    args: &[Value],
    empty: EqSatLang,
    make_node: fn([Id; 2]) -> EqSatLang,
    expr: &mut RecExpr<EqSatLang>,
    ctx: &mut ConvertCtx,
) -> WqResult<Option<Id>> {
    let Some((first, rest)) = args.split_first() else {
        return Ok(Some(expr.add(empty)));
    };
    let Some(mut acc) = value_to_recexpr(first, expr, ctx)? else {
        return Ok(None);
    };
    for arg in rest {
        let Some(rhs) = value_to_recexpr(arg, expr, ctx)? else {
            return Ok(None);
        };
        acc = expr.add(make_node([acc, rhs]));
    }
    Ok(Some(acc))
}

fn recexpr_to_value(expr: &RecExpr<EqSatLang>, ctx: &ConvertCtx) -> WqResult<Value> {
    let mut values = Vec::with_capacity(expr.as_ref().len());
    for node in expr.as_ref() {
        let value = match node {
            EqSatLang::Num(n) => Value::Int(*n),
            EqSatLang::Sym(sym) => sym_to_value(sym, ctx)?,
            EqSatLang::Add([lhs, rhs]) => Value::from_cas_op(
                CasOp::Add,
                vec![child(&values, *lhs)?, child(&values, *rhs)?],
            ),
            EqSatLang::Mul([lhs, rhs]) => Value::from_cas_op(
                CasOp::Multiply,
                vec![child(&values, *lhs)?, child(&values, *rhs)?],
            ),
            EqSatLang::Pow([base, exp]) => Value::from_cas_op(
                CasOp::Power,
                vec![child(&values, *base)?, child(&values, *exp)?],
            ),
            EqSatLang::Ln(arg) => {
                Value::from_cas_function(CasFunction::Ln, vec![child(&values, *arg)?])
            }
            EqSatLang::Abs(arg) => {
                Value::from_cas_function(CasFunction::Abs, vec![child(&values, *arg)?])
            }
            EqSatLang::Sin(arg) => {
                Value::from_cas_function(CasFunction::Sin, vec![child(&values, *arg)?])
            }
            EqSatLang::Cos(arg) => {
                Value::from_cas_function(CasFunction::Cos, vec![child(&values, *arg)?])
            }
            EqSatLang::Tan(arg) => {
                Value::from_cas_function(CasFunction::Tan, vec![child(&values, *arg)?])
            }
            EqSatLang::ArcSin(arg) => {
                Value::from_cas_function(CasFunction::ArcSin, vec![child(&values, *arg)?])
            }
            EqSatLang::ArcCos(arg) => {
                Value::from_cas_function(CasFunction::ArcCos, vec![child(&values, *arg)?])
            }
            EqSatLang::ArcTan(arg) => {
                Value::from_cas_function(CasFunction::ArcTan, vec![child(&values, *arg)?])
            }
        };
        values.push(value);
    }
    values
        .pop()
        .ok_or_else(|| cas_err("egg extraction produced an empty expression"))
}

fn child(values: &[Value], id: Id) -> WqResult<Value> {
    let idx: usize = id.into();
    values
        .get(idx)
        .cloned()
        .ok_or_else(|| cas_err("egg extraction referenced an invalid child"))
}

fn sym_to_value(sym: &Symbol, ctx: &ConvertCtx) -> WqResult<Value> {
    let text = sym.to_string();
    if let Some(name) = text.strip_prefix("v:") {
        return Ok(Value::from_cas_var(name));
    }
    if let Some(name) = text.strip_prefix("c:") {
        let konst = crate::value::cas::CasConst::from_name(name)
            .ok_or_else(|| cas_err("egg extraction produced an invalid constant"))?;
        return Ok(Value::from_cas_const(konst));
    }
    if let Some(idx) = text.strip_prefix("lit:") {
        let idx = idx
            .parse::<usize>()
            .map_err(|_| cas_err("egg extraction produced an invalid literal id"))?;
        return ctx
            .literals
            .get(idx)
            .cloned()
            .ok_or_else(|| cas_err("egg extraction referenced an unknown literal"));
    }
    Ok(Value::from_cas_var(text))
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;

    #[test]
    fn egg_rewrite_factors_common_product() {
        let expr = Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(
                    CasOp::Multiply,
                    vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
                ),
                Value::from_cas_op(
                    CasOp::Multiply,
                    vec![Value::from_cas_var("x"), Value::from_cas_var("z")],
                ),
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        let rewritten = rewrite_with_egg(&simplified)
            .expect("egg rewrite")
            .expect("expected rewrite");
        let text = rewritten.to_string();
        assert!(
            text == "x*(y + z)" || text == "x*(z + y)",
            "unexpected factored form: {text}"
        );
    }

    #[test]
    fn egg_rewrite_factors_product_plus_bare_factor() {
        let common = Value::from_cas_op(
            CasOp::Multiply,
            vec![Value::from_cas_var("a"), Value::from_cas_var("b")],
        );
        let expr = Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(
                    CasOp::Multiply,
                    vec![
                        common.clone(),
                        Value::from_cas_op(
                            CasOp::Power,
                            vec![Value::from_cas_var("x"), Value::Int(3)],
                        ),
                    ],
                ),
                common,
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        let rewritten = rewrite_with_egg(&simplified)
            .expect("egg rewrite")
            .expect("expected rewrite");
        let text = rewritten.to_string();
        assert!(
            text.contains("x^3 + 1") && text.contains("a") && text.contains("b"),
            "unexpected factored form: {text}"
        );
    }

    #[test]
    fn egg_rewrite_interns_repeated_literals() {
        let common = Value::from_cas_op(
            CasOp::Multiply,
            vec![
                Value::from_fraction_parts(BigInt::from(3), BigInt::from(5)),
                Value::from_cas_op(
                    CasOp::Power,
                    vec![
                        Value::Int(3),
                        Value::from_fraction_parts(BigInt::from(1), BigInt::from(4)),
                    ],
                ),
                Value::from_cas_var("z"),
            ],
        );
        let expr = Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(
                    CasOp::Multiply,
                    vec![
                        common.clone(),
                        Value::from_cas_op(
                            CasOp::Power,
                            vec![Value::from_cas_var("x"), Value::Int(3)],
                        ),
                    ],
                ),
                common,
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        let rewritten = rewrite_with_egg(&simplified)
            .expect("egg rewrite")
            .expect("expected rewrite");
        let text = rewritten.to_string();
        assert!(
            text.contains("x^3 + 1") && text.contains("3/5") && text.contains("3^(1/4)"),
            "unexpected factored form: {text}"
        );
    }

    #[test]
    fn egg_prefilter_skips_non_factorable_nested_power_sum() {
        let expr = Value::from_cas_op(
            CasOp::Multiply,
            vec![
                Value::from_fraction_parts(BigInt::from(2), BigInt::from(9)),
                Value::from_cas_op(
                    CasOp::Power,
                    vec![
                        Value::from_cas_op(
                            CasOp::Add,
                            vec![
                                Value::from_cas_op(
                                    CasOp::Power,
                                    vec![Value::from_cas_var("x"), Value::Int(3)],
                                ),
                                Value::Int(1),
                            ],
                        ),
                        Value::from_fraction_parts(BigInt::from(3), BigInt::from(2)),
                    ],
                ),
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        assert!(
            !egg_rewrite_may_help(&simplified),
            "non-factorable nested power sum should not enter egg: {simplified}"
        );
    }

    #[test]
    fn egg_prefilter_keeps_nested_inverse_trig_rewrite() {
        let expr = Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::Int(1),
                Value::from_cas_function(
                    CasFunction::Sin,
                    vec![Value::from_cas_function(
                        CasFunction::ArcSin,
                        vec![Value::from_cas_var("x")],
                    )],
                ),
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        assert!(
            egg_rewrite_may_help(&simplified),
            "nested inverse trig rewrite should still enter egg: {simplified}"
        );
    }

    #[test]
    fn egg_prefilter_skips_uncancellable_quotient_with_adds() {
        let expr = Value::from_cas_op(
            CasOp::Multiply,
            vec![
                Value::from_cas_op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
                Value::from_cas_op(
                    CasOp::Power,
                    vec![
                        Value::from_cas_op(
                            CasOp::Add,
                            vec![Value::from_cas_var("x"), Value::Int(2)],
                        ),
                        Value::Int(-1),
                    ],
                ),
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        assert!(
            !egg_rewrite_may_help(&simplified),
            "uncancellable quotient should not enter egg: {simplified}"
        );
    }

    #[test]
    fn egg_prefilter_skips_unit_common_factor() {
        let expr = Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(
                    CasOp::Multiply,
                    vec![Value::Int(-1), Value::from_cas_var("x")],
                ),
                Value::Int(-1),
            ],
        );

        assert!(
            !egg_rewrite_may_help(&expr),
            "unit common factor should not enter egg: {expr}"
        );
    }

    #[test]
    fn egg_prefilter_keeps_distribution_with_cancellation() {
        let expr = Value::from_cas_op(
            CasOp::Multiply,
            vec![
                Value::from_cas_var("x"),
                Value::from_cas_op(
                    CasOp::Add,
                    vec![
                        Value::Int(1),
                        Value::from_cas_op(
                            CasOp::Multiply,
                            vec![
                                Value::from_cas_var("y"),
                                Value::from_cas_op(
                                    CasOp::Power,
                                    vec![Value::from_cas_var("x"), Value::Int(-1)],
                                ),
                            ],
                        ),
                    ],
                ),
            ],
        );

        let simplified = simplify_cas_value(&expr).expect("simplify");
        assert!(
            egg_rewrite_may_help(&simplified),
            "distribution with cancellation should still enter egg: {simplified}"
        );
    }
}
