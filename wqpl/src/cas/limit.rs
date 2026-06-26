use crate::cas::diff::diff_expr;
use crate::cas::{
    cas_product, contains_cas_var, numeric_is_zero, simplify_cas_value, substitute_cas,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cas::{CasConst, CasFunction, CasOp};
use crate::value::{Value, WqResult};

const MAX_LHOPITAL_DEPTH: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LimitDirection {
    /// x -> a+ (approach from above)
    Right,
    /// x -> a- (approach from below)
    Left,
}

/// Evaluate the limit of `expr` as `var` -> `point`.
///
/// Strategies are tried in order.  When all fail, returns an unevaluated
/// `limit(...)` CAS expression node.
pub(crate) fn limit_cas(
    expr: &Value,
    var: &Value,
    point: &Value,
    direction: Option<LimitDirection>,
) -> WqResult<Value> {
    let expr = simplify_cas_value(expr)?;
    let point = canonical_limit_point(point);
    let dir_fmt = match direction {
        Some(LimitDirection::Right) => "+",
        Some(LimitDirection::Left) => "-",
        None => "",
    };
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] limit enter: expr={} var={} point={} dir={dir_fmt}",
        expr.format_cas().unwrap_or_else(|| expr.to_string()),
        var.format_cas().unwrap_or_else(|| var.to_string()),
        point.format_cas().unwrap_or_else(|| point.to_string())
    );
    let result = limit_cas_inner(&expr, var, &point, direction, 0)?;
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] limit exit: {}",
        result.format_cas().unwrap_or_else(|| result.to_string())
    );
    Ok(result)
}

fn canonical_limit_point(point: &Value) -> Value {
    match point {
        Value::Float(f) if (**f).is_infinite() && (**f).is_sign_positive() => {
            Value::from_cas_const(CasConst::Infinity)
        }
        Value::Float(f) if (**f).is_infinite() && (**f).is_sign_negative() => {
            Value::from_cas_const(CasConst::NegInfinity)
        }
        _ => point.clone(),
    }
}

fn limit_cas_inner(
    expr: &Value,
    var: &Value,
    point: &Value,
    direction: Option<LimitDirection>,
    lhopital_depth: usize,
) -> WqResult<Value> {
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        lhopital_depth,
        "[cas-v] limit_cas_inner enter lhopital_depth={lhopital_depth} expr={}",
        expr.format_cas().unwrap_or_else(|| expr.to_string())
    );

    macro_rules! try_strategy {
        ($name:literal, $call:expr) => {
            if let Some(result) = $call? {
                cas_trace_depth!(
                    DebugLogFlags::CAS_VERBOSE,
                    lhopital_depth,
                    "[cas-v] limit_cas_inner strategy={} lhopital_depth={} -> success: {}",
                    $name,
                    lhopital_depth,
                    result.format_cas().unwrap_or_else(|| result.to_string())
                );
                return Ok(result);
            }
            cas_trace_depth!(
                DebugLogFlags::CAS_VERBOSE,
                lhopital_depth,
                "[cas-v] limit_cas_inner strategy={} lhopital_depth={} -> failed",
                $name,
                lhopital_depth
            );
        };
    }

    // Strategy 0: limits at infinity (must run before substitution --
    // substituting inf into expressions produces garbage like inf^(-1)).
    try_strategy!("infinity", try_limit_at_infinity(expr, var, point));

    // Strategy 1: direct substitution
    try_strategy!("direct_subst", try_direct_substitution(expr, var, point));

    // Strategy 1.5: series expansion at point 0 (faster than L'Hopital)
    if matches!(point, Value::Int(0)) {
        try_strategy!("series", try_series_expansion(expr, var));
    }

    // Strategy 2: L'Hopital's rule for 0/0 and inf/inf forms
    if lhopital_depth < MAX_LHOPITAL_DEPTH {
        try_strategy!(
            "lhopital",
            try_lhopital(expr, var, point, direction, lhopital_depth)
        );
    } else {
        cas_trace_depth!(
            DebugLogFlags::CAS_VERBOSE,
            lhopital_depth,
            "[cas-v] limit_cas_inner strategy=lhopital lhopital_depth={lhopital_depth} -> skipped (max)"
        );
    }

    // Strategy 2.5: known limits table (classic patterns)
    try_strategy!("known_limits", try_known_limits(expr, var, point));

    // Strategy 3: pole analysis -- num/den where den->0, num->c!=0
    try_strategy!("pole", try_pole_limit(expr, var, point, direction));

    // Fallback: unevaluated limit node
    let fallback = Value::from_cas_limit(expr.clone(), var.clone(), point.clone(), direction);
    cas_trace_depth!(
        DebugLogFlags::CAS_VERBOSE,
        lhopital_depth,
        "[cas-v] limit_cas_inner exit lhopital_depth={lhopital_depth} -> unevaluated limit"
    );
    Ok(fallback)
}

/// Strategy 1: substitute the point and check if the result is determinate.
fn try_direct_substitution(expr: &Value, var: &Value, point: &Value) -> WqResult<Option<Value>> {
    // Substituting infinity produces garbage like inf^(-1) -- skip.
    if matches!(
        point.cas_const(),
        Some(CasConst::Infinity | CasConst::NegInfinity)
    ) {
        return Ok(None);
    }
    match substitute_cas(expr, var, point) {
        Ok(result) => {
            if !result.is_cas_expr() {
                return Ok(Some(result));
            }
            // CAS result that no longer contains the variable is determinate
            // (e.g. ln(2) after substituting x=2 into ln(x)).
            let var_name = var.cas_var_name().unwrap_or("x");
            if !contains_cas_var(&result, var_name) {
                return Ok(Some(result));
            }
            Ok(None)
        }
        Err(_) => Ok(None),
    }
}

/// Strategy 2: L'Hopital's rule.
///
/// When the expression is a quotient f/g where both f and g -> 0 (or both ->
/// inf) at the limit point, differentiate numerator and denominator and retry.
fn try_lhopital(
    expr: &Value,
    var: &Value,
    point: &Value,
    direction: Option<LimitDirection>,
    depth: usize,
) -> WqResult<Option<Value>> {
    let var_name = var.cas_var_name().unwrap_or("x");

    // Try standard fraction form first.
    let (num, den) = match split_fraction(expr) {
        Some(pair) => pair,
        None => {
            // Try inf*0 product form: (* a (^ e (* -1 b))) -> a / e^b
            match split_inf_times_zero_product(expr, var_name) {
                Some((n, d)) => (n, d),
                None => return Ok(None),
            }
        }
    };

    // Check for 0/0 or inf/inf form.
    let zero_zero = is_zero_at(&num, var, point)? && is_zero_at(&den, var, point)?;
    let inf_inf = is_inf_at(&num, var, point)? && is_inf_at(&den, var, point)?;
    if !zero_zero && !inf_inf {
        return Ok(None);
    }

    // Differentiate numerator and denominator.
    let d_num = diff_expr(&num, var_name)?;
    let d_den = diff_expr(&den, var_name)?;

    // Build the new quotient and recurse.
    let new_expr = simplify_cas_value(&Value::from_cas_op(CasOp::Divide, vec![d_num, d_den]))?;

    Ok(Some(limit_cas_inner(
        &new_expr,
        var,
        point,
        direction,
        depth + 1,
    )?))
}

/// Check whether `expr` evaluates to zero when `var = point`.
fn is_zero_at(expr: &Value, var: &Value, point: &Value) -> WqResult<bool> {
    match substitute_cas(expr, var, point) {
        Ok(v) => Ok(v.as_f64().map(|f| f == 0.0).unwrap_or(false) || matches!(v, Value::Int(0))),
        Err(_) => Ok(false),
    }
}

