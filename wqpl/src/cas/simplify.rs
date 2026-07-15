use std::cell::RefCell;
use std::sync::Arc;

use ahash::AHashMap;
use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::rewrite::{is_provably_positive, push_flattened};
use super::{
    cas_err, cas_product, collect_single_poly_var, contains_cas_var, ensure_expr_arg,
    eval_exact_numeric_div, eval_numeric_binary_gcd, eval_numeric_call, eval_numeric_cas,
    expand_expr, extract_algebraic_content, extract_linear_coefficients, factor_expr, numeric_add,
    numeric_is_negative, numeric_is_one, numeric_is_zero, numeric_mul, numeric_pow, poly_add,
    poly_degree, poly_divide, poly_from_expr, poly_gcd, poly_is_zero, poly_mul, poly_to_expr,
    poly_trim, resolve_cas_root, sort_canonical, split_off_results, square_free_factor,
    try_cancel_affine_over_factor, try_eval_with_const_resolve, try_exact_polynomial_division,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasConst, CasFunction, CasOp, CasSymbol};
use crate::value::{Value, WqResult};

/// Stack frame for iterative simplify.
enum SimplifyFrame {
    Expr(Value),
    Add(usize),
    Mul(usize),
    Pow,
    Div,
    Neg,
    Sub,
    Function {
        function: CasFunction,
        n: usize,
    },
    Apply {
        name: CasSymbol,
        n: usize,
    },
    NamedArg {
        name: CasSymbol,
    },
    Limit {
        var: Value,
        direction: Option<crate::cas::limit::LimitDirection>,
    },
    Eq,
}

#[derive(Default)]
struct CasDivCache {
    entries: AHashMap<(Value, Value), Value>,
}

thread_local! {
    static CAS_DIV_CACHE: RefCell<Option<CasDivCache>> = const { RefCell::new(None) };
}

struct CasDivCacheScope {
    owns_cache: bool,
}

impl CasDivCacheScope {
    fn enter() -> Self {
        let owns_cache = CAS_DIV_CACHE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if slot.is_some() {
                false
            } else {
                *slot = Some(CasDivCache::default());
                true
            }
        });
        Self { owns_cache }
    }
}

impl Drop for CasDivCacheScope {
    fn drop(&mut self) {
        if self.owns_cache {
            CAS_DIV_CACHE.with(|slot| {
                *slot.borrow_mut() = None;
            });
        }
    }
}

pub(crate) fn with_cas_div_cache<T>(f: impl FnOnce() -> T) -> T {
    let _scope = CasDivCacheScope::enter();
    f()
}

fn cas_div_cache_get(lhs: &Value, rhs: &Value) -> Option<Value> {
    CAS_DIV_CACHE.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|cache| cache.entries.get(&(lhs.clone(), rhs.clone())).cloned())
    })
}

fn cas_div_cache_insert(lhs: Value, rhs: Value, result: Value) {
    CAS_DIV_CACHE.with(|slot| {
        if let Some(cache) = slot.borrow_mut().as_mut() {
            cache.entries.insert((lhs, rhs), result);
        }
    });
}

pub(crate) fn cas_neg(arg: Value) -> WqResult<Value> {
    let arg = simplify_cas_value(&arg)?;
    match arg.cas_const() {
        Some(CasConst::Infinity) => return Ok(Value::from_cas_const(CasConst::NegInfinity)),
        Some(CasConst::NegInfinity) => return Ok(Value::from_cas_const(CasConst::Infinity)),
        _ => {}
    }
    cas_mul(vec![Value::Int(-1), arg])
}

pub(crate) fn cas_sub(lhs: Value, rhs: Value) -> WqResult<Value> {
    cas_add(vec![lhs, cas_neg(rhs)?])
}

pub(crate) fn cas_div(lhs: Value, rhs: Value) -> WqResult<Value> {
    with_cas_div_cache(|| cas_div_cached(lhs, rhs))
}

fn cas_div_cached(lhs: Value, rhs: Value) -> WqResult<Value> {
    if let Some(cached) = cas_div_cache_get(&lhs, &rhs) {
        cas_trace!(
            DebugLogFlags::CAS_VERBOSE,
            "[cas-v] cas_div cache hit: {lhs} / {rhs}"
        );
        return Ok(cached);
    }
    cas_trace!(DebugLogFlags::CAS, "[cas_div] {lhs} / {rhs}");
    let result = cas_div_uncached(lhs.clone(), rhs.clone())?;
    cas_div_cache_insert(lhs, rhs, result.clone());
    Ok(result)
}

fn cas_div_uncached(lhs: Value, rhs: Value) -> WqResult<Value> {
    let lhs = simplify_cas_value(&lhs)?;
    let rhs = simplify_cas_value(&rhs)?;
    if !lhs.is_cas_expr() && !rhs.is_cas_expr() {
        return eval_exact_numeric_div(&lhs, &rhs);
    }
    if let Some(quotient) = try_exact_polynomial_division(&lhs, &rhs)? {
        return Ok(quotient);
    }
    if let Some(cancelled) = try_cancel_affine_over_factor(&lhs, &rhs)? {
        return Ok(cancelled);
    }
    if !rhs.is_cas_expr() {
        let factored = factor_expr(&lhs)?;
        if let Some(args) = factored.cas_op_args(CasOp::Multiply) {
            let mut numeric = Value::Int(1);
            let mut symbolic = Vec::new();
            for arg in args {
                if !arg.is_cas_expr() {
                    numeric = numeric_mul(&numeric, arg)?;
                } else {
                    symbolic.push(arg.clone());
                }
            }
            if let Ok(new_numeric) = eval_exact_numeric_div(&numeric, &rhs) {
                if numeric_is_one(&new_numeric) && symbolic.len() == 1 {
                    return Ok(symbolic.into_iter().next().unwrap());
                }
                let mut out = Vec::new();
                if !numeric_is_one(&new_numeric) {
                    out.push(new_numeric);
                }
                out.extend(symbolic);
                return cas_mul(out);
            }
        }
    }
    // Try simplifying: C / (A*S + B) = (C/A) / (S + B/A)
    // This clears nested radicals when A involves a reciprocal.
    // Only apply when A is not a trivial integer (avoid rewriting 1/(1-x^2)).
    if let Some(add_args) = rhs.cas_op_args(CasOp::Add)
        && add_args.len() == 2
    {
        for (i, j) in [(0, 1), (1, 0)] {
            let const_term = &add_args[i];
            let other = &add_args[j];
            if !const_term.is_cas_expr()
                && let Some(mul_args) = other.cas_op_args(CasOp::Multiply)
                && let Some((first_mul, rest_mul)) = mul_args.split_first()
                && !first_mul.is_cas_expr()
            {
                // Skip trivial integer coefficients (-1, 1, etc.)
                let is_trivial_int = matches!(first_mul, Value::Int(n) if *n == 1 || *n == -1);
                if !is_trivial_int {
                    let a = first_mul;
                    let s = cas_product(rest_mul.to_vec());
                    let b = const_term;
                    let new_lhs = cas_div(lhs, a.clone())?;
                    let new_rhs_second = cas_div(b.clone(), a.clone())?;
                    let new_rhs = cas_add(vec![s, new_rhs_second])?;
                    return cas_div(new_lhs, new_rhs);
                }
            }
        }
    }
    if !rhs.is_cas_expr() {
        let recip = eval_exact_numeric_div(&Value::Int(1), &rhs)?;
        cas_mul(vec![lhs, recip])
    } else {
        cas_mul(vec![lhs, cas_pow(rhs, Value::Int(-1))?])
    }
}

fn bigint_mod_i64(value: &BigInt, modulus: i64) -> Option<i64> {
    let modulus_big = BigInt::from(modulus);
    let mut rem = value % &modulus_big;
    if rem.is_negative() {
        rem += &modulus_big;
    }
    rem.to_i64()
}

fn exact_pi_multiple(value: &Value) -> Option<Value> {
    if value.cas_const() == Some(CasConst::Pi) {
        return Some(Value::Int(1));
    }

    let (CasOp::Multiply, args) = value.cas_op_parts()? else {
        return None;
    };

    let mut coeff = Value::Int(1);
    let mut found_pi = false;
    for arg in args {
        if arg.cas_const() == Some(CasConst::Pi) {
            if found_pi {
                return None;
            }
            found_pi = true;
        } else if !arg.is_cas_expr() && arg.rational_parts().is_some() {
            coeff = numeric_mul(&coeff, arg).ok()?;
        } else {
            return None;
        }
    }

    found_pi.then_some(coeff)
}

fn exact_trig_value(function: CasFunction, arg: &Value) -> Option<Value> {
    if numeric_is_zero(arg) {
        return match function {
            CasFunction::Sin | CasFunction::Tan => Some(Value::Int(0)),
            CasFunction::Cos => Some(Value::Int(1)),
            _ => None,
        };
    }

    let multiple = exact_pi_multiple(arg)?;
    let (numer, denom) = multiple.rational_parts()?;
    if denom.is_one() {
        return match function {
            CasFunction::Sin | CasFunction::Tan => Some(Value::Int(0)),
            CasFunction::Cos => {
                let parity = bigint_mod_i64(&numer, 2)?;
                Some(Value::Int(if parity == 0 { 1 } else { -1 }))
            }
            _ => None,
        };
    }

    if denom == BigInt::from(2) && bigint_mod_i64(&numer, 2)? == 1 {
        return match function {
            CasFunction::Sin => {
                let quarter = bigint_mod_i64(&numer, 4)?;
                Some(Value::Int(if quarter == 1 { 1 } else { -1 }))
            }
            CasFunction::Cos => Some(Value::Int(0)),
            CasFunction::Tan => Some(Value::from_cas_const(CasConst::Undefined)),
            _ => None,
        };
    }

    None
}

fn exact_function_value(function: CasFunction, args: &[Value]) -> Option<Value> {
    let [arg] = args else {
        return None;
    };
    match function {
        CasFunction::Sin | CasFunction::Cos | CasFunction::Tan => exact_trig_value(function, arg),
        _ => None,
    }
}

fn should_keep_exact_function_symbolic(function: CasFunction, args: &[Value]) -> bool {
    matches!(
        function,
        CasFunction::Sin | CasFunction::Cos | CasFunction::Tan
    ) && matches!(args, [arg] if exact_pi_multiple(arg).is_some())
}

