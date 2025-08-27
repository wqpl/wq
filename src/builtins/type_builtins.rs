use super::arity_error;
use crate::value::{Value, WqError, WqResult};

pub fn type_of_verbose(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("type", "1 argument", args.len()));
    }
    Ok(Value::List(
        args[0]
            .type_name_verbose()
            .to_string()
            .chars()
            .map(Value::Char)
            .collect(),
    ))
}

pub fn type_of_simple(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("type", "1 argument", args.len()));
    }
    Ok(Value::List(
        args[0]
            .type_name_simple()
            .to_string()
            .chars()
            .map(Value::Char)
            .collect(),
    ))
}

pub fn to_symbol(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("symbol", "1 argument", args.len()));
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

pub fn is_null(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("isnull", "1 argument", args.len()));
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
