use std::sync::Arc;

use num_bigint::BigInt;

use super::*;
use crate::value::Value;
use crate::value::algebraic::AlgebraicData;

fn contains_op(value: &Value, needle: &str) -> bool {
    if let Some((op, args)) = value.cas_op_parts() {
        if op == needle {
            return true;
        }
        return args.iter().any(|arg| contains_op(arg, needle));
    }
    if let Some((_name, args)) = value.cas_call_parts() {
        return args.iter().any(|arg| contains_op(arg, needle));
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_op(lhs, needle) || contains_op(rhs, needle);
    }
    false
}

#[test]
fn cas_var_formats_like_identifier() {
    assert_eq!(Value::from_cas_var("x").to_string(), "x");
}

#[test]
fn canonical_addition_orders_consistently() {
    let lhs = simplify_cas_value(&Value::from_cas_op(
        "+",
        vec![Value::from_cas_var("x"), Value::Int(1)],
    ))
    .unwrap();
    let rhs = simplify_cas_value(&Value::from_cas_op(
        "+",
        vec![Value::Int(1), Value::from_cas_var("x")],
    ))
    .unwrap();
    assert_eq!(lhs, rhs);
    assert_eq!(lhs.to_string(), "x + 1");
}

#[test]
fn canonical_form_eliminates_subtraction_and_division() {
    let expr = simplify_cas_value(&Value::from_cas_op(
        "/",
        vec![
            Value::from_cas_op("-", vec![Value::from_cas_var("x"), Value::Int(1)]),
            Value::from_cas_var("y"),
        ],
    ))
    .unwrap();
    assert!(!contains_op(&expr, "-"));
    assert!(!contains_op(&expr, "/"));
}

#[test]
fn simplify_combines_like_terms() {
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_var("x"),
            Value::from_cas_op("*", vec![Value::Int(2), Value::from_cas_var("x")]),
            Value::Int(1),
        ],
    );
    assert_eq!(simplify_cas_value(&expr).unwrap().to_string(), "3*x + 1");
}

#[test]
fn simplify_keeps_root_of_square_until_rewritten() {
    let expr = Value::from_cas_call(
        "sqrt",
        vec![Value::from_cas_op(
            "^",
            vec![Value::from_cas_var("x"), Value::Int(2)],
        )],
    );
    assert_eq!(
        simplify_cas_value(&expr).unwrap().to_string(),
        "(x^2)^(1/2)"
    );
    assert_eq!(rewrite_cas(&expr).unwrap().to_string(), "abs[x]");
}

#[test]
fn rewrite_combines_log_terms() {
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_call("ln", vec![Value::from_cas_var("x")]),
            Value::from_cas_call("ln", vec![Value::from_cas_var("y")]),
        ],
    );
    assert_eq!(rewrite_cas(&expr).unwrap().to_string(), "ln[x*y]");
}

#[test]
fn rewrite_factors_common_product_with_egg() {
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_op(
                "*",
                vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
            ),
            Value::from_cas_op(
                "*",
                vec![Value::from_cas_var("x"), Value::from_cas_var("z")],
            ),
        ],
    );
    let text = rewrite_cas(&expr).unwrap().to_string();
    assert!(
        text == "x*(y + z)" || text == "x*(z + y)",
        "unexpected factored form: {text}"
    );
}

#[test]
fn rewrite_keeps_fractional_log_sum_expanded() {
    let x = Value::from_cas_var("x");
    let x2 = Value::from_cas_op("^", vec![x.clone(), Value::Int(2)]);
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_op(
                "*",
                vec![
                    Value::from_cas_call("ln", vec![x]),
                    x2.clone(),
                    Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
                ],
            ),
            Value::from_cas_op(
                "*",
                vec![
                    Value::Int(-1),
                    x2,
                    Value::from_fraction_parts(BigInt::from(1), BigInt::from(4)),
                ],
            ),
        ],
    );

    assert_eq!(
        rewrite_cas(&expr).unwrap().to_string(),
        "ln[x]*x^2/2 - x^2/4"
    );
}

