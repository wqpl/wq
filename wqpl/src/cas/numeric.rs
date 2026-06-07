use num_traits::{One, Signed, Zero};

use crate::value::cas::{CasConst, CasFunction, CasOp};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NumericOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

impl NumericOp {
    fn from_symbol(op: &str) -> Option<Self> {
        match op {
            "+" => Some(Self::Add),
            "-" => Some(Self::Subtract),
            "*" => Some(Self::Multiply),
            "/" => Some(Self::Divide),
            "^" => Some(Self::Power),
            _ => None,
        }
    }

    fn eval(self, lhs: &Value, rhs: &Value) -> WqResult<Value> {
        let res = match self {
            Self::Add => lhs.add(rhs),
            Self::Subtract => lhs.subtract(rhs),
            Self::Multiply => lhs.multiply(rhs),
            Self::Divide => lhs.divide_dot(rhs),
            Self::Power => lhs.power_dot(rhs),
        };
        res.map_err(|e| e.src("cas"))
    }
}

pub(crate) fn eval_numeric_binary(op: &str, lhs: &Value, rhs: &Value) -> WqResult<Value> {
    let Some(op) = NumericOp::from_symbol(op) else {
        return Err(cas_err(format!("unsupported symbolic operator '{op}'")));
    };
    eval_numeric_op(op, lhs, rhs)
}

pub(crate) fn eval_numeric_op(op: NumericOp, lhs: &Value, rhs: &Value) -> WqResult<Value> {
    op.eval(lhs, rhs)
}

pub(crate) fn numeric_add(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    eval_numeric_op(NumericOp::Add, lhs, rhs)
}

pub(crate) fn numeric_sub(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    eval_numeric_op(NumericOp::Subtract, lhs, rhs)
}

pub(crate) fn numeric_mul(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    eval_numeric_op(NumericOp::Multiply, lhs, rhs)
}

pub(crate) fn numeric_div(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    eval_numeric_op(NumericOp::Divide, lhs, rhs)
}

