use std::sync::Arc;

use num_bigint::BigInt;

use super::assumption::CasAssumptions;
use super::*;
use crate::value::Value;
use crate::value::algebraic::{AlgebraicData, AlgebraicField};
use crate::value::cas::{CasConst, CasFunction, CasOp, CasPredicate};

fn op(op: CasOp, args: Vec<Value>) -> Value {
    Value::from_cas_op(op, args)
}

fn call(function: CasFunction, args: Vec<Value>) -> Value {
    Value::from_cas_function(function, args)
}

fn contains_op(value: &Value, needle: CasOp) -> bool {
    if let Some((op, args)) = value.cas_op_parts() {
        if op == needle {
            return true;
        }
        return args.iter().any(|arg| contains_op(arg, needle));
    }
    if let Some((_name, args)) = value.cas_function_parts() {
        return args.iter().any(|arg| contains_op(arg, needle));
    }
    if let Some((_name, args)) = value.cas_apply_parts() {
        return args.iter().any(|arg| contains_op(arg, needle));
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return contains_op(lhs, needle) || contains_op(rhs, needle);
    }
    false
}

fn assert_dict_entries(value: &Value, expected: &[(&str, Value)]) {
    let Value::Dict(map) = value else {
        panic!("expected dict, got {value:?}");
    };
    assert_eq!(map.len(), expected.len(), "dict: {map:?}");
    for ((actual_key, actual_value), (expected_key, expected_value)) in map.iter().zip(expected) {
        assert_eq!(actual_key.as_ref(), *expected_key);
        assert_eq!(actual_value, expected_value);
    }
}

fn count_inverse_powers(value: &Value) -> usize {
    if let Some((op, args)) = value.cas_op_parts() {
        let here =
            usize::from(op == CasOp::Power && matches!(args, [_, exp] if exp.exact_int_is(-1)));
        return here + args.iter().map(count_inverse_powers).sum::<usize>();
    }
    if let Some((_name, args)) = value.cas_function_parts() {
        return args.iter().map(count_inverse_powers).sum();
    }
    if let Some((_name, args)) = value.cas_apply_parts() {
        return args.iter().map(count_inverse_powers).sum();
    }
    if let Some((lhs, rhs)) = value.cas_eq_parts() {
        return count_inverse_powers(lhs) + count_inverse_powers(rhs);
    }
    0
}

#[test]
fn cas_var_formats_like_identifier() {
    assert_eq!(Value::from_cas_var("x").to_string(), "@s x");
}

#[test]
fn expression_argument_errors_name_and_quote_the_cas_construct() {
    let x = Value::from_cas_var("x");
    let equation = Value::from_cas_eq(x.clone(), Value::Int(1));
    let err = cas_binary_expr(CasOp::Add, &equation, &Value::Int(2))
        .expect_err("an equation is not an operator operand");
    assert_eq!(
        err.msg.as_deref(),
        Some("operator '+' expects an expression rather than an equation")
    );

    let condition = Value::from_cas_predicate(CasPredicate::Positive(x));
    let err = cas_call_expr(CasFunction::Sin, &[condition])
        .expect_err("a condition is not a function argument");
    assert_eq!(
        err.msg.as_deref(),
        Some("function 'sin' expects an expression rather than a condition")
    );

    let err = cas_symbolic_call_expr("f", &[equation], &[])
        .expect_err("an equation is not an application argument");
    assert_eq!(
        err.msg.as_deref(),
        Some("application 'f' expects an expression rather than an equation")
    );
}

#[test]
fn canonical_addition_orders_consistently() {
    let lhs = simplify_cas_value(&op(
        CasOp::Add,
        vec![Value::from_cas_var("x"), Value::Int(1)],
    ))
    .unwrap();
    let rhs = simplify_cas_value(&op(
        CasOp::Add,
        vec![Value::Int(1), Value::from_cas_var("x")],
    ))
    .unwrap();
    assert_eq!(lhs, rhs);
    assert_eq!(lhs.to_string(), "@s x + 1");
}

#[test]
fn canonical_form_eliminates_subtraction_and_division() {
    let expr = simplify_cas_value(&op(
        CasOp::Divide,
        vec![
            op(
                CasOp::Subtract,
                vec![Value::from_cas_var("x"), Value::Int(1)],
            ),
            Value::from_cas_var("y"),
        ],
    ))
    .unwrap();
    assert!(!contains_op(&expr, CasOp::Subtract));
    assert!(!contains_op(&expr, CasOp::Divide));
}

#[test]
fn cas_neg_flips_infinity_constants() {
    assert_eq!(
        cas_neg(Value::from_cas_const(CasConst::Infinity)).unwrap(),
        Value::from_cas_const(CasConst::NegInfinity)
    );
    assert_eq!(
        cas_neg(Value::from_cas_const(CasConst::NegInfinity)).unwrap(),
        Value::from_cas_const(CasConst::Infinity)
    );
}

