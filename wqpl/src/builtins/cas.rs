use crate::builtins::{
    BuiltinContext, BuiltinEnum, BuiltinFnArgs, check_arity, check_arity_any_named,
    check_registered_args, check_registered_named_args,
};
use crate::cas::diff::diff_cas_with_debug;
use crate::cas::integrate::{definite_integrate_cas_with_debug, integrate_cas_with_debug};
use crate::cas::limit::{limit_cas_with_debug, parse_limit_direction};
use crate::cas::{
    CasAssumptions, CasDebug, SolveDomain, eval_numeric_cas, expand_cas_with_debug, factor_cas,
    infer_single_cas_var, normalize_root_objective_cas, rewrite_cas_with_debug,
    simplify_cas_value_with_debug, solve_cas_with_options, solve_system_cas_with_assumptions,
    solve_system_infer_cas_with_assumptions, substitute_cas, substitute_cas_bindings,
};
use crate::value::cas::{CasOp, CasPredicate};
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn eq(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Eq, [2], &args)?;
    let mut iter = args.into_iter();
    let a = iter.next().unwrap();
    let b = iter.next().unwrap();
    Ok(Value::from_cas_eq(a, b))
}

fn predicate(
    args: BuiltinFnArgs,
    builtin: BuiltinEnum,
    make: impl FnOnce(Value) -> CasPredicate,
) -> WqResult<Value> {
    check_arity(builtin, [1], &args)?;
    let expr = &args[0];
    if expr.is_cas_equation() || expr.cas_predicate().is_some() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(builtin)
            .msg(format!(
                "'{}' expects a numeric or symbolic expression",
                builtin.name()
            ))
            .got1(expr));
    }
    Ok(Value::from_cas_predicate(make(expr.clone())))
}

pub(super) fn zero(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Zero, CasPredicate::Zero)
}

pub(super) fn nonzero(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Nonzero, CasPredicate::NonZero)
}

pub(super) fn positive(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Positive, CasPredicate::Positive)
}

pub(super) fn negative(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Negative, CasPredicate::Negative)
}

pub(super) fn nonnegative(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Nonnegative, CasPredicate::NonNegative)
}

pub(super) fn real(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Real, CasPredicate::Real)
}

pub(super) fn integer(args: BuiltinFnArgs) -> WqResult<Value> {
    predicate(args, BuiltinEnum::Integer, CasPredicate::Integer)
}

pub(super) fn simplify(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Simplify, [1], &args)?;
    simplify_cas_value_with_debug(&args[0], CasDebug::from_context(context))
}

pub(super) fn rewrite(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Rewrite, [1], &args)?;
    rewrite_cas_with_debug(&args[0], CasDebug::from_context(context))
}

pub(super) fn numeric(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity_any_named(BuiltinEnum::Numeric, [1], &args)?;
    let expr = if args.has_named() {
        substitute_cas_bindings(&args[0], args.named_items())
            .map_err(|e| e.src(BuiltinEnum::Numeric))?
    } else {
        args[0].clone()
    };
    eval_numeric_cas(&expr).map_err(|e| e.src(BuiltinEnum::Numeric))
}

pub(super) fn diff(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Diff, [1, 2], &args)?;
    let n = args.len();
    let mut iter = args.into_iter();
    let expr = iter.next().unwrap();
    let var = if n == 1 {
        let inferred = infer_single_cas_var(&expr).map_err(|e| e.src(BuiltinEnum::Diff))?;
        Value::from_cas_var(inferred)
    } else {
        iter.next().unwrap()
    };
    diff_cas_with_debug(&expr, &var, CasDebug::from_context(context))
}

