use std::sync::Arc;

use rayon::prelude::*;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::mat::index_path;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

/// Transpose a matrix or higher-dimensional array.
/// - Atoms and 1D vectors: returned as-is
/// - 2D matrices: transposed (swap rows and columns)
/// - Higher-dimensional arrays: transpose last 2 axes
pub(crate) fn transpose(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Transpose, [1, 2], &args)?;
    let (v, axes_arg) = match &*args {
        [v] => (v.clone(), None),
        [v, axes] => (v.clone(), Some(axes)),
        _ => unreachable!(),
    };
    let shape = v.shape_uniform().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Transpose)
            .msg("could not determine shape")
            .at_arg(0)
    })?;
    let rank = shape.len();
    let axes = axes_arg.map(|axes| parse_axes(axes, rank)).transpose()?;
    // Atoms and 1D vectors: return as-is
    if rank <= 1 {
        return Ok(v);
    }
    if let Some(axes) = axes {
        return transpose_axes(&v, &shape, &axes);
    }
    // For 2D and higher: transpose last 2 axes
    let m = shape[rank - 2];
    let n = shape[rank - 1];
    // If either dimension is 0, return as-is
    if m == 0 || n == 0 {
        return Ok(v);
    }
    if rank == 2 {
        // Simple 2D transpose
        transpose_2d(&v, m, n)
    } else {
        // Higher-dimensional: recursively transpose last 2 axes
        transpose_batched(&v, &shape)
    }
}

/// Transpose a 2D matrix
fn transpose_2d(v: &Value, m: usize, n: usize) -> WqResult<Value> {
    // Build transposed matrix: new shape is (n, m)
    let cols: WqResult<Vec<Value>> = (0..n)
        .into_par_iter()
        .map(|j| {
            let mut col = Vec::with_capacity(m);
            for i in 0..m {
                let elem = index_path(v, &[i, j]).ok_or_else(|| {
                    WqError::new(WqErrorType::Domain)
                        .src(BE::Transpose)
                        .msg(format!("could not access element at [{}, {}]", i, j))
                })?;
                col.push(elem);
            }
            Ok(Value::from_items(col))
        })
        .collect();
    Ok(Value::List(Arc::new(cols?)))
}

/// Transpose higher-dimensional arrays (transpose last 2 axes for each batch)
fn transpose_batched(v: &Value, shape: &[usize]) -> WqResult<Value> {
    let rank = shape.len();
    if rank == 2 {
        return transpose_2d(v, shape[0], shape[1]);
    }
    // Recursively process each element in the batch dimension
    let batch_size = shape[0];
    let items: WqResult<Vec<Value>> = (0..batch_size)
        .into_par_iter()
        .map(|i| {
            let elem = index_path(v, &[i]).ok_or_else(|| {
                WqError::new(WqErrorType::Domain)
                    .src(BE::Transpose)
                    .msg(format!("could not access batch element at [{}]", i))
            })?;
            let transposed = transpose_batched(&elem, &shape[1..])?;
            Ok(transposed)
        })
        .collect();
    Ok(Value::List(Arc::new(items?)))
}

fn parse_axes(v: &Value, rank: usize) -> WqResult<Vec<usize>> {
    let mut raw_axes = Vec::new();
    match v {
        Value::Int(n) => raw_axes.push(*n),
        Value::IntList(items) => raw_axes.extend(items.iter().copied()),
        Value::List(items) => {
            raw_axes.reserve(items.len());
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Int(n) => raw_axes.push(*n),
                    other => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Transpose)
                            .msg("axis list must contain ints")
                            .at_arg(1)
                            .unexpected_element(other, i));
                    }
                }
            }
        }
        other => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Transpose)
                .msg("expected int or list<int> axes")
                .at_arg(1)
                .got1(other));
        }
    }

    if raw_axes.len() != rank {
        return Err(WqError::new(WqErrorType::Length)
            .src(BE::Transpose)
            .msg("axis list length must match array rank")
            .at_arg(1)
            .attach_note(format!(
                "rank is {rank}, axis list length is {}",
                raw_axes.len()
            )));
    }

    let mut axes = Vec::with_capacity(raw_axes.len());
    for (i, raw_axis) in raw_axes.into_iter().enumerate() {
        let axis = usize::try_from(raw_axis).map_err(|_| {
            WqError::new(WqErrorType::Domain)
                .src(BE::Transpose)
                .msg("axis must be non-negative")
                .at_arg(1)
                .attach_note(format!("at index {i}"))
        })?;
        if axis >= rank {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Transpose)
                .msg("axis out of range")
                .at_arg(1)
                .attach_note(format!("axis {axis} is not in 0..{rank}")));
        }
        axes.push(axis);
    }

    validate_axes_cover_result(&axes)?;
    Ok(axes)
}