#[test]
fn typed_op_constructors_canonicalize_like_raw_ops() {
    let x = Value::from_cas_var("x");
    let add = simplify_cas_value(&op(CasOp::Add, vec![x.clone(), Value::Int(1)])).unwrap();
    assert_eq!(add.to_string(), "@s x + 1");

    let mul = simplify_cas_value(&op(CasOp::Multiply, vec![Value::Int(2), x.clone()])).unwrap();
    assert_eq!(mul.to_string(), "@s 2*x");

    let pow = simplify_cas_value(&op(CasOp::Power, vec![x.clone(), Value::Int(2)])).unwrap();
    assert_eq!(pow.to_string(), "@s x^2");

    let neg = simplify_cas_value(&op(CasOp::Subtract, vec![x.clone()])).unwrap();
    assert_eq!(neg.to_string(), "@s -x");

    let sub = simplify_cas_value(&op(CasOp::Subtract, vec![x.clone(), Value::Int(1)])).unwrap();
    assert_eq!(sub.to_string(), "@s x - 1");

    let div = simplify_cas_value(&op(CasOp::Divide, vec![x, Value::Int(2)])).unwrap();
    assert_eq!(div.to_string(), "@s x/2");
}

#[test]
fn simplify_combines_like_terms() {
    let expr = op(
        CasOp::Add,
        vec![
            Value::from_cas_var("x"),
            op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            ),
            Value::Int(1),
        ],
    );
    assert_eq!(simplify_cas_value(&expr).unwrap().to_string(), "@s 3*x + 1");
}

#[test]
fn simplify_keeps_root_of_square_until_rewritten() {
    let expr = call(
        CasFunction::Sqrt,
        vec![op(
            CasOp::Power,
            vec![Value::from_cas_var("x"), Value::Int(2)],
        )],
    );
    assert_eq!(
        simplify_cas_value(&expr).unwrap().to_string(),
        "@s (x^2)^(1/2)"
    );
    assert_eq!(rewrite_cas(&expr).unwrap().to_string(), "@s abs[x]");
}

#[test]
fn simplify_large_squarefree_sqrt_stays_symbolic() {
    let expr = op(
        CasOp::Power,
        vec![
            Value::from_bigint(BigInt::from(9_999_999_967_i64)),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
        ],
    );
    assert_eq!(
        simplify_cas_value(&expr).unwrap().to_string(),
        "@s 9999999967^(1/2)"
    );
}

#[test]
fn simplify_exact_odd_root_of_negative_rational() {
    let expr = op(
        CasOp::Power,
        vec![
            Value::from_fraction_parts(BigInt::from(-8), BigInt::from(27)),
            Value::from_fraction_parts(BigInt::from(1), BigInt::from(3)),
        ],
    );

    assert_eq!(
        simplify_cas_value(&expr).unwrap(),
        Value::from_fraction_parts(BigInt::from(-2), BigInt::from(3))
    );
}

#[test]
fn rewrite_combines_log_terms() {
    let expr = op(
        CasOp::Add,
        vec![
            call(CasFunction::Ln, vec![Value::from_cas_var("x")]),
            call(CasFunction::Ln, vec![Value::from_cas_var("y")]),
        ],
    );
    assert_eq!(rewrite_cas(&expr).unwrap().to_string(), "@s ln[x*y]");
}

#[test]
fn rewrite_factors_common_product() {
    let expr = op(
        CasOp::Add,
        vec![
            op(
                CasOp::Multiply,
                vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
            ),
            op(
                CasOp::Multiply,
                vec![Value::from_cas_var("x"), Value::from_cas_var("z")],
            ),
        ],
    );
    let text = rewrite_cas(&expr).unwrap().to_string();
    assert!(
        text == "@s x*(y + z)" || text == "@s x*(z + y)",
        "unexpected factored form: {text}"
    );
}

