use crate::value::{IntoWqValue, Value};

impl Value {
    /// Returns the uniform shape as a vector of dimensions.
    /// Returns None if the value is not uniformly shaped (ragged arrays).
    /// For internal use - returns usize directly without conversion overhead.
    pub fn shape_uniform(&self) -> Option<Vec<usize>> {
        match self {
            Value::IntList(items) => Some(vec![items.len()]),
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
                    // all values in the dict must have the same shape
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
            v if v.is_atom() => Some(vec![]),
            _ => {
                eprintln!("unexpected value at shape_vec {self:?}");
                Some(vec![])
            }
        }
    }

    /// Returns the shape as a Value (IntList for uniform arrays, scalar for ragged).
    /// This is the user-facing API for the shape() builtin function.
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
            Value::IntList(_) => 1,
            Value::List(items) => {
                if items.is_empty() {
                    1
                } else {
                    let mut max_child = 0i64;
                    for it in items {
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
}
