use rand::Rng;

use super::arity_error;
use crate::value::{Value, WqError, WqResult};

pub fn abs(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("abs", "1", args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(n.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| abs(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`abs`: expected 'numbers', got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn neg(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("neg", "1", args.len()));
    }

    args[0].neg_value().ok_or_else(|| {
        WqError::TypeError(format!(
            "`neg`: expected 'numbers', got {}",
            args[0].type_name_verbose()
        ))
    })
}

pub fn sgn(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("sgn", "1", args.len()));
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
            let result: WqResult<Vec<Value>> = items.iter().map(|v| sgn(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`sgn`: expected 'numbers', got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn sqrt(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("sqrt", "1", args.len()));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError(
                    "`sqrt`: sqrt of negative number".to_string(),
                ))
            } else {
                Ok(Value::Float((*n as f64).sqrt()))
            }
        }
        Value::Float(f) => {
            if *f < 0.0 {
                Err(WqError::DomainError(
                    "`sqrt`: sqrt of negative number".to_string(),
                ))
            } else {
                Ok(Value::Float(f.sqrt()))
            }
        }
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| sqrt(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`sqrt`: expected numbers, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn exp(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("exp", "1", args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Float((*n as f64).exp())),
        Value::Float(f) => Ok(Value::Float(f.exp())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| exp(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`exp`: expected numbers, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn ln(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("ln", "1", args.len()));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n <= 0 {
                Err(WqError::DomainError(
                    "`ln`: ln of non-positive number".to_string(),
                ))
            } else {
                Ok(Value::Float((*n as f64).ln()))
            }
        }
        Value::Float(f) => {
            if *f <= 0.0 {
                Err(WqError::DomainError(
                    "`ln`: ln of non-positive number".to_string(),
                ))
            } else {
                Ok(Value::Float(f.ln()))
            }
        }
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| ln(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`ln`: expected numbers, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn floor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("floor", "1", args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(f.floor() as i64)),
        Value::IntList(items) => Ok(Value::IntList(items.clone())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| floor(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`floor`: expected numbers, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn ceil(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("ceil", "1", args.len()));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(f.ceil() as i64)),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| ceil(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(format!(
            "`ceil`: expected numbers, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

pub fn rand(args: &[Value]) -> WqResult<Value> {
    let mut rng = rand::rng();
    match args.len() {
        0 => Ok(Value::Float(rng.random())),
        1 => match &args[0] {
            Value::Int(n) if *n > 0 => Ok(Value::Int(rng.random_range(0..*n))),
            Value::Float(f) if *f > 0.0 => Ok(Value::Float(rng.random_range(0.0..*f))),
            Value::Int(_) | Value::Float(_) => Err(WqError::DomainError(format!(
                "`rand`: expected positive numbers, got {}",
                args[0].type_name_verbose()
            ))),
            _ => Err(WqError::TypeError(format!(
                "`rand`: expected numbers, got {}",
                args[0].type_name_verbose()
            ))),
        },
        2 => match (&args[0], &args[1]) {
            (Value::Int(a), Value::Int(b)) if a < b => Ok(Value::Int(rng.random_range(*a..*b))),
            (a, b) => {
                let af = match a {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "`rand`: expected numbers, got {}",
                            a.type_name_verbose()
                        )));
                    }
                };
                let bf = match b {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "`rand`: expected numbers, got {}",
                            b.type_name_verbose()
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

macro_rules! bind_math {
    ($name:ident, $func:path) => {
        pub fn $name(args: &[Value]) -> WqResult<Value> {
            if args.len() != 1 {
                return Err(arity_error(stringify!($name), "1", args.len()));
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
                        + " expected numbers, got "
                        + other.type_name_verbose(),
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
bind_math!(arcsin, f64::asin);
bind_math!(arccos, f64::acos);
bind_math!(arctan, f64::atan);

// fn parse_prefix_arg(args: &[Value], name: &str) -> WqResult<bool> {
//     Ok(match args.len() {
//         1 => false,
//         2 => match &args[1] {
//             Value::Bool(b) => *b,
//             _ => {
//                 return Err(WqError::TypeError(format!(
//                     "`{name}`: expected bool at arg1"
//                 )));
//             }
//         },
//         _ => return Err(arity_error(name, "1 or 2", args.len())),
//     })
// }

// fn int_to_hex_chars(n: i64, prefix: bool) -> Vec<Value> {
//     let (sign, abs) = if n < 0 {
//         ('-', (-n) as u64)
//     } else {
//         ('+', n as u64)
//     };
//     let mut s = if prefix {
//         format!("{abs:#x}")
//     } else {
//         format!("{abs:x}")
//     };
//     if sign == '-' {
//         s.insert(0, '-');
//     }
//     s.chars().map(Value::Char).collect()
// }

// fn int_to_bin_chars(n: i64, prefix: bool) -> Vec<Value> {
//     let (sign, abs) = if n < 0 {
//         ('-', (-n) as u64)
//     } else {
//         ('+', n as u64)
//     };
//     let mut s = if prefix {
//         format!("{abs:#b}")
//     } else {
//         format!("{abs:b}")
//     };
//     if sign == '-' {
//         s.insert(0, '-');
//     }
//     s.chars().map(Value::Char).collect()
// }

// pub fn hex(args: &[Value]) -> WqResult<Value> {
//     let prefix = parse_prefix_arg(args, "hex")?;

//     match &args[0] {
//         Value::Int(n) => Ok(Value::List(int_to_hex_chars(*n, prefix))),
//         Value::IntList(items) => {
//             let result: Vec<Value> = items
//                 .iter()
//                 .map(|&n| Value::List(int_to_hex_chars(n, prefix)))
//                 .collect();
//             Ok(Value::List(result))
//         }
//         Value::List(items) => {
//             let mut out = Vec::new();
//             for v in items {
//                 if let Value::Int(n) = v {
//                     out.push(Value::List(int_to_hex_chars(*n, prefix)));
//                 } else {
//                     return Err(WqError::TypeError(
//                         "hex expects integers or list of integers".to_string(),
//                     ));
//                 }
//             }
//             Ok(Value::List(out))
//         }
//         _ => Err(WqError::TypeError(
//             "hex expects integers or list of integers".to_string(),
//         )),
//     }
// }

// pub fn bin(args: &[Value]) -> WqResult<Value> {
//     let prefix = parse_prefix_arg(args, "bin")?;

//     match &args[0] {
//         Value::Int(n) => Ok(Value::List(int_to_bin_chars(*n, prefix))),
//         Value::IntList(items) => {
//             let result: Vec<Value> = items
//                 .iter()
//                 .map(|&n| Value::List(int_to_bin_chars(n, prefix)))
//                 .collect();
//             Ok(Value::List(result))
//         }
//         Value::List(items) => {
//             let mut out = Vec::new();
//             for v in items {
//                 if let Value::Int(n) = v {
//                     out.push(Value::List(int_to_bin_chars(*n, prefix)));
//                 } else {
//                     return Err(WqError::TypeError(
//                         "bin expects integers or list of integers".to_string(),
//                     ));
//                 }
//             }
//             Ok(Value::List(out))
//         }
//         _ => Err(WqError::TypeError(
//             "bin expects integers or list of integers".to_string(),
//         )),
//     }
// }

#[cfg(test)]
mod tests {
    // use super::*;

    // #[test]
    // fn hex_single() {
    //     let res = hex(&[Value::Int(255)]).unwrap();
    //     assert_eq!(res, Value::List("ff".chars().map(Value::Char).collect()));
    // }

    // #[test]
    // fn hex_list_prefix() {
    //     let res = hex(&[Value::IntList(vec![10, 11]), Value::Bool(true)]).unwrap();
    //     let expected = Value::List(vec![
    //         Value::List("0xa".chars().map(Value::Char).collect()),
    //         Value::List("0xb".chars().map(Value::Char).collect()),
    //     ]);
    //     assert_eq!(res, expected);
    // }

    // #[test]
    // fn bin_negative_prefix() {
    //     let res = bin(&[Value::Int(-5), Value::Bool(true)]).unwrap();
    //     assert_eq!(
    //         res,
    //         Value::List("-0b101".chars().map(Value::Char).collect())
    //     );
    // }
}
