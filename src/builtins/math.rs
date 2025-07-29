use rand::Rng;

use crate::value::valuei::{Value, WqError, WqResult};

pub fn abs(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("abs expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(n.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| abs(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("abs only works on numbers".to_string())),
    }
}

pub fn neg(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("neg expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(-n)),
        Value::Float(f) => Ok(Value::Float(-f)),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| neg(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("neg only works on numbers".to_string())),
    }
}

pub fn signum(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("signum expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(if *n > 0 {
            1
        } else if *n < 0 {
            -1
        } else {
            0
        })),
        Value::Float(f) => Ok(Value::Float(if *f > 0.0 {
            1.0
        } else if *f < 0.0 {
            -1.0
        } else {
            0.0
        })),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| signum(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(
            "signum only works on numbers".to_string(),
        )),
    }
}

pub fn sqrt(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("sqrt expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError("sqrt of negative number".to_string()))
            } else {
                Ok(Value::Float((*n as f64).sqrt()))
            }
        }
        Value::Float(f) => {
            if *f < 0.0 {
                Err(WqError::DomainError("sqrt of negative number".to_string()))
            } else {
                Ok(Value::Float(f.sqrt()))
            }
        }
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| sqrt(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("sqrt only works on numbers".to_string())),
    }
}

pub fn exp(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("exp expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Float((*n as f64).exp())),
        Value::Float(f) => Ok(Value::Float(f.exp())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| exp(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("exp only works on numbers".to_string())),
    }
}

pub fn ln(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("ln expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n <= 0 {
                Err(WqError::DomainError(
                    "ln of non-positive number".to_string(),
                ))
            } else {
                Ok(Value::Float((*n as f64).ln()))
            }
        }
        Value::Float(f) => {
            if *f <= 0.0 {
                Err(WqError::DomainError(
                    "ln of non-positive number".to_string(),
                ))
            } else {
                Ok(Value::Float(f.ln()))
            }
        }
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| ln(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("ln only works on numbers".to_string())),
    }
}

pub fn floor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("floor expects 1 argument".to_string()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(f.floor() as i64)),
        Value::IntList(items) => Ok(Value::IntList(items.clone())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| floor(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(
            "floor only works on numbers".to_string(),
        )),
    }
}

pub fn ceiling(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError(
            "ceiling expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(f.ceil() as i64)),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> =
                items.iter().map(|v| ceiling(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(
            "ceiling only works on numbers".to_string(),
        )),
    }
}

fn rand_dims_int(dims: &[usize], rng: &mut impl Rng, low: i64, high: i64) -> WqResult<Value> {
    match dims.len() {
        0 => Err(WqError::DomainError("shape cannot be an empty list".into())),
        1 => {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(rng.random_range(low..high));
            }
            Ok(Value::IntList(out))
        }
        _ => {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(rand_dims_int(&dims[1..], rng, low, high)?);
            }
            Ok(Value::List(out))
        }
    }
}

fn rand_dims_float(dims: &[usize], rng: &mut impl Rng, low: f64, high: f64) -> WqResult<Value> {
    match dims.len() {
        0 => Err(WqError::DomainError("shape cannot be an empty list".into())),
        1 => {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(Value::Float(rng.random_range(low..high)));
            }
            Ok(Value::List(out))
        }
        _ => {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(rand_dims_float(&dims[1..], rng, low, high)?);
            }
            Ok(Value::List(out))
        }
    }
}

fn rand_shape_int(shape: &Value, rng: &mut impl Rng, low: i64, high: i64) -> WqResult<Value> {
    match shape {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError(
                    "rand length must be non-negative".into(),
                ))
            } else {
                Ok(rand_dims_int(&[*n as usize], rng, low, high)?)
            }
        }
        Value::IntList(dims) => {
            if dims.iter().any(|&d| d < 0) {
                Err(WqError::DomainError(
                    "rand length must be non-negative".into(),
                ))
            } else {
                let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
                Ok(rand_dims_int(&dims, rng, low, high)?)
            }
        }
        Value::List(items) => {
            if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
                let dims: Vec<usize> = items
                    .iter()
                    .map(|v| {
                        if let Value::Int(n) = v {
                            *n as usize
                        } else {
                            unreachable!()
                        }
                    })
                    .collect();
                Ok(rand_dims_int(&dims, rng, low, high)?)
            } else {
                let mut out = Vec::with_capacity(items.len());
                for v in items {
                    out.push(rand_shape_int(v, rng, low, high)?);
                }
                Ok(Value::List(out))
            }
        }
        _ => Err(WqError::TypeError("rand expects an integer shape".into())),
    }
}

