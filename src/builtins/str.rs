use super::arity_error;
use crate::{
    builtins::values_to_strings,
    value::{Value, WqResult},
};

pub fn to_str(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("str", "1 argument", args.len()));
    }
    let arg = &args[0];
    match arg {
        Value::Char(c) => Ok(Value::Char(*c)),
        Value::List(_) if arg.is_string() => Ok(arg.clone()),
        _ => {
            let s = args[0].to_string();
            let chars: Vec<Value> = s.chars().map(Value::Char).collect();
            Ok(Value::List(chars))
        }
    }
}

pub fn fmt(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(arity_error("fmt", "at least 1 argument", args.len()));
    }

    let fmt = values_to_strings(&[args[0].clone()])?
        .into_iter()
        .next()
        .unwrap();

    let mut output = String::new();
    let mut iter = fmt.chars().peekable();
    let mut arg_idx = 0usize;

    while let Some(ch) = iter.next() {
        if ch == '{' {
            match iter.peek() {
                Some('{') => {
                    output.push('{');
                    iter.next();
                }
                Some('}') => {
                    iter.next();
                    if arg_idx + 1 >= args.len() {
                        return Err(arity_error("fmt", "more arguments", args.len()));
                    }
                    output.push_str(&value_to_plain_string(&args[arg_idx + 1]));
                    arg_idx += 1;
                }
                _ => output.push('{'),
            }
        } else if ch == '}' {
            match iter.peek() {
                Some('}') => {
                    output.push('}');
                    iter.next();
                }
                _ => output.push('}'),
            }
        } else {
            output.push(ch);
        }
    }

    if arg_idx + 1 < args.len() {
        return Err(arity_error("fmt", "fewer arguments", args.len()));
    }

    Ok(Value::List(output.chars().map(Value::Char).collect()))
}

fn value_to_plain_string(v: &Value) -> String {
    match v {
        Value::List(items) if items.iter().all(|c| matches!(c, Value::Char(_))) => items
            .iter()
            .map(|c| {
                if let Value::Char(ch) = c {
                    *ch
                } else {
                    unreachable!()
                }
            })
            .collect(),
        Value::Char(c) => c.to_string(),
        _ => v.to_string(),
    }
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