pub(super) fn substitute(args: BuiltinFnArgs) -> WqResult<Value> {
    let arity = if args.has_named() {
        &[1, 2, 3][..]
    } else {
        &[2, 3][..]
    };
    check_arity_any_named(BuiltinEnum::Substitute, arity, &args)?;
    let named = args.named_items().to_vec();
    let mut iter = args.into_iter();
    let first = iter.next().expect("substitute arity checked");
    let Some(second) = iter.next() else {
        return substitute_cas_bindings(&first, &named).map_err(|e| e.src(BuiltinEnum::Substitute));
    };

    if let Some(third) = iter.next() {
        let result = substitute_cas(&first, &second, &third)?;
        return substitute_cas_bindings(&result, &named)
            .map_err(|e| e.src(BuiltinEnum::Substitute));
    }
    let result = if let Some((lhs, rhs)) = second.cas_eq_parts() {
        substitute_cas(&first, lhs, rhs)?
    } else {
        let items = match second {
            Value::List(items) => items,
            other => {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Substitute)
                    .msg("'substitute' expects an equation or a list of equations")
                    .got1(&other));
            }
        };
        let mut result = first;
        for item in items.iter() {
            let Some((lhs, rhs)) = item.cas_eq_parts() else {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Substitute)
                    .msg("'substitute' expects a list of equations")
                    .got1(item));
            };
            result = substitute_cas(&result, lhs, rhs)?;
        }
        result
    };
    substitute_cas_bindings(&result, &named).map_err(|e| e.src(BuiltinEnum::Substitute))
}

pub(super) fn expand(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Expand, [1], &args)?;
    expand_cas_with_debug(&args[0], CasDebug::from_context(context))
}

pub(super) fn factor_common(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::FactorCommon, [1], &args)?;
    factor_cas(&args[0])
}

pub(super) fn integrate(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Integrate, [1, 2, 4], &args)?;
    let debug = CasDebug::from_context(context);
    if args.len() >= 4 {
        return definite_integrate_cas_with_debug(&args[0], &args[1], &args[2], &args[3], debug);
    }
    let n = args.len();
    let mut iter = args.into_iter();
    let expr = iter.next().unwrap();
    let var = if n == 1 {
        let inferred = infer_single_cas_var(&expr).map_err(|e| e.src(BuiltinEnum::Integrate))?;
        Value::from_cas_var(inferred)
    } else {
        iter.next().unwrap()
    };
    integrate_cas_with_debug(&expr, &var, debug)
}

fn inferred_limit_var(expr: &Value) -> WqResult<Value> {
    let inferred = infer_single_cas_var(expr).map_err(|e| e.src(BuiltinEnum::Limit))?;
    Ok(Value::from_cas_var(inferred))
}

fn required_limit_var(value: Value) -> WqResult<Value> {
    if value.cas_var_name().is_some() && parse_limit_direction(&value).is_none() {
        Ok(value)
    } else {
        Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Limit)
            .msg("'limit' target must be a symbolic variable")
            .got1(&value))
    }
}

fn required_limit_direction(value: &Value) -> WqResult<crate::cas::limit::LimitDirection> {
    parse_limit_direction(value).ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Limit)
            .msg("'limit' direction must be '@s+' or '@s-'")
            .got1(value)
    })
}

pub(super) fn limit(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    // Inferred form: expr, point.
    // Explicit forms: expr, var1, point1.
    // Additional args come in (var, point) pairs. The named argument `direction`
    // supplies the final direction.
    check_registered_named_args(&args, BuiltinEnum::Limit)?;
    let direction = args
        .named("direction")
        .map(required_limit_direction)
        .transpose()?;
    let debug = CasDebug::from_context(context);
    let argc = args.len();
    if argc < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::Limit)
            .msg("'limit' expects at least 2 arguments: 'limit[expr;point]'"));
    }
    let mut iter = args.into_iter();
    let mut result = iter.next().unwrap();

    if argc == 2 {
        let point = iter.next().unwrap();
        let var = inferred_limit_var(&result)?;
        return limit_cas_with_debug(&result, &var, &point, direction, debug);
    }

    let n = argc - 1;
    if !n.is_multiple_of(2) {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::Limit)
            .msg(
                "'limit' expects 'limit[expr;point]' or 'limit[expr;var;point]', optionally followed by additional 'var;point' pairs",
            ));
    }

    let n_pairs = n / 2;

    for i in 0..n_pairs {
        let var = required_limit_var(iter.next().unwrap())?;
        let point = iter.next().unwrap();
        let dir = if i == n_pairs - 1 { direction } else { None };
        result = limit_cas_with_debug(&result, &var, &point, dir, debug)?;
    }
    Ok(result)
}

