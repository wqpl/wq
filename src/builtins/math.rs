use crate::{
    builtins::{BuiltinEnum, wqerror_helper::check_arity},
    value::{Value, WqResult},
    vm::Vm,
    wqerror::{WqError, WqErrorType},
};

use rand::RngExt;

macro_rules! def_unary_math_fn {
    ($fn_name:ident, $enum_variant:ident, $method:ident) => {
        pub fn $fn_name(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
            check_arity(BuiltinEnum::$enum_variant, [1], args)?;
            args[0]
                .$method()
                .map_err(|e| e.into_wqerror().src(BuiltinEnum::$enum_variant))
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
def_unary_math_fn!(floor, Floor, floor);
def_unary_math_fn!(ceil, Ceil, ceil);
def_unary_math_fn!(round, Round, round);
def_unary_math_fn!(sin, Sin, sin);
def_unary_math_fn!(cos, Cos, cos);
def_unary_math_fn!(tan, Tan, tan);
def_unary_math_fn!(arcsin, Arcsin, arcsin);
def_unary_math_fn!(arccos, Arccos, arccos);
def_unary_math_fn!(arctan, Arctan, arctan);
def_unary_math_fn!(sinh, Sinh, sinh);
def_unary_math_fn!(cosh, Cosh, cosh);
def_unary_math_fn!(tanh, Tanh, tanh);
def_unary_math_fn!(arcsinh, Arcsinh, arcsinh);
def_unary_math_fn!(arccosh, Arccosh, arccosh);
def_unary_math_fn!(arctanh, Arctanh, arctanh);

macro_rules! def_binary_math_fn {
    ($fn_name:ident, $enum_variant:ident, $method:ident) => {
        pub fn $fn_name(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
            check_arity(BuiltinEnum::$enum_variant, [2], args)?;
            args[0]
                .$method(&args[1])
                .map_err(|e| e.into_wqerror().src(BuiltinEnum::$enum_variant))
        }
    };
}

def_binary_math_fn!(log, Log, log);
def_binary_math_fn!(arctan2, Arctan2, arctan2);

pub fn rand(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Rand, [0, 1, 2], args)?;
    let mut rng = rand::rng();
    match args.len() {
        0 => Ok(Value::Float(rng.random())),
        1 => match &args[0] {
            Value::Int(n) if *n > 0 => Ok(Value::Int(rng.random_range(0..*n))),
            Value::Float(f) if *f > 0.0 => Ok(Value::Float(rng.random_range(0.0..*f))),
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
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BuiltinEnum::Rand)
                            .msg("expected positive int or float")
                            .at_arg(0));
                    }
                };
                let bf = match b {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BuiltinEnum::Rand)
                            .msg("expected positive int or float")
                            .at_arg(1));
                    }
                };
                if af < bf {
                    Ok(Value::Float(rng.random_range(af..bf)))
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
