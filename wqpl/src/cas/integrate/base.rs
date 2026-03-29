use num_bigint::BigInt;
use num_traits::One as _;

use crate::cas::{
    cas_add, cas_div, cas_mul, cas_neg, cas_pow, cas_sub, eval_numeric_binary,
    extract_linear_coefficients, numeric_is_negative, numeric_is_one, numeric_is_zero, poly_degree,
    poly_from_expr, simplify_cas_value, substitute_expr,
};
use crate::value::{Value, WqResult};

pub(super) fn integrate_by_table(expr: &Value, var: &str) -> WqResult<Option<Value>> {
    // Case 1: f(ax+b) — Call node like exp[2*x], sin[3*x+1]
    if let Some((name, args)) = expr.cas_call_parts()
        && let [arg] = args
        && let Some((a, _b)) = extract_linear_coefficients(arg, var)
    {
        let base = base_integral_of_call(name, var)?;
        if let Some(base) = base {
            let substituted = substitute_expr(&base, var, arg)?;
            return if numeric_is_one(&a) {
                Ok(Some(substituted))
            } else {
                simplify_cas_value(&cas_div(substituted, a)?).map(Some)
            };
        }
    }

    // Case 2: e^(ax+b) — Pow node with Const("e") base
    if let Some(("^", args)) = expr.cas_op_parts()
        && args.len() == 2
        && args[0].cas_const_name() == Some("e")
        && let Some((a, _b)) = extract_linear_coefficients(&args[1], var)
    {
        // ∫ e^(kx+b) dx = e^(kx+b) / k — same as exp[kx+b], returned directly
        let substituted = expr.clone();
        return if numeric_is_one(&a) {
            Ok(Some(substituted))
        } else {
            simplify_cas_value(&cas_div(substituted, a)?).map(Some)
        };
    }

    // Case 3: Gaussian — exp(-a·x^2) → √(π/a)/2 · erf(√a·x)
    if let Some((name, args)) = expr.cas_call_parts()
        && name == "exp"
        && args.len() == 1
        && let Some(result) = try_gaussian_table(&args[0], var)?
    {
        return Ok(Some(result));
    }
    if let Some(("^", args)) = expr.cas_op_parts()
        && args.len() == 2
        && args[0].cas_const_name() == Some("e")
        && let Some(result) = try_gaussian_table(&args[1], var)?
    {
        return Ok(Some(result));
    }

    Ok(None)
}

/// If `arg` is -a·x^2 + b (pure quadratic, no linear term, a > 0),
/// return √(π/a)/2 · e^b · erf(√a·x).
fn try_gaussian_table(arg: &Value, var: &str) -> WqResult<Option<Value>> {
    let coeffs = match poly_from_expr(arg, var) {
        Ok(c) => c,
        Err(_) => return Ok(None),
    };
    let deg = poly_degree(&coeffs);
    if deg != 2 {
        return Ok(None);
    }
    // Must have no linear term
    let b_term = coeffs.get(1).cloned().unwrap_or(Value::Int(0));
    if !numeric_is_zero(&b_term) {
        return Ok(None);
    }
    let a = &coeffs[2];
    // a must be negative for convergence
    if !numeric_is_negative(a) {
        return Ok(None);
    }
    let a_pos = eval_numeric_binary("*", a, &Value::Int(-1))?; // -a > 0

    let g_const = coeffs.first().cloned().unwrap_or(Value::Int(0));

    // Build √(π/a)/2
    let pi = Value::from_cas_const("pi");
    let sqrt_pi = Value::from_cas_call("sqrt", vec![pi]);
    let sqrt_a = Value::from_cas_call("sqrt", vec![a_pos.clone()]);
    let two_sqrt_a = cas_mul(vec![Value::Int(2), sqrt_a.clone()])?;
    let mut factor = simplify_cas_value(&cas_div(sqrt_pi, two_sqrt_a)?)?;

    if !numeric_is_zero(&g_const) {
        let exp_const = Value::from_cas_call("exp", vec![g_const]);
        factor = cas_mul(vec![factor, exp_const])?;
    }

    let x = Value::from_cas_var(var);
    let erf_arg = cas_mul(vec![sqrt_a, x])?;
    let erf_term = Value::from_cas_call("erf", vec![erf_arg]);

    simplify_cas_value(&cas_mul(vec![factor, erf_term])?).map(Some)
}

