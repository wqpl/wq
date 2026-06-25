use std::sync::Arc;

use num_bigint::BigInt;
use ordered_float::OrderedFloat;
use rayon::prelude::*;

use crate::astnode::BinaryOperator;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

struct MmCtx<'a> {
    a: &'a Value,
    b: &'a Value,
    a_batch: &'a [usize],
    b_batch: &'a [usize],
    out_batch_dims: &'a [usize],
    a_rank: usize,
    b_rank: usize,
    m_opt: Option<usize>,
    k: usize,
    n_opt: Option<usize>,
}

/// Accumulator for matrix multiplication that stays in i64 until overflow.
enum Accum {
    I64(i64),
    Big(Box<BigInt>),
}

impl Accum {
    #[inline]
    fn zero() -> Self {
        Accum::I64(0)
    }

    #[inline]
    fn add_mul(&mut self, a: i64, b: i64) {
        match self {
            Accum::I64(v) => {
                if let Some(prod) = a.checked_mul(b) {
                    if let Some(sum) = v.checked_add(prod) {
                        *v = sum;
                    } else {
                        *self = Accum::Big(Box::new(BigInt::from(*v) + BigInt::from(prod)));
                    }
                } else {
                    *self = Accum::Big(Box::new(
                        BigInt::from(*v) + BigInt::from(a) * BigInt::from(b),
                    ));
                }
            }
            Accum::Big(big) => {
                **big += BigInt::from(a) * BigInt::from(b);
            }
        }
    }

    #[inline]
    fn into_value(self) -> Value {
        match self {
            Accum::I64(v) => Value::Int(v),
            Accum::Big(b) => Value::from_bigint(*b),
        }
    }
}

#[inline]
fn as_int_slice(v: &Value) -> Option<&[i64]> {
    match v {
        Value::IntList(items) => Some(items.as_ref()),
        _ => None,
    }
}

fn extract_int_rows(v: &Value) -> Option<Vec<&[i64]>> {
    let rows = match v {
        Value::List(items) => items,
        _ => return None,
    };
    let mut int_rows: Vec<&[i64]> = Vec::with_capacity(rows.len());
    for r in rows.iter() {
        int_rows.push(as_int_slice(r)?);
    }
    Some(int_rows)
}

/// Float matmul row view. Packed rows borrow their storage directly; only a
/// general `List(Float|Int)` row needs an owned conversion buffer.
enum FloatRow<'a> {
    FloatList(&'a [OrderedFloat<f64>]),
    IntList(&'a [i64]),
    Owned(Vec<f64>),
}

impl<'a> FloatRow<'a> {
    fn from_value(v: &'a Value) -> Option<Self> {
        match v {
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::Float(f) => Some(f.0),
                    Value::Int(i) => Some(*i as f64),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(Self::Owned),
            Value::IntList(items) => Some(Self::IntList(items.as_slice())),
            Value::FloatList(items) => Some(Self::FloatList(items.as_slice())),
            _ => None,
        }
    }

    fn len(&self) -> usize {
        match self {
            Self::FloatList(items) => items.len(),
            Self::IntList(items) => items.len(),
            Self::Owned(items) => items.len(),
        }
    }

    #[inline]
    fn at(&self, idx: usize) -> f64 {
        match self {
            Self::FloatList(items) => items[idx].0,
            Self::IntList(items) => items[idx] as f64,
            Self::Owned(items) => items[idx],
        }
    }
}

fn as_float_row(v: &Value) -> Option<FloatRow<'_>> {
    FloatRow::from_value(v)
}

fn extract_float_rows(v: &Value) -> Option<Vec<FloatRow<'_>>> {
    match v {
        Value::List(items) => items.iter().map(FloatRow::from_value).collect(),
        _ => None,
    }
}

/// K tiling size: keeps B tile (TILE_K × N × 8 bytes) in L2 cache.
const TILE_K: usize = 64;