#[test]
fn rewrite_handles_trig_rules() {
    let odd = rewrite_cas(&Value::from_cas_call(
        "sin",
        vec![Value::from_cas_op("-", vec![Value::from_cas_var("x")])],
    ))
    .unwrap();
    assert_eq!(odd.to_string(), "-sin[x]");

    let double_angle = rewrite_cas(&Value::from_cas_call(
        "sin",
        vec![Value::from_cas_op(
            "*",
            vec![Value::Int(2), Value::from_cas_var("x")],
        )],
    ))
    .unwrap();
    assert_eq!(double_angle.to_string(), "2*cos[x]*sin[x]");
}

#[test]
fn rewrite_removes_abs_square() {
    let expr = Value::from_cas_op(
        "^",
        vec![
            Value::from_cas_call("abs", vec![Value::from_cas_var("x")]),
            Value::Int(2),
        ],
    );
    assert_eq!(rewrite_cas(&expr).unwrap().to_string(), "x^2");
}

#[test]
fn simplify_evaluates_extended_numeric_calls() {
    assert_eq!(
        simplify_cas_value(&Value::from_cas_call("log2", vec![Value::Int(8)])).unwrap(),
        Value::float(3.0)
    );
    assert_eq!(
        simplify_cas_value(&Value::from_cas_call("floor", vec![Value::float(2.9)])).unwrap(),
        Value::Int(2)
    );
}

#[test]
fn simplify_combines_inverse_square_roots() {
    let a = cas_add(vec![Value::from_cas_var("x"), Value::Int(1)]).unwrap();
    let b = cas_add(vec![Value::from_cas_var("x"), Value::Int(-1)]).unwrap();
    let expr = cas_mul(vec![
        cas_pow(
            a,
            Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
        )
        .unwrap(),
        cas_pow(
            b,
            Value::from_fraction_parts(BigInt::from(-1), BigInt::from(2)),
        )
        .unwrap(),
    ])
    .unwrap();

    let text = expr.to_string();
    assert!(
        text.contains("(-1/2)") && text.contains("x + 1") && text.contains("x - 1"),
        "expected merged inverse square root, got: {text}"
    );
}

#[test]
fn substitute_evaluates_numeric_value() {
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(2)]),
            Value::Int(1),
        ],
    );
    let result = substitute_cas(&expr, &Value::from_cas_var("x"), &Value::Int(5)).unwrap();
    assert_eq!(result, Value::Int(26));
}

#[test]
fn expand_binomial_square() {
    let expr = Value::from_cas_op(
        "^",
        vec![
            Value::from_cas_op("+", vec![Value::from_cas_var("x"), Value::Int(1)]),
            Value::Int(2),
        ],
    );
    let result = expand_cas(&expr).unwrap();
    assert_eq!(result.to_string(), "x^2 + 2*x + 1");
}

#[test]
fn expand_deep_nested_addition() {
    // Build ((((x + 1) + 1) + 1) + ...) with depth 2000.
    // Both expand_expr and simplify_cas_value are now iterative and must survive.
    let mut expr = Value::from_cas_var("x");
    for _ in 0..2000 {
        expr = Value::from_cas_op("+", vec![expr, Value::Int(1)]);
    }
    let result = expand_expr(&expr).unwrap();
    assert!(result.to_string().contains("x"));
}

#[test]
fn expand_high_power_no_stack_overflow() {
    // (x + 1)^20 — the original recursive power loop recursed 20 times
    // on growing intermediate expressions.
    let base = Value::from_cas_op("+", vec![Value::from_cas_var("x"), Value::Int(1)]);
    let expr = Value::from_cas_op("^", vec![base, Value::Int(20)]);
    let result = expand_cas(&expr).unwrap();
    let s = result.to_string();
    assert!(s.contains("x^20"), "expected x^20 in expansion: {s}");
}

