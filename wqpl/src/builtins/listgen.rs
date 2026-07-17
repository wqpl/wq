pub(super) mod transpose;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::range::make_range;
use crate::value::seq::IntRangeData;
use crate::value::{Value, WqResult};
use crate::wqerror::{Requirement, WqError, WqErrorType};

const LISTGEN_CACHE_CAPACITY: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum ListGenKey {
    Till(Vec<i64>),
    Alloc(Vec<i64>, Option<i64>),
    Iota(Vec<i64>),
}

static LISTGEN_CACHE: Mutex<Option<HashMap<ListGenKey, Value>>> = Mutex::new(None);

fn cache_get(key: &ListGenKey) -> Option<Value> {
    LISTGEN_CACHE.lock().unwrap().as_ref()?.get(key).cloned()
}

fn cache_insert(key: ListGenKey, value: Value) -> Value {
    let mut guard = LISTGEN_CACHE.lock().unwrap();
    let cache = guard.get_or_insert_with(HashMap::new);
    if cache.len() >= LISTGEN_CACHE_CAPACITY {
        cache.clear();
    }
    cache.insert(key, value.clone());
    value
}

/// Extract a shape vector of i64 from common Value shapes.
/// Returns None for types that are not simple int-based shapes.
fn extract_int_shape(v: &Value) -> Option<Vec<i64>> {
    v.exact_int_seq().map(|items| items.to_vec())
}

pub(super) fn alloc(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Alloc, [1, 2], &args)?;
    let (shape, fill) = match &*args {
        [shape] => (shape, Value::Int(0)),
        [shape, fill] => (shape, fill.clone()),
        _ => unreachable!(),
    };
    let dims = parse_shape(shape, BE::Alloc, 0)?;
    if let (Some(s), Value::Int(f)) = (extract_int_shape(shape), &fill) {
        let key = ListGenKey::Alloc(s, Some(*f));
        if let Some(v) = cache_get(&key) {
            return Ok(v);
        }
        let val = build_filled_shape(&dims, &fill).expect("int fill should have packed builder");
        return Ok(cache_insert(key, val));
    }
    if let Some(val) = build_filled_shape(&dims, &fill) {
        return Ok(val);
    }
    let mut generator = || fill.clone();
    Ok(build_from_parsed_shape(&dims, &mut generator))
}

pub(super) fn til(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Til, [1], &args)?;
    if let Value::Int(n) = args[0] {
        let Ok(len) = usize::try_from(n) else {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Til)
                .expected(shape_requirement())
                .at_arg(0)
                .got1(&args[0]));
        };
        return Ok(Value::IntRange(Arc::new(IntRangeData::new(0, 1, len))));
    }
    let dims = parse_shape(&args[0], BE::Til, 0)?;
    if dims.is_empty() {
        return Ok(Value::Int(0));
    }
    if let Some(shape) = extract_int_shape(&args[0]) {
        let key = ListGenKey::Till(shape);
        if let Some(v) = cache_get(&key) {
            return Ok(v);
        }
        let mut next = 0i64;
        let mut generator = || {
            let v = Value::Int(next);
            next += 1;
            v
        };
        let val = build_from_parsed_shape(&dims, &mut generator);
        return Ok(cache_insert(key, val));
    }
    let mut next = 0i64;
    let mut generator = || {
        let v = Value::Int(next);
        next += 1;
        v
    };
    Ok(build_from_parsed_shape(&dims, &mut generator))
}