pub(super) fn solve(args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_args(BuiltinEnum::Solve, &args)?;
    let assumptions = args
        .named("assuming")
        .map(CasAssumptions::from_value)
        .transpose()?
        .unwrap_or_default();
    let domain = args
        .named("domain")
        .map(parse_solve_domain)
        .transpose()?
        .unwrap_or(SolveDomain::Complex);
    let n = args.len();
    let mut iter = args.into_iter();
    let expr = iter.next().unwrap();
    let var = if n == 1 {
        let inferred = infer_single_cas_var(&expr).map_err(|e| e.src(BuiltinEnum::Solve))?;
        Value::from_cas_var(inferred)
    } else {
        iter.next().unwrap()
    };
    solve_cas_with_options(&expr, &var, &assumptions, domain)
}

fn parse_solve_domain(value: &Value) -> WqResult<SolveDomain> {
    let name = match value {
        Value::Tag(name) => name.as_ref(),
        Value::String(name) => name.as_str(),
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg(
                    "'solve' domain must be the tag `complex or `real, or the string \"complex\" or \"real\"",
                )
                .got1(value));
        }
    };
    match name {
        "complex" => Ok(SolveDomain::Complex),
        "real" => Ok(SolveDomain::Real),
        _ => Err(WqError::new(WqErrorType::Domain)
            .msg(
                "'solve' domain must be the tag `complex or `real, or the string \"complex\" or \"real\"",
            )
            .got1(value)),
    }
}

pub(super) fn solve_system(args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_args(BuiltinEnum::SolveSystem, &args)?;
    let assumptions = args
        .named("assuming")
        .map(CasAssumptions::from_value)
        .transpose()?
        .unwrap_or_default();
    if args.len() == 1 {
        solve_system_infer_cas_with_assumptions(&args[0], &assumptions)
    } else {
        solve_system_cas_with_assumptions(&args[0], &args[1], &assumptions)
    }
}

fn parse_root_objective(arg: &Value, src: BuiltinEnum) -> WqResult<(Value, Value)> {
    let expr = normalize_root_objective_cas(arg).map_err(|e| e.src(src))?;
    let var = Value::from_cas_var(infer_single_cas_var(&expr).map_err(|e| e.src(src))?);
    Ok((expr, var))
}

fn eval_root_objective(expr: &Value, var: &Value, x: f64, src: BuiltinEnum) -> WqResult<f64> {
    let value = substitute_cas(expr, var, &Value::float(x)).map_err(|e| e.src(src))?;
    let fx = value.as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(src)
            .msg("root solver expects the symbolic expression to evaluate to a real numeric value")
            .got1(&value)
    })?;
    if fx.is_finite() {
        Ok(fx)
    } else {
        Err(WqError::new(WqErrorType::Domain)
            .src(src)
            .msg("root solver expression evaluated to a non-finite value")
            .got1(&value))
    }
}

pub(super) fn brent_with_context(
    context: &mut dyn BuiltinContext,
    args: BuiltinFnArgs,
) -> WqResult<Value> {
    brent_impl(args, &mut || context.poll_interrupt())
}

#[cfg(test)]
fn brent(args: BuiltinFnArgs) -> WqResult<Value> {
    brent_impl(args, &mut || false)
}