#[test]
fn simplify_deep_nested_addition() {
    // Build ((((x + 1) + 1) + 1) + ...) with depth 2000.
    // The iterative simplify_cas_value must survive without stack overflow.
    let mut expr = Value::from_cas_var("x");
    for _ in 0..2000 {
        expr = Value::from_cas_op("+", vec![expr, Value::Int(1)]);
    }
    let result = simplify_cas_value(&expr).unwrap();
    assert!(result.to_string().contains("x"));
}

#[test]
fn simplify_deep_nested_multiplication() {
    // Build ((((x * 2) * 2) * 2) * ...) with depth 2000.
    let mut expr = Value::from_cas_var("x");
    for _ in 0..2000 {
        expr = Value::from_cas_op("*", vec![expr, Value::Int(2)]);
    }
    let result = simplify_cas_value(&expr).unwrap();
    assert!(result.to_string().contains("x"));
}

#[test]
fn factor_extracts_common_term() {
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(2)]),
            Value::from_cas_var("x"),
        ],
    );
    let result = factor_cas(&expr).unwrap();
    assert_eq!(result.to_string(), "x*(x + 1)");
}

#[test]
fn simplify_performs_exact_polynomial_division() {
    let expr = Value::from_cas_op(
        "/",
        vec![
            Value::from_cas_op(
                "-",
                vec![
                    Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(2)]),
                    Value::Int(1),
                ],
            ),
            Value::from_cas_op("-", vec![Value::from_cas_var("x"), Value::Int(1)]),
        ],
    );
    assert_eq!(simplify_cas_value(&expr).unwrap().to_string(), "x + 1");
}

#[test]
fn solve_quadratic_equation() {
    let expr = Value::from_cas_op(
        "-",
        vec![
            Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(2)]),
            Value::Int(1),
        ],
    );
    let result = solve_cas(
        &Value::from_cas_eq(expr, Value::Int(0)),
        &Value::from_cas_var("x"),
    )
    .unwrap();
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    assert_eq!(roots.len(), 2);
    assert!(roots.contains(&Value::float(1.0)));
    assert!(roots.contains(&Value::float(-1.0)));
}

#[test]
fn solve_monomial_cubic_equation() {
    let expr = Value::from_cas_op(
        "-",
        vec![
            Value::from_cas_op("^", vec![Value::from_cas_var("x"), Value::Int(3)]),
            Value::Int(8),
        ],
    );
    let result = solve_cas(
        &Value::from_cas_eq(expr, Value::Int(0)),
        &Value::from_cas_var("x"),
    )
    .unwrap();
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    assert_eq!(roots.len(), 3);
    assert!(roots.iter().any(|root| {
        root.as_f64()
            .is_some_and(|value| (value - 2.0).abs() < 1e-9)
    }));
}

#[test]
fn solve_linear_system_returns_values_in_variable_order() {
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            Value::from_cas_op(
                "+",
                vec![
                    Value::from_cas_op("*", vec![Value::Int(2), Value::from_cas_var("x")]),
                    Value::from_cas_var("y"),
                ],
            ),
            Value::Int(5),
        ),
        Value::from_cas_eq(
            Value::from_cas_op(
                "-",
                vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
            ),
            Value::Int(1),
        ),
    ]));
    let vars = Value::List(Arc::new(vec![
        Value::from_cas_var("x"),
        Value::from_cas_var("y"),
    ]));
    assert_eq!(
        solve_system_cas(&equations, &vars).unwrap(),
        Value::IntList(Arc::new(vec![2, 1]))
    );
}

#[test]
fn linear_coeff_var() {
    assert_eq!(
        extract_linear_coefficients(&Value::from_cas_var("x"), "x"),
        Some((Value::Int(1), Value::Int(0)))
    );
}

