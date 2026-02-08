use crate::{
    value::{Value, WqResult},
    wqerror::{WqError, WqErrorType},
};

use indexmap::IndexMap;

#[derive(Debug, Clone, PartialEq)]
pub enum BcError {
    Length {
        path: Vec<usize>,
        left: usize,
        right: usize,
    },
    Key {
        path: Vec<usize>,
        left: String,
        right: String,
    },
    Wq {
        path: Vec<usize>,
        wqerror: WqError,
    },
}

pub type BcResult<T> = Result<T, BcError>;

impl BcError {
    /// Prepend an index as error bubbles up.
    #[inline]
    pub fn at(mut self, idx: usize) -> Self {
        match &mut self {
            BcError::Length { path, .. } | BcError::Wq { path, .. } | BcError::Key { path, .. } => {
                path.insert(0, idx)
            }
        }
        self
    }
}

#[inline]
fn bc_len_mismatch(left: usize, right: usize, path: &[usize]) -> BcError {
    BcError::Length {
        path: path.to_vec(),
        left,
        right,
    }
}

/// Map a `Result<_, WqError>` (from `f`/`op`) into `BcErr::WqErr` with the current path.
trait WqToBc<T> {
    fn bc_at_path(self, path: &[usize]) -> BcResult<T>;
}

impl<T> WqToBc<T> for Result<T, WqError> {
    #[inline]
    fn bc_at_path(self, path: &[usize]) -> BcResult<T> {
        self.map_err(|wqerr| BcError::Wq {
            path: path.to_vec(),
            wqerror: wqerr,
        })
    }
}

impl BcError {
    pub fn into_wqerror(self) -> WqError {
        match self {
            BcError::Length { path, left, right } => {
                let e = WqError::new(WqErrorType::Length)
                    .msg(format!("length mismatch: left {}, right {}", left, right));
                if path.is_empty() {
                    return e;
                }
                let path_str: String = path
                    .iter()
                    .map(|i| format!("[{i}]"))
                    .collect::<Vec<_>>()
                    .join("");
                e.attach_note(format!("at {path_str}"))
            }
            BcError::Key { path, left, right } => {
                let e = WqError::new(WqErrorType::Length)
                    .msg(format!("key mismatch: left {}, right {}", left, right));
                if path.is_empty() {
                    return e;
                }
                let path_str: String = path
                    .iter()
                    .map(|i| format!("[{i}]"))
                    .collect::<Vec<_>>()
                    .join("");
                e.attach_note(format!("at {path_str}"))
            }
            BcError::Wq { path, wqerror } => {
                if path.is_empty() {
                    return wqerror;
                }
                let path_str: String = path
                    .iter()
                    .map(|i| format!("[{i}]"))
                    .collect::<Vec<_>>()
                    .join("");
                wqerror.attach_note(format!("at {path_str}"))
            }
        }
    }
}

impl Value {
    pub fn bc1<F>(&self, f: F) -> BcResult<Value>
    where
        F: FnMut(&Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        let mut f = f;
        self.bc1_until_with_path(&|_, _| false, &mut f, &mut path)
    }

    /// 1-arg broadcasting, stops when `is_leaf(self)` is true.
    pub fn bc1_until<F, P>(&self, is_leaf: P, f: F) -> BcResult<Value>
    where
        F: FnMut(&Value) -> WqResult<Value>,
        P: Fn(usize, &Value) -> bool,
    {
        let mut path = Vec::new();
        let mut f = f;
        self.bc1_until_with_path(&is_leaf, &mut f, &mut path)
    }

