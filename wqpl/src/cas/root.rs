use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::{
    CasNamedArg, cas_err, cas_internal_err, infer_single_cas_var, normalize_root_objective_cas,
    numeric_mul, poly_degree, poly_divide, poly_from_expr,
};
use crate::value::algebraic::{AlgebraicData, AlgebraicField, validate_real_root_interval};
use crate::value::{Value, WqResult};

pub(crate) fn cas_root_expr(args: &[Value], named: &[CasNamedArg]) -> WqResult<Value> {
    if !named.is_empty() {
        return Err(cas_err("'root' does not accept named arguments"));
    }
    if !(2..=4).contains(&args.len()) {
        return Err(cas_err(
            "'root' expects 'root[poly;near]', 'root[poly;lo;hi]', 'root[poly;var;near]', or 'root[poly;var;lo;hi]'",
        ));
    }

    let objective = normalize_root_objective_cas(&args[0])?;
    let (var, selector_start) = match args {
        [_, var, _] if var.cas_var_name().is_some() => (
            var.cas_var_name()
                .expect("explicit root variable checked")
                .to_string(),
            2,
        ),
        [_, var, _, _] => (
            var.cas_var_name()
                .ok_or_else(|| cas_err("'root' target must be a symbolic variable").got1(var))?
                .to_string(),
            2,
        ),
        _ => (infer_single_cas_var(&objective)?, 1),
    };
    let coeffs = poly_from_expr(&objective, &var)?;
    if poly_degree(&coeffs) < 2 {
        return Err(cas_err("'root' expects a polynomial of degree at least 2"));
    }
    let poly = integer_poly_from_coeffs(&coeffs)?;

    let selectors = &args[selector_start..];
    let interval = if let [near] = selectors {
        let near = required_finite_f64(near, "near")?;
        root_interval_near(&poly, near)?
    } else if let [lo, hi] = selectors {
        let lo = required_finite_f64(lo, "lo")?;
        let hi = required_finite_f64(hi, "hi")?;
        (lo, hi)
    } else {
        return Err(cas_err(
            "'root' expects one near value or a lower and upper bound",
        ));
    };

    let poly = match select_rational_factor(poly, interval)? {
        RootSelection::Exact(value) => return Ok(value),
        RootSelection::Polynomial(poly) => poly,
    };
    let field = AlgebraicField::new_real_root(poly, interval)?;
    AlgebraicData::generator(field)
}

enum RootSelection {
    Exact(Value),
    Polynomial(Vec<BigInt>),
}

fn rational_in_interval(value: &Value, interval: (f64, f64)) -> WqResult<bool> {
    let Some((numer, denom)) = value.rational_parts() else {
        return Err(cas_internal_err("selecting an exact polynomial root"));
    };
    let value = BigRational::new(numer, denom);
    let lo = BigRational::from_float(interval.0)
        .ok_or_else(|| cas_err("root lower bound must be finite"))?;
    let hi = BigRational::from_float(interval.1)
        .ok_or_else(|| cas_err("root upper bound must be finite"))?;
    Ok(lo < value && value < hi)
}

fn select_rational_factor(poly: Vec<BigInt>, interval: (f64, f64)) -> WqResult<RootSelection> {
    validate_real_root_interval(&poly, interval)?;
    let mut coeffs = poly
        .iter()
        .cloned()
        .map(Value::from_bigint)
        .collect::<Vec<_>>();

    loop {
        let degree = poly_degree(&coeffs);
        if degree == 1 {
            let root = coeffs[0].neg()?.divide(&coeffs[1])?;
            if rational_in_interval(&root, interval)? {
                return Ok(RootSelection::Exact(root));
            }
            return Err(cas_internal_err("selecting an exact polynomial root"));
        }

        let Some(root) = super::integrate::rational::find_rational_root_value(&coeffs) else {
            return integer_poly_from_coeffs(&coeffs).map(RootSelection::Polynomial);
        };
        if rational_in_interval(&root, interval)? {
            return Ok(RootSelection::Exact(root));
        }

        let factor = vec![root.neg()?, Value::Int(1)];
        let (quotient, remainder) = poly_divide(&coeffs, &factor)?;
        if !remainder.iter().all(super::numeric_is_zero) {
            return Err(cas_internal_err("selecting an exact polynomial root"));
        }
        coeffs = quotient;
    }
}