/// Lightweight sequence accessor that avoids recursive `index_path` traversal.
/// Borrows from a `Value::List` or `Value::IntList` and provides O(1) element
/// access.
enum ValueSeq<'a> {
    List(&'a [Value]),
    IntList(&'a [i64]),
    FloatList(&'a [OrderedFloat<f64>]),
}

impl<'a> ValueSeq<'a> {
    fn from_value(v: &'a Value) -> Option<Self> {
        match v {
            Value::List(items) => Some(ValueSeq::List(items.as_slice())),
            Value::IntList(items) => Some(ValueSeq::IntList(items.as_slice())),
            Value::FloatList(items) => Some(ValueSeq::FloatList(items.as_slice())),
            _ => None,
        }
    }

    #[inline]
    fn get(&self, idx: usize) -> Option<Value> {
        match self {
            ValueSeq::List(items) => items.get(idx).cloned(),
            ValueSeq::IntList(items) => items.get(idx).map(|&x| Value::Int(x)),
            ValueSeq::FloatList(items) => items.get(idx).copied().map(Value::Float),
        }
    }
}

#[inline]
fn broadcast_shapes(a: &[usize], b: &[usize]) -> Option<Vec<usize>> {
    let mut out_rev = Vec::with_capacity(a.len().max(b.len()));
    let mut ia = a.len() as isize - 1;
    let mut ib = b.len() as isize - 1;
    while ia >= 0 || ib >= 0 {
        let da = if ia >= 0 { a[ia as usize] } else { 1 };
        let db = if ib >= 0 { b[ib as usize] } else { 1 };
        if da == db || da == 1 || db == 1 {
            out_rev.push(da.max(db));
        } else {
            return None;
        }
        ia -= 1;
        ib -= 1;
    }
    out_rev.reverse();
    Some(out_rev)
}

pub(crate) fn index_path(v: &Value, idxs: &[usize]) -> Option<Value> {
    if idxs.is_empty() {
        return Some(v.clone());
    }
    match v {
        Value::List(items) => {
            let i0 = *idxs.first()?;
            let next = items.get(i0)?;
            index_path(next, &idxs[1..])
        }
        Value::IntList(items) => {
            if idxs.len() != 1 {
                None
            } else {
                let i0 = *idxs.first()?;
                items.get(i0).copied().map(Value::Int)
            }
        }
        Value::FloatList(items) => {
            if idxs.len() != 1 {
                None
            } else {
                let i0 = *idxs.first()?;
                items.get(i0).copied().map(Value::Float)
            }
        }

        _ => None,
    }
}

#[inline]
fn map_bc_index(src_dims: &[usize], out_batch_dims: &[usize], out_idx: &[usize]) -> Vec<usize> {
    // Align from right: for each axis in src_dims, pick corresponding out_idx axis,
    // or 0 when the src axis is size 1 (broadcasted).
    let l_src = src_dims.len();
    let l_out = out_batch_dims.len();
    src_dims
        .iter()
        .enumerate()
        .map(|(j, &src_dim)| {
            let out_pos = l_out - l_src + j;
            if src_dim == 1 { 0 } else { out_idx[out_pos] }
        })
        .collect()
}

fn mm_core(ctx: &MmCtx<'_>, out_batch_idx: &[usize]) -> WqResult<Value> {
    let a_bidx = map_bc_index(ctx.a_batch, ctx.out_batch_dims, out_batch_idx);
    let b_bidx = map_bc_index(ctx.b_batch, ctx.out_batch_dims, out_batch_idx);

    // Pre-resolve batch prefix once to avoid per-element index_path traversal.
    let a_base = index_path(ctx.a, &a_bidx)
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid index while reading A"))?;
    let b_base = index_path(ctx.b, &b_bidx)
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("invalid index while reading B"))?;

    let a_seq = ValueSeq::from_value(&a_base)
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("A must be a sequence for matmul"))?;
    let b_seq = ValueSeq::from_value(&b_base)
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("B must be a sequence for matmul"))?;

    match (ctx.a_rank >= 2, ctx.b_rank >= 2) {
        (false, false) => {
            // dot product -> scalar
            let mut acc: Option<Value> = None;
            for kk in 0..ctx.k {
                let av = a_seq
                    .get(kk)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("A index out of range"))?;
                let bv = b_seq
                    .get(kk)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("B index out of range"))?;
                let prod = av.multiply(&bv)?;
                acc = Some(match acc {
                    None => prod,
                    Some(s) => s.add(&prod)?,
                });
            }
            Ok(acc.unwrap_or(Value::Int(0)))
        }
        (true, false) => {
            // (A: MxK) x (B: K) -> M
            let m = ctx.m_opt.expect("M known when a_rank>=2");
            // Pre-extract A rows from base to avoid per-element index_path
            let a_rows: Vec<ValueSeq> = match &a_base {
                Value::List(items) => items
                    .iter()
                    .map(ValueSeq::from_value)
                    .collect::<Option<Vec<_>>>(),

                _ => None,
            }
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("A rows must be sequences"))?;

            let result: Vec<Value> = (0..m)
                .into_par_iter()
                .map(|i| {
                    let a_row = &a_rows[i];
                    let mut acc: Option<Value> = None;
                    for kk in 0..ctx.k {
                        let av = a_row.get(kk).ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("A index out of range")
                        })?;
                        let bv = b_seq.get(kk).ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("B index out of range")
                        })?;
                        let prod = av.multiply(&bv)?;
                        acc = Some(match acc {
                            None => prod,
                            Some(s) => s.add(&prod)?,
                        });
                    }
                    Ok(acc.unwrap_or(Value::Int(0)))
                })
                .collect::<WqResult<Vec<Value>>>()?;
            Ok(Value::from_items(result))
        }
        (false, true) => {
            // (A: K) x (B: KxN) -> N
            // Loop interchange: iterate kk outer, j inner for sequential B access
            let n = ctx.n_opt.expect("N known when b_rank>=2");
            // Pre-extract B rows from base
            let b_rows: Vec<ValueSeq> = match &b_base {
                Value::List(items) => items
                    .iter()
                    .map(ValueSeq::from_value)
                    .collect::<Option<Vec<_>>>(),

                _ => None,
            }
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("B rows must be sequences"))?;

            let mut acc: Vec<Option<Value>> = (0..n).map(|_| None).collect();
            for (kk, b_row) in b_rows.iter().enumerate().take(ctx.k) {
                let av = a_seq
                    .get(kk)
                    .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("A index out of range"))?;
                for (j, acc_j) in acc.iter_mut().enumerate().take(n) {
                    let bv = b_row.get(j).ok_or_else(|| {
                        WqError::new(WqErrorType::Domain).msg("B index out of range")
                    })?;
                    let prod = av.clone().multiply(&bv)?;
                    *acc_j = Some(match acc_j.take() {
                        None => prod,
                        Some(s) => s.add(&prod)?,
                    });
                }
            }
            let result: Vec<Value> = acc
                .into_iter()
                .map(|opt| opt.unwrap_or(Value::Int(0)))
                .collect();
            Ok(Value::from_items(result))
        }
        (true, true) => {
            // (A: MxK) x (B: KxN) -> MxN
            // Loop interchange: iterate i (parallel) -> kk -> j for sequential B access
            let m = ctx.m_opt.expect("M known when a_rank>=2");
            let n = ctx.n_opt.expect("N known when b_rank>=2");

            // Pre-extract rows from bases
            let a_rows: Vec<ValueSeq> = match &a_base {
                Value::List(items) => items
                    .iter()
                    .map(ValueSeq::from_value)
                    .collect::<Option<Vec<_>>>(),

                _ => None,
            }
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("A rows must be sequences"))?;
            let b_rows: Vec<ValueSeq> = match &b_base {
                Value::List(items) => items
                    .iter()
                    .map(ValueSeq::from_value)
                    .collect::<Option<Vec<_>>>(),

                _ => None,
            }
            .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("B rows must be sequences"))?;

            let out_rows: WqResult<Vec<Value>> = (0..m)
                .into_par_iter()
                .map(|i| {
                    let a_row = &a_rows[i];
                    let mut acc: Vec<Option<Value>> = (0..n).map(|_| None).collect();
                    for (kk, b_row) in b_rows.iter().enumerate().take(ctx.k) {
                        let av = a_row.get(kk).ok_or_else(|| {
                            WqError::new(WqErrorType::Domain).msg("A index out of range")
                        })?;
                        for (j, acc_j) in acc.iter_mut().enumerate().take(n) {
                            let bv = b_row.get(j).ok_or_else(|| {
                                WqError::new(WqErrorType::Domain).msg("B index out of range")
                            })?;
                            let prod = av.clone().multiply(&bv)?;
                            *acc_j = Some(match acc_j.take() {
                                None => prod,
                                Some(s) => s.add(&prod)?,
                            });
                        }
                    }
                    let row: Vec<Value> = acc
                        .into_iter()
                        .map(|opt| opt.unwrap_or(Value::Int(0)))
                        .collect();
                    Ok(Value::from_items(row))
                })
                .collect();
            Ok(Value::List(Arc::new(out_rows?)))
        }
    }
}

