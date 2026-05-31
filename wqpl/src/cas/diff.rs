use num_bigint::BigInt;

use crate::cas::{
    cas_add, cas_debug_log_depth, cas_div, cas_err, cas_mul, cas_neg, cas_pow, cas_product,
    cas_sub, eval_numeric_binary, numeric_is_one, rewrite_cas, rewrite_loop, simplify_cas_value,
    var_name_from_value,
};
use crate::session::dbglog::DebugLogFlags;
use crate::value::{Value, WqResult};

fn fmt_cas(v: &Value) -> String {
    v.format_cas().unwrap_or_else(|| v.to_string())
}

/// Compute 1 - m·sin²(φ), used by both ellik and ellie derivatives.
fn ell_inner(phi: &Value, m: &Value) -> WqResult<Value> {
    let sin_sq = cas_pow(
        Value::from_cas_call("sin", vec![phi.clone()]),
        Value::Int(2),
    )?;
    cas_sub(Value::Int(1), cas_mul(vec![m.clone(), sin_sq])?)
}

pub(crate) fn diff_cas(expr: &Value, var: &Value) -> WqResult<Value> {
    let var = var_name_from_value(var)?;
    let expr = simplify_cas_value(expr)?;
    let expr_fmt = fmt_cas(&expr);
    cas_trace!(
        DebugLogFlags::CAS,
        "[cas] diff enter: expr={expr_fmt} var={var}"
    );
    let mut current = diff_expr(&expr, &var)?;
    // Apply tree rewrites (sgn/abs, -1 distribution) first, then simplify to
    // combine rational terms. The generalized quotient rule in diff_expr
    // already produces a single fraction for rational-exponential products,
    // avoiding the need to recombine fractions with different denominators.
    rewrite_loop(&mut current)?;
    let result = simplify_cas_value(&current)?;
    let result = rewrite_cas(&result)?;
    cas_trace!(DebugLogFlags::CAS, "[cas] diff exit: {}", fmt_cas(&result));
    Ok(result)
}

pub(super) fn diff_expr(expr: &Value, var: &str) -> WqResult<Value> {
    let expr_fmt = fmt_cas(expr);
    cas_debug_log_depth(
        DebugLogFlags::CAS_VERBOSE,
        0,
        format!("[cas-v] diff_expr enter: {expr_fmt} var={var}"),
    );
    let result = diff_expr_inner(expr, var);
    if let Ok(ref val) = result {
        let val_fmt = fmt_cas(val);
        cas_debug_log_depth(
            DebugLogFlags::CAS_VERBOSE,
            0,
            format!("[cas-v] diff_expr exit: {expr_fmt} -> {val_fmt}"),
        );
    } else {
        cas_debug_log_depth(
            DebugLogFlags::CAS_VERBOSE,
            0,
            format!("[cas-v] diff_expr exit: {expr_fmt} -> Err"),
        );
    }
    result
}

