use std::sync::Arc;

use indexmap::IndexMap;
use num_traits::ToPrimitive;

use crate::value::seq::ValueSeq;
use crate::value::{IntoWqValue as _, Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) enum BulkIndexKey<'a> {
    IntList(&'a [i64]),
    IntRange(&'a crate::value::seq::IntRangeData),
    List(&'a [Value]),
}

impl Value {
    pub(crate) fn bulk_index_key(&self) -> Option<BulkIndexKey<'_>> {
        match self {
            Value::IntList(idxs) => Some(BulkIndexKey::IntList(idxs.as_slice())),
            Value::IntRange(idxs) => Some(BulkIndexKey::IntRange(idxs)),
            Value::List(idxs) => Some(BulkIndexKey::List(idxs.as_slice())),
            _ => None,
        }
    }

    /// Index into a list or dict
    /// Index with multiple raw arguments, avoiding the `Value::from_items`
    /// allocation when the caller already has a `Vec<Value>` of indices.
    pub(crate) fn index_many(&self, keys: &[Value]) -> Option<Value> {
        if let Some(seq) = ValueSeq::from_value(self) {
            let idxs = normalize_list_indices(keys, seq.len())?;
            return seq.gather(&idxs);
        }

        match self {
            // dict[x;y;z] ============================================================
            Value::Dict(map) => {
                let mut result = Vec::with_capacity(keys.len());
                for k in keys {
                    match k {
                        Value::Tag(s) => result.push(map.get(s.as_ref())?.clone()),
                        Value::Int(i) => {
                            let idx = normalize_idx(*i, map.len())?;
                            let (_, v) = map.get_index(idx)?;
                            result.push(v.clone());
                        }
                        Value::BigInt(i) => {
                            let idx = normalize_idx(i.to_i64()?, map.len())?;
                            let (_, v) = map.get_index(idx)?;
                            result.push(v.clone());
                        }
                        _ => return None,
                    }
                }
                Some(Value::from_items(result))
            }

            // Fallback to scalar index path (e.g. atom multiply) =====================
            other => {
                if keys.len() == 1 {
                    other.index(&keys[0])
                } else {
                    None
                }
            }
        }
    }

    pub(crate) fn index(&self, key: &Value) -> Option<Value> {
        if let Some(seq) = ValueSeq::from_value(self) {
            return resolve_single_idx(key, seq.len())
                .and_then(|idx| seq.get(idx))
                .or_else(|| {
                    let idxs = resolve_many_idx(key, seq.len())?;
                    seq.gather(&idxs)
                });
        }

        match (self, key) {
            // dict[x] ================================================================
            (Value::Dict(map), key) => resolve_single_idx(key, map.len())
                .and_then(|idx| map.get_index(idx).map(|(_, v)| v.clone()))
                .or_else(|| {
                    let idxs = resolve_many_idx(key, map.len())?;
                    let mut result = Vec::with_capacity(idxs.len());
                    for idx in idxs {
                        result.push(map.get_index(idx).map(|(_, v)| v.clone())?);
                    }
                    Some(Value::from_items(result))
                })
                .or_else(|| match key {
                    Value::Tag(key) => map.get(key.as_ref()).cloned(),
                    Value::List(keys) => {
                        let mut result = Vec::with_capacity(keys.len());
                        for k in keys.iter() {
                            match k {
                                Value::Tag(s) => result.push(map.get(s.as_ref())?.clone()),
                                Value::Int(i) => {
                                    let idx = normalize_idx(*i, map.len())?;
                                    let (_, v) = map.get_index(idx)?;
                                    result.push(v.clone());
                                }
                                Value::BigInt(i) => {
                                    let idx = normalize_idx(i.to_i64()?, map.len())?;
                                    let (_, v) = map.get_index(idx)?;
                                    result.push(v.clone());
                                }
                                _ => return None,
                            }
                        }
                        Some(Value::from_items(result))
                    }
                    _ => None,
                }),

            // complex[x] =============================================================
            (Value::Complex(z), key) => match key {
                Value::Tag(s) if s.as_ref() == "re" => Some(Value::float(z.re)),
                Value::Tag(s) if s.as_ref() == "im" => Some(Value::float(z.im)),
                Value::List(keys) => {
                    let mut result = Vec::with_capacity(keys.len());
                    for k in keys.iter() {
                        match k {
                            Value::Tag(s) if s.as_ref() == "re" => result.push(Value::float(z.re)),
                            Value::Tag(s) if s.as_ref() == "im" => result.push(Value::float(z.im)),
                            _ => return None,
                        }
                    }
                    Some(Value::from_items(result))
                }
                _ => None,
            },

            // fraction[x] ============================================================
            (Value::Fraction(fd), key) => match key {
                Value::Tag(s) if matches!(s.as_ref(), "n" | "numer") => {
                    Some(Value::from_bigint(fd.numer().clone()))
                }
                Value::Tag(s) if matches!(s.as_ref(), "d" | "denom") => {
                    Some(Value::from_bigint(fd.denom().clone()))
                }
                Value::List(keys) => {
                    let mut result = Vec::with_capacity(keys.len());
                    for k in keys.iter() {
                        match k {
                            Value::Tag(s) if matches!(s.as_ref(), "n" | "numer") => {
                                result.push(Value::from_bigint(fd.numer().clone()));
                            }
                            Value::Tag(s) if matches!(s.as_ref(), "d" | "denom") => {
                                result.push(Value::from_bigint(fd.denom().clone()));
                            }
                            _ => return None,
                        }
                    }
                    Some(Value::from_items(result))
                }
                _ => None,
            },

            // atom: multiplication
            (a, _b) if a.is_atom() => Some(a.clone()),
            _ => None,
        }
    }

    /// Mutate list or dict element by index/key. Returns `Some(())` on success
    /// and `None` if the key does not exist or the types are incompatible.
    pub(crate) fn assign_by_index(&mut self, key: &Value, value: Value) -> Option<()> {
        materialize_int_range(self);
        match self {
            Value::IntList(items) => {
                if let Some(idx) = resolve_single_idx(key, items.len()) {
                    if let Value::Int(v) = value {
                        Arc::make_mut(items)[idx] = v;
                    } else {
                        let mut list = promote_ints(items);
                        list[idx] = value;
                        *self = Value::List(Arc::new(list));
                    }
                    return Some(());
                }
                let idxs = resolve_many_idx(key, items.len())?;
                match value {
                    Value::Int(v) => {
                        for idx in idxs {
                            Arc::make_mut(items)[idx] = v;
                        }
                    }
                    Value::IntList(vals) => {
                        if idxs.len() != vals.len() {
                            return None;
                        }
                        for (idx, val) in idxs.into_iter().zip(vals.iter().copied()) {
                            Arc::make_mut(items)[idx] = val;
                        }
                    }
                    Value::IntRange(vals) => {
                        if idxs.len() != vals.len() {
                            return None;
                        }
                        for (idx, val) in idxs.into_iter().zip(vals.iter()) {
                            Arc::make_mut(items)[idx] = val;
                        }
                    }
                    Value::List(vals) => {
                        if idxs.len() != vals.len() {
                            return None;
                        }
                        if let Some(ints) = collect_exact_ints(&vals) {
                            for (idx, val) in idxs.into_iter().zip(ints) {
                                Arc::make_mut(items)[idx] = val;
                            }
                        } else {
                            let mut list = promote_ints(items);
                            for (idx, val) in idxs.into_iter().zip(vals.iter().cloned()) {
                                list[idx] = val;
                            }
                            *self = Value::List(Arc::new(list));
                        }
                    }
                    atom => {
                        let mut list = promote_ints(items);
                        for idx in idxs {
                            list[idx] = atom.clone();
                        }
                        *self = Value::List(Arc::new(list));
                    }
                }
                Some(())
            }
            Value::String(s) => {
                let len = s.chars().count();
                if let Some(idx) = resolve_single_idx(key, len) {
                    match value {
                        Value::Char(c) => {
                            assign_string_char(Arc::make_mut(s), idx, c);
                        }
                        other => {
                            let mut list: Vec<Value> = s.chars().map(Value::Char).collect();
                            list[idx] = other;
                            *self = Value::List(Arc::new(list));
                        }
                    }
                    return Some(());
                }
                let idxs = resolve_many_idx(key, len)?;
                match value {
                    Value::List(vals) => {
                        if idxs.len() != vals.len() {
                            return None;
                        }
                        if let Some(chars) = vals
                            .iter()
                            .map(|v| match v {
                                Value::Char(c) => Some(*c),
                                _ => None,
                            })
                            .collect::<Option<Vec<_>>>()
                        {
                            let s = Arc::make_mut(s);
                            for (byte_idx, ch) in idxs.into_iter().zip(chars) {
                                assign_string_char(s, byte_idx, ch);
                            }
                        } else {
                            let mut list: Vec<Value> = s.chars().map(Value::Char).collect();
                            for (idx, val) in idxs.into_iter().zip(vals.iter().cloned()) {
                                list[idx] = val;
                            }
                            *self = Value::List(Arc::new(list));
                        }
                    }
                    Value::Char(c) => {
                        let s = Arc::make_mut(s);
                        for byte_idx in idxs {
                            assign_string_char(s, byte_idx, c);
                        }
                    }
                    other => {
                        let mut list: Vec<Value> = s.chars().map(Value::Char).collect();
                        for idx in idxs {
                            list[idx] = other.clone();
                        }
                        *self = Value::List(Arc::new(list));
                    }
                }
                Some(())
            }
            Value::List(items) => {
                if let Some(idx) = resolve_single_idx(key, items.len()) {
                    Arc::make_mut(items)[idx] = value;
                    return Some(());
                }
                let idxs = resolve_many_idx(key, items.len())?;
                let items = Arc::make_mut(items);
                assign_list_bulk(items, idxs, value)
            }

            Value::Dict(map) => match key {
                Value::Tag(key_str) => {
                    Arc::make_mut(map).insert(key_str.clone(), value);
                    Some(())
                }
                key => {
                    if let Some(idx) = resolve_single_idx(key, map.len()) {
                        let (_, slot) = Arc::make_mut(map).get_index_mut(idx)?;
                        *slot = value;
                        return Some(());
                    }
                    match key {
                        Value::List(keys) => {
                            let keys = normalize_dict_bulk_keys(keys, map.len())?;
                            assign_dict_bulk(Arc::make_mut(map), keys, value)
                        }
                        Value::IntList(idxs) => {
                            let idxs = normalize_many(idxs.iter().copied(), map.len())?;
                            let keys = idxs.into_iter().map(DictBulkKey::Position).collect();
                            assign_dict_bulk(Arc::make_mut(map), keys, value)
                        }
                        Value::IntRange(idxs) => {
                            let idxs = normalize_many(idxs.iter(), map.len())?;
                            let keys = idxs.into_iter().map(DictBulkKey::Position).collect();
                            assign_dict_bulk(Arc::make_mut(map), keys, value)
                        }
                        _ => None,
                    }
                }
            },

            _ => None,
        }
    }
}

/// Resolve a single index from `Value::Int` or `Value::BigInt`.
fn resolve_single_idx(key: &Value, len: usize) -> Option<usize> {
    match key {
        Value::Int(i) => normalize_idx(*i, len),
        Value::BigInt(i) => normalize_idx(i.to_i64()?, len),
        _ => None,
    }
}

/// Convert possibly-negative i64 index to a valid usize for a sequence of
/// length `len`. Returns None if out-of-bounds or conversion fails.
fn normalize_idx(i: i64, len: usize) -> Option<usize> {
    if i >= 0 {
        usize::try_from(i).ok().filter(|&idx| idx < len)
    } else {
        // distance from the end; e.g. -1 => last element
        let off = usize::try_from(i.unsigned_abs()).ok()?;
        len.checked_sub(off) // None if off > len
    }
}

/// Replace the character at character-position `char_idx` in a `String` with
/// `new_char`. Handles UTF-8 byte boundary correctly.
fn assign_string_char(s: &mut String, char_idx: usize, new_char: char) {
    let byte_pos = s
        .char_indices()
        .nth(char_idx)
        .map(|(pos, _)| pos)
        .expect("char index must be valid");
    let old_char = s[byte_pos..].chars().next().expect("char must exist");
    let old_len = old_char.len_utf8();

    // Replace: remove old char bytes, insert new char at same position
    let tail = s[byte_pos + old_len..].to_owned();
    s.truncate(byte_pos);
    s.push(new_char);
    s.push_str(&tail);
}

/// Convert many i64 indices into `usize` indices, failing on the first bad one.
fn normalize_many(idxs: impl IntoIterator<Item = i64>, len: usize) -> Option<Vec<usize>> {
    let mut out = Vec::new();
    for i in idxs {
        out.push(normalize_idx(i, len)?);
    }
    Some(out)
}

fn normalize_list_indices(idxs: &[Value], len: usize) -> Option<Vec<usize>> {
    let raw = idxs
        .iter()
        .map(|v| match v {
            Value::Int(i) => Some(*i),
            Value::BigInt(b) => b.to_i64(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    normalize_many(raw, len)
}

/// Resolve bulk indices from `Value::IntList` or `Value::List`.
fn resolve_many_idx(key: &Value, len: usize) -> Option<Vec<usize>> {
    match key.bulk_index_key()? {
        BulkIndexKey::IntList(idxs) => normalize_many(idxs.iter().copied(), len),
        BulkIndexKey::IntRange(idxs) => normalize_many(idxs.iter(), len),
        BulkIndexKey::List(idxs) => normalize_list_indices(idxs, len),
    }
}

fn collect_exact_ints(values: &[Value]) -> Option<Vec<i64>> {
    values
        .iter()
        .map(|v| match v {
            Value::Int(i) => Some(*i),
            _ => None,
        })
        .collect()
}

fn promote_ints(items: &[i64]) -> Vec<Value> {
    items.iter().copied().map(Value::Int).collect()
}

fn assign_list_bulk(items: &mut [Value], idxs: Vec<usize>, value: Value) -> Option<()> {
    match value {
        Value::List(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            for (idx, val) in idxs.into_iter().zip(vals.iter().cloned()) {
                items[idx] = val;
            }
        }
        Value::IntList(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            for (idx, val) in idxs.into_iter().zip(vals.iter().copied()) {
                items[idx] = Value::Int(val);
            }
        }
        Value::IntRange(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            for (idx, val) in idxs.into_iter().zip(vals.iter()) {
                items[idx] = Value::Int(val);
            }
        }
        atom => {
            for idx in idxs {
                items[idx] = atom.clone();
            }
        }
    }
    Some(())
}

#[derive(Clone)]
enum DictBulkKey {
    Symbol(Arc<str>),
    Position(usize),
}

fn normalize_dict_bulk_keys(keys: &[Value], len: usize) -> Option<Vec<DictBulkKey>> {
    keys.iter()
        .map(|key| match key {
            Value::Tag(s) => Some(DictBulkKey::Symbol(s.clone())),
            Value::Int(i) => normalize_idx(*i, len).map(DictBulkKey::Position),
            Value::BigInt(b) => normalize_idx(b.to_i64()?, len).map(DictBulkKey::Position),
            _ => None,
        })
        .collect()
}

fn assign_dict_entry(
    map: &mut indexmap::IndexMap<Arc<str>, Value>,
    key: DictBulkKey,
    value: Value,
) -> Option<()> {
    match key {
        DictBulkKey::Symbol(symbol) => {
            map.insert(symbol, value);
            Some(())
        }
        DictBulkKey::Position(idx) => {
            let (_, slot) = map.get_index_mut(idx)?;
            *slot = value;
            Some(())
        }
    }
}

fn assign_dict_bulk(
    map: &mut indexmap::IndexMap<Arc<str>, Value>,
    keys: Vec<DictBulkKey>,
    value: Value,
) -> Option<()> {
    match value {
        Value::List(vals) => {
            if keys.len() != vals.len() {
                return None;
            }
            for (key, value) in keys.into_iter().zip(vals.iter().cloned()) {
                assign_dict_entry(map, key, value)?;
            }
        }
        Value::IntList(vals) => {
            if keys.len() != vals.len() {
                return None;
            }
            for (key, value) in keys.into_iter().zip(vals.iter().copied()) {
                assign_dict_entry(map, key, Value::Int(value))?;
            }
        }
        Value::IntRange(vals) => {
            if keys.len() != vals.len() {
                return None;
            }
            for (key, value) in keys.into_iter().zip(vals.iter()) {
                assign_dict_entry(map, key, Value::Int(value))?;
            }
        }
        atom => {
            for key in keys {
                assign_dict_entry(map, key, atom.clone())?;
            }
        }
    }
    Some(())
}

// in-place mutation ================================================

pub(crate) fn insert_in_place(
    data: &mut Value,
    xs: &Value,
    dsts: Option<&Value>,
) -> WqResult<Value> {
    materialize_int_range(data);
    if data.is_string_like() {
        insert_string_in_place(data, dsts, xs)?;
        return Ok(data.clone());
    }

    match data {
        Value::List(items) => {
            let (positions, is_multi) = parse_insert_positions(dsts, items.len())?;
            if positions.is_empty() {
                return Ok(data.clone());
            }

            if is_multi {
                let values = list_insert_pairwise(xs, positions.len())?;
                let base = std::mem::take(Arc::make_mut(items));
                *items = Arc::new(insert_many_owned(
                    base,
                    positions.into_iter().zip(values).collect(),
                ));
            } else {
                let idx = positions[0];
                let mut tail = Arc::make_mut(items).split_off(idx);
                Arc::make_mut(items).extend(list_insert_items(xs));
                Arc::make_mut(items).append(&mut tail);
            }
            Ok(data.clone())
        }
        Value::IntList(items) => {
            let (positions, is_multi) = parse_insert_positions(dsts, items.len())?;
            if positions.is_empty() {
                return Ok(data.clone());
            }

            if is_multi {
                if let Some(values) = exact_int_insert_pairwise(xs, positions.len()) {
                    let base = std::mem::take(Arc::make_mut(items));
                    *items = Arc::new(insert_many_owned(
                        base,
                        positions.into_iter().zip(values).collect::<Vec<_>>(),
                    ));
                    return Ok(data.clone());
                }

                let values = list_insert_pairwise(xs, positions.len())?;
                let base = std::mem::take(Arc::make_mut(items))
                    .into_iter()
                    .map(Value::Int)
                    .collect::<Vec<_>>();
                *data = Value::List(Arc::new(insert_many_owned(
                    base,
                    positions.into_iter().zip(values).collect(),
                )));
            } else {
                let idx = positions[0];
                if let Some(values) = exact_int_insert_items(xs) {
                    let mut tail = Arc::make_mut(items).split_off(idx);
                    Arc::make_mut(items).extend(values);
                    Arc::make_mut(items).append(&mut tail);
                    return Ok(data.clone());
                }

                let values = list_insert_items(xs);
                let mut base = std::mem::take(Arc::make_mut(items))
                    .into_iter()
                    .map(Value::Int)
                    .collect::<Vec<_>>();
                let mut tail = base.split_off(idx);
                base.extend(values);
                base.append(&mut tail);
                *data = Value::List(Arc::new(base));
            }
            Ok(data.clone())
        }
        Value::Dict(map) => {
            match dsts {
                Some(dsts @ Value::Dict(_)) => {
                    let destinations = dict_insert_destinations(dsts, map.len())?;
                    if destinations.is_empty() {
                        return Ok(data.clone());
                    }
                    let values = dict_insert_values(xs, destinations.len())?;
                    let ops = destinations
                        .into_iter()
                        .zip(values)
                        .map(|((idx, key), value)| (idx, key, value))
                        .collect::<Vec<_>>();
                    dict_shift_insert_many(Arc::make_mut(map), ops);
                }
                _ => {
                    let (positions, is_multi) = parse_insert_positions(dsts, map.len())?;
                    if positions.is_empty() {
                        return Ok(data.clone());
                    }

                    if is_multi {
                        let entries = dict_insert_entries(xs, positions.len())?;
                        let ops = positions
                            .into_iter()
                            .zip(entries)
                            .map(|(idx, (key, value))| (idx, key, value))
                            .collect::<Vec<_>>();
                        dict_shift_insert_many(Arc::make_mut(map), ops);
                    } else {
                        let idx = positions[0];
                        let entries = match xs {
                            Value::Dict(entries) => entries
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone()))
                                .collect::<Vec<_>>(),
                            _ => {
                                return Err(WqError::new(WqErrorType::Domain)
                                    .msg("expected dict insert value"));
                            }
                        };
                        let ops = entries
                            .into_iter()
                            .map(|(key, value)| (idx, key, value))
                            .collect::<Vec<_>>();
                        dict_shift_insert_many(Arc::make_mut(map), ops);
                    }
                }
            }
            Ok(data.clone())
        }

        other => Err(WqError::new(WqErrorType::Domain)
            .msg("expected string, list, or dict")
            .got1(other)),
    }
}

pub(crate) fn parse_pop_count(arg: &Value) -> WqResult<usize> {
    match arg {
        Value::Int(n) if *n >= 0 => Ok(*n as usize),
        _ => Err(WqError::new(WqErrorType::Domain).msg("count must be non-negative int")),
    }
}

pub(crate) fn pop_in_place(data: &mut Value, n: usize) -> WqResult<Value> {
    materialize_int_range(data);
    if n == 0 {
        return Ok(Value::unit());
    }

    // Direct String handling — avoid List<Char> round-trip allocation.
    if let Value::String(s) = data {
        let chars: Vec<char> = s.chars().collect();
        if n >= chars.len() {
            let removed = std::mem::replace(s, Arc::new(String::new()));
            return Ok(Value::String(removed));
        }
        let split = chars.len() - n;
        let remaining: String = chars[..split].iter().collect();
        let removed_str: String = chars[split..].iter().collect();
        *data = Value::String(Arc::new(remaining));
        if n == 1 {
            return Ok(Value::Char(removed_str.chars().next().unwrap()));
        }
        return Ok(Value::String(Arc::new(removed_str)));
    }

    Ok(match data {
        Value::IntList(items) => {
            let split = items.len().saturating_sub(n);
            let removed = Arc::make_mut(items).split_off(split);
            if n == 1 {
                removed
                    .into_iter()
                    .next()
                    .map(Value::Int)
                    .unwrap_or_else(Value::unit)
            } else {
                Value::IntList(Arc::new(removed))
            }
        }
        Value::List(items) => {
            let split = items.len().saturating_sub(n);
            let removed = Arc::make_mut(items).split_off(split);
            if n == 1 {
                removed.into_iter().next().unwrap_or_else(Value::unit)
            } else {
                Value::from_items(removed)
            }
        }
        Value::Dict(map) => {
            let take = n.min(map.len());
            let mut removed = Vec::with_capacity(take);
            for _ in 0..take {
                let idx = map.len() - 1;
                let (_, value) = Arc::make_mut(map)
                    .shift_remove_index(idx)
                    .expect("dict index should exist");
                removed.push(value);
            }
            removed.reverse();
            if n == 1 {
                removed.into_iter().next().unwrap_or_else(Value::unit)
            } else {
                Value::from_items(removed)
            }
        }

        atom => {
            let popped = atom.clone();
            *atom = Value::unit();
            popped
        }
    })
}

pub(crate) fn remove_in_place(data: &mut Value, idx: &Value) -> WqResult<Value> {
    materialize_int_range(data);
    // Direct String handling — avoid List<Char> round-trip allocation.
    if let Value::String(s) = data {
        let chars: Vec<char> = s.chars().collect();
        let (positions, is_multi) = parse_remove_positions(idx, chars.len())?;
        if is_multi {
            ensure_unique_positions(&positions, "remove indices")?;
            let removed_str: String = positions.iter().map(|&i| chars[i]).collect();
            let mut remaining = chars;
            let mut sorted = positions;
            sorted.sort_unstable_by(|a, b| b.cmp(a));
            for i in sorted {
                remaining.remove(i);
            }
            *data = Value::String(Arc::new(remaining.iter().collect()));
            return Ok(Value::String(Arc::new(removed_str)));
        } else {
            let i = positions[0];
            let removed_char = chars[i];
            let mut remaining = chars;
            remaining.remove(i);
            *data = Value::String(Arc::new(remaining.iter().collect()));
            return Ok(Value::Char(removed_char));
        }
    }
    match data {
        Value::IntList(items) => {
            let (positions, is_multi) = parse_remove_positions(idx, items.len())?;
            if is_multi {
                ensure_unique_positions(&positions, "remove indices")?;
                let removed = positions.iter().map(|&i| items[i]).collect::<Vec<_>>();
                let mut sorted = positions;
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                for idx in sorted {
                    Arc::make_mut(items).remove(idx);
                }
                Ok(Value::IntList(Arc::new(removed)))
            } else {
                Ok(Value::Int(Arc::make_mut(items).remove(positions[0])))
            }
        }
        Value::List(items) => {
            let (positions, is_multi) = parse_remove_positions(idx, items.len())?;
            if is_multi {
                ensure_unique_positions(&positions, "remove indices")?;
                let removed = positions
                    .iter()
                    .map(|&i| items[i].clone())
                    .collect::<Vec<_>>();
                let mut sorted = positions;
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                for idx in sorted {
                    Arc::make_mut(items).remove(idx);
                }
                Ok(Value::List(Arc::new(removed)))
            } else {
                Ok(Arc::make_mut(items).remove(positions[0]))
            }
        }
        Value::Dict(map) => {
            let (keys, is_multi) = dict_remove_keys(idx, map)?;
            let mut dedup = keys.clone();
            dedup.sort();
            if dedup.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(WqError::new(WqErrorType::Domain).msg("duplicate remove keys"));
            }
            let mut removed = Vec::with_capacity(keys.len());
            for key in keys {
                let value = Arc::make_mut(map)
                    .shift_remove(&key)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
                removed.push(value);
            }
            if is_multi {
                Ok(Value::from_items(removed))
            } else {
                Ok(removed.into_iter().next().unwrap_or_else(Value::unit))
            }
        }

        atom => {
            let (positions, is_multi) = parse_remove_positions(idx, 1)?;
            if is_multi {
                ensure_unique_positions(&positions, "remove indices")?;
                let removed = positions.iter().map(|_| atom.clone()).collect::<Vec<_>>();
                *atom = Value::unit();
                Ok(Value::from_items(removed))
            } else {
                let removed = atom.clone();
                *atom = Value::unit();
                Ok(removed)
            }
        }
    }
}

fn list_insert_items(xs: &Value) -> Vec<Value> {
    match xs {
        Value::List(items) => items.iter().cloned().collect(),
        Value::IntList(items) => items.iter().copied().map(Value::Int).collect(),
        Value::IntRange(items) => items.iter().map(Value::Int).collect(),
        other => vec![other.clone()],
    }
}

fn list_insert_pairwise(xs: &Value, len: usize) -> WqResult<Vec<Value>> {
    match xs {
        Value::List(items) if items.len() == len => Ok(items.iter().cloned().collect()),
        Value::IntList(items) if items.len() == len => {
            Ok(items.iter().copied().map(Value::Int).collect())
        }
        Value::IntRange(items) if items.len() == len => Ok(items.iter().map(Value::Int).collect()),
        // Broadcast a single (non-container) value to all positions.
        _ if !matches!(xs, Value::List(_) | Value::IntList(_) | Value::IntRange(_)) => {
            Ok(vec![xs.clone(); len])
        }
        _ => Err(WqError::new(WqErrorType::Domain)
            .msg("dsts and xs must have matching lengths for pairwise insert")),
    }
}

fn exact_int_insert_items(xs: &Value) -> Option<Vec<i64>> {
    match xs {
        Value::Int(i) => Some(vec![*i]),
        Value::BigInt(b) => b.to_i64().map(|i| vec![i]),
        Value::IntList(items) => Some(items.iter().copied().collect()),
        Value::IntRange(items) => Some(items.to_vec()),
        Value::List(items) => items.iter().map(int_arg_to_i64).collect(),
        _ => None,
    }
}

fn exact_int_insert_pairwise(xs: &Value, len: usize) -> Option<Vec<i64>> {
    match xs {
        Value::Int(i) => Some(vec![*i; len]),
        Value::BigInt(b) => b.to_i64().map(|i| vec![i; len]),
        Value::IntList(items) if items.len() == len => Some(items.iter().copied().collect()),
        Value::IntRange(items) if items.len() == len => Some(items.to_vec()),
        Value::List(items) if items.len() == len => items.iter().map(int_arg_to_i64).collect(),
        _ => None,
    }
}

fn string_insert_pairwise(xs: &Value, len: usize) -> WqResult<Vec<String>> {
    match xs {
        Value::List(items) if items.len() == len => items
            .iter()
            .map(Value::to_rust_string_with_note)
            .collect::<WqResult<Vec<_>>>(),
        _ if !matches!(xs, Value::List(_) | Value::IntList(_) | Value::IntRange(_)) => {
            Ok(vec![xs.to_rust_string_with_note()?; len])
        }
        other => {
            let s = other.to_rust_string_with_note()?;
            let chars = s.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
            if chars.len() == len {
                Ok(chars)
            } else {
                Err(WqError::new(WqErrorType::Domain)
                    .msg("dsts and xs must have matching lengths for pairwise insert"))
            }
        }
    }
}

fn insert_string_in_place(data: &mut Value, dsts: Option<&Value>, xs: &Value) -> WqResult<()> {
    let s = data.to_rust_string_with_note()?;
    let chars = s.chars().collect::<Vec<_>>();
    let (positions, is_multi) = parse_insert_positions(dsts, chars.len())?;
    if positions.is_empty() {
        return Ok(());
    }
    let updated = if is_multi {
        let values = string_insert_pairwise(xs, positions.len())?;
        insert_string_chunks(&chars, positions.into_iter().zip(values).collect())
    } else {
        let idx = positions[0];
        let insert_text = xs.to_rust_string_with_note()?;
        insert_string_chunks(&chars, vec![(idx, insert_text)])
    };
    *data = updated.into_wq_value();
    Ok(())
}

fn dict_insert_entries(xs: &Value, len: usize) -> WqResult<Vec<(Arc<str>, Value)>> {
    match xs {
        Value::Dict(entries) if entries.len() == len => Ok(entries
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()),
        _ => Err(WqError::new(WqErrorType::Domain)
            .msg("dsts and xs must have matching lengths for pairwise insert")),
    }
}

fn dict_insert_values(xs: &Value, len: usize) -> WqResult<Vec<Value>> {
    list_insert_pairwise(xs, len)
}

fn dict_insert_destinations(dsts: &Value, len: usize) -> WqResult<Vec<(usize, Arc<str>)>> {
    match dsts {
        Value::Dict(entries) => entries
            .iter()
            .map(|(key, idx)| {
                let raw = int_arg_to_i64(idx)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid insert index"))?;
                let idx = normalize_insert_idx(raw, len)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid insert index"))?;
                Ok((idx, key.clone()))
            })
            .collect(),
        _ => Err(WqError::new(WqErrorType::Domain).msg("expected dict<tag,int> destination")),
    }
}

fn dict_shift_insert_many(
    map: &mut IndexMap<Arc<str>, Value>,
    mut ops: Vec<(usize, Arc<str>, Value)>,
) {
    ops.sort_by_key(|(idx, _, _)| *idx);
    for (offset, (idx, key, value)) in ops.into_iter().enumerate() {
        map.shift_insert(idx + offset, key, value);
    }
}

fn dict_remove_keys(
    idx: &Value,
    map: &indexmap::IndexMap<Arc<str>, Value>,
) -> WqResult<(Vec<Arc<str>>, bool)> {
    match idx {
        Value::Tag(key) => {
            if map.contains_key(key.as_ref()) {
                Ok((vec![key.clone()], false))
            } else {
                Err(WqError::new(WqErrorType::Domain).msg("invalid remove key"))
            }
        }
        Value::Int(i) => {
            let pos = normalize_remove_idx(*i, map.len())
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
            let (key, _) = map
                .get_index(pos)
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
            Ok((vec![key.clone()], false))
        }
        Value::BigInt(b) => {
            let pos = normalize_remove_idx(
                b.to_i64()
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?,
                map.len(),
            )
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
            let (key, _) = map
                .get_index(pos)
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
            Ok((vec![key.clone()], false))
        }
        Value::IntList(idxs) => {
            let mut keys = Vec::with_capacity(idxs.len());
            for &idx in idxs.iter() {
                let pos = normalize_remove_idx(idx, map.len())
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
                let (key, _) = map
                    .get_index(pos)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
                keys.push(key.clone());
            }
            Ok((keys, true))
        }
        Value::IntRange(idxs) => {
            let mut keys = Vec::with_capacity(idxs.len());
            for idx in idxs.iter() {
                let pos = normalize_remove_idx(idx, map.len())
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
                let (key, _) = map
                    .get_index(pos)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove key"))?;
                keys.push(key.clone());
            }
            Ok((keys, true))
        }
        Value::List(items) => {
            let mut keys: Vec<Arc<str>> = Vec::with_capacity(items.len());
            for item in items.iter() {
                match item {
                    Value::Tag(key) => {
                        if !map.contains_key(key.as_ref()) {
                            return Err(WqError::new(WqErrorType::Domain).msg("invalid remove key"));
                        }
                        keys.push(key.clone());
                    }
                    Value::Int(i) => {
                        let pos = normalize_remove_idx(*i, map.len()).ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("invalid remove key")
                        })?;
                        let (key, _) = map.get_index(pos).ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("invalid remove key")
                        })?;
                        keys.push(key.clone());
                    }
                    Value::BigInt(b) => {
                        let pos = normalize_remove_idx(
                            b.to_i64().ok_or_else(|| {
                                WqError::new(WqErrorType::Domain).msg("invalid remove key")
                            })?,
                            map.len(),
                        )
                        .ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("invalid remove key")
                        })?;
                        let (key, _) = map.get_index(pos).ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("invalid remove key")
                        })?;
                        keys.push(key.clone());
                    }
                    _ => {
                        return Err(
                            WqError::new(WqErrorType::Domain).msg("expected tag or int dict key")
                        );
                    }
                }
            }
            Ok((keys, true))
        }
        _ => Err(WqError::new(WqErrorType::Domain).msg("expected tag, int, or list key")),
    }
}

