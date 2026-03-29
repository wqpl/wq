use num_traits::{One, Signed, Zero};

use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) fn cas_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Domain).msg(msg.into())
}

pub(crate) fn numeric_is_zero(value: &Value) -> bool {
    match value {
        Value::Int(0) => true,
        Value::BigInt(n) => n.is_zero(),
        Value::Float(f) => **f == 0.0,
        Value::Algebraic(a) => a.is_zero(),
        _ => value.rational_parts().is_some_and(|(n, _)| n.is_zero()),
    }
}

pub(crate) fn numeric_is_one(value: &Value) -> bool {
    match value {
        Value::Int(1) => true,
        Value::BigInt(n) => n.is_one(),
        Value::Float(f) => **f == 1.0,
        Value::Algebraic(a) => a.is_one(),
        _ => value
            .rational_parts()
            .is_some_and(|(n, d)| n.is_one() && d.is_one()),
    }
}

pub(crate) fn numeric_is_negative(value: &Value) -> bool {
    match value {
        Value::Int(n) => *n < 0,
        Value::BigInt(n) => n.is_negative(),
        Value::Float(f) => **f < 0.0,
        Value::Algebraic(a) => a.is_negative(),
        _ => value.rational_parts().is_some_and(|(n, _)| n.is_negative()),
    }
}

pub(super) fn numeric_abs(value: &Value) -> Value {
    if numeric_is_negative(value) {
        value.neg().expect("numeric absolute value should succeed")
    } else {
        value.clone()
    }
}

pub(super) fn ensure_expr_arg(value: &Value, ctx: &str) -> WqResult<()> {
    if value.is_cas_equation() {
        Err(cas_err(format!("{ctx} expects an expression, got equation")).got1(value))
    } else {
        Ok(())
    }
}

pub(crate) fn eval_numeric_binary(op: &str, lhs: &Value, rhs: &Value) -> WqResult<Value> {
    let res = match op {
        "+" => lhs.add(rhs),
        "-" => lhs.subtract(rhs),
        "*" => lhs.multiply(rhs),
        "/" => lhs.divide_dot(rhs),
        "^" => lhs.power_dot(rhs),
        _ => return Err(cas_err(format!("unsupported symbolic operator '{op}'"))),
    };
    res.map_err(|e| e.src("cas"))
}

/// Try to resolve CAS constants (pi, e, oo, _oo) to numeric values and
/// evaluate the call.  Returns `None` if any arg is a non-constant CAS node
/// or an unresolved variable.
pub(super) fn try_eval_with_const_resolve(name: &str, args: &[Value]) -> WqResult<Option<Value>> {
    let mut numeric_args = Vec::with_capacity(args.len());
    for arg in args {
        if !arg.is_cas_expr() {
            numeric_args.push(arg.clone());
        } else if let Some(const_name) = arg.cas_const_name() {
            numeric_args.push(match const_name {
                "pi" => Value::float(std::f64::consts::PI),
                "e" => Value::float(std::f64::consts::E),
                // oo/_oo: functions like sin(∞) are undefined, skip
                _ => return Ok(None),
            });
        } else {
            // Variable, operator, or other CAS node — can't resolve
            return Ok(None);
        }
    }
    eval_numeric_call(name, &numeric_args)
}