#[test]
fn rewrite_keeps_fractional_log_sum_expanded() {
    let x = Value::from_cas_var("x");
    let x2 = op(CasOp::Power, vec![x.clone(), Value::Int(2)]);
    let expr = op(
        CasOp::Add,
        vec![
            op(
                CasOp::Multiply,
                vec![
                    call(CasFunction::Ln, vec![x]),
                    x2.clone(),
                    Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
                ],
            ),
            op(
                CasOp::Multiply,
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
        "@s ln[x]*x^2/2 - x^2/4"
    );
}

#[test]
fn rewrite_handles_trig_rules() {
    let odd = rewrite_cas(&call(
        CasFunction::Sin,
        vec![op(CasOp::Subtract, vec![Value::from_cas_var("x")])],
    ))
    .unwrap();
    assert_eq!(odd.to_string(), "@s -sin[x]");

    let double_angle = rewrite_cas(&call(
        CasFunction::Sin,
        vec![op(
            CasOp::Multiply,
            vec![Value::Int(2), Value::from_cas_var("x")],
        )],
    ))
    .unwrap();
    assert_eq!(double_angle.to_string(), "@s 2*cos[x]*sin[x]");
}

#[test]
fn rewrite_removes_abs_square() {
    let expr = op(
        CasOp::Power,
        vec![
            call(CasFunction::Abs, vec![Value::from_cas_var("x")]),
            Value::Int(2),
        ],
    );
    assert_eq!(rewrite_cas(&expr).unwrap().to_string(), "@s x^2");
}

#[test]
fn simplify_evaluates_extended_numeric_calls() {
    assert_eq!(
        simplify_cas_value(&call(CasFunction::Log2, vec![Value::Int(8)])).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        simplify_cas_value(&call(CasFunction::Floor, vec![Value::float(2.9)])).unwrap(),
        Value::Int(2)
    );
}

#[test]
fn simplify_trig_special_constants_exactly() {
    assert_eq!(
        simplify_cas_value(&call(
            CasFunction::Sin,
            vec![Value::from_cas_const(CasConst::Pi)]
        ))
        .unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        simplify_cas_value(&call(
            CasFunction::Tan,
            vec![Value::from_cas_const(CasConst::Pi)]
        ))
        .unwrap(),
        Value::Int(0)
    );

    let half_pi = cas_div(Value::from_cas_const(CasConst::Pi), Value::Int(2)).unwrap();
    assert_eq!(
        simplify_cas_value(&call(CasFunction::Cos, vec![half_pi.clone()])).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        simplify_cas_value(&call(CasFunction::Sin, vec![half_pi])).unwrap(),
        Value::Int(1)
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
    let expr = op(
        CasOp::Add,
        vec![
            op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
            Value::Int(1),
        ],
    );
    let result = substitute_cas(&expr, &Value::from_cas_var("x"), &Value::Int(5)).unwrap();
    assert_eq!(result, Value::Int(26));
}

#[test]
fn substitute_recurses_into_symbolic_application_args() {
    let expr = Value::from_cas_apply("f", vec![Value::from_cas_var("x")]);
    let result = substitute_cas(&expr, &Value::from_cas_var("x"), &Value::Int(2)).unwrap();
    assert_eq!(result.to_string(), "@s f[2]");
}

#[test]
fn substitute_recurses_into_limit_point_but_not_bound_body() {
    let y = Value::from_cas_var("y");
    let x = Value::from_cas_var("x");
    let inner = cas_div(
        Value::from_cas_function(CasFunction::Sin, vec![y.clone()]),
        y.clone(),
    )
    .unwrap();
    let limit = Value::from_cas_limit(close_cas_scope(&inner, "y"), x.clone(), None);

    let result = substitute_cas(&limit, &x, &Value::Int(0)).unwrap();
    assert_eq!(result.to_string(), "@s limit[sin[y]/y;y;0]");
}

#[test]
fn substitute_avoids_limit_capture() {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let limit = Value::from_cas_limit(close_cas_scope(&y, "x"), Value::Int(0), None);

    let result = substitute_cas(&limit, &y, &x).expect("capture-free substitution");
    let (scope, _, _) = result.cas_limit_parts().expect("limit");
    assert_eq!(scope.body().cas_var_name(), Some("x"));
    assert_eq!(result.to_string(), "@s limit[x;x1;0]");
}

#[test]
fn substitute_avoids_integral_capture() {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let integral = Value::from_cas_integral(close_cas_scope(&y, "x"), None);

    let result = substitute_cas(&integral, &y, &x).expect("capture-free substitution");
    let (scope, bounds) = result.cas_integral_parts().expect("integral");
    assert_eq!(scope.body().cas_var_name(), Some("x"));
    assert_eq!(bounds, None);
    assert_eq!(result.to_string(), "@s integrate[x;x1]");
}

#[test]
fn single_var_inference_traverses_calculus_forms_without_counting_binders() {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let integral = Value::from_cas_integral(close_cas_scope(&y, "x"), None);
    let limit = Value::from_cas_limit(close_cas_scope(&integral, "y"), x, None);

    assert_eq!(
        infer_single_cas_var(&integral).expect("integral should have one free variable"),
        "y"
    );
    assert_eq!(
        infer_single_cas_var(&limit).expect("limit point should be the only free variable"),
        "x"
    );
}

#[test]
fn simplify_recurses_into_symbolic_application_args() {
    let expr = Value::from_cas_apply(
        "f",
        vec![op(
            CasOp::Add,
            vec![Value::from_cas_var("x"), Value::Int(0)],
        )],
    );
    let result = simplify_cas_value(&expr).unwrap();
    assert_eq!(result.to_string(), "@s f[x]");
}

#[test]
fn expand_binomial_square() {
    let expr = op(
        CasOp::Power,
        vec![
            op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
            Value::Int(2),
        ],
    );
    let result = expand_cas(&expr).unwrap();
    assert_eq!(result.to_string(), "@s x^2 + 2*x + 1");
}

#[test]
fn expand_deep_nested_addition() {
    // Build ((((x + 1) + 1) + 1) + ...) with depth 2000.
    // Both expand_expr and simplify_cas_value are now iterative and must survive.
    let mut expr = Value::from_cas_var("x");
    for _ in 0..2000 {
        expr = op(CasOp::Add, vec![expr, Value::Int(1)]);
    }
    let result = expand_expr(&expr).unwrap();
    assert!(result.to_string().contains("x"));
}

#[test]
fn expand_high_power_no_stack_overflow() {
    // (x + 1)^20
    let base = op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]);
    let expr = op(CasOp::Power, vec![base, Value::Int(20)]);
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
        expr = op(CasOp::Add, vec![expr, Value::Int(1)]);
    }
    let result = simplify_cas_value(&expr).unwrap();
    assert!(result.to_string().contains("x"));
}

#[test]
fn simplify_deep_nested_multiplication() {
    // Build ((((x * 2) * 2) * 2) * ...) with depth 2000.
    let mut expr = Value::from_cas_var("x");
    for _ in 0..2000 {
        expr = op(CasOp::Multiply, vec![expr, Value::Int(2)]);
    }
    let result = simplify_cas_value(&expr).unwrap();
    assert!(result.to_string().contains("x"));
}

#[test]
fn factor_extracts_common_term() {
    let expr = op(
        CasOp::Add,
        vec![
            op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
            Value::from_cas_var("x"),
        ],
    );
    let result = factor_cas(&expr).unwrap();
    assert_eq!(result.to_string(), "@s x*(x + 1)");
}