fn required_finite_f64(value: &Value, name: &str) -> WqResult<f64> {
    let n = value
        .as_f64()
        .ok_or_else(|| cas_err(format!("'root' expects a real {name} value")).got1(value))?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(cas_err(format!("'root' expects a finite {name} value")).got1(value))
    }
}

fn integer_poly_from_coeffs(coeffs: &[Value]) -> WqResult<Vec<BigInt>> {
    let mut lcm = BigInt::one();
    for coeff in coeffs {
        let Some((_numer, denom)) = coeff.rational_parts() else {
            return Err(
                cas_err("'root' expects exact rational polynomial coefficients").got1(coeff),
            );
        };
        lcm = bigint_lcm(&lcm, &denom);
    }

    let lcm_value = Value::from_bigint(lcm);
    let mut poly = Vec::with_capacity(coeffs.len());
    for coeff in coeffs {
        let scaled = numeric_mul(coeff, &lcm_value)?;
        let Some((numer, denom)) = scaled.rational_parts() else {
            return Err(
                cas_err("'root' expects exact rational polynomial coefficients").got1(coeff),
            );
        };
        if !denom.is_one() {
            return Err(cas_internal_err("normalizing a root polynomial"));
        }
        poly.push(numer);
    }

    while poly.len() > 1 && poly.last().is_some_and(Zero::is_zero) {
        poly.pop();
    }
    if poly.iter().all(Zero::is_zero) {
        return Err(cas_err("'root' expects a non-zero polynomial"));
    }
    Ok(poly)
}

fn bigint_lcm(lhs: &BigInt, rhs: &BigInt) -> BigInt {
    if lhs.is_zero() || rhs.is_zero() {
        return BigInt::one();
    }
    (lhs / bigint_gcd(lhs, rhs)) * rhs
}

fn bigint_gcd(lhs: &BigInt, rhs: &BigInt) -> BigInt {
    let mut a = lhs.abs();
    let mut b = rhs.abs();
    while !b.is_zero() {
        let r = &a % &b;
        a = b;
        b = r;
    }
    a
}

fn root_interval_near(poly: &[BigInt], near: f64) -> WqResult<(f64, f64)> {
    let mut width = near.abs().max(1.0) * 1e-12;
    for _ in 0..100 {
        let lo = near - width;
        let hi = near + width;
        if interval_brackets_root(poly, lo, hi)? {
            return refine_root_interval(poly, lo, hi);
        }
        width *= 2.0;
    }

    Err(cas_err(
        "root could not isolate a real root near the requested point",
    ))
}

fn refine_root_interval(poly: &[BigInt], mut lo: f64, mut hi: f64) -> WqResult<(f64, f64)> {
    let mut lo_value = eval_integer_poly_f64(poly, lo)?;
    let hi_value = eval_integer_poly_f64(poly, hi)?;
    if lo_value == 0.0 {
        return Ok((lo, hi));
    }
    if hi_value == 0.0 {
        return Ok((lo, hi));
    }
    if lo_value.is_sign_positive() == hi_value.is_sign_positive() {
        return Ok((lo, hi));
    }

    for _ in 0..96 {
        let mid = lo + (hi - lo) * 0.5;
        if mid <= lo || mid >= hi {
            break;
        }
        let mid_value = eval_integer_poly_f64(poly, mid)?;
        if mid_value == 0.0 {
            return Ok((mid, hi));
        }
        if lo_value.is_sign_positive() == mid_value.is_sign_positive() {
            lo = mid;
            lo_value = mid_value;
        } else {
            hi = mid;
        }
        let scale = mid.abs().max(1.0);
        if (hi - lo).abs() <= scale * 1e-12 {
            break;
        }
    }
    Ok((lo, hi))
}