fn validate_axes_cover_result(axes: &[usize]) -> WqResult<()> {
    let result_rank = axes.iter().copied().max().map_or(0, |axis| axis + 1);
    let mut seen = vec![false; result_rank];
    for &axis in axes {
        seen[axis] = true;
    }
    for (axis, present) in seen.iter().enumerate() {
        if !present {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Transpose)
                .msg("axis list must use contiguous result axes")
                .at_arg(1)
                .attach_note(format!("missing result axis {axis}")));
        }
    }
    Ok(())
}

fn shape_after_axes(shape: &[usize], axes: &[usize]) -> Vec<usize> {
    let result_rank = axes.iter().copied().max().map_or(0, |axis| axis + 1);
    let mut out_shape: Vec<Option<usize>> = vec![None; result_rank];
    for (src_axis, &dst_axis) in axes.iter().enumerate() {
        out_shape[dst_axis] = Some(match out_shape[dst_axis] {
            Some(dim) => dim.min(shape[src_axis]),
            None => shape[src_axis],
        });
    }
    out_shape
        .into_iter()
        .map(|dim| dim.expect("validated axes cover every result axis"))
        .collect()
}

fn transpose_axes(v: &Value, shape: &[usize], axes: &[usize]) -> WqResult<Value> {
    if shape.contains(&0) {
        return Ok(v.clone());
    }
    let out_shape = shape_after_axes(shape, axes);
    build_transposed_axes(v, axes, &out_shape, &[])
}