#[test]
fn simplify_performs_exact_polynomial_division() {
    let expr = op(
        CasOp::Divide,
        vec![
            op(
                CasOp::Subtract,
                vec![
                    op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
                    Value::Int(1),
                ],
            ),
            op(
                CasOp::Subtract,
                vec![Value::from_cas_var("x"), Value::Int(1)],
            ),
        ],
    );
    assert_eq!(simplify_cas_value(&expr).unwrap().to_string(), "@s x + 1");
}

#[test]
fn solve_quadratic_equation() {
    let expr = op(
        CasOp::Subtract,
        vec![
            op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(2)]),
            Value::Int(4),
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
    assert!(roots.contains(&Value::Int(2)));
    assert!(roots.contains(&Value::Int(-2)));
}

#[test]
fn solve_quadratic_exact_radical_roots() {
    let x = Value::from_cas_var("x");
    let expr = cas_sub(
        cas_pow(x.clone(), Value::Int(2)).expect("x^2"),
        Value::Int(2),
    )
    .expect("x^2 - 2");
    let result = solve_cas(&expr, &x).expect("quadratic solve");
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    let root_text: Vec<String> = roots.iter().map(ToString::to_string).collect();
    assert_eq!(roots.len(), 2);
    assert!(root_text.iter().any(|root| root == "@s 2^(1/2)"));
    assert!(root_text.iter().any(|root| root == "@s -2^(1/2)"));
    assert!(
        roots
            .iter()
            .all(|root| !matches!(root, Value::Float(_) | Value::Complex(_))),
        "roots: {roots:?}"
    );
}

#[test]
fn solve_quadratic_repeated_root_stays_exact() {
    let x = Value::from_cas_var("x");
    let expr = cas_pow(
        cas_sub(x.clone(), Value::Int(1)).expect("x - 1"),
        Value::Int(2),
    )
    .expect("(x - 1)^2");
    let result = solve_cas(&expr, &x).expect("quadratic solve");
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    assert_eq!(roots.as_ref(), &vec![Value::Int(1), Value::Int(1)]);
}

#[test]
fn solve_real_domain_excludes_complex_roots() {
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![
        cas_pow(x.clone(), Value::Int(2)).expect("x^2"),
        Value::Int(1),
    ])
    .expect("x^2 + 1");
    let result = solve_cas_with_options(&expr, &x, &CasAssumptions::default(), SolveDomain::Real)
        .expect("real solve");
    assert_eq!(result, Value::List(Arc::new(Vec::new())));
}

#[test]
fn solve_real_domain_deduplicates_repeated_root() {
    let x = Value::from_cas_var("x");
    let expr = cas_pow(
        cas_sub(x.clone(), Value::Int(1)).expect("x - 1"),
        Value::Int(2),
    )
    .expect("(x - 1)^2");
    let result = solve_cas_with_options(&expr, &x, &CasAssumptions::default(), SolveDomain::Real)
        .expect("real solve");
    assert_eq!(result, Value::List(Arc::new(vec![Value::Int(1)])));
}

#[test]
fn solve_real_parameterized_quadratic_returns_discriminant_cases() {
    let a = Value::from_cas_var("a");
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![cas_pow(x.clone(), Value::Int(2)).expect("x^2"), a]).expect("x^2 + a");
    let assumptions = CasAssumptions::from_value(&Value::from_cas_predicate(CasPredicate::Real(
        Value::from_cas_var("a"),
    )))
    .expect("real coefficient assumption");
    let result = solve_cas_with_options(&expr, &x, &assumptions, SolveDomain::Real)
        .expect("real parameterized solve");
    let Value::Dict(result) = result else {
        panic!("expected conditional result");
    };
    let Value::List(cases) = &result["cases"] else {
        panic!("expected case list");
    };
    assert_eq!(cases.len(), 3);
}

#[test]
fn solve_real_parameterized_polynomial_requires_real_coefficients() {
    let a = Value::from_cas_var("a");
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![x.clone(), a]).expect("x + a");
    let error = solve_cas_with_options(&expr, &x, &CasAssumptions::default(), SolveDomain::Real)
        .expect_err("a symbolic coefficient needs a real assumption");
    assert!(
        error
            .msg
            .as_deref()
            .is_some_and(|message| message.contains("real[@s a]")),
        "unexpected error: {error:?}"
    );
}

#[test]
fn solve_identity_reports_infinite_solutions() {
    let x = Value::from_cas_var("x");
    let result = solve_cas(&Value::from_cas_eq(x.clone(), x.clone()), &x)
        .expect("identity should have an explicit result");
    assert_eq!(result, Value::Tag(Arc::from("all")));
}

#[test]
fn solve_parameterized_linear_equation() {
    let a = Value::from_cas_var("a");
    let b = Value::from_cas_var("b");
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![
        cas_mul(vec![a.clone(), x.clone()]).expect("a*x"),
        b.clone(),
    ])
    .expect("linear expression");
    let assumptions = CasAssumptions::from_value(&Value::from_cas_nonzero(a.clone()))
        .expect("valid leading coefficient assumption");
    let result =
        solve_cas_with_assumptions(&expr, &x, &assumptions).expect("parameterized linear solve");
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    let expected = cas_div(cas_neg(b).expect("-b"), a).expect("-b/a");
    assert_eq!(roots.as_ref(), &vec![expected]);
}

#[test]
fn solve_parameterized_linear_equation_returns_coefficient_cases() {
    let a = Value::from_cas_var("a");
    let x = Value::from_cas_var("x");
    let expr = cas_mul(vec![a, x.clone()]).expect("a*x");

    let result = solve_cas(&expr, &x).expect("unknown leading coefficient should branch");
    let Value::Dict(result) = result else {
        panic!("expected conditional result");
    };
    let Value::List(cases) = &result["cases"] else {
        panic!("expected case list");
    };
    assert_eq!(cases.len(), 2);
}

