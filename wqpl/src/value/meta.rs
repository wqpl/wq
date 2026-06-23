use std::sync::Arc;

use crate::value::{IntoWqValue, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShapeMeta {
    Uniform(Vec<usize>),
    Ragged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValueMeta {
    pub(crate) len: usize,
    pub(crate) depth: i64,
    pub(crate) shape: ShapeMeta,
}

impl Value {
    pub fn strong_count(&self) -> Option<usize> {
        #[deny(clippy::wildcard_enum_match_arm)]
        let count = match self {
            Value::BigInt(v) => Arc::strong_count(v),
            Value::Tag(v) => Arc::strong_count(v),
            Value::Fraction(v) => Arc::strong_count(v),

            Value::Cas(v) => Arc::strong_count(v),
            Value::Algebraic(v) => Arc::strong_count(v),

            Value::IntList(v) => Arc::strong_count(v),
            Value::IntRange(v) => Arc::strong_count(v),
            Value::List(v) => Arc::strong_count(v),
            Value::String(v) => Arc::strong_count(v),
            Value::Dict(v) => Arc::strong_count(v),

            Value::CompiledFunction(v) => Arc::strong_count(v),
            Value::Closure(v) => Arc::strong_count(v),
            Value::LiftedCallable(v) => Arc::strong_count(v),

            Value::Stream(v) => Arc::strong_count(v),

            Value::Int(_)
            | Value::Float(_)
            | Value::Complex(_)
            | Value::Char(_)
            | Value::Bool(_)
            | Value::BuiltinFunction { .. } => {
                return None;
            }
        };
        Some(count)
    }

    /// Returns the uniform shape as a vector of dimensions.
    /// Returns None if the value is not uniformly shaped (ragged arrays).
    /// For internal use - returns usize directly without conversion overhead.
    pub(crate) fn shape_uniform(&self) -> Option<Vec<usize>> {
        match self {
            Value::IntList(items) => Some(vec![items.len()]),
            Value::IntRange(items) => Some(vec![items.len()]),
            Value::List(items) => {
                if items.is_empty() {
                    Some(vec![0])
                } else {
                    let first = items[0].shape_uniform()?;
                    for it in &items[1..] {
                        let s = it.shape_uniform()?;
                        if s != first {
                            return None;
                        }
                    }
                    let mut dims = Vec::with_capacity(first.len() + 1);
                    dims.push(items.len());
                    dims.extend(first);
                    Some(dims)
                }
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    Some(vec![0])
                } else {
                    let mut iter = map.values();
                    let first = iter.next().unwrap().shape_uniform()?;
                    for v in iter {
                        let s = v.shape_uniform()?;
                        if s != first {
                            return None;
                        }
                    }
                    let mut dims = Vec::with_capacity(first.len() + 1);
                    dims.push(map.len());
                    dims.extend(first);
                    Some(dims)
                }
            }
            Value::String(s) => Some(vec![s.chars().count()]),
            v if v.is_atom() => Some(vec![]),
            _ => {
                eprintln!("unexpected value at shape_vec {self:?}");
                Some(vec![])
            }
        }
    }

    /// Returns the shape as a Value (IntList for uniform arrays, scalar for
    /// ragged). This is the user-facing API for the shape() builtin
    /// function.
    pub fn shape(&self) -> Value {
        match self.shape_uniform() {
            Some(dims) => Value::from_items(dims.iter().map(|d| d.into_wq_value()).collect()),
            None => self.len().into_wq_value(),
        }
    }

    /// Returns the number of axes (rank) of the value.
    pub fn axes(&self) -> Value {
        match self.shape_uniform() {
            Some(dims) => dims.len().into_wq_value(),
            None => Value::Int(1),
        }
    }

    pub fn depth(&self) -> i64 {
        match self {
            Value::IntList(_) | Value::IntRange(_) => 1,
            Value::List(items) => {
                if items.is_empty() {
                    1
                } else {
                    let mut max_child = 0i64;
                    for it in items.iter() {
                        let d = it.depth();
                        if d > max_child {
                            max_child = d;
                        }
                    }
                    1 + max_child
                }
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    1
                } else {
                    let mut max_child = 0i64;
                    for (_, v) in map.iter() {
                        let d = v.depth();
                        if d > max_child {
                            max_child = d;
                        }
                    }
                    1 + max_child
                }
            }
            Value::String(_) => 1,
            v if v.is_atom() => 0,
            _ => {
                eprintln!("Unexpected value type in depth_of: {self:?}");
                0
            }
        }
    }

    pub fn is_uniform(&self) -> bool {
        self.shape_uniform().is_some()
    }

    pub(crate) fn display_meta(&self) -> ValueMeta {
        let shape = match self.shape_uniform() {
            Some(dims) => ShapeMeta::Uniform(dims),
            None => ShapeMeta::Ragged,
        };
        ValueMeta {
            len: self.len(),
            depth: self.depth(),
            shape,
        }
    }
}
