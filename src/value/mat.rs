use crate::{
    value::{Value, WqResult},
    wqerr::{WqErr, WqErrType},
};

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

pub fn index_path(v: &Value, idxs: &[usize]) -> Option<Value> {
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
    // Map out_batch_idx to the concrete indices in A and B respecting broadcasting
    let a_bidx = map_bc_index(ctx.a_batch, ctx.out_batch_dims, out_batch_idx);
    let b_bidx = map_bc_index(ctx.b_batch, ctx.out_batch_dims, out_batch_idx);

    // Helper to fetch A(b..., i?, k)
    let a_at = |i: Option<usize>, kk: usize| -> WqResult<Value> {
        let mut idx = Vec::with_capacity(a_bidx.len() + if ctx.a_rank >= 2 { 2 } else { 1 });
        idx.extend_from_slice(&a_bidx);
        if ctx.a_rank >= 2 {
            idx.push(i.expect("row index required"));
        }
        idx.push(kk);
        index_path(ctx.a, &idx)
            .ok_or_else(|| WqErr::new(WqErrType::Domain).msg("invalid index while reading A"))
    };

    // Helper to fetch B(b..., k, j?)
    let b_at = |kk: usize, j: Option<usize>| -> WqResult<Value> {
        let mut idx = Vec::with_capacity(b_bidx.len() + if ctx.b_rank >= 2 { 2 } else { 1 });
        idx.extend_from_slice(&b_bidx);
        if ctx.b_rank >= 2 {
            idx.push(kk);
        } else {
            // rank-1: only K axis present
            return index_path(ctx.b, &{
                let mut t = b_bidx.clone();
                t.push(kk);
                t
            })
            .ok_or_else(|| WqErr::new(WqErrType::Domain).msg("invalid index while reading B"));
        }
        idx.push(j.expect("col index required"));
        index_path(ctx.b, &idx)
            .ok_or_else(|| WqErr::new(WqErrType::Domain).msg("invalid index while reading B"))
    };

    // Cases following NumPy-like matmul squeeze rules for 1D inputs
    match (ctx.a_rank >= 2, ctx.b_rank >= 2) {
        (false, false) => {
            // dot product -> scalar
            let mut acc: Option<Value> = None;
            for kk in 0..ctx.k {
                let av = a_at(None, kk)?;
                let bv = b_at(kk, None)?;
                let prod = av.multiply(&bv).map_err(|e| e.into_wqerror())?;
                acc = Some(match acc {
                    None => prod,
                    Some(s) => s.add(&prod).map_err(|e| e.into_wqerror())?,
                });
            }
            Ok(acc.unwrap_or(Value::Int(0)))
        }
        (true, false) => {
            // (A: MxK) x (B: K) -> (M)
            let m = ctx.m_opt.expect("M known when a_rank>=2");
            let mut out_row = Vec::with_capacity(m);
            for i in 0..m {
                let mut acc: Option<Value> = None;
                for kk in 0..ctx.k {
                    let av = a_at(Some(i), kk)?;
                    let bv = b_at(kk, None)?;
                    let prod = av.multiply(&bv).map_err(|e| e.into_wqerror())?;
                    acc = Some(match acc {
                        None => prod,
                        Some(s) => s.add(&prod).map_err(|e| e.into_wqerror())?,
                    });
                }
                out_row.push(acc.unwrap_or(Value::Int(0)));
            }
            Ok(Value::from_items(out_row))
        }
        (false, true) => {
            // (A: K) x (B: KxN) -> (N)
            let n = ctx.n_opt.expect("N known when b_rank>=2");
            let mut out_col = Vec::with_capacity(n);
            for j in 0..n {
                let mut acc: Option<Value> = None;
                for kk in 0..ctx.k {
                    let av = a_at(None, kk)?;
                    let bv = b_at(kk, Some(j))?;
                    let prod = av.multiply(&bv).map_err(|e| e.into_wqerror())?;
                    acc = Some(match acc {
                        None => prod,
                        Some(s) => s.add(&prod).map_err(|e| e.into_wqerror())?,
                    });
                }
                out_col.push(acc.unwrap_or(Value::Int(0)));
            }
            Ok(Value::from_items(out_col))
        }
        (true, true) => {
            // (A: MxK) x (B: KxN) -> (M x N)
            let m = ctx.m_opt.expect("M known when a_rank>=2");
            let n = ctx.n_opt.expect("N known when b_rank>=2");
            let mut out = Vec::with_capacity(m);
            for i in 0..m {
                let mut row = Vec::with_capacity(n);
                for j in 0..n {
                    let mut acc: Option<Value> = None;
                    for kk in 0..ctx.k {
                        let av = a_at(Some(i), kk)?;
                        let bv = b_at(kk, Some(j))?;
                        let prod = av.multiply(&bv).map_err(|e| e.into_wqerror())?;
                        acc = Some(match acc {
                            None => prod,
                            Some(s) => s.add(&prod).map_err(|e| e.into_wqerror())?,
                        });
                    }
                    row.push(acc.unwrap_or(Value::Int(0)));
                }
                out.push(Value::from_items(row));
            }
            Ok(Value::List(out))
        }
    }
}