#[test]
fn solve_parameterized_linear_equation_downgrades_under_zero_assumption() {
    let a = Value::from_cas_var("a");
    let b = Value::from_cas_var("b");
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![
        cas_mul(vec![a.clone(), x.clone()]).expect("a*x"),
        b.clone(),
    ])
    .expect("a*x+b");
    let assumptions = CasAssumptions::from_value(&Value::List(Arc::new(vec![
        Value::from_cas_eq(a, Value::Int(0)),
        Value::from_cas_nonzero(b),
    ])))
    .expect("valid degenerate assumptions");

    let result = solve_cas_with_assumptions(&expr, &x, &assumptions)
        .expect("nonzero constant equation has no roots");
    assert_eq!(result, Value::List(Arc::new(Vec::new())));
}

#[test]
fn solve_parameterized_quadratic_equation() {
    let a = Value::from_cas_var("a");
    let b = Value::from_cas_var("b");
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![
        cas_pow(x.clone(), Value::Int(2)).expect("x^2"),
        cas_mul(vec![a.clone(), x.clone()]).expect("a*x"),
        b.clone(),
    ])
    .expect("quadratic expression");
    let result = solve_cas(&expr, &x).expect("parameterized quadratic solve");
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };

    let disc = cas_sub(
        cas_pow(a.clone(), Value::Int(2)).expect("a^2"),
        cas_mul(vec![Value::Int(4), b]).expect("4*b"),
    )
    .expect("discriminant");
    let sqrt_disc = cas_pow(
        disc,
        Value::from_fraction_parts(BigInt::from(1), BigInt::from(2)),
    )
    .expect("sqrt discriminant");
    let neg_a = cas_neg(a).expect("-a");
    let expected_plus = simplify_cas_value(
        &cas_div(
            cas_add(vec![neg_a.clone(), sqrt_disc.clone()]).expect("plus numerator"),
            Value::Int(2),
        )
        .expect("plus root"),
    )
    .expect("simplified plus root");
    let expected_minus = simplify_cas_value(
        &cas_div(
            cas_sub(neg_a, sqrt_disc).expect("minus numerator"),
            Value::Int(2),
        )
        .expect("minus root"),
    )
    .expect("simplified minus root");

    assert_eq!(roots.len(), 2);
    assert!(roots.contains(&expected_plus), "roots: {roots:?}");
    assert!(roots.contains(&expected_minus), "roots: {roots:?}");
}

#[test]
fn solve_monomial_cubic_equation() {
    let expr = op(
        CasOp::Subtract,
        vec![
            op(CasOp::Power, vec![Value::from_cas_var("x"), Value::Int(3)]),
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
fn solve_real_monomial_cubic_returns_only_real_root() {
    let x = Value::from_cas_var("x");
    let expr = cas_sub(
        cas_pow(x.clone(), Value::Int(3)).expect("x^3"),
        Value::Int(8),
    )
    .expect("x^3 - 8");
    let result = solve_cas_with_options(&expr, &x, &CasAssumptions::default(), SolveDomain::Real)
        .expect("real cubic solve");
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    assert_eq!(roots.len(), 1);
    assert!(
        roots[0]
            .as_f64()
            .is_some_and(|value| (value - 2.0).abs() < 1e-9)
    );
}

#[test]
fn solve_monomial_quintic_equation() {
    let x = Value::from_cas_var("x");
    let expr = cas_sub(
        cas_pow(x.clone(), Value::Int(5)).expect("x^5"),
        Value::Int(1),
    )
    .expect("x^5 - 1");
    let result = solve_cas(&expr, &x).expect("monomial quintic solve");
    let Value::List(roots) = result else {
        panic!("expected list of roots");
    };
    assert_eq!(roots.len(), 5);
    assert_eq!(roots.first(), Some(&Value::Int(1)));
    assert!(
        roots
            .iter()
            .all(|root| !matches!(root, Value::Float(_) | Value::Complex(_)))
    );
}

#[test]
fn solve_general_cubic_reports_binomial_limit() {
    let x = Value::from_cas_var("x");
    let expr = cas_add(vec![
        cas_pow(x.clone(), Value::Int(3)).expect("x^3"),
        x.clone(),
        Value::Int(-1),
    ])
    .expect("x^3 + x - 1");
    let err = solve_cas(&expr, &x).expect_err("general cubic solve should fail");
    assert!(
        err.msg
            .as_deref()
            .is_some_and(|msg| msg.contains("a*x^3 + b = 0")),
        "unexpected error: {err:?}"
    );
}

fn xy_sum_diff_equations() -> Value {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    Value::List(Arc::new(vec![
        Value::from_cas_eq(
            cas_add(vec![x.clone(), y.clone()]).expect("x+y"),
            Value::Int(3),
        ),
        Value::from_cas_eq(cas_sub(x, y).expect("x-y"), Value::Int(1)),
    ]))
}

#[test]
fn solve_linear_system_returns_dict_in_variable_order() {
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            op(
                CasOp::Add,
                vec![
                    op(
                        CasOp::Multiply,
                        vec![Value::Int(2), Value::from_cas_var("x")],
                    ),
                    Value::from_cas_var("y"),
                ],
            ),
            Value::Int(5),
        ),
        Value::from_cas_eq(
            op(
                CasOp::Subtract,
                vec![Value::from_cas_var("x"), Value::from_cas_var("y")],
            ),
            Value::Int(1),
        ),
    ]));
    let vars = Value::List(Arc::new(vec![
        Value::from_cas_var("x"),
        Value::from_cas_var("y"),
    ]));
    let result = solve_system_cas(&equations, &vars).unwrap();
    assert_dict_entries(&result, &[("x", Value::Int(2)), ("y", Value::Int(1))]);
}

