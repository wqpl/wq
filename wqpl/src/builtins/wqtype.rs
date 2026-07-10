use std::sync::Arc;

use num_bigint::BigInt;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::seq::ValueSeq;
use crate::value::{Value, WqResult, into_wq_string};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn type_of(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Type, [1], &args)?;
    Ok(into_wq_string(args[0].type_name()))
}

pub(super) fn is_atom(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::AtomQ, [1], &args)?;
    Ok(Value::Bool(args[0].is_atom()))
}

pub(super) fn is_unit(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::UnitQ, [1], &args)?;
    Ok(Value::Bool(args[0].is_unit()))
}

fn is_valid_tag_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_alphanumeric() || ch == '_' || ch == '?')
}

pub(super) fn to_tag(args: BuiltinFnArgs) -> WqResult<Value> {
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

pub(super) fn to_char(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Char, [1], &args)?;
    let input = args.into_iter().next().unwrap();
    let s = match input {
        Value::Char(c) => return Ok(Value::Char(c)),
        ref val if val.is_string() => val
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

pub(super) fn to_bool(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Bool, [1], &args)?;
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

pub(super) fn to_list(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::List, [1], &args)?;
    let input = args.into_iter().next().unwrap();
    if matches!(
        &input,
        Value::List(_)
            | Value::IntList(_)
            | Value::IntRange(_)
            | Value::FloatList(_)
            | Value::BoolList(_)
    ) {
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

pub(super) fn to_dict(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Dict, [1], &args)?;

    fn extract_entry(v: &Value) -> WqResult<(Arc<str>, Value)> {
        let Some(pair) = ValueSeq::from_value(v).filter(|pair| pair.len() == 2) else {
            return Err(WqError::new(WqErrorType::Domain)
                .msg(format!("expected list of pairs, got {}", v.type_name())));
        };
        let key_value = pair.get(0).expect("two-item pair has a key");
        let key = match key_value {
            Value::Tag(s) => s,
            Value::String(s) => Arc::from(s.as_str()),
            Value::Char(c) => Arc::from(c.to_string()),
            Value::Int(i) => Arc::from(i.to_string()),
            Value::BigInt(i) => Arc::from(i.to_string()),
            other => {
                return Err(WqError::new(WqErrorType::Domain).msg(format!(
                    "dict key must be tag, string, char or int, got {}",
                    other.type_name()
                )));
            }
        };
        Ok((key, pair.get(1).expect("two-item pair has a value")))
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

    // #[test]
    // fn tag_accepts_question_mark() {
    //     let mut vm = Vm::new(vec![]);
    //     let val = "a?".into_wq_value();
    //     let result = to_tag(BuiltinFnArgs::from(val)).unwrap();
    //     assert_eq!(result, Value::Tag("a?".to_string()));
    // }

    #[test]
    fn counts_as_atom() {
        let val = Value::List(Arc::new(vec![Value::Int(1)]));
        let result = is_atom(BuiltinFnArgs::from(val)).unwrap();
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn list_conversion_preserves_all_non_string_list_storage() {
        let values = [
            Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 3))),
            Value::FloatList(Arc::new(vec![ordered_float::OrderedFloat(1.5)])),
            Value::BoolList(Arc::new(vec![true, false])),
        ];

        for value in values {
            let converted = to_list(BuiltinFnArgs::from(value.clone())).expect("list succeeds");
            assert_eq!(converted.type_name_verbose(), value.type_name_verbose());
            assert_eq!(converted, value);
        }
    }

    #[test]
    fn dict_conversion_reads_pairs_through_the_list_abstraction() {
        let pairs = Value::List(Arc::new(vec![
            Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 2))),
            Value::String(Arc::new("ab".to_owned())),
        ]));
        let converted = to_dict(BuiltinFnArgs::from(pairs)).expect("dict succeeds");
        let Value::Dict(entries) = converted else {
            unreachable!("dict conversion should return a dict");
        };
        assert_eq!(entries.get("1"), Some(&Value::Int(2)));
        assert_eq!(entries.get("a"), Some(&Value::Char('b')));
    }
}