pub(super) fn eval_numeric_call(name: &str, args: &[Value]) -> WqResult<Option<Value>> {
    // Algebraic values cannot be numerically evaluated — keep symbolic
    if args.iter().any(|a| a.is_algebraic_number()) {
        return Ok(None);
    }
    let value = match (name, args) {
        ("abs", [arg]) => Some(arg.abs().map_err(|e| e.src("cas"))?),
        ("sgn", [arg]) => Some(arg.sgn().map_err(|e| e.src("cas"))?),
        ("sin", [arg]) => Some(arg.sin().map_err(|e| e.src("cas"))?),
        ("cos", [arg]) => Some(arg.cos().map_err(|e| e.src("cas"))?),
        ("tan", [arg]) => Some(arg.tan().map_err(|e| e.src("cas"))?),
        ("sec", [arg]) => Some(arg.sec().map_err(|e| e.src("cas"))?),
        ("csc", [arg]) => Some(arg.csc().map_err(|e| e.src("cas"))?),
        ("cot", [arg]) => Some(arg.cot().map_err(|e| e.src("cas"))?),
        ("erf", [arg]) => Some(arg.erf().map_err(|e| e.src("cas"))?),
        ("erfc", [arg]) => Some(arg.erfc().map_err(|e| e.src("cas"))?),
        ("gamma", [arg]) => Some(arg.gamma().map_err(|e| e.src("cas"))?),
        ("lngamma", [arg]) => Some(arg.lngamma().map_err(|e| e.src("cas"))?),
        ("si", [arg]) => Some(arg.si().map_err(|e| e.src("cas"))?),
        ("ci", [arg]) => Some(arg.ci().map_err(|e| e.src("cas"))?),
        ("ei", [arg]) => Some(arg.ei().map_err(|e| e.src("cas"))?),
        ("en", [n, x]) => Some(n.en(x).map_err(|e| e.src("cas"))?),
        ("ellpk", [arg]) => Some(arg.ellpk().map_err(|e| e.src("cas"))?),
        ("ellpe", [arg]) => Some(arg.ellpe().map_err(|e| e.src("cas"))?),
        ("ellik", [phi, m]) => Some(phi.ellik(m).map_err(|e| e.src("cas"))?),
        ("ellie", [phi, m]) => Some(phi.ellie(m).map_err(|e| e.src("cas"))?),
        ("heaviside", [arg]) => Some(arg.heaviside().map_err(|e| e.src("cas"))?),
        ("delta", [arg]) => {
            if let Some(f) = arg.as_f64() {
                if f == 0.0 {
                    return Err(cas_err("Dirac delta is singular at zero"));
                }
                Some(Value::float(0.0))
            } else {
                None
            }
        }
        ("exp", [_arg]) => {
            // Never evaluate exp numerically during simplification —
            // conversion to e / e^n is handled by the Call simplifier.
            return Ok(None);
        }
        ("ln", [_arg]) => {
            // Keep ln symbolic — ln(n) is transcendental for n ≠ 1.
            // ln(1)→0 and ln(e)→1 are handled by the Call simplifier.
            return Ok(None);
        }
        ("log2", [arg]) => Some(arg.log2().map_err(|e| e.src("cas"))?),
        ("log10", [arg]) => Some(arg.log10().map_err(|e| e.src("cas"))?),
        ("sqrt", [arg]) => Some(arg.sqrt().map_err(|e| e.src("cas"))?),
        ("arcsin", [arg]) => Some(arg.arcsin().map_err(|e| e.src("cas"))?),
        ("arccos", [arg]) => Some(arg.arccos().map_err(|e| e.src("cas"))?),
        ("arctan", [arg]) => Some(arg.arctan().map_err(|e| e.src("cas"))?),
        ("sinh", [arg]) => Some(arg.sinh().map_err(|e| e.src("cas"))?),
        ("cosh", [arg]) => Some(arg.cosh().map_err(|e| e.src("cas"))?),
        ("tanh", [arg]) => Some(arg.tanh().map_err(|e| e.src("cas"))?),
        ("arcsinh", [arg]) => Some(arg.arcsinh().map_err(|e| e.src("cas"))?),
        ("arccosh", [arg]) => Some(arg.arccosh().map_err(|e| e.src("cas"))?),
        ("arctanh", [arg]) => Some(arg.arctanh().map_err(|e| e.src("cas"))?),
        ("floor", [arg]) => Some(arg.floor().map_err(|e| e.src("cas"))?),
        ("ceil", [arg]) => Some(arg.ceil().map_err(|e| e.src("cas"))?),
        ("round", [arg]) => Some(arg.round().map_err(|e| e.src("cas"))?),
        ("log", [lhs, rhs]) => Some(lhs.log(rhs).map_err(|e| e.src("cas"))?),
        ("arctan2", [lhs, rhs]) => Some(lhs.arctan2(rhs).map_err(|e| e.src("cas"))?),
        _ => None,
    };
    Ok(value)
}

pub(crate) fn eval_exact_numeric_div(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    let Some((lhs_n, lhs_d)) = lhs.rational_parts() else {
        return eval_numeric_binary("/", lhs, rhs);
    };
    let Some((rhs_n, rhs_d)) = rhs.rational_parts() else {
        return eval_numeric_binary("/", lhs, rhs);
    };
    if rhs_n.is_zero() {
        return eval_numeric_binary("/", lhs, rhs);
    }
    let value = Value::from_fraction_parts(lhs_n * rhs_d, lhs_d * rhs_n);
    if let Some((numer, denom)) = value.rational_parts()
        && denom.is_one()
    {
        Ok(Value::from_bigint(numer))
    } else {
        Ok(value)
    }
}

/// Extract f64 from a numeric Value, or return a CAS error contextualised
/// with the original expression.
fn f64_or_err(v: &Value, orig: &Value) -> WqResult<f64> {
    v.as_f64()
        .ok_or_else(|| cas_err("expected numeric result in CAS evaluation").got1(orig))
}

