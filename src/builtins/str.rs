use super::arity_error;
use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

pub fn to_str(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("str", "1", args.len()));
    }
    let arg = &args[0];
    match arg {
        Value::Char(c) => Ok(Value::Char(*c)),
        Value::List(_) if arg.is_str() => Ok(arg.clone()),
        _ => {
            let s = args[0].to_string();
            let chars: Vec<Value> = s.chars().map(Value::Char).collect();
            Ok(Value::List(chars))
        }
    }
}

// Append Value to an output Vec<Value::Char(..)>, avoiding extra allocations when possible.
fn push_value_as_chars(out: &mut Vec<Value>, v: &Value) {
    match v {
        Value::List(items) if items.iter().all(|c| matches!(c, Value::Char(_))) => {
            out.extend(items.iter().map(|c| {
                let Value::Char(ch) = c else { unreachable!() };
                Value::Char(*ch)
            }));
        }
        Value::Char(c) => out.push(Value::Char(*c)),
        _ => out.extend(v.to_string().chars().map(Value::Char)),
    }
}

// Count "{}" placeholders in a list-of-chars format, respecting "{{" and "}}".
fn count_placeholders(fmt_chars: &[Value]) -> usize {
    let mut i = 0usize;
    let mut count = 0usize;
    while i < fmt_chars.len() {
        let ch = match fmt_chars[i] {
            Value::Char(c) => c,
            _ => unreachable!(),
        };
        if ch == '{' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('{')) => i += 2, // "{{" -> literal '{'
                Some(Value::Char('}')) => {
                    count += 1;
                    i += 2
                } // "{}" -> placeholder
                _ => i += 1,                      // lone '{' -> literal
            }
        } else if ch == '}' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('}')) => i += 2, // "}}" -> literal '}'
                _ => i += 1,                      // lone '}' -> literal
            }
        } else {
            i += 1;
        }
    }
    count
}

pub fn fmt(args: &[Value]) -> WqResult<Value> {
    // Runtime check
    let fmt_chars: &Vec<Value> = match args.first() {
        Some(Value::List(items)) if items.iter().all(|c| matches!(c, Value::Char(_))) => items,
        Some(Value::Char(c)) => {
            return Ok(Value::Char(*c));
        }
        Some(s) => {
            return Err(WqError::DomainError(format!(
                "`fmt`: invalid template, expected 'str', got {}",
                s.type_name_verbose()
            )));
        }
        None => {
            return Err(arity_error("fmt", "at least 1", 0));
        }
    };

    // Pre-count placeholders for arity errors
    let needed = count_placeholders(fmt_chars);
    let provided = args.len().saturating_sub(1);
    if provided < needed {
        return Err(arity_error("fmt", needed.to_string().as_str(), provided));
    }

    // Format
    let mut out: Vec<Value> = Vec::with_capacity(fmt_chars.len() + 16);
    let mut i = 0usize;
    let mut arg_idx = 0usize;

    while i < fmt_chars.len() {
        let ch = match fmt_chars[i] {
            Value::Char(c) => c,
            _ => unreachable!(),
        };

        if ch == '{' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('{')) => {
                    out.push(Value::Char('{'));
                    i += 2;
                }
                Some(Value::Char('}')) => {
                    // "{}" -> substitute next argument
                    // This is safe because of the pre-check using `needed`.
                    push_value_as_chars(&mut out, &args[arg_idx + 1]);
                    arg_idx += 1;
                    i += 2;
                }
                _ => {
                    out.push(Value::Char('{'));
                    i += 1;
                }
            }
        } else if ch == '}' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('}')) => {
                    out.push(Value::Char('}'));
                    i += 2;
                }
                _ => {
                    out.push(Value::Char('}'));
                    i += 1;
                }
            }
        } else {
            out.push(Value::Char(ch));
            i += 1;
        }
    }

    // Redundant args
    if provided > needed {
        return Err(arity_error(
            "fmt",
            (needed + 1).to_string().as_str(),
            provided + 1,
        ));
    }

    Ok(Value::List(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation() {
        let test = Value::List("x = {}".chars().map(Value::Char).collect());
        let res = fmt(&[test, Value::Int(5)]).unwrap();
        assert_eq!(res, Value::List("x = 5".chars().map(Value::Char).collect()));
    }

    #[test]
    fn escape_braces() {
        let test = Value::List("{{}}".chars().map(Value::Char).collect());
        let res = fmt(&[test]).unwrap();
        assert_eq!(res, Value::List("{}".chars().map(Value::Char).collect()));
    }
}