pub(crate) fn numeric_pow(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    eval_numeric_op(NumericOp::Power, lhs, rhs)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NumericCallMode {
    Simplify,
    Evaluate,
}

/// Try to resolve CAS constants (pi, e, oo, _oo) to numeric values and
/// evaluate the call.  Returns `None` if any arg is a non-constant CAS node
/// or an unresolved variable.
pub(super) fn try_eval_with_const_resolve(
    function: CasFunction,
    args: &[Value],
) -> WqResult<Option<Value>> {
    let mut numeric_args = Vec::with_capacity(args.len());
    for arg in args {
        if !arg.is_cas_expr() {
            numeric_args.push(arg.clone());
        } else if let Some(konst) = arg.cas_const() {
            numeric_args.push(match konst {
                CasConst::Pi => Value::float(std::f64::consts::PI),
                CasConst::E => Value::float(std::f64::consts::E),
                // oo/_oo: functions like sin(∞) are undefined, skip
                _ => return Ok(None),
            });
        } else {
            // Variable, operator, or other CAS node — can't resolve
            return Ok(None);
        }
    }
    eval_numeric_call(function, &numeric_args)
}

pub(super) fn eval_numeric_call(function: CasFunction, args: &[Value]) -> WqResult<Option<Value>> {
    eval_numeric_call_with_mode(function, args, NumericCallMode::Simplify)
}

fn eval_numeric_call_with_mode(
    function: CasFunction,
    args: &[Value],
    mode: NumericCallMode,
) -> WqResult<Option<Value>> {
    if mode == NumericCallMode::Simplify {
        if args.iter().any(|a| a.is_algebraic_number()) {
            return Ok(None);
        }
        match (function, args) {
            (CasFunction::Exp, [_arg]) => {
                // Never evaluate exp numerically during simplification:
                // conversion to e / e^n is handled by the Call simplifier.
                return Ok(None);
            }
            (CasFunction::Ln, [_arg]) => {
                // Keep ln symbolic; ln(1) and ln(e) are handled by the Call
                // simplifier, while most other integer logs are transcendental.
                return Ok(None);
            }
            _ => {}
        }
    }

    let value = match (function, args) {
        (CasFunction::Abs, [arg]) => Some(arg.abs().map_err(|e| e.src("cas"))?),
        (CasFunction::Sgn, [arg]) => Some(arg.sgn().map_err(|e| e.src("cas"))?),
        (CasFunction::Sin, [arg]) => Some(arg.sin().map_err(|e| e.src("cas"))?),
        (CasFunction::Cos, [arg]) => Some(arg.cos().map_err(|e| e.src("cas"))?),
        (CasFunction::Tan, [arg]) => Some(arg.tan().map_err(|e| e.src("cas"))?),
        (CasFunction::Sec, [arg]) => Some(arg.sec().map_err(|e| e.src("cas"))?),
        (CasFunction::Csc, [arg]) => Some(arg.csc().map_err(|e| e.src("cas"))?),
        (CasFunction::Cot, [arg]) => Some(arg.cot().map_err(|e| e.src("cas"))?),
        (CasFunction::Erf, [arg]) => Some(arg.erf().map_err(|e| e.src("cas"))?),
        (CasFunction::Erfc, [arg]) => Some(arg.erfc().map_err(|e| e.src("cas"))?),
        (CasFunction::Gamma, [arg]) => Some(arg.gamma().map_err(|e| e.src("cas"))?),
        (CasFunction::LnGamma, [arg]) => Some(arg.lngamma().map_err(|e| e.src("cas"))?),
        (CasFunction::Si, [arg]) => Some(arg.si().map_err(|e| e.src("cas"))?),
        (CasFunction::Ci, [arg]) => Some(arg.ci().map_err(|e| e.src("cas"))?),
        (CasFunction::Ei, [arg]) => Some(arg.ei().map_err(|e| e.src("cas"))?),
        (CasFunction::En, [n, x]) => Some(n.en(x).map_err(|e| e.src("cas"))?),
        (CasFunction::EllPk, [arg]) => Some(arg.ellpk().map_err(|e| e.src("cas"))?),
        (CasFunction::EllPe, [arg]) => Some(arg.ellpe().map_err(|e| e.src("cas"))?),
        (CasFunction::EllIk, [phi, m]) => Some(phi.ellik(m).map_err(|e| e.src("cas"))?),
        (CasFunction::EllIe, [phi, m]) => Some(phi.ellie(m).map_err(|e| e.src("cas"))?),
        (CasFunction::Heaviside, [arg]) => Some(arg.heaviside().map_err(|e| e.src("cas"))?),
        (CasFunction::Delta, [arg]) => {
            if let Some(f) = arg.as_f64() {
                if f == 0.0 {
                    return Err(cas_err("Dirac delta is singular at zero"));
                }
                Some(Value::float(0.0))
            } else {
                None
            }
        }
        (CasFunction::Exp, [arg]) => Some(arg.exp().map_err(|e| e.src("cas"))?),
        (CasFunction::Ln, [arg]) => Some(arg.ln().map_err(|e| e.src("cas"))?),
        (CasFunction::Log2, [arg]) => Some(arg.log2().map_err(|e| e.src("cas"))?),
        (CasFunction::Log10, [arg]) => Some(arg.log10().map_err(|e| e.src("cas"))?),
        (CasFunction::Sqrt, [arg]) => Some(arg.sqrt().map_err(|e| e.src("cas"))?),
        (CasFunction::ArcSin, [arg]) => Some(arg.arcsin().map_err(|e| e.src("cas"))?),
        (CasFunction::ArcCos, [arg]) => Some(arg.arccos().map_err(|e| e.src("cas"))?),
        (CasFunction::ArcTan, [arg]) => Some(arg.arctan().map_err(|e| e.src("cas"))?),
        (CasFunction::Sinh, [arg]) => Some(arg.sinh().map_err(|e| e.src("cas"))?),
        (CasFunction::Cosh, [arg]) => Some(arg.cosh().map_err(|e| e.src("cas"))?),
        (CasFunction::Tanh, [arg]) => Some(arg.tanh().map_err(|e| e.src("cas"))?),
        (CasFunction::ArcSinh, [arg]) => Some(arg.arcsinh().map_err(|e| e.src("cas"))?),
        (CasFunction::ArcCosh, [arg]) => Some(arg.arccosh().map_err(|e| e.src("cas"))?),
        (CasFunction::ArcTanh, [arg]) => Some(arg.arctanh().map_err(|e| e.src("cas"))?),
        (CasFunction::Floor, [arg]) => Some(arg.floor().map_err(|e| e.src("cas"))?),
        (CasFunction::Ceil, [arg]) => Some(arg.ceil().map_err(|e| e.src("cas"))?),
        (CasFunction::Round, [arg]) => Some(arg.round().map_err(|e| e.src("cas"))?),
        (CasFunction::Log, [lhs, rhs]) => Some(lhs.log(rhs).map_err(|e| e.src("cas"))?),
        (CasFunction::ArcTan2, [lhs, rhs]) => Some(lhs.arctan2(rhs).map_err(|e| e.src("cas"))?),
        _ => None,
    };
    Ok(value)
}

pub(crate) fn eval_exact_numeric_div(lhs: &Value, rhs: &Value) -> WqResult<Value> {
    let Some((lhs_n, lhs_d)) = lhs.rational_parts() else {
        return numeric_div(lhs, rhs);
    };
    let Some((rhs_n, rhs_d)) = rhs.rational_parts() else {
        return numeric_div(lhs, rhs);
    };
    if rhs_n.is_zero() {
        return numeric_div(lhs, rhs);
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

fn eval_numeric_coeff_as_f64(coeff: &Value) -> WqResult<f64> {
    if let Some(value) = coeff.as_f64() {
        Ok(value)
    } else {
        f64_or_err(&eval_numeric_cas(coeff)?, coeff)
    }
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
            let cf = eval_numeric_coeff_as_f64(c)?;
            result += cf * alpha_pow;
            alpha_pow *= alpha;
        }
        return Ok(Value::float(result));
    }

    // Symbolic constants.
    if let Some(konst) = expr.cas_const() {
        return match konst {
            CasConst::E => Ok(Value::float(std::f64::consts::E)),
            CasConst::Pi => Ok(Value::float(std::f64::consts::PI)),
            CasConst::Infinity => Ok(Value::float(f64::INFINITY)),
            CasConst::NegInfinity => Ok(Value::float(f64::NEG_INFINITY)),
            CasConst::Undefined => Err(cas_err(
                "unknown symbolic constant 'undef' in numeric evaluation",
            )
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
            (CasOp::Add, args) => {
                let mut acc = 0.0;
                for arg in args {
                    acc += f64_or_err(&eval_numeric_cas(arg)?, expr)?;
                }
                Ok(Value::float(acc))
            }
            (CasOp::Multiply, args) => {
                let mut acc = 1.0;
                for arg in args {
                    acc *= f64_or_err(&eval_numeric_cas(arg)?, expr)?;
                }
                Ok(Value::float(acc))
            }
            (CasOp::Subtract, [arg]) => {
                let v = f64_or_err(&eval_numeric_cas(arg)?, expr)?;
                Ok(Value::float(-v))
            }
            (CasOp::Subtract, [lhs, rhs]) => {
                let l = f64_or_err(&eval_numeric_cas(lhs)?, expr)?;
                let r = f64_or_err(&eval_numeric_cas(rhs)?, expr)?;
                Ok(Value::float(l - r))
            }
            (CasOp::Divide, [lhs, rhs]) => {
                let l = f64_or_err(&eval_numeric_cas(lhs)?, expr)?;
                let r = f64_or_err(&eval_numeric_cas(rhs)?, expr)?;
                if r == 0.0 {
                    return Err(cas_err("division by zero in numeric evaluation"));
                }
                Ok(Value::float(l / r))
            }
            (CasOp::Power, [base, exp]) => {
                let b = f64_or_err(&eval_numeric_cas(base)?, expr)?;
                let e = f64_or_err(&eval_numeric_cas(exp)?, expr)?;
                Ok(Value::float(b.powf(e)))
            }
            _ => Err(cas_err(format!(
                "unsupported operator '{}' in numeric evaluation",
                op.symbol()
            ))
            .got1(expr)),
        }
    } else if let Some((function, args)) = expr.cas_call_parts() {
        // Recursively evaluate arguments.
        let mut vals = Vec::with_capacity(args.len());
        for arg in args {
            vals.push(eval_numeric_cas(arg)?);
        }
        eval_numeric_call_with_mode(function, vals.as_slice(), NumericCallMode::Evaluate)?
            .ok_or_else(|| {
                cas_err(format!(
                    "unsupported function '{}' in numeric evaluation",
                    function.name()
                ))
                .got1(expr)
            })
    } else {
        Err(cas_err("expected symbolic expression for numeric evaluation").got1(expr))
    }
}