fn brent_impl(args: BuiltinFnArgs, poll_interrupt: &mut dyn FnMut() -> bool) -> WqResult<Value> {
    check_arity(BuiltinEnum::Brent, [3, 4, 5], &args)?;

    let (expr, var) = parse_root_objective(&args[0], BuiltinEnum::Brent)?;
    let mut a = args[1].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Brent)
            .msg("'brent' expects a real lower bound")
            .at_arg(1)
    })?;
    let mut b = args[2].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Brent)
            .msg("'brent' expects a real upper bound")
            .at_arg(2)
    })?;
    let tol = if args.len() >= 4 {
        args[3].as_f64().ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Brent)
                .msg("'brent' expects a real tolerance")
                .at_arg(3)
        })?
    } else {
        1e-12
    };
    let max_iter = if args.len() == 5 {
        usize::try_from(args[4].as_i64().ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Brent)
                .msg("'brent' expects an int iteration limit")
                .at_arg(4)
        })?)
        .map_err(|_| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Brent)
                .msg("'brent' expects a non-negative iteration limit")
                .at_arg(4)
        })?
    } else {
        100
    };

    if !a.is_finite() || !b.is_finite() || a >= b {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Brent)
            .msg("'brent' expects finite bounds with lower < upper"));
    }
    if !tol.is_finite() || tol <= 0.0 {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Brent)
            .msg("'brent' expects a positive finite tolerance"));
    }
    if max_iter == 0 {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Brent)
            .msg("'brent' expects a positive iteration limit"));
    }

    let mut fa = eval_root_objective(&expr, &var, a, BuiltinEnum::Brent)?;
    let mut fb = eval_root_objective(&expr, &var, b, BuiltinEnum::Brent)?;
    if fa == 0.0 {
        return Ok(Value::float(a));
    }
    if fb == 0.0 {
        return Ok(Value::float(b));
    }
    if fa.signum() == fb.signum() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Brent)
            .msg("'brent' requires the interval to bracket a root"));
    }

    let mut c = a;
    let mut fc = fa;
    let mut d = b - a;
    let mut e = d;

    for _ in 0..max_iter {
        if poll_interrupt() {
            return Ok(Value::empty_list());
        }
        if fb.signum() == fc.signum() {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            let old_b = b;
            let old_fb = fb;
            a = b;
            fa = fb;
            b = c;
            fb = fc;
            c = old_b;
            fc = old_fb;
        }

        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * tol;
        let xm = 0.5 * (c - b);
        if xm.abs() <= tol1 || fb == 0.0 {
            return Ok(Value::float(b));
        }

        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q) = if a == c {
                (2.0 * xm * s, 1.0 - s)
            } else {
                let q1 = fa / fc;
                let r = fb / fc;
                (
                    s * (2.0 * xm * q1 * (q1 - r) - (b - a) * (r - 1.0)),
                    (q1 - 1.0) * (r - 1.0) * (s - 1.0),
                )
            };
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if 2.0 * p < min1.min(min2) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }

        a = b;
        fa = fb;
        b += if d.abs() > tol1 { d } else { tol1.copysign(xm) };
        fb = eval_root_objective(&expr, &var, b, BuiltinEnum::Brent)?;
    }

    Err(WqError::new(WqErrorType::Domain)
        .src(BuiltinEnum::Brent)
        .msg("'brent' did not converge within the iteration limit"))
}

pub(super) fn newton(context: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Newton, [2, 3, 4], &args)?;

    let (expr, var) = parse_root_objective(&args[0], BuiltinEnum::Newton)?;
    let deriv = diff_cas_with_debug(&expr, &var, CasDebug::from_context(context))
        .map_err(|e| e.src(BuiltinEnum::Newton))?;
    let mut x = args[1].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Newton)
            .msg("'newton' expects a real initial guess")
            .at_arg(1)
    })?;
    let tol = if args.len() >= 3 {
        args[2].as_f64().ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Newton)
                .msg("'newton' expects a real tolerance")
                .at_arg(2)
        })?
    } else {
        1e-12
    };
    let max_iter = if args.len() == 4 {
        usize::try_from(args[3].as_i64().ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Newton)
                .msg("'newton' expects an int iteration limit")
                .at_arg(3)
        })?)
        .map_err(|_| {
            WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Newton)
                .msg("'newton' expects a non-negative iteration limit")
                .at_arg(3)
        })?
    } else {
        50
    };

    if !x.is_finite() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Newton)
            .msg("'newton' expects a finite initial guess"));
    }
    if !tol.is_finite() || tol <= 0.0 {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Newton)
            .msg("'newton' expects a positive finite tolerance"));
    }
    if max_iter == 0 {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Newton)
            .msg("'newton' expects a positive iteration limit"));
    }

    for _ in 0..max_iter {
        if context.poll_interrupt() {
            return Ok(Value::empty_list());
        }
        let fx = eval_root_objective(&expr, &var, x, BuiltinEnum::Newton)?;
        if fx.abs() <= tol {
            return Ok(Value::float(x));
        }

        let dfx = eval_root_objective(&deriv, &var, x, BuiltinEnum::Newton)?;
        if dfx == 0.0 {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Newton)
                .msg("'newton' encountered a zero derivative"));
        }

        let next = x - fx / dfx;
        if !next.is_finite() {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Newton)
                .msg("'newton' produced a non-finite iterate"));
        }
        if (next - x).abs() <= tol {
            return Ok(Value::float(next));
        }
        x = next;
    }

    Err(WqError::new(WqErrorType::Domain)
        .src(BuiltinEnum::Newton)
        .msg("'newton' did not converge within the iteration limit"))
}