pub(super) fn iota(args: BuiltinFnArgs) -> WqResult<Value> {
    fn build_coords(dims: &[usize], prefix: &mut Vec<i64>) -> WqResult<Value> {
        if dims.is_empty() {
            return Ok(Value::IntList(Arc::new(prefix.clone())));
        }
        let n = dims[0];
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            prefix.push(i.try_into().map_err(|e| {
                WqError::new(WqErrorType::Domain)
                    .src(BE::Iota)
                    .attach_note(e)
            })?);
            out.push(build_coords(&dims[1..], prefix)?);
            prefix.pop();
        }
        Ok(Value::List(Arc::new(out)))
    }

    check_arity(BE::Iota, [1], &args)?;
    match &args[0] {
        // 1D case: simple range 0..n-1
        Value::Int(n) => {
            let Ok(len) = usize::try_from(*n) else {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Iota)
                    .expected(shape_requirement())
                    .at_arg(0)
                    .got1(&args[0]));
            };
            Ok(Value::IntRange(Arc::new(IntRangeData::new(0, 1, len))))
        }
        // Multidimensional: nested grid of coordinate vectors, shaped by dims
        _ => {
            let dims = parse_shape(&args[0], BE::Iota, 0)?;
            if dims.is_empty() {
                // Preserve existing behavior for empty shape
                return Ok(Value::IntList(Arc::new(vec![])));
            }
            if let Some(shape) = extract_int_shape(&args[0]) {
                let key = ListGenKey::Iota(shape);
                if let Some(v) = cache_get(&key) {
                    return Ok(v);
                }
                let mut prefix = Vec::<i64>::with_capacity(dims.len());
                let val = build_coords(&dims, &mut prefix)?;
                return Ok(cache_insert(key, val));
            }
            let mut prefix = Vec::<i64>::with_capacity(dims.len());
            Ok(build_coords(&dims, &mut prefix)?)
        }
    }
}

pub(super) fn range(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Range, [2, 3], &args)?;
    match &*args {
        [start, end] => make_range(start, end, None, false).map_err(|e| e.src(BE::Range)),
        [start, end, step] => {
            make_range(start, end, Some(step), false).map_err(|e| e.src(BE::Range))
        }
        _ => unreachable!(),
    }
}

pub(super) fn reshape(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Reshape, [2], &args)?;
    let flattened = args[0].flatten();
    let dims = parse_shape(&args[1], BE::Reshape, 1)?;
    if flattened.is_empty() {
        let mut generator = || Value::Int(0);
        return Ok(build_from_parsed_shape(&dims, &mut generator));
    }
    let n = flattened.len();
    let mut i = 0usize;
    let mut generator = || {
        let v = flattened[i].clone();
        i += 1;
        if i == n {
            i = 0;
        }
        v
    };
    Ok(build_from_parsed_shape(&dims, &mut generator))
}

pub(super) fn repeat(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Repeat, [2], &args)?;
    let count = match &args[1] {
        Value::Int(n) => {
            if *n < 0 {
                return Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Repeat)
                    .expected(Requirement::non_negative(Requirement::INT))
                    .at_arg(1)
                    .got1(&args[1]));
            }
            usize::try_from(*n).map_err(|_| {
                WqError::new(WqErrorType::Domain)
                    .src(BE::Repeat)
                    .msg("repeat count is too large")
                    .at_arg(1)
            })?
        }
        other => {
            return Err(WqError::new(WqErrorType::Domain)
                .src(BE::Repeat)
                .expected(Requirement::non_negative(Requirement::INT))
                .at_arg(1)
                .got1(other));
        }
    };
    let too_large = || {
        WqError::new(WqErrorType::Domain)
            .src(BE::Repeat)
            .msg("repeated value is too large")
    };
    let repeated_len = |len: usize| len.checked_mul(count).ok_or_else(&too_large);

    match &args[0] {
        Value::String(s) => {
            let mut repeated = String::new();
            repeated
                .try_reserve_exact(repeated_len(s.len())?)
                .map_err(|_| too_large())?;
            for _ in 0..count {
                repeated.push_str(s);
            }
            Ok(Value::String(Arc::new(repeated)))
        }
        Value::Char(c) => {
            let mut repeated = String::new();
            repeated
                .try_reserve_exact(repeated_len(c.len_utf8())?)
                .map_err(|_| too_large())?;
            repeated.extend(std::iter::repeat_n(*c, count));
            Ok(Value::String(Arc::new(repeated)))
        }
        Value::IntList(items) => {
            let mut res = Vec::new();
            res.try_reserve_exact(repeated_len(items.len())?)
                .map_err(|_| too_large())?;
            for _ in 0..count {
                res.extend(items.iter().copied());
            }
            Ok(Value::IntList(Arc::new(res)))
        }
        Value::IntRange(items) if count == 1 => Ok(Value::IntRange(items.clone())),
        Value::IntRange(items) => {
            let mut res = Vec::new();
            res.try_reserve_exact(repeated_len(items.len())?)
                .map_err(|_| too_large())?;
            for _ in 0..count {
                res.extend(items.iter());
            }
            Ok(Value::IntList(Arc::new(res)))
        }
        Value::FloatList(items) => {
            let mut res = Vec::new();
            res.try_reserve_exact(repeated_len(items.len())?)
                .map_err(|_| too_large())?;
            for _ in 0..count {
                res.extend(items.iter().copied());
            }
            Ok(Value::FloatList(Arc::new(res)))
        }
        Value::BoolList(items) => {
            let mut res = Vec::new();
            res.try_reserve_exact(repeated_len(items.len())?)
                .map_err(|_| too_large())?;
            for _ in 0..count {
                res.extend(items.iter().copied());
            }
            Ok(Value::BoolList(Arc::new(res)))
        }
        Value::List(items) => {
            let mut res = Vec::new();
            res.try_reserve_exact(repeated_len(items.len())?)
                .map_err(|_| too_large())?;
            for _ in 0..count {
                res.extend(items.iter().cloned());
            }
            Ok(Value::List(Arc::new(res)))
        }

        other => {
            let mut res = Vec::new();
            res.try_reserve_exact(count).map_err(|_| too_large())?;
            for _ in 0..count {
                res.push(other.clone());
            }
            Ok(Value::from_items(res))
        }
    }
}