fn build_transposed_axes(
    v: &Value,
    axes: &[usize],
    out_shape: &[usize],
    out_idx: &[usize],
) -> WqResult<Value> {
    if out_idx.len() == out_shape.len() {
        let src_idx: Vec<usize> = axes.iter().map(|&axis| out_idx[axis]).collect();
        return index_path(v, &src_idx).ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BE::Transpose)
                .msg(format!("could not access element at {src_idx:?}"))
        });
    }

    let dim = out_shape[out_idx.len()];
    let items: WqResult<Vec<Value>> = (0..dim)
        .into_par_iter()
        .map(|i| {
            let mut next_idx = Vec::with_capacity(out_idx.len() + 1);
            next_idx.extend_from_slice(out_idx);
            next_idx.push(i);
            build_transposed_axes(v, axes, out_shape, &next_idx)
        })
        .collect();
    Ok(Value::from_items(items?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Vm;

    fn il(v: &[i64]) -> Value {
        Value::IntList(v.to_vec().into())
    }

    fn mat(rows: &[&[i64]]) -> Value {
        let rs: Vec<Value> = rows.iter().map(|r| il(r)).collect();
        Value::List(Arc::new(rs))
    }

    fn arr_2x3x4() -> Value {
        let mut n = 0;
        let mut planes = Vec::new();
        for _ in 0..2 {
            let mut rows = Vec::new();
            for _ in 0..3 {
                let row: Vec<i64> = (n..n + 4).collect();
                n += 4;
                rows.push(il(&row));
            }
            planes.push(Value::List(Arc::new(rows)));
        }
        Value::List(Arc::new(planes))
    }

    // Transpose tests
    // =================================================================

    #[test]
    fn transpose_atom() {
        let atom = Value::Int(42);
        let res = transpose(BuiltinFnArgs::from(atom.clone())).expect("transpose atom");
        assert_eq!(res, atom);
    }

    #[test]
    fn transpose_vector() {
        let vec = il(&[1, 2, 3, 4, 5]);
        let res = transpose(BuiltinFnArgs::from(vec.clone())).expect("transpose vector");
        assert_eq!(res, vec);
    }

    #[test]
    fn transpose_2x3_matrix() {
        // Original: [[1, 2, 3], [4, 5, 6]]
        let m = mat(&[&[1, 2, 3], &[4, 5, 6]]);
        let res = transpose(BuiltinFnArgs::from(m)).expect("transpose 2x3");
        // Expected: [[1, 4], [2, 5], [3, 6]]
        let expect = mat(&[&[1, 4], &[2, 5], &[3, 6]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_square_matrix() {
        // Original: [[1, 2], [3, 4]]
        let m = mat(&[&[1, 2], &[3, 4]]);
        let res = transpose(BuiltinFnArgs::from(m)).expect("transpose square");
        // Expected: [[1, 3], [2, 4]]
        let expect = mat(&[&[1, 3], &[2, 4]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_double_transpose() {
        let m = mat(&[&[1, 2, 3], &[4, 5, 6]]);
        let t1 = transpose(BuiltinFnArgs::from(m.clone())).expect("first transpose");
        let t2 = transpose(BuiltinFnArgs::from(t1)).expect("second transpose");
        assert_eq!(t2, m);
    }

    #[test]
    fn transpose_3d_array() {
        // 3D array: (2, 2, 3) - two 2x3 matrices
        let m0 = mat(&[&[1, 2, 3], &[4, 5, 6]]);
        let m1 = mat(&[&[7, 8, 9], &[10, 11, 12]]);
        let arr = Value::List(Arc::new(vec![m0, m1]));

        let res = transpose(BuiltinFnArgs::from(arr)).expect("transpose 3d");

        // Expected: (2, 3, 2) - two 3x2 matrices
        let exp0 = mat(&[&[1, 4], &[2, 5], &[3, 6]]);
        let exp1 = mat(&[&[7, 10], &[8, 11], &[9, 12]]);
        let expect = Value::List(Arc::new(vec![exp0, exp1]));

        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_4d_array() {
        let _vm = Vm::new(vec![]);
        // 4D array: (2, 2, 2, 2) - batch of batches of 2x2 matrices
        let m00 = mat(&[&[1, 2], &[3, 4]]);
        let m01 = mat(&[&[5, 6], &[7, 8]]);
        let b0 = Value::List(Arc::new(vec![m00, m01]));

        let m10 = mat(&[&[9, 10], &[11, 12]]);
        let m11 = mat(&[&[13, 14], &[15, 16]]);
        let b1 = Value::List(Arc::new(vec![m10, m11]));

        let arr = Value::List(Arc::new(vec![b0, b1]));

        let res = transpose(BuiltinFnArgs::from(arr)).expect("transpose 4d");

        // Expected: transpose only last 2 axes in each 2x2 matrix
        let exp00 = mat(&[&[1, 3], &[2, 4]]);
        let exp01 = mat(&[&[5, 7], &[6, 8]]);
        let expb0 = Value::List(Arc::new(vec![exp00, exp01]));

        let exp10 = mat(&[&[9, 11], &[10, 12]]);
        let exp11 = mat(&[&[13, 15], &[14, 16]]);
        let expb1 = Value::List(Arc::new(vec![exp10, exp11]));

        let expect = Value::List(Arc::new(vec![expb0, expb1]));

        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_single_row() {
        // 1x3 matrix becomes 3x1 matrix
        let m = mat(&[&[1, 2, 3]]);
        let res = transpose(BuiltinFnArgs::from(m)).expect("transpose single row");
        let expect = mat(&[&[1], &[2], &[3]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_single_col() {
        // 3x1 matrix becomes 1x3 matrix
        let m = mat(&[&[1], &[2], &[3]]);
        let res = transpose(BuiltinFnArgs::from(m)).expect("transpose single col");
        let expect = mat(&[&[1, 2, 3]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_axis_list_permutates_by_new_axis_positions() {
        let arr = arr_2x3x4();
        let axes = il(&[2, 0, 1]);

        let res =
            transpose(BuiltinFnArgs::from(vec![arr.clone(), axes])).expect("transpose with axes");

        assert_eq!(res.shape_uniform(), Some(vec![3, 4, 2]));
        assert_eq!(index_path(&res, &[0, 0, 0]), index_path(&arr, &[0, 0, 0]));
        assert_eq!(index_path(&res, &[1, 2, 0]), index_path(&arr, &[0, 1, 2]));
        assert_eq!(index_path(&res, &[2, 3, 1]), index_path(&arr, &[1, 2, 3]));
    }

    #[test]
    fn transpose_repeated_axes_selects_diagonal() {
        let m = mat(&[&[1, 2, 3, 4], &[5, 6, 7, 8], &[9, 10, 11, 12]]);
        let axes = il(&[0, 0]);

        let res = transpose(BuiltinFnArgs::from(vec![m, axes])).expect("transpose repeated axes");

        assert_eq!(res, il(&[1, 6, 11]));
    }

    #[test]
    fn transpose_axis_list_length_must_match_rank() {
        let m = mat(&[&[1, 2], &[3, 4]]);
        let axes = il(&[0]);

        let err = transpose(BuiltinFnArgs::from(vec![m, axes])).expect_err("axis length mismatch");

        assert_eq!(err.err_type, WqErrorType::Length);
    }

    #[test]
    fn transpose_axis_list_rejects_missing_result_axis() {
        let arr = arr_2x3x4();
        let axes = il(&[0, 2, 2]);

        let err = transpose(BuiltinFnArgs::from(vec![arr, axes])).expect_err("missing result axis");

        assert_eq!(err.err_type, WqErrorType::Domain);
    }
}