fn rand_shape_float(shape: &Value, rng: &mut impl Rng, low: f64, high: f64) -> WqResult<Value> {
    match shape {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError(
                    "rand length must be non-negative".into(),
                ))
            } else {
                Ok(rand_dims_float(&[*n as usize], rng, low, high)?)
            }
        }
        Value::IntList(dims) => {
            if dims.iter().any(|&d| d < 0) {
                Err(WqError::DomainError(
                    "rand length must be non-negative".into(),
                ))
            } else {
                let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
                Ok(rand_dims_float(&dims, rng, low, high)?)
            }
        }
        Value::List(items) => {
            if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
                let dims: Vec<usize> = items
                    .iter()
                    .map(|v| {
                        if let Value::Int(n) = v {
                            *n as usize
                        } else {
                            unreachable!()
                        }
                    })
                    .collect();
                Ok(rand_dims_float(&dims, rng, low, high)?)
            } else {
                let mut out = Vec::with_capacity(items.len());
                for v in items {
                    out.push(rand_shape_float(v, rng, low, high)?);
                }
                Ok(Value::List(out))
            }
        }
        _ => Err(WqError::TypeError("rand expects an integer shape".into())),
    }
}

pub fn rand(args: &[Value]) -> WqResult<Value> {
    let mut rng = rand::rng();
    match args.len() {
        0 => Ok(Value::Float(rng.random())),
        1 => rand_shape_float(&args[0], &mut rng, 0.0, 1.0),
        2 => match &args[1] {
            Value::Int(n) if *n > 0 => rand_shape_int(&args[0], &mut rng, 0, *n),
            Value::Float(f) if *f > 0.0 => rand_shape_float(&args[0], &mut rng, 0.0, *f),
            v => Err(WqError::DomainError(format!(
                "expected positive int or float, got {}",
                v.type_name()
            ))),
        },
        3 => match (&args[1], &args[2]) {
            (Value::Int(a), Value::Int(b)) if a < b => rand_shape_int(&args[0], &mut rng, *a, *b),
            (a, b) => {
                let af = match a {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "expected numbers, got {}",
                            a.type_name()
                        )));
                    }
                };
                let bf = match b {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "expected numbers, got {}",
                            b.type_name()
                        )));
                    }
                };
                if af < bf {
                    rand_shape_float(&args[0], &mut rng, af, bf)
                } else {
                    Err(WqError::RuntimeError(format!(
                        "require a < b, got {af} >= {bf}"
                    )))
                }
            }
        },
        other => Err(WqError::ArityError(format!(
            "rand expects 0 to 3 arguments, got {other}"
        ))),
    }
}

macro_rules! bind_math {
    ($name:ident, $func:path) => {
        pub fn $name(args: &[Value]) -> WqResult<Value> {
            if args.len() != 1 {
                return Err(WqError::ArityError(
                    stringify!($name).to_string() + " expects 1 argument",
                ));
            }
            match &args[0] {
                Value::Int(n) => Ok(Value::Float($func(*n as f64))),
                Value::Float(f) => Ok(Value::Float($func(*f))),
                Value::List(items) => {
                    let result: WqResult<Vec<Value>> =
                        items.iter().map(|v| $name(&[v.clone()])).collect();
                    Ok(Value::List(result?))
                }
                Value::IntList(arr) => {
                    let result: WqResult<Vec<Value>> =
                        arr.iter().map(|&i| $name(&[Value::Int(i)])).collect();
                    Ok(Value::List(result?))
                }
                other => Err(WqError::TypeError(
                    stringify!($name).to_string()
                        + " only works on numbers or lists of numbers, got "
                        + other.type_name(),
                )),
            }
        }
    };
}

bind_math!(sin, f64::sin);
bind_math!(cos, f64::cos);
bind_math!(tan, f64::tan);
bind_math!(sinh, f64::sinh);
bind_math!(cosh, f64::cosh);
bind_math!(tanh, f64::tanh);
