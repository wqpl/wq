use std::sync::Arc;

use indexmap::IndexMap;
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use ordered_float::OrderedFloat;

use crate::value::seq::{ExactIntSeq, ListStorageSeq, ValueSeq};
use crate::value::{IntoWqValue as _, Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

impl Value {
    pub(crate) fn bulk_index_key(&self) -> Option<()> {
        matches!(
            self,
            Value::IntList(_)
                | Value::IntRange(_)
                | Value::FloatList(_)
                | Value::BoolList(_)
                | Value::List(_)
        )
        .then_some(())
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

            Value::Complex(z) => index_structured_keys(keys, |key| index_complex_key(z, key)),
            Value::Fraction(fd) => index_structured_keys(keys, |key| index_fraction_key(fd, key)),

            // Fallback to single-index path =========================================
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
                Value::Tag(_) => index_complex_key(z, key),
                Value::List(keys) => {
                    let mut result = Vec::with_capacity(keys.len());
                    for k in keys.iter() {
                        result.push(index_complex_key(z, k)?);
                    }
                    Some(Value::from_items(result))
                }
                _ => None,
            },

            // fraction[x] ============================================================
            (Value::Fraction(fd), key) => match key {
                Value::Tag(_) => index_fraction_key(fd, key),
                Value::List(keys) => {
                    let mut result = Vec::with_capacity(keys.len());
                    for k in keys.iter() {
                        result.push(index_fraction_key(fd, k)?);
                    }
                    Some(Value::from_items(result))
                }
                _ => None,
            },

            _ => None,
        }
    }

    /// Mutate by multiple raw index keys without first materializing them as a
    /// list key. Returns `Some(())` on success and `None` on incompatible keys.
    pub(crate) fn assign_by_indices(&mut self, keys: &[Value], value: Value) -> Option<()> {
        match keys {
            [] => return None,
            [key] => return self.assign_by_index(key, value),
            _ => {}
        }

        materialize_int_range(self);
        if let Some(len) = packed_list_len(self) {
            let idxs = normalize_list_indices(keys, len)?;
            return assign_packed_list_indices(self, idxs, PackedAssignMode::Bulk, value);
        }

        match self {
            Value::String(s) => {
                let idxs = normalize_list_indices(keys, s.chars().count())?;
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
                let idxs = normalize_list_indices(keys, items.len())?;
                let items = Arc::make_mut(items);
                assign_list_bulk(items, idxs, value)
            }
            Value::Dict(map) => {
                let keys = normalize_dict_bulk_keys(keys, map.len())?;
                assign_dict_bulk(Arc::make_mut(map), keys, value)
            }
            _ => None,
        }
    }

    /// Mutate list or dict element by index/key. Returns `Some(())` on success
    /// and `None` if the key does not exist or the types are incompatible.
    pub(crate) fn assign_by_index(&mut self, key: &Value, value: Value) -> Option<()> {
        materialize_int_range(self);
        if let Some(len) = packed_list_len(self) {
            if let Some(idx) = resolve_single_idx(key, len) {
                return assign_packed_list_indices(
                    self,
                    vec![idx],
                    PackedAssignMode::Single,
                    value,
                );
            }
            let idxs = resolve_many_idx(key, len)?;
            return assign_packed_list_indices(self, idxs, PackedAssignMode::Bulk, value);
        }

        match self {
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
                        key => {
                            let idxs = key.exact_int_seq()?;
                            let idxs = normalize_many(idxs.iter(), map.len())?;
                            let keys = idxs.into_iter().map(DictBulkKey::Position).collect();
                            assign_dict_bulk(Arc::make_mut(map), keys, value)
                        }
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
        .map(int_arg_to_i64)
        .collect::<Option<Vec<_>>>()?;
    normalize_many(raw, len)
}

fn index_structured_keys(
    keys: &[Value],
    mut index_key: impl FnMut(&Value) -> Option<Value>,
) -> Option<Value> {
    match keys {
        [] => None,
        [key] => index_key(key),
        keys => {
            let mut result = Vec::with_capacity(keys.len());
            for key in keys {
                result.push(index_key(key)?);
            }
            Some(Value::from_items(result))
        }
    }
}

fn index_complex_key(z: &num_complex::Complex64, key: &Value) -> Option<Value> {
    match key {
        Value::Tag(s) if s.as_ref() == "re" => Some(Value::float(z.re)),
        Value::Tag(s) if s.as_ref() == "im" => Some(Value::float(z.im)),
        _ => None,
    }
}

fn index_fraction_key(fd: &num_rational::Ratio<BigInt>, key: &Value) -> Option<Value> {
    match key {
        Value::Tag(s) if matches!(s.as_ref(), "n" | "numer") => {
            Some(Value::from_bigint(fd.numer().clone()))
        }
        Value::Tag(s) if matches!(s.as_ref(), "d" | "denom") => {
            Some(Value::from_bigint(fd.denom().clone()))
        }
        _ => None,
    }
}

/// Resolve bulk indices from an exact-int sequence.
fn resolve_many_idx(key: &Value, len: usize) -> Option<Vec<usize>> {
    normalize_many(key.exact_int_seq()?.iter(), len)
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

fn collect_bools(values: &[Value]) -> Option<Vec<bool>> {
    values
        .iter()
        .map(|v| match v {
            Value::Bool(b) => Some(*b),
            _ => None,
        })
        .collect()
}

fn collect_floats(values: &[Value]) -> Option<Vec<OrderedFloat<f64>>> {
    values
        .iter()
        .map(|v| match v {
            Value::Float(f) => Some(*f),
            _ => None,
        })
        .collect()
}

fn promote_ints(items: &[i64]) -> Vec<Value> {
    items.iter().copied().map(Value::Int).collect()
}

fn promote_floats(items: &[OrderedFloat<f64>]) -> Vec<Value> {
    items.iter().copied().map(Value::Float).collect()
}

fn promote_bools(items: &[bool]) -> Vec<Value> {
    items.iter().copied().map(Value::Bool).collect()
}

enum PackedListAssignment {
    KeepPacked,
    Promote(Vec<Value>),
}

#[derive(Clone, Copy)]
enum PackedAssignMode {
    Single,
    Bulk,
}

enum PackedListMut<'a> {
    Int(&'a mut Arc<Vec<i64>>),
    Float(&'a mut Arc<Vec<OrderedFloat<f64>>>),
    Bool(&'a mut Arc<Vec<bool>>),
}

impl<'a> PackedListMut<'a> {
    fn from_value(value: &'a mut Value) -> Option<Self> {
        match value {
            Value::IntList(items) => Some(Self::Int(items)),
            Value::FloatList(items) => Some(Self::Float(items)),
            Value::BoolList(items) => Some(Self::Bool(items)),
            _ => None,
        }
    }

    fn assign_indices(
        &mut self,
        idxs: Vec<usize>,
        mode: PackedAssignMode,
        value: Value,
    ) -> Option<PackedListAssignment> {
        match self {
            Self::Int(items) => assign_int_list_indices(items, idxs, mode, value),
            Self::Float(items) => assign_float_list_indices(items, idxs, mode, value),
            Self::Bool(items) => assign_bool_list_indices(items, idxs, mode, value),
        }
    }
}

fn packed_list_len(value: &Value) -> Option<usize> {
    match value {
        Value::IntList(items) => Some(items.len()),
        Value::FloatList(items) => Some(items.len()),
        Value::BoolList(items) => Some(items.len()),
        _ => None,
    }
}

fn assign_packed_list_indices(
    data: &mut Value,
    idxs: Vec<usize>,
    mode: PackedAssignMode,
    value: Value,
) -> Option<()> {
    if idxs.is_empty() {
        return None;
    }
    let assignment = {
        let mut target = PackedListMut::from_value(data)?;
        target.assign_indices(idxs, mode, value)?
    };
    match assignment {
        PackedListAssignment::KeepPacked => {}
        PackedListAssignment::Promote(items) => {
            *data = Value::List(Arc::new(items));
        }
    }
    Some(())
}

fn assign_int_list_indices(
    items: &mut Arc<Vec<i64>>,
    idxs: Vec<usize>,
    mode: PackedAssignMode,
    value: Value,
) -> Option<PackedListAssignment> {
    if matches!(mode, PackedAssignMode::Single) {
        let idx = idxs
            .into_iter()
            .next()
            .expect("single packed-list assignment has one index");
        return match value {
            Value::Int(v) => {
                Arc::make_mut(items)[idx] = v;
                Some(PackedListAssignment::KeepPacked)
            }
            value => {
                let mut list = promote_ints(items);
                list[idx] = value;
                Some(PackedListAssignment::Promote(list))
            }
        };
    }

    match value {
        Value::Int(v) => {
            let items = Arc::make_mut(items);
            for idx in idxs {
                items[idx] = v;
            }
            Some(PackedListAssignment::KeepPacked)
        }
        Value::List(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            if let Some(ints) = collect_exact_ints(&vals) {
                let items = Arc::make_mut(items);
                for (idx, val) in idxs.into_iter().zip(ints) {
                    items[idx] = val;
                }
                Some(PackedListAssignment::KeepPacked)
            } else {
                let mut list = promote_ints(items);
                for (idx, val) in idxs.into_iter().zip(vals.iter().cloned()) {
                    list[idx] = val;
                }
                Some(PackedListAssignment::Promote(list))
            }
        }
        atom => {
            if let Some(vals) = atom.packed_int_seq() {
                assign_exact_int_seq_to_int_list(items, idxs, vals)?;
                Some(PackedListAssignment::KeepPacked)
            } else {
                let mut list = promote_ints(items);
                for idx in idxs {
                    list[idx] = atom.clone();
                }
                Some(PackedListAssignment::Promote(list))
            }
        }
    }
}

fn assign_bool_list_indices(
    items: &mut Arc<Vec<bool>>,
    idxs: Vec<usize>,
    mode: PackedAssignMode,
    value: Value,
) -> Option<PackedListAssignment> {
    if matches!(mode, PackedAssignMode::Single) {
        let idx = idxs
            .into_iter()
            .next()
            .expect("single packed-list assignment has one index");
        return match value {
            Value::Bool(v) => {
                Arc::make_mut(items)[idx] = v;
                Some(PackedListAssignment::KeepPacked)
            }
            value => {
                let mut list = promote_bools(items);
                list[idx] = value;
                Some(PackedListAssignment::Promote(list))
            }
        };
    }

    match value {
        Value::Bool(v) => {
            let items = Arc::make_mut(items);
            for idx in idxs {
                items[idx] = v;
            }
            Some(PackedListAssignment::KeepPacked)
        }
        Value::BoolList(vals) => {
            assign_bool_slice_to_bool_list(items, idxs, vals.as_slice())?;
            Some(PackedListAssignment::KeepPacked)
        }
        Value::List(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            if let Some(bools) = collect_bools(&vals) {
                let items = Arc::make_mut(items);
                for (idx, val) in idxs.into_iter().zip(bools) {
                    items[idx] = val;
                }
                Some(PackedListAssignment::KeepPacked)
            } else {
                let mut list = promote_bools(items);
                for (idx, val) in idxs.into_iter().zip(vals.iter().cloned()) {
                    list[idx] = val;
                }
                Some(PackedListAssignment::Promote(list))
            }
        }
        atom => {
            let mut list = promote_bools(items);
            for idx in idxs {
                list[idx] = atom.clone();
            }
            Some(PackedListAssignment::Promote(list))
        }
    }
}

fn assign_float_list_indices(
    items: &mut Arc<Vec<OrderedFloat<f64>>>,
    idxs: Vec<usize>,
    mode: PackedAssignMode,
    value: Value,
) -> Option<PackedListAssignment> {
    if matches!(mode, PackedAssignMode::Single) {
        let idx = idxs
            .into_iter()
            .next()
            .expect("single packed-list assignment has one index");
        return match value {
            Value::Float(v) => {
                Arc::make_mut(items)[idx] = v;
                Some(PackedListAssignment::KeepPacked)
            }
            value => {
                let mut list = promote_floats(items);
                list[idx] = value;
                Some(PackedListAssignment::Promote(list))
            }
        };
    }

    match value {
        Value::Float(v) => {
            let items = Arc::make_mut(items);
            for idx in idxs {
                items[idx] = v;
            }
            Some(PackedListAssignment::KeepPacked)
        }
        Value::FloatList(vals) => {
            assign_float_slice_to_float_list(items, idxs, vals.as_slice())?;
            Some(PackedListAssignment::KeepPacked)
        }
        Value::List(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            if let Some(floats) = collect_floats(&vals) {
                let items = Arc::make_mut(items);
                for (idx, val) in idxs.into_iter().zip(floats) {
                    items[idx] = val;
                }
                Some(PackedListAssignment::KeepPacked)
            } else {
                let mut list = promote_floats(items);
                for (idx, val) in idxs.into_iter().zip(vals.iter().cloned()) {
                    list[idx] = val;
                }
                Some(PackedListAssignment::Promote(list))
            }
        }
        atom => {
            let mut list = promote_floats(items);
            for idx in idxs {
                list[idx] = atom.clone();
            }
            Some(PackedListAssignment::Promote(list))
        }
    }
}

fn assign_list_bulk(items: &mut [Value], idxs: Vec<usize>, value: Value) -> Option<()> {
    if let Some(values) = ListStorageSeq::from_value(&value) {
        if idxs.len() != values.len() {
            return None;
        }
        for (idx, val) in idxs.into_iter().zip(values.values()) {
            items[idx] = val;
        }
    } else {
        for idx in idxs {
            items[idx] = value.clone();
        }
    }
    Some(())
}

fn assign_exact_int_seq_to_int_list(
    items: &mut Arc<Vec<i64>>,
    idxs: Vec<usize>,
    vals: ExactIntSeq<'_>,
) -> Option<()> {
    if idxs.len() != vals.len() {
        return None;
    }
    let items = Arc::make_mut(items);
    for (idx, val) in idxs.into_iter().zip(vals.iter()) {
        items[idx] = val;
    }
    Some(())
}

fn assign_bool_slice_to_bool_list(
    items: &mut Arc<Vec<bool>>,
    idxs: Vec<usize>,
    vals: &[bool],
) -> Option<()> {
    if idxs.len() != vals.len() {
        return None;
    }
    let items = Arc::make_mut(items);
    for (idx, val) in idxs.into_iter().zip(vals.iter().copied()) {
        items[idx] = val;
    }
    Some(())
}

fn assign_float_slice_to_float_list(
    items: &mut Arc<Vec<OrderedFloat<f64>>>,
    idxs: Vec<usize>,
    vals: &[OrderedFloat<f64>],
) -> Option<()> {
    if idxs.len() != vals.len() {
        return None;
    }
    let items = Arc::make_mut(items);
    for (idx, val) in idxs.into_iter().zip(vals.iter().copied()) {
        items[idx] = val;
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
    if let Some(values) = ListStorageSeq::from_value(&value) {
        if keys.len() != values.len() {
            return None;
        }
        for (key, value) in keys.into_iter().zip(values.values()) {
            assign_dict_entry(map, key, value)?;
        }
    } else {
        for key in keys {
            assign_dict_entry(map, key, value.clone())?;
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
    if data.is_string() {
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
        Value::BoolList(items) => {
            let (positions, is_multi) = parse_insert_positions(dsts, items.len())?;
            if positions.is_empty() {
                return Ok(data.clone());
            }

            if is_multi {
                if let Some(values) = bool_insert_pairwise(xs, positions.len()) {
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
                    .map(Value::Bool)
                    .collect::<Vec<_>>();
                *data = Value::List(Arc::new(insert_many_owned(
                    base,
                    positions.into_iter().zip(values).collect(),
                )));
            } else {
                let idx = positions[0];
                if let Some(values) = bool_insert_items(xs) {
                    let mut tail = Arc::make_mut(items).split_off(idx);
                    Arc::make_mut(items).extend(values);
                    Arc::make_mut(items).append(&mut tail);
                    return Ok(data.clone());
                }

                let values = list_insert_items(xs);
                let mut base = std::mem::take(Arc::make_mut(items))
                    .into_iter()
                    .map(Value::Bool)
                    .collect::<Vec<_>>();
                let mut tail = base.split_off(idx);
                base.extend(values);
                base.append(&mut tail);
                *data = Value::List(Arc::new(base));
            }
            Ok(data.clone())
        }
        Value::FloatList(items) => {
            let (positions, is_multi) = parse_insert_positions(dsts, items.len())?;
            if positions.is_empty() {
                return Ok(data.clone());
            }

            if is_multi {
                if let Some(values) = float_insert_pairwise(xs, positions.len()) {
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
                    .map(Value::Float)
                    .collect::<Vec<_>>();
                *data = Value::List(Arc::new(insert_many_owned(
                    base,
                    positions.into_iter().zip(values).collect(),
                )));
            } else {
                let idx = positions[0];
                if let Some(values) = float_insert_items(xs) {
                    let mut tail = Arc::make_mut(items).split_off(idx);
                    Arc::make_mut(items).extend(values);
                    Arc::make_mut(items).append(&mut tail);
                    return Ok(data.clone());
                }

                let values = list_insert_items(xs);
                let mut base = std::mem::take(Arc::make_mut(items))
                    .into_iter()
                    .map(Value::Float)
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
        Value::Int(n) if *n >= 0 => usize::try_from(*n)
            .map_err(|_| WqError::new(WqErrorType::Domain).msg("count is too large")),
        _ => Err(WqError::new(WqErrorType::Domain).msg("count must be non-negative int")),
    }
}

pub(crate) fn pop_in_place(data: &mut Value, n: usize) -> WqResult<Value> {
    materialize_int_range(data);
    if n == 0 {
        return Ok(Value::unit());
    }

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
        Value::BoolList(items) => {
            let split = items.len().saturating_sub(n);
            let removed = Arc::make_mut(items).split_off(split);
            if n == 1 {
                removed
                    .into_iter()
                    .next()
                    .map(Value::Bool)
                    .unwrap_or_else(Value::unit)
            } else {
                Value::BoolList(Arc::new(removed))
            }
        }
        Value::FloatList(items) => {
            let split = items.len().saturating_sub(n);
            let removed = Arc::make_mut(items).split_off(split);
            if n == 1 {
                removed
                    .into_iter()
                    .next()
                    .map(Value::Float)
                    .unwrap_or_else(Value::unit)
            } else {
                Value::FloatList(Arc::new(removed))
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
        Value::BoolList(items) => {
            let (positions, is_multi) = parse_remove_positions(idx, items.len())?;
            if is_multi {
                ensure_unique_positions(&positions, "remove indices")?;
                let removed = positions.iter().map(|&i| items[i]).collect::<Vec<_>>();
                let mut sorted = positions;
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                for idx in sorted {
                    Arc::make_mut(items).remove(idx);
                }
                Ok(Value::BoolList(Arc::new(removed)))
            } else {
                Ok(Value::Bool(Arc::make_mut(items).remove(positions[0])))
            }
        }
        Value::FloatList(items) => {
            let (positions, is_multi) = parse_remove_positions(idx, items.len())?;
            if is_multi {
                ensure_unique_positions(&positions, "remove indices")?;
                let removed = positions.iter().map(|&i| items[i]).collect::<Vec<_>>();
                let mut sorted = positions;
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                for idx in sorted {
                    Arc::make_mut(items).remove(idx);
                }
                Ok(Value::FloatList(Arc::new(removed)))
            } else {
                Ok(Value::Float(Arc::make_mut(items).remove(positions[0])))
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
    ListStorageSeq::from_value(xs)
        .map(|items| items.to_values_vec())
        .unwrap_or_else(|| vec![xs.clone()])
}

fn list_insert_pairwise(xs: &Value, len: usize) -> WqResult<Vec<Value>> {
    if let Some(items) = ListStorageSeq::from_value(xs) {
        if items.len() == len {
            Ok(items.to_values_vec())
        } else {
            Err(WqError::new(WqErrorType::Domain)
                .msg("dsts and xs must have matching lengths for pairwise insert"))
        }
    } else {
        Ok(vec![xs.clone(); len])
    }
}

fn exact_int_insert_items(xs: &Value) -> Option<Vec<i64>> {
    xs.exact_int_seq().map(|items| items.to_vec())
}

fn exact_int_insert_pairwise(xs: &Value, len: usize) -> Option<Vec<i64>> {
    let items = xs.exact_int_seq()?;
    if items.is_atom() {
        let value = items
            .iter()
            .next()
            .expect("atom exact int sequence has one item");
        return Some(vec![value; len]);
    }
    (items.len() == len).then(|| items.to_vec())
}

fn bool_insert_items(xs: &Value) -> Option<Vec<bool>> {
    match xs {
        Value::Bool(b) => Some(vec![*b]),
        Value::BoolList(items) => Some(items.iter().copied().collect()),
        Value::List(items) => collect_bools(items),
        _ => None,
    }
}

fn bool_insert_pairwise(xs: &Value, len: usize) -> Option<Vec<bool>> {
    match xs {
        Value::Bool(b) => Some(vec![*b; len]),
        Value::BoolList(items) if items.len() == len => Some(items.iter().copied().collect()),
        Value::List(items) if items.len() == len => collect_bools(items),
        _ => None,
    }
}

fn float_insert_items(xs: &Value) -> Option<Vec<OrderedFloat<f64>>> {
    match xs {
        Value::Float(f) => Some(vec![*f]),
        Value::FloatList(items) => Some(items.iter().copied().collect()),
        Value::List(items) => collect_floats(items),
        _ => None,
    }
}

fn float_insert_pairwise(xs: &Value, len: usize) -> Option<Vec<OrderedFloat<f64>>> {
    match xs {
        Value::Float(f) => Some(vec![*f; len]),
        Value::FloatList(items) if items.len() == len => Some(items.iter().copied().collect()),
        Value::List(items) if items.len() == len => collect_floats(items),
        _ => None,
    }
}

fn string_insert_pairwise(xs: &Value, len: usize) -> WqResult<Vec<String>> {
    match xs {
        Value::List(items) if items.len() == len => items
            .iter()
            .map(Value::to_rust_string_with_note)
            .collect::<WqResult<Vec<_>>>(),
        _ if ListStorageSeq::from_value(xs).is_none() => {
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
    parse_exact_int_positions(
        idx,
        len,
        normalize_remove_idx,
        "invalid remove index",
        "expected int or list<int> index",
    )
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
        Some(dsts) => parse_exact_int_positions(
            dsts,
            len,
            normalize_insert_idx,
            "invalid insert index",
            "expected int or list<int> destination",
        ),
    }
}

fn parse_exact_int_positions(
    value: &Value,
    len: usize,
    normalize: fn(i64, usize) -> Option<usize>,
    invalid_msg: &'static str,
    expected_msg: &'static str,
) -> WqResult<(Vec<usize>, bool)> {
    let Some(items) = value.exact_int_seq() else {
        let msg = if matches!(value, Value::List(_)) {
            invalid_msg
        } else {
            expected_msg
        };
        return Err(WqError::new(WqErrorType::Domain).msg(msg));
    };

    let mut out = Vec::with_capacity(items.len());
    for idx in items.iter() {
        out.push(
            normalize(idx, len)
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg(invalid_msg))?,
        );
    }
    Ok((out, !items.is_atom()))
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
    fn list_bulk_assign_accepts_raw_index_keys() {
        let mut list = Value::List(Arc::new(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]));

        assert_eq!(
            list.assign_by_indices(&[Value::Int(0), Value::Int(2)], Value::Bool(true)),
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
    fn intlist_bulk_assign_accepts_raw_index_keys() {
        let mut list = Value::IntList(Arc::new(vec![10, 20, 30, 40]));

        assert_eq!(
            list.assign_by_indices(
                &[Value::Int(1), Value::Int(3)],
                Value::List(Arc::new(vec![Value::Int(99), Value::Int(77)])),
            ),
            Some(())
        );
        assert_eq!(list, Value::IntList(Arc::new(vec![10, 99, 30, 77])));
    }

    #[test]
    fn intlist_bulk_assign_accepts_packed_int_rhs() {
        let mut list = Value::IntList(Arc::new(vec![10, 20, 30, 40]));
        let values = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(7, 2, 2)));

        assert_eq!(
            list.assign_by_indices(&[Value::Int(1), Value::Int(3)], values),
            Some(())
        );
        assert_eq!(list, Value::IntList(Arc::new(vec![10, 7, 30, 9])));
    }

    #[test]
    fn intlist_one_item_bulk_assign_uses_pairwise_rhs() {
        let mut list = Value::IntList(Arc::new(vec![10, 20, 30]));
        let key = Value::List(Arc::new(vec![Value::Int(1)]));

        assert_eq!(
            list.assign_by_index(&key, Value::IntList(Arc::new(vec![99]))),
            Some(())
        );
        assert_eq!(list, Value::IntList(Arc::new(vec![10, 99, 30])));
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

        assert_eq!(
            list.assign_by_index(&Value::Int(1), Value::Int(99)),
            Some(())
        );
        assert_eq!(list, Value::IntList(Arc::new(vec![10, 99, 14])));
    }

    #[test]
    fn boollist_assignment_preserves_or_widens_storage() {
        let mut list = Value::BoolList(Arc::new(vec![true, false, true]));

        assert_eq!(
            list.assign_by_index(&Value::Int(1), Value::Bool(true)),
            Some(())
        );
        assert_eq!(list, Value::BoolList(Arc::new(vec![true, true, true])));

        assert_eq!(
            list.assign_by_index(&Value::Int(0), Value::Int(1)),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(Arc::new(vec![
                Value::Int(1),
                Value::Bool(true),
                Value::Bool(true)
            ]))
        );
    }

    #[test]
    fn boollist_bulk_assign_accepts_raw_index_keys() {
        let mut list = Value::BoolList(Arc::new(vec![true, false, true]));

        assert_eq!(
            list.assign_by_indices(&[Value::Int(0), Value::Int(2)], Value::Bool(false)),
            Some(())
        );
        assert_eq!(list, Value::BoolList(Arc::new(vec![false, false, false])));
    }

    #[test]
    fn boollist_one_item_bulk_assign_uses_pairwise_rhs() {
        let mut list = Value::BoolList(Arc::new(vec![true, false, true]));
        let key = Value::List(Arc::new(vec![Value::Int(1)]));

        assert_eq!(
            list.assign_by_index(&key, Value::BoolList(Arc::new(vec![true]))),
            Some(())
        );
        assert_eq!(list, Value::BoolList(Arc::new(vec![true, true, true])));
    }

    #[test]
    fn boollist_bulk_assign_promotes_pairwise_for_mixed_values() {
        let mut list = Value::BoolList(Arc::new(vec![true, false, true]));

        assert_eq!(
            list.assign_by_indices(
                &[Value::Int(0), Value::Int(2)],
                Value::List(Arc::new(vec![Value::Bool(false), Value::Int(7)])),
            ),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(Arc::new(vec![
                Value::Bool(false),
                Value::Bool(false),
                Value::Int(7),
            ]))
        );
    }

    #[test]
    fn floatlist_bulk_assign_preserves_or_widens_storage() {
        let mut list = Value::FloatList(Arc::new(vec![
            OrderedFloat(1.0),
            OrderedFloat(2.0),
            OrderedFloat(3.0),
        ]));

        assert_eq!(
            list.assign_by_indices(
                &[Value::Int(0), Value::Int(2)],
                Value::FloatList(Arc::new(vec![OrderedFloat(10.0), OrderedFloat(30.0)])),
            ),
            Some(())
        );
        assert_eq!(
            list,
            Value::FloatList(Arc::new(vec![
                OrderedFloat(10.0),
                OrderedFloat(2.0),
                OrderedFloat(30.0),
            ]))
        );

        assert_eq!(
            list.assign_by_index(&Value::Int(1), Value::Int(99)),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(Arc::new(vec![
                Value::float(10.0),
                Value::Int(99),
                Value::float(30.0),
            ]))
        );
    }

    #[test]
    fn floatlist_insert_pop_and_remove_preserve_storage() {
        let mut list = Value::FloatList(Arc::new(vec![OrderedFloat(1.0), OrderedFloat(4.0)]));

        assert_eq!(
            insert_in_place(
                &mut list,
                &Value::FloatList(Arc::new(vec![OrderedFloat(2.0), OrderedFloat(3.0)])),
                Some(&Value::Int(1)),
            ),
            Ok(Value::FloatList(Arc::new(vec![
                OrderedFloat(1.0),
                OrderedFloat(2.0),
                OrderedFloat(3.0),
                OrderedFloat(4.0),
            ])))
        );
        assert_eq!(pop_in_place(&mut list, 1), Ok(Value::float(4.0)));
        assert_eq!(
            remove_in_place(&mut list, &Value::IntList(Arc::new(vec![0, 2]))),
            Ok(Value::FloatList(Arc::new(vec![
                OrderedFloat(1.0),
                OrderedFloat(3.0),
            ])))
        );
        assert_eq!(list, Value::FloatList(Arc::new(vec![OrderedFloat(2.0)])));
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
    fn dict_bulk_assign_accepts_raw_index_keys() {
        let mut dict = sample_dict();

        assert_eq!(
            dict.assign_by_indices(
                &[Value::Tag("a".into()), Value::Int(1)],
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

    // String indexing and mutation tests

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
    fn string_assign_multi_accepts_raw_index_keys() {
        let mut s = crate::value::into_wq_string("hello");
        let vals = Value::List(Arc::new(vec![Value::Char('a'), Value::Char('b')]));
        assert_eq!(
            s.assign_by_indices(&[Value::Int(1), Value::Int(3)], vals),
            Some(())
        );
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
    fn plain_atom_indexing_is_invalid() {
        let atom = Value::Int(7);

        assert_eq!(atom.index(&Value::Int(0)), None);
        assert_eq!(atom.index(&Value::Tag("re".into())), None);
        assert_eq!(atom.index_many(&[Value::Int(0), Value::Int(1)]), None);
    }

    #[test]
    fn complex_index_by_tag() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        assert_eq!(z.index(&Value::Tag("re".into())), Some(Value::float(3.0)));
        assert_eq!(z.index(&Value::Tag("im".into())), Some(Value::float(4.0)));
    }

    #[test]
    fn complex_index_many_by_tags() {
        let z = Value::from_complex64(num_complex::Complex64::new(3.0, 4.0));
        let keys = [Value::Tag("re".into()), Value::Tag("im".into())];

        assert_eq!(
            z.index_many(&keys),
            Some(Value::FloatList(Arc::new(vec![
                OrderedFloat(3.0),
                OrderedFloat(4.0)
            ])))
        );
    }

    #[test]
    fn fraction_index_by_tag() {
        let f = Value::from_fraction_parts(BigInt::from(3), BigInt::from(4));
        assert_eq!(f.index(&Value::Tag("n".into())), Some(Value::Int(3)));
        assert_eq!(f.index(&Value::Tag("d".into())), Some(Value::Int(4)));
    }

    #[test]
    fn fraction_index_many_by_tags() {
        let f = Value::from_fraction_parts(BigInt::from(3), BigInt::from(4));
        let keys = [Value::Tag("n".into()), Value::Tag("d".into())];

        assert_eq!(
            f.index_many(&keys),
            Some(Value::IntList(Arc::new(vec![3, 4])))
        );
    }
}