pub(super) fn factor_poly(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Factor, [1, 2, 3], &args)?;
    // Parse arguments: factor[expr] | factor[expr; var] |
    //             factor[expr; complex] | factor[expr; complex; var]
    let (complex, var) = if args.len() == 1 {
        (
            false,
            infer_single_cas_var(&args[0]).map_err(|e| e.src(BuiltinEnum::Factor))?,
        )
    } else if args.len() == 2 {
        if matches!(&args[1], Value::Bool(true)) {
            (
                true,
                infer_single_cas_var(&args[0]).map_err(|e| e.src(BuiltinEnum::Factor))?,
            )
        } else if let Some(v) = args[1].cas_var_name() {
            (false, v.to_string())
        } else {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Factor)
                .msg(
                    "'factor' second argument must be a symbolic variable or 'T' for complex factorization",
                )
                .got1(&args[1]));
        }
    } else {
        // args.len() == 3: factor[expr; T; var]
        if !matches!(&args[1], Value::Bool(true)) {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Factor)
                .msg("'factor' with 3 arguments requires 'T' as the second argument")
                .got1(&args[1]));
        }
        match args[2].cas_var_name() {
            Some(v) => (true, v.to_string()),
            None => {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BuiltinEnum::Factor)
                    .msg("'factor' expects a symbolic variable as the third argument")
                    .got1(&args[2]));
            }
        }
    };

    let mut coeffs =
        crate::cas::poly_from_expr(&args[0], &var).map_err(|e| e.src(BuiltinEnum::Factor))?;
    crate::cas::poly_trim(&mut coeffs);
    if crate::cas::poly_degree(&coeffs) == 0 {
        return Ok(coeffs.first().cloned().unwrap_or(Value::Int(0)));
    }

    // Square-free factorization
    let sf_factors =
        crate::cas::square_free_factor(&coeffs).map_err(|e| e.src(BuiltinEnum::Factor))?;

    // Factor each square-free factor
    let mut factored_parts: Vec<(Value, usize)> = Vec::new();
    let mut product_poly = vec![Value::Int(1)];
    for (factor, mult) in sf_factors {
        let sub_factors = if complex {
            factor_polynomial_complex(&factor).map_err(|e| e.src(BuiltinEnum::Factor))?
        } else {
            factor_polynomial_full(&factor).map_err(|e| e.src(BuiltinEnum::Factor))?
        };
        for sub in sub_factors {
            for _ in 0..mult {
                product_poly = crate::cas::poly_mul(&product_poly, &sub)
                    .map_err(|e| e.src(BuiltinEnum::Factor))?;
            }
            let expr = if complex {
                // Complex factors may have complex coefficients
                poly_to_expr_complex(&sub, &var)
            } else {
                crate::cas::poly_to_expr(&sub, &var).map_err(|e| e.src(BuiltinEnum::Factor))?
            };
            factored_parts.push((expr, mult));
        }
    }

    // Build product: ∏ factor^mult
    let mut factors: Vec<Value> = Vec::new();
    let original_lead = coeffs.last().expect("non-constant poly has lead");
    let product_lead = product_poly
        .last()
        .expect("factor product should have a leading coefficient");
    if !crate::cas::numeric_is_zero(product_lead) {
        let scale = crate::cas::eval_exact_numeric_div(original_lead, product_lead)
            .map_err(|e| e.src(BuiltinEnum::Factor))?;
        if !crate::cas::numeric_is_one(&scale) {
            factors.push(scale);
        }
    }
    for (expr, mult) in factored_parts {
        if mult == 1 {
            factors.push(expr);
        } else {
            factors.push(Value::from_cas_op(
                CasOp::Power,
                vec![expr, Value::Int(mult as i64)],
            ));
        }
    }

    match factors.len() {
        0 => Ok(Value::Int(1)),
        1 => Ok(factors.into_iter().next().expect("one factor")),
        _ => Ok(Value::from_cas_op(CasOp::Multiply, factors)),
    }
}