fn base_integral_of_call(name: &str, var: &str) -> WqResult<Option<Value>> {
    let v = Value::from_cas_var(var);
    let value = match name {
        "sin" => Some(cas_neg(Value::from_cas_call("cos", vec![v.clone()]))?),
        "cos" => Some(Value::from_cas_call("sin", vec![v.clone()])),
        "tan" => Some(cas_neg(Value::from_cas_call(
            "ln",
            vec![Value::from_cas_call("cos", vec![v.clone()])],
        ))?),
        "sec" => Some(Value::from_cas_call(
            "ln",
            vec![cas_add(vec![
                Value::from_cas_call("sec", vec![v.clone()]),
                Value::from_cas_call("tan", vec![v.clone()]),
            ])?],
        )),
        "csc" => Some(cas_neg(Value::from_cas_call(
            "ln",
            vec![cas_add(vec![
                Value::from_cas_call("csc", vec![v.clone()]),
                Value::from_cas_call("cot", vec![v.clone()]),
            ])?],
        ))?),
        "cot" => Some(Value::from_cas_call(
            "ln",
            vec![Value::from_cas_call("sin", vec![v.clone()])],
        )),
        "exp" => Some(Value::from_cas_call("exp", vec![v.clone()])),
        "ln" => Some(cas_sub(
            cas_mul(vec![v.clone(), Value::from_cas_call("ln", vec![v.clone()])])?,
            v.clone(),
        )?),
        "log2" => Some(cas_div(
            cas_sub(
                cas_mul(vec![v.clone(), Value::from_cas_call("ln", vec![v.clone()])])?,
                v.clone(),
            )?,
            Value::from_cas_call("ln", vec![Value::Int(2)]),
        )?),
        "log10" => Some(cas_div(
            cas_sub(
                cas_mul(vec![v.clone(), Value::from_cas_call("ln", vec![v.clone()])])?,
                v.clone(),
            )?,
            Value::from_cas_call("ln", vec![Value::Int(10)]),
        )?),
        "sinh" => Some(Value::from_cas_call("cosh", vec![v.clone()])),
        "cosh" => Some(Value::from_cas_call("sinh", vec![v.clone()])),
        "tanh" => Some(Value::from_cas_call(
            "ln",
            vec![Value::from_cas_call("cosh", vec![v.clone()])],
        )),
        "arcsin" => Some(cas_add(vec![
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("arcsin", vec![v.clone()]),
            ])?,
            Value::from_cas_call(
                "sqrt",
                vec![cas_sub(Value::Int(1), cas_pow(v.clone(), Value::Int(2))?)?],
            ),
        ])?),
        "arccos" => Some(cas_sub(
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("arccos", vec![v.clone()]),
            ])?,
            Value::from_cas_call(
                "sqrt",
                vec![cas_sub(Value::Int(1), cas_pow(v.clone(), Value::Int(2))?)?],
            ),
        )?),
        "arctan" => Some(cas_sub(
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("arctan", vec![v.clone()]),
            ])?,
            cas_mul(vec![
                Value::from_fraction_parts(BigInt::one(), BigInt::from(2)),
                Value::from_cas_call(
                    "ln",
                    vec![cas_add(vec![
                        Value::Int(1),
                        cas_pow(v.clone(), Value::Int(2))?,
                    ])?],
                ),
            ])?,
        )?),
        "arcsinh" => Some(cas_sub(
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("arcsinh", vec![v.clone()]),
            ])?,
            Value::from_cas_call(
                "sqrt",
                vec![cas_add(vec![
                    Value::Int(1),
                    cas_pow(v.clone(), Value::Int(2))?,
                ])?],
            ),
        )?),
        "arccosh" => Some(cas_sub(
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("arccosh", vec![v.clone()]),
            ])?,
            Value::from_cas_call(
                "sqrt",
                vec![cas_sub(cas_pow(v.clone(), Value::Int(2))?, Value::Int(1))?],
            ),
        )?),
        "arctanh" => Some(cas_add(vec![
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("arctanh", vec![v.clone()]),
            ])?,
            cas_mul(vec![
                Value::from_fraction_parts(BigInt::one(), BigInt::from(2)),
                Value::from_cas_call(
                    "ln",
                    vec![cas_sub(Value::Int(1), cas_pow(v.clone(), Value::Int(2))?)?],
                ),
            ])?,
        ])?),
        "abs" => Some(cas_div(
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("abs", vec![v.clone()]),
            ])?,
            Value::Int(2),
        )?),
        "sgn" => Some(Value::from_cas_call("abs", vec![v.clone()])),
        "erf" => Some(cas_add(vec![
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("erf", vec![v.clone()]),
            ])?,
            cas_mul(vec![
                cas_pow(
                    Value::from_cas_const("pi"),
                    Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
                )?,
                Value::from_cas_call("exp", vec![cas_neg(cas_pow(v.clone(), Value::Int(2))?)?]),
            ])?,
        ])?),
        "erfc" => Some(cas_sub(
            cas_mul(vec![
                v.clone(),
                Value::from_cas_call("erfc", vec![v.clone()]),
            ])?,
            cas_mul(vec![
                cas_pow(
                    Value::from_cas_const("pi"),
                    Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
                )?,
                Value::from_cas_call("exp", vec![cas_neg(cas_pow(v.clone(), Value::Int(2))?)?]),
            ])?,
        )?),
        "si" => Some(cas_add(vec![
            cas_mul(vec![v.clone(), Value::from_cas_call("si", vec![v.clone()])])?,
            Value::from_cas_call("cos", vec![v.clone()]),
        ])?),
        "ci" => Some(cas_sub(
            cas_mul(vec![v.clone(), Value::from_cas_call("ci", vec![v.clone()])])?,
            Value::from_cas_call("sin", vec![v.clone()]),
        )?),
        "ei" => Some(cas_sub(
            cas_mul(vec![v.clone(), Value::from_cas_call("ei", vec![v.clone()])])?,
            Value::from_cas_call("exp", vec![v.clone()]),
        )?),
        "heaviside" => Some(cas_mul(vec![
            v.clone(),
            Value::from_cas_call("heaviside", vec![v.clone()]),
        ])?),
        "delta" => Some(Value::from_cas_call("heaviside", vec![v.clone()])),
        _ => None,
    };
    Ok(value)
}
