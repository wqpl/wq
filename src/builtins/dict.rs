use crate::{
    builtins::{
        BuiltinEnum,
        wqerr_ext::{check_arity, type_mismatch},
    },
    value::{IntoWqValue, Value, WqResult},
    vm::Vm,
};

fn normalize_idx(i: i64, len: usize) -> Option<usize> {
    if i >= 0 {
        usize::try_from(i).ok().filter(|&idx| idx < len)
    } else {
        let off = usize::try_from(i.unsigned_abs()).ok()?;
        len.checked_sub(off)
    }
}

pub fn keys(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::Keys, [1], args)?;
    match &args[0] {
        Value::Dict(map) => {
            let ks: Vec<String> = map.keys().cloned().collect();
            let list = ks.into_iter().map(Value::Symbol).collect();
            Ok(Value::List(list))
        }
        v => Err(type_mismatch(BuiltinEnum::Keys, 0, "dict", v)),
    }
}

pub fn has_key(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::HasKeyQ, [2], args)?;
    let key = match &args[0] {
        Value::Symbol(s) => s,
        other => return Err(type_mismatch(BuiltinEnum::HasKeyQ, 0, "symbol", other)),
    };
    let dict = match &args[1] {
        Value::Dict(map) => map,
        other => return Err(type_mismatch(BuiltinEnum::HasKeyQ, 1, "dict", other)),
    };
    Ok(Value::Bool(dict.contains_key(key)))
}

/// Returns the key at the given positional index, supporting negative indices.
pub fn idx_to_key(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::IdxToKey, [2], args)?;
    let idx = match &args[0] {
        Value::Int(i) => i,
        other => return Err(type_mismatch(BuiltinEnum::IdxToKey, 0, "int", other)),
    };
    let dict = match &args[1] {
        Value::Dict(map) => map,
        other => return Err(type_mismatch(BuiltinEnum::IdxToKey, 1, "dict", other)),
    };
    let Some(norm_idx) = normalize_idx(*idx, dict.len()) else {
        return Ok(Value::unit());
    };
    match dict.get_index(norm_idx) {
        Some((k, _)) => Ok(Value::Symbol(k.clone())),
        None => Ok(Value::unit()),
    }
}

/// Returns the positional index for the given key or unit if absent.
pub fn key_to_idx(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BuiltinEnum::KeyToIdx, [2], args)?;
    let key = match &args[0] {
        Value::Symbol(s) => s,
        other => return Err(type_mismatch(BuiltinEnum::KeyToIdx, 0, "symbol", other)),
    };
    let dict = match &args[1] {
        Value::Dict(map) => map,
        other => return Err(type_mismatch(BuiltinEnum::KeyToIdx, 1, "dict", other)),
    };
    match dict.get_index_of(key) {
        Some(idx) => Ok(idx.into_wq_value()),
        None => Ok(Value::unit()),
    }
}