/// Check whether `expr` -> +/-inf when `var -> point`.
fn is_inf_at(expr: &Value, var: &Value, point: &Value) -> WqResult<bool> {
    if let Some(lim) = try_limit_at_infinity(expr, var, point)? {
        Ok(matches!(
            lim.cas_const(),
            Some(CasConst::Infinity | CasConst::NegInfinity)
        ))
    } else {
        Ok(false)
    }
}

/// Extract (numerator, denominator) from a simplified CAS product.
///
/// After `simplify_cas_value`, `a/b` is represented as `(* a (^ b -1))`.
/// Also handles `a / b^n` = `(* a (^ b -n))` for n > 1.
fn split_fraction(expr: &Value) -> Option<(Value, Value)> {
    // Case: (^ den -n) -> numerator = 1, denominator = den^n
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && let Some(n) = exp.as_i64()
        && n < 0
    {
        let den = if n == -1 {
            base.clone()
        } else {
            Value::from_cas_op(CasOp::Power, vec![base.clone(), Value::Int(-n)])
        };
        return Some((Value::Int(1), den));
    }

    // Case: (* num (^ den -n) ...) -> extract denominator, rest is numerator
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let mut denom: Option<Value> = None;
        let mut num_factors = Vec::new();
        for arg in args {
            if denom.is_none()
                && let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
                && let Some(n) = exp.as_i64()
                && n < 0
            {
                let den = if n == -1 {
                    base.clone()
                } else {
                    Value::from_cas_op(CasOp::Power, vec![base.clone(), Value::Int(-n)])
                };
                denom = Some(den);
            } else {
                num_factors.push(arg.clone());
            }
        }
        if let Some(den) = denom {
            let num = cas_product(num_factors);
            return Some((num, den));
        }
    }

    None
}

/// For inf*0 product forms like `(* x (^ e (* -1 x)))` = x*e^(-x),
/// rewrite as a quotient `x / e^x` for L'Hopital.
/// Returns `(numerator, denominator)` where the denominator is the
/// inverted zero-factor.
fn split_inf_times_zero_product(expr: &Value, var_name: &str) -> Option<(Value, Value)> {
    let (CasOp::Multiply, args) = expr.cas_op_parts()? else {
        return None;
    };
    // Find a factor that looks like e^(-var) = (^ e (* -1 var))
    let mut num_factors = Vec::new();
    let mut denom_inner: Option<Value> = None;
    for arg in args {
        if denom_inner.is_none()
            && let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && base.cas_const_name() == Some("e")
        {
            // Invert: e^(k*x) in denominator, so denominator = e^(-k*x)
            // For e^(-x): exp = (* -1 x), inverted denom = (^ e x) = e^x
            let (coeff, _) = extract_linear_coeff(exp, var_name);
            if coeff.as_f64().map(|c| c < 0.0).unwrap_or(false) {
                // Build e^(-coeff * x) as denominator
                let pos_exp = if coeff.as_f64() == Some(-1.0) {
                    Value::from_cas_var(var_name)
                } else {
                    Value::from_cas_op(
                        CasOp::Multiply,
                        vec![
                            Value::float(-coeff.as_f64().unwrap()),
                            Value::from_cas_var(var_name),
                        ],
                    )
                };
                denom_inner = Some(Value::from_cas_op(
                    CasOp::Power,
                    vec![Value::from_cas_const(CasConst::E), pos_exp],
                ));
            }
        } else {
            num_factors.push(arg.clone());
        }
    }
    let denom = denom_inner?;
    let num = cas_product(num_factors);
    Some((num, denom))
}

/// Strategy 3: limits at infinity (point is `inf` or `-inf`).
///
/// Analyzes the asymptotic behaviour of the expression rather than
/// substituting.  Handles rational functions via degree comparison,
/// exponentials, and simple products.
fn try_limit_at_infinity(expr: &Value, var: &Value, point: &Value) -> WqResult<Option<Value>> {
    let is_pos_inf = point.cas_const() == Some(CasConst::Infinity);
    let is_neg_inf = point.cas_const() == Some(CasConst::NegInfinity);
    if !is_pos_inf && !is_neg_inf {
        return Ok(None);
    }
    let var_name = var.cas_var_name().unwrap_or("x");
    let sign = if is_pos_inf { 1 } else { -1 };

    // Try rational function analysis via degree comparison.
    if let Some((num, den)) = split_fraction(expr) {
        if let Some(result) = rational_limit_at_infinity(&num, &den, var_name, sign)? {
            return Ok(Some(result));
        }
        // num/den where den -> inf (not caught by rational analysis):
        // if num is constant and den -> inf, limit = 0
        if !num.is_cas_expr()
            && let Some(den_lim) = try_limit_at_infinity(&den, var, point)?
            && matches!(
                den_lim.cas_const(),
                Some(CasConst::Infinity | CasConst::NegInfinity)
            )
        {
            return Ok(Some(Value::Int(0)));
        }
    }

    // exp(arg) or a^arg: base e or any constant base
    if let Some((CasOp::Power, [base, exp_arg])) = expr.cas_op_parts()
        && !base.is_cas_expr()
    {
        // a^(k*x) as x->inf: a>1 -> inf, 0<a<1 -> 0
        let a = base.as_f64().unwrap_or(2.0);
        if a > 1.0 {
            let (coeff, _) = extract_linear_coeff(exp_arg, var_name);
            let c = coeff.as_f64().unwrap_or(0.0);
            if c > 0.0 {
                return Ok(Some(inf_with_sign(sign)));
            } else if c < 0.0 {
                return Ok(Some(Value::Int(0)));
            }
        } else if a > 0.0 && a < 1.0 {
            let (coeff, _) = extract_linear_coeff(exp_arg, var_name);
            let c = coeff.as_f64().unwrap_or(0.0);
            if c > 0.0 {
                return Ok(Some(Value::Int(0)));
            } else if c < 0.0 {
                return Ok(Some(inf_with_sign(sign)));
            }
        }
        return Ok(None);
    }
    // Also handle e^arg specifically
    if let Some((CasOp::Power, [base, exp_arg])) = expr.cas_op_parts()
        && base.cas_const_name() == Some("e")
    {
        return limit_exp_at_infinity(exp_arg, var_name, sign);
    }
    // Also check the raw call form (before simplify).
    if let Some((CasFunction::Exp, [arg])) = expr.cas_function_parts() {
        return limit_exp_at_infinity(arg, var_name, sign);
    }

    // abs(arg) -> inf when arg -> +/-inf, abs(inf)=inf, abs(-inf)=inf
    if let Some((CasFunction::Abs, [arg])) = expr.cas_function_parts() {
        let inner = try_limit_at_infinity(arg, var, point)?;
        if let Some(v) = inner {
            if v.cas_const() == Some(CasConst::NegInfinity) {
                return Ok(Some(Value::from_cas_const(CasConst::Infinity)));
            }
            return Ok(Some(v));
        }
        return Ok(None);
    }

    // Composition: f(inner) where inner -> L at infinity.
    // Try computing limit of inner, then substitute into outer.
    if let Some((name, args)) = expr.cas_function_parts()
        && args.len() == 1
    {
        let inner_limit = try_limit_at_infinity(&args[0], var, point)?;
        if let Some(lim) = inner_limit {
            // Known asymptotic values for inner -> +/-inf
            if matches!(
                lim.cas_const(),
                Some(CasConst::Infinity | CasConst::NegInfinity)
            ) {
                if let Some(asymp) = asymp_at_infinity(name, &lim) {
                    return Ok(Some(asymp));
                }
                // Bounded oscillation: sin(inf), cos(inf) -> doesn't exist
                if matches!(name, CasFunction::Sin | CasFunction::Cos) {
                    return Ok(Some(Value::from_cas_const(CasConst::Undefined)));
                }
                let call = Value::from_cas_function(name, vec![lim]);
                return Ok(Some(call));
            }
            if !lim.is_cas_expr() {
                let call = Value::from_cas_function(name, vec![lim]);
                return Ok(Some(simplify_cas_value(&call)?));
            }
            let call = Value::from_cas_function(name, vec![lim]);
            return Ok(Some(call));
        }
    }
    if let Some((name, args)) = expr.cas_apply_parts()
        && args.len() == 1
    {
        let inner_limit = try_limit_at_infinity(&args[0], var, point)?;
        if let Some(lim) = inner_limit {
            return Ok(Some(Value::from_cas_apply(name.as_str(), vec![lim])));
        }
    }

    // Products: analyze each factor's limit at infinity
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let mut limits = Vec::with_capacity(args.len());
        for arg in args {
            if !arg.is_cas_expr() {
                limits.push((arg.clone(), arg.clone()));
            } else if let Some(lim) = try_limit_at_infinity(arg, var, point)? {
                limits.push((arg.clone(), lim));
            } else {
                return Ok(None);
            }
        }
        match combine_product_limits(&limits) {
            ProductResult::Determinate(v) => return Ok(Some(v)),
            ProductResult::InfTimesZero => {
                // inf*0: dominated by exponential decay -> 0.
                // E.g. x*e^(-x) -> 0 as x->inf (exp dominates polynomial).
                if has_exp_decay(args, var_name) {
                    return Ok(Some(Value::Int(0)));
                }
                return Ok(None);
            }
            ProductResult::Indeterminate => return Ok(None),
        }
    }

    // Variable itself -> inf with sign
    if expr.cas_var_name() == Some(var_name) {
        return Ok(Some(inf_with_sign(sign)));
    }

    // (1 + 1/x)^x -> e as x->inf
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && exp.cas_var_name() == Some(var_name)
        && let Some((CasOp::Add, add_args)) = base.cas_op_parts()
        && add_args.len() == 2
    {
        let has_one = add_args.iter().any(|a| matches!(a, Value::Int(1)));
        let has_recip = add_args.iter().any(|a| {
            if let Some((CasOp::Power, [b, e])) = a.cas_op_parts()
                && b.cas_var_name() == Some(var_name)
                && let Some(n) = e.as_i64()
                && n == -1
            {
                true
            } else {
                false
            }
        });
        if has_one && has_recip && is_pos_inf {
            return Ok(Some(Value::float(std::f64::consts::E)));
        }
    }

    // Power: x^n
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && base.cas_var_name() == Some(var_name)
    {
        if let Some(n) = exp.as_f64() {
            if n > 0.0 {
                return Ok(Some(inf_with_sign(sign)));
            } else if n < 0.0 {
                return Ok(Some(Value::Int(0)));
            }
        }
        if let Some(n) = exp.as_i64() {
            if n > 0 {
                return Ok(Some(inf_with_sign(sign)));
            } else if n < 0 {
                return Ok(Some(Value::Int(0)));
            }
        }
    }

    Ok(None)
}

