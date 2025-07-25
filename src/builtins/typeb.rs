use crate::value::valuei::{Value, WqError, WqResult};

pub fn type_of(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "type expects 1 argument".to_string(),
        ));
    }
    Ok(Value::List(
        args[0]
            .type_name()
            .to_string()
            .chars()
            .map(Value::Char)
            .collect(),
    ))
}

pub fn to_symbol(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "symbol expects 1 argument".to_string(),
        ));
    }

    let input = &args[0];
    let name = match input {
        Value::Symbol(s) => s.clone(),
        Value::Char(c) => (*c).to_string(),
        Value::List(items) if input.is_string() => {
            let mut s = String::new();
            for v in items {
                if let Value::Char(c) = v {
                    s.push(*c);
                }
            }
            s
        }
        _ => return Err(WqError::TypeError("symbol expects a string".to_string())),
    };

    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '?')
    {
        return Err(WqError::DomainError(format!("invalid symbol name: {name}")));
    }

    Ok(Value::symbol(name))
}

pub fn to_string(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "string expects 1 argument".to_string(),
        ));
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

pub fn is_null(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "isnull expects 1 argument".to_string(),
        ));
    }
    match args[0] {
        Value::Null => Ok(Value::Bool(true)),
        _ => Ok(Value::Bool(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_accepts_question_mark() {
        let val = Value::List("a?".chars().map(Value::Char).collect());
        let result = to_symbol(&[val]).unwrap();
        assert_eq!(result, Value::symbol("a?".to_string()));
    }
}