fn build_batched(ctx: &MmCtx<'_>, out_idx: &[usize]) -> WqResult<Value> {
    let depth = out_idx.len();
    if depth == ctx.out_batch_dims.len() {
        return mm_core(ctx, out_idx);
    }
    let dim = ctx.out_batch_dims[depth];
    let items: WqResult<Vec<Value>> = (0..dim)
        .into_par_iter()
        .map(|i| {
            let mut local_idx = Vec::with_capacity(out_idx.len() + 1);
            local_idx.extend_from_slice(out_idx);
            local_idx.push(i);
            build_batched(ctx, &local_idx)
        })
        .collect();
    Ok(Value::from_items(items?))
}

/// Fast path: dot product of two IntList vectors (rank 1 × rank 1).
fn mm_intlist_dot(a: &Value, b: &Value, k: usize) -> Option<WqResult<Value>> {
    let a_slice = as_int_slice(a)?;
    let b_slice = as_int_slice(b)?;
    if a_slice.len() != k || b_slice.len() != k {
        return None;
    }
    let mut acc = Accum::zero();
    for kk in 0..k {
        acc.add_mul(a_slice[kk], b_slice[kk]);
    }
    Some(Ok(acc.into_value()))
}

/// Fast path: matrix-vector (M×K × K → M) where A is `List(IntList)`, B is
/// `IntList`.
fn mm_intlist_mv(a: &Value, b: &Value, m: usize, k: usize) -> Option<WqResult<Value>> {
    let a_int_rows = extract_int_rows(a)?;
    let b_slice = as_int_slice(b)?;
    if a_int_rows.len() != m || a_int_rows.iter().any(|r| r.len() != k) || b_slice.len() != k {
        return None;
    }
    let result: Vec<Value> = (0..m)
        .into_par_iter()
        .map(|i| {
            let a_row = a_int_rows[i];
            let mut acc = Accum::zero();
            for kk in 0..k {
                acc.add_mul(a_row[kk], b_slice[kk]);
            }
            acc.into_value()
        })
        .collect();
    Some(Ok(Value::from_items(result)))
}