/// Known asymptotic values of functions at +/-inf.
fn asymp_at_infinity(name: CasFunction, inf_val: &Value) -> Option<Value> {
    let is_pos = inf_val.cas_const() == Some(CasConst::Infinity);
    match name {
        CasFunction::ArcTan => Some(Value::float(if is_pos {
            std::f64::consts::FRAC_PI_2
        } else {
            -std::f64::consts::FRAC_PI_2
        })),
        CasFunction::Tanh => Some(Value::float(if is_pos { 1.0 } else { -1.0 })),
        CasFunction::ArcTanh => None, // arctanh domain is (-1,1), undefined at inf
        CasFunction::ArcCosh => Some(inf_val.clone()), // arccosh(inf) = inf
        CasFunction::ArcSinh => Some(inf_val.clone()), // arcsinh(inf) = inf
        CasFunction::Exp => Some(if is_pos {
            inf_val.clone()
        } else {
            Value::Int(0)
        }),
        CasFunction::Heaviside => Some(Value::float(if is_pos { 1.0 } else { 0.0 })),
        CasFunction::Erf => Some(Value::float(if is_pos { 1.0 } else { -1.0 })),
        CasFunction::Erfc => Some(Value::float(if is_pos { 0.0 } else { 2.0 })),
        _ => None,
    }
}

fn inf_with_sign(sign: i32) -> Value {
    if sign >= 0 {
        Value::from_cas_const(CasConst::Infinity)
    } else {
        Value::from_cas_const(CasConst::NegInfinity)
    }
}

enum ProductResult {
    Determinate(Value),
    InfTimesZero,
    Indeterminate,
}

/// Combine limits of product factors at infinity.
fn combine_product_limits(factors: &[(Value, Value)]) -> ProductResult {
    let mut has_zero = false;
    let mut has_inf = false;
    let mut inf_sign: Option<CasConst> = None;
    let mut finite: Option<Value> = None;

    for (_, lim) in factors {
        if lim.as_f64().map(|f| f == 0.0).unwrap_or(false) || matches!(lim, Value::Int(0)) {
            has_zero = true;
        } else if matches!(
            lim.cas_const(),
            Some(CasConst::Infinity | CasConst::NegInfinity)
        ) {
            has_inf = true;
            let lim_sign = if lim.cas_const() == Some(CasConst::Infinity) {
                CasConst::Infinity
            } else {
                CasConst::NegInfinity
            };
            if inf_sign.is_none() {
                inf_sign = Some(lim_sign);
            } else if inf_sign != Some(lim_sign) {
                inf_sign = Some(CasConst::NegInfinity);
            }
        } else if !lim.is_cas_expr() {
            finite = Some(match finite.take() {
                None => lim.clone(),
                Some(prev) => {
                    Value::float(prev.as_f64().unwrap_or(1.0) * lim.as_f64().unwrap_or(1.0))
                }
            });
        } else {
            return ProductResult::Indeterminate;
        }
    }

    if has_zero && has_inf {
        return ProductResult::InfTimesZero;
    }
    if has_inf {
        let sign = inf_sign.unwrap_or(CasConst::Infinity);
        let neg_coeff = finite
            .as_ref()
            .and_then(|f| f.as_f64())
            .map(|v| v < 0.0)
            .unwrap_or(false);
        if neg_coeff {
            return ProductResult::Determinate(Value::from_cas_const(
                if sign == CasConst::Infinity {
                    CasConst::NegInfinity
                } else {
                    CasConst::Infinity
                },
            ));
        }
        return ProductResult::Determinate(Value::from_cas_const(sign));
    }
    if has_zero {
        ProductResult::Determinate(Value::Int(0))
    } else if let Some(f) = finite {
        ProductResult::Determinate(f)
    } else {
        ProductResult::Indeterminate
    }
}

/// Check whether any factor in the product represents exponential decay
/// (e^(-x) or similar), which dominates polynomial growth at infinity.
fn has_exp_decay(args: &[Value], var_name: &str) -> bool {
    args.iter().any(|arg| {
        // e^(-x) = (^ e (* -1 x))
        if let Some((CasOp::Power, [base, exp])) = arg.cas_op_parts()
            && base.cas_const_name() == Some("e")
        {
            // Check exponent is negative as var -> inf
            let (coeff, _) = extract_linear_coeff(exp, var_name);
            coeff.as_f64().map(|c| c < 0.0).unwrap_or(false)
        } else {
            false
        }
    })
}