#[test]
fn linear_coeff_product() {
    let expr = Value::from_cas_op("*", vec![Value::Int(2), Value::from_cas_var("x")]);
    assert_eq!(
        extract_linear_coefficients(&simplify_cas_value(&expr).unwrap(), "x"),
        Some((Value::Int(2), Value::Int(0)))
    );
}

#[test]
fn linear_coeff_sum() {
    let expr = Value::from_cas_op(
        "+",
        vec![
            Value::from_cas_op("*", vec![Value::Int(2), Value::from_cas_var("x")]),
            Value::Int(3),
        ],
    );
    assert_eq!(
        extract_linear_coefficients(&simplify_cas_value(&expr).unwrap(), "x"),
        Some((Value::Int(2), Value::Int(3)))
    );
}

#[test]
fn linear_coeff_product_of_sum() {
    let expr = simplify_cas_value(&Value::from_cas_op(
        "/",
        vec![
            Value::from_cas_op("+", vec![Value::from_cas_var("x"), Value::Int(1)]),
            Value::Int(2),
        ],
    ))
    .unwrap();
    let (a, b) = extract_linear_coefficients(&expr, "x").unwrap();
    assert_eq!(a, Value::from_fraction_parts(1u64.into(), 2u64.into()));
    assert_eq!(b, Value::from_fraction_parts(1u64.into(), 2u64.into()));
}

#[test]
fn linear_coeff_negative_product_of_sum() {
    let expr = simplify_cas_value(&Value::from_cas_op(
        "*",
        vec![
            Value::Int(-1),
            Value::from_cas_op("+", vec![Value::from_cas_var("x"), Value::Int(1)]),
        ],
    ))
    .unwrap();
    assert_eq!(
        extract_linear_coefficients(&expr, "x"),
        Some((Value::Int(-1), Value::Int(-1)))
    );
}

#[test]
fn numeric_erf_zero() {
    let result = simplify_cas_value(&Value::from_cas_call("erf", vec![Value::Int(0)])).unwrap();
    assert_eq!(result, Value::float(0.0));
}

#[test]
fn numeric_erfc_zero() {
    let result = simplify_cas_value(&Value::from_cas_call("erfc", vec![Value::Int(0)])).unwrap();
    assert_eq!(result, Value::float(1.0));
}

#[test]
fn numeric_gamma_five() {
    // gamma(5) = 4! = 24
    let result = simplify_cas_value(&Value::from_cas_call("gamma", vec![Value::Int(5)])).unwrap();
    if let Value::Float(f) = result {
        assert!((*f - 24.0).abs() < 1e-10);
    } else {
        panic!("expected Float");
    }
}

#[test]
fn numeric_heaviside_pos() {
    let result =
        simplify_cas_value(&Value::from_cas_call("heaviside", vec![Value::float(3.0)])).unwrap();
    assert_eq!(result, Value::float(1.0));
}

#[test]
fn numeric_heaviside_neg() {
    let result =
        simplify_cas_value(&Value::from_cas_call("heaviside", vec![Value::float(-3.0)])).unwrap();
    assert_eq!(result, Value::float(0.0));
}

#[test]
fn numeric_rejects_non_numeric_algebraic_coefficients() {
    let malformed = Value::Algebraic(Arc::new(AlgebraicData {
        poly: Arc::new([BigInt::from(-2), BigInt::from(0), BigInt::from(1)]),
        interval: (1.0, 2.0),
        coeffs: Arc::new([Value::from_cas_var("x")]),
    }));

    let err = eval_numeric_cas(&malformed).expect_err("symbolic coefficient should fail");
    assert!(
        err.msg
            .as_deref()
            .is_some_and(|msg| msg.contains("contains variable")),
        "unexpected error: {err:?}",
    );
}

