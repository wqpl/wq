use rand::RngExt;

use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::{Excerpt, Value, WqResult};
use crate::vm::Vm;
use crate::wqerror::{WqError, WqErrorType};

macro_rules! def_unary_math_fn {
    ($fn_name:ident, $enum_variant:ident, $method:ident) => {
        pub(super) fn $fn_name(args: BuiltinFnArgs) -> WqResult<Value> {
            check_arity(BuiltinEnum::$enum_variant, [1], &args)?;
            args[0]
                .$method()
                .map_err(|e| e.src(BuiltinEnum::$enum_variant))
        }
    };
}

def_unary_math_fn!(neg, Neg, neg);
def_unary_math_fn!(abs, Abs, abs);
def_unary_math_fn!(sgn, Sgn, sgn);
def_unary_math_fn!(sqrt, Sqrt, sqrt);
def_unary_math_fn!(exp, Exp, exp);
def_unary_math_fn!(ln, Ln, ln);
def_unary_math_fn!(log2, Log2, log2);
def_unary_math_fn!(log10, Log10, log10);
def_unary_math_fn!(sin, Sin, sin);
def_unary_math_fn!(cos, Cos, cos);
def_unary_math_fn!(tan, Tan, tan);
def_unary_math_fn!(sec, Sec, sec);
def_unary_math_fn!(csc, Csc, csc);
def_unary_math_fn!(cot, Cot, cot);
def_unary_math_fn!(arcsin, Arcsin, arcsin);
def_unary_math_fn!(arccos, Arccos, arccos);
def_unary_math_fn!(arctan, Arctan, arctan);
def_unary_math_fn!(sinh, Sinh, sinh);
def_unary_math_fn!(cosh, Cosh, cosh);
def_unary_math_fn!(tanh, Tanh, tanh);
def_unary_math_fn!(arcsinh, Arcsinh, arcsinh);
def_unary_math_fn!(arccosh, Arccosh, arccosh);
def_unary_math_fn!(arctanh, Arctanh, arctanh);
def_unary_math_fn!(erf, Erf, erf);
def_unary_math_fn!(erfc, Erfc, erfc);
def_unary_math_fn!(gamma, Gamma, gamma);
def_unary_math_fn!(lngamma, Lngamma, lngamma);
def_unary_math_fn!(heaviside, Heaviside, heaviside);
def_unary_math_fn!(si, Si, si);
def_unary_math_fn!(ci, Ci, ci);
def_unary_math_fn!(ei, Ei, ei);

pub(super) fn en(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::En, [2], &args)?;
    args[0].en(&args[1]).map_err(|e| e.src(BuiltinEnum::En))
}

def_unary_math_fn!(ellpk, Ellpk, ellpk);
def_unary_math_fn!(ellpe, Ellpe, ellpe);

pub(super) fn delta(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Delta, [1], &args)?;
    let input = args[0].as_f64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Delta)
            .msg("expected a number")
            .got1(&args[0])
    })?;
    if input == 0.0 {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Delta)
            .msg("Dirac delta is singular at zero"));
    }
    Ok(Value::float(0.0))
}

macro_rules! def_binary_math_fn {
    ($fn_name:ident, $enum_variant:ident, $method:ident) => {
        pub(super) fn $fn_name(args: BuiltinFnArgs) -> WqResult<Value> {
            check_arity(BuiltinEnum::$enum_variant, [2], &args)?;
            args[0]
                .$method(&args[1])
                .map_err(|e| e.src(BuiltinEnum::$enum_variant))
        }
    };
}

def_binary_math_fn!(log, Log, log);
def_binary_math_fn!(arctan2, Arctan2, arctan2);
def_binary_math_fn!(ellik, Ellik, ellik);
def_binary_math_fn!(ellie, Ellie, ellie);

macro_rules! def_rounding_math_fn {
    ($fn_name:ident, $enum_variant:ident, $method:ident, $f64_method:ident) => {
        pub(super) fn $fn_name(args: BuiltinFnArgs) -> WqResult<Value> {
            check_arity(BuiltinEnum::$enum_variant, [1, 2], &args)?;
            let val_arg = &args[0];

            let dec_count = if args.len() == 1 {
                0
            } else {
                match &args[1] {
                    Value::Int(n) => *n,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BuiltinEnum::$enum_variant)
                            .msg("expected int for decimal count")
                            .at_arg(1));
                    }
                }
            };

            if dec_count == 0 {
                val_arg
                    .$method()
                    .map_err(|e| e.src(BuiltinEnum::$enum_variant))
            } else {
                let val = match val_arg.as_f64() {
                    Some(v) => v,
                    None => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BuiltinEnum::$enum_variant)
                            .msg("expected real")
                            .at_arg(0));
                    }
                };
                if val.is_nan() {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BuiltinEnum::$enum_variant)
                        .msg(format!(
                            "{} is not defined for given value",
                            stringify!($fn_name)
                        ))
                        .attach_note("builtin math functions are defined on real set")
                        .attach_note(format!("got {}", val_arg.excerpt())));
                }
                let factor = 10_f64.powi(dec_count as i32);
                let res = (val * factor).$f64_method() / factor;
                Ok(Value::float(res))
            }
        }
    };
}

def_rounding_math_fn!(floor, Floor, floor, floor);
def_rounding_math_fn!(ceil, Ceil, ceil, ceil);
def_rounding_math_fn!(round, Round, round, round);

pub(super) fn rand(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Rand, [0, 1, 2], &args)?;
    let mut rng = rand::rng();
    match args.len() {
        0 => Ok(Value::float(rng.random::<f64>())),
        1 => match &args[0] {
            Value::Int(n) if *n > 0 => Ok(Value::Int(rng.random_range(0..*n))),
            Value::Float(f) if **f > 0.0 => Ok(Value::float(rng.random_range(0.0..**f))),
            _ => Err(WqError::new(WqErrorType::Domain)
                .src(BuiltinEnum::Rand)
                .msg("expected positive int or float")
                .at_arg(0)),
        },
        2 => match (&args[0], &args[1]) {
            (Value::Int(a), Value::Int(b)) if a < b => Ok(Value::Int(rng.random_range(*a..*b))),
            (a, b) => {
                let af = match a {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => **f,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BuiltinEnum::Rand)
                            .msg("expected positive int or float")
                            .at_arg(0));
                    }
                };
                let bf = match b {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => **f,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BuiltinEnum::Rand)
                            .msg("expected positive int or float")
                            .at_arg(1));
                    }
                };
                if af < bf {
                    Ok(Value::float(rng.random_range(af..bf)))
                } else {
                    Err(WqError::new(WqErrorType::Domain)
                        .src(BuiltinEnum::Rand)
                        .msg("expected lower < upper")
                        .attach_note(format!("got {af} for lower"))
                        .attach_note(format!("got {bf} for upper")))
                }
            }
        },
        _ => unreachable!(),
    }
}
