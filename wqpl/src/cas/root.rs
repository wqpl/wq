use num_bigint::BigInt;
use num_traits::{One, Signed, ToPrimitive, Zero};

use super::quote::CasNamedArg;
use super::{
    cas_err, infer_single_cas_var, normalize_root_objective_cas, numeric_mul, poly_degree,
    poly_from_expr, poly_to_expr,
};
use crate::value::algebraic::{AlgebraicData, AlgebraicField};
use crate::value::{Value, WqResult};

pub(crate) fn cas_root_expr(args: &[Value], named: &[CasNamedArg]) -> WqResult<Value> {
    if !named.is_empty() {
        return Err(cas_err("root does not accept named arguments"));
    }
    if args.len() != 2 && args.len() != 3 {
        return Err(cas_err("root expects poly;near or poly;lo;hi"));
    }

    let objective = normalize_root_objective_cas(&args[0])?;
    let var = infer_single_cas_var(&objective)?;
    let coeffs = poly_from_expr(&objective, &var)?;
    if poly_degree(&coeffs) < 2 {
        return Err(cas_err("root expects a polynomial of degree at least 2"));
    }
    let poly = integer_poly_from_coeffs(&coeffs)?;

    let interval = if args.len() == 2 {
        let near = required_finite_f64(&args[1], "near")?;
        root_interval_near(&poly, near)?
    } else {
        let lo = required_finite_f64(&args[1], "lo")?;
        let hi = required_finite_f64(&args[2], "hi")?;
        (lo, hi)
    };

    let field = AlgebraicField::new_real_root(poly, interval)?;
    let normalized_poly = integer_poly_expr(field.poly(), &var)?;
    let (lo, hi) = field.interval();
    Ok(Value::from_cas_root(normalized_poly, lo, hi))
}

pub(crate) fn resolve_cas_root(value: &Value) -> WqResult<Option<Value>> {
    let Some((poly, lo, hi)) = value.cas_root_parts() else {
        return Ok(None);
    };
    root_value(poly, (lo, hi)).map(Some)
}

fn root_value(poly_expr: &Value, interval: (f64, f64)) -> WqResult<Value> {
    let objective = normalize_root_objective_cas(poly_expr)?;
    let var = infer_single_cas_var(&objective)?;
    let coeffs = poly_from_expr(&objective, &var)?;
    if poly_degree(&coeffs) < 2 {
        return Err(cas_err("root expects a polynomial of degree at least 2"));
    }
    let poly = integer_poly_from_coeffs(&coeffs)?;
    let field = AlgebraicField::new_real_root(poly, interval)?;
    AlgebraicData::generator(field)
}

fn integer_poly_expr(poly: &[BigInt], var: &str) -> WqResult<Value> {
    let coeffs = poly
        .iter()
        .cloned()
        .map(Value::from_bigint)
        .collect::<Vec<_>>();
    poly_to_expr(&coeffs, var)
}

fn required_finite_f64(value: &Value, name: &str) -> WqResult<f64> {
    let n = value
        .as_f64()
        .ok_or_else(|| cas_err(format!("root expects a real {name} value")).got1(value))?;
    if n.is_finite() {
        Ok(n)
    } else {
        Err(cas_err(format!("root expects a finite {name} value")).got1(value))
    }
}

fn integer_poly_from_coeffs(coeffs: &[Value]) -> WqResult<Vec<BigInt>> {
    let mut lcm = BigInt::one();
    for coeff in coeffs {
        let Some((_numer, denom)) = coeff.rational_parts() else {
            return Err(cas_err("root expects exact rational polynomial coefficients").got1(coeff));
        };
        lcm = bigint_lcm(&lcm, &denom);
    }

    let lcm_value = Value::from_bigint(lcm);
    let mut poly = Vec::with_capacity(coeffs.len());
    for coeff in coeffs {
        let scaled = numeric_mul(coeff, &lcm_value)?;
        let Some((numer, denom)) = scaled.rational_parts() else {
            return Err(cas_err("root expects exact rational polynomial coefficients").got1(coeff));
        };
        if !denom.is_one() {
            return Err(
                cas_err("root could not clear polynomial coefficient denominators").got1(coeff),
            );
        }
        poly.push(numer);
    }

    while poly.len() > 1 && poly.last().is_some_and(Zero::is_zero) {
        poly.pop();
    }
    if poly.iter().all(Zero::is_zero) {
        return Err(cas_err("root expects a non-zero polynomial"));
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
        "root could not isolate a real root near the selector",
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
        let coeff = coeff
            .to_f64()
            .ok_or_else(|| cas_err("root polynomial is too large to evaluate as a selector"))?;
        acc = acc.mul_add(x, coeff);
    }
    if acc.is_finite() {
        Ok(acc)
    } else {
        Err(cas_err(
            "root polynomial selector evaluation produced a non-finite value",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::cas::CasOp;

    fn sqrt2_poly() -> Value {
        Value::from_cas_op(
            CasOp::Add,
            vec![
                Value::from_cas_op(CasOp::Power, vec![Value::from_cas_var("_"), Value::Int(2)]),
                Value::Int(-2),
            ],
        )
    }

    #[test]
    fn root_special_form_constructs_cas_node() {
        let root =
            cas_root_expr(&[sqrt2_poly(), Value::Int(1), Value::Int(2)], &[]).expect("root node");
        let (poly, lo, hi) = root.cas_root_parts().expect("root parts");

        assert_eq!(poly.to_string(), "_^2 - 2");
        assert_eq!(lo, 1.0);
        assert_eq!(hi, 2.0);
    }

    #[test]
    fn root_node_lowers_to_algebraic_data() {
        let root =
            cas_root_expr(&[sqrt2_poly(), Value::Int(1), Value::Int(2)], &[]).expect("root node");
        let value = resolve_cas_root(&root).expect("lowering").expect("value");

        assert!(matches!(value, Value::Algebraic(_)));
        assert_eq!(value.to_string(), "2^(1/2)");
    }
}