pub(super) fn wq_where(args: BuiltinFnArgs) -> WqResult<Value> {
    fn fmt_path(path: &[i64]) -> String {
        if path.is_empty() {
            "[]".to_string()
        } else {
            path.iter().map(|i| format!("[{}]", i)).collect()
        }
    }

    fn fail_type<T>(got: &Value, path: &[i64]) -> WqResult<T> {
        Err(WqError::new(WqErrorType::Domain)
            .src(BE::Where)
            .expected(where_requirement())
            .at_arg(0)
            .attach_note(format!("at path {}", fmt_path(path)))
            .got1(got))
    }

    // Helper used by the nested collector: emit Int for length-1 paths, IntList
    // otherwise.
    fn push_coord(out: &mut Vec<Value>, coord: Vec<i64>) {
        if coord.len() == 1 {
            out.push(Value::Int(coord[0]));
        } else {
            out.push(Value::IntList(Arc::new(coord)));
        }
    }

    // Helper: flat 1D case (list of ints/bools).
    fn where_1d_list(items: &[Value]) -> WqResult<Value> {
        let mut indices = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match item {
                Value::Int(n) if *n != 0 => indices.push(i.try_into().map_err(|e| {
                    WqError::new(WqErrorType::Domain)
                        .src(BE::Where)
                        .attach_note(e)
                })?),
                Value::Bool(b) if *b => indices.push(i.try_into().map_err(|e| {
                    WqError::new(WqErrorType::Domain)
                        .src(BE::Where)
                        .attach_note(e)
                })?),
                Value::Int(_) | Value::Bool(_) => {}
                _ => {
                    // precise path for flat case: [i]
                    return fail_type::<Value>(
                        item,
                        &[i.try_into().map_err(|e| {
                            WqError::new(WqErrorType::Domain)
                                .src(BE::Where)
                                .attach_note(e)
                        })?],
                    );
                }
            }
        }
        Ok(Value::IntList(Arc::new(indices)))
    }

    // Recursively collect coordinates. Single-index paths become Int atoms.
    fn collect_coords(v: &Value, prefix: &mut Vec<i64>, out: &mut Vec<Value>) -> WqResult<()> {
        match v {
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    prefix.push(i.try_into().map_err(|e| {
                        WqError::new(WqErrorType::Domain)
                            .src(BE::Where)
                            .attach_note(e)
                    })?);
                    collect_coords(item, prefix, out)?;
                    prefix.pop();
                }
                Ok(())
            }
            value @ (Value::IntList(_) | Value::IntRange(_)) => {
                let items = value
                    .packed_int_seq()
                    .expect("guard checked value is a packed int sequence");
                for (i, n) in items.iter().enumerate() {
                    if n != 0 {
                        let mut coord = prefix.clone();
                        coord.push(i.try_into().map_err(|e| {
                            WqError::new(WqErrorType::Domain)
                                .src(BE::Where)
                                .attach_note(e)
                        })?);
                        push_coord(out, coord);
                    }
                }
                Ok(())
            }
            Value::BoolList(items) => {
                for (i, &b) in items.iter().enumerate() {
                    if b {
                        let mut coord = prefix.clone();
                        coord.push(i.try_into().map_err(|e| {
                            WqError::new(WqErrorType::Domain)
                                .src(BE::Where)
                                .attach_note(e)
                        })?);
                        push_coord(out, coord);
                    }
                }
                Ok(())
            }
            Value::Int(n) => {
                if *n != 0 {
                    push_coord(out, prefix.clone());
                }
                Ok(())
            }
            Value::Bool(b) => {
                if *b {
                    push_coord(out, prefix.clone());
                }
                Ok(())
            }
            // Anything else: report precise nested path
            other => fail_type::<()>(other, prefix),
        }
    }

    check_arity(BE::Where, [1], &args)?;
    match &args[0] {
        // Flat vector of ints -> indices as list of ints
        value @ (Value::IntList(_) | Value::IntRange(_)) => {
            let mut indices = Vec::new();
            let items = value
                .packed_int_seq()
                .expect("guard checked value is a packed int sequence");
            for (i, n) in items.iter().enumerate() {
                if n != 0 {
                    indices.push(i.try_into().map_err(|e| {
                        WqError::new(WqErrorType::Domain)
                            .src(BE::Where)
                            .attach_note(e)
                    })?);
                }
            }
            Ok(Value::IntList(Arc::new(indices)))
        }
        Value::BoolList(items) => {
            let mut indices = Vec::new();
            for (i, b) in items.iter().enumerate() {
                if *b {
                    indices.push(i.try_into().map_err(|e| {
                        WqError::new(WqErrorType::Domain)
                            .src(BE::Where)
                            .attach_note(e)
                    })?);
                }
            }
            Ok(Value::IntList(Arc::new(indices)))
        }
        // Generic list: nested -> coordinate vectors (with atom for length-1), else flat
        // ints/bools.
        Value::List(items) => {
            let has_nested = items.iter().any(|x| {
                matches!(
                    x,
                    Value::List(_) | Value::IntList(_) | Value::IntRange(_) | Value::BoolList(_)
                )
            });
            if has_nested {
                let mut out = Vec::new();
                let mut pref = Vec::new();
                collect_coords(&args[0], &mut pref, &mut out)?;
                Ok(Value::List(Arc::new(out)))
            } else {
                where_1d_list(items)
            }
        }
        other => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Where)
            .expected(where_requirement())
            .at_arg(0)
            .got1(other)),
    }
}

