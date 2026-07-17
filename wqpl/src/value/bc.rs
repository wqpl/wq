use std::sync::Arc;

use indexmap::IndexMap;

use crate::value::seq::ValueSeq;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BcError {
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
        wqerror: Box<WqError>,
    },
}

pub(crate) type BcResult<T> = Result<T, BcError>;

#[inline]
fn bc_len_mismatch(left: usize, right: usize, path: &[usize]) -> BcError {
    BcError::Length {
        path: path.to_vec(),
        left,
        right,
    }
}

/// Map a `Result<_, WqError>` (from `f`/`op`) into `BcErr::WqErr` with the
/// current path.
pub(crate) trait WqToBc<T> {
    fn bc_at_path(self, path: &[usize]) -> BcResult<T>;
}

impl<T> WqToBc<T> for Result<T, WqError> {
    #[inline]
    fn bc_at_path(self, path: &[usize]) -> BcResult<T> {
        self.map_err(|wqerr| BcError::Wq {
            path: path.to_vec(),
            wqerror: Box::new(wqerr),
        })
    }
}

impl BcError {
    pub(crate) fn into_wqerror(self) -> WqError {
        match self {
            BcError::Length { path, left, right } => {
                let e = WqError::new(WqErrorType::Length)
                    .msg("list lengths do not match")
                    .attach_note(format!("left length is {left}"))
                    .attach_note(format!("right length is {right}"));
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
                    .msg("dict keys do not match")
                    .attach_note(format!("left key is `{left}"))
                    .attach_note(format!("right key is `{right}"));
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
                    return *wqerror;
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

// ---------------------------------------------------------------------------
// Stop-condition enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum Bc1Stop {
    /// Stop at atoms (the common case).  All `bc1_until` call sites except
    /// `_map` use this.
    Atom,
    /// Stop at atoms or when reaching `el` layers from the root.
    AtomOrDepth(i64),
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Bc2Stop {
    /// Stop when both operands are atoms (the common case).  All `bc2_until`
    /// call sites except `_zipw` / `_zip` use this.
    BothAtom,
    /// Stop when both are atoms or when reaching `el` layers from the root.
    BothAtomOrDepth(i64),
}

impl Value {
    // ---------------------------------------------------------------------------
    // bc1 – unary broadcasting
    // ---------------------------------------------------------------------------

    pub(crate) fn bc1<F>(&self, mut f: F) -> BcResult<Value>
    where
        F: FnMut(&Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        self.bc1_until_with_path(Bc1Stop::Atom, &mut f, &mut path)
    }

    /// 1-arg broadcasting with explicit stop condition.
    pub(crate) fn bc1_until<F>(&self, stop: Bc1Stop, f: F) -> BcResult<Value>
    where
        F: FnMut(&Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        let mut f = f;
        self.bc1_until_with_path(stop, &mut f, &mut path)
    }

    pub(crate) fn bc1_for_each_until<F>(&self, stop: Bc1Stop, f: F) -> BcResult<()>
    where
        F: FnMut(&Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        let mut f = f;
        self.bc1_for_each_until_with_path(stop, &mut f, &mut path)
    }

    fn bc1_until_with_path<F>(
        &self,
        stop: Bc1Stop,
        f: &mut F,
        path: &mut Vec<usize>,
    ) -> BcResult<Value>
    where
        F: FnMut(&Value) -> WqResult<Value>,
    {
        let should_stop = match stop {
            Bc1Stop::Atom => self.is_atom(),
            Bc1Stop::AtomOrDepth(el) => self.is_atom() || path.len() as i64 >= el,
        };
        if should_stop {
            return f(self).bc_at_path(path);
        }
        match self {
            Value::List(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, x) in a.iter().enumerate() {
                    path.push(i);
                    out.push(x.bc1_until_with_path(stop, f, path)?);
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
            Value::IntRange(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, x) in a.iter().enumerate() {
                    path.push(i);
                    out.push(f(&Value::Int(x)).bc_at_path(path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            Value::BoolList(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, &x) in a.iter().enumerate() {
                    path.push(i);
                    out.push(f(&Value::Bool(x)).bc_at_path(path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            Value::FloatList(a) => {
                let mut out = Vec::with_capacity(a.len());
                for (i, &x) in a.iter().enumerate() {
                    path.push(i);
                    out.push(f(&Value::Float(x)).bc_at_path(path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }
            Value::Dict(m) => {
                let mut out = IndexMap::with_capacity(m.len());
                for (i, (k, v)) in m.iter().enumerate() {
                    path.push(i);
                    let vv = v.bc1_until_with_path(stop, f, path)?;
                    path.pop();
                    out.insert(k.clone(), vv);
                }
                Ok(Value::Dict(Arc::new(out)))
            }
            Value::String(s) => {
                let mut out = Vec::with_capacity(s.chars().count());
                for (i, c) in s.chars().enumerate() {
                    path.push(i);
                    out.push(f(&Value::Char(c)).bc_at_path(path)?);
                    path.pop();
                }
                Ok(Value::from_items(out))
            }

            _ => unreachable!("bc1: is_atom guard excludes other variants"),
        }
    }

    fn bc1_for_each_until_with_path<F>(
        &self,
        stop: Bc1Stop,
        f: &mut F,
        path: &mut Vec<usize>,
    ) -> BcResult<()>
    where
        F: FnMut(&Value) -> WqResult<Value>,
    {
        let should_stop = match stop {
            Bc1Stop::Atom => self.is_atom(),
            Bc1Stop::AtomOrDepth(el) => self.is_atom() || path.len() as i64 >= el,
        };
        if should_stop {
            return f(self).map(|_| ()).bc_at_path(path);
        }
        match self {
            Value::List(a) => {
                for (i, x) in a.iter().enumerate() {
                    path.push(i);
                    x.bc1_for_each_until_with_path(stop, f, path)?;
                    path.pop();
                }
            }
            Value::IntList(a) => {
                for (i, &x) in a.iter().enumerate() {
                    path.push(i);
                    f(&Value::Int(x)).map(|_| ()).bc_at_path(path)?;
                    path.pop();
                }
            }
            Value::IntRange(a) => {
                for (i, x) in a.iter().enumerate() {
                    path.push(i);
                    f(&Value::Int(x)).map(|_| ()).bc_at_path(path)?;
                    path.pop();
                }
            }
            Value::BoolList(a) => {
                for (i, &x) in a.iter().enumerate() {
                    path.push(i);
                    f(&Value::Bool(x)).map(|_| ()).bc_at_path(path)?;
                    path.pop();
                }
            }
            Value::FloatList(a) => {
                for (i, &x) in a.iter().enumerate() {
                    path.push(i);
                    f(&Value::Float(x)).map(|_| ()).bc_at_path(path)?;
                    path.pop();
                }
            }
            Value::Dict(m) => {
                for (i, (_, v)) in m.iter().enumerate() {
                    path.push(i);
                    v.bc1_for_each_until_with_path(stop, f, path)?;
                    path.pop();
                }
            }
            Value::String(s) => {
                for (i, c) in s.chars().enumerate() {
                    path.push(i);
                    f(&Value::Char(c)).map(|_| ()).bc_at_path(path)?;
                    path.pop();
                }
            }

            _ => unreachable!("bc1: is_atom guard excludes other variants"),
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// bc2 helpers – eliminate the left/right atom duplication in the original
// ---------------------------------------------------------------------------

/// Left operand is an atom, right is a container
/// broadcast the atom across every element of the container.
fn broadcast_left<F>(
    atom: &Value,
    container: &Value,
    stop: Bc2Stop,
    op: &mut F,
    path: &mut Vec<usize>,
) -> BcResult<Value>
where
    F: FnMut(&Value, &Value) -> WqResult<Value>,
{
    match container {
        Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, y) in items.iter().enumerate() {
                path.push(i);
                out.push(atom.bc2_until_with_path(y, stop, op, path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::IntList(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, &y) in items.iter().enumerate() {
                path.push(i);
                let rhs = Value::Int(y);
                out.push(op(atom, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::IntRange(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, y) in items.iter().enumerate() {
                path.push(i);
                let rhs = Value::Int(y);
                out.push(op(atom, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::BoolList(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, &y) in items.iter().enumerate() {
                path.push(i);
                let rhs = Value::Bool(y);
                out.push(op(atom, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::FloatList(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, &y) in items.iter().enumerate() {
                path.push(i);
                let rhs = Value::Float(y);
                out.push(op(atom, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::Dict(map) => {
            let mut out = IndexMap::with_capacity(map.len());
            for (i, (k, v)) in map.iter().enumerate() {
                path.push(i);
                let vv = atom.bc2_until_with_path(v, stop, op, path)?;
                path.pop();
                out.insert(k.clone(), vv);
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        Value::String(s) => {
            let mut out = Vec::with_capacity(s.chars().count());
            for (i, c) in s.chars().enumerate() {
                path.push(i);
                let ch = Value::Char(c);
                out.push(op(atom, &ch).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }

        _ => unreachable!("broadcast_left: container kind not handled"),
    }
}

/// Right operand is an atom, left is a container
/// broadcast the atom across every element of the container.
fn broadcast_right<F>(
    container: &Value,
    atom: &Value,
    stop: Bc2Stop,
    op: &mut F,
    path: &mut Vec<usize>,
) -> BcResult<Value>
where
    F: FnMut(&Value, &Value) -> WqResult<Value>,
{
    match container {
        Value::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, x) in items.iter().enumerate() {
                path.push(i);
                out.push(x.bc2_until_with_path(atom, stop, op, path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::IntList(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, &x) in items.iter().enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                out.push(op(&lhs, atom).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::IntRange(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, x) in items.iter().enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                out.push(op(&lhs, atom).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::BoolList(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, &x) in items.iter().enumerate() {
                path.push(i);
                let lhs = Value::Bool(x);
                out.push(op(&lhs, atom).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::FloatList(items) => {
            let mut out = Vec::with_capacity(items.len());
            for (i, &x) in items.iter().enumerate() {
                path.push(i);
                let lhs = Value::Float(x);
                out.push(op(&lhs, atom).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        Value::Dict(map) => {
            let mut out = IndexMap::with_capacity(map.len());
            for (i, (k, v)) in map.iter().enumerate() {
                path.push(i);
                let vv = v.bc2_until_with_path(atom, stop, op, path)?;
                path.pop();
                out.insert(k.clone(), vv);
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        Value::String(s) => {
            let mut out = Vec::with_capacity(s.chars().count());
            for (i, c) in s.chars().enumerate() {
                path.push(i);
                let ch = Value::Char(c);
                out.push(op(&ch, atom).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }

        _ => unreachable!("broadcast_right: container kind not handled"),
    }
}

fn should_stream_mixed_sequence(value: &Value) -> bool {
    matches!(
        value,
        Value::IntRange(_) | Value::BoolList(_) | Value::FloatList(_)
    )
}

fn zip_value_sequences<F>(
    left: ValueSeq<'_>,
    right: ValueSeq<'_>,
    stop: Bc2Stop,
    op: &mut F,
    path: &mut Vec<usize>,
) -> BcResult<Value>
where
    F: FnMut(&Value, &Value) -> WqResult<Value>,
{
    if left.len() != right.len() {
        return Err(bc_len_mismatch(left.len(), right.len(), path));
    }

    let len = left.len();
    let mut out = Vec::with_capacity(len);
    let mut left_values = left.values();
    let mut right_values = right.values();
    for i in 0..len {
        path.push(i);
        let lhs = left_values
            .next()
            .expect("iterator length matches sequence length");
        let rhs = right_values
            .next()
            .expect("iterator length matches sequence length");
        out.push(lhs.bc2_until_with_path(&rhs, stop, op, path)?);
        path.pop();
    }
    Ok(Value::from_items(out))
}

fn zip_sequence_with_dict<F>(
    seq: ValueSeq<'_>,
    map: &IndexMap<Arc<str>, Value>,
    sequence_left: bool,
    stop: Bc2Stop,
    op: &mut F,
    path: &mut Vec<usize>,
) -> BcResult<Value>
where
    F: FnMut(&Value, &Value) -> WqResult<Value>,
{
    if seq.len() != map.len() {
        return Err(bc_len_mismatch(seq.len(), map.len(), path));
    }

    let mut out = IndexMap::with_capacity(map.len());
    let mut seq_values = seq.values();
    for (i, (key, dict_value)) in map.iter().enumerate() {
        path.push(i);
        let seq_value = seq_values
            .next()
            .expect("iterator length matches sequence length");
        let value = if sequence_left {
            seq_value.bc2_until_with_path(dict_value, stop, op, path)?
        } else {
            dict_value.bc2_until_with_path(&seq_value, stop, op, path)?
        };
        path.pop();
        out.insert(key.clone(), value);
    }
    Ok(Value::Dict(Arc::new(out)))
}

fn zip_containers<F>(
    left: &Value,
    right: &Value,
    stop: Bc2Stop,
    op: &mut F,
    path: &mut Vec<usize>,
) -> BcResult<Value>
where
    F: FnMut(&Value, &Value) -> WqResult<Value>,
{
    match (left, right) {
        (Value::IntRange(a), Value::IntRange(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                let rhs = Value::Int(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            return Ok(Value::from_items(out));
        }
        (Value::IntRange(a), Value::IntList(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (x, &y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                let rhs = Value::Int(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            return Ok(Value::from_items(out));
        }
        (Value::IntList(a), Value::IntRange(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (&x, y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                let rhs = Value::Int(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            return Ok(Value::from_items(out));
        }
        (Value::BoolList(a), Value::BoolList(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Bool(x);
                let rhs = Value::Bool(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            return Ok(Value::from_items(out));
        }
        (Value::FloatList(a), Value::FloatList(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Float(x);
                let rhs = Value::Float(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            return Ok(Value::from_items(out));
        }
        _ => {}
    }

    let stream_left = should_stream_mixed_sequence(left);
    let stream_right = should_stream_mixed_sequence(right);
    if stream_left || stream_right {
        match (left, right) {
            (Value::Dict(map), _) if stream_right => {
                if let Some(seq) = ValueSeq::from_value(right) {
                    return zip_sequence_with_dict(seq, map, false, stop, op, path);
                }
            }
            (_, Value::Dict(map)) if stream_left => {
                if let Some(seq) = ValueSeq::from_value(left) {
                    return zip_sequence_with_dict(seq, map, true, stop, op, path);
                }
            }
            _ => {
                if let (Some(left), Some(right)) =
                    (ValueSeq::from_value(left), ValueSeq::from_value(right))
                {
                    return zip_value_sequences(left, right, stop, op, path);
                }
            }
        }
    }

    if let Value::IntRange(range) = left {
        let left = Value::IntList(Arc::new(range.to_vec()));
        return zip_containers(&left, right, stop, op, path);
    }
    if let Value::IntRange(range) = right {
        let right = Value::IntList(Arc::new(range.to_vec()));
        return zip_containers(left, &right, stop, op, path);
    }
    if let Value::BoolList(items) = left {
        let left = Value::List(Arc::new(items.iter().copied().map(Value::Bool).collect()));
        return zip_containers(&left, right, stop, op, path);
    }
    if let Value::BoolList(items) = right {
        let right = Value::List(Arc::new(items.iter().copied().map(Value::Bool).collect()));
        return zip_containers(left, &right, stop, op, path);
    }
    if let Value::FloatList(items) = left {
        let left = Value::List(Arc::new(items.iter().copied().map(Value::Float).collect()));
        return zip_containers(&left, right, stop, op, path);
    }
    if let Value::FloatList(items) = right {
        let right = Value::List(Arc::new(items.iter().copied().map(Value::Float).collect()));
        return zip_containers(left, &right, stop, op, path);
    }

    match (left, right) {
        // Same type pairs = 5
        (Value::IntList(a), Value::IntList(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (&x, &y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                let rhs = Value::Int(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        (Value::List(a), Value::List(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                out.push(x.bc2_until_with_path(y, stop, op, path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        (Value::String(a), Value::String(b)) => {
            if a.chars().count() != b.chars().count() {
                return Err(bc_len_mismatch(a.chars().count(), b.chars().count(), path));
            }
            let mut out = Vec::with_capacity(a.chars().count());
            for (i, (ca, cb)) in a.chars().zip(b.chars()).enumerate() {
                path.push(i);
                let lhs = Value::Char(ca);
                let rhs = Value::Char(cb);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
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
                        left: kx.to_string(),
                        right: ky.to_string(),
                    });
                }
                path.push(i);
                let v = vx.bc2_until_with_path(vy, stop, op, path)?;
                path.pop();
                out.insert(kx.clone(), v);
            }
            Ok(Value::Dict(Arc::new(out)))
        }

        // IntList x List = 2
        (Value::IntList(a), Value::List(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (&x, y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                out.push(lhs.bc2_until_with_path(y, stop, op, path)?);
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
                out.push(x.bc2_until_with_path(&rhs, stop, op, path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }

        // String x (IntList, List) = 4
        (Value::String(a), Value::List(b)) => {
            if a.chars().count() != b.len() {
                return Err(bc_len_mismatch(a.chars().count(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.chars().count());
            for (i, (c, y)) in a.chars().zip(b.iter()).enumerate() {
                path.push(i);
                let ch = Value::Char(c);
                out.push(ch.bc2_until_with_path(y, stop, op, path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        (Value::List(a), Value::String(b)) => {
            if a.len() != b.chars().count() {
                return Err(bc_len_mismatch(a.len(), b.chars().count(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (x, c)) in a.iter().zip(b.chars()).enumerate() {
                path.push(i);
                let ch = Value::Char(c);
                out.push(x.bc2_until_with_path(&ch, stop, op, path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        (Value::String(a), Value::IntList(b)) => {
            if a.chars().count() != b.len() {
                return Err(bc_len_mismatch(a.chars().count(), b.len(), path));
            }
            let mut out = Vec::with_capacity(a.chars().count());
            for (i, (c, &y)) in a.chars().zip(b.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Char(c);
                let rhs = Value::Int(y);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }
        (Value::IntList(a), Value::String(b)) => {
            if a.len() != b.chars().count() {
                return Err(bc_len_mismatch(a.len(), b.chars().count(), path));
            }
            let mut out = Vec::with_capacity(a.len());
            for (i, (&x, c)) in a.iter().zip(b.chars()).enumerate() {
                path.push(i);
                let lhs = Value::Int(x);
                let rhs = Value::Char(c);
                out.push(op(&lhs, &rhs).bc_at_path(path)?);
                path.pop();
            }
            Ok(Value::from_items(out))
        }

        // Dict x (IntList, List, Set, String) = 8
        (Value::Dict(dx), Value::List(ys)) => {
            if dx.len() != ys.len() {
                return Err(bc_len_mismatch(dx.len(), ys.len(), path));
            }
            let mut out = IndexMap::with_capacity(dx.len());
            for (i, ((k, xv), yv)) in dx.iter().zip(ys.iter()).enumerate() {
                path.push(i);
                out.insert(k.clone(), xv.bc2_until_with_path(yv, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        (Value::List(xs), Value::Dict(dy)) => {
            if xs.len() != dy.len() {
                return Err(bc_len_mismatch(xs.len(), dy.len(), path));
            }
            let mut out = IndexMap::with_capacity(dy.len());
            for (i, (xv, (k, yv))) in xs.iter().zip(dy.iter()).enumerate() {
                path.push(i);
                out.insert(k.clone(), xv.bc2_until_with_path(yv, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        (Value::Dict(dx), Value::IntList(ys)) => {
            if dx.len() != ys.len() {
                return Err(bc_len_mismatch(dx.len(), ys.len(), path));
            }
            let mut out = IndexMap::with_capacity(dx.len());
            for (i, ((k, xv), &yi)) in dx.iter().zip(ys.iter()).enumerate() {
                path.push(i);
                let rhs = Value::Int(yi);
                out.insert(k.clone(), xv.bc2_until_with_path(&rhs, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        (Value::IntList(xs), Value::Dict(dy)) => {
            if xs.len() != dy.len() {
                return Err(bc_len_mismatch(xs.len(), dy.len(), path));
            }
            let mut out = IndexMap::with_capacity(dy.len());
            for (i, (&xi, (k, yv))) in xs.iter().zip(dy.iter()).enumerate() {
                path.push(i);
                let lhs = Value::Int(xi);
                out.insert(k.clone(), lhs.bc2_until_with_path(yv, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        (Value::String(a), Value::Dict(dy)) => {
            if a.chars().count() != dy.len() {
                return Err(bc_len_mismatch(a.chars().count(), dy.len(), path));
            }
            let mut out = IndexMap::with_capacity(dy.len());
            for (i, (c, (k, yv))) in a.chars().zip(dy.iter()).enumerate() {
                path.push(i);
                let ch = Value::Char(c);
                out.insert(k.clone(), ch.bc2_until_with_path(yv, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        (Value::Dict(dx), Value::String(b)) => {
            if dx.len() != b.chars().count() {
                return Err(bc_len_mismatch(dx.len(), b.chars().count(), path));
            }
            let mut out = IndexMap::with_capacity(dx.len());
            for (i, ((k, xv), c)) in dx.iter().zip(b.chars()).enumerate() {
                path.push(i);
                let ch = Value::Char(c);
                out.insert(k.clone(), xv.bc2_until_with_path(&ch, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }

        _ => unreachable!("zip_containers: missing container pair"),
    }
}

// ---------------------------------------------------------------------------
// bc2 – binary broadcasting
// ---------------------------------------------------------------------------

impl Value {
    pub(crate) fn bc2<F>(&self, other: &Value, mut op: F) -> BcResult<Value>
    where
        F: FnMut(&Value, &Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        self.bc2_until_with_path(other, Bc2Stop::BothAtom, &mut op, &mut path)
    }

    /// 2-arg broadcasting with explicit stop condition.
    pub(crate) fn bc2_until<F>(&self, other: &Value, stop: Bc2Stop, mut op: F) -> BcResult<Value>
    where
        F: FnMut(&Value, &Value) -> WqResult<Value>,
    {
        let mut path = Vec::new();
        self.bc2_until_with_path(other, stop, &mut op, &mut path)
    }

    fn bc2_until_with_path<F>(
        &self,
        other: &Value,
        stop: Bc2Stop,
        op: &mut F,
        path: &mut Vec<usize>,
    ) -> BcResult<Value>
    where
        F: FnMut(&Value, &Value) -> WqResult<Value>,
    {
        let should_stop = match stop {
            Bc2Stop::BothAtom => self.is_atom() && other.is_atom(),
            Bc2Stop::BothAtomOrDepth(el) => {
                (self.is_atom() && other.is_atom()) || path.len() as i64 >= el
            }
        };
        if should_stop {
            return op(self, other).bc_at_path(path);
        }

        let left_atom = self.is_atom();
        let right_atom = other.is_atom();

        if left_atom && right_atom {
            op(self, other).bc_at_path(path)
        } else if left_atom {
            broadcast_left(self, other, stop, op, path)
        } else if right_atom {
            broadcast_right(self, other, stop, op, path)
        } else {
            zip_containers(self, other, stop, op, path)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn broadcast_mismatches_use_structured_plain_language_notes() {
        let length = BcError::Length {
            path: Vec::new(),
            left: 2,
            right: 3,
        }
        .into_wqerror();
        assert_eq!(length.msg.as_deref(), Some("list lengths do not match"));
        assert_eq!(
            length.notes.as_slice(),
            ["left length is 2", "right length is 3"]
        );

        let keys = BcError::Key {
            path: Vec::new(),
            left: "alpha".to_string(),
            right: "beta".to_string(),
        }
        .into_wqerror();
        assert_eq!(keys.msg.as_deref(), Some("dict keys do not match"));
        assert_eq!(
            keys.notes.as_slice(),
            ["left key is `alpha", "right key is `beta"]
        );
    }

    #[test]
    fn bool_list_broadcasts_as_bool_atoms() {
        let value = Value::BoolList(Arc::new(vec![true, false]));

        assert_eq!(
            value.bool_or(&Value::Bool(true)).expect("bool broadcast"),
            Value::BoolList(Arc::new(vec![true, true]))
        );
        assert_eq!(
            value.bool_and(&Value::Bool(false)).expect("bool broadcast"),
            Value::BoolList(Arc::new(vec![false, false]))
        );
    }

    #[test]
    fn float_list_broadcasts_as_float_atoms() {
        let value = Value::FloatList(Arc::new(vec![
            ordered_float::OrderedFloat(1.5),
            ordered_float::OrderedFloat(2.5),
        ]));

        assert_eq!(
            value.add(&Value::float(1.0)).expect("float broadcast"),
            Value::FloatList(Arc::new(vec![
                ordered_float::OrderedFloat(2.5),
                ordered_float::OrderedFloat(3.5),
            ]))
        );
    }

    #[test]
    fn packed_containers_zip_as_atoms() {
        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(0, 1, 3)));
        assert_eq!(
            range.eq_bc(&range).expect("range zip"),
            Value::BoolList(Arc::new(vec![true, true, true]))
        );

        let bools = Value::BoolList(Arc::new(vec![true, false]));
        let other_bools = Value::BoolList(Arc::new(vec![true, true]));
        assert_eq!(
            bools.eq_bc(&other_bools).expect("bool zip"),
            Value::BoolList(Arc::new(vec![true, false]))
        );

        let floats = Value::FloatList(Arc::new(vec![
            ordered_float::OrderedFloat(1.0),
            ordered_float::OrderedFloat(2.0),
        ]));
        assert_eq!(
            floats.eq_bc(&floats).expect("float zip"),
            Value::BoolList(Arc::new(vec![true, true]))
        );
    }

    #[test]
    fn mixed_packed_containers_zip_without_widening_semantics() {
        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(0, 1, 3)));
        let list = Value::List(Arc::new(vec![
            Value::Int(10),
            Value::Int(20),
            Value::Int(30),
        ]));
        assert_eq!(
            range.add(&list).expect("range and list zip"),
            Value::IntList(Arc::new(vec![10, 21, 32]))
        );

        let bools = Value::BoolList(Arc::new(vec![true, false]));
        let floats = Value::FloatList(Arc::new(vec![
            ordered_float::OrderedFloat(1.0),
            ordered_float::OrderedFloat(0.0),
        ]));
        assert_eq!(
            bools.eq_bc(&floats).expect("bools and floats zip"),
            Value::BoolList(Arc::new(vec![false, false]))
        );

        let floats = Value::FloatList(Arc::new(vec![
            ordered_float::OrderedFloat(1.5),
            ordered_float::OrderedFloat(2.5),
        ]));
        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("b".into(), Value::Int(2));
        let result = floats
            .add(&Value::Dict(Arc::new(map)))
            .expect("float list and dict zip");

        let mut expected = IndexMap::new();
        expected.insert("a".into(), Value::float(2.5));
        expected.insert("b".into(), Value::float(4.5));
        assert_eq!(result, Value::Dict(Arc::new(expected)));
    }
}