fn interval_brackets_root(poly: &[BigInt], lo: f64, hi: f64) -> WqResult<bool> {
    if !lo.is_finite() || !hi.is_finite() || lo >= hi {
        return Ok(false);
    }
    let lo_value = eval_integer_poly_f64(poly, lo)?;
    let hi_value = eval_integer_poly_f64(poly, hi)?;
    Ok(lo_value == 0.0
        || hi_value == 0.0
        || lo_value.is_sign_positive() != hi_value.is_sign_positive())
}

fn eval_integer_poly_f64(poly: &[BigInt], x: f64) -> WqResult<f64> {
    let mut acc = 0.0f64;
    for coeff in poly.iter().rev() {
        let coeff = coeff.to_f64().ok_or_else(|| {
            cas_err("root polynomial is too large to evaluate near the requested point")
        })?;
        acc = acc.mul_add(x, coeff);
    }
    if acc.is_finite() {
        Ok(acc)
    } else {
        Err(cas_err(
            "root polynomial evaluation produced a non-finite value",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::*;
    use crate::value::cas::CasOp;

    fn sqrt2_poly() -> Value {
        Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(CasOp::Power, vec![Value::from_cas_var("t"), Value::Int(2)]),
                Value::Int(-2),
            ],
        )
    }

    #[test]
    fn root_special_form_constructs_algebraic_value() {
        let root = cas_root_expr(
            &[
                sqrt2_poly(),
                Value::from_cas_var("t"),
                Value::Int(1),
                Value::Int(2),
            ],
            &[],
        )
        .expect("root value");

        assert!(matches!(root, Value::Algebraic(_)));
        assert_eq!(root.to_string(), "2^(1/2)");
    }

    #[test]
    fn root_arity_error_shows_complete_call_syntax() {
        let err = cas_root_expr(&[sqrt2_poly()], &[]).expect_err("root arity should fail");

        assert_eq!(
            err.msg.as_deref(),
            Some(
                "'root' expects 'root[poly;near]', 'root[poly;lo;hi]', 'root[poly;var;near]', or 'root[poly;var;lo;hi]'"
            )
        );
    }

    #[test]
    fn equal_root_selections_have_stable_identity_and_hash() {
        let first =
            cas_root_expr(&[sqrt2_poly(), Value::Int(1), Value::Int(2)], &[]).expect("first root");
        let second = cas_root_expr(&[sqrt2_poly(), Value::float(1.4)], &[]).expect("second root");
        let mut first_hash = DefaultHasher::new();
        let mut second_hash = DefaultHasher::new();
        first.hash(&mut first_hash);
        second.hash(&mut second_hash);

        assert_eq!(first, second);
        assert_eq!(first_hash.finish(), second_hash.finish());
    }

    #[test]
    fn root_removes_unselected_rational_factor_before_field_construction() {
        let placeholder = Value::from_cas_var("t");
        let rational_factor = Value::from_cas_op(CasOp::Add, vec![placeholder, Value::Int(-3)]);
        let polynomial = Value::from_cas_op(CasOp::Multiply, vec![sqrt2_poly(), rational_factor]);
        let value =
            cas_root_expr(&[polynomial, Value::Int(1), Value::Int(2)], &[]).expect("root value");
        let denominator = value.subtract(&Value::Int(3)).expect("alpha minus three");
        let quotient = Value::Int(1)
            .divide(&denominator)
            .expect("selected root field supports division");
        let product = quotient.multiply(&denominator).expect("inverse product");

        assert_eq!(value.to_string(), "2^(1/2)");
        assert!(crate::cas::numeric_is_one(&product));
    }
}