/// Find the greatest common numeric divisor of all terms in a sum.
/// Each term is assumed to be in the form `coeff * core` from `split_add_term`.
/// Returns None if no common factor > 1 exists.
pub(super) fn common_numeric_gcd(terms: &[Value]) -> Option<Value> {
    let mut common: Option<Value> = None;
    for term in terms {
        let (coeff, _) = split_add_term(term);
        if numeric_is_zero(&coeff) {
            continue;
        }
        let abs = if numeric_is_negative(&coeff) {
            numeric_mul(&coeff, &Value::Int(-1)).ok()?
        } else {
            coeff.clone()
        };
        common = Some(match common.take() {
            None => abs,
            Some(prev) => eval_numeric_binary_gcd(&prev, &abs).unwrap_or_else(|_| Value::Int(1)),
        });
    }
    common.filter(|c| !numeric_is_one(c) && !numeric_is_zero(c))
}

pub(super) fn split_add_term(term: &Value) -> (Value, Option<Value>) {
    // Pure numeric term (Int, BigInt, Fraction) -> no symbolic core
    if !term.is_cas_expr() && !term.is_algebraic_number() {
        return (term.clone(), None);
    }
    // Standalone Algebraic value: extract rational content as coefficient,
    // keep the primitive algebraic as core.
    if let Value::Algebraic(a) = term {
        let content = extract_algebraic_content(a);
        if numeric_is_one(&content) {
            return (Value::Int(1), Some(term.clone()));
        }
        if let Ok(reduced) = cas_div(term.clone(), content.clone()) {
            return (content, Some(reduced));
        }
        return (content, None);
    }
    // Product (* a b ...): first non-CAS constant factor becomes the coefficient.
    if let Some(args) = term.cas_op_args(CasOp::Multiply)
        && let Some((first, rest)) = args.split_first()
        && !first.is_cas_expr()
    {
        // If the first factor is Algebraic, peel off its rational content
        if let Value::Algebraic(a) = first {
            let content = extract_algebraic_content(a);
            if !numeric_is_one(&content)
                && let Ok(reduced) = cas_div(first.clone(), content.clone())
            {
                let mut new_args = vec![reduced];
                new_args.extend(rest.iter().cloned());
                let core = cas_product(new_args);
                return (content, Some(core));
            }
        }
        let core = match rest {
            [] => None,
            [single] => Some(single.clone()),
            _ => Some(Value::from_cas_op(CasOp::Multiply, rest.to_vec())),
        };
        return (first.clone(), core);
    }
    // All other cases (+, ^, Call, etc.): the whole expression is the core.
    (Value::Int(1), Some(term.clone()))
}

pub(super) fn rebuild_scaled_term(coeff: Value, core: Option<Value>) -> WqResult<Value> {
    match core {
        None => Ok(coeff),
        Some(core) if numeric_is_one(&coeff) => Ok(core),
        Some(core) => cas_mul(vec![coeff, core]),
    }
}

pub(super) fn split_mul_factor(factor: &Value) -> (Value, Value) {
    if let Some([base, exp]) = factor.cas_op_args(CasOp::Power) {
        return (base.clone(), exp.clone());
    }
    (factor.clone(), Value::Int(1))
}

/// Exact linear form for variable-free radical coefficients such as
/// `2^(1/5)*(5^(1/2)+1)`, used before polynomial rational recombination.
#[derive(Clone, PartialEq, Eq)]
struct RadicalFactor {
    base: Value,
    denom: BigInt,
    exp: BigInt,
}

#[derive(Clone, PartialEq, Eq)]
struct RadicalMono {
    factors: Vec<RadicalFactor>,
}

struct RadicalLinear {
    terms: Vec<(RadicalMono, Value)>,
}

impl RadicalMono {
    fn one() -> Self {
        Self {
            factors: Vec::new(),
        }
    }
}

impl RadicalLinear {
    fn zero() -> Self {
        Self { terms: Vec::new() }
    }

    fn one() -> Self {
        Self::constant(Value::Int(1))
    }

    fn constant(coeff: Value) -> Self {
        if numeric_is_zero(&coeff) {
            Self::zero()
        } else {
            Self {
                terms: vec![(RadicalMono::one(), coeff)],
            }
        }
    }

    fn single(mono: RadicalMono, coeff: Value) -> Self {
        if numeric_is_zero(&coeff) {
            Self::zero()
        } else {
            Self {
                terms: vec![(mono, coeff)],
            }
        }
    }

    fn add_term(&mut self, mono: RadicalMono, coeff: Value) -> WqResult<()> {
        if numeric_is_zero(&coeff) {
            return Ok(());
        }
        if let Some(i) = self
            .terms
            .iter()
            .position(|(existing_mono, _)| existing_mono == &mono)
        {
            let new_coeff = numeric_add(&self.terms[i].1, &coeff)?;
            if numeric_is_zero(&new_coeff) {
                self.terms.remove(i);
            } else {
                self.terms[i].1 = new_coeff;
            }
            return Ok(());
        }
        self.terms.push((mono, coeff));
        Ok(())
    }

    fn add(&self, rhs: &Self) -> WqResult<Self> {
        let mut out = Self::zero();
        for (mono, coeff) in self.terms.iter().chain(rhs.terms.iter()) {
            out.add_term(mono.clone(), coeff.clone())?;
        }
        Ok(out)
    }

    fn mul(&self, rhs: &Self) -> WqResult<Option<Self>> {
        let mut out = Self::zero();
        for (lhs_mono, lhs_coeff) in &self.terms {
            for (rhs_mono, rhs_coeff) in &rhs.terms {
                let Some((mono, scale)) = multiply_radical_monomials(lhs_mono, rhs_mono)? else {
                    return Ok(None);
                };
                let coeff = numeric_mul(&numeric_mul(lhs_coeff, rhs_coeff)?, &scale)?;
                out.add_term(mono, coeff)?;
            }
        }
        Ok(Some(out))
    }
}

fn bigint_floor_div_rem(numer: &BigInt, denom: &BigInt) -> (BigInt, BigInt) {
    let quotient = numer / denom;
    let remainder = numer % denom;
    if remainder.is_negative() {
        (quotient - BigInt::one(), remainder + denom)
    } else {
        (quotient, remainder)
    }
}

fn rational_integer_power(value: &Value, exp: &BigInt) -> Option<Value> {
    if exp.is_zero() {
        return Some(Value::Int(1));
    }
    let (numer, denom) = value.rational_parts()?;
    if exp.is_negative() {
        if numer.is_zero() {
            return None;
        }
        let power = (-exp).to_u32()?;
        Some(Value::from_fraction_parts(
            denom.pow(power),
            numer.pow(power),
        ))
    } else {
        let power = exp.to_u32()?;
        Some(Value::from_fraction_parts(
            numer.pow(power),
            denom.pow(power),
        ))
    }
}

fn normalize_radical_factor(
    base: Value,
    denom: BigInt,
    exp: BigInt,
) -> WqResult<Option<(RadicalFactor, Value)>> {
    if denom.is_zero() {
        return Ok(None);
    }
    let denom = denom.abs();
    let (whole, rem) = bigint_floor_div_rem(&exp, &denom);
    let Some(scale) = rational_integer_power(&base, &whole) else {
        return Ok(None);
    };
    if rem.is_zero() {
        return Ok(Some((
            RadicalFactor {
                base,
                denom,
                exp: BigInt::zero(),
            },
            scale,
        )));
    }
    Ok(Some((
        RadicalFactor {
            base,
            denom,
            exp: rem,
        },
        scale,
    )))
}

fn push_radical_factor(
    factors: &mut Vec<RadicalFactor>,
    factor: RadicalFactor,
    scale: &mut Value,
) -> WqResult<Option<()>> {
    if factor.exp.is_zero() {
        return Ok(Some(()));
    }
    if let Some(pos) = factors
        .iter()
        .position(|existing| existing.base == factor.base && existing.denom == factor.denom)
    {
        let existing = factors.remove(pos);
        let Some((normalized, factor_scale)) =
            normalize_radical_factor(existing.base, existing.denom, existing.exp + factor.exp)?
        else {
            return Ok(None);
        };
        *scale = numeric_mul(scale, &factor_scale)?;
        if !normalized.exp.is_zero() {
            factors.push(normalized);
        }
    } else {
        let Some((normalized, factor_scale)) =
            normalize_radical_factor(factor.base, factor.denom, factor.exp)?
        else {
            return Ok(None);
        };
        *scale = numeric_mul(scale, &factor_scale)?;
        if !normalized.exp.is_zero() {
            factors.push(normalized);
        }
    }
    factors.sort_by_cached_key(|factor| {
        (
            factor.base.to_string(),
            factor.denom.to_string(),
            factor.exp.to_string(),
        )
    });
    Ok(Some(()))
}

fn multiply_radical_monomials(
    lhs: &RadicalMono,
    rhs: &RadicalMono,
) -> WqResult<Option<(RadicalMono, Value)>> {
    let mut factors = lhs.factors.clone();
    let mut scale = Value::Int(1);
    for factor in &rhs.factors {
        if push_radical_factor(&mut factors, factor.clone(), &mut scale)?.is_none() {
            return Ok(None);
        }
    }
    Ok(Some((RadicalMono { factors }, scale)))
}

fn radical_from_rational_power(
    base: &Value,
    numer: BigInt,
    denom: BigInt,
) -> WqResult<Option<RadicalLinear>> {
    if denom.is_zero() || base.rational_parts().is_none() {
        return Ok(None);
    }
    if denom.is_one() {
        let Some(coeff) = rational_integer_power(base, &numer) else {
            return Ok(None);
        };
        return Ok(Some(RadicalLinear::constant(coeff)));
    }
    if numeric_is_negative(base) && (&denom % BigInt::from(2)).is_zero() {
        return Ok(None);
    }
    let Some((factor, scale)) = normalize_radical_factor(base.clone(), denom, numer)? else {
        return Ok(None);
    };
    if factor.exp.is_zero() {
        return Ok(Some(RadicalLinear::constant(scale)));
    }
    Ok(Some(RadicalLinear::single(
        RadicalMono {
            factors: vec![factor],
        },
        scale,
    )))
}

fn radical_monomial_pow(
    mono: &RadicalMono,
    numer: &BigInt,
    denom: &BigInt,
) -> WqResult<Option<(RadicalMono, Value)>> {
    let mut out = RadicalMono::one();
    let mut scale = Value::Int(1);
    for factor in &mono.factors {
        let combined_denom = &factor.denom * denom;
        let combined_exp = &factor.exp * numer;
        let Some((normalized, factor_scale)) =
            normalize_radical_factor(factor.base.clone(), combined_denom, combined_exp)?
        else {
            return Ok(None);
        };
        scale = numeric_mul(&scale, &factor_scale)?;
        if push_radical_factor(&mut out.factors, normalized, &mut scale)?.is_none() {
            return Ok(None);
        }
    }
    Ok(Some((out, scale)))
}