fn where_requirement() -> Requirement {
    Requirement::phrase(
        "list whose leaves are ints or bools",
        "lists whose leaves are ints or bools",
    )
}

fn shape_requirement() -> Requirement {
    Requirement::one_of([
        Requirement::non_negative(Requirement::INT),
        Requirement::list(Requirement::non_negative(Requirement::INT)),
    ])
}

fn parse_shape(v: &Value, src: BE, position: usize) -> WqResult<Vec<usize>> {
    let Some(dims) = v.exact_int_seq() else {
        return Err(WqError::new(WqErrorType::Domain)
            .src(src)
            .expected(shape_requirement())
            .at_arg(position)
            .got1(v));
    };
    dims.iter()
        .enumerate()
        .map(|(i, d)| {
            usize::try_from(d).map_err(|_| {
                let error = WqError::new(WqErrorType::Domain)
                    .src(src)
                    .expected(shape_requirement())
                    .at_arg(position);
                if dims.is_atom() {
                    error.got1(v)
                } else {
                    error.got_at_index(&Value::Int(d), i)
                }
            })
        })
        .collect()
}

fn build_from_parsed_shape<F>(shape: &[usize], next: &mut F) -> Value
where
    F: FnMut() -> Value,
{
    match shape.len() {
        0 => next(),
        1 => {
            let n = shape[0];
            if n == 0 {
                return Value::IntList(Arc::new(vec![]));
            }
            let mut tmp: Vec<Value> = Vec::with_capacity(n);
            for _ in 0..n {
                let v = next();
                tmp.push(v);
            }
            Value::from_items(tmp)
        }
        _ => {
            let m = shape[0];
            let mut out = Vec::with_capacity(m);
            for _ in 0..m {
                out.push(build_from_parsed_shape(&shape[1..], next));
            }
            Value::List(Arc::new(out))
        }
    }
}

