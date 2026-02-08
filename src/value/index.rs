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
                // All elements must be Int
                let idx_iter = idxs.iter().map(|v| match v {
                    Value::Int(i) => Some(*i),
                    Value::BigInt(b) => b.to_i64(),
                    _ => None,
                });
                // Fail on non-Int
                let mut raw = Vec::with_capacity(idxs.len());
                for maybe_i in idx_iter {
                    raw.push(maybe_i?);
                }
                let idxs = normalize_many(raw, items.len())?;
                gather(items, &idxs).map(Value::from_items)
            }
            (Value::List(items), Value::IntList(idxs)) => {
                let idxs = normalize_many(idxs.iter().copied(), items.len())?;
                gather(items, &idxs).map(Value::from_items)
            }
            (Value::IntList(items), Value::List(idxs)) => {
                let mut raw = Vec::with_capacity(idxs.len());
                for v in idxs {
                    match v {
                        Value::Int(i) => raw.push(*i),
                        Value::BigInt(b) => raw.push(b.to_i64()?),
                        _ => return None,
                    }
                }
                let idxs = normalize_many(raw, items.len())?;
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
            Value::List(items) => {
                let idx_val = match key {
                    Value::Int(i) => *i,
                    Value::BigInt(b) => b.to_i64()?,
                    _ => return None,
                };
                let idx = normalize_idx(idx_val, items.len())?;
                *items.get_mut(idx)? = value;
                Some(())
            }
            Value::IntList(items) => {
                let idx_val = match key {
                    Value::Int(i) => *i,
                    Value::BigInt(b) => b.to_i64()?,
                    _ => return None,
                };
                let idx = normalize_idx(idx_val, items.len())?;
                if let Value::Int(v) = value {
                    *items.get_mut(idx)? = v;
                    Some(())
                } else {
                    // Promote IntList -> List on type mismatch, preserving values
                    let mut list: Vec<Value> = items.iter().copied().map(Value::Int).collect();
                    *list.get_mut(idx)? = value;
                    *self = Value::List(list);
                    Some(())
                }
            }
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
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_str() -> Value {
        Value::List(vec![Value::Char('a'), Value::Char('b'), Value::Char('c')])
    }

    #[test]
    fn str_index_single_returns_char() {
        let s = sample_str();
        let idx = Value::Int(1);
        let result = s.index(&idx).expect("index within bounds");
        assert_eq!(result, Value::Char('b'));
    }
}