/// Handle limit of exp(arg) as var -> +/-inf.
fn limit_exp_at_infinity(arg: &Value, var_name: &str, sign: i32) -> WqResult<Option<Value>> {
    // exp(-x) -> 0 as x->inf, exp(x) -> inf as x->inf
    // Look for linear arg: a*x + b. exp(a*x + b): a>0 -> inf as x->inf; a<0 -> 0 as
    // x->inf
    let (coeff, _) = extract_linear_coeff(arg, var_name);
    let a = coeff.as_f64().unwrap_or(0.0);
    if a == 0.0 {
        return Ok(None);
    }
    let effective_sign = if a * (sign as f64) > 0.0 { 1 } else { -1 };
    if effective_sign > 0 {
        Ok(Some(Value::from_cas_const(CasConst::Infinity)))
    } else {
        Ok(Some(Value::Int(0)))
    }
}

/// Extract the coefficient of `var` in a linear expression a*var + b.
/// Returns (a, b) where a is the coefficient of var.
fn extract_linear_coeff(expr: &Value, var_name: &str) -> (Value, Value) {
    if expr.cas_var_name() == Some(var_name) {
        return (Value::Int(1), Value::Int(0));
    }
    if let Some((CasOp::Subtract, [arg])) = expr.cas_op_parts()
        && arg.cas_var_name() == Some(var_name)
    {
        return (Value::Int(-1), Value::Int(0));
    }
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let mut coeff = Value::Int(1);
        let mut var_count = 0;
        for arg in args {
            if arg.cas_var_name() == Some(var_name) {
                var_count += 1;
            } else if !arg.is_cas_expr() {
                coeff = Value::float(coeff.as_f64().unwrap_or(1.0) * arg.as_f64().unwrap_or(1.0));
            }
        }
        if var_count == 1 {
            return (coeff, Value::Int(0));
        }
    }
    if let Some((CasOp::Add, args)) = expr.cas_op_parts() {
        for arg in args {
            let (a, b) = extract_linear_coeff(arg, var_name);
            if !numeric_is_zero(&a) {
                // Find the constant term from other args
                let mut const_term = b;
                for other in args {
                    if other != arg && !other.is_cas_expr() {
                        const_term = Value::float(
                            const_term.as_f64().unwrap_or(0.0) + other.as_f64().unwrap_or(0.0),
                        );
                    }
                }
                return (a, const_term);
            }
        }
    }
    (Value::Int(0), Value::Int(0))
}

/// Analyze limit of a rational function num/den at infinity.
fn rational_limit_at_infinity(
    num: &Value,
    den: &Value,
    var_name: &str,
    sign: i32,
) -> WqResult<Option<Value>> {
    use crate::cas::{poly_degree, poly_from_expr};

    let num_poly = match poly_from_expr(num, var_name) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let den_poly = match poly_from_expr(den, var_name) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };

    let num_deg = poly_degree(&num_poly);
    let den_deg = poly_degree(&den_poly);

    if num_deg < den_deg {
        return Ok(Some(Value::Int(0)));
    }

    if num_deg == den_deg {
        // Ratio of leading coefficients
        let lead_num = &num_poly[num_deg];
        let lead_den = &den_poly[den_deg];
        if numeric_is_zero(lead_den) {
            return Ok(None);
        }
        let ratio =
            Value::float(lead_num.as_f64().unwrap_or(0.0) / lead_den.as_f64().unwrap_or(1.0));
        return Ok(Some(ratio));
    }

    // num_deg > den_deg -> +/-inf
    let lead_num = &num_poly[num_deg];
    let lead_den = &den_poly[den_deg];
    let ratio_sign = if numeric_is_zero(lead_den) || numeric_is_zero(lead_num) {
        0.0
    } else {
        lead_num.as_f64().unwrap_or(0.0).signum() * lead_den.as_f64().unwrap_or(1.0).signum()
    };
    // Odd/even degree difference determines sign behavior
    let deg_diff = (num_deg - den_deg) as i32;
    let result_sign = if deg_diff % 2 == 0 {
        (ratio_sign as i32).max(1)
    } else {
        (ratio_sign as i32).max(1) * sign
    };
    Ok(Some(inf_with_sign(result_sign)))
}

/// Strategy 2.5: known limits table for classic patterns.
fn try_known_limits(expr: &Value, var: &Value, point: &Value) -> WqResult<Option<Value>> {
    let (num, den) = match split_fraction(expr) {
        Some(pair) => pair,
        None => return Ok(None),
    };
    let var_name = var.cas_var_name().unwrap_or("x");

    // Check that point is 0 and denominator is just the variable.
    if !matches!(point, Value::Int(0)) {
        return Ok(None);
    }
    if den.cas_var_name() != Some(var_name) {
        // Also check x^n
        if let Some((CasOp::Power, [base, _])) = den.cas_op_parts()
            && base.cas_var_name() == Some(var_name)
        {
            // OK
        } else {
            return Ok(None);
        }
    }

    // sin(x) -> 1
    if let Some((CasFunction::Sin, [arg])) = num.cas_function_parts()
        && arg.cas_var_name() == Some(var_name)
    {
        return Ok(Some(Value::Int(1)));
    }

    // tan(x) -> 1
    if let Some((CasFunction::Tan, [arg])) = num.cas_function_parts()
        && arg.cas_var_name() == Some(var_name)
    {
        return Ok(Some(Value::Int(1)));
    }

    // e^x - 1 -> 1
    if let Some((CasOp::Add, args)) = num.cas_op_parts()
        && args.len() == 2
    {
        // (e^x + (-1)) or similar -- match exp(x) - 1
        for arg in args {
            if let Some((CasFunction::Exp, [inner])) = arg.cas_function_parts()
                && inner.cas_var_name() == Some(var_name)
            {
                // Check other term is -1
                let other = if args[0] == *arg { &args[1] } else { &args[0] };
                if matches!(other, Value::Int(-1))
                    || (other.as_f64().map(|f| f == -1.0).unwrap_or(false))
                {
                    return Ok(Some(Value::Int(1)));
                }
            }
        }
    }

    // ln(1+x) -> 1
    if let Some((CasFunction::Ln, [arg])) = num.cas_function_parts()
        && let Some((CasOp::Add, add_args)) = arg.cas_op_parts()
        && add_args.len() == 2
    {
        let has_one = add_args.iter().any(|a| matches!(a, Value::Int(1)));
        let has_var = add_args.iter().any(|a| a.cas_var_name() == Some(var_name));
        if has_one && has_var {
            return Ok(Some(Value::Int(1)));
        }
    }

    Ok(None)
}

// -- Strategy 2.6: series expansion at x=0 --

/// Known Taylor series at x=0, up to order 6.  Coefficients are indexed by
/// degree: `coeffs[i]` is the coefficient of `x^i`.
fn taylor_series(name: CasFunction) -> Option<Vec<f64>> {
    match name {
        CasFunction::Sin => Some(vec![0.0, 1.0, 0.0, -1.0 / 6.0, 0.0, 1.0 / 120.0, 0.0]),
        CasFunction::Cos => Some(vec![1.0, 0.0, -1.0 / 2.0, 0.0, 1.0 / 24.0, 0.0]),
        CasFunction::Tan => Some(vec![0.0, 1.0, 0.0, 1.0 / 3.0, 0.0, 2.0 / 15.0]),
        CasFunction::Exp => Some(vec![
            1.0,
            1.0,
            1.0 / 2.0,
            1.0 / 6.0,
            1.0 / 24.0,
            1.0 / 120.0,
        ]),
        CasFunction::Ln => Some(vec![0.0, 1.0, -1.0 / 2.0, 1.0 / 3.0, -1.0 / 4.0, 1.0 / 5.0]),
        CasFunction::Sqrt => Some(vec![0.0, 1.0]),
        _ => None,
    }
}