fn normalize_remove_idx(i: i64, len: usize) -> Option<usize> {
    if i >= 0 {
        usize::try_from(i).ok().filter(|&idx| idx < len)
    } else {
        let off = usize::try_from(i.unsigned_abs()).ok()?;
        len.checked_sub(off)
    }
}

fn normalize_insert_idx(i: i64, len: usize) -> Option<usize> {
    if i >= 0 {
        usize::try_from(i).ok().filter(|&idx| idx <= len)
    } else {
        let off = usize::try_from(i.unsigned_abs()).ok()?;
        len.checked_sub(off)
    }
}

fn int_arg_to_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Int(i) => Some(*i),
        Value::BigInt(b) => b.to_i64(),
        _ => None,
    }
}

fn parse_remove_positions(idx: &Value, len: usize) -> WqResult<(Vec<usize>, bool)> {
    match idx {
        Value::Int(i) => normalize_remove_idx(*i, len)
            .map(|idx| (vec![idx], false))
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove index")),
        Value::BigInt(b) => normalize_remove_idx(
            b.to_i64()
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove index"))?,
            len,
        )
        .map(|idx| (vec![idx], false))
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove index")),
        Value::IntList(idxs) => {
            let mut out = Vec::with_capacity(idxs.len());
            for &idx in idxs.iter() {
                out.push(normalize_remove_idx(idx, len).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain).msg("invalid remove index")
                })?);
            }
            Ok((out, true))
        }
        Value::IntRange(idxs) => {
            let mut out = Vec::with_capacity(idxs.len());
            for idx in idxs.iter() {
                out.push(normalize_remove_idx(idx, len).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain).msg("invalid remove index")
                })?);
            }
            Ok((out, true))
        }
        Value::List(idxs) => {
            let mut out = Vec::with_capacity(idxs.len());
            for idx in idxs.iter() {
                let raw = int_arg_to_i64(idx)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid remove index"))?;
                out.push(normalize_remove_idx(raw, len).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain).msg("invalid remove index")
                })?);
            }
            Ok((out, true))
        }
        _ => Err(WqError::new(WqErrorType::Domain).msg("expected int or list<int> index")),
    }
}