fn diff_expr_inner(expr: &Value, var: &str) -> WqResult<Value> {
    if !expr.is_cas_expr() {
        return Ok(Value::Int(0));
    }
    if let Some(name) = expr.cas_var_name() {
        return Ok(Value::Int((name == var) as i64));
    }
    if expr.cas_const_name().is_some() {
        return Ok(Value::Int(0));
    }
    if let Some((op, args)) = expr.cas_op_parts() {
        let out = match (op, args) {
            ("+", args) => {
                let mut terms = Vec::with_capacity(args.len());
                for arg in args {
                    terms.push(diff_expr(arg, var)?);
                }
                cas_add(terms)?
            }
            ("*", args) => {
                // Generalized quotient rule: for N / D where both are products
                // of arbitrary factors, produce (N'·D − N·D') / D² directly
                // instead of a sum of fractions with different denominators.
                //
                // Separate factors into numerator (positive/unknown exponent)
                // and denominator (negative integer exponent).
                let mut num_factors: Vec<Value> = Vec::new();
                let mut denom_parts: Vec<(Value, Value)> = Vec::new(); // (base, abs_k)

                for arg in args {
                    let mut is_denom = false;
                    if let Some(("^", [base, e])) = arg.cas_op_parts()
                        && let Some(k) = e.exact_int()
                        && k < BigInt::from(0)
                    {
                        let k_abs = Value::from_bigint(-k);
                        denom_parts.push((base.clone(), k_abs));
                        is_denom = true;
                    }
                    if !is_denom {
                        num_factors.push(arg.clone());
                    }
                }

                if !denom_parts.is_empty() {
                    // Build N from numerator factors
                    let n = cas_product(num_factors);

                    // Build D from denominator factors: product of base^k
                    let d_parts: Vec<Value> = denom_parts
                        .iter()
                        .map(|(base, k)| {
                            if numeric_is_one(k) {
                                base.clone()
                            } else {
                                Value::from_cas_op("^", vec![base.clone(), k.clone()])
                            }
                        })
                        .collect();
                    let d = cas_product(d_parts);

                    let n_diff = diff_expr(&n, var)?;
                    let d_diff = diff_expr(&d, var)?;
                    let num = rewrite_cas(&cas_sub(
                        cas_mul(vec![n_diff, d.clone()])?,
                        cas_mul(vec![n.clone(), d_diff])?,
                    )?)?;
                    let denom_factor = Value::from_cas_op("^", vec![d, Value::Int(-2)]);
                    return simplify_cas_value(&cas_mul(vec![num, denom_factor])?);
                }

                // No denominator factors — use the general product rule.
                let mut terms = Vec::with_capacity(args.len());
                for idx in 0..args.len() {
                    let mut factors = Vec::with_capacity(args.len());
                    for (j, arg) in args.iter().enumerate() {
                        factors.push(if idx == j {
                            diff_expr(arg, var)?
                        } else {
                            arg.clone()
                        });
                    }
                    terms.push(cas_mul(factors)?);
                }
                cas_add(terms)?
            }
            ("^", [base, exp]) if !exp.is_cas_expr() => {
                exp.rational_parts().ok_or_else(|| {
                    cas_err("symbolic differentiation currently requires exact rational exponents")
                })?;
                let next = eval_numeric_binary("-", exp, &Value::Int(1))?;
                cas_mul(vec![
                    exp.clone(),
                    cas_pow(base.clone(), next)?,
                    diff_expr(base, var)?,
                ])?
            }
            ("^", [base, exp]) => cas_mul(vec![
                cas_pow(base.clone(), exp.clone())?,
                cas_add(vec![
                    cas_mul(vec![
                        diff_expr(exp, var)?,
                        Value::from_cas_call("ln", vec![base.clone()]),
                    ])?,
                    cas_mul(vec![
                        exp.clone(),
                        cas_div(diff_expr(base, var)?, base.clone())?,
                    ])?,
                ])?,
            ])?,
            _ => {
                return Err(cas_err(format!(
                    "unsupported symbolic derivative for operator '{op}'"
                )));
            }
        };
        return simplify_cas_value(&out);
    }
    if let Some((name, args)) = expr.cas_call_parts() {
        let out = match (name, args) {
            ("sin", [arg]) => cas_mul(vec![
                Value::from_cas_call("cos", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("cos", [arg]) => cas_mul(vec![
                cas_neg(Value::from_cas_call("sin", vec![arg.clone()]))?,
                diff_expr(arg, var)?,
            ])?,
            ("tan", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_pow(
                    Value::from_cas_call("cos", vec![arg.clone()]),
                    Value::Int(2),
                )?,
            )?,
            ("sec", [arg]) => cas_mul(vec![
                Value::from_cas_call("sec", vec![arg.clone()]),
                Value::from_cas_call("tan", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("csc", [arg]) => cas_mul(vec![
                cas_neg(Value::from_cas_call("csc", vec![arg.clone()]))?,
                Value::from_cas_call("cot", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("cot", [arg]) => cas_mul(vec![
                cas_neg(cas_pow(
                    Value::from_cas_call("csc", vec![arg.clone()]),
                    Value::Int(2),
                )?)?,
                diff_expr(arg, var)?,
            ])?,
            ("sqrt", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_mul(vec![
                    Value::Int(2),
                    Value::from_cas_call("sqrt", vec![arg.clone()]),
                ])?,
            )?,
            ("erf", [arg]) => cas_mul(vec![
                Value::Int(2),
                cas_pow(
                    Value::from_cas_const("pi"),
                    Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
                )?,
                Value::from_cas_call("exp", vec![cas_neg(cas_pow(arg.clone(), Value::Int(2))?)?]),
                diff_expr(arg, var)?,
            ])?,
            ("erfc", [arg]) => cas_mul(vec![
                Value::Int(-2),
                cas_pow(
                    Value::from_cas_const("pi"),
                    Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
                )?,
                Value::from_cas_call("exp", vec![cas_neg(cas_pow(arg.clone(), Value::Int(2))?)?]),
                diff_expr(arg, var)?,
            ])?,
            ("si", [arg]) => cas_mul(vec![
                cas_div(Value::from_cas_call("sin", vec![arg.clone()]), arg.clone())?,
                diff_expr(arg, var)?,
            ])?,
            ("ci", [arg]) => cas_mul(vec![
                cas_div(Value::from_cas_call("cos", vec![arg.clone()]), arg.clone())?,
                diff_expr(arg, var)?,
            ])?,
            ("ei", [arg]) => cas_mul(vec![
                cas_div(Value::from_cas_call("exp", vec![arg.clone()]), arg.clone())?,
                diff_expr(arg, var)?,
            ])?,
            ("en", [n, arg]) => {
                // d/dx En(n, x) = -En(n-1, x) for n > 1
                // For n = 1, E1'(x) = -exp(-x)/x
                let dn = if let Some(f) = n.as_f64() {
                    if f > 1.0 {
                        cas_sub(n.clone(), Value::Int(1))?
                    } else {
                        n.clone()
                    }
                } else {
                    n.clone()
                };
                let inner = if n.as_f64().is_some_and(|f| f <= 1.0) {
                    cas_div(
                        Value::from_cas_call("exp", vec![cas_neg(arg.clone())?]),
                        arg.clone(),
                    )?
                } else {
                    Value::from_cas_call("en", vec![dn, arg.clone()])
                };
                cas_mul(vec![cas_neg(inner)?, diff_expr(arg, var)?])?
            }
            ("heaviside", [arg]) => cas_mul(vec![
                Value::from_cas_call("delta", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("exp", [arg]) => cas_mul(vec![
                Value::from_cas_call("exp", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("ln", [arg]) => cas_div(diff_expr(arg, var)?, arg.clone())?,
            ("log2", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_mul(vec![
                    arg.clone(),
                    Value::from_cas_call("ln", vec![Value::Int(2)]),
                ])?,
            )?,
            ("log10", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_mul(vec![
                    arg.clone(),
                    Value::from_cas_call("ln", vec![Value::Int(10)]),
                ])?,
            )?,
            ("sinh", [arg]) => cas_mul(vec![
                Value::from_cas_call("cosh", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("cosh", [arg]) => cas_mul(vec![
                Value::from_cas_call("sinh", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("tanh", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_pow(
                    Value::from_cas_call("cosh", vec![arg.clone()]),
                    Value::Int(2),
                )?,
            )?,
            ("arcsin", [arg]) => cas_div(
                diff_expr(arg, var)?,
                Value::from_cas_call(
                    "sqrt",
                    vec![cas_sub(
                        Value::Int(1),
                        cas_pow(arg.clone(), Value::Int(2))?,
                    )?],
                ),
            )?,
            ("arccos", [arg]) => cas_div(
                cas_neg(diff_expr(arg, var)?)?,
                Value::from_cas_call(
                    "sqrt",
                    vec![cas_sub(
                        Value::Int(1),
                        cas_pow(arg.clone(), Value::Int(2))?,
                    )?],
                ),
            )?,
            ("arctan", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_add(vec![Value::Int(1), cas_pow(arg.clone(), Value::Int(2))?])?,
            )?,
            ("arcsinh", [arg]) => cas_div(
                diff_expr(arg, var)?,
                Value::from_cas_call(
                    "sqrt",
                    vec![cas_add(vec![
                        Value::Int(1),
                        cas_pow(arg.clone(), Value::Int(2))?,
                    ])?],
                ),
            )?,
            ("arccosh", [arg]) => cas_div(
                diff_expr(arg, var)?,
                Value::from_cas_call(
                    "sqrt",
                    vec![cas_sub(
                        cas_pow(arg.clone(), Value::Int(2))?,
                        Value::Int(1),
                    )?],
                ),
            )?,
            ("arctanh", [arg]) => cas_div(
                diff_expr(arg, var)?,
                cas_sub(Value::Int(1), cas_pow(arg.clone(), Value::Int(2))?)?,
            )?,
            ("abs", [arg]) => cas_mul(vec![
                Value::from_cas_call("sgn", vec![arg.clone()]),
                diff_expr(arg, var)?,
            ])?,
            ("sgn", [_arg]) => Value::Int(0),
            ("ellik", [phi, m]) => {
                // d/dx F(φ(x), m) = φ'(x) / √(1 - m·sin²(φ))
                let dphi = diff_expr(phi, var)?;
                let inner = ell_inner(phi, m)?;
                cas_div(dphi, Value::from_cas_call("sqrt", vec![inner]))?
            }
            ("ellie", [phi, m]) => {
                // d/dx E(φ(x), m) = φ'(x) · √(1 - m·sin²(φ))
                let dphi = diff_expr(phi, var)?;
                let inner = ell_inner(phi, m)?;
                cas_mul(vec![dphi, Value::from_cas_call("sqrt", vec![inner])])?
            }
            ("ellpk", [m1]) => {
                // d/dm1 K(m1) = (E(m1) - m1'·K(m1)) / (2·m1·m1')
                // where m1' = 1 - m1, and K(m1) = F(π/2, 1-m1)
                let dm1 = diff_expr(m1, var)?;
                let one = Value::Int(1);
                let two = Value::Int(2);
                let m1_prime = cas_sub(one, m1.clone())?;
                let num = cas_sub(
                    Value::from_cas_call("ellpe", vec![m1.clone()]),
                    cas_mul(vec![
                        m1_prime.clone(),
                        Value::from_cas_call("ellpk", vec![m1.clone()]),
                    ])?,
                )?;
                let denom = cas_mul(vec![two, m1.clone(), m1_prime])?;
                cas_mul(vec![dm1, cas_div(num, denom)?])?
            }
            ("ellpe", [m1]) => {
                // d/dm1 E(m1) = (E(m1) - K(m1)) / (2·m1)
                let dm1 = diff_expr(m1, var)?;
                let two = Value::Int(2);
                let num = cas_sub(
                    Value::from_cas_call("ellpe", vec![m1.clone()]),
                    Value::from_cas_call("ellpk", vec![m1.clone()]),
                )?;
                let denom = cas_mul(vec![two, m1.clone()])?;
                cas_mul(vec![dm1, cas_div(num, denom)?])?
            }
            _ => {
                return Err(cas_err(format!(
                    "unsupported symbolic derivative for function '{name}'"
                )));
            }
        };
        return simplify_cas_value(&out);
    }
    Err(cas_err("expected symbolic expression").got1(expr))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use num_bigint::BigInt;
    use num_traits::One as _;

    use super::*;
    use crate::value::algebraic::AlgebraicData;

    #[test]
    fn differentiate_quadratic() {
        let expr = Value::from_cas_op(
            "+",
            vec![
                Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(2)]),
                Value::from_cas_op("*", vec![Value::Int(2), Value::from_cas_var("x")]),
                Value::Int(1),
            ],
        );
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "2*x + 2");
    }

    #[test]
    fn differentiate_exp_times_polynomial_rewrites_common_factor() {
        let x = Value::from_cas_var("x");
        let poly = Value::from_cas_op(
            "+",
            vec![
                Value::from_cas_op("^", vec![x.clone(), Value::Int(2)]),
                Value::from_cas_op("*", vec![Value::Int(-2), x.clone()]),
                Value::Int(2),
            ],
        );
        let expr = Value::from_cas_op("*", vec![Value::from_cas_call("exp", vec![x]), poly]);

        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^x*x^2");
    }

    #[test]
    fn differentiate_tanh() {
        let expr = Value::from_cas_call("tanh", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "cosh[x]^-2");
    }

    #[test]
    fn differentiate_fractional_power() {
        let expr = Value::from_cas_op(
            "^",
            vec![
                Value::from_cas_var("x"),
                Value::from_fraction_parts(BigInt::one(), BigInt::from(2)),
            ],
        );
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "x^(-1/2)/2");
    }

    #[test]
    fn differentiate_arcsin() {
        let expr = Value::from_cas_call("arcsin", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "(-x^2 + 1)^(-1/2)");
    }

    #[test]
    fn differentiate_arccos() {
        let expr = Value::from_cas_call("arccos", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-(-x^2 + 1)^(-1/2)");
    }

    #[test]
    fn differentiate_arctan() {
        let expr = Value::from_cas_call("arctan", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "(x^2 + 1)^-1");
    }

    #[test]
    fn differentiate_arcsinh() {
        let expr = Value::from_cas_call("arcsinh", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "(x^2 + 1)^(-1/2)");
    }

    #[test]
    fn differentiate_arccosh() {
        let expr = Value::from_cas_call("arccosh", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "(x^2 - 1)^(-1/2)");
    }

    #[test]
    fn differentiate_arctanh() {
        let expr = Value::from_cas_call("arctanh", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "(-x^2 + 1)^-1");
    }

    #[test]
    fn differentiate_abs() {
        let expr = Value::from_cas_call("abs", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "sgn[x]");
    }

    #[test]
    fn differentiate_sgn() {
        let expr = Value::from_cas_call("sgn", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "0");
    }

    #[test]
    fn differentiate_ln_abs() {
        let expr = Value::from_cas_call(
            "ln",
            vec![Value::from_cas_call("abs", vec![Value::from_cas_var("x")])],
        );
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "x^-1");
    }

    #[test]
    fn differentiate_sec() {
        let expr = Value::from_cas_call("sec", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "sec[x]*tan[x]");
    }

    #[test]
    fn differentiate_csc() {
        let expr = Value::from_cas_call("csc", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-cot[x]*csc[x]");
    }

    #[test]
    fn differentiate_cot() {
        let expr = Value::from_cas_call("cot", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-csc[x]^2");
    }

    #[test]
    fn differentiate_sqrt() {
        let expr = Value::from_cas_call("sqrt", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "x^(-1/2)/2");
    }

    #[test]
    fn differentiate_sec_composite() {
        let expr = Value::from_cas_call(
            "sec",
            vec![Value::from_cas_op(
                "*",
                vec![Value::Int(2), Value::from_cas_var("x")],
            )],
        );
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "2*sec[2*x]*tan[2*x]");
    }

    #[test]
    fn differentiate_erf() {
        let expr = Value::from_cas_call("erf", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert!(result.to_string().contains("e^(-x^2)"));
    }

    #[test]
    fn differentiate_erfc() {
        let expr = Value::from_cas_call("erfc", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert!(result.to_string().contains("e^(-x^2)"));
    }

    #[test]
    fn differentiate_heaviside() {
        let expr = Value::from_cas_call("heaviside", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "delta[x]");
    }

    #[test]
    fn differentiate_si() {
        let expr = Value::from_cas_call("si", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "sin[x]/x");
    }

    #[test]
    fn differentiate_ci() {
        let expr = Value::from_cas_call("ci", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "cos[x]/x");
    }

    #[test]
    fn differentiate_ei() {
        let expr = Value::from_cas_call("ei", vec![Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "e^x/x");
    }

    #[test]
    fn differentiate_en() {
        let expr = Value::from_cas_call("en", vec![Value::Int(2), Value::from_cas_var("x")]);
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result.to_string(), "-en[1;x]");
    }

    #[test]
    fn diff_algebraic_in_cas() {
        // ∛2: poly x^3 - 2 = 0, interval (1,2), coeffs [0,1] -> α
        let cube_root_2 = Value::Algebraic(Arc::new(AlgebraicData {
            poly: Arc::new([
                BigInt::from(-2),
                BigInt::from(0),
                BigInt::from(0),
                BigInt::from(1),
            ]),
            interval: (1.0, 2.0),
            coeffs: Arc::new([Value::Int(0), Value::Int(1)]),
        }));

        // Expression: arctan[∛2 * x]
        let expr = Value::from_cas_call(
            "arctan",
            vec![Value::from_cas_op(
                "*",
                vec![cube_root_2.clone(), Value::from_cas_var("x")],
            )],
        );
        let result = diff_cas(&expr, &Value::from_cas_var("x")).unwrap();
        assert!(
            result.to_string().contains("2^(1/3)"),
            "expected 2^(1/3) in arctan derivative, got: {}",
            result
        );

        // Expression: ∛2 * arctan[x]
        let expr2 = Value::from_cas_op(
            "*",
            vec![
                cube_root_2.clone(),
                Value::from_cas_call("arctan", vec![Value::from_cas_var("x")]),
            ],
        );
        let result2 = diff_cas(&expr2, &Value::from_cas_var("x")).unwrap();
        assert_eq!(result2.to_string(), "2^(1/3)/(x^2 + 1)");
    }

    #[test]
    fn test_cas_mul_algebraic_fold() {
        let a = Value::Algebraic(Arc::new(AlgebraicData {
            poly: Arc::new([
                BigInt::from(-1),
                BigInt::from(0),
                BigInt::from(0),
                BigInt::from(108),
            ]),
            interval: (0.0, 1.0),
            coeffs: Arc::new([Value::Int(0), Value::Int(1)]),
        }));
        let a2 = cas_pow(a.clone(), Value::Int(2)).unwrap();

        // Test: (-36*a^2) * (108*a^2)^(-1)
        let inv = cas_pow(
            cas_mul(vec![Value::Int(108), a2.clone()]).unwrap(),
            Value::Int(-1),
        )
        .unwrap();
        let result = cas_mul(vec![Value::Int(-36), a2.clone(), inv]).unwrap();
        assert_eq!(result.to_string(), "-1/3", "expected -1/3, got: {}", result);
    }

    #[test]
    fn diff_algebraic_complex() {
        // a = ∛(1/108)
        let a = Value::Algebraic(Arc::new(AlgebraicData {
            poly: Arc::new([
                BigInt::from(-1),
                BigInt::from(0),
                BigInt::from(0),
                BigInt::from(108),
            ]),
            interval: (0.0, 1.0),
            coeffs: Arc::new([Value::Int(0), Value::Int(1)]),
        }));
        let a2 = cas_pow(a.clone(), Value::Int(2)).unwrap();
        let inner = cas_mul(vec![Value::Int(108), a2.clone()]).unwrap();
        let coeff = cas_pow(
            inner.clone(),
            Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
        )
        .unwrap();

        // Full first term from integrate @s 1/(x^3-2):
        // -18 * a^2 * arctan[...] * (108*a^2)^(-1/2)
        let full_term = cas_mul(vec![
            Value::Int(-18),
            a2.clone(),
            Value::from_cas_call(
                "arctan",
                vec![
                    cas_mul(vec![
                        coeff.clone(),
                        Value::from_cas_op(
                            "+",
                            vec![
                                Value::from_cas_op(
                                    "*",
                                    vec![Value::Int(2), Value::from_cas_var("x")],
                                ),
                                Value::from_cas_op("*", vec![Value::Int(6), a.clone()]),
                            ],
                        ),
                    ])
                    .unwrap(),
                ],
            ),
            coeff.clone(),
        ])
        .unwrap();
        let derivative = diff_cas(&full_term, &Value::from_cas_var("x")).unwrap();
        // After field normalization, the expression uses ∛2 instead of ∛(1/108)
        assert!(
            derivative.to_string().contains("2^(1/3)"),
            "expected simplified expression with 2^(1/3), got: {}",
            derivative
        );

        // Test the actual integrate then diff pipeline
        use crate::cas::integrate::integrate_cas;
        let integrand = Value::from_cas_op(
            "^",
            vec![
                Value::from_cas_op(
                    "+",
                    vec![
                        Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(3)]),
                        Value::Int(-2),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let integral = integrate_cas(&integrand, &Value::from_cas_var("x")).unwrap();
        let derivative = diff_cas(&integral, &Value::from_cas_var("x")).unwrap();
        // After algebraic field normalization + rational combination,
        // diff(integrate(1/(x^3-2))) simplifies back to 1/(x^3-2).
        assert!(
            derivative.to_string().contains("x^3 - 2"),
            "expected 1/(x^3-2) from diff integrate pipeline, got: {}",
            derivative
        );
    }

    #[test]
    fn diff_integrate_one_over_x5_minus_2_roundtrips() {
        use crate::cas::integrate::integrate_cas;

        let integrand = Value::from_cas_op(
            "^",
            vec![
                Value::from_cas_op(
                    "+",
                    vec![
                        Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(5)]),
                        Value::Int(-2),
                    ],
                ),
                Value::Int(-1),
            ],
        );
        let integral = integrate_cas(&integrand, &Value::from_cas_var("x")).unwrap();
        let derivative = diff_cas(&integral, &Value::from_cas_var("x")).unwrap();

        assert_eq!(derivative.to_string(), "(x^5 - 2)^-1");
    }

    #[test]
    fn diff_integrate_sqrt_x3_plus_1_roundtrips() {
        use crate::cas::integrate::integrate_cas;

        let x = Value::from_cas_var("x");
        let integrand = Value::from_cas_op(
            "^",
            vec![
                Value::from_cas_op(
                    "+",
                    vec![
                        Value::from_cas_op("^", vec![x.clone(), Value::Int(3)]),
                        Value::Int(1),
                    ],
                ),
                Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
            ],
        );
        let integral = integrate_cas(&integrand, &x).unwrap();
        let derivative = diff_cas(&integral, &x).unwrap();

        assert_eq!(derivative.to_string(), "(x^3 + 1)^(1/2)");
    }
}
