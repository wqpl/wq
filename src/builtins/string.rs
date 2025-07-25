use crate::{
    builtins::values_to_strings,
    value::valuei::{Value, WqError, WqResult},
};

pub fn format_string(args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::FnArgCountMismatchError(
            "format expects at least 1 argument".to_string(),
        ));
    }

    let fmt = values_to_strings(&[args[0].clone()])?
        .into_iter()
        .next()
        .unwrap();

    let mut output = String::new();
    let mut iter = fmt.chars().peekable();
    let mut arg_idx = 0usize;

    while let Some(ch) = iter.next() {
        if ch == '\\' {
            match iter.peek() {
                Some('\\') => {
                    output.push('\\');
                    iter.next();
                }
                Some('n') => {
                    output.push('\n');
                    iter.next();
                }
                _ => output.push('\\'),
            }
        } else if ch == '{' {
            match iter.peek() {
                Some('{') => {
                    output.push('{');
                    iter.next();
                }
                Some('}') => {
                    iter.next();
                    if arg_idx + 1 >= args.len() {
                        return Err(WqError::FnArgCountMismatchError(
                            "format expects more arguments".to_string(),
                        ));
                    }
                    output.push_str(&args[arg_idx + 1].to_string());
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
        return Err(WqError::FnArgCountMismatchError(
            "too many arguments for format".to_string(),
        ));
    }

    Ok(Value::List(output.chars().map(Value::Char).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newline_escape() {
        let fmt = Value::List("\\n".chars().map(Value::Char).collect());
        let res = format_string(&[fmt]).unwrap();
        assert_eq!(res, Value::List("\n".chars().map(Value::Char).collect()));
    }

    #[test]
    fn escape_backslash_n() {
        let fmt = Value::List("\\\\n".chars().map(Value::Char).collect());
        let res = format_string(&[fmt]).unwrap();
        assert_eq!(res, Value::List("\\n".chars().map(Value::Char).collect()));
    }

    #[test]
    fn interpolation() {
        let fmt = Value::List("x = {}".chars().map(Value::Char).collect());
        let res = format_string(&[fmt, Value::Int(5)]).unwrap();
        assert_eq!(res, Value::List("x = 5".chars().map(Value::Char).collect()));
    }

    #[test]
    fn escape_braces() {
        let fmt = Value::List("{{}}".chars().map(Value::Char).collect());
        let res = format_string(&[fmt]).unwrap();
        assert_eq!(res, Value::List("{}".chars().map(Value::Char).collect()));
    }
}