fn ensure_unique_positions(positions: &[usize], what: &str) -> WqResult<()> {
    let mut sorted = positions.to_vec();
    sorted.sort_unstable();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(WqError::new(WqErrorType::Domain).msg(format!("duplicate {what}")));
    }
    Ok(())
}

fn parse_insert_positions(dsts: Option<&Value>, len: usize) -> WqResult<(Vec<usize>, bool)> {
    match dsts {
        None => Ok(((1..len).collect(), true)),
        Some(Value::Int(i)) => normalize_insert_idx(*i, len)
            .map(|idx| (vec![idx], false))
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid insert index")),
        Some(Value::BigInt(b)) => normalize_insert_idx(
            b.to_i64()
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid insert index"))?,
            len,
        )
        .map(|idx| (vec![idx], false))
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid insert index")),
        Some(Value::IntList(idxs)) => {
            let mut out = Vec::with_capacity(idxs.len());
            for &idx in idxs.iter() {
                out.push(normalize_insert_idx(idx, len).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain).msg("invalid insert index")
                })?);
            }
            Ok((out, true))
        }
        Some(Value::IntRange(idxs)) => {
            let mut out = Vec::with_capacity(idxs.len());
            for idx in idxs.iter() {
                out.push(normalize_insert_idx(idx, len).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain).msg("invalid insert index")
                })?);
            }
            Ok((out, true))
        }
        Some(Value::List(idxs)) => {
            let mut out = Vec::with_capacity(idxs.len());
            for idx in idxs.iter() {
                let raw = int_arg_to_i64(idx)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid insert index"))?;
                out.push(normalize_insert_idx(raw, len).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain).msg("invalid insert index")
                })?);
            }
            Ok((out, true))
        }
        Some(_) => {
            Err(WqError::new(WqErrorType::Domain).msg("expected int or list<int> destination"))
        }
    }
}