#[test]
fn normalize_merges_multiple_reciprocals() {
    // simplifiy_cas_value should combine terms with multiple (^ D -1) factors.
    let d1 = cas_add(vec![Value::from_cas_var("x"), Value::Int(1)]).unwrap();
    let d2 = cas_add(vec![Value::from_cas_var("x"), Value::Int(-1)]).unwrap();
    let inv1 = Value::from_cas_op("^", vec![d1, Value::Int(-1)]);
    let inv2 = Value::from_cas_op("^", vec![d2, Value::Int(-1)]);
    // Two terms with the same multi-factor denominator structure
    let term_a = cas_mul(vec![Value::Int(5), inv1.clone(), inv2.clone()]).unwrap();
    let term_b = cas_mul(vec![Value::Int(3), inv1, inv2]).unwrap();
    let sum = cas_add(vec![term_a, term_b]).unwrap();
    let simplified = simplify_cas_value(&sum).unwrap();
    let s = simplified.to_string();
    assert!(
        s.contains("x + 1") && s.contains("x - 1"),
        "expected combined denominator in '{}'",
        s
    );
}

#[test]
fn rewrite_distributes_negation_over_sum() {
    // (* -1 (+ x y)) → (+ (* -1 x) (* -1 y))
    let sum = cas_add(vec![Value::from_cas_var("x"), Value::from_cas_var("y")]).unwrap();
    let product = cas_mul(vec![Value::Int(-1), sum]).unwrap();
    let rewritten = rewrite_expr(&product).unwrap();
    assert!(
        rewritten.cas_op_parts().is_some_and(|(op, _)| op == "+"),
        "expected sum, got: {}",
        rewritten
    );
}

#[test]
fn rewrite_sgn_abs_product_cancels() {
    // sgn(u) * abs(u)^(-1) → u^(-1)
    let u = Value::from_cas_var("x");
    let sgn = Value::from_cas_call("sgn", vec![u.clone()]);
    let abs_inv = Value::from_cas_op(
        "^",
        vec![Value::from_cas_call("abs", vec![u.clone()]), Value::Int(-1)],
    );
    let product = cas_mul(vec![sgn, abs_inv]).unwrap();
    let rewritten = rewrite_expr(&product).unwrap();
    assert!(
        rewritten.to_string().contains("x^-1"),
        "expected x^-1, got: {}",
        rewritten
    );
}

#[test]
fn divide_cancels_affine_over_factor() {
    let x = Value::from_cas_var("x");
    let sqrt_part = cas_pow(
        cas_add(vec![
            cas_pow(x.clone(), Value::Int(2)).unwrap(),
            Value::Int(1),
        ])
        .unwrap(),
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )
    .unwrap();

    let lhs = cas_add(vec![
        cas_mul(vec![
            x.clone(),
            cas_pow(sqrt_part.clone(), Value::Int(-1)).unwrap(),
        ])
        .unwrap(),
        Value::Int(-1),
    ])
    .unwrap();

    let rhs = factor_expr(
        &cas_add(vec![
            cas_pow(x.clone(), Value::Int(2)).unwrap(),
            cas_mul(vec![Value::Int(-1), x.clone(), sqrt_part.clone()]).unwrap(),
        ])
        .unwrap(),
    )
    .unwrap();

    let simplified = cas_div(lhs, rhs).unwrap();
    assert_eq!(simplified.to_string(), "(x^2 + 1)^(-1/2)/x");
}

#[test]
fn rewrite_combines_var_free_denominator_sum() {
    let denom = cas_add(vec![
        cas_pow(
            Value::Int(3),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        )
        .unwrap(),
        Value::Int(1),
    ])
    .unwrap();
    let expr = cas_add(vec![
        cas_div(Value::from_cas_var("x"), denom.clone()).unwrap(),
        Value::Int(2),
    ])
    .unwrap();

    let rewritten = rewrite_cas(&expr).unwrap();
    let text = rewritten.to_string();
    assert!(
        text.contains("/(3^(1/2) + 1)") && text.contains("2*(3^(1/2) + 1)"),
        "expected combined constant denominator, got: {text}"
    );
}