/// Build a truncated Taylor series for `expr` around x=0, up to `order`.
/// Returns coefficients as `Vec<f64>` where index = degree, or `None` if the
/// expression can't be handled.
fn expand_series(expr: &Value, var_name: &str, order: usize) -> Option<Vec<f64>> {
    // Constant
    if let Some(f) = expr.as_f64() {
        let mut c = vec![0.0; order + 1];
        c[0] = f;
        return Some(c);
    }
    if matches!(expr, Value::Int(0)) {
        return Some(vec![0.0; order + 1]);
    }
    if let Some(n) = expr.as_i64() {
        let mut c = vec![0.0; order + 1];
        c[0] = n as f64;
        return Some(c);
    }

    // Variable: x -> series [0, 1, 0, 0, ...]
    if expr.cas_var_name() == Some(var_name) {
        let mut c = vec![0.0; order + 1];
        c[1] = 1.0;
        return Some(c);
    }

    // x^n
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && base.cas_var_name() == Some(var_name)
        && let Some(n) = exp.as_i64()
        && n >= 0
    {
        let n = n as usize;
        if n <= order {
            let mut c = vec![0.0; order + 1];
            c[n] = 1.0;
            return Some(c);
        }
    }

    // Sum: f + g
    if let Some((CasOp::Add, args)) = expr.cas_op_parts() {
        let mut total = vec![0.0; order + 1];
        for arg in args {
            let s = expand_series(arg, var_name, order)?;
            for i in 0..=order {
                total[i] += s[i];
            }
        }
        return Some(total);
    }

    // Negation: -f
    if let Some((CasOp::Subtract, [arg])) = expr.cas_op_parts() {
        let s = expand_series(arg, var_name, order)?;
        return Some(s.iter().map(|c| -c).collect());
    }

    // Subtraction: f - g
    if let Some((CasOp::Subtract, [lhs, rhs])) = expr.cas_op_parts() {
        let a = expand_series(lhs, var_name, order)?;
        let b = expand_series(rhs, var_name, order)?;
        let mut c = vec![0.0; order + 1];
        for i in 0..=order {
            c[i] = a[i] - b[i];
        }
        return Some(c);
    }

    // Product: f * g (truncated convolution)
    if let Some((CasOp::Multiply, args)) = expr.cas_op_parts() {
        let series: Vec<Vec<f64>> = args
            .iter()
            .map(|a| expand_series(a, var_name, order))
            .collect::<Option<_>>()?;
        // Multiply all series
        let mut result = vec![1.0]; // 1 (empty product)
        result.resize(order + 1, 0.0);
        result[0] = 1.0;
        for s in &series {
            let mut next = vec![0.0; order + 1];
            for i in 0..=order {
                for j in 0..=order - i {
                    next[i + j] += result[i] * s[j];
                }
            }
            result = next;
        }
        return Some(result);
    }

    // Power: f^n for integer n >= 0
    if let Some((CasOp::Power, [base, exp])) = expr.cas_op_parts()
        && let Some(n) = exp.as_i64()
        && (0..=6).contains(&n)
    {
        let b = expand_series(base, var_name, order)?;
        let mut result = vec![1.0]; // identity
        result.resize(order + 1, 0.0);
        for _ in 0..(n as usize) {
            let mut next = vec![0.0; order + 1];
            for i in 0..=order {
                for j in 0..=order - i {
                    next[i + j] += result[i] * b[j];
                }
            }
            result = next;
        }
        return Some(result);
    }

    // Known function calls
    if let Some((name, args)) = expr.cas_function_parts() {
        let mut table = taylor_series(name)?;
        // Pad to order+1 if the table is shorter
        table.resize(order + 1, 0.0);
        // For ln(1+x): arg is (+ 1 x)
        if name == CasFunction::Ln && args.len() == 1 {
            let inner = expand_series(&args[0], var_name, order)?;
            if (inner[0] - 1.0).abs() < 1e-12 {
                return Some(table);
            }
            return None;
        }
        // For exp(x), sin(x), cos(x), tan(x): arg must be just x
        if args.len() == 1 && args[0].cas_var_name() == Some(var_name) {
            return Some(table);
        }
        return None;
    }

    None
}

/// Strategy 2.6: series expansion at x=0.
///
/// For quotients f(x)/g(x) where both -> 0, expand both as Taylor series,
/// cancel common powers of x, and evaluate the leading term.
fn try_series_expansion(expr: &Value, var: &Value) -> WqResult<Option<Value>> {
    let (num, den) = match split_fraction(expr) {
        Some(p) => p,
        None => return Ok(None),
    };
    let var_name = var.cas_var_name().unwrap_or("x");

    // Only apply when both num and den -> 0 at x=0
    if !is_zero_at(&num, var, &Value::Int(0))? {
        return Ok(None);
    }
    if !is_zero_at(&den, var, &Value::Int(0))? {
        return Ok(None);
    }

    // Expand both up to order 6
    let num_series = match expand_series(&num, var_name, 6) {
        Some(s) => s,
        None => return Ok(None),
    };
    let den_series = match expand_series(&den, var_name, 6) {
        Some(s) => s,
        None => return Ok(None),
    };

    // Find lowest non-zero term in each
    let num_start = num_series.iter().position(|&c| c.abs() > 1e-12);
    let den_start = den_series.iter().position(|&c| c.abs() > 1e-12);

    match (num_start, den_start) {
        (Some(ni), Some(di)) if ni >= di => {
            // Cancel x^di: limit = num_coeff[ni] / den_coeff[di] if ni == di,
            // or 0 if ni > di
            if ni == di {
                Ok(Some(Value::float(num_series[ni] / den_series[di])))
            } else {
                Ok(Some(Value::float(0.0)))
            }
        }
        (Some(_), Some(_)) => {
            // num starts before den -> blows up (shouldn't happen for 0/0)
            Ok(None)
        }
        (None, Some(_)) => {
            // num is identically 0 -> limit = 0
            Ok(Some(Value::float(0.0)))
        }
        _ => Ok(None),
    }
}

/// Strategy 3: pole analysis -- handle limits where substitution fails due to
/// division by zero and the numerator approaches a non-zero constant.
///
/// `limit(1/x, x->0+) = inf`, `limit(1/x, x->0-) = -inf`
fn try_pole_limit(
    expr: &Value,
    var: &Value,
    point: &Value,
    direction: Option<LimitDirection>,
) -> WqResult<Option<Value>> {
    let (num, den) = match split_fraction(expr) {
        Some(pair) => pair,
        None => return Ok(None),
    };

    // Numerator must approach a non-zero value at the point.
    let num_at_point = match substitute_cas(&num, var, point) {
        Ok(v) if v.as_f64().map(|f| f != 0.0).unwrap_or(true) => v,
        _ => return Ok(None),
    };
    let num_sign = num_at_point
        .as_f64()
        .map(|f| f.signum() as i32)
        .unwrap_or(1);

    // Denominator must approach 0 at the point.
    let den_at_point = match substitute_cas(&den, var, point) {
        Ok(v) => v,
        Err(_) => return Ok(None), // denominator substitution itself fails
    };
    if den_at_point.as_f64().map(|f| f != 0.0).unwrap_or(true) {
        return Ok(None); // denominator doesn't vanish
    }

    // Probe denominator sign near the point using a small epsilon.
    let den_sign = probe_denominator_sign(&den, var, point, direction)?;
    let den_sign = match den_sign {
        Some(s) => s,
        None => return Ok(None),
    };
    if den_sign == 0 {
        // Two-sided: denominator changes sign -> limit doesn't exist
        return Ok(Some(Value::from_cas_const(CasConst::Undefined)));
    }

    let result_sign = num_sign * den_sign;
    Ok(Some(if result_sign > 0 {
        Value::from_cas_const(CasConst::Infinity)
    } else {
        Value::from_cas_const(CasConst::NegInfinity)
    }))
}