fn insert_many_owned<T>(base: Vec<T>, mut ops: Vec<(usize, T)>) -> Vec<T> {
    ops.sort_by_key(|(idx, _)| *idx);
    let base_len = base.len();
    let mut out = Vec::with_capacity(base_len + ops.len());
    let mut base_iter = base.into_iter();
    let mut ops_iter = ops.into_iter().peekable();
    for i in 0..=base_len {
        while matches!(ops_iter.peek(), Some((idx, _)) if *idx == i) {
            out.push(ops_iter.next().expect("insert op should exist").1);
        }
        if i < base_len {
            out.push(base_iter.next().expect("base element should exist"));
        }
    }
    out
}

fn insert_string_chunks(base: &[char], mut ops: Vec<(usize, String)>) -> String {
    ops.sort_by_key(|(idx, _)| *idx);
    let mut out = String::new();
    let mut next = 0usize;
    for i in 0..=base.len() {
        while next < ops.len() && ops[next].0 == i {
            out.push_str(&ops[next].1);
            next += 1;
        }
        if i < base.len() {
            out.push(base[i]);
        }
    }
    out
}

fn materialize_int_range(value: &mut Value) {
    if let Value::IntRange(range) = value {
        *value = Value::IntList(Arc::new(range.to_vec()));
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use num_bigint::BigInt;

    use super::*;

    fn sample_str() -> Value {
        Value::List(Arc::new(vec![
            Value::Char('a'),
            Value::Char('b'),
            Value::Char('c'),
        ]))
    }

    fn sample_dict() -> Value {
        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("b".into(), Value::Int(2));
        Value::Dict(Arc::new(map))
    }

    #[test]
    fn str_index_single_returns_char() {
        let s = sample_str();
        let idx = Value::Int(1);
        let result = s.index(&idx).expect("index within bounds");
        assert_eq!(result, Value::Char('b'));
    }

    #[test]
    fn list_bulk_assign_broadcasts_atom() {
        let mut list = Value::List(Arc::new(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]));

        assert_eq!(
            list.assign_by_index(&Value::IntList(Arc::new(vec![0, 2])), Value::Bool(true)),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Int(1),
                Value::Bool(true),
                Value::Int(3),
            ]))
        );
    }

    #[test]
    fn list_bulk_assign_is_atomic_on_bad_index() {
        let mut list = Value::List(Arc::new(vec![Value::Int(0), Value::Int(1), Value::Int(2)]));
        let original = list.clone();

        assert_eq!(
            list.assign_by_index(&Value::IntList(Arc::new(vec![0, 9])), Value::Int(42)),
            None
        );
        assert_eq!(list, original);
    }

    #[test]
    fn intlist_bulk_assign_stays_intlist_for_exact_int_values() {
        let mut list = Value::IntList(Arc::new(vec![10, 20, 30, 40]));

        assert_eq!(
            list.assign_by_index(
                &Value::List(Arc::new(vec![Value::Int(1), Value::Int(3)])),
                Value::List(Arc::new(vec![Value::Int(99), Value::Int(77)])),
            ),
            Some(())
        );
        assert_eq!(list, Value::IntList(Arc::new(vec![10, 99, 30, 77])));
    }

    #[test]
    fn intlist_bulk_assign_promotes_for_non_int_values() {
        let mut list = Value::IntList(Arc::new(vec![10, 20, 30]));

        assert_eq!(
            list.assign_by_index(
                &Value::IntList(Arc::new(vec![0, 2])),
                Value::List(Arc::new(vec![Value::float(1.5), Value::Int(7)])),
            ),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(Arc::new(vec![
                Value::float(1.5),
                Value::Int(20),
                Value::Int(7)
            ]))
        );
    }

    #[test]
    fn int_range_assign_materializes_before_mutating() {
        let mut list = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(10, 2, 3)));

        assert_eq!(list.assign_by_index(&Value::Int(1), Value::Int(99)), Some(()));
        assert_eq!(list, Value::IntList(Arc::new(vec![10, 99, 14])));
    }

    #[test]
    fn dict_bulk_assign_supports_pairwise_intlist_rhs() {
        let mut dict = sample_dict();

        assert_eq!(
            dict.assign_by_index(
                &Value::List(Arc::new(vec![Value::Tag("a".into()), Value::Int(1)])),
                Value::IntList(Arc::new(vec![7, 9]))
            ),
            Some(())
        );
        assert_eq!(dict.index(&Value::Tag("a".into())), Some(Value::Int(7)));
        assert_eq!(dict.index(&Value::Tag("b".into())), Some(Value::Int(9)));
    }

    #[test]
    fn dict_bulk_assign_is_atomic_on_invalid_key() {
        let mut dict = sample_dict();
        let original = dict.clone();

        assert_eq!(
            dict.assign_by_index(
                &Value::List(Arc::new(vec![Value::Tag("c".into()), Value::Bool(true)])),
                Value::Int(5),
            ),
            None
        );
        assert_eq!(dict, original);
    }

    // ── String indexing and mutation tests ──

    #[test]
    fn string_index_single() {
        let s = crate::value::into_wq_string("hello");
        assert_eq!(s.index(&Value::Int(0)), Some(Value::Char('h')));
        assert_eq!(s.index(&Value::Int(4)), Some(Value::Char('o')));
        assert_eq!(s.index(&Value::Int(-1)), Some(Value::Char('o')));
        assert_eq!(s.index(&Value::Int(5)), None);
    }

    #[test]
    fn string_index_multi() {
        let s = crate::value::into_wq_string("hello");
        let keys = Value::List(Arc::new(vec![Value::Int(1), Value::Int(3)]));
        let result = s.index(&keys);
        assert_eq!(result, Some(Value::String(Arc::new("el".to_owned()))));
    }

    #[test]
    fn string_assign_single() {
        let mut s = crate::value::into_wq_string("hello");
        assert_eq!(
            s.assign_by_index(&Value::Int(1), Value::Char('a')),
            Some(())
        );
        assert_eq!(s.index(&Value::Int(1)), Some(Value::Char('a')));
        assert_eq!(s.to_string(), "\"hallo\"");
    }

    #[test]
    fn string_assign_same_width_utf8() {
        let mut s = crate::value::into_wq_string("hell");
        // 'a' (1 byte) → 'ñ' (2 bytes) — different widths
        assert_eq!(
            s.assign_by_index(&Value::Int(1), Value::Char('ñ')),
            Some(())
        );
        assert_eq!(s.to_string(), "\"hñll\"");
        assert_eq!(s.index(&Value::Int(1)), Some(Value::Char('ñ')));
    }

    #[test]
    fn string_assign_cow() {
        let s = crate::value::into_wq_string("hello");
        let mut s2 = s.clone();
        // s2 shares the same Arc<String>
        assert_eq!(
            s2.assign_by_index(&Value::Int(0), Value::Char('j')),
            Some(())
        );
        // Original should be unchanged (CoW)
        assert_eq!(s.to_string(), "\"hello\"");
        assert_eq!(s2.to_string(), "\"jello\"");
    }

    #[test]
    fn string_assign_multi() {
        let mut s = crate::value::into_wq_string("hello");
        let keys = Value::IntList(Arc::new(vec![1, 3]));
        let vals = Value::List(Arc::new(vec![Value::Char('a'), Value::Char('b')]));
        assert_eq!(s.assign_by_index(&keys, vals), Some(()));
        // h e l l o → h a l b o
        assert_eq!(s.to_string(), "\"halbo\"");
    }

    #[test]
    fn string_assign_non_char_promotes_to_list() {
        let mut s = crate::value::into_wq_string("hi");
        assert_eq!(s.assign_by_index(&Value::Int(0), Value::Int(42)), Some(()));
        assert_eq!(
            s,
            Value::List(Arc::new(vec![Value::Int(42), Value::Char('i')]))
        );
    }

    #[test]
    fn string_assign_bulk_mixed_promotes_to_list() {
        let mut s = crate::value::into_wq_string("hello");
        let keys = Value::IntList(Arc::new(vec![1, 3]));
        let vals = Value::List(Arc::new(vec![Value::Char('a'), Value::Int(99)]));
        assert_eq!(s.assign_by_index(&keys, vals), Some(()));
        assert_eq!(
            s,
            Value::List(Arc::new(vec![
                Value::Char('h'),
                Value::Char('a'),
                Value::Char('l'),
                Value::Int(99),
                Value::Char('o'),
            ]))
        );
    }

    #[test]
    fn string_assign_broadcast_atom_promotes_to_list() {
        let mut s = crate::value::into_wq_string("abc");
        let keys = Value::IntList(Arc::new(vec![0, 2]));
        assert_eq!(s.assign_by_index(&keys, Value::Bool(true)), Some(()));
        assert_eq!(
            s,
            Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Char('b'),
                Value::Bool(true),
            ]))
        );
    }

    #[test]
    fn index_many_list() {
        let list = Value::List(Arc::new(vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
        ]));
        let keys = vec![Value::Int(0), Value::Int(2)];
        assert_eq!(
            list.index_many(&keys),
            Some(Value::IntList(Arc::new(vec![10, 30])))
        );
    }

    #[test]
    fn index_many_dict_mixed_keys() {
        let dict = sample_dict();
        let keys = vec![Value::Tag("a".into()), Value::Int(1)];
        assert_eq!(
            dict.index_many(&keys),
            Some(Value::IntList(Arc::new(vec![1, 2])))
        );
    }

    #[test]
    fn complex_index_by_tag() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        assert_eq!(z.index(&Value::Tag("re".into())), Some(Value::float(3.0)));
        assert_eq!(z.index(&Value::Tag("im".into())), Some(Value::float(4.0)));
    }

    #[test]
    fn fraction_index_by_tag() {
        let f = Value::from_fraction_parts(BigInt::from(3), BigInt::from(4));
        assert_eq!(f.index(&Value::Tag("n".into())), Some(Value::Int(3)));
        assert_eq!(f.index(&Value::Tag("d".into())), Some(Value::Int(4)));
    }
}