    fn bc1_until_with_path<F, P>(
        &self,
        is_leaf: &P,
        f: &mut F,
        path: &mut Vec<usize>,
    ) -> BcResult<Value>
    where
        F: FnMut(&Value) -> WqResult<Value>,
        P: Fn(usize, &Value) -> bool,
    {
        let depth_from_root = path.len();
        if is_leaf(depth_from_root, self) {
            return f(self).bc_at_path(path);
        }
        match self {
            Value::List(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, x) in a.iter().enumerate() {
                    path.push(i);
                    out.push(x.bc1_until_with_path(is_leaf, f, path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            Value::IntList(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, &x) in a.iter().enumerate() {
                    path.push(i);
                    out.push(f(&Value::Int(x)).bc_at_path(path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            Value::Dict(m) => {
                let mut out = IndexMap::with_capacity(m.len());
                for (i, (k, v)) in m.iter().enumerate() {
                    path.push(i);
                    let vv = v.bc1_until_with_path(is_leaf, f, path)?;
                    path.pop();
                    out.insert(k.clone(), vv);
                }
                Ok(Value::Dict(out))
            }
            _ => f(self).bc_at_path(path),
        }
    }

    pub fn bc2<F>(&self, other: &Value, mut op: F) -> BcResult<Value>
    where
        F: FnMut(&Value, &Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        self.bc2_until_with_path(other, &|_, _, _| false, &mut op, &mut path)
    }

    /// 2-arg broadcasting, stops when `is_leaf(self, other)` is true.
    pub fn bc2_until<F, P>(&self, other: &Value, is_leaf: P, mut op: F) -> BcResult<Value>
    where
        F: FnMut(&Value, &Value) -> WqResult<Value>,
        P: Fn(usize, &Value, &Value) -> bool,
    {
        let mut path = Vec::new();
        self.bc2_until_with_path(other, &is_leaf, &mut op, &mut path)
    }

    fn bc2_until_with_path<F, P>(
        &self,
        other: &Value,
        is_leaf: &P,
        op: &mut F,
        path: &mut Vec<usize>,
    ) -> BcResult<Value>
    where
        F: FnMut(&Value, &Value) -> WqResult<Value>,
        P: Fn(usize, &Value, &Value) -> bool,
    {
        let depth_from_root = path.len();
        if is_leaf(depth_from_root, self, other) {
            return op(self, other).bc_at_path(path);
        }

        let left_atom = self.is_atom();
        let right_atom = other.is_atom();

        if left_atom && right_atom {
            return op(self, other).bc_at_path(path);
        }

        if left_atom {
            return match other {
                Value::List(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, y) in items.iter().enumerate() {
                        path.push(i);
                        out.push(self.bc2_until_with_path(y, is_leaf, op, path)?);
                        path.pop();
                    }
                    Ok(Value::from_items(out))
                }
                Value::IntList(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, &y) in items.iter().enumerate() {
                        path.push(i);
                        let result = {
                            let rhs = Value::Int(y);
                            self.bc2_until_with_path(&rhs, is_leaf, op, path)?
                        };
                        path.pop();
                        out.push(result);
                    }
                    Ok(Value::from_items(out))
                }
                Value::Dict(map) => {
                    let mut out = IndexMap::with_capacity(map.len());
                    for (i, (k, v)) in map.iter().enumerate() {
                        path.push(i);
                        let vv = self.bc2_until_with_path(v, is_leaf, op, path)?;
                        path.pop();
                        out.insert(k.clone(), vv);
                    }
                    Ok(Value::Dict(out))
                }
                _ => op(self, other).bc_at_path(path),
            };
        }

        if right_atom {
            return match self {
                Value::List(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, x) in items.iter().enumerate() {
                        path.push(i);
                        out.push(x.bc2_until_with_path(other, is_leaf, op, path)?);
                        path.pop();
                    }
                    Ok(Value::from_items(out))
                }
                Value::IntList(items) => {
                    let mut out = Vec::with_capacity(items.len());
                    for (i, &x) in items.iter().enumerate() {
                        path.push(i);
                        let result = {
                            let lhs = Value::Int(x);
                            lhs.bc2_until_with_path(other, is_leaf, op, path)?
                        };
                        path.pop();
                        out.push(result);
                    }
                    Ok(Value::from_items(out))
                }
                Value::Dict(map) => {
                    let mut out = IndexMap::with_capacity(map.len());
                    for (i, (k, v)) in map.iter().enumerate() {
                        path.push(i);
                        let vv = v.bc2_until_with_path(other, is_leaf, op, path)?;
                        path.pop();
                        out.insert(k.clone(), vv);
                    }
                    Ok(Value::Dict(out))
                }
                _ => op(self, other).bc_at_path(path),
            };
        }

        match (self, other) {
            (Value::List(a), Value::List(b)) => {
                if a.len() != b.len() {
                    return Err(bc_len_mismatch(a.len(), b.len(), path));
                }
                let mut out = Vec::with_capacity(a.len());
                for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                    path.push(i);
                    out.push(x.bc2_until_with_path(y, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() != b.len() {
                    return Err(bc_len_mismatch(a.len(), b.len(), path));
                }
                let mut out = Vec::with_capacity(a.len());
                for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
                    path.push(i);
                    let res = {
                        let lhs = Value::Int(x);
                        let rhs = Value::Int(y);
                        lhs.bc2_until_with_path(&rhs, is_leaf, op, path)?
                    };
                    path.pop();
                    out.push(res);
                }
                Ok(Value::from_items(out))
            }
            (Value::IntList(a), Value::List(b)) => {
                if a.len() != b.len() {
                    return Err(bc_len_mismatch(a.len(), b.len(), path));
                }
                let mut out = Vec::with_capacity(a.len());
                for (i, (&x, y)) in a.iter().zip(b.iter()).enumerate() {
                    path.push(i);
                    let lhs = Value::Int(x);
                    out.push(lhs.bc2_until_with_path(y, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            (Value::List(a), Value::IntList(b)) => {
                if a.len() != b.len() {
                    return Err(bc_len_mismatch(a.len(), b.len(), path));
                }
                let mut out = Vec::with_capacity(a.len());
                for (i, (x, &y)) in a.iter().zip(b.iter()).enumerate() {
                    path.push(i);
                    let rhs = Value::Int(y);
                    out.push(x.bc2_until_with_path(&rhs, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            (Value::Dict(dx), Value::Dict(dy)) => {
                if dx.len() != dy.len() {
                    return Err(bc_len_mismatch(dx.len(), dy.len(), path));
                }
                let mut out = IndexMap::with_capacity(dx.len());
                for (i, ((kx, vx), (ky, vy))) in dx.iter().zip(dy.iter()).enumerate() {
                    if kx != ky {
                        return Err(BcError::Key {
                            path: path.to_vec(),
                            left: kx.clone(),
                            right: ky.clone(),
                        });
                    }
                    path.push(i);
                    let v = vx.bc2_until_with_path(vy, is_leaf, op, path)?;
                    path.pop();
                    out.insert(kx.clone(), v);
                }
                Ok(Value::Dict(out))
            }
            (Value::Dict(dx), Value::List(ys)) => {
                if dx.len() != ys.len() {
                    return Err(bc_len_mismatch(dx.len(), ys.len(), path));
                }
                let mut out = IndexMap::with_capacity(dx.len());
                for (i, ((k, xv), yv)) in dx.iter().zip(ys.iter()).enumerate() {
                    path.push(i);
                    out.insert(k.clone(), xv.bc2_until_with_path(yv, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::Dict(out))
            }
            (Value::List(xs), Value::Dict(dy)) => {
                if xs.len() != dy.len() {
                    return Err(bc_len_mismatch(xs.len(), dy.len(), path));
                }
                let mut out = IndexMap::with_capacity(dy.len());
                for (i, (xv, (k, yv))) in xs.iter().zip(dy.iter()).enumerate() {
                    path.push(i);
                    out.insert(k.clone(), xv.bc2_until_with_path(yv, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::Dict(out))
            }
            (Value::Dict(dx), Value::IntList(ys)) => {
                if dx.len() != ys.len() {
                    return Err(bc_len_mismatch(dx.len(), ys.len(), path));
                }
                let mut out = IndexMap::with_capacity(dx.len());
                for (i, ((k, xv), &yi)) in dx.iter().zip(ys.iter()).enumerate() {
                    path.push(i);
                    let rhs = Value::Int(yi);
                    out.insert(k.clone(), xv.bc2_until_with_path(&rhs, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::Dict(out))
            }
            (Value::IntList(xs), Value::Dict(dy)) => {
                if xs.len() != dy.len() {
                    return Err(bc_len_mismatch(xs.len(), dy.len(), path));
                }
                let mut out = IndexMap::with_capacity(dy.len());
                for (i, (&xi, (k, yv))) in xs.iter().zip(dy.iter()).enumerate() {
                    path.push(i);
                    let lhs = Value::Int(xi);
                    out.insert(k.clone(), lhs.bc2_until_with_path(yv, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::Dict(out))
            }
            (Value::Dict(dx), y) => {
                let mut out = IndexMap::with_capacity(dx.len());
                for (i, (k, xv)) in dx.iter().enumerate() {
                    path.push(i);
                    out.insert(k.clone(), xv.bc2_until_with_path(y, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::Dict(out))
            }
            (x, Value::Dict(dy)) => {
                let mut out = IndexMap::with_capacity(dy.len());
                for (i, (k, yv)) in dy.iter().enumerate() {
                    path.push(i);
                    out.insert(k.clone(), x.bc2_until_with_path(yv, is_leaf, op, path)?);
                    path.pop();
                }
                Ok(Value::Dict(out))
            }
            _ => op(self, other).bc_at_path(path),
        }
    }
}