fn build_batched(ctx: &MmCtx<'_>, out_idx: &mut Vec<usize>) -> WqResult<Value> {
    let depth = out_idx.len();
    if depth == ctx.out_batch_dims.len() {
        return mm_core(ctx, out_idx);
    }
    let dim = ctx.out_batch_dims[depth];
    let mut items = Vec::with_capacity(dim);
    for i in 0..dim {
        out_idx.push(i);
        items.push(build_batched(ctx, out_idx)?);
        out_idx.pop();
    }
    Ok(Value::from_items(items))
}

impl Value {
    pub fn mm(&self, other: &Value) -> WqResult<Value> {
        // Reject non-uniform shapes
        if !self.is_uniform() || !other.is_uniform() {
            return Err(WqErr::new(WqErrType::Domain)
                .msg("left operand must be uniform")
                .got2(self, other));
        }

        let a_shape = self
            .shape_uniform()
            .ok_or_else(|| WqErr::new(WqErrType::Domain).msg("could not determine left shape"))?;
        let b_shape = other
            .shape_uniform()
            .ok_or_else(|| WqErr::new(WqErrType::Domain).msg("could not determine right shape"))?;

        let a_rank = a_shape.len();
        let b_rank = b_shape.len();

        if a_rank == 0 || b_rank == 0 {
            return self.multiply(other).map_err(|e| e.into_wqerror());
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
            return Err(WqErr::new(WqErrType::Length)
                .msg("inner dimensions must match (K)")
                .attach_note(format!("left K={}, right K={}", k_a, k_b)));
        }
        let k = k_a;

        let out_batch = broadcast_shapes(a_batch, b_batch).ok_or_else(|| {
            WqErr::new(WqErrType::Length)
                .msg("batch dimensions are not broadcast-compatible")
                .attach_note(format!(
                    "left batch={:?}, right batch={:?}",
                    a_batch, b_batch
                ))
        })?;

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
        build_batched(&ctx, &mut Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use crate::value::Value;

    fn il(v: &[i64]) -> Value {
        Value::IntList(v.to_vec())
    }

    fn mat(rows: &[&[i64]]) -> Value {
        let rs: Vec<Value> = rows.iter().map(|r| il(r)).collect();
        Value::List(rs)
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
    fn mm_batched_broadcast() {
        // A: (2, 2, 2)
        let a0 = mat(&[&[1, 2], &[3, 4]]);
        let a1 = mat(&[&[5, 6], &[7, 8]]);
        let a = Value::List(vec![a0, a1]);

        // B: (2, 2) broadcast across batch
        let b = mat(&[&[9, 10], &[11, 12]]);

        let res = a.mm(&b).expect("batched AB");
        let exp0 = mat(&[&[31, 34], &[71, 78]]);
        let exp1 = mat(&[&[111, 122], &[151, 166]]);
        let expect = Value::List(vec![exp0, exp1]);
        assert_eq!(res, expect);
    }
}
