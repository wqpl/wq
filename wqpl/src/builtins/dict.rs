use std::sync::Arc;

use num_traits::ToPrimitive;

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

pub(super) fn values(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Values, [1], &args)?;
    match &args[0] {
        Value::Dict(map) => Ok(Value::from_items(map.values().cloned().collect())),
        value => Err(type_mismatch(
            BuiltinEnum::Values,
            0,
            Requirement::DICT,
            value,
        )),
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
        Value::Int(i) => Some(*i),
        Value::BigInt(i) => i.to_i64(),
        other => {
            return Err(type_mismatch(
                BuiltinEnum::IdxToKey,
                1,
                Requirement::INT,
                other,
            ));
        }
    };
    let Some(norm_idx) = idx.and_then(|idx| normalize_idx(idx, dict.len())) else {
        return Ok(Value::empty_list());
    };
    match dict.get_index(norm_idx) {
        Some((k, _)) => Ok(Value::Tag(k.clone())),
        None => Ok(Value::empty_list()),
    }
}

/// Returns the positional index for the given key or an empty list if absent.
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
        None => Ok(Value::empty_list()),
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use num_bigint::BigInt;
    use smallvec::smallvec;

    use super::*;

    fn sample_dict() -> Value {
        Value::Dict(Arc::new(IndexMap::from([
            (Arc::from("a"), Value::Int(10)),
            (Arc::from("b"), Value::Int(20)),
        ])))
    }

    #[test]
    fn values_follow_stored_order() {
        assert_eq!(
            values(BuiltinFnArgs::from(sample_dict())).expect("values succeeds"),
            Value::IntList(Arc::new(vec![10, 20]))
        );
    }

    #[test]
    fn idx_to_key_accepts_all_int_representations() {
        assert_eq!(
            idx_to_key(BuiltinFnArgs::from(smallvec![
                sample_dict(),
                Value::BigInt(Arc::new(BigInt::from(1)))
            ]))
            .expect("fitting bigint-backed position succeeds"),
            Value::Tag(Arc::from("b"))
        );

        assert_eq!(
            idx_to_key(BuiltinFnArgs::from(smallvec![
                sample_dict(),
                Value::BigInt(Arc::new(BigInt::from(u128::MAX)))
            ]))
            .expect("out-of-range bigint-backed position succeeds"),
            Value::empty_list()
        );
    }
}