/// Full polynomial factorization including quadratics via discriminant.
fn factor_polynomial_full(poly: &[Value]) -> WqResult<Vec<Vec<Value>>> {
    let deg = crate::cas::poly_degree(poly);
    if deg <= 1 {
        return Ok(vec![poly.to_vec()]);
    }
    if deg == 2 {
        return factor_quadratic(poly);
    }
    // For degree >= 3, use the existing rational.rs factor_polynomial
    crate::cas::integrate::rational::factor_polynomial(poly)
}

/// Factor a quadratic ax²+bx+c over Q. Checks discriminant b²-4ac.
fn factor_quadratic(poly: &[Value]) -> WqResult<Vec<Vec<Value>>> {
    let a = poly.get(2).cloned().unwrap_or(Value::Int(1));
    let b = poly.get(1).cloned().unwrap_or(Value::Int(0));
    let c = poly.first().cloned().unwrap_or(Value::Int(0));

    // Discriminant D = b² - 4ac
    let b_sq =
        crate::cas::eval_numeric_binary("*", &b, &b).map_err(|e| e.src(BuiltinEnum::Factor))?;
    let four_ac = crate::cas::eval_numeric_binary(
        "*",
        &Value::Int(4),
        &crate::cas::eval_numeric_binary("*", &a, &c).map_err(|e| e.src(BuiltinEnum::Factor))?,
    )
    .map_err(|e| e.src(BuiltinEnum::Factor))?;
    let d = crate::cas::eval_numeric_binary("-", &b_sq, &four_ac)
        .map_err(|e| e.src(BuiltinEnum::Factor))?;

    // Check if D is a perfect square rational
    let (d_num, d_den) = match d.rational_parts() {
        Some(parts) => parts,
        None => return Ok(vec![poly.to_vec()]), // not rational, irreducible
    };
    if d_num < num_bigint::BigInt::from(0) {
        return Ok(vec![poly.to_vec()]); // negative discriminant, no real roots
    }
    // Check perfect square: sqrt(d_num/d_den) must be rational
    let sqrt_num = d_num.sqrt();
    let sqrt_den = d_den.sqrt();
    if &sqrt_num * &sqrt_num != d_num || &sqrt_den * &sqrt_den != d_den {
        return Ok(vec![poly.to_vec()]); // not a perfect square
    }

    // Roots: (-b ± √D) / (2a)
    let sqrt_d = Value::from_fraction_parts(sqrt_num, sqrt_den);
    let neg_b = crate::cas::eval_numeric_binary("*", &b, &Value::Int(-1))
        .map_err(|e| e.src(BuiltinEnum::Factor))?;
    let two_a = crate::cas::eval_numeric_binary("*", &Value::Int(2), &a)
        .map_err(|e| e.src(BuiltinEnum::Factor))?;

    let r1 = crate::cas::eval_exact_numeric_div(
        &crate::cas::eval_numeric_binary("+", &neg_b, &sqrt_d)
            .map_err(|e| e.src(BuiltinEnum::Factor))?,
        &two_a,
    )
    .map_err(|e| e.src(BuiltinEnum::Factor))?;
    let r2 = crate::cas::eval_exact_numeric_div(
        &crate::cas::eval_numeric_binary("-", &neg_b, &sqrt_d)
            .map_err(|e| e.src(BuiltinEnum::Factor))?,
        &two_a,
    )
    .map_err(|e| e.src(BuiltinEnum::Factor))?;

    // Build (x - r1) = [-r1, 1], (x - r2) = [-r2, 1]
    let neg_r1 = crate::cas::eval_numeric_binary("*", &r1, &Value::Int(-1))
        .map_err(|e| e.src(BuiltinEnum::Factor))?;
    let neg_r2 = crate::cas::eval_numeric_binary("*", &r2, &Value::Int(-1))
        .map_err(|e| e.src(BuiltinEnum::Factor))?;
    Ok(vec![
        vec![neg_r1, Value::Int(1)],
        vec![neg_r2, Value::Int(1)],
    ])
}