/// Fast path: vector-matrix (K × K×N → N) where A is `IntList`, B is
/// `List(IntList)`. Loop interchange + K tiling for cache-friendly sequential
/// access.
fn mm_intlist_vm(a: &Value, b: &Value, k: usize, n: usize) -> Option<WqResult<Value>> {
    let a_slice = as_int_slice(a)?;
    let b_int_rows = extract_int_rows(b)?;
    if a_slice.len() != k || b_int_rows.len() != k || b_int_rows.iter().any(|r| r.len() != n) {
        return None;
    }
    let mut acc: Vec<Accum> = (0..n).map(|_| Accum::zero()).collect();
    let mut kk = 0;
    while kk < k {
        let tile_end = (kk + TILE_K).min(k);
        for kk_in in kk..tile_end {
            let av = a_slice[kk_in];
            let b_row = b_int_rows[kk_in];
            for j in 0..n {
                acc[j].add_mul(av, b_row[j]);
            }
        }
        kk = tile_end;
    }
    let result: Vec<Value> = acc.into_iter().map(|a| a.into_value()).collect();
    Some(Ok(Value::from_items(result)))
}

/// Fast path for 2D matrix multiplication where both operands are
/// `Value::List` of `Value::IntList`. Uses loop interchange (i -> kk -> j)
/// and K tiling so that B tile stays in L2 cache.
fn mm_intlist_2d(a: &Value, b: &Value, m: usize, k: usize, n: usize) -> Option<WqResult<Value>> {
    let a_int_rows = extract_int_rows(a)?;
    let b_int_rows = extract_int_rows(b)?;

    // Verify dimensions
    if a_int_rows.len() != m || a_int_rows.iter().any(|r| r.len() != k) {
        return None;
    }
    if b_int_rows.len() != k || b_int_rows.iter().any(|r| r.len() != n) {
        return None;
    }

    let result: WqResult<Vec<Value>> = (0..m)
        .into_par_iter()
        .map(|i| {
            let a_row = a_int_rows[i];
            let mut acc: Vec<Accum> = (0..n).map(|_| Accum::zero()).collect();

            let mut kk = 0;
            while kk < k {
                let tile_end = (kk + TILE_K).min(k);
                for kk_in in kk..tile_end {
                    let av = a_row[kk_in];
                    let b_row = b_int_rows[kk_in];
                    for (j, &bv) in b_row.iter().enumerate().take(n) {
                        acc[j].add_mul(av, bv);
                    }
                }
                kk = tile_end;
            }

            let mut out_row = Vec::with_capacity(n);
            for a in acc {
                out_row.push(a.into_value());
            }
            Ok(Value::from_items(out_row))
        })
        .collect();

    Some(result.map(|rows| Value::List(Arc::new(rows))))
}

