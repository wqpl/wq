use crate::value::Value;

use num_traits::ToPrimitive;

/// Convert possibly-negative i64 index to a valid usize for a sequence of length `len`.
/// Returns None if out-of-bounds or conversion fails.
fn normalize_idx(i: i64, len: usize) -> Option<usize> {
    if i >= 0 {
        usize::try_from(i).ok().filter(|&idx| idx < len)
    } else {
        // distance from the end; e.g. -1 => last element
        let off = usize::try_from(i.unsigned_abs()).ok()?;
        len.checked_sub(off) // None if off > len
    }
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
            for (idx, val) in idxs.into_iter().zip(vals) {
                items[idx] = val;
            }
        }
        Value::IntList(vals) => {
            if idxs.len() != vals.len() {
                return None;
            }
            for (idx, val) in idxs.into_iter().zip(vals) {
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
    Symbol(String),
    Position(usize),
}

fn normalize_dict_bulk_keys(keys: &[Value], len: usize) -> Option<Vec<DictBulkKey>> {
    keys.iter()
        .map(|key| match key {
            Value::Symbol(s) => Some(DictBulkKey::Symbol(s.clone())),
            Value::Int(i) => normalize_idx(*i, len).map(DictBulkKey::Position),
            Value::BigInt(b) => normalize_idx(b.to_i64()?, len).map(DictBulkKey::Position),
            _ => None,
        })
        .collect()
}

fn assign_dict_entry(
    map: &mut indexmap::IndexMap<String, Value>,
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
    map: &mut indexmap::IndexMap<String, Value>,
    keys: Vec<DictBulkKey>,
    value: Value,
) -> Option<()> {
    match value {
        Value::List(vals) => {
            if keys.len() != vals.len() {
                return None;
            }
            for (key, value) in keys.into_iter().zip(vals) {
                assign_dict_entry(map, key, value)?;
            }
        }
        Value::IntList(vals) => {
            if keys.len() != vals.len() {
                return None;
            }
            for (key, value) in keys.into_iter().zip(vals) {
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

/// Gather helper: map `indices` into cloned values from `items`.
fn gather<T: Clone>(items: &[T], indices: &[usize]) -> Option<Vec<T>> {
    let mut out = Vec::with_capacity(indices.len());
    for &idx in indices {
        out.push(items.get(idx)?.clone());
    }
    Some(out)
}

impl Value {
    /// Index into a list or dict
    pub fn index(&self, key: &Value) -> Option<Value> {
        match (self, key) {
            // atom[x] ================================================================
            (atom, Value::Int(i)) if atom.is_atom() => normalize_idx(*i, 1).map(|_| atom.clone()),
            (atom, Value::BigInt(i)) if atom.is_atom() => {
                normalize_idx(i.to_i64()?, 1).map(|_| atom.clone())
            }
            // list[x] ================================================================
            (Value::List(items), Value::Int(i)) => {
                let idx = normalize_idx(*i, items.len())?;
                items.get(idx).cloned()
            }
            (Value::List(items), Value::BigInt(i)) => {
                let idx = normalize_idx(i.to_i64()?, items.len())?;
                items.get(idx).cloned()
            }
            (Value::IntList(items), Value::Int(i)) => {
                let idx = normalize_idx(*i, items.len())?;
                items.get(idx).copied().map(Value::Int)
            }
            (Value::IntList(items), Value::BigInt(i)) => {
                let idx = normalize_idx(i.to_i64()?, items.len())?;
                items.get(idx).copied().map(Value::Int)
            }

            // list[list] ================================================================
            (Value::List(items), Value::List(idxs)) => {
                let idxs = normalize_list_indices(idxs, items.len())?;
                gather(items, &idxs).map(Value::from_items)
            }
            (Value::List(items), Value::IntList(idxs)) => {
                let idxs = normalize_many(idxs.iter().copied(), items.len())?;
                gather(items, &idxs).map(Value::from_items)
            }
            (Value::IntList(items), Value::List(idxs)) => {
                let idxs = normalize_list_indices(idxs, items.len())?;
                let mut out = Vec::with_capacity(idxs.len());
                for &idx in &idxs {
                    out.push(*items.get(idx)?);
                }
                Some(Value::IntList(out))
            }
            (Value::IntList(items), Value::IntList(idxs)) => {
                let idxs = normalize_many(idxs.iter().copied(), items.len())?;
                let mut out = Vec::with_capacity(idxs.len());
                for &idx in &idxs {
                    out.push(*items.get(idx)?);
                }
                Some(Value::IntList(out))
            }

            // dict[x] ================================================================
            (Value::Dict(map), Value::Int(i)) => {
                let idx = normalize_idx(*i, map.len())?;
                map.get_index(idx).map(|(_, v)| v.clone())
            }
            (Value::Dict(map), Value::BigInt(i)) => {
                let idx = normalize_idx(i.to_i64()?, map.len())?;
                map.get_index(idx).map(|(_, v)| v.clone())
            }
            (Value::Dict(map), Value::IntList(idxs)) => {
                let idxs = normalize_many(idxs.iter().copied(), map.len())?;
                let mut result = Vec::with_capacity(idxs.len());
                for &idx in &idxs {
                    let (_, v) = map.get_index(idx)?;
                    result.push(v.clone());
                }
                Some(Value::from_items(result))
            }
            (Value::Dict(map), Value::Symbol(key)) => map.get(key).cloned(),
            (Value::Dict(map), Value::List(keys)) => {
                let mut result = Vec::with_capacity(keys.len());
                for k in keys {
                    match k {
                        Value::Symbol(s) => result.push(map.get(s)?.clone()),
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
        }
    }

    /// Mutate list or dict element by index/key. Returns `Some(())` on success
    /// and `None` if the key does not exist or the types are incompatible.
    pub fn assign_by_index(&mut self, key: &Value, value: Value) -> Option<()> {
        match self {
            Value::List(items) => match key {
                Value::Int(i) => {
                    let idx = normalize_idx(*i, items.len())?;
                    items[idx] = value;
                    Some(())
                }
                Value::BigInt(b) => {
                    let idx = normalize_idx(b.to_i64()?, items.len())?;
                    items[idx] = value;
                    Some(())
                }
                Value::IntList(idxs) => {
                    let idxs = normalize_many(idxs.iter().copied(), items.len())?;
                    assign_list_bulk(items, idxs, value)
                }
                Value::List(idxs) => {
                    let idxs = normalize_list_indices(idxs, items.len())?;
                    assign_list_bulk(items, idxs, value)
                }
                _ => None,
            },
            Value::IntList(items) => match key {
                Value::Int(i) => {
                    let idx = normalize_idx(*i, items.len())?;
                    if let Value::Int(v) = value {
                        items[idx] = v;
                    } else {
                        let mut list = promote_ints(items);
                        list[idx] = value;
                        *self = Value::List(list);
                    }
                    Some(())
                }
                Value::BigInt(b) => {
                    let idx = normalize_idx(b.to_i64()?, items.len())?;
                    if let Value::Int(v) = value {
                        items[idx] = v;
                    } else {
                        let mut list = promote_ints(items);
                        list[idx] = value;
                        *self = Value::List(list);
                    }
                    Some(())
                }
                Value::IntList(idxs) => {
                    let idxs = normalize_many(idxs.iter().copied(), items.len())?;
                    match value {
                        Value::Int(v) => {
                            for idx in idxs {
                                items[idx] = v;
                            }
                        }
                        Value::IntList(vals) => {
                            if idxs.len() != vals.len() {
                                return None;
                            }
                            for (idx, val) in idxs.into_iter().zip(vals) {
                                items[idx] = val;
                            }
                        }
                        Value::List(vals) => {
                            if idxs.len() != vals.len() {
                                return None;
                            }
                            if let Some(ints) = collect_exact_ints(&vals) {
                                for (idx, val) in idxs.into_iter().zip(ints) {
                                    items[idx] = val;
                                }
                            } else {
                                let mut list = promote_ints(items);
                                for (idx, val) in idxs.into_iter().zip(vals) {
                                    list[idx] = val;
                                }
                                *self = Value::List(list);
                            }
                        }
                        atom => {
                            let mut list = promote_ints(items);
                            for idx in idxs {
                                list[idx] = atom.clone();
                            }
                            *self = Value::List(list);
                        }
                    }
                    Some(())
                }
                Value::List(idxs) => {
                    let idxs = normalize_list_indices(idxs, items.len())?;
                    match value {
                        Value::Int(v) => {
                            for idx in idxs {
                                items[idx] = v;
                            }
                        }
                        Value::IntList(vals) => {
                            if idxs.len() != vals.len() {
                                return None;
                            }
                            for (idx, val) in idxs.into_iter().zip(vals) {
                                items[idx] = val;
                            }
                        }
                        Value::List(vals) => {
                            if idxs.len() != vals.len() {
                                return None;
                            }
                            if let Some(ints) = collect_exact_ints(&vals) {
                                for (idx, val) in idxs.into_iter().zip(ints) {
                                    items[idx] = val;
                                }
                            } else {
                                let mut list = promote_ints(items);
                                for (idx, val) in idxs.into_iter().zip(vals) {
                                    list[idx] = val;
                                }
                                *self = Value::List(list);
                            }
                        }
                        atom => {
                            let mut list = promote_ints(items);
                            for idx in idxs {
                                list[idx] = atom.clone();
                            }
                            *self = Value::List(list);
                        }
                    }
                    Some(())
                }
                _ => None,
            },
            Value::Dict(map) => match key {
                Value::Symbol(key_str) => {
                    map.insert(key_str.clone(), value);
                    Some(())
                }
                Value::Int(i) => {
                    let idx = normalize_idx(*i, map.len())?;
                    let (_, slot) = map.get_index_mut(idx)?;
                    *slot = value;
                    Some(())
                }
                Value::BigInt(i) => {
                    let idx = normalize_idx(i.to_i64()?, map.len())?;
                    let (_, slot) = map.get_index_mut(idx)?;
                    *slot = value;
                    Some(())
                }
                Value::List(keys) => {
                    let keys = normalize_dict_bulk_keys(keys, map.len())?;
                    assign_dict_bulk(map, keys, value)
                }
                Value::IntList(idxs) => {
                    let idxs = normalize_many(idxs.iter().copied(), map.len())?;
                    let keys = idxs.into_iter().map(DictBulkKey::Position).collect();
                    assign_dict_bulk(map, keys, value)
                }
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn sample_str() -> Value {
        Value::List(vec![Value::Char('a'), Value::Char('b'), Value::Char('c')])
    }

    fn sample_dict() -> Value {
        let mut map = IndexMap::new();
        map.insert("a".to_string(), Value::Int(1));
        map.insert("b".to_string(), Value::Int(2));
        Value::Dict(Box::new(map))
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
        let mut list = Value::List(vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]);

        assert_eq!(
            list.assign_by_index(&Value::IntList(vec![0, 2]), Value::Bool(true)),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(vec![
                Value::Bool(true),
                Value::Int(1),
                Value::Bool(true),
                Value::Int(3),
            ])
        );
    }

    #[test]
    fn list_bulk_assign_is_atomic_on_bad_index() {
        let mut list = Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)]);
        let original = list.clone();

        assert_eq!(
            list.assign_by_index(&Value::IntList(vec![0, 9]), Value::Int(42)),
            None
        );
        assert_eq!(list, original);
    }

    #[test]
    fn intlist_bulk_assign_stays_intlist_for_exact_int_values() {
        let mut list = Value::IntList(vec![10, 20, 30, 40]);

        assert_eq!(
            list.assign_by_index(
                &Value::List(vec![Value::Int(1), Value::Int(3)]),
                Value::List(vec![Value::Int(99), Value::Int(77)]),
            ),
            Some(())
        );
        assert_eq!(list, Value::IntList(vec![10, 99, 30, 77]));
    }

    #[test]
    fn intlist_bulk_assign_promotes_for_non_int_values() {
        let mut list = Value::IntList(vec![10, 20, 30]);

        assert_eq!(
            list.assign_by_index(
                &Value::IntList(vec![0, 2]),
                Value::List(vec![Value::Float(1.5), Value::Int(7)]),
            ),
            Some(())
        );
        assert_eq!(
            list,
            Value::List(vec![Value::Float(1.5), Value::Int(20), Value::Int(7)])
        );
    }

    #[test]
    fn dict_bulk_assign_supports_pairwise_intlist_rhs() {
        let mut dict = sample_dict();

        assert_eq!(
            dict.assign_by_index(
                &Value::List(vec![Value::Symbol("a".into()), Value::Int(1)]),
                Value::IntList(vec![7, 9])
            ),
            Some(())
        );
        assert_eq!(dict.index(&Value::Symbol("a".into())), Some(Value::Int(7)));
        assert_eq!(dict.index(&Value::Symbol("b".into())), Some(Value::Int(9)));
    }

    #[test]
    fn dict_bulk_assign_is_atomic_on_invalid_key() {
        let mut dict = sample_dict();
        let original = dict.clone();

        assert_eq!(
            dict.assign_by_index(
                &Value::List(vec![Value::Symbol("c".into()), Value::Bool(true)]),
                Value::Int(5),
            ),
            None
        );
        assert_eq!(dict, original);
    }
}
