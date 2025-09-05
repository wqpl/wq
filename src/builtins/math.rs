use rand::Rng;

use super::arity_error;
use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

macro_rules! def_math_fn {
    // unary
    ($name:ident => |$a:ident| $body:expr) => {
        pub fn $name(args: &[Value]) -> WqResult<Value> {
            if args.len() != 1 {
                return Err(arity_error(stringify!($name), "1", args.len()));
            }
            let $a = &args[0];
            $body
        }
    };
    // binary
    ($name:ident => |$a:ident, $b:ident| $body:expr) => {
        pub fn $name(args: &[Value]) -> WqResult<Value> {
            if args.len() != 2 {
                return Err(arity_error(stringify!($name), "2", args.len()));
            }
            let ($a, $b) = (&args[0], &args[1]);
            $body
        }
    };
}

// Shorthand to define many unary "method-forwards" at once
macro_rules! def_unary_math_fns {
    ($($name:ident),+ $(,)?) => {
        $( def_math_fn!($name => |x| x.$name()); )+
    }
}

def_unary_math_fns!(
    neg, abs, sgn, sqrt, exp, ln, floor, ceil, round, sin, cos, tan, arcsin, arccos, arctan, sinh,
    cosh, tanh, arcsinh, arccosh, arctanh
);

// Example of a binary like atan2 (y, x)
def_math_fn!(log => |y, x| y.log(x));

pub fn rand(args: &[Value]) -> WqResult<Value> {
    let mut rng = rand::rng();
    match args.len() {
        0 => Ok(Value::Float(rng.random())),
        1 => match &args[0] {
            Value::Int(n) if *n > 0 => Ok(Value::Int(rng.random_range(0..*n))),
            Value::Float(f) if *f > 0.0 => Ok(Value::Float(rng.random_range(0.0..*f))),
            Value::Int(_) | Value::Float(_) => Err(WqError::DomainError(format!(
                "`rand`: expected a positive number, got {}",
                args[0].type_name()
            ))),
            _ => Err(WqError::DomainError(format!(
                "`rand`: expected numbers, got {}",
                args[0].type_name()
            ))),
        },
        2 => match (&args[0], &args[1]) {
            (Value::Int(a), Value::Int(b)) if a < b => Ok(Value::Int(rng.random_range(*a..*b))),
            (a, b) => {
                let af = match a {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::DomainError(format!(
                            "`rand`: expected numbers, got {}",
                            a.type_name()
                        )));
                    }
                };
                let bf = match b {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::DomainError(format!(
                            "`rand`: expected numbers, got {}",
                            b.type_name()
                        )));
                    }
                };
                if af < bf {
                    Ok(Value::Float(rng.random_range(af..bf)))
                } else {
                    Err(WqError::DomainError(format!(
                        "`rand`: expected 'lower<upper', got '{af}>={bf}'"
                    )))
                }
            }
        },
        other => Err(arity_error("rand", "0, 1 or 2", other)),
    }
}