/// Fast path: dot product of two vectors as native f64.
fn mm_float_dot(a: &Value, b: &Value, k: usize) -> Option<WqResult<Value>> {
    let a_vals = as_float_row(a)?;
    let b_vals = as_float_row(b)?;
    if a_vals.len() != k || b_vals.len() != k {
        return None;
    }
    let mut acc: f64 = 0.0;
    for kk in 0..k {
        acc += a_vals.at(kk) * b_vals.at(kk);
    }
    Some(Ok(Value::Float(OrderedFloat(acc))))
}

/// Fast path: matrix-vector (M×K × K → M) with native f64 arithmetic.
fn mm_float_mv(a: &Value, b: &Value, m: usize, k: usize) -> Option<WqResult<Value>> {
    let a_rows = extract_float_rows(a)?;
    let b_vals = as_float_row(b)?;
    if a_rows.len() != m || a_rows.iter().any(|r| r.len() != k) || b_vals.len() != k {
        return None;
    }
    let result: Vec<OrderedFloat<f64>> = (0..m)
        .into_par_iter()
        .map(|i| {
            let a_row = &a_rows[i];
            let mut acc: f64 = 0.0;
            for kk in 0..k {
                acc += a_row.at(kk) * b_vals.at(kk);
            }
            OrderedFloat(acc)
        })
        .collect();
    Some(Ok(Value::FloatList(Arc::new(result))))
}

/// Fast path: vector-matrix (K × K×N → N) with native f64 arithmetic.
/// Loop interchange + K tiling for sequential B access.
fn mm_float_vm(a: &Value, b: &Value, k: usize, n: usize) -> Option<WqResult<Value>> {
    let a_vals = as_float_row(a)?;
    let b_rows = extract_float_rows(b)?;
    if a_vals.len() != k || b_rows.len() != k || b_rows.iter().any(|r| r.len() != n) {
        return None;
    }
    let mut acc: Vec<f64> = vec![0.0; n];
    let mut kk = 0;
    while kk < k {
        let tile_end = (kk + TILE_K).min(k);
        for (kk_in, b_row) in b_rows.iter().enumerate().take(tile_end).skip(kk) {
            let av = a_vals.at(kk_in);
            for (j, acc_j) in acc.iter_mut().enumerate().take(n) {
                *acc_j += av * b_row.at(j);
            }
        }
        kk = tile_end;
    }
    let result = acc.into_iter().map(OrderedFloat).collect();
    Some(Ok(Value::FloatList(Arc::new(result))))
}