/// Factor any polynomial over C (splits all quadratics via quadratic formula).
fn factor_polynomial_complex(poly: &[Value]) -> WqResult<Vec<Vec<Value>>> {
    let deg = crate::cas::poly_degree(poly);
    if deg <= 1 {
        return Ok(vec![poly.to_vec()]);
    }
    if deg == 2 {
        return factor_quadratic_complex(poly);
    }
    // For degree >= 3: first factor over Q, then split quadratics over C
    let q_factors = factor_polynomial_full(poly)?;
    let mut result = Vec::new();
    for f in q_factors {
        let f_deg = crate::cas::poly_degree(&f);
        if f_deg == 2 {
            result.extend(factor_quadratic_complex(&f)?);
        } else {
            result.push(f);
        }
    }
    Ok(result)
}

fn factor_poly_complex_coeff(value: &Value) -> WqResult<num_complex::Complex64> {
    if let Some(z) = value.as_complex64() {
        return Ok(z);
    }
    let numeric = eval_numeric_cas(value)?;
    numeric.as_complex64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .msg("'factor' complex factorization expects numeric coefficients")
            .got1(value)
    })
}

/// Factor a quadratic ax²+bx+c over C using the quadratic formula.
/// Handles negative discriminants by producing Complex roots.
fn factor_quadratic_complex(poly: &[Value]) -> WqResult<Vec<Vec<Value>>> {
    use num_complex::Complex64;

    let a = poly.get(2).cloned().unwrap_or(Value::Int(1));
    let b = poly.get(1).cloned().unwrap_or(Value::Int(0));
    let c = poly.first().cloned().unwrap_or(Value::Int(0));

    let a = factor_poly_complex_coeff(&a)?;
    let b = factor_poly_complex_coeff(&b)?;
    let c = factor_poly_complex_coeff(&c)?;
    if a.norm() <= 1e-12 {
        return Err(WqError::new(WqErrorType::Domain)
            .msg("'factor' quadratic factorization requires a non-zero leading coefficient"));
    }

    let sqrt_d = (b * b - 4.0 * a * c).sqrt();
    let two_a = 2.0 * a;
    let r1 = (-b + sqrt_d) / two_a;
    let r2 = (-b - sqrt_d) / two_a;

    // Build (x - r1) = [Value::Complex(-r1), Value::Int(1)]
    let neg_r1 = Value::from_complex64(Complex64::new(-r1.re, -r1.im));
    let neg_r2 = Value::from_complex64(Complex64::new(-r2.re, -r2.im));
    Ok(vec![
        vec![neg_r1, Value::Int(1)],
        vec![neg_r2, Value::Int(1)],
    ])
}