fn build_filled_shape(shape: &[usize], fill: &Value) -> Option<Value> {
    match fill {
        Value::Int(n) => Some(build_filled_int_shape(shape, *n)),
        Value::Bool(b) => Some(build_filled_bool_shape(shape, *b)),
        Value::Float(f) => Some(build_filled_float_shape(shape, *f)),
        _ => None,
    }
}

fn build_filled_int_shape(shape: &[usize], fill: i64) -> Value {
    match shape {
        [] => Value::Int(fill),
        [n] => Value::IntList(Arc::new(vec![fill; *n])),
        [n, rest @ ..] => {
            let mut out = Vec::with_capacity(*n);
            for _ in 0..*n {
                out.push(build_filled_int_shape(rest, fill));
            }
            Value::List(Arc::new(out))
        }
    }
}

fn build_filled_bool_shape(shape: &[usize], fill: bool) -> Value {
    match shape {
        [] => Value::Bool(fill),
        [n] => Value::BoolList(Arc::new(vec![fill; *n])),
        [n, rest @ ..] => {
            let mut out = Vec::with_capacity(*n);
            for _ in 0..*n {
                out.push(build_filled_bool_shape(rest, fill));
            }
            Value::List(Arc::new(out))
        }
    }
}

fn build_filled_float_shape(shape: &[usize], fill: ordered_float::OrderedFloat<f64>) -> Value {
    match shape {
        [] => Value::Float(fill),
        [n] => Value::FloatList(Arc::new(vec![fill; *n])),
        [n, rest @ ..] => {
            let mut out = Vec::with_capacity(*n);
            for _ in 0..*n {
                out.push(build_filled_float_shape(rest, fill));
            }
            Value::List(Arc::new(out))
        }
    }
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;
    use smallvec::smallvec;

    use super::*;

    #[test]
    fn ints_empty_shape() {
        assert_eq!(
            til(BuiltinFnArgs::from(Value::List(Arc::new(vec![])))).expect("til succeeds"),
            Value::Int(0)
        );
    }

    #[test]
    fn iota_zero() {
        let result = iota(BuiltinFnArgs::from(Value::Int(0))).expect("iota succeeds");
        assert!(matches!(&result, Value::IntRange(range) if range.len() == 0));
        assert_eq!(result, Value::IntList(Arc::new(vec![])));
    }

    #[test]
    fn scalar_til_and_iota_return_virtual_ranges() {
        let tilled = til(BuiltinFnArgs::from(Value::Int(4))).expect("til succeeds");
        assert!(matches!(
            &tilled,
            Value::IntRange(range) if range.start() == 0 && range.step() == 1 && range.len() == 4
        ));
        assert_eq!(tilled, Value::IntList(Arc::new(vec![0, 1, 2, 3])));

        let iotaed = iota(BuiltinFnArgs::from(Value::Int(4))).expect("iota succeeds");
        assert!(matches!(
            &iotaed,
            Value::IntRange(range) if range.start() == 0 && range.step() == 1 && range.len() == 4
        ));
        assert_eq!(iotaed, Value::IntList(Arc::new(vec![0, 1, 2, 3])));
    }

    #[test]
    fn alloc_uses_packed_fill_storage_for_atoms() {
        assert_eq!(
            alloc(BuiltinFnArgs::from(Value::Int(3))).expect("alloc succeeds"),
            Value::IntList(Arc::new(vec![0, 0, 0]))
        );
        assert_eq!(
            alloc(BuiltinFnArgs::from(smallvec![Value::Int(3), Value::Int(7)]))
                .expect("alloc succeeds"),
            Value::IntList(Arc::new(vec![7, 7, 7]))
        );
        assert_eq!(
            alloc(BuiltinFnArgs::from(smallvec![
                Value::Int(2),
                Value::Bool(true)
            ]))
            .expect("alloc succeeds"),
            Value::BoolList(Arc::new(vec![true, true]))
        );
        assert_eq!(
            alloc(BuiltinFnArgs::from(smallvec![
                Value::Int(2),
                Value::Float(OrderedFloat(1.5))
            ]))
            .expect("alloc succeeds"),
            Value::FloatList(Arc::new(vec![OrderedFloat(1.5), OrderedFloat(1.5)]))
        );
    }

    #[test]
    fn shaped_alloc_uses_packed_leaf_storage() {
        let shape = Value::IntList(Arc::new(vec![2, 3]));
        assert_eq!(
            alloc(BuiltinFnArgs::from(smallvec![shape, Value::Int(4)])).expect("alloc succeeds"),
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![4, 4, 4])),
                Value::IntList(Arc::new(vec![4, 4, 4]))
            ]))
        );
    }

    #[test]
    fn repeat_preserves_packed_list_element_semantics() {
        assert_eq!(
            repeat(BuiltinFnArgs::from(smallvec![
                Value::FloatList(Arc::new(vec![OrderedFloat(1.5), OrderedFloat(2.5)])),
                Value::Int(2),
            ]))
            .expect("repeat succeeds"),
            Value::FloatList(Arc::new(vec![
                OrderedFloat(1.5),
                OrderedFloat(2.5),
                OrderedFloat(1.5),
                OrderedFloat(2.5),
            ]))
        );
        assert_eq!(
            repeat(BuiltinFnArgs::from(smallvec![
                Value::BoolList(Arc::new(vec![true, false])),
                Value::Int(2),
            ]))
            .expect("repeat succeeds"),
            Value::BoolList(Arc::new(vec![true, false, true, false]))
        );

        let range = Value::IntRange(Arc::new(IntRangeData::new(2, 2, 3)));
        assert_eq!(
            repeat(BuiltinFnArgs::from(smallvec![range, Value::Int(2)])).expect("repeat succeeds"),
            Value::IntList(Arc::new(vec![2, 4, 6, 2, 4, 6]))
        );

        let error = repeat(BuiltinFnArgs::from(smallvec![
            Value::Char('x'),
            Value::Int(i64::MAX),
        ]))
        .expect_err("oversized repeat should fail");
        assert_eq!(error.msg.as_deref(), Some("repeated value is too large"));
    }

    #[test]
    fn repeat_reports_a_composable_count_requirement() {
        let error = repeat(BuiltinFnArgs::from(smallvec![
            Value::Char('x'),
            Value::Int(-1),
        ]))
        .expect_err("negative repeat count should fail");

        assert_eq!(error.msg.as_deref(), Some("expected non-negative int"));
        assert_eq!(error.notes.as_ref(), &["at argument 2", "got -1 (int)"]);
    }

    #[test]
    fn shape_parsing_is_independent_of_int_list_storage() {
        let general = Value::List(Arc::new(vec![Value::Int(0), Value::Int(2)]));
        let packed = Value::IntList(Arc::new(vec![0, 2]));
        assert_eq!(
            parse_shape(&general, BE::Alloc, 0).expect("general shape"),
            vec![0, 2]
        );
        assert_eq!(
            parse_shape(&packed, BE::Alloc, 0).expect("packed shape"),
            vec![0, 2]
        );

        let range = Value::IntRange(Arc::new(IntRangeData::new(2, 1, 2)));
        assert_eq!(
            parse_shape(&range, BE::Alloc, 0).expect("range shape"),
            vec![2, 3]
        );
    }

    #[test]
    fn shape_errors_identify_the_failing_dimension() {
        let shape = Value::IntList(Arc::new(vec![2, -1]));
        let error =
            parse_shape(&shape, BE::Alloc, 0).expect_err("negative shape dimension should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected non-negative int or list of non-negative ints")
        );
        assert_eq!(
            error.notes.as_ref(),
            &["at argument 1", "at index 1", "got -1 (int)"]
        );
    }

    #[test]
    fn where_treats_virtual_ranges_as_int_lists() {
        let range = Value::IntRange(Arc::new(IntRangeData::new(0, 1, 3)));
        assert_eq!(
            wq_where(BuiltinFnArgs::from(range)).expect("where succeeds"),
            Value::IntList(Arc::new(vec![1, 2]))
        );

        let nested = Value::List(Arc::new(vec![Value::IntRange(Arc::new(
            IntRangeData::new(0, 1, 2),
        ))]));
        assert_eq!(
            wq_where(BuiltinFnArgs::from(nested)).expect("nested where succeeds"),
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![0, 1]))]))
        );
    }
}