/// Fast path for 2D Float matrix multiplication with native f64 arithmetic.
/// Loop interchange (i → kk → j) + K tiling.
fn mm_float_mm(a: &Value, b: &Value, m: usize, k: usize, n: usize) -> Option<WqResult<Value>> {
    let a_rows = extract_float_rows(a)?;
    let b_rows = extract_float_rows(b)?;
    if a_rows.len() != m || a_rows.iter().any(|r| r.len() != k) {
        return None;
    }
    if b_rows.len() != k || b_rows.iter().any(|r| r.len() != n) {
        return None;
    }

    let result: WqResult<Vec<Value>> = (0..m)
        .into_par_iter()
        .map(|i| {
            let a_row = &a_rows[i];
            let mut acc: Vec<f64> = vec![0.0; n];

            let mut kk = 0;
            while kk < k {
                let tile_end = (kk + TILE_K).min(k);
                for (kk_in, b_row) in b_rows.iter().enumerate().take(tile_end).skip(kk) {
                    let av = a_row.at(kk_in);
                    for (j, acc_j) in acc.iter_mut().enumerate().take(n) {
                        *acc_j += av * b_row.at(j);
                    }
                }
                kk = tile_end;
            }

            let row = acc.into_iter().map(OrderedFloat).collect();
            Ok(Value::FloatList(Arc::new(row)))
        })
        .collect();

    Some(result.map(|rows| Value::List(Arc::new(rows))))
}

impl Value {
    pub(crate) fn mm(&self, other: &Value) -> WqResult<Value> {
        if let Some(res) = Value::lift_callable_binary(BinaryOperator::Matmul, self, other) {
            return Ok(res);
        }

        // Reject non-uniform shapes
        if !self.is_uniform() || !other.is_uniform() {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("left operand must be uniform")
                .got2(self, other));
        }

        let a_shape = self.shape_uniform().ok_or_else(|| {
            WqError::new(WqErrorType::Domain).msg("could not determine left shape")
        })?;
        let b_shape = other.shape_uniform().ok_or_else(|| {
            WqError::new(WqErrorType::Domain).msg("could not determine right shape")
        })?;

        let a_rank = a_shape.len();
        let b_rank = b_shape.len();

        if a_rank == 0 || b_rank == 0 {
            return self.multiply(other);
        }

        // Extract (batch, M?, K) and (batch, K, N?)
        let (a_batch, m_opt, k_a) = if a_rank >= 2 {
            (
                &a_shape[..a_rank - 2],
                Some(a_shape[a_rank - 2]),
                a_shape[a_rank - 1],
            )
        } else {
            (&a_shape[..0], None, a_shape[a_rank - 1])
        };

        let (b_batch, k_b, n_opt) = if b_rank >= 2 {
            (
                &b_shape[..b_rank - 2],
                b_shape[b_rank - 2],
                Some(b_shape[b_rank - 1]),
            )
        } else {
            (&b_shape[..0], b_shape[b_rank - 1], None)
        };

        if k_a != k_b {
            return Err(WqError::new(WqErrorType::Length)
                .msg("inner dimensions must match (K)")
                .attach_note(format!("left K={}, right K={}", k_a, k_b)));
        }
        let k = k_a;

        let out_batch = broadcast_shapes(a_batch, b_batch).ok_or_else(|| {
            WqError::new(WqErrorType::Length)
                .msg("batch dimensions are not broadcast-compatible")
                .attach_note(format!(
                    "left batch={:?}, right batch={:?}",
                    a_batch, b_batch
                ))
        })?;