/// Build a CAS polynomial expression from coefficient vector that may contain
/// Complex values.
fn poly_to_expr_complex(coeffs: &[Value], var: &str) -> Value {
    let mut terms: Vec<Value> = Vec::new();
    for (deg, coeff) in coeffs.iter().enumerate() {
        if crate::cas::numeric_is_zero(coeff) {
            continue;
        }
        let term = if deg == 0 {
            coeff.clone()
        } else if deg == 1 {
            let x = Value::from_cas_var(var);
            if crate::cas::numeric_is_one(coeff) {
                x
            } else {
                Value::from_cas_op(CasOp::Multiply, vec![coeff.clone(), x])
            }
        } else {
            let x_pow = Value::from_cas_op(
                CasOp::Power,
                vec![Value::from_cas_var(var), Value::Int(deg as i64)],
            );
            if crate::cas::numeric_is_one(coeff) {
                x_pow
            } else {
                Value::from_cas_op(CasOp::Multiply, vec![coeff.clone(), x_pow])
            }
        };
        terms.push(term);
    }
    match terms.len() {
        0 => Value::Int(0),
        1 => terms.into_iter().next().unwrap(),
        _ => Value::from_cas_op(CasOp::Add, terms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cas::simplify_cas_value;
    use crate::value::cas::CasFunction;

    #[test]
    fn solve_domain_accepts_tags_and_strings() {
        assert_eq!(
            parse_solve_domain(&Value::Tag("real".into())).expect("real tag"),
            SolveDomain::Real
        );
        assert_eq!(
            parse_solve_domain(&Value::String("complex".to_string().into()))
                .expect("complex string"),
            SolveDomain::Complex
        );
        let err = parse_solve_domain(&Value::Tag("integer".into()))
            .expect_err("unsupported solve domain should fail");
        assert_eq!(
            err.msg.as_deref(),
            Some(
                "'solve' domain must be the tag `complex or `real, or the string \"complex\" or \"real\""
            )
        );
    }

    #[test]
    fn limit_direction_error_quotes_symbolic_forms() {
        let err = required_limit_direction(&Value::from_cas_var("x"))
            .expect_err("ordinary symbolic variable is not a direction");

        assert_eq!(
            err.msg.as_deref(),
            Some("'limit' direction must be '@s+' or '@s-'")
        );
    }

    #[test]
    fn factor_errors_use_public_builtin_and_argument_terms() {
        let err = factor_poly(vec![Value::from_cas_var("x"), Value::Int(0)].into())
            .expect_err("invalid second argument should fail");
        assert_eq!(
            err.msg.as_deref(),
            Some(
                "'factor' second argument must be a symbolic variable or 'T' for complex factorization"
            )
        );

        let err = factor_poly(
            vec![
                Value::from_cas_var("x"),
                Value::Bool(false),
                Value::from_cas_var("x"),
            ]
            .into(),
        )
        .expect_err("three-argument form requires T");
        assert_eq!(
            err.msg.as_deref(),
            Some("'factor' with 3 arguments requires 'T' as the second argument")
        );
    }

    #[test]
    fn root_solver_iteration_limit_uses_public_int_term() {
        let err = brent(
            vec![
                Value::from_cas_var("x"),
                Value::Int(-1),
                Value::Int(1),
                Value::float(1e-12),
                Value::String("ten".to_string().into()),
            ]
            .into(),
        )
        .expect_err("string iteration limit should fail");

        assert_eq!(
            err.msg.as_deref(),
            Some("'brent' expects an int iteration limit")
        );
    }

    #[test]
    fn brent_stops_at_an_algorithm_interrupt_safe_point() {
        let mut polls = 0;
        let result = brent_impl(
            vec![Value::from_cas_var("x"), Value::Int(-1), Value::Int(1)].into(),
            &mut || {
                polls += 1;
                true
            },
        )
        .expect("interruption should halt without a runtime error");

        assert!(result.is_unit());
        assert_eq!(polls, 1);
    }

    #[test]
    fn complex_quadratic_factorization_uses_algebraic_coefficients() {
        let sqrt2 = simplify_cas_value(&Value::from_cas_function(
            CasFunction::Sqrt,
            vec![Value::Int(2)],
        ))
        .expect("sqrt[2] should simplify");
        let factors = factor_quadratic_complex(&[Value::Int(1), sqrt2, Value::Int(1)])
            .expect("quadratic should factor over complex numbers");

        assert_eq!(factors.len(), 2);
        let first = factors[0][0]
            .as_complex64()
            .expect("first factor constant should be complex");
        let second = factors[1][0]
            .as_complex64()
            .expect("second factor constant should be complex");
        let expected = 0.5_f64.sqrt();

        assert!((first.re - expected).abs() < 1e-12, "{first:?}");
        assert!((second.re - expected).abs() < 1e-12, "{second:?}");
        assert!((first.im.abs() - expected).abs() < 1e-12, "{first:?}");
        assert!((second.im.abs() - expected).abs() < 1e-12, "{second:?}");
        assert!((first.im + second.im).abs() < 1e-12, "{first:?} {second:?}");
    }
}