/// Probe the sign of `den(var)` as `var` approaches `point` from the given
/// direction.  Returns `Some(sign)` where sign is 1 or -1, or `None` if the
/// probing failed (e.g. the limit point is not a float).
///
/// Uses a small epsilon (1e-10) to evaluate the denominator near the point.
fn probe_denominator_sign(
    den: &Value,
    var: &Value,
    point: &Value,
    direction: Option<LimitDirection>,
) -> WqResult<Option<i32>> {
    let base = match point.as_f64() {
        Some(f) => f,
        None => return Ok(None), // non-numeric point (e.g. symbolic)
    };
    let eps = 1e-10;

    let sign_at = |offset: f64| -> Option<i32> {
        let probe = Value::float(base + offset);
        match substitute_cas(den, var, &probe) {
            Ok(v) => v.as_f64().map(|f| {
                if f > 0.0 {
                    1
                } else if f < 0.0 {
                    -1
                } else {
                    0
                }
            }),
            Err(_) => None,
        }
    };

    let result = match direction {
        None => {
            let right = sign_at(eps);
            let left = sign_at(-eps);
            match (right, left) {
                (Some(r), Some(l)) if r != l => Some(0),
                (Some(r), Some(_)) => Some(r),
                _ => None,
            }
        }
        Some(LimitDirection::Right) => sign_at(eps),
        Some(LimitDirection::Left) => sign_at(-eps),
    };
    Ok(result)
}

