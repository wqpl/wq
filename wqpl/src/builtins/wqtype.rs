use std::sync::Arc;

use num_bigint::BigInt;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{Value, WqResult, into_wq_string};
use crate::vm::Vm;
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn type_of(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Type, [1], &args)?;
    Ok(into_wq_string(args[0].type_name()))
}

pub(super) fn is_atom(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::AtomQ, [1], &args)?;
    Ok(Value::Bool(args[0].is_atom()))
}

pub(super) fn is_unit(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::UnitQ, [1], &args)?;
    Ok(Value::Bool(args[0].is_unit()))
}

fn is_valid_tag_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '?')
}

pub(super) fn to_tag(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Tag, [1], &args)?;
    let input = args.into_iter().next().unwrap();
    if matches!(&input, Value::Tag(_)) {
        return Ok(input);
    }
    let name = input
        .to_rust_string_with_note()
        .map_err(|e| e.src(BE::Tag).at_arg(0))?;
    if !is_valid_tag_name(&name) {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Tag)
            .msg("invalid tag name")
            .at_arg(0)
            .attach_note(
                "tag names must be non-empty and contain only alphanumeric characters, '_' or '?'",
            ));
    }
    Ok(Value::Tag(name.into()))
}

pub(super) fn to_char(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Char, [1], &args)?;
    let input = args.into_iter().next().unwrap();
    let s = match input {
        Value::Char(c) => return Ok(Value::Char(c)),
        ref val if val.is_string_like() => val
            .to_rust_string_with_note()
            .map_err(|e| e.src(BE::Char).at_arg(0))?,
        ref val => val.to_string(),
    };
    let mut chars = s.chars();
    let ch = chars.next().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Char)
            .msg("expected single character")
            .at_arg(0)
            .attach_note("got empty string")
    })?;
    if chars.next().is_some() {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Char)
            .msg("expected single character")
            .at_arg(0)
            .attach_note(format!("got string of length {}", s.chars().count())));
    }
    Ok(Value::Char(ch))
}

pub(super) fn to_bool(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::ToBool, [1], &args)?;
    let res = match &args[0] {
        Value::Int(1) => true,
        Value::Int(0) => false,
        Value::BigInt(bi) if **bi == BigInt::from(1) => true,
        Value::BigInt(bi) if **bi == BigInt::from(0) => false,
        Value::Bool(b) => *b,
        v => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Char)
                .msg("Only 0 or 1 can be converted to bool")
                .at_arg(0)
                .got1(v));
        }
    };
    Ok(Value::Bool(res))
}

pub(super) fn to_list(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::ToList, [1], &args)?;
    let input = args.into_iter().next().unwrap();
    if matches!(&input, Value::List(_) | Value::IntList(_)) {
        return Ok(input);
    }
    match input {
        Value::Dict(map) => Ok(Value::List(Arc::new(
            map.iter()
                .map(|(k, v)| Value::List(Arc::new(vec![Value::Tag(k.clone()), v.clone()])))
                .collect(),
        ))),
        atom => Ok(Value::List(Arc::new(vec![atom]))),
    }
}

pub(super) fn to_dict(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::ToDict, [1], &args)?;

    fn extract_entry(v: &Value) -> WqResult<(Arc<str>, Value)> {
        match v {
            Value::List(pair) if pair.len() == 2 => {
                let key = match &pair[0] {
                    Value::Tag(s) => s.clone(),
                    Value::String(s) => Arc::from(s.as_str()),
                    Value::Char(c) => Arc::from(c.to_string().as_str()),
                    other => {
                        return Err(crate::wqerror::WqError::new(
                            crate::wqerror::WqErrorType::Domain,
                        )
                        .msg(format!(
                            "dict key must be tag, string or char, got {}",
                            other.type_name()
                        )));
                    }
                };
                Ok((key, pair[1].clone()))
            }
            Value::IntList(pair) if pair.len() == 2 => {
                Ok((Arc::from(pair[0].to_string().as_str()), Value::Int(pair[1])))
            }
            other => Err(
                crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Domain)
                    .msg(format!("expected list of pairs, got {}", other.type_name())),
            ),
        }
    }

    let entries = match &args[0] {
        Value::List(items) => items
            .iter()
            .map(extract_entry)
            .collect::<WqResult<Vec<_>>>()?,

        other => {
            return Err(
                crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Domain).msg(format!(
                    "expected list or set of pairs, got {}",
                    other.type_name()
                )),
            );
        }
    };
    let mut map = indexmap::IndexMap::with_capacity(entries.len());
    for (k, v) in entries {
        map.insert(k, v);
    }
    Ok(Value::Dict(Arc::new(map)))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    // use crate::value::IntoWqValue;
    use crate::vm::Vm;

    // #[test]
    // fn tag_accepts_question_mark() {
    //     let mut vm = Vm::new(vec![]);
    //     let val = "a?".into_wq_value();
    //     let result = to_tag(&mut vm, BuiltinFnArgs::from(val)).unwrap();
    //     assert_eq!(result, Value::Tag("a?".to_string()));
    // }

    #[test]
    fn counts_as_atom() {
        let mut vm = Vm::new(vec![]);
        let val = Value::List(Arc::new(vec![Value::Int(1)]));
        let result = is_atom(&mut vm, BuiltinFnArgs::from(val)).unwrap();
        assert_eq!(result, Value::Bool(false));
    }
}