#[test]
fn solve_linear_system_uses_explicit_variable_order_for_dict() {
    let equations = xy_sum_diff_equations();
    let vars = Value::List(Arc::new(vec![
        Value::from_cas_var("y"),
        Value::from_cas_var("x"),
    ]));

    let result = solve_system_cas(&equations, &vars).unwrap();
    assert_dict_entries(&result, &[("y", Value::Int(1)), ("x", Value::Int(2))]);
}

#[test]
fn solve_linear_system_rejects_duplicate_explicit_variables() {
    let equations = xy_sum_diff_equations();
    let vars = Value::List(Arc::new(vec![
        Value::from_cas_var("x"),
        Value::from_cas_var("x"),
    ]));

    let err = solve_system_cas(&equations, &vars).expect_err("duplicate variables should fail");
    assert!(
        err.msg
            .as_deref()
            .is_some_and(|msg| msg.contains("appears more than once")),
        "unexpected error: {err:?}"
    );
}

#[test]
fn solve_linear_system_allows_explicit_parameters() {
    let b = Value::from_cas_var("b");
    let c = Value::from_cas_var("c");
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            cas_add(vec![
                cas_mul(vec![Value::Int(2), x.clone()]).expect("2*x"),
                y.clone(),
            ])
            .expect("first lhs"),
            b.clone(),
        ),
        Value::from_cas_eq(cas_sub(x.clone(), y.clone()).expect("x-y"), c.clone()),
    ]));
    let vars = Value::List(Arc::new(vec![x, y]));
    let result = solve_system_cas(&equations, &vars).expect("parameterized system solve");

    let expected_x = cas_div(
        cas_add(vec![b.clone(), c.clone()]).expect("b+c"),
        Value::Int(3),
    )
    .expect("x solution");
    let expected_y = cas_div(
        cas_sub(b, cas_mul(vec![Value::Int(2), c]).expect("2*c")).expect("b-2*c"),
        Value::Int(3),
    )
    .expect("y solution");

    assert_dict_entries(&result, &[("x", expected_x), ("y", expected_y)]);
}

#[test]
fn solve_linear_system_infers_variables_in_name_order() {
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            op(
                CasOp::Add,
                vec![Value::from_cas_var("b"), Value::from_cas_var("a")],
            ),
            Value::Int(3),
        ),
        Value::from_cas_eq(
            op(
                CasOp::Subtract,
                vec![Value::from_cas_var("b"), Value::from_cas_var("a")],
            ),
            Value::Int(1),
        ),
    ]));

    let result = solve_system_infer_cas(&equations).unwrap();
    assert_dict_entries(&result, &[("a", Value::Int(1)), ("b", Value::Int(2))]);
}

#[test]
fn solve_linear_system_accepts_overdetermined_unique_system() {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            cas_add(vec![x.clone(), y.clone()]).expect("x+y"),
            Value::Int(3),
        ),
        Value::from_cas_eq(cas_sub(x.clone(), y.clone()).expect("x-y"), Value::Int(1)),
        Value::from_cas_eq(
            cas_add(vec![
                cas_mul(vec![Value::Int(2), x]).expect("2*x"),
                cas_mul(vec![Value::Int(2), y]).expect("2*y"),
            ])
            .expect("2*x+2*y"),
            Value::Int(6),
        ),
    ]));

    let result = solve_system_infer_cas(&equations).unwrap();
    assert_dict_entries(&result, &[("x", Value::Int(2)), ("y", Value::Int(1))]);
}

#[test]
fn solve_linear_system_returns_parametric_dependent_system() {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            cas_add(vec![x.clone(), y.clone()]).expect("x+y"),
            Value::Int(3),
        ),
        Value::from_cas_eq(
            cas_add(vec![
                cas_mul(vec![Value::Int(2), x]).expect("2*x"),
                cas_mul(vec![Value::Int(2), y]).expect("2*y"),
            ])
            .expect("2*x+2*y"),
            Value::Int(6),
        ),
    ]));

    let result = solve_system_infer_cas(&equations).expect("dependent system should solve");
    let Value::Dict(result) = result else {
        panic!("expected parametric result");
    };
    let Value::List(parameters) = &result["parameters"] else {
        panic!("expected parameter list");
    };
    assert_eq!(parameters.len(), 1);
    assert!(matches!(&result["solution"], Value::Dict(_)));
}

#[test]
fn solve_linear_system_returns_no_solution_for_inconsistent_system() {
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            cas_add(vec![x.clone(), y.clone()]).expect("x+y"),
            Value::Int(3),
        ),
        Value::from_cas_eq(cas_add(vec![x, y]).expect("x+y"), Value::Int(4)),
    ]));

    let result = solve_system_infer_cas(&equations).expect("inconsistent system should resolve");
    assert_eq!(result, Value::Tag(Arc::from("none")));
}