        // Fast path: IntList, then Float (no batch dims)
        if out_batch.is_empty() {
            let intl = match (a_rank >= 2, b_rank >= 2) {
                (false, false) => mm_intlist_dot(self, other, k),
                (true, false) => mm_intlist_mv(self, other, m_opt.expect("M known"), k),
                (false, true) => mm_intlist_vm(self, other, k, n_opt.expect("N known")),
                (true, true) => mm_intlist_2d(
                    self,
                    other,
                    m_opt.expect("M known"),
                    k,
                    n_opt.expect("N known"),
                ),
            };
            if let Some(res) = intl {
                return res;
            }

            let float = match (a_rank >= 2, b_rank >= 2) {
                (false, false) => mm_float_dot(self, other, k),
                (true, false) => mm_float_mv(self, other, m_opt.expect("M known"), k),
                (false, true) => mm_float_vm(self, other, k, n_opt.expect("N known")),
                (true, true) => mm_float_mm(
                    self,
                    other,
                    m_opt.expect("M known"),
                    k,
                    n_opt.expect("N known"),
                ),
            };
            if let Some(res) = float {
                return res;
            }
        }

        // Build context and recurse
        let ctx = MmCtx {
            a: self,
            b: other,
            a_batch,
            b_batch,
            out_batch_dims: &out_batch,
            a_rank,
            b_rank,
            m_opt,
            k,
            n_opt,
        };
        build_batched(&ctx, &[])
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ordered_float::OrderedFloat;

    use crate::value::Value;

    fn il(v: &[i64]) -> Value {
        Value::IntList(v.to_vec().into())
    }

    fn mat(rows: &[&[i64]]) -> Value {
        let rs: Vec<Value> = rows.iter().map(|r| il(r)).collect();
        Value::List(Arc::new(rs))
    }

    fn fl(v: &[f64]) -> Value {
        Value::FloatList(Arc::new(v.iter().copied().map(OrderedFloat).collect()))
    }

    fn fmat(rows: &[&[f64]]) -> Value {
        let rs: Vec<Value> = rows.iter().map(|r| fl(r)).collect();
        Value::List(Arc::new(rs))
    }

    #[test]
    fn mm_dot_product() {
        let x = il(&[1, 2, 3]);
        let y = il(&[4, 5, 6]);
        let res = x.mm(&y).expect("dot product");
        assert_eq!(res, Value::Int(32));
    }

    #[test]
    fn mm_mat_vec() {
        let a = mat(&[&[1, 2], &[3, 4]]);
        let x = il(&[5, 6]);
        let res = a.mm(&x).expect("Ax");
        assert_eq!(res, il(&[17, 39]));
    }

    #[test]
    fn mm_mat_mat() {
        let a = mat(&[&[1, 2], &[3, 4]]);
        let b = mat(&[&[5, 6], &[7, 8]]);
        let res = a.mm(&b).expect("AB");
        let expect = mat(&[&[19, 22], &[43, 50]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn mm_float_mat_vec_returns_floatlist() {
        let a = mat(&[&[1, 2], &[3, 4]]);
        let x = fl(&[0.5, 1.5]);
        let res = a.mm(&x).expect("Ax");
        assert_eq!(res, fl(&[3.5, 7.5]));
    }

    #[test]
    fn mm_float_mat_mat_returns_floatlist_rows() {
        let a = fmat(&[&[1.0, 2.0], &[3.0, 4.0]]);
        let b = fmat(&[&[5.0, 6.0], &[7.0, 8.0]]);
        let res = a.mm(&b).expect("AB");
        let expect = fmat(&[&[19.0, 22.0], &[43.0, 50.0]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn mm_batched_broadcast() {
        // A: (2, 2, 2)
        let a0 = mat(&[&[1, 2], &[3, 4]]);
        let a1 = mat(&[&[5, 6], &[7, 8]]);
        let a = Value::List(Arc::new(vec![a0, a1]));

        // B: (2, 2) broadcast across batch
        let b = mat(&[&[9, 10], &[11, 12]]);

        let res = a.mm(&b).expect("batched AB");
        let exp0 = mat(&[&[31, 34], &[71, 78]]);
        let exp1 = mat(&[&[111, 122], &[151, 166]]);
        let expect = Value::List(Arc::new(vec![exp0, exp1]));
        assert_eq!(res, expect);
    }
}
