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

pub fn chr(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "chr expects 1 argument".to_string(),
        ));
    }
    match &args[0] {
        Value::Int(n) => {
            let ch = char::from_u32(*n as u32)
                .ok_or_else(|| WqError::DomainError("invalid char code".into()))?;
            Ok(Value::Char(ch))
        }
        Value::IntList(items) => {
            let mut out = Vec::new();
            for &n in items {
                let ch = char::from_u32(n as u32)
                    .ok_or_else(|| WqError::DomainError("invalid char code".into()))?;
                out.push(Value::Char(ch));
            }
            Ok(Value::List(out))
        }
        Value::List(items) => {
            let mut out = Vec::new();
            for v in items {
                if let Value::Int(n) = v {
                    let ch = char::from_u32(*n as u32)
                        .ok_or_else(|| WqError::DomainError("invalid char code".into()))?;
                    out.push(Value::Char(ch));
                } else {
                    return Err(WqError::TypeError(
                        "chr expects integers or list of integers".to_string(),
                    ));
                }
            }
            Ok(Value::List(out))
        }
        _ => Err(WqError::TypeError("chr expects an integer".to_string())),
    }
}

pub fn ord(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "ord expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Char(c) => Ok(Value::Int(*c as i64)),
        Value::List(items) => {
            let mut out = Vec::new();
            for v in items {
                match v {
                    Value::Char(ch) => out.push(*ch as i64),
                    _ => {
                        return Err(WqError::TypeError(
                            "ord expects characters or list of characters".to_string(),
                        ));
                    }
                }
            }
            Ok(Value::IntList(out))
        }
        Value::Symbol(s) => Ok(Value::IntList(s.chars().map(|c| c as i64).collect())),
        _ => Err(WqError::TypeError(
            "ord expects a character or string".to_string(),
        )),
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

    #[test]
    fn chr_single_int() {
        let result = chr(&[Value::Int(65)]).unwrap();
        assert_eq!(result, Value::Char('A'));
    }

    #[test]
    fn chr_int_list() {
        let result = chr(&[Value::IntList(vec![65, 66])]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Char('A'), Value::Char('B')])
        );
    }

    #[test]
    fn ord_single_char() {
        let result = ord(&[Value::Char('A')]).unwrap();
        assert_eq!(result, Value::Int(65));
    }

    #[test]
    fn ord_char_list() {
        let input = Value::List(vec![Value::Char('A'), Value::Char('B')]);
        let result = ord(&[input]).unwrap();
        assert_eq!(result, Value::IntList(vec![65, 66]));
    }
}