#[test]
fn rewrite_cancels_affine_over_product_form() {
    let x = Value::from_cas_var("x");
    let sqrt_part = cas_pow(
        cas_add(vec![
            cas_pow(x.clone(), Value::Int(2)).unwrap(),
            Value::Int(1),
        ])
        .unwrap(),
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )
    .unwrap();
    let numerator = cas_add(vec![
        cas_mul(vec![
            x.clone(),
            cas_pow(sqrt_part.clone(), Value::Int(-1)).unwrap(),
        ])
        .unwrap(),
        Value::Int(-1),
    ])
    .unwrap();
    let denominator = cas_add(vec![
        cas_pow(x.clone(), Value::Int(2)).unwrap(),
        cas_mul(vec![Value::Int(-1), x.clone(), sqrt_part]).unwrap(),
    ])
    .unwrap();
    let expr = cas_mul(vec![
        numerator,
        cas_pow(denominator, Value::Int(-1)).unwrap(),
    ])
    .unwrap();

    let rewritten = rewrite_cas(&expr).unwrap();
    assert_eq!(rewritten.to_string(), "(x^2 + 1)^(-1/2)/x");
}

#[test]
fn rewrite_combines_unit_with_fraction_sum() {
    let x = Value::from_cas_var("x");
    let denom = cas_add(vec![x.clone(), Value::Int(1)]).unwrap();
    let frac = cas_div(x, denom).unwrap();
    let expr = cas_add(vec![Value::Int(1), cas_neg(frac).unwrap()]).unwrap();

    let rewritten = rewrite_cas(&expr).unwrap();
    assert_eq!(rewritten.to_string(), "(x + 1)^-1");
}

#[test]
fn rewrite_merges_var_free_pair_in_larger_sum() {
    let x_sq = cas_pow(Value::from_cas_var("x"), Value::Int(2)).unwrap();
    let a = cas_pow(
        Value::Int(2),
        Value::from_fraction_parts(BigInt::from(2), BigInt::from(5)),
    )
    .unwrap();
    let b = cas_pow(
        Value::Int(5),
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )
    .unwrap();
    let term1 = cas_mul(vec![
        a.clone(),
        cas_add(vec![
            Value::Int(6),
            cas_mul(vec![Value::Int(-2), b.clone()]).unwrap(),
        ])
        .unwrap(),
    ])
    .unwrap();
    let term2 = cas_mul(vec![
        a,
        cas_add(vec![
            Value::Int(10),
            cas_mul(vec![Value::Int(2), b]).unwrap(),
        ])
        .unwrap(),
    ])
    .unwrap();
    let expr = cas_add(vec![x_sq, term1, term2]).unwrap();

    let rewritten = rewrite_cas(&expr).unwrap();
    let text = rewritten.to_string();
    assert!(
        text.contains("16") && !text.contains("5^(1/2)"),
        "expected sqrt(5) cancellation in constants, got: {text}"
    );
}

#[test]
fn diff_integrate_roundtrip_inverse_x_sqrt_is_clean() {
    let x = Value::from_cas_var("x");
    let sqrt_part = cas_pow(
        cas_add(vec![
            cas_pow(x.clone(), Value::Int(2)).unwrap(),
            Value::Int(1),
        ])
        .unwrap(),
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )
    .unwrap();
    let integrand = cas_div(Value::Int(1), cas_mul(vec![x.clone(), sqrt_part]).unwrap()).unwrap();

    let antiderivative = crate::cas::integrate::integrate_cas(&integrand, &x).unwrap();
    let roundtrip = crate::cas::diff::diff_cas(&antiderivative, &x).unwrap();
    assert_eq!(roundtrip.to_string(), "(x^2 + 1)^(-1/2)/x");
}