#[test]
fn solve_symbolic_single_equation_returns_determinant_cases() {
    let a = Value::from_cas_var("a");
    let x = Value::from_cas_var("x");
    let equations = Value::List(Arc::new(vec![Value::from_cas_eq(
        cas_mul(vec![a.clone(), x.clone()]).expect("a*x"),
        Value::Int(1),
    )]));
    let vars = Value::List(Arc::new(vec![x]));

    let result =
        solve_system_cas(&equations, &vars).expect("an unknown determinant should produce cases");
    let Value::Dict(result) = result else {
        panic!("expected conditional result");
    };
    let Value::List(cases) = &result["cases"] else {
        panic!("expected case list");
    };
    assert_eq!(cases.len(), 2);
}

#[test]
fn solve_symbolic_seven_by_seven_system_returns_determinant_cases() {
    let a = Value::from_cas_var("a");
    let vars = (0..7)
        .map(|idx| Value::from_cas_var(format!("x{idx}")))
        .collect::<Vec<_>>();
    let equations = vars
        .iter()
        .enumerate()
        .map(|(idx, var)| {
            let lhs = if idx == 0 {
                cas_mul(vec![a.clone(), var.clone()]).expect("a*x0")
            } else {
                var.clone()
            };
            Value::from_cas_eq(lhs, Value::Int(idx as i64 + 1))
        })
        .collect::<Vec<_>>();
    let result = solve_system_cas(
        &Value::List(Arc::new(equations)),
        &Value::List(Arc::new(vars)),
    )
    .expect("seven by seven symbolic solve");
    let Value::Dict(result) = result else {
        panic!("expected conditional result");
    };
    let Value::List(cases) = &result["cases"] else {
        panic!("expected case list");
    };
    assert_eq!(cases.len(), 2);
}

#[test]
fn solve_symbolic_single_equation_uses_nonzero_assumption() {
    let a = Value::from_cas_var("a");
    let x = Value::from_cas_var("x");
    let equations = Value::List(Arc::new(vec![Value::from_cas_eq(
        cas_mul(vec![a.clone(), x.clone()]).expect("a*x"),
        Value::Int(1),
    )]));
    let vars = Value::List(Arc::new(vec![x]));
    let predicate = Value::from_cas_nonzero(a.clone());
    let assumptions = CasAssumptions::from_value(&predicate).expect("valid assumption");

    let result = solve_system_cas_with_assumptions(&equations, &vars, &assumptions)
        .expect("nonzero determinant should solve");
    assert_dict_entries(&result, &[("x", cas_pow(a, Value::Int(-1)).expect("a^-1"))]);
}

#[test]
fn solve_symbolic_two_by_two_uses_determinant_assumption() {
    let a = Value::from_cas_var("a");
    let b = Value::from_cas_var("b");
    let c = Value::from_cas_var("c");
    let d = Value::from_cas_var("d");
    let x = Value::from_cas_var("x");
    let y = Value::from_cas_var("y");
    let equations = Value::List(Arc::new(vec![
        Value::from_cas_eq(
            cas_add(vec![
                cas_mul(vec![a.clone(), x.clone()]).expect("a*x"),
                cas_mul(vec![b.clone(), y.clone()]).expect("b*y"),
            ])
            .expect("first lhs"),
            Value::Int(1),
        ),
        Value::from_cas_eq(
            cas_add(vec![
                cas_mul(vec![c.clone(), x.clone()]).expect("c*x"),
                cas_mul(vec![d.clone(), y.clone()]).expect("d*y"),
            ])
            .expect("second lhs"),
            Value::Int(2),
        ),
    ]));
    let vars = Value::List(Arc::new(vec![x, y]));
    let determinant = cas_sub(
        cas_mul(vec![a, d]).expect("a*d"),
        cas_mul(vec![b, c]).expect("b*c"),
    )
    .expect("determinant");
    let assumptions = CasAssumptions::from_value(&Value::from_cas_nonzero(determinant))
        .expect("valid determinant assumption");

    solve_system_cas_with_assumptions(&equations, &vars, &assumptions)
        .expect("a nonzero determinant should solve without assuming a is nonzero");
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
    let expr = op(
        CasOp::Multiply,
        vec![Value::Int(2), Value::from_cas_var("x")],
    );
    assert_eq!(
        extract_linear_coefficients(&simplify_cas_value(&expr).unwrap(), "x"),
        Some((Value::Int(2), Value::Int(0)))
    );
}

#[test]
fn linear_coeff_sum() {
    let expr = op(
        CasOp::Add,
        vec![
            op(
                CasOp::Multiply,
                vec![Value::Int(2), Value::from_cas_var("x")],
            ),
            Value::Int(3),
        ],
    );
    assert_eq!(
        extract_linear_coefficients(&simplify_cas_value(&expr).unwrap(), "x"),
        Some((Value::Int(2), Value::Int(3)))
    );
}

#[test]
fn linear_coeff_symbolic_params() {
    let expr = cas_add(vec![
        cas_mul(vec![Value::from_cas_var("a"), Value::from_cas_var("x")]).expect("a*x"),
        Value::from_cas_var("b"),
    ])
    .expect("a*x+b");
    assert_eq!(
        extract_linear_coefficients_with_params(&expr, "x"),
        Some((Value::from_cas_var("a"), Value::from_cas_var("b")))
    );
}