fn radical_linear_pow(
    linear: &RadicalLinear,
    numer: &BigInt,
    denom: &BigInt,
) -> WqResult<Option<RadicalLinear>> {
    if linear.terms.len() == 1 {
        let (mono, coeff) = &linear.terms[0];
        let Some(coeff_pow) = radical_from_rational_power(coeff, numer.clone(), denom.clone())?
        else {
            return Ok(None);
        };
        let Some((mono_pow, scale)) = radical_monomial_pow(mono, numer, denom)? else {
            return Ok(None);
        };
        let mono_linear = RadicalLinear::single(mono_pow, scale);
        return coeff_pow.mul(&mono_linear);
    }

    if !denom.is_one() || numer.is_negative() {
        return Ok(None);
    }
    let Some(power) = numer.to_usize() else {
        return Ok(None);
    };
    if power > 8 {
        return Ok(None);
    }
    let mut out = RadicalLinear::one();
    for _ in 0..power {
        let Some(next) = out.mul(linear)? else {
            return Ok(None);
        };
        out = next;
    }
    Ok(Some(out))
}

fn radical_linear_from_algebraic(
    alg: &crate::value::algebraic::AlgebraicData,
) -> WqResult<Option<RadicalLinear>> {
    let deg = alg.degree();
    let poly = alg.poly();
    if deg == 0 || poly[1..deg].iter().any(|coeff| !coeff.is_zero()) {
        return Ok(None);
    }
    let constant = &poly[0];
    let leading = &poly[deg];
    if constant.is_zero() || leading.is_zero() {
        return Ok(None);
    }
    let base = Value::from_fraction_parts(-constant.clone(), leading.clone());
    if numeric_is_negative(&base) {
        return Ok(None);
    }

    let mut out = RadicalLinear::zero();
    for (power, coeff) in alg.coeffs.iter().enumerate() {
        if numeric_is_zero(coeff) {
            continue;
        }
        let Some(coeff_linear) = radical_linear_from_expr(coeff)? else {
            return Ok(None);
        };
        let Some(radical_linear) =
            radical_from_rational_power(&base, BigInt::from(power), BigInt::from(deg))?
        else {
            return Ok(None);
        };
        let Some(term) = coeff_linear.mul(&radical_linear)? else {
            return Ok(None);
        };
        out = out.add(&term)?;
    }
    Ok(Some(out))
}

fn radical_linear_from_expr(expr: &Value) -> WqResult<Option<RadicalLinear>> {
    if let Value::Algebraic(alg) = expr {
        return radical_linear_from_algebraic(alg);
    }
    if !expr.is_cas_expr() {
        return Ok(expr
            .rational_parts()
            .map(|_| RadicalLinear::constant(expr.clone())));
    }
    if expr.cas_var_name().is_some() || expr.cas_const_name().is_some() {
        return Ok(None);
    }
    let Some((op, args)) = expr.cas_known_op_parts() else {
        return Ok(None);
    };
    match (op, args) {
        (CasOp::Add, args) => {
            let mut out = RadicalLinear::zero();
            for arg in args {
                let Some(part) = radical_linear_from_expr(arg)? else {
                    return Ok(None);
                };
                out = out.add(&part)?;
            }
            Ok(Some(out))
        }
        (CasOp::Multiply, args) => {
            let mut out = RadicalLinear::one();
            for arg in args {
                let Some(part) = radical_linear_from_expr(arg)? else {
                    return Ok(None);
                };
                let Some(product) = out.mul(&part)? else {
                    return Ok(None);
                };
                out = product;
            }
            Ok(Some(out))
        }
        (CasOp::Power, [base, exp]) => {
            let Some((numer, denom)) = exp.rational_parts() else {
                return Ok(None);
            };
            if !base.is_cas_expr() {
                return radical_from_rational_power(base, numer, denom);
            }
            let Some(base_linear) = radical_linear_from_expr(base)? else {
                return Ok(None);
            };
            radical_linear_pow(&base_linear, &numer, &denom)
        }
        _ => Ok(None),
    }
}

fn radical_linear_to_value(linear: &RadicalLinear) -> WqResult<Value> {
    let mut terms = Vec::with_capacity(linear.terms.len());
    for (mono, coeff) in &linear.terms {
        let mut factors = Vec::with_capacity(mono.factors.len() + 1);
        if !numeric_is_one(coeff) || mono.factors.is_empty() {
            factors.push(coeff.clone());
        }
        for factor in &mono.factors {
            factors.push(Value::from_cas_op(
                CasOp::Power,
                vec![
                    factor.base.clone(),
                    Value::from_fraction_parts(factor.exp.clone(), factor.denom.clone()),
                ],
            ));
        }
        terms.push(cas_mul(factors)?);
    }
    cas_add(terms)
}

fn normalize_radical_constant(expr: &Value) -> WqResult<Option<Value>> {
    let Some(linear) = radical_linear_from_expr(expr)? else {
        return Ok(None);
    };
    Ok(Some(radical_linear_to_value(&linear)?))
}

fn contains_any_symbolic_var(expr: &Value) -> bool {
    if expr.cas_var_name().is_some() {
        return true;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        return args.iter().any(contains_any_symbolic_var);
    }
    if let Some((_, args)) = expr.cas_function_parts() {
        return args.iter().any(contains_any_symbolic_var);
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        return args.iter().any(contains_any_symbolic_var);
    }
    if let Some((_name, value)) = expr.cas_named_arg_parts() {
        return contains_any_symbolic_var(value);
    }
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        return contains_any_symbolic_var(lhs) || contains_any_symbolic_var(rhs);
    }
    false
}

fn contains_negative_power_expr(expr: &Value) -> bool {
    if let Some([_, exp]) = expr.cas_op_args(CasOp::Power)
        && exp.rational_parts().is_some_and(|(n, _)| n.is_negative())
    {
        return true;
    }
    if let Some((_, args)) = expr.cas_op_parts() {
        return args.iter().any(contains_negative_power_expr);
    }
    if let Some((_, args)) = expr.cas_function_parts() {
        return args.iter().any(contains_negative_power_expr);
    }
    if let Some((_, args)) = expr.cas_apply_parts() {
        return args.iter().any(contains_negative_power_expr);
    }
    if let Some((_name, value)) = expr.cas_named_arg_parts() {
        return contains_negative_power_expr(value);
    }
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        return contains_negative_power_expr(lhs) || contains_negative_power_expr(rhs);
    }
    false
}

fn normalize_relaxed_poly_coeff(expr: &Value) -> WqResult<Value> {
    let simplified = factor_expr(&simplify_cas_value(expr)?)?;
    if simplified.is_cas_expr()
        && let Some(normalized) = normalize_radical_constant(&simplified)?
    {
        factor_expr(&simplify_cas_value(&normalized)?)
    } else {
        Ok(simplified)
    }
}

