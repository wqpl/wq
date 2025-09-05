use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

use super::arity_error;

pub fn wq_typev(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("type", "1", args.len()));
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

pub fn wq_type_of(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("type", "1", args.len()));
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
        return Err(arity_error("symbol", "1", args.len()));
    }

    let input = &args[0];
    let name = match input {
        Value::Symbol(s) => s.clone(),
        Value::Char(c) => (*c).to_string(),
        Value::List(items) if input.is_str() => {
            let mut s = String::new();
            for v in items {
                if let Value::Char(c) = v {
                    s.push(*c);
                }
            }
            s
        }
        _ => {
            return Err(WqError::DomainError(format!(
                "`symbol`: expected 'str', got {}",
                input.type_name()
            )));
        }
    };

    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '?')
    {
        return Err(WqError::DomainError(format!("invalid symbol name: {name}")));
    }

    Ok(Value::Symbol(name))
}

pub fn is_atom(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("atom?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_atom()))
}

pub fn is_int(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("int?", "1", args.len()));
    }
    Ok(Value::Bool(matches!(args[0], Value::Int(_))))
}

// pub fn is_float(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("float?", "1", args.len()));
//     }
//     Ok(Value::Bool(matches!(args[0], Value::Float(_))))
// }

pub fn is_number(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("number?", "1", args.len()));
    }
    Ok(Value::Bool(matches!(
        args[0],
        Value::Int(_) | Value::Float(_)
    )))
}

pub fn is_fn(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("fn?", "1", args.len()));
    }
    Ok(Value::Bool(matches!(
        args[0],
        Value::Function { .. }
            | Value::CompiledFunction { .. }
            | Value::BuiltinFunction(_)
            | Value::Closure { .. }
    )))
}

pub fn is_bool(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("bool?", "1", args.len()));
    }
    Ok(Value::Bool(matches!(args[0], Value::Bool(_))))
}

pub fn is_stream(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("stream?", "1", args.len()));
    }
    Ok(Value::Bool(matches!(args[0], Value::Stream(_))))
}

pub fn is_list(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("list?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_list()))
}

pub fn is_str(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("str?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_str()))
}

pub fn is_unit(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("unit?", "1", args.len()));
    }
    Ok(Value::Bool(args[0].is_empty()))
}

pub fn is_dict(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("dict?", "1", args.len()));
    }
    Ok(Value::Bool(matches!(args[0], Value::Dict(_))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_accepts_question_mark() {
        let val = Value::List("a?".chars().map(Value::Char).collect());
        let result = to_symbol(&[val]).unwrap();
        assert_eq!(result, Value::Symbol("a?".to_string()));
    }
}