#[test]
fn linear_coeff_product_of_sum() {
    let expr = simplify_cas_value(&op(
        CasOp::Divide,
        vec![
            op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
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
    let expr = simplify_cas_value(&op(
        CasOp::Multiply,
        vec![
            Value::Int(-1),
            op(CasOp::Add, vec![Value::from_cas_var("x"), Value::Int(1)]),
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
    let result = simplify_cas_value(&call(CasFunction::Erf, vec![Value::Int(0)])).unwrap();
    assert_eq!(result, Value::Int(0));
}

#[test]
fn numeric_erfc_zero() {
    let result = simplify_cas_value(&call(CasFunction::Erfc, vec![Value::Int(0)])).unwrap();
    assert_eq!(result, Value::Int(1));
}

#[test]
fn numeric_gamma_five() {
    // gamma(5) = 4! = 24
    let result = simplify_cas_value(&call(CasFunction::Gamma, vec![Value::Int(5)])).unwrap();
    assert_eq!(result, Value::Int(24));
}

#[test]
fn numeric_heaviside_pos() {
    let result =
        simplify_cas_value(&call(CasFunction::Heaviside, vec![Value::float(3.0)])).unwrap();
    assert_eq!(result, Value::float(1.0));
}

#[test]
fn numeric_heaviside_neg() {
    let result =
        simplify_cas_value(&call(CasFunction::Heaviside, vec![Value::float(-3.0)])).unwrap();
    assert_eq!(result, Value::float(0.0));
}

#[test]
fn numeric_eval_handles_binary_math_calls() {
    let log = call(CasFunction::Log, vec![Value::Int(8), Value::Int(2)]);
    assert_eq!(eval_numeric_cas(&log).unwrap(), Value::float(3.0));

    let arctan2 = call(CasFunction::ArcTan2, vec![Value::Int(1), Value::Int(1)]);
    let result = eval_numeric_cas(&arctan2).unwrap();
    let Value::Float(value) = result else {
        panic!("expected float");
    };
    assert!((*value - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
}

#[test]
fn numeric_rejects_symbolic_application() {
    let expr = Value::from_cas_apply("f", vec![Value::Int(2)]);
    let err = eval_numeric_cas(&expr).expect_err("application should stay symbolic");
    assert!(
        err.msg.as_deref().is_some_and(|msg| {
            msg.contains("application 'f' is not supported in numeric evaluation")
        }),
        "unexpected error: {err:?}",
    );
}

#[test]
fn algebraic_constructor_rejects_symbolic_coefficients() {
    let field = AlgebraicField::new_real_root(
        vec![BigInt::from(-2), BigInt::from(0), BigInt::from(1)],
        (1.0, 2.0),
    )
    .expect("valid sqrt2 field");
    assert!(AlgebraicData::new(field, vec![Value::from_cas_var("x")]).is_err());
}

#[test]
fn normalize_merges_multiple_reciprocals() {
    // simplifiy_cas_value should combine terms with multiple (^ D -1) factors.
    let d1 = cas_add(vec![Value::from_cas_var("x"), Value::Int(1)]).unwrap();
    let d2 = cas_add(vec![Value::from_cas_var("x"), Value::Int(-1)]).unwrap();
    let inv1 = op(CasOp::Power, vec![d1, Value::Int(-1)]);
    let inv2 = op(CasOp::Power, vec![d2, Value::Int(-1)]);
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
fn simplify_leaves_large_distinct_rational_sum_uncombined() {
    let x = Value::from_cas_var("x");
    let mut terms = Vec::new();
    for offset in 1..=12 {
        let denom = cas_add(vec![x.clone(), Value::Int(offset)])
            .expect("linear denominator should simplify");
        terms.push(cas_pow(denom, Value::Int(-1)).expect("reciprocal should simplify"));
    }

    let sum = cas_add(terms).expect("rational sum should simplify");
    let simplified = simplify_cas_value(&sum).expect("rational sum should remain valid");
    assert!(
        count_inverse_powers(&simplified) > 1,
        "expected large distinct rational sum to stay uncombined, got: {simplified}"
    );
}

#[test]
fn rewrite_distributes_negation_over_sum() {
    // (* -1 (+ x y)) -> (+ (* -1 x) (* -1 y))
    let sum = cas_add(vec![Value::from_cas_var("x"), Value::from_cas_var("y")]).unwrap();
    let product = cas_mul(vec![Value::Int(-1), sum]).unwrap();
    let rewritten = rewrite_expr(&product).unwrap();
    assert!(
        rewritten
            .cas_op_parts()
            .is_some_and(|(op, _)| op == CasOp::Add),
        "expected sum, got: {}",
        rewritten
    );
}

#[test]
fn rewrite_sgn_abs_product_cancels() {
    // sgn(u) * abs(u)^(-1) -> u^(-1)
    let u = Value::from_cas_var("x");
    let sgn = call(CasFunction::Sgn, vec![u.clone()]);
    let abs_inv = op(
        CasOp::Power,
        vec![call(CasFunction::Abs, vec![u.clone()]), Value::Int(-1)],
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
    assert_eq!(simplified.to_string(), "@s (x^2 + 1)^(-1/2)/x");
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
    assert_eq!(rewritten.to_string(), "@s (x^2 + 1)^(-1/2)/x");
}

#[test]
fn rewrite_combines_unit_with_fraction_sum() {
    let x = Value::from_cas_var("x");
    let denom = cas_add(vec![x.clone(), Value::Int(1)]).unwrap();
    let frac = cas_div(x, denom).unwrap();
    let expr = cas_add(vec![Value::Int(1), cas_neg(frac).unwrap()]).unwrap();

    let rewritten = rewrite_cas(&expr).unwrap();
    assert_eq!(rewritten.to_string(), "@s (x + 1)^-1");
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
    assert_eq!(roundtrip.to_string(), "@s (x^2 + 1)^(-1/2)/x");
}