/// Evaluate a CAS expression to a single Float, approximating Algebraic
/// values by the midpoint of their isolating interval.
///
/// Returns an error if the expression still contains symbolic variables.
pub(crate) fn eval_numeric_cas(expr: &Value) -> WqResult<Value> {
    // Fast path: already a plain number.
    if let Some(f) = expr.as_f64() {
        return Ok(Value::float(f));
    }

    // Algebraic numbers → evaluate using coefficients and generator's approx.
    if let Value::Algebraic(a) = expr {
        let alpha = (a.interval.0 + a.interval.1) * 0.5;
        let mut result = 0.0f64;
        let mut alpha_pow = 1.0f64;
        for c in a.coeffs.iter() {
            let cf = c.as_f64().unwrap_or_else(|| {
                eval_numeric_cas(c)
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0)
            });
            result += cf * alpha_pow;
            alpha_pow *= alpha;
        }
        return Ok(Value::float(result));
    }

    // Symbolic constants.
    if let Some(name) = expr.cas_const_name() {
        return match name {
            "e" => Ok(Value::float(std::f64::consts::E)),
            "pi" => Ok(Value::float(std::f64::consts::PI)),
            "∞" | "oo" => Ok(Value::float(f64::INFINITY)),
            "-∞" | "-oo" | "_oo" => Ok(Value::float(f64::NEG_INFINITY)),
            _ => Err(cas_err(format!(
                "unknown symbolic constant '{name}' in numeric evaluation"
            ))
            .got1(expr)),
        };
    }

    // Variables are not allowed.
    if expr.cas_var_name().is_some() {
        return Err(
            cas_err("cannot evaluate symbolic expression to numeric: contains variable").got1(expr),
        );
    }

    // Operators.
    if let Some((op, args)) = expr.cas_op_parts() {
        match (op, args) {
            ("+", args) => {
                let mut acc = 0.0;
                for arg in args {
                    acc += f64_or_err(&eval_numeric_cas(arg)?, expr)?;
                }
                Ok(Value::float(acc))
            }
            ("*", args) => {
                let mut acc = 1.0;
                for arg in args {
                    acc *= f64_or_err(&eval_numeric_cas(arg)?, expr)?;
                }
                Ok(Value::float(acc))
            }
            ("-", [arg]) => {
                let v = f64_or_err(&eval_numeric_cas(arg)?, expr)?;
                Ok(Value::float(-v))
            }
            ("-", [lhs, rhs]) => {
                let l = f64_or_err(&eval_numeric_cas(lhs)?, expr)?;
                let r = f64_or_err(&eval_numeric_cas(rhs)?, expr)?;
                Ok(Value::float(l - r))
            }
            ("/", [lhs, rhs]) => {
                let l = f64_or_err(&eval_numeric_cas(lhs)?, expr)?;
                let r = f64_or_err(&eval_numeric_cas(rhs)?, expr)?;
                if r == 0.0 {
                    return Err(cas_err("division by zero in numeric evaluation"));
                }
                Ok(Value::float(l / r))
            }
            ("^", [base, exp]) => {
                let b = f64_or_err(&eval_numeric_cas(base)?, expr)?;
                let e = f64_or_err(&eval_numeric_cas(exp)?, expr)?;
                Ok(Value::float(b.powf(e)))
            }
            _ => Err(
                cas_err(format!("unsupported operator '{op}' in numeric evaluation")).got1(expr),
            ),
        }
    } else if let Some((name, args)) = expr.cas_call_parts() {
        // Recursively evaluate arguments.
        let mut vals = Vec::with_capacity(args.len());
        for arg in args {
            vals.push(eval_numeric_cas(arg)?);
        }
        // Dispatch to Value's numeric methods.
        let result = match (name, vals.as_slice()) {
            ("abs", [a]) => a.abs(),
            ("sgn", [a]) => a.sgn(),
            ("sin", [a]) => a.sin(),
            ("cos", [a]) => a.cos(),
            ("tan", [a]) => a.tan(),
            ("sec", [a]) => a.sec(),
            ("csc", [a]) => a.csc(),
            ("cot", [a]) => a.cot(),
            ("sinh", [a]) => a.sinh(),
            ("cosh", [a]) => a.cosh(),
            ("tanh", [a]) => a.tanh(),
            ("arcsin", [a]) => a.arcsin(),
            ("arccos", [a]) => a.arccos(),
            ("arctan", [a]) => a.arctan(),
            ("arcsinh", [a]) => a.arcsinh(),
            ("arccosh", [a]) => a.arccosh(),
            ("arctanh", [a]) => a.arctanh(),
            ("exp", [a]) => a.exp(),
            ("ln", [a]) => a.ln(),
            ("log2", [a]) => a.log2(),
            ("log10", [a]) => a.log10(),
            ("sqrt", [a]) => a.sqrt(),
            ("erf", [a]) => a.erf(),
            ("erfc", [a]) => a.erfc(),
            ("gamma", [a]) => a.gamma(),
            ("lngamma", [a]) => a.lngamma(),
            ("si", [a]) => a.si(),
            ("ci", [a]) => a.ci(),
            ("ei", [a]) => a.ei(),
            ("en", [n, x]) => n.en(x),
            ("ellpk", [a]) => a.ellpk(),
            ("ellpe", [a]) => a.ellpe(),
            ("ellik", [phi, m]) => phi.ellik(m),
            ("ellie", [phi, m]) => phi.ellie(m),
            ("heaviside", [a]) => a.heaviside(),
            ("floor", [a]) => a.floor(),
            ("ceil", [a]) => a.ceil(),
            ("round", [a]) => a.round(),
            _ => {
                return Err(cas_err(format!(
                    "unsupported function '{name}' in numeric evaluation"
                ))
                .got1(expr));
            }
        };
        result.map_err(|e| e.src("cas"))
    } else {
        Err(cas_err("expected symbolic expression for numeric evaluation").got1(expr))
    }
}