pub(crate) fn parse_limit_direction(value: &Value) -> Option<LimitDirection> {
    match value.cas_var_name() {
        Some("+") => Some(LimitDirection::Right),
        Some("-") => Some(LimitDirection::Left),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cas_var(name: &str) -> Value {
        Value::from_cas_var(name)
    }

    fn cas_div_expr(num: Value, den: Value) -> Value {
        Value::from_cas_op(CasOp::Divide, vec![num, den])
    }

    fn op(op: CasOp, args: Vec<Value>) -> Value {
        Value::from_cas_op(op, args)
    }

    fn call(function: CasFunction, args: Vec<Value>) -> Value {
        Value::from_cas_function(function, args)
    }

    fn konst(konst: CasConst) -> Value {
        Value::from_cas_const(konst)
    }

    // -- helpers --

    #[test]
    fn split_fraction_simple_reciprocal() {
        // 1/x -> num=1, den=x
        let expr = simplify_cas_value(&cas_div_expr(Value::Int(1), cas_var("x"))).unwrap();
        let (num, den) = split_fraction(&expr).unwrap();
        assert_eq!(num, Value::Int(1));
        assert_eq!(den, cas_var("x"));
    }

    #[test]
    fn split_fraction_product_num() {
        // sin(x)/x -> num=sin(x), den=x
        let expr = simplify_cas_value(&cas_div_expr(
            call(CasFunction::Sin, vec![cas_var("x")]),
            cas_var("x"),
        ))
        .unwrap();
        let (num, den) = split_fraction(&expr).unwrap();
        assert_eq!(den, cas_var("x"));
        assert!(num.is_cas_expr());
    }

    #[test]
    fn split_fraction_no_denom_returns_none() {
        // x+1 has no denominator
        let expr = simplify_cas_value(&op(CasOp::Add, vec![cas_var("x"), Value::Int(1)])).unwrap();
        assert_eq!(split_fraction(&expr), None);
    }

    // -- direct substitution --

    #[test]
    fn limit_var_approaching_zero() {
        let result = limit_cas(&cas_var("x"), &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_constant() {
        let result = limit_cas(&Value::Int(2), &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn limit_linear_expression() {
        let expr = op(CasOp::Add, vec![cas_var("x"), Value::Int(1)]);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(2), None).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn limit_polynomial() {
        let expr = op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(3), None).unwrap();
        assert_eq!(result, Value::Int(9));
    }

    #[test]
    fn limit_sin_at_zero() {
        let expr = call(CasFunction::Sin, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, Value::float(0.0));
    }

    #[test]
    fn limit_cancelling_fraction() {
        let num = op(
            CasOp::Subtract,
            vec![
                op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let den = op(CasOp::Subtract, vec![cas_var("x"), Value::Int(1)]);
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(1), None).unwrap();
        assert_eq!(result, Value::Int(2));
    }

    #[test]
    fn limit_x_over_x() {
        let expr = cas_div_expr(cas_var("x"), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, Value::Int(1));
    }

    // -- L'Hopital --

    #[test]
    fn limit_sin_x_over_x_lhopital() {
        // limit(sin(x)/x, x->0) = 1
        // 0/0 -> diff: cos(x)/1 -> sub 0 -> 1
        let expr = cas_div_expr(call(CasFunction::Sin, vec![cas_var("x")]), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    #[test]
    fn limit_one_minus_cos_over_x() {
        // limit((1-cos(x))/x, x->0) = 0
        // 0/0 -> diff: sin(x)/1 -> sub 0 -> 0
        let num = op(
            CasOp::Subtract,
            vec![Value::Int(1), call(CasFunction::Cos, vec![cas_var("x")])],
        );
        let expr = cas_div_expr(num, cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, Value::float(0.0));
    }

    #[test]
    fn limit_exp_minus_one_over_x() {
        // limit((e^x-1)/x, x->0) = 1
        // 0/0 -> L'Hopital: e^x/1 -> sub 0 -> 1
        // (ln(e)->1 fix makes diff(e^x) = e^x, not ln(e)*e^x)
        let num = op(
            CasOp::Subtract,
            vec![call(CasFunction::Exp, vec![cas_var("x")]), Value::Int(1)],
        );
        let expr = cas_div_expr(num, cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    #[test]
    fn limit_tan_x_over_x() {
        // limit(tan(x)/x, x->0) = 1
        // 0/0 -> diff: sec^2(x)/1 -> sub 0 -> 1
        let expr = cas_div_expr(call(CasFunction::Tan, vec![cas_var("x")]), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    // -- still unevaluated --

    #[test]
    fn limit_one_over_x_at_zero_two_sided_undef() {
        // limit(1/x, x->0) = undef (two-sided -- left=-inf, right=+inf)
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, konst(CasConst::Undefined));
    }

    // -- limits at infinity --

    fn inf() -> Value {
        konst(CasConst::Infinity)
    }

    fn ninf() -> Value {
        konst(CasConst::NegInfinity)
    }

    #[test]
    fn limit_one_over_x_at_infinity() {
        // limit(1/x, x->inf) = 0
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_one_over_x_at_neg_infinity() {
        // limit(1/x, x->-inf) = 0
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &ninf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_one_over_x_squared_at_infinity() {
        // limit(1/x^2, x->inf) = 0
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]);
        let expr = cas_div_expr(Value::Int(1), den);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_x_at_infinity() {
        // limit(x, x->inf) = inf
        let result = limit_cas(&cas_var("x"), &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, inf());
    }

    #[test]
    fn limit_rational_same_degree() {
        // limit((x+1)/(x-1), x->inf) = 1
        let num = op(CasOp::Add, vec![cas_var("x"), Value::Int(1)]);
        let den = op(CasOp::Subtract, vec![cas_var("x"), Value::Int(1)]);
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    #[test]
    fn limit_rational_num_less_deg() {
        // limit((x+1)/(x^2-1), x->inf) = 0
        let num = op(CasOp::Add, vec![cas_var("x"), Value::Int(1)]);
        let den = op(
            CasOp::Subtract,
            vec![
                op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]),
                Value::Int(1),
            ],
        );
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_exp_neg_x_at_infinity() {
        // limit(exp(-x), x->inf) = 0
        let arg = op(CasOp::Subtract, vec![cas_var("x")]);
        let expr = call(CasFunction::Exp, vec![arg]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_exp_x_at_neg_infinity() {
        // limit(exp(x), x->-inf) = 0
        let expr = call(CasFunction::Exp, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &ninf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_e_to_x_at_negated_oo() {
        let expr = op(CasOp::Power, vec![konst(CasConst::E), cas_var("x")]);
        let negated_oo = crate::cas::cas_neg(konst(CasConst::Infinity)).unwrap();
        let result = limit_cas(&expr, &cas_var("x"), &negated_oo, None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_exp_x_at_infinity() {
        // limit(exp(x), x->inf) = inf
        let expr = call(CasFunction::Exp, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, inf());
    }

    // -- pole limits (one-sided -> +/-inf) --

    #[test]
    fn limit_one_over_x_right() {
        // limit(1/x, x->0+) = inf
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(
            &expr,
            &cas_var("x"),
            &Value::Int(0),
            Some(LimitDirection::Right),
        )
        .unwrap();
        assert_eq!(result, inf());
    }

    #[test]
    fn limit_one_over_x_left() {
        // limit(1/x, x->0-) = -inf
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(
            &expr,
            &cas_var("x"),
            &Value::Int(0),
            Some(LimitDirection::Left),
        )
        .unwrap();
        assert_eq!(result, ninf());
    }

    #[test]
    fn limit_one_over_x_two_sided_undef() {
        // limit(1/x, x->0) = undef (two-sided doesn't exist)
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, konst(CasConst::Undefined));
    }

    #[test]
    fn limit_one_over_x_squared_right() {
        // limit(1/x^2, x->0+) = inf (even power, same both sides)
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]);
        let expr = cas_div_expr(Value::Int(1), den);
        let result = limit_cas(
            &expr,
            &cas_var("x"),
            &Value::Int(0),
            Some(LimitDirection::Right),
        )
        .unwrap();
        assert_eq!(result, inf());
    }

    #[test]
    fn limit_one_over_x_squared_two_sided() {
        // limit(1/x^2, x->0) = inf (even power, limit exists two-sided)
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]);
        let expr = cas_div_expr(Value::Int(1), den);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result, inf());
    }

    #[test]
    fn limit_neg_one_over_x_right() {
        // limit(-1/x, x->0+) = -inf
        let num = Value::Int(-1);
        let expr = cas_div_expr(num, cas_var("x"));
        let result = limit_cas(
            &expr,
            &cas_var("x"),
            &Value::Int(0),
            Some(LimitDirection::Right),
        )
        .unwrap();
        assert_eq!(result, ninf());
    }

    #[test]
    fn limit_one_over_x_minus_1_right() {
        // limit(1/(x-1), x->1+) = inf
        let den = op(CasOp::Subtract, vec![cas_var("x"), Value::Int(1)]);
        let expr = cas_div_expr(Value::Int(1), den);
        let result = limit_cas(
            &expr,
            &cas_var("x"),
            &Value::Int(1),
            Some(LimitDirection::Right),
        )
        .unwrap();
        assert_eq!(result, inf());
    }

    #[test]
    fn limit_one_over_x_minus_1_left() {
        // limit(1/(x-1), x->1-) = -inf
        let den = op(CasOp::Subtract, vec![cas_var("x"), Value::Int(1)]);
        let expr = cas_div_expr(Value::Int(1), den);
        let result = limit_cas(
            &expr,
            &cas_var("x"),
            &Value::Int(1),
            Some(LimitDirection::Left),
        )
        .unwrap();
        assert_eq!(result, ninf());
    }

    // -- inf/inf L'Hopital (partial) --

    #[test]
    fn limit_x_over_exp_x_at_infinity() {
        // limit(x/exp(x), x->inf) = 0 -- inf/inf L'Hopital via
        // split_inf_times_zero_product.
        let expr = cas_div_expr(cas_var("x"), call(CasFunction::Exp, vec![cas_var("x")]));
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    // -- product analysis at infinity --

    #[test]
    fn limit_two_x_at_infinity() {
        // limit(2*x, x->inf) = inf (finite coeff * inf)
        let expr = op(CasOp::Multiply, vec![Value::Int(2), cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, inf());
    }

    #[test]
    fn limit_neg_x_at_infinity() {
        // limit(-x, x->inf) = -inf
        let expr = op(CasOp::Multiply, vec![Value::Int(-1), cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, ninf());
    }

    #[test]
    fn limit_x_times_exp_neg_x_at_infinity() {
        // limit(x*e^(-x), x->inf) = 0 (exp dominates polynomial)
        let x = cas_var("x");
        let exp_neg_x = call(CasFunction::Exp, vec![op(CasOp::Subtract, vec![x.clone()])]);
        let expr = op(CasOp::Multiply, vec![x, exp_neg_x]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    // -- known limits table --

    #[test]
    fn limit_ln_one_plus_x_over_x() {
        let ln_arg = op(CasOp::Add, vec![Value::Int(1), cas_var("x")]);
        let expr = cas_div_expr(call(CasFunction::Ln, vec![ln_arg]), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    // -- series expansion --

    #[test]
    fn series_expand_tan() {
        // Sanity check: expand tan(x) as series
        let tan_x = call(CasFunction::Tan, vec![cas_var("x")]);
        let s = expand_series(&tan_x, "x", 6).unwrap();
        assert!((s[1] - 1.0).abs() < 1e-12, "tan coeff x^1 = 1");
        assert!((s[3] - 1.0 / 3.0).abs() < 1e-10, "tan coeff x^3 = 1/3");
    }

    #[test]
    fn limit_tan_x_minus_x_over_x_cubed() {
        // limit((tan(x)-x)/x^3, x->0) = 1/3  (series expansion)
        let num = op(
            CasOp::Subtract,
            vec![call(CasFunction::Tan, vec![cas_var("x")]), cas_var("x")],
        );
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(3)]);
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert!((result.as_f64().unwrap() - 1.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn limit_sin_x_minus_x_over_x_cubed() {
        // limit((sin(x)-x)/x^3, x->0) = -1/6  (series: -x^3/6 / x^3)
        let num = op(
            CasOp::Subtract,
            vec![call(CasFunction::Sin, vec![cas_var("x")]), cas_var("x")],
        );
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(3)]);
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert!((result.as_f64().unwrap() + 1.0 / 6.0).abs() < 1e-10);
    }

    #[test]
    fn limit_cos_x_minus_1_over_x_squared() {
        // limit((cos(x)-1)/x^2, x->0) = -1/2  (series: -x^2/2 / x^2)
        let num = op(
            CasOp::Subtract,
            vec![call(CasFunction::Cos, vec![cas_var("x")]), Value::Int(1)],
        );
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]);
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert!((result.as_f64().unwrap() + 0.5).abs() < 1e-10);
    }

    #[test]
    fn limit_exp_x_minus_1_minus_x_over_x_squared() {
        // limit((exp(x)-1-x)/x^2, x->0) = 1/2  (series: x^2/2 / x^2)
        let num = op(
            CasOp::Subtract,
            vec![
                op(
                    CasOp::Subtract,
                    vec![call(CasFunction::Exp, vec![cas_var("x")]), Value::Int(1)],
                ),
                cas_var("x"),
            ],
        );
        let den = op(CasOp::Power, vec![cas_var("x"), Value::Int(2)]);
        let expr = cas_div_expr(num, den);
        let result = limit_cas(&expr, &cas_var("x"), &Value::Int(0), None).unwrap();
        assert!((result.as_f64().unwrap() - 0.5).abs() < 1e-10);
    }

    // -- trig asymptotics at infinity --

    #[test]
    fn limit_arctan_at_infinity() {
        // limit(arctan(x), x->inf) = pi/2
        let expr = call(CasFunction::ArcTan, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert!((result.as_f64().unwrap() - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    }

    #[test]
    fn limit_tanh_at_infinity() {
        // limit(tanh(x), x->inf) = 1
        let expr = call(CasFunction::Tanh, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    #[test]
    fn limit_sin_at_infinity_undef() {
        // limit(sin(x), x->inf) = undef (bounded oscillation)
        let expr = call(CasFunction::Sin, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, konst(CasConst::Undefined));
    }

    #[test]
    fn limit_cos_at_infinity_undef() {
        // limit(cos(x), x->inf) = undef
        let expr = call(CasFunction::Cos, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, konst(CasConst::Undefined));
    }

    #[test]
    fn limit_sin_one_over_x_at_infinity() {
        // limit(sin(1/x), x->inf) = 0 (inner -> 0)
        let inner = cas_div_expr(Value::Int(1), cas_var("x"));
        let expr = call(CasFunction::Sin, vec![inner]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result, Value::float(0.0));
    }

    #[test]
    fn limit_heaviside_at_infinity() {
        let expr = call(CasFunction::Heaviside, vec![cas_var("x")]);
        let result = limit_cas(&expr, &cas_var("x"), &inf(), None).unwrap();
        assert_eq!(result.as_f64().unwrap(), 1.0);
    }

    #[test]
    fn limit_a_to_the_n() {
        // limit(1/2^n, n->inf) = 0
        let expr = cas_div_expr(
            Value::Int(1),
            op(CasOp::Power, vec![Value::Int(2), cas_var("n")]),
        );
        let result = limit_cas(&expr, &cas_var("n"), &inf(), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    #[test]
    fn limit_runtime_float_infinity_matches_symbolic_infinity() {
        let expr = cas_div_expr(Value::Int(1), cas_var("x"));
        let result = limit_cas(&expr, &cas_var("x"), &Value::float(f64::INFINITY), None).unwrap();
        assert_eq!(result, Value::Int(0));
    }

    // -- Limit infrastructure: constants and direction parsing --

    #[test]
    fn cas_const_infinity_displays() {
        assert_eq!(konst(CasConst::Infinity).to_string(), "inf");
        assert_eq!(konst(CasConst::NegInfinity).to_string(), "-inf");
        assert_eq!(konst(CasConst::Undefined).to_string(), "undef");
    }

    #[test]
    fn cas_const_infinity_is_cas_expr() {
        assert!(konst(CasConst::Infinity).is_cas_expr());
        assert!(konst(CasConst::NegInfinity).is_cas_expr());
    }

    #[test]
    fn cas_const_infinity_const_name() {
        assert_eq!(konst(CasConst::Infinity).cas_const_name(), Some("inf"));
        assert_eq!(konst(CasConst::NegInfinity).cas_const_name(), Some("-inf"));
    }

    #[test]
    fn eval_numeric_infinity() {
        let pos = crate::cas::eval_numeric_cas(&konst(CasConst::Infinity)).unwrap();
        assert!(pos.as_f64().unwrap().is_infinite());
        assert!(pos.as_f64().unwrap().is_sign_positive());

        let neg = crate::cas::eval_numeric_cas(&konst(CasConst::NegInfinity)).unwrap();
        assert!(neg.as_f64().unwrap().is_infinite());
        assert!(neg.as_f64().unwrap().is_sign_negative());
    }

    #[test]
    fn eval_numeric_undef_is_error() {
        assert!(crate::cas::eval_numeric_cas(&konst(CasConst::Undefined)).is_err());
    }

    #[test]
    fn parse_limit_direction_right() {
        assert_eq!(
            parse_limit_direction(&Value::from_cas_var("+")),
            Some(LimitDirection::Right)
        );
    }

    #[test]
    fn parse_limit_direction_left() {
        assert_eq!(
            parse_limit_direction(&Value::from_cas_var("-")),
            Some(LimitDirection::Left)
        );
    }

    #[test]
    fn parse_limit_direction_invalid_is_none() {
        assert_eq!(parse_limit_direction(&Value::from_cas_var("x")), None);
        assert_eq!(parse_limit_direction(&Value::Int(0)), None);
        assert_eq!(parse_limit_direction(&konst(CasConst::Infinity)), None);
        assert_eq!(
            parse_limit_direction(&crate::value::into_wq_string("+")),
            None
        );
        assert_eq!(
            parse_limit_direction(&crate::value::into_wq_string("")),
            None
        );
    }

    #[test]
    fn limit_direction_display() {
        assert_eq!(format!("{:?}", LimitDirection::Right), "Right");
        assert_eq!(format!("{:?}", LimitDirection::Left), "Left");
    }

    // -- Limit expression node construction/destruction --

    #[test]
    fn from_cas_limit_two_sided() {
        let expr = Value::from_cas_var("x");
        let var = Value::from_cas_var("x");
        let point = Value::Int(0);
        let limit = Value::from_cas_limit(expr.clone(), var.clone(), point.clone(), None);
        let (e, v, p, d) = limit.cas_limit_parts().unwrap();
        assert_eq!(e, &expr);
        assert_eq!(v, &var);
        assert_eq!(p, &point);
        assert_eq!(d, None);
    }

    #[test]
    fn from_cas_limit_one_sided_right() {
        let limit = Value::from_cas_limit(
            Value::from_cas_var("x"),
            Value::from_cas_var("x"),
            Value::Int(0),
            Some(LimitDirection::Right),
        );
        let (_, _, _, d) = limit.cas_limit_parts().unwrap();
        assert_eq!(d, Some(LimitDirection::Right));
    }

    #[test]
    fn from_cas_limit_one_sided_left() {
        let limit = Value::from_cas_limit(
            Value::from_cas_var("x"),
            Value::from_cas_var("x"),
            Value::Int(0),
            Some(LimitDirection::Left),
        );
        let (_, _, _, d) = limit.cas_limit_parts().unwrap();
        assert_eq!(d, Some(LimitDirection::Left));
    }

    #[test]
    fn from_cas_limit_at_infinity() {
        let limit = Value::from_cas_limit(
            op(CasOp::Divide, vec![Value::Int(1), Value::from_cas_var("x")]),
            Value::from_cas_var("x"),
            konst(CasConst::Infinity),
            None,
        );
        assert!(limit.is_cas_expr());
        let (e, v, p, d) = limit.cas_limit_parts().unwrap();
        assert!(e.is_cas_expr());
        assert_eq!(v, &Value::from_cas_var("x"));
        assert_eq!(p, &konst(CasConst::Infinity));
        assert_eq!(d, None);
    }

    #[test]
    fn cas_limit_parts_rejects_non_limit_call() {
        let sin = call(CasFunction::Sin, vec![Value::from_cas_var("x")]);
        assert_eq!(sin.cas_limit_parts(), None);
    }

    #[test]
    fn cas_limit_parts_rejects_non_call() {
        assert_eq!(Value::Int(0).cas_limit_parts(), None);
        assert_eq!(Value::from_cas_var("x").cas_limit_parts(), None);
    }

    #[test]
    fn limit_expression_roundtrips() {
        let original = Value::from_cas_limit(
            call(CasFunction::Sin, vec![Value::from_cas_var("x")]),
            Value::from_cas_var("x"),
            Value::Int(0),
            Some(LimitDirection::Right),
        );
        let (expr, var, point, dir) = original.cas_limit_parts().unwrap();
        let reconstructed = Value::from_cas_limit(expr.clone(), var.clone(), point.clone(), dir);
        assert_eq!(original, reconstructed);
    }
}
