use std::sync::Arc;

use indexmap::IndexMap;

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
            Value::Set(a) => {
                let mut out = indexmap::IndexSet::with_capacity(a.len());
                for (i, x) in a.iter().enumerate() {
                    path.push(i);
                    out.insert(f(x).bc_at_path(path)?);
                    path.pop();
                }
                Ok(Value::Set(Arc::new(out)))
            }
            // `is_atom()` gate above ensures we only enter this match for
            // List|IntList|Dict|String|Set — all five are handled.
            _ => unreachable!("bc1: is_atom guard excludes other variants"),
        }
    }
}

// ---------------------------------------------------------------------------
// bc2 helpers – eliminate the left/right atom duplication in the original
// ---------------------------------------------------------------------------

/// Left operand is an atom, right is a container — broadcast the atom across
/// every element of the container.
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
        Value::Set(items) => {
            let mut out = indexmap::IndexSet::with_capacity(items.len());
            for (i, y) in items.iter().enumerate() {
                path.push(i);
                out.insert(atom.bc2_until_with_path(y, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Set(Arc::new(out)))
        }
        // `container` is !is_atom() (checked by caller), so it must be
        // one of the five containers above.
        _ => unreachable!("broadcast_left: container kind not handled"),
    }
}

/// Right operand is an atom, left is a container — broadcast the atom across
/// every element of the container.
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
        Value::Set(items) => {
            let mut out = indexmap::IndexSet::with_capacity(items.len());
            for (i, x) in items.iter().enumerate() {
                path.push(i);
                out.insert(x.bc2_until_with_path(atom, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Set(Arc::new(out)))
        }
        _ => unreachable!("broadcast_right: container kind not handled"),
    }
}

/// Both operands are containers — zip element-wise, with String decomposed
/// into chars and Dict key-ordering preserved.
///          Atom        List     IntList    String      Dict       Set
///          ────        ────     ───────    ──────      ────       ───
/// Atom      op        bc → L     bc → L     bc → L     bc → D     bc → E
/// List      bc → L   zip → L    zip → L    zip → L    zip → D    zip → L
/// IntList   bc → L   zip → L    zip → L    zip → L    zip → D    zip → L
/// String    bc → L   zip → L    zip → L    zip → L    zip → D    zip → L
/// Dict      bc → D   zip → D    zip → D    zip → D    zip → D*   zip → D
/// Set       bc → E   zip → L    zip → L    zip → L    zip → D    zip → E
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
        // ── Same type pairs = 5 ──────────────────────────────────────────
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
        (Value::Set(a), Value::Set(b)) => {
            if a.len() != b.len() {
                return Err(bc_len_mismatch(a.len(), b.len(), path));
            }
            let mut out = indexmap::IndexSet::with_capacity(a.len());
            for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
                path.push(i);
                out.insert(x.bc2_until_with_path(y, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Set(Arc::new(out)))
        }

        // ── IntList x List = 2 ─────────────────────
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

        // ── String x IntList, List = 4 ──────────
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

        // ── Dict x IntList, List, Set, String, 8 ──────────
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
        (Value::Dict(dx), Value::Set(s)) => {
            if dx.len() != s.len() {
                return Err(bc_len_mismatch(dx.len(), s.len(), path));
            }
            let mut out = IndexMap::with_capacity(dx.len());
            for (i, ((k, xv), yv)) in dx.iter().zip(s.iter()).enumerate() {
                path.push(i);
                out.insert(k.clone(), xv.bc2_until_with_path(yv, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }
        (Value::Set(s), Value::Dict(dy)) => {
            if s.len() != dy.len() {
                return Err(bc_len_mismatch(s.len(), dy.len(), path));
            }
            let mut out = IndexMap::with_capacity(dy.len());
            for (i, (xv, (k, yv))) in s.iter().zip(dy.iter()).enumerate() {
                path.push(i);
                out.insert(k.clone(), xv.bc2_until_with_path(yv, stop, op, path)?);
                path.pop();
            }
            Ok(Value::Dict(Arc::new(out)))
        }

        // ── Set x List, IntList, String = 6 ─────────────
        (Value::Set(a), Value::List(b)) => {
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
        (Value::List(a), Value::Set(b)) => {
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
        (Value::Set(a), Value::IntList(b)) => {
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
        (Value::IntList(a), Value::Set(b)) => {
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
        (Value::String(a), Value::Set(b)) => {
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
        (Value::Set(a), Value::String(b)) => {
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

        // 5 x 5 = 25 container pairs
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