fn poly_from_expr_relaxed_constants(expr: &Value, var: &str) -> WqResult<Vec<Value>> {
    if let Ok(coeffs) = poly_from_expr(expr, var) {
        return Ok(coeffs);
    }
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
    if !contains_any_symbolic_var(expr) && !contains_negative_power_expr(expr) {
        return Ok(vec![normalize_relaxed_poly_coeff(expr)?]);
    }
    if let Some((op, args)) = expr.cas_known_op_parts() {
        return match (op, args) {
            (CasOp::Add, args) => {
                let mut acc = vec![Value::Int(0)];
                for arg in args {
                    acc = poly_add(&acc, &poly_from_expr_relaxed_constants(arg, var)?)?;
                }
                Ok(acc)
            }
            (CasOp::Multiply, args) => {
                let mut acc = vec![Value::Int(1)];
                for arg in args {
                    acc = poly_mul(&acc, &poly_from_expr_relaxed_constants(arg, var)?)?;
                }
                Ok(acc)
            }
            (CasOp::Power, [base, exp]) => {
                if base.cas_var_name() == Some(var) {
                    let n = exp.exact_int().and_then(|n| n.to_usize()).ok_or_else(|| {
                        cas_err("solve currently supports non-negative integer powers only")
                    })?;
                    let mut coeffs = vec![Value::Int(0); n + 1];
                    coeffs[n] = Value::Int(1);
                    Ok(coeffs)
                } else if !contains_any_symbolic_var(base) {
                    let Some(n) = exp.exact_int() else {
                        return Err(cas_err(
                            "solve currently supports polynomial expressions with exact numeric coefficients",
                        ));
                    };
                    if n.is_negative() || contains_negative_power_expr(base) {
                        return Err(cas_err(
                            "solve currently supports polynomial expressions with exact numeric coefficients",
                        ));
                    }
                    Ok(vec![normalize_relaxed_poly_coeff(expr)?])
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
    if expr.cas_op_parts().is_some() {
        return Err(cas_err(
            "solve currently supports polynomial expressions with exact numeric coefficients",
        ));
    }
    Err(cas_err("solve expected a symbolic polynomial expression").got1(expr))
}

fn normalize_poly_coeffs(coeffs: &mut Vec<Value>) -> WqResult<()> {
    for coeff in coeffs.iter_mut() {
        *coeff = normalize_relaxed_poly_coeff(coeff)?;
    }
    poly_trim(coeffs);
    Ok(())
}

fn normalized_poly_expr(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    let mut coeffs = match poly_from_expr_relaxed_constants(expr, var) {
        Ok(coeffs) => coeffs,
        Err(_) => return Ok(None),
    };
    normalize_poly_coeffs(&mut coeffs)?;
    Ok(Some(poly_to_expr(&coeffs, var)?))
}

fn normalized_inverse_base(expr: &Value) -> WqResult<Option<Value>> {
    let mut var = None;
    if !collect_single_poly_var(expr, &mut var) {
        return Ok(None);
    }
    let Some(var) = var else {
        return Ok(None);
    };
    let Some(normalized) = normalized_poly_expr(expr, &var)? else {
        return Ok(None);
    };
    if normalized == *expr {
        Ok(None)
    } else {
        Ok(Some(normalized))
    }
}

fn try_collapse_numerator_over_single_inverse(factors: &[Value]) -> WqResult<Option<Value>> {
    let mut inverse = None;
    let mut denom = None;
    for (i, factor) in factors.iter().enumerate() {
        if let Some([base, exp]) = factor.cas_op_args(CasOp::Power)
            && exp.exact_int_is(-1)
        {
            if inverse.is_some() {
                return Ok(None);
            }
            inverse = Some((i, factor.clone()));
            denom = Some(base.clone());
        }
    }
    let (inverse_idx, inverse_factor) = match inverse {
        Some(parts) => parts,
        None => return Ok(None),
    };
    let Some(denom) = denom else {
        return Ok(None);
    };
    let mut var = None;
    if !collect_single_poly_var(&denom, &mut var) {
        return Ok(None);
    }
    let Some(var) = var else {
        return Ok(None);
    };

    let numer_parts: Vec<_> = factors
        .iter()
        .enumerate()
        .filter_map(|(i, factor)| {
            if i == inverse_idx {
                None
            } else {
                Some(factor.clone())
            }
        })
        .collect();
    let numer = cas_product(numer_parts);
    if !contains_cas_var(&numer, &var) {
        return Ok(None);
    }

    let mut expanded = numer;
    for _ in 0..3 {
        let next = simplify_cas_value(&expand_expr(&expanded)?)?;
        if next == expanded {
            break;
        }
        expanded = next;
        if !contains_cas_var(&expanded, &var) {
            break;
        }
    }
    if contains_cas_var(&expanded, &var) {
        return Ok(None);
    }
    if numeric_is_zero(&expanded) {
        return Ok(Some(Value::Int(0)));
    }

    let mut rebuilt = Vec::with_capacity(2);
    if !numeric_is_one(&expanded) {
        rebuilt.push(expanded);
    }
    rebuilt.push(inverse_factor);
    sort_canonical(&mut rebuilt);
    Ok(Some(match rebuilt.len() {
        0 => Value::Int(1),
        1 => rebuilt
            .into_iter()
            .next()
            .expect("single collapsed product factor"),
        _ => Value::from_cas_op(CasOp::Multiply, rebuilt),
    }))
}

struct RationalTerm {
    denom: Value,
    numer: Value,
    originals: Vec<(Value, Value)>,
}

fn negative_power_denominator(value: &Value) -> WqResult<Option<(Value, bool)>> {
    let Some([base, exp]) = value.cas_op_args(CasOp::Power) else {
        return Ok(None);
    };
    let Some(power) = exp.exact_int() else {
        return Ok(None);
    };
    if !power.is_negative() {
        return Ok(None);
    }
    let denominator_power = -power;
    if denominator_power.is_one() {
        Ok(Some((base.clone(), false)))
    } else {
        let mut var = None;
        if !collect_single_poly_var(base, &mut var) {
            return Ok(None);
        }
        let Some(var) = var else {
            return Ok(None);
        };
        let Ok(poly) = poly_from_expr(base, &var) else {
            return Ok(None);
        };
        if poly.iter().any(|coeff| coeff.rational_parts().is_none()) {
            return Ok(None);
        }
        Ok(Some((
            cas_pow(base.clone(), Value::from_bigint(denominator_power))?,
            true,
        )))
    }
}

fn split_rational_core(core: &Value, coeff: &Value) -> WqResult<Option<(Value, Value)>> {
    if let Some((denominator, _)) = negative_power_denominator(core)? {
        return Ok(Some((denominator, coeff.clone())));
    }
    let Some(args) = core.cas_op_args(CasOp::Multiply) else {
        return Ok(None);
    };
    let mut denominator_parts = Vec::new();
    let mut numerator_parts = Vec::new();
    let mut has_higher_power = false;
    let mut has_symbolic_numerator = false;
    if !numeric_is_one(coeff) {
        numerator_parts.push(coeff.clone());
    }
    for arg in args {
        if let Some((denominator, higher_power)) = negative_power_denominator(arg)? {
            denominator_parts.push(denominator);
            has_higher_power |= higher_power;
        } else {
            numerator_parts.push(arg.clone());
            has_symbolic_numerator = true;
        }
    }
    if denominator_parts.is_empty() || (has_symbolic_numerator && !has_higher_power) {
        return Ok(None);
    }
    Ok(Some((
        cas_product(denominator_parts),
        cas_product(numerator_parts),
    )))
}

const MAX_RATIONAL_COMBINE_TERMS: usize = 6;
const MAX_RATIONAL_COMBINE_DEGREE: usize = 10;

fn push_rational_term(
    terms: &mut Vec<RationalTerm>,
    denom: Value,
    numer: Value,
    core: Value,
    original_coeff: Value,
) -> WqResult<()> {
    for existing in terms.iter_mut() {
        if existing.denom == denom || existing.denom.to_string() == denom.to_string() {
            existing.numer = factor_expr(&cas_add(vec![existing.numer.clone(), numer])?)?;
            existing.originals.push((core, original_coeff));
            return Ok(());
        }
    }
    terms.push(RationalTerm {
        denom,
        numer,
        originals: vec![(core, original_coeff)],
    });
    Ok(())
}

fn should_combine_rational_polys(d_polys: &[Vec<Value>], n_polys: &[Vec<Value>]) -> bool {
    if d_polys.len() > MAX_RATIONAL_COMBINE_TERMS {
        return false;
    }
    let denominator_degree: usize = d_polys.iter().map(|poly| poly_degree(poly)).sum();
    if denominator_degree > MAX_RATIONAL_COMBINE_DEGREE {
        return false;
    }
    let largest_numerator_degree = n_polys
        .iter()
        .map(|poly| poly_degree(poly))
        .max()
        .unwrap_or(0);
    denominator_degree.saturating_add(largest_numerator_degree) <= MAX_RATIONAL_COMBINE_DEGREE
}

fn push_original_rational_terms(
    keep: &mut Vec<(Value, Value)>,
    terms: &[RationalTerm],
    indices: Vec<usize>,
) {
    for i in indices {
        keep.extend(terms[i].originals.iter().cloned());
    }
}

/// Combine rational terms sharing the same polynomial variable.
/// Input: `grouped` after rational core normalization.
/// Terms with core `(^ D -1)` (i.e. `1/D`) where D is polynomial in some var
/// are combined into `(sum N_i*product_{j!=i} D_j) / (product D_i)`.
fn combine_rational_terms(grouped: &mut Vec<(Value, Value)>) -> WqResult<()> {
    use std::collections::HashMap;
    // Separate rational terms (core = (^ D -1)) by variable
    let mut rational_by_var: HashMap<String, Vec<RationalTerm>> = HashMap::new();
    let mut keep = Vec::new();
    for (core, coeff) in grouped.drain(..) {
        if let Some((denominator, numerator)) = split_rational_core(&core, &coeff)? {
            let mut var: Option<String> = None;
            if collect_single_poly_var(&denominator, &mut var)
                && let Some(ref v) = var
                && let Ok(Some(d_norm)) = normalized_poly_expr(&denominator, v)
                && let Ok(d_poly) = poly_from_expr_relaxed_constants(&d_norm, v)
                && poly_degree(&d_poly) >= 1
                && coeff_ok_in_var(&numerator, v)
            {
                push_rational_term(
                    rational_by_var.entry(v.clone()).or_default(),
                    d_norm,
                    numerator,
                    core,
                    coeff,
                )?;
                continue;
            }
            keep.push((core, coeff));
            continue;
        }
        keep.push((core, coeff));
    }

    // Combine each variable group
    for (var, terms) in rational_by_var {
        if terms.len() < 2 && terms.iter().all(|term| term.originals.len() < 2) {
            for term in terms {
                keep.extend(term.originals);
            }
            continue;
        }

        // Convert to polynomials.  Reconstruct each rational term N/D,
        // simplify it to give cas_mul a chance to merge matching powers
        // (e.g. two half-powers -> integer power), then re-extract the
        // numerator and denominator.
        let mut d_polys: Vec<Vec<Value>> = Vec::with_capacity(terms.len());
        let mut n_polys: Vec<Vec<Value>> = Vec::with_capacity(terms.len());
        let mut succeeded = Vec::with_capacity(terms.len());
        for (i, term) in terms.iter().enumerate() {
            let mut d_poly = poly_from_expr_relaxed_constants(&term.denom, &var)
                .unwrap_or_else(|_| vec![Value::Int(1)]);
            normalize_poly_coeffs(&mut d_poly)?;
            match poly_from_expr_relaxed_constants(&term.numer, &var) {
                Ok(mut p) => {
                    normalize_poly_coeffs(&mut p)?;
                    d_polys.push(d_poly);
                    n_polys.push(p);
                    succeeded.push(i);
                }
                Err(_) => {
                    keep.extend(term.originals.iter().cloned());
                }
            }
        }
        if d_polys.len() < 2 && succeeded.iter().all(|&i| terms[i].originals.len() < 2) {
            push_original_rational_terms(&mut keep, &terms, succeeded);
            continue;
        }
        if !should_combine_rational_polys(&d_polys, &n_polys) {
            push_original_rational_terms(&mut keep, &terms, succeeded);
            continue;
        }

        // Common denominator: product D_i
        let mut prefix_products = Vec::with_capacity(d_polys.len() + 1);
        prefix_products.push(vec![Value::Int(1)]);
        for d_poly in &d_polys {
            let prev = prefix_products
                .last()
                .expect("prefix products always contain an initial identity");
            prefix_products.push(poly_mul(prev, d_poly)?);
        }
        let mut suffix_products = vec![Vec::new(); d_polys.len() + 1];
        suffix_products[d_polys.len()] = vec![Value::Int(1)];
        for i in (0..d_polys.len()).rev() {
            suffix_products[i] = poly_mul(&d_polys[i], &suffix_products[i + 1])?;
        }
        let mut d_common = prefix_products
            .last()
            .expect("prefix products include the full denominator")
            .clone();
        normalize_poly_coeffs(&mut d_common)?;

        // Combined numerator: sum (N_i * product_{j!=i} D_j)
        let mut n_common = vec![Value::Int(0)];
        for (i, n_poly) in n_polys.iter().enumerate() {
            let other_d = poly_mul(&prefix_products[i], &suffix_products[i + 1])?;
            let term_num = poly_mul(n_poly, &other_d)?;
            n_common = poly_add(&n_common, &term_num)?;
        }
        normalize_poly_coeffs(&mut n_common)?;

        // Cancel a common constant factor: if N is constant c != 0,1 and every
        // coefficient of D is divisible by c, cancel c from both sides.
        // This handles cases like (fourth_root(2)^2/2) / (fourth_root(2)^2*(x^4/2 - 1))
        // -> 1/(x^4-2).
        if poly_degree(&n_common) == 0 && poly_degree(&d_common) >= 1 {
            let n_const = &n_common[0];
            if !numeric_is_one(n_const) && !numeric_is_zero(n_const) {
                let mut can_cancel = true;
                let mut new_d = Vec::with_capacity(d_common.len());
                for c in d_common.iter() {
                    if numeric_is_zero(c) {
                        new_d.push(Value::Int(0));
                    } else if let Ok(q) = eval_exact_numeric_div(c, n_const) {
                        new_d.push(q);
                    } else {
                        can_cancel = false;
                        break;
                    }
                }
                if can_cancel {
                    n_common = vec![Value::Int(1)];
                    d_common = new_d;
                }
            }
        }

        // Cancel a common polynomial factor via poly_gcd.  This handles cases
        // like (4x^2+4*cbrt(2)*x+4*cbrt(2)^2) / (4x^5+...-8*cbrt(2)^2) -> 1/(x^3-2)
        // where numerator and denominator share a non-trivial polynomial
        // factor.
        if poly_degree(&n_common) >= 1 && poly_degree(&d_common) >= 1 {
            let g = poly_gcd(&n_common, &d_common)?;
            if poly_degree(&g) >= 1 {
                let (q_n, r_n) = poly_divide(&n_common, &g)?;
                let (q_d, r_d) = poly_divide(&d_common, &g)?;
                if poly_is_zero(&r_n) && poly_is_zero(&r_d) {
                    n_common = q_n;
                    d_common = q_d;
                }
            }
        }

        // Build combined expression: N/D
        let mut n_expr = poly_to_expr(&n_common, &var)?;
        if contains_cas_var(&n_expr, &var) {
            let mut expanded = n_expr.clone();
            for _ in 0..3 {
                let next = simplify_cas_value(&expand_expr(&expanded)?)?;
                if next == expanded {
                    break;
                }
                expanded = next;
                if !contains_cas_var(&expanded, &var) {
                    break;
                }
            }
            if !contains_cas_var(&expanded, &var) {
                n_expr = expanded;
            }
        }
        let d_expr = poly_to_expr(&d_common, &var)?;
        let combined = cas_div(n_expr, d_expr)?;

        // Decompose back to (core, coeff) for cas_add output
        let (coeff, core_opt) = split_add_term(&combined);
        match core_opt {
            Some(core) => keep.push((core, coeff)),
            None => {
                // Pure constant; wrap so rebuild_scaled_term(c, Some(1)) = c*1 = c
                keep.push((Value::Int(1), coeff));
            }
        }
    }

    *grouped = keep;
    Ok(())
}

/// Combine log terms with matching prefactors: c*ln|A| + (-c)*ln|B| ->
/// c*ln|A/B|.
fn combine_log_terms(grouped: &mut Vec<(Value, Value)>) -> WqResult<()> {
    let mut i = 0usize;
    while i < grouped.len() {
        let (core_i, coeff_i) = (&grouped[i].0, grouped[i].1.clone());
        let (pref_i, arg_i) = match extract_ln_abs_pref(core_i) {
            Some(p) => p,
            None => {
                i += 1;
                continue;
            }
        };

        let neg_coeff_i = numeric_mul(&coeff_i, &Value::Int(-1))?;
        let mut found = false;
        let mut j = i + 1;
        while j < grouped.len() {
            let (core_j, coeff_j) = (&grouped[j].0, grouped[j].1.clone());
            if coeff_j == neg_coeff_i
                && let Some((pref_j, arg_j)) = extract_ln_abs_pref(core_j)
                && pref_i == pref_j
            {
                // Combine: coeff_i * pref * ln|arg_i/arg_j|
                let ratio = cas_div(arg_i.clone(), arg_j.clone())?;
                let ln_combined = Value::from_cas_function(
                    CasFunction::Ln,
                    vec![Value::from_cas_function(CasFunction::Abs, vec![ratio])],
                );
                let new_core = if pref_i == Value::Int(1) {
                    ln_combined
                } else {
                    Value::from_cas_op(CasOp::Multiply, vec![pref_i.clone(), ln_combined])
                };
                grouped[i] = (new_core, coeff_i);
                grouped.remove(j);
                found = true;
                break;
            }
            j += 1;
        }
        if !found {
            i += 1;
        }
    }
    Ok(())
}

/// Extract (prefactor, argument) from a core that is pref * ln|abs[arg]|.
/// Returns None if the core doesn't match this pattern.
fn extract_ln_abs_pref(core: &Value) -> Option<(Value, Value)> {
    // Direct ln|abs[arg]| call
    if let Some((CasFunction::Ln, [ln_arg])) = core.cas_function_parts()
        && let Some((CasFunction::Abs, [abs_arg])) = ln_arg.cas_function_parts()
    {
        return Some((Value::Int(1), abs_arg.clone()));
    }
    // Product: pref * ln|abs[arg]|
    if let Some(args) = core.cas_op_args(CasOp::Multiply)
        && args.len() == 2
    {
        let (pref, rest) = if args[0].cas_function_parts().is_some() {
            (&args[1], &args[0])
        } else {
            (&args[0], &args[1])
        };
        if !rest.is_cas_expr() {
            // pref is the ln term, rest is the coefficient; swap
            if let Some((CasFunction::Ln, [ln_arg])) = pref.cas_function_parts()
                && let Some((CasFunction::Abs, [abs_arg])) = ln_arg.cas_function_parts()
            {
                return Some((rest.clone(), abs_arg.clone()));
            }
        } else if let Some((CasFunction::Ln, [ln_arg])) = rest.cas_function_parts()
            && let Some((CasFunction::Abs, [abs_arg])) = ln_arg.cas_function_parts()
        {
            return Some((pref.clone(), abs_arg.clone()));
        }
    }
    None
}

/// Check that `coeff` (the numerator of a rational term) is a polynomial
/// in the given variable.  Returns true when `poly_from_expr` would succeed.
fn coeff_ok_in_var(coeff: &Value, var: &str) -> bool {
    detect_poly_var(coeff).is_none_or(|v| v == var)
}

pub(crate) fn cas_add(args: Vec<Value>) -> WqResult<Value> {
    let mut flat = Vec::with_capacity(args.len());
    for arg in args {
        push_flattened(&mut flat, CasOp::Add, simplify_cas_value(&arg)?);
    }

    let mut numeric: Option<Value> = None;
    let mut grouped: Vec<(Value, Value)> = Vec::new();
    for arg in flat {
        let (coeff, core) = split_add_term(&arg);
        if let Some(core) = core {
            let mut merged = false;
            for (existing_core, existing_coeff) in &mut grouped {
                if *existing_core == core {
                    *existing_coeff = numeric_add(existing_coeff, &coeff)?;
                    merged = true;
                    break;
                }
            }
            if !merged {
                grouped.push((core, coeff));
            }
        } else {
            numeric = Some(match numeric.take() {
                Some(acc) => numeric_add(&acc, &coeff)?,
                None => coeff,
            });
        }
    }

    // Normalize rational cores: (* N (^ D1 -1) (^ D2 -1) ...) ->
    // extract N into coefficient, keep the (^ Di -1) factors as core structure.
    // For multiple Di, the combined denominator (* D1 D2 ...) is handled by
    // combine_rational_terms below.
    for (core, coeff) in &mut grouped {
        if let Some(args) = core.cas_op_args(CasOp::Multiply)
            && args.len() >= 2
        {
            let mut denom_count = 0;
            let mut num_parts: Vec<Value> = Vec::new();
            let mut denom_parts: Vec<Value> = Vec::new();
            for arg in args.iter() {
                if let Some([_, e]) = arg.cas_op_args(CasOp::Power)
                    && e.exact_int_is(-1)
                {
                    denom_count += 1;
                    denom_parts.push(arg.clone());
                } else {
                    num_parts.push(arg.clone());
                }
            }
            if denom_count >= 1 {
                let numer = cas_product(num_parts);
                let new_coeff =
                    numeric_mul(coeff, &numer).or_else(|_| cas_mul(vec![coeff.clone(), numer]))?;
                *core = cas_product(denom_parts);
                *coeff = new_coeff;
            }
        }
    }

    // Re-merge groups that now share the same core after normalization
    let mut merged_grouped: Vec<(Value, Value)> = Vec::new();
    for (core, coeff) in grouped {
        let mut merged = false;
        for (existing_core, existing_coeff) in &mut merged_grouped {
            if *existing_core == core {
                *existing_coeff = numeric_add(existing_coeff, &coeff)?;
                merged = true;
                break;
            }
        }
        if !merged {
            merged_grouped.push((core, coeff));
        }
    }
    grouped = merged_grouped;

    // Combine rational terms with different denominators: N1/D1 + N2/D2 ->
    // (N1*D2+N2*D1)/(D1*D2)
    combine_rational_terms(&mut grouped)?;

    // Combine log terms: c*ln|A| - c*ln|B| -> c*ln|A/B|
    combine_log_terms(&mut grouped)?;

    let mut out = Vec::with_capacity(grouped.len() + 1);
    if let Some(ref num) = numeric
        && !numeric_is_zero(num)
    {
        out.push(num.clone());
    }
    for (core, coeff) in grouped {
        if numeric_is_zero(&coeff) {
            continue;
        }
        out.push(rebuild_scaled_term(coeff, Some(core))?);
    }
    // Extract common numeric factor from sums involving algebraic terms.
    // Guarded by has_algebraic to avoid interfering with factor_expr
    // for purely numeric/rational expressions.
    let has_algebraic = out.iter().any(|t| {
        t.is_algebraic_number()
            || t.cas_op_args(CasOp::Multiply)
                .is_some_and(|a| a.iter().any(|x| x.is_algebraic_number()))
    });
    if out.len() > 1
        && has_algebraic
        && let Some(gcd) = common_numeric_gcd(&out)
    {
        let mut new_out = Vec::with_capacity(out.len());
        for term in out {
            new_out.push(cas_div(term, gcd.clone())?);
        }
        let inner = match new_out.len() {
            0 => Value::Int(0),
            1 => new_out.into_iter().next().unwrap(),
            _ => Value::from_cas_op(CasOp::Add, new_out),
        };
        return cas_mul(vec![gcd, inner]);
    }
    sort_canonical(&mut out);
    match out.len() {
        0 => Ok(Value::Int(0)),
        1 => Ok(out.into_iter().next().expect("single simplified sum term")),
        _ => Ok(Value::from_cas_op(CasOp::Add, out)),
    }
}

fn delta_root_product_is_zero(factors: &[Value]) -> WqResult<bool> {
    for (delta_idx, factor) in factors.iter().enumerate() {
        let Some((var, root)) = linear_delta_root(factor)? else {
            continue;
        };
        let var_expr = Value::from_cas_var(&var);
        for (idx, other) in factors.iter().enumerate() {
            if idx == delta_idx {
                continue;
            }
            let substituted = substitute_cas(other, &var_expr, &root)?;
            if is_zero_at_delta_root(&simplify_cas_value(&substituted)?) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn is_zero_at_delta_root(value: &Value) -> bool {
    if numeric_is_zero(value) {
        return true;
    }
    let Ok(numeric) = eval_numeric_cas(value) else {
        return false;
    };
    numeric_is_zero(&numeric) || numeric.as_f64().is_some_and(|f| f.abs() <= 1e-10)
}

fn linear_delta_root(value: &Value) -> WqResult<Option<(String, Value)>> {
    let Some((CasFunction::Delta, [arg])) = value.cas_function_parts() else {
        return Ok(None);
    };
    let mut found = None;
    if !collect_single_poly_var(arg, &mut found) {
        return Ok(None);
    }
    let Some(var) = found else {
        return Ok(None);
    };
    let Some((a, b)) = extract_linear_coefficients(arg, &var) else {
        return Ok(None);
    };
    if numeric_is_zero(&a) {
        return Ok(None);
    }
    let root = eval_exact_numeric_div(&cas_neg(b)?, &a)?;
    Ok(Some((var, root)))
}

pub(crate) fn cas_mul(args: Vec<Value>) -> WqResult<Value> {
    let mut flat = Vec::with_capacity(args.len());
    for arg in args {
        push_flattened(&mut flat, CasOp::Multiply, simplify_cas_value(&arg)?);
    }
    if delta_root_product_is_zero(&flat)? {
        return Ok(Value::Int(0));
    }

    let mut numeric: Option<Value> = None;
    let mut grouped: Vec<(Value, Value)> = Vec::new();
    for arg in flat {
        if !arg.is_cas_expr() {
            if numeric_is_zero(&arg) {
                return Ok(Value::Int(0));
            }
            numeric = Some(match numeric.take() {
                Some(acc) => numeric_mul(&acc, &arg)?,
                None => arg,
            });
            continue;
        }

        let (mut base, power) = split_mul_factor(&arg);
        if power.exact_int().is_some_and(|int| int.is_negative())
            && let Some(normalized) = normalized_inverse_base(&base)?
        {
            base = normalized;
        }

        let mut merged = false;
        for (existing_base, existing_power) in &mut grouped {
            if *existing_base == base {
                *existing_power = numeric_add(existing_power, &power)?;
                merged = true;
                break;
            }
        }
        if !merged {
            grouped.push((base, power));
        }
    }

    // Combine products of matching inverse square roots:
    // a^(-1/2) * b^(-1/2) -> (a*b)^(-1/2)
    // This helps cancel nested ratio square-roots generated by inverse-trig
    // chain rules.
    if grouped.len() >= 2 {
        let mut i = 0usize;
        while i < grouped.len() {
            if !grouped[i].1.exact_neg_half() {
                i += 1;
                continue;
            }
            let mut merged_here = false;
            let mut j = i + 1;
            while j < grouped.len() {
                if grouped[j].1 == grouped[i].1 {
                    let merged_base = simplify_cas_value(&cas_product(vec![
                        grouped[i].0.clone(),
                        grouped[j].0.clone(),
                    ]))?;
                    grouped[i].0 = merged_base;
                    grouped.remove(j);
                    merged_here = true;
                    break;
                }
                j += 1;
            }
            if !merged_here {
                i += 1;
            }
        }
    }

    // Fold algebraic factors that share the same field into the numeric
    // coefficient so that e.g. (-36*alpha^2) * (108*alpha^2)^(-1) simplifies to
    // -1/3.
    if let Some(num) = numeric.clone()
        && let Value::Algebraic(num_a) = &num
    {
        let mut changed = true;
        while changed {
            changed = false;
            for i in 0..grouped.len() {
                if let Value::Algebraic(base_a) = &grouped[i].0
                    && num_a.same_field(base_a)
                {
                    let base_pow = if numeric_is_one(&grouped[i].1) {
                        Value::Algebraic(Arc::new((**base_a).clone()))
                    } else if let Some(n) = grouped[i].1.exact_int()
                        && let Ok(n_i64) = i64::try_from(n)
                    {
                        crate::value::algebraic::algebraic_pow(base_a, n_i64)?
                    } else {
                        continue;
                    };
                    numeric = Some(numeric_mul(&num, &base_pow)?);
                    grouped.remove(i);
                    changed = true;
                    break;
                }
            }
        }
    } else if numeric.is_some() && !numeric.as_ref().is_some_and(|n| n.is_algebraic_number()) {
        // Fold Algebraic bases with integer exponents into a plain numeric
        // coefficient, e.g. 31 * alpha^2 -> Algebraic([0, 0, 31]).  This allows
        // down-stream poly_from_expr to see a single Algebraic value instead
        // of a CAS product.
        let mut num = numeric.take().unwrap();
        let mut i = 0;
        while i < grouped.len() {
            if let Value::Algebraic(base_a) = &grouped[i].0
                && let Some(n) = grouped[i].1.exact_int()
                && let Ok(n_i64) = i64::try_from(n)
            {
                let base_pow = if n_i64 == 1 {
                    Value::Algebraic(Arc::new((**base_a).clone()))
                } else {
                    crate::value::algebraic::algebraic_pow(base_a, n_i64)?
                };
                num = numeric_mul(&num, &base_pow)?;
                grouped.remove(i);
            } else {
                i += 1;
            }
        }
        numeric = Some(num);
    }

    let mut out = Vec::with_capacity(grouped.len() + 1);
    if let Some(num) = numeric {
        if numeric_is_zero(&num) {
            return Ok(Value::Int(0));
        }
        if !numeric_is_one(&num) {
            out.push(num);
        }
    }
    for (base, power) in grouped {
        if numeric_is_zero(&power) {
            continue;
        }
        if numeric_is_one(&power) {
            out.push(base);
        } else {
            out.push(cas_pow(base, power)?);
        }
    }
    if let Some(collapsed) = try_collapse_numerator_over_single_inverse(&out)? {
        return Ok(collapsed);
    }
    sort_canonical(&mut out);
    match out.len() {
        0 => Ok(Value::Int(1)),
        1 => Ok(out
            .into_iter()
            .next()
            .expect("single simplified product factor")),
        _ => Ok(Value::from_cas_op(CasOp::Multiply, out)),
    }
}

/// Check whether `n` is a perfect `q`-th power (n = m^q for some integer m).
fn is_perfect_power(n: &BigInt, q: &BigInt) -> bool {
    if n.is_zero() || n.is_one() {
        return true;
    }
    let Some(q_u) = q.to_u32() else {
        return false;
    };
    if q_u == 0 {
        return false;
    }
    if n.is_negative() && q_u.is_multiple_of(2) {
        return false;
    }

    let abs_n = n.abs();
    if let Some(n_f) = abs_n.to_f64() {
        let root_f = n_f.powf(1.0 / q_u as f64);
        let candidate = root_f.round() as i64;
        for c in [candidate - 1, candidate, candidate + 1] {
            if c >= 0 {
                let c_bi = BigInt::from(c);
                if c_bi.pow(q_u) == abs_n {
                    return true;
                }
            }
        }
    }
    false
}

const PERFECT_POWER_TRIAL_DIVISION_LIMIT: i64 = 10_000;

/// Factor `n` into `p^q * r` where `r` is q-th-power-free.
/// Uses bounded trial division up to the q-th root of n.
pub(crate) fn extract_perfect_power_factor(n: &BigInt, q: u32) -> (BigInt, BigInt) {
    if n.is_zero() || n.is_one() {
        return (n.clone(), BigInt::one());
    }
    let mut p = BigInt::one();
    let mut r = n.clone();
    let Some(limit) = r
        .abs()
        .to_f64()
        .filter(|f| f.is_finite())
        .map(|f| (f.powf(1.0 / q as f64).ceil() as i64).max(1))
    else {
        return (BigInt::one(), n.clone());
    };
    if limit > PERFECT_POWER_TRIAL_DIVISION_LIMIT {
        return (BigInt::one(), n.clone());
    }
    let mut k = BigInt::from(2);
    let limit_bi = BigInt::from(limit);
    while k <= limit_bi {
        let k_pow = k.pow(q);
        if k_pow > r {
            break;
        }
        while (&r % &k_pow).is_zero() {
            p *= &k;
            r /= &k_pow;
        }
        k += 1;
    }
    (p, r)
}

fn is_monomial_poly(coeffs: &[Value]) -> bool {
    let deg = poly_degree(coeffs);
    if deg == 0 {
        return false;
    }
    // Only the leading coefficient should be non-zero
    for (i, c) in coeffs.iter().enumerate() {
        if i == deg {
            continue;
        }
        if !numeric_is_zero(c) {
            return false;
        }
    }
    true
}

fn exact_int_value_is(value: &Value, expected: i64) -> bool {
    value
        .exact_int()
        .and_then(|int| int.to_i64())
        .is_some_and(|int| int == expected)
}

/// Detect the single polynomial variable in an expression.
fn detect_poly_var(expr: &Value) -> Option<String> {
    let mut found = None;
    if collect_single_poly_var(expr, &mut found) {
        found
    } else {
        None
    }
}

/// Try to simplify sqrt(polynomial) by extracting square factors.
/// If poly = outside^2 * inside, returns abs(outside) * sqrt(inside)
/// unless outside is provably positive.
fn try_simplify_sqrt_poly(coeffs: &[Value], var: &str, is_sqrt: bool) -> WqResult<Option<Value>> {
    let factors = square_free_factor(coeffs)?;
    // Check if any factor has multiplicity >= 2
    let has_reduction = factors.iter().any(|(_, m)| *m >= 2);
    if !has_reduction {
        return Ok(None);
    }
    // Skip pure monomial squares like x^2, x^4. rewrite_cas handles them
    // with abs() semantics.
    if factors.len() == 1 && factors[0].1 == 2 && is_monomial_poly(&factors[0].0) {
        return Ok(None);
    }

    let mut outside = vec![Value::Int(1)];
    let mut inside = vec![Value::Int(1)];
    for (factor, mult) in factors {
        let out_pow = mult / 2;
        let in_pow = mult % 2;
        if out_pow > 0 {
            let factor_pow = {
                let mut p = factor.clone();
                for _ in 1..out_pow {
                    p = poly_mul(&p, &factor)?;
                }
                p
            };
            outside = poly_mul(&outside, &factor_pow)?;
        }
        if in_pow > 0 {
            inside = poly_mul(&inside, &factor)?;
        }
    }
    poly_trim(&mut outside);
    poly_trim(&mut inside);

    let out_expr = poly_to_expr(&outside, var)?;
    let in_deg = poly_degree(&inside);
    let outside_term = if is_provably_positive(&out_expr) {
        out_expr
    } else {
        Value::from_cas_function(CasFunction::Abs, vec![out_expr])
    };
    let outside_term = simplify_cas_value(&outside_term)?;

    if in_deg == 0 {
        return if is_sqrt {
            Ok(Some(outside_term))
        } else {
            Ok(Some(cas_pow(outside_term, Value::Int(-1))?))
        };
    }

    // Build abs(out) * in^(1/2) or abs(out)^(-1) * in^(-1/2).
    let in_expr = poly_to_expr(&inside, var)?;
    let in_pow = Value::from_cas_op(
        CasOp::Power,
        vec![
            in_expr,
            if is_sqrt {
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2))
            } else {
                Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2))
            },
        ],
    );
    let result = if is_sqrt {
        cas_mul(vec![outside_term, in_pow])?
    } else {
        cas_mul(vec![cas_pow(outside_term, Value::Int(-1))?, in_pow])?
    };
    Ok(Some(simplify_cas_value(&result)?))
}

pub(crate) fn cas_pow(base: Value, exp: Value) -> WqResult<Value> {
    let base = simplify_cas_value(&base)?;
    let exp = simplify_cas_value(&exp)?;
    // Algebraic values can't be numerically evaluated
    if !base.is_cas_expr() && !exp.is_cas_expr() && !base.is_algebraic_number() {
        // For fractional powers of rationals, keep symbolic unless it's a
        // perfect power (e.g. (4)^(1/2) = 2 stays numeric, but (3/4)^(1/2)
        // becomes symbolic to avoid float pollution).
        if let (Some((bn, bd)), Some((en, ed))) = (base.rational_parts(), exp.rational_parts())
            && !ed.is_one()
        {
            let is_exact = is_perfect_power(&bn, &ed) && is_perfect_power(&bd, &ed);
            if !is_exact {
                if let Some(q) = ed.to_u32() {
                    // Extract perfect q-th powers: (a/b)^(1/q)
                    // Compute N = a * b^(q-1), then factor N = p^q * r
                    let n = &bn * &bd.pow(q - 1);
                    let (p, r) = extract_perfect_power_factor(&n, q);
                    if !p.is_one() || r != n {
                        let p_val = Value::from_bigint(p);
                        let bd_val = Value::from_bigint(bd);
                        let rat_part = eval_exact_numeric_div(&p_val, &bd_val)?;
                        let radical = Value::from_cas_op(
                            CasOp::Power,
                            vec![
                                Value::from_bigint(r),
                                Value::from_fraction_parts(BigInt::one(), BigInt::from(q)),
                            ],
                        );
                        let base_simp = cas_mul(vec![rat_part, radical])?;
                        if en.is_one() {
                            return Ok(base_simp);
                        }
                        return cas_pow(base_simp, Value::from_bigint(en));
                    }
                }
                return Ok(Value::from_cas_op(
                    CasOp::Power,
                    vec![
                        Value::from_fraction_parts(bn, bd),
                        Value::from_fraction_parts(en, ed),
                    ],
                ));
            }
        }
        return numeric_pow(&base, &exp);
    }
    if numeric_is_zero(&exp) {
        return Ok(Value::Int(1));
    }
    if numeric_is_one(&exp) {
        return Ok(base);
    }
    if numeric_is_zero(&base) {
        if exp.is_cas_expr() {
            return Ok(Value::from_cas_op(CasOp::Power, vec![base, exp]));
        }
        return Ok(Value::Int(0));
    }
    if numeric_is_one(&base) {
        return Ok(Value::Int(1));
    }
    if let Value::Algebraic(a) = &base {
        if let Some(n) = exp.as_i64() {
            return crate::value::algebraic::algebraic_pow(a, n);
        }
        if let Some((num, den)) = exp.rational_parts()
            && let Ok(result) = crate::value::algebraic::algebraic_rational_pow(a, &num, &den)
        {
            return Ok(result);
        }
    }
    if let Some([inner_base, inner_exp]) = base.cas_op_args(CasOp::Power)
        && inner_exp.rational_parts().is_some()
        && exp.exact_int().is_some()
    {
        return cas_pow(inner_base.clone(), numeric_mul(inner_exp, &exp)?);
    }
    // Distribute integer exponent over product: (a*b*...)^n = a^n * b^n * ...
    if let Some(args) = base.cas_op_args(CasOp::Multiply)
        && exp.exact_int().is_some()
    {
        let mut new_args = Vec::with_capacity(args.len());
        for arg in args {
            new_args.push(cas_pow(arg.clone(), exp.clone())?);
        }
        return cas_mul(new_args);
    }
    // Expand (sum)^2 = sum of squares + 2 * sum of distinct products
    if let Some(args) = base.cas_op_args(CasOp::Add)
        && exact_int_value_is(&exp, 2)
    {
        let mut terms = Vec::new();
        for i in 0..args.len() {
            terms.push(cas_pow(args[i].clone(), Value::Int(2))?);
            for j in (i + 1)..args.len() {
                terms.push(cas_mul(vec![
                    Value::Int(2),
                    args[i].clone(),
                    args[j].clone(),
                ])?);
            }
        }
        return cas_add(terms);
    }
    // Simplify sqrt(polynomial): extract square factors from under the sqrt.
    // e.g. sqrt(x^4+2x^2+1) = x^2+1, sqrt(x^3+2x^2+x) = (x+1)*sqrt(x)
    if (exp.exact_half() || exp.exact_neg_half())
        && let Some(var) = detect_poly_var(&base)
        && let Ok(coeffs) = poly_from_expr(&base, &var)
        && poly_degree(&coeffs) >= 1
        && let Some(simplified) = try_simplify_sqrt_poly(&coeffs, &var, exp.exact_half())?
    {
        return Ok(simplified);
    }
    Ok(Value::from_cas_op(CasOp::Power, vec![base, exp]))
}

/// If `value` is an Algebraic with a denested field, return the normalized
/// form.
fn try_normalize_algebraic(value: &Value) -> Option<Value> {
    if let Value::Algebraic(a) = value
        && let Some(normalized) = crate::value::algebraic::normalize_algebraic_field(a)
    {
        Some(Value::Algebraic(Arc::new(normalized)).unwrap_algebraic_constant())
    } else {
        None
    }
}

pub(crate) fn simplify_cas_value(value: &Value) -> WqResult<Value> {
    // Normalize algebraic field before processing (e.g. Q(cbrt(1/108)) ->
    // Q(cbrt(2)))
    if let Some(normalized) = try_normalize_algebraic(value) {
        return Ok(normalized);
    }
    if !value.is_cas_expr() || value.cas_var_name().is_some() {
        return Ok(value.unwrap_algebraic_constant());
    }

    let mut stack = vec![SimplifyFrame::Expr(value.clone())];
    let mut results: Vec<Value> = Vec::new();

    while let Some(frame) = stack.pop() {
        match frame {
            SimplifyFrame::Expr(expr) => {
                if !expr.is_cas_expr() || expr.cas_var_name().is_some() {
                    // Normalize algebraic values (e.g. Q(cbrt(1/108)) -> Q(cbrt(2)))
                    if let Some(normalized) = try_normalize_algebraic(&expr) {
                        results.push(normalized);
                        continue;
                    }
                    results.push(expr.unwrap_algebraic_constant());
                    continue;
                }

                if let Some((lhs, rhs)) = expr.cas_eq_parts() {
                    stack.push(SimplifyFrame::Eq);
                    stack.push(SimplifyFrame::Expr(rhs.clone()));
                    stack.push(SimplifyFrame::Expr(lhs.clone()));
                    continue;
                }

                if let Some((op, args)) = expr.cas_known_op_parts() {
                    match (op, args) {
                        (CasOp::Add, args) if args.len() >= 2 => {
                            stack.push(SimplifyFrame::Add(args.len()));
                            for arg in args.iter().rev() {
                                stack.push(SimplifyFrame::Expr(arg.clone()));
                            }
                        }
                        (CasOp::Add, _) => {
                            return Err(cas_err("malformed '+' expression").got1(&expr));
                        }
                        (CasOp::Multiply, args) if args.len() >= 2 => {
                            stack.push(SimplifyFrame::Mul(args.len()));
                            for arg in args.iter().rev() {
                                stack.push(SimplifyFrame::Expr(arg.clone()));
                            }
                        }
                        (CasOp::Multiply, _) => {
                            return Err(cas_err("malformed '*' expression").got1(&expr));
                        }
                        (CasOp::Subtract, [arg]) => {
                            stack.push(SimplifyFrame::Neg);
                            stack.push(SimplifyFrame::Expr(arg.clone()));
                        }
                        (CasOp::Subtract, [lhs, rhs]) => {
                            stack.push(SimplifyFrame::Sub);
                            stack.push(SimplifyFrame::Expr(rhs.clone()));
                            stack.push(SimplifyFrame::Expr(lhs.clone()));
                        }
                        (CasOp::Divide, [lhs, rhs]) => {
                            stack.push(SimplifyFrame::Div);
                            stack.push(SimplifyFrame::Expr(rhs.clone()));
                            stack.push(SimplifyFrame::Expr(lhs.clone()));
                        }
                        (CasOp::Power, [base, exp]) => {
                            stack.push(SimplifyFrame::Pow);
                            stack.push(SimplifyFrame::Expr(exp.clone()));
                            stack.push(SimplifyFrame::Expr(base.clone()));
                        }
                        (CasOp::Subtract, _) => {
                            return Err(cas_err("malformed '-' expression").got1(&expr));
                        }
                        (CasOp::Divide, _) => {
                            return Err(cas_err("malformed '/' expression").got1(&expr));
                        }
                        (CasOp::Power, _) => {
                            return Err(cas_err("malformed '^' expression").got1(&expr));
                        }
                    }
                    continue;
                }

                if let Some((function, args)) = expr.cas_function_parts() {
                    let n = args.len();
                    if !function.accepts_arity(n) {
                        return Err(cas_err(format!(
                            "malformed '{}': expected {}",
                            function.name(),
                            function.arity_description()
                        ))
                        .got1(&expr));
                    }
                    stack.push(SimplifyFrame::Function { function, n });
                    for arg in args.iter().rev() {
                        stack.push(SimplifyFrame::Expr(arg.clone()));
                    }
                    continue;
                }

                if let Some((name, args)) = expr.cas_apply_parts() {
                    let n = args.len();
                    stack.push(SimplifyFrame::Apply {
                        name: name.clone(),
                        n,
                    });
                    for arg in args.iter().rev() {
                        stack.push(SimplifyFrame::Expr(arg.clone()));
                    }
                    continue;
                }

                if let Some((name, value)) = expr.cas_named_arg_parts() {
                    stack.push(SimplifyFrame::NamedArg { name: name.clone() });
                    stack.push(SimplifyFrame::Expr(value.clone()));
                    continue;
                }

                if let Some((inner, var, point, direction)) = expr.cas_limit_parts() {
                    stack.push(SimplifyFrame::Limit {
                        var: var.clone(),
                        direction,
                    });
                    stack.push(SimplifyFrame::Expr(point.clone()));
                    stack.push(SimplifyFrame::Expr(inner.clone()));
                    continue;
                }

                if let Some(root) = resolve_cas_root(&expr)? {
                    results.push(root);
                    continue;
                }

                results.push(expr.clone());
            }
            SimplifyFrame::Add(n) => {
                let children = split_off_results(&mut results, n)?;
                results.push(cas_add(children)?);
            }
            SimplifyFrame::Mul(n) => {
                let children = split_off_results(&mut results, n)?;
                results.push(cas_mul(children)?);
            }
            SimplifyFrame::Pow => {
                let exp = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing exponent for ^"))?;
                let base = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing base for ^"))?;
                results.push(cas_pow(base, exp)?);
            }
            SimplifyFrame::Div => {
                let rhs = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing rhs for /"))?;
                let lhs = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing lhs for /"))?;
                results.push(cas_div(lhs, rhs)?);
            }
            SimplifyFrame::Neg => {
                let arg = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing arg for unary -"))?;
                results.push(cas_neg(arg)?);
            }
            SimplifyFrame::Sub => {
                let rhs = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing rhs for -"))?;
                let lhs = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing lhs for -"))?;
                results.push(cas_sub(lhs, rhs)?);
            }
            SimplifyFrame::Function { function, n } => {
                let args = split_off_results(&mut results, n)?;
                if function == CasFunction::Sqrt
                    && let [arg] = args.as_slice()
                {
                    results.push(cas_pow(
                        arg.clone(),
                        Value::from_fraction_parts(BigInt::one(), BigInt::from(2)),
                    )?);
                } else if let Some(value) = exact_function_value(function, &args) {
                    results.push(value);
                } else if args.iter().all(|arg| !arg.is_cas_expr())
                    && let Some(value) = eval_numeric_call(function, &args)?
                {
                    results.push(value);
                } else if function == CasFunction::Exp
                    && let [arg] = args.as_slice()
                {
                    // Keep exp[n] symbolic as e or e^n
                    let e = Value::from_cas_const(CasConst::E);
                    if numeric_is_zero(arg) {
                        results.push(Value::Int(1));
                    } else if numeric_is_one(arg) {
                        results.push(e);
                    } else {
                        results.push(cas_pow(e, arg.clone())?);
                    }
                } else if function == CasFunction::Ln
                    && let [arg] = args.as_slice()
                {
                    if arg.cas_const() == Some(CasConst::E) {
                        results.push(Value::Int(1));
                    } else if numeric_is_one(arg) {
                        // ln(1) = 0
                        results.push(Value::Int(0));
                    } else {
                        results.push(Value::from_cas_function(CasFunction::Ln, args));
                    }
                } else if function == CasFunction::Abs
                    && let [arg] = args.as_slice()
                {
                    // abs(inf) = inf, abs(-inf) = inf
                    if matches!(
                        arg.cas_const(),
                        Some(CasConst::Infinity | CasConst::NegInfinity)
                    ) {
                        results.push(Value::from_cas_const(CasConst::Infinity));
                    } else {
                        results.push(Value::from_cas_function(CasFunction::Abs, args));
                    }
                } else if should_keep_exact_function_symbolic(function, &args) {
                    results.push(Value::from_cas_function(function, args));
                } else if let Some(value) = try_eval_with_const_resolve(function, &args)? {
                    results.push(value);
                } else {
                    results.push(Value::from_cas_function(function, args));
                }
            }
            SimplifyFrame::Apply { name, n } => {
                let args = split_off_results(&mut results, n)?;
                results.push(Value::from_cas_apply(name.as_str(), args));
            }
            SimplifyFrame::NamedArg { name } => {
                let value = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing named argument value"))?;
                results.push(Value::from_cas_named_arg(name.as_str(), value));
            }
            SimplifyFrame::Limit { var, direction } => {
                let point = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing point for limit"))?;
                let inner = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing expression for limit"))?;
                results.push(Value::from_cas_limit(inner, var, point, direction));
            }
            SimplifyFrame::Eq => {
                let rhs = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing rhs for eq"))?;
                let lhs = results
                    .pop()
                    .ok_or_else(|| cas_err("simplify: missing lhs for eq"))?;
                results.push(Value::from_cas_eq(lhs, rhs));
            }
        }
    }

    let result = results
        .pop()
        .ok_or_else(|| cas_err("simplify: empty result stack"))?;

    Ok(result)
}

pub(crate) fn cas_binary_expr(op: CasOp, lhs: &Value, rhs: &Value) -> WqResult<Value> {
    ensure_expr_arg(lhs, op.symbol())?;
    ensure_expr_arg(rhs, op.symbol())?;
    simplify_cas_value(&Value::from_cas_op(op, vec![lhs.clone(), rhs.clone()]))
}

pub(crate) fn cas_unary_expr(op: CasOp, arg: &Value) -> WqResult<Value> {
    ensure_expr_arg(arg, op.symbol())?;
    simplify_cas_value(&Value::from_cas_op(op, vec![arg.clone()]))
}

pub(crate) fn cas_call_expr(function: CasFunction, args: &[Value]) -> WqResult<Value> {
    if !function.accepts_arity(args.len()) {
        return Err(cas_err(format!(
            "{} expects {}",
            function.name(),
            function.arity_description()
        )));
    }
    for arg in args {
        ensure_expr_arg(arg, function.name())?;
    }
    simplify_cas_value(&Value::from_cas_function(function, args.to_vec()))
}

pub(super) fn var_name_from_value(value: &Value) -> WqResult<String> {
    if let Some(name) = value.cas_var_name() {
        return Ok(name.to_string());
    }
    if let Value::Tag(name) = value {
        return Ok(name.to_string());
    }
    value
        .try_to_rust_string()
        .ok_or_else(|| cas_err("expected symbolic variable, symbol, or string").got1(value))
}

pub(super) fn substitute_expr(expr: &Value, var: &str, val: &Value) -> WqResult<Value> {
    if let Some((lhs, rhs)) = expr.cas_eq_parts() {
        return Ok(Value::from_cas_eq(
            substitute_expr(lhs, var, val)?,
            substitute_expr(rhs, var, val)?,
        ));
    }
    if let Some(name) = expr.cas_var_name() {
        return Ok(if name == var {
            val.clone()
        } else {
            expr.clone()
        });
    }
    if expr.cas_const_name().is_some() {
        return Ok(expr.clone());
    }
    if expr.cas_root_parts().is_some() {
        return Ok(expr.clone());
    }
    if !expr.is_cas_expr() {
        return Ok(expr.clone());
    }
    if let Some((op, args)) = expr.cas_known_op_parts() {
        return match (op, args) {
            (CasOp::Add, args) => {
                let mut out = Vec::with_capacity(args.len());
                for arg in args {
                    out.push(substitute_expr(arg, var, val)?);
                }
                cas_add(out)
            }
            (CasOp::Multiply, args) => {
                let mut out = Vec::with_capacity(args.len());
                for arg in args {
                    out.push(substitute_expr(arg, var, val)?);
                }
                cas_mul(out)
            }
            (CasOp::Power, [base, exp]) => cas_pow(
                substitute_expr(base, var, val)?,
                substitute_expr(exp, var, val)?,
            ),
            _ => Ok(expr.clone()),
        };
    }
    if let Some((function, args)) = expr.cas_function_parts() {
        let mut out = Vec::with_capacity(args.len());
        for arg in args {
            out.push(substitute_expr(arg, var, val)?);
        }
        return simplify_cas_value(&Value::from_cas_function(function, out));
    }
    if let Some((name, args)) = expr.cas_apply_parts() {
        let mut out = Vec::with_capacity(args.len());
        for arg in args {
            out.push(substitute_expr(arg, var, val)?);
        }
        return simplify_cas_value(&Value::from_cas_apply(name.as_str(), out));
    }
    if let Some((name, value)) = expr.cas_named_arg_parts() {
        return Ok(Value::from_cas_named_arg(
            name.as_str(),
            substitute_expr(value, var, val)?,
        ));
    }
    if let Some((inner, limit_var, point, direction)) = expr.cas_limit_parts() {
        let substituted_point = substitute_expr(point, var, val)?;
        let substituted_inner = match limit_var.cas_var_name() {
            Some(bound) if bound == var => inner.clone(),
            Some(bound) if contains_cas_var(val, bound) && contains_cas_var(inner, var) => {
                return Err(cas_err(format!(
                    "substitute would capture bound limit variable '{bound}'"
                ))
                .got1(expr));
            }
            _ => substitute_expr(inner, var, val)?,
        };
        return Ok(Value::from_cas_limit(
            substituted_inner,
            limit_var.clone(),
            substituted_point,
            direction,
        ));
    }
    Ok(expr.clone())
}

pub(crate) fn substitute_cas(expr: &Value, var: &Value, val: &Value) -> WqResult<Value> {
    let var = var_name_from_value(var)?;
    if val.is_cas_equation() {
        return Err(
            cas_err("substitute expects a replacement expression or value, got equation").got1(val),
        );
    }
    let expr = simplify_cas_value(expr)?;
    simplify_cas_value(&substitute_expr(&expr, &var, val)?)
}

pub(crate) fn substitute_cas_bindings(
    expr: &Value,
    bindings: &[(Arc<str>, Value)],
) -> WqResult<Value> {
    let mut result = expr.clone();
    for (name, value) in bindings {
        result = substitute_cas(&result, &Value::from_cas_var(name.as_ref()), value)?;
    }
    Ok(result)
}

#[cfg(test)]
mod rational_combine_tests {
    use super::*;

    #[test]
    fn degree_limited_mixed_rational_term_restores_original_coefficient() {
        let x = Value::from_cas_var("x");
        let high_degree_base = cas_add(vec![
            cas_pow(x.clone(), Value::Int(6)).expect("x^6"),
            Value::Int(1),
        ])
        .expect("x^6 + 1");
        let mixed_core = cas_mul(vec![
            x.clone(),
            cas_pow(high_degree_base, Value::Int(-2)).expect("inverse square"),
        ])
        .expect("mixed rational core");
        let linear_core = cas_pow(
            cas_add(vec![x, Value::Int(1)]).expect("x + 1"),
            Value::Int(-1),
        )
        .expect("linear inverse");
        let mut grouped = vec![
            (mixed_core.clone(), Value::Int(2)),
            (linear_core.clone(), Value::Int(1)),
        ];

        combine_rational_terms(&mut grouped).expect("combine rational terms");

        assert!(grouped.contains(&(mixed_core, Value::Int(2))));
        assert!(grouped.contains(&(linear_core, Value::Int(1))));
    }

    #[test]
    fn matching_denominators_combine_after_bucket_merge() {
        let x = Value::from_cas_var("x");
        let denominator = cas_add(vec![
            cas_pow(x.clone(), Value::Int(4)).expect("x^4"),
            Value::Int(1),
        ])
        .expect("x^4 + 1");
        let inverse = cas_pow(denominator, Value::Int(-1)).expect("inverse quartic");
        let mut grouped = vec![
            (inverse.clone(), x.clone()),
            (
                inverse.clone(),
                cas_add(vec![
                    Value::Int(1),
                    cas_mul(vec![Value::Int(-1), x]).expect("-x"),
                ])
                .expect("1 - x"),
            ),
        ];

        combine_rational_terms(&mut grouped).expect("combine rational terms");

        assert_eq!(grouped, vec![(inverse, Value::Int(1))]);
    }
}
