use std::sync::Arc;

use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity, type_mismatch};
use crate::value::{IntoWqValue, Value, WqResult};
use crate::wqerror::Requirement;

fn normalize_idx(i: i64, len: usize) -> Option<usize> {
    if i >= 0 {
        usize::try_from(i).ok().filter(|&idx| idx < len)
    } else {
        let off = usize::try_from(i.unsigned_abs()).ok()?;
        len.checked_sub(off)
    }
}

pub(super) fn keys(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Keys, [1], &args)?;
    match &args[0] {
        Value::Dict(map) => {
            let list = map.keys().cloned().map(Value::Tag).collect();
            Ok(Value::List(Arc::new(list)))
        }
        v => Err(type_mismatch(BuiltinEnum::Keys, 0, Requirement::DICT, v)),
    }
}

/// Returns the key at the given positional index, supporting negative indices.
pub(super) fn idx_to_key(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::IdxToKey, [2], &args)?;
    let dict = match &args[0] {
        Value::Dict(map) => map,
        other => {
            return Err(type_mismatch(
                BuiltinEnum::IdxToKey,
                0,
                Requirement::DICT,
                other,
            ));
        }
    };
    let idx = match &args[1] {
        Value::Int(i) => i,
        other => {
            return Err(type_mismatch(
                BuiltinEnum::IdxToKey,
                1,
                Requirement::INT,
                other,
            ));
        }
    };
    let Some(norm_idx) = normalize_idx(*idx, dict.len()) else {
        return Ok(Value::unit());
    };
    match dict.get_index(norm_idx) {
        Some((k, _)) => Ok(Value::Tag(k.clone())),
        None => Ok(Value::unit()),
    }
}

/// Returns the positional index for the given key or unit if absent.
pub(super) fn key_to_idx(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::KeyToIdx, [2], &args)?;
    let dict = match &args[0] {
        Value::Dict(map) => map,
        other => {
            return Err(type_mismatch(
                BuiltinEnum::KeyToIdx,
                0,
                Requirement::DICT,
                other,
            ));
        }
    };
    let key = match &args[1] {
        Value::Tag(s) => s,
        other => {
            return Err(type_mismatch(
                BuiltinEnum::KeyToIdx,
                1,
                Requirement::TAG,
                other,
            ));
        }
    };
    match dict.get_index_of(key.as_ref()) {
        Some(idx) => Ok(idx.into_wq_value()),
        None => Ok(Value::unit()),
    }
}
