use std::sync::Arc;

use rayon::prelude::*;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::mat::index_path;
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::wqerror::{WqError, WqErrorType};

/// Transpose a matrix or higher-dimensional array.
/// - Atoms and 1D vectors: returned as-is
/// - 2D matrices: transposed (swap rows and columns)
/// - Higher-dimensional arrays: transpose last 2 axes
pub(super) fn transpose(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Transpose, [1], &args)?;
    let v = args.into_iter().next().unwrap();
    let shape = v.shape_uniform().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Transpose)
            .msg("could not determine shape")
            .at_arg(0)
    })?;
    let rank = shape.len();
    // Atoms and 1D vectors: return as-is
    if rank <= 1 {
        return Ok(v);
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

    // Transpose tests
    // =================================================================

    #[test]
    fn transpose_atom() {
        let mut vm = Vm::new(vec![]);
        let atom = Value::Int(42);
        let res = transpose(&mut vm, BuiltinFnArgs::from(atom.clone())).expect("transpose atom");
        assert_eq!(res, atom);
    }

    #[test]
    fn transpose_vector() {
        let mut vm = Vm::new(vec![]);
        let vec = il(&[1, 2, 3, 4, 5]);
        let res = transpose(&mut vm, BuiltinFnArgs::from(vec.clone())).expect("transpose vector");
        assert_eq!(res, vec);
    }

    #[test]
    fn transpose_2x3_matrix() {
        let mut vm = Vm::new(vec![]);
        // Original: [[1, 2, 3], [4, 5, 6]]
        let m = mat(&[&[1, 2, 3], &[4, 5, 6]]);
        let res = transpose(&mut vm, BuiltinFnArgs::from(m)).expect("transpose 2x3");
        // Expected: [[1, 4], [2, 5], [3, 6]]
        let expect = mat(&[&[1, 4], &[2, 5], &[3, 6]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_square_matrix() {
        let mut vm = Vm::new(vec![]);
        // Original: [[1, 2], [3, 4]]
        let m = mat(&[&[1, 2], &[3, 4]]);
        let res = transpose(&mut vm, BuiltinFnArgs::from(m)).expect("transpose square");
        // Expected: [[1, 3], [2, 4]]
        let expect = mat(&[&[1, 3], &[2, 4]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_double_transpose() {
        let mut vm = Vm::new(vec![]);
        let m = mat(&[&[1, 2, 3], &[4, 5, 6]]);
        let t1 = transpose(&mut vm, BuiltinFnArgs::from(m.clone())).expect("first transpose");
        let t2 = transpose(&mut vm, BuiltinFnArgs::from(t1)).expect("second transpose");
        assert_eq!(t2, m);
    }

    #[test]
    fn transpose_3d_array() {
        let mut vm = Vm::new(vec![]);
        // 3D array: (2, 2, 3) - two 2x3 matrices
        let m0 = mat(&[&[1, 2, 3], &[4, 5, 6]]);
        let m1 = mat(&[&[7, 8, 9], &[10, 11, 12]]);
        let arr = Value::List(Arc::new(vec![m0, m1]));

        let res = transpose(&mut vm, BuiltinFnArgs::from(arr)).expect("transpose 3d");

        // Expected: (2, 3, 2) - two 3x2 matrices
        let exp0 = mat(&[&[1, 4], &[2, 5], &[3, 6]]);
        let exp1 = mat(&[&[7, 10], &[8, 11], &[9, 12]]);
        let expect = Value::List(Arc::new(vec![exp0, exp1]));

        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_4d_array() {
        let mut vm = Vm::new(vec![]);
        // 4D array: (2, 2, 2, 2) - batch of batches of 2x2 matrices
        let m00 = mat(&[&[1, 2], &[3, 4]]);
        let m01 = mat(&[&[5, 6], &[7, 8]]);
        let b0 = Value::List(Arc::new(vec![m00, m01]));

        let m10 = mat(&[&[9, 10], &[11, 12]]);
        let m11 = mat(&[&[13, 14], &[15, 16]]);
        let b1 = Value::List(Arc::new(vec![m10, m11]));

        let arr = Value::List(Arc::new(vec![b0, b1]));

        let res = transpose(&mut vm, BuiltinFnArgs::from(arr)).expect("transpose 4d");

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
        let mut vm = Vm::new(vec![]);
        // 1x3 matrix becomes 3x1 matrix
        let m = mat(&[&[1, 2, 3]]);
        let res = transpose(&mut vm, BuiltinFnArgs::from(m)).expect("transpose single row");
        let expect = mat(&[&[1], &[2], &[3]]);
        assert_eq!(res, expect);
    }

    #[test]
    fn transpose_single_col() {
        let mut vm = Vm::new(vec![]);
        // 3x1 matrix becomes 1x3 matrix
        let m = mat(&[&[1], &[2], &[3]]);
        let res = transpose(&mut vm, BuiltinFnArgs::from(m)).expect("transpose single col");
        let expect = mat(&[&[1, 2, 3]]);
        assert_eq!(res, expect);
    }
}
