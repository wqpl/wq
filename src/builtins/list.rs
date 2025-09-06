use std::cmp::Ordering;

use super::arity_error;
use crate::{
    builtins::INTS_CACHE,
    value::{Value, WqResult},
    wqerror::WqError,
};
// use std::cmp::Ordering;

pub fn cat(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("cat", "2", args.len()));
    }
    let left = &args[0];
    let right = &args[1];

    match (left, right) {
        (Value::IntList(a), Value::IntList(b)) => {
            let mut res = a.clone();
            res.extend(b.clone());
            Ok(Value::IntList(res))
        }
        (Value::IntList(a), Value::Int(bv)) => {
            let mut res = a.clone();
            res.push(*bv);
            Ok(Value::IntList(res))
        }
        (Value::Int(av), Value::IntList(b)) => {
            let mut res = Vec::with_capacity(b.len() + 1);
            res.push(*av);
            res.extend(b.clone());
            Ok(Value::IntList(res))
        }
        (Value::IntList(a), Value::List(b)) => {
            let mut res: Vec<Value> = a.iter().copied().map(Value::Int).collect();
            res.extend(b.clone());
            Ok(Value::List(res))
        }
        (Value::List(a), Value::IntList(b)) => {
            let mut res = a.clone();
            res.extend(b.iter().copied().map(Value::Int));
            Ok(Value::List(res))
        }
        (Value::List(a), Value::List(b)) => {
            let mut res = a.clone();
            res.extend(b.clone());
            Ok(Value::List(res))
        }
        (Value::List(a), b) => {
            let mut res = a.clone();
            res.push(b.clone());
            Ok(Value::List(res))
        }
        (Value::IntList(a), b) => {
            let mut res: Vec<Value> = a.iter().cloned().map(Value::Int).collect();
            res.push(b.clone());
            Ok(Value::List(res))
        }
        (a, Value::List(b)) => {
            let mut res = vec![a.clone()];
            res.extend(b.clone());
            Ok(Value::List(res))
        }
        (a, Value::IntList(b)) => {
            let mut res = Vec::with_capacity(b.len() + 1);
            res.push(a.clone());
            res.extend(b.iter().copied().map(Value::Int));
            Ok(Value::List(res))
        }
        (Value::Int(a), Value::Int(b)) => Ok(Value::IntList(vec![*a, *b])),
        (a, b) => Ok(Value::List(vec![a.clone(), b.clone()])),
    }
}

pub fn count(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("count", "1", args.len()));
    }
    Ok(Value::Int(args[0].len() as i64))
}

fn uniform_shape_vec(v: &Value) -> Option<Vec<i64>> {
    match v {
        Value::IntList(items) => Some(vec![items.len() as i64]),
        Value::List(items) => {
            if items.is_empty() {
                Some(vec![0])
            } else {
                let first = uniform_shape_vec(&items[0])?;
                for it in &items[1..] {
                    let s = uniform_shape_vec(it)?;
                    if s != first {
                        return None;
                    }
                }
                let mut dims = Vec::with_capacity(first.len() + 1);
                dims.push(items.len() as i64);
                dims.extend(first);
                Some(dims)
            }
        }
        v if v.is_atom() => Some(vec![]),
        _ => {
            eprintln!("unexpected value at shape_vec {v:?}");
            Some(vec![])
        }
    }
}

pub fn shape(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("shape", "1", args.len()));
    }
    match uniform_shape_vec(&args[0]) {
        Some(dims) => {
            if dims.is_empty() {
                Ok(Value::IntList(vec![]))
            } else if dims.len() == 1 {
                Ok(Value::Int(dims[0]))
            } else {
                Ok(Value::IntList(dims))
            }
        }
        None => Ok(Value::Int(args[0].len() as i64)),
    }
}

pub fn depth(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("depth", "1", args.len()));
    }

    fn depth_of(v: &Value) -> i64 {
        match v {
            // Specialized flat list of ints counts as one level
            Value::IntList(_) => 1,

            // Heterogeneous or nested list
            Value::List(items) => {
                if items.is_empty() {
                    1
                } else {
                    let mut max_child = 0i64;
                    for it in items {
                        let d = depth_of(it);
                        if d > max_child {
                            max_child = d;
                        }
                    }
                    1 + max_child
                }
            }

            // Dict: one level for the container + maximum of values
            Value::Dict(map) => {
                if map.is_empty() {
                    1
                } else {
                    let mut max_child = 0i64;
                    for (_, v) in map.iter() {
                        let d = depth_of(v);
                        if d > max_child {
                            max_child = d;
                        }
                    }
                    1 + max_child
                }
            }
            // Atomic values
            v if v.is_atom() => 0,

            _ => {
                eprintln!("Unexpected value type in depth_of: {v:?}");
                0
            }
        }
    }

    Ok(Value::Int(depth_of(&args[0])))
}

pub fn is_uniform(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("uniform?", "1", args.len()));
    }
    let v = &args[0];
    let res = match v {
        Value::List(_) | Value::IntList(_) => uniform_shape_vec(v).is_some(),
        Value::Dict(_) => false,
        v if v.is_atom() => true,
        _ => {
            eprintln!("unexpected value at is_uniform");
            true
        }
    };
    Ok(Value::Bool(res))
}

// pub fn alloc(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("alloc", "1", args.len()));
//     }

//     fn alloc_dims(dims: &[usize]) -> Value {
//         if dims.len() == 1 {
//             Value::IntList(vec![0; dims[0]])
//         } else {
//             let mut out = Vec::with_capacity(dims[0]);
//             for _ in 0..dims[0] {
//                 out.push(alloc_dims(&dims[1..]));
//             }
//             Value::List(out)
//         }
//     }

//     fn alloc_shape(shape: &Value) -> WqResult<Value> {
//         match shape {
//             Value::Int(n) => {
//                 if *n < 0 {
//                     Err(WqError::DomainError("`alloc`: negative length".to_string()))
//                 } else {
//                     Ok(Value::IntList(vec![0; *n as usize]))
//                 }
//             }
//             Value::IntList(dims) => {
//                 if dims.iter().any(|&d| d < 0) {
//                     return Err(WqError::DomainError("`alloc`: negative length".to_string()));
//                 }
//                 let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
//                 Ok(alloc_dims(&dims))
//             }
//             Value::List(items) => {
//                 if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
//                     let dims: Vec<usize> = items
//                         .iter()
//                         .map(|v| {
//                             if let Value::Int(n) = v {
//                                 *n as usize
//                             } else {
//                                 unreachable!()
//                             }
//                         })
//                         .collect();
//                     Ok(alloc_dims(&dims))
//                 } else {
//                     let mut out = Vec::with_capacity(items.len());
//                     for v in items {
//                         out.push(alloc_shape(v)?);
//                     }
//                     Ok(Value::List(out))
//                 }
//             }
//             _ => Err(WqError::DomainError(format!(
//                 "`alloc`: invalid shape, expected int or list, got {}",
//                 shape.type_name_verbose()
//             ))),
//         }
//     }
//     alloc_shape(&args[0])
// }

// pub fn iota(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("iota", "1", args.len()));
//     }

//     fn iota_dims(dims: &[usize], next: &mut i64) -> Value {
//         if dims.is_empty() {
//             Value::IntList(Vec::new())
//         } else if dims.len() == 1 {
//             let mut out = Vec::with_capacity(dims[0]);
//             for _ in 0..dims[0] {
//                 out.push(*next);
//                 *next += 1;
//             }
//             Value::IntList(out)
//         } else {
//             let mut out = Vec::with_capacity(dims[0]);
//             for _ in 0..dims[0] {
//                 out.push(iota_dims(&dims[1..], next));
//             }
//             Value::List(out)
//         }
//     }

//     fn iota_shape(shape: &Value) -> WqResult<Value> {
//         match shape {
//             Value::Int(n) => {
//                 if *n < 0 {
//                     Err(WqError::DomainError("`iota`: negative shape".into()))
//                 } else {
//                     let mut cache = IOTA_CACHE.lock().unwrap();
//                     if let Some(v) = cache.get(n) {
//                         return Ok(v.clone());
//                     }
//                     let items: Vec<i64> = (0..*n).collect();
//                     let val = Value::IntList(items);
//                     cache.insert(*n, val.clone());
//                     Ok(val)
//                 }
//             }
//             Value::IntList(dims) => {
//                 if dims.iter().any(|&d| d < 0) {
//                     return Err(WqError::DomainError("`iota`: negative shape".to_string()));
//                 }
//                 if dims.is_empty() {
//                     return Ok(Value::Int(0));
//                 }
//                 let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
//                 let mut next = 0i64;
//                 Ok(iota_dims(&dims, &mut next))
//             }
//             Value::List(items) => {
//                 if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
//                     if items.is_empty() {
//                         Ok(Value::Int(0))
//                     } else {
//                         let dims: Vec<usize> = items
//                             .iter()
//                             .map(|v| {
//                                 if let Value::Int(n) = v {
//                                     *n as usize
//                                 } else {
//                                     unreachable!()
//                                 }
//                             })
//                             .collect();
//                         let mut next = 0i64;
//                         Ok(iota_dims(&dims, &mut next))
//                     }
//                 } else {
//                     let mut out = Vec::with_capacity(items.len());
//                     for v in items {
//                         out.push(iota_shape(v)?);
//                     }
//                     Ok(Value::List(out))
//                 }
//             }
//             _ => Err(WqError::DomainError(format!(
//                 "`iota`: invalid shape, expected int or list, got {}",
//                 shape.type_name_verbose()
//             ))),
//         }
//     }
//     iota_shape(&args[0])
// }

fn parse_shape_dims(shape: &Value, fname: &str) -> WqResult<Vec<usize>> {
    match shape {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError(format!("`{fname}`: negative length")))
            } else {
                Ok(vec![*n as usize])
            }
        }
        Value::IntList(dims) => {
            if dims.iter().any(|&d| d < 0) {
                Err(WqError::DomainError(format!("`{fname}`: negative length")))
            } else {
                Ok(dims.iter().map(|&d| d as usize).collect())
            }
        }
        Value::List(items) => {
            if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
                Ok(items
                    .iter()
                    .map(|v| {
                        if let Value::Int(n) = v {
                            *n as usize
                        } else {
                            unreachable!()
                        }
                    })
                    .collect())
            } else {
                Err(WqError::DomainError(format!(
                    "`{fname}`: shape must be a rank-1 list of non-negative ints (or an int)"
                )))
            }
        }
        _ => Err(WqError::DomainError(format!(
            "`{fname}`: invalid shape, expected int or rank-1 list of ints, got {}",
            shape.type_name()
        ))),
    }
}

// fn product(dims: &[usize]) -> usize {
//     dims.iter()
//         .copied()
//         .fold(1usize, |a, b| a.saturating_mul(b))
// }

fn build_array_from_dims<F>(dims: &[usize], next: &mut F) -> Value
where
    F: FnMut() -> Value,
{
    match dims.len() {
        0 => next(),
        1 => {
            let n = dims[0];
            if n == 0 {
                return Value::IntList(vec![]);
            }
            let mut tmp: Vec<Value> = Vec::with_capacity(n);
            let mut all_int = true;
            for _ in 0..n {
                let v = next();
                if !matches!(v, Value::Int(_)) {
                    all_int = false;
                }
                tmp.push(v);
            }
            if all_int {
                Value::IntList(
                    tmp.into_iter()
                        .map(|v| {
                            if let Value::Int(i) = v {
                                i
                            } else {
                                unreachable!()
                            }
                        })
                        .collect(),
                )
            } else {
                Value::List(tmp)
            }
        }
        _ => {
            let m = dims[0];
            let mut out = Vec::with_capacity(m);
            for _ in 0..m {
                out.push(build_array_from_dims(&dims[1..], next));
            }
            Value::List(out)
        }
    }
}

fn ravel_atoms(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::Int(_) => out.push(v.clone()),
        Value::IntList(xs) => {
            for &i in xs {
                out.push(Value::Int(i));
            }
        }
        Value::List(items) => {
            for it in items {
                ravel_atoms(it, out);
            }
        }
        vv if vv.is_atom() => out.push(vv.clone()),
        _ => out.push(v.clone()),
    }
}

pub fn alloc(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("alloc", "1", args.len()));
    }
    let dims = parse_shape_dims(&args[0], "alloc")?;
    let mut generator = || Value::Int(0);
    Ok(build_array_from_dims(&dims, &mut generator))
}

pub fn ints(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("ints", "1", args.len()));
    }

    if let Value::Int(n) = args[0] {
        if n < 0 {
            return Err(WqError::DomainError("`ints`: negative shape".into()));
        }
        let mut cache = INTS_CACHE.lock().unwrap();
        if let Some(v) = cache.get(&n) {
            return Ok(v.clone());
        }
        let val = Value::IntList((0..n).collect());
        cache.insert(n, val.clone());
        return Ok(val);
    }

    let dims = parse_shape_dims(&args[0], "ints")?;
    if dims.is_empty() {
        return Ok(Value::Int(0));
    }

    let mut next = 0i64;
    let mut generator = || {
        let v = Value::Int(next);
        next += 1;
        v
    };
    Ok(build_array_from_dims(&dims, &mut generator))
}

pub fn iota(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("iota", "1", args.len()));
    }

    match &args[0] {
        // 1D case: simple range 0..n-1
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError("`iota`: negative length".into()))
            } else {
                Ok(Value::IntList((0..*n).collect()))
            }
        }
        // Multidimensional: nested grid of coordinate vectors, shaped by dims
        _ => {
            fn build_coords(dims: &[usize], prefix: &mut Vec<i64>) -> Value {
                if dims.is_empty() {
                    return Value::IntList(prefix.clone());
                }
                let n = dims[0];
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    prefix.push(i as i64);
                    out.push(build_coords(&dims[1..], prefix));
                    prefix.pop();
                }
                Value::List(out)
            }

            let dims = parse_shape_dims(&args[0], "iota")?;
            if dims.is_empty() {
                // Preserve existing behavior for empty shape
                return Ok(Value::IntList(vec![]));
            }
            let mut prefix = Vec::<i64>::with_capacity(dims.len());
            Ok(build_coords(&dims, &mut prefix))
        }
    }
}

pub fn reshape(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("reshape", "2", args.len()));
    }
    let dims = parse_shape_dims(&args[0], "reshape")?;

    let mut pool = Vec::<Value>::new();
    ravel_atoms(&args[1], &mut pool);

    if pool.is_empty() {
        let mut generator = || Value::Int(0);
        return Ok(build_array_from_dims(&dims, &mut generator));
    }

    let n = pool.len();
    let mut i = 0usize;
    let mut generator = || {
        let v = pool[i].clone();
        i += 1;
        if i == n {
            i = 0;
        }
        v
    };
    Ok(build_array_from_dims(&dims, &mut generator))
}

pub fn range(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 && args.len() != 3 {
        return Err(arity_error("rg", "2 or 3", args.len()));
    }

    // extract start
    let start = match &args[0] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::DomainError(format!(
                "`rg`: invalid start, expected int, got {}",
                args[0].type_name()
            )));
        }
    };
    // extract end
    let end = match &args[1] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::DomainError(format!(
                "`rg`: invalid end, expected int, got {}",
                args[1].type_name()
            )));
        }
    };
    // extract optional step (default = 1)
    let step = if args.len() == 3 {
        match &args[2] {
            Value::Int(n) => *n,
            _ => {
                return Err(WqError::DomainError(format!(
                    "`rg`: invalid step, expected int, got {}",
                    args[2].type_name()
                )));
            }
        }
    } else {
        1
    };
    if step == 0 {
        return Err(WqError::DomainError("`rg`: step must not be 0".into()));
    }
    // build the sequence
    let mut items = Vec::new();
    if step > 0 {
        let mut cur = start;
        while cur < end {
            items.push(cur);
            cur += step;
        }
    } else {
        let mut cur = start;
        while cur > end {
            items.push(cur);
            cur += step; // step is negative here
        }
    }

    Ok(Value::IntList(items))
}

// pub fn fst(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("fst", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             if items.is_empty() {
//                 Ok(Value::Null)
//             } else {
//                 Ok(items[0].clone())
//             }
//         }
//         Value::IntList(items) => {
//             if items.is_empty() {
//                 Ok(Value::Null)
//             } else {
//                 Ok(Value::Int(items[0]))
//             }
//         }
//         _ => Err(WqError::TypeError(format!(
//             "`fst`: expected list at arg0, got {}",
//             args[0].type_name_verbose()
//         ))),
//     }
// }

// pub fn lst(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("lst", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             if items.is_empty() {
//                 Ok(Value::Null)
//             } else {
//                 Ok(items[items.len() - 1].clone())
//             }
//         }
//         Value::IntList(items) => {
//             if items.is_empty() {
//                 Ok(Value::Null)
//             } else {
//                 Ok(Value::Int(items[items.len() - 1]))
//             }
//         }
//         _ => Err(WqError::TypeError(format!(
//             "`lst`: expected list at arg0, got {}",
//             args[0].type_name_verbose()
//         ))),
//     }
// }

pub fn reverse(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("reverse", "1", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            let mut reversed = items.clone();
            reversed.reverse();
            Ok(Value::List(reversed))
        }
        Value::IntList(items) => {
            let mut reversed = items.clone();
            reversed.reverse();
            Ok(Value::IntList(reversed))
        }
        _ => Err(WqError::DomainError(format!(
            "`reverse`: expected list at arg0, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn wq_where(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("where", "1", args.len()));
    }

    // Helper used by the nested collector: emit Int for length-1 paths, IntList otherwise.
    fn push_coord(out: &mut Vec<Value>, coord: Vec<i64>) {
        if coord.len() == 1 {
            out.push(Value::Int(coord[0]));
        } else {
            out.push(Value::IntList(coord));
        }
    }

    // Helper: flat 1D case (list of ints/bools or IntList).
    fn where_1d_list(items: &[Value]) -> WqResult<Value> {
        let mut indices = Vec::new();
        for (i, item) in items.iter().enumerate() {
            match item {
                Value::Int(n) if *n != 0 => indices.push(Value::Int(i as i64)),
                Value::Bool(b) if *b => indices.push(Value::Int(i as i64)),
                Value::Int(_) | Value::Bool(_) => {}
                _ => {
                    return Err(WqError::DomainError(format!(
                        "`where`: expected T, where T is list of (int, bool or T), got {} in list",
                        item.type_name()
                    )));
                }
            }
        }
        Ok(Value::List(indices))
    }

    // Recursively collect coordinates. Single-index paths become Int atoms.
    fn collect_coords(v: &Value, prefix: &mut Vec<i64>, out: &mut Vec<Value>) -> WqResult<()> {
        match v {
            Value::List(items) => {
                for (i, item) in items.iter().enumerate() {
                    prefix.push(i as i64);
                    collect_coords(item, prefix, out)?;
                    prefix.pop();
                }
                Ok(())
            }
            Value::IntList(items) => {
                for (i, &n) in items.iter().enumerate() {
                    if n != 0 {
                        let mut coord = prefix.clone();
                        coord.push(i as i64);
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
            _ => Err(WqError::DomainError(format!(
                "`where`: expected T, where T is list of (int, bool or T), got {} in list",
                v.type_name()
            ))),
        }
    }

    match &args[0] {
        // Flat vector of ints -> indices as list of ints
        Value::IntList(items) => {
            let mut indices = Vec::new();
            for (i, n) in items.iter().enumerate() {
                if *n != 0 {
                    indices.push(Value::Int(i as i64));
                }
            }
            Ok(Value::List(indices))
        }
        // Generic list: nested -> coordinate vectors (with atom for length-1), else flat ints/bools.
        Value::List(items) => {
            let has_nested = items
                .iter()
                .any(|x| matches!(x, Value::List(_) | Value::IntList(_)));
            if has_nested {
                let mut out = Vec::new();
                let mut pref = Vec::new();
                collect_coords(&args[0], &mut pref, &mut out)?;
                Ok(Value::List(out))
            } else {
                where_1d_list(items)
            }
        }
        _ => Err(WqError::DomainError(format!(
            "`where`: expected T, where T is list of (int, bool or T), got {}",
            args[0].type_name()
        ))),
    }
}

pub fn flatten(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("flatten", "1", args.len()));
    }

    fn flatten_value(val: &Value, out: &mut Vec<Value>) {
        match val {
            Value::List(items) => {
                for v in items {
                    flatten_value(v, out);
                }
            }
            Value::IntList(items) => {
                for &v in items {
                    out.push(Value::Int(v));
                }
            }
            other => out.push(other.clone()),
        }
    }

    let mut result = Vec::new();
    flatten_value(&args[0], &mut result);
    Ok(Value::List(result))
}

pub fn sort(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("sort", "1", args.len()));
    }
    let v = &args[0];
    let res = match v {
        Value::IntList(items) => {
            let mut sorted = items.clone();
            sorted.sort();
            Value::IntList(sorted)
        }
        Value::List(items) => {
            let mut sorted = items.clone();
            sorted.sort_by(|a, b| {
                if let (Some(sa), Some(sb)) = (a.try_str(), b.try_str()) {
                    return sa.cmp(&sb);
                }
                match (a, b) {
                    (Value::Int(x), Value::Int(y)) => x.cmp(y),
                    (Value::Float(x), Value::Float(y)) => {
                        x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
                    }
                    (Value::Int(x), Value::Float(y)) => (*x as f64)
                        .partial_cmp(y)
                        .unwrap_or(std::cmp::Ordering::Equal),
                    (Value::Float(x), Value::Int(y)) => x
                        .partial_cmp(&(*y as f64))
                        .unwrap_or(std::cmp::Ordering::Equal),
                    (Value::Char(x), Value::Char(y)) => x.cmp(y),
                    (Value::Char(x), Value::Int(y)) => (*x as i64).cmp(y),
                    (Value::Int(x), Value::Char(y)) => x.cmp(&(*y as i64)),
                    (Value::Char(x), Value::Float(y)) => ((*x as u8) as f64)
                        .partial_cmp(y)
                        .unwrap_or(Ordering::Equal),
                    (Value::Float(x), Value::Char(y)) => x
                        .partial_cmp(&((*y as u8) as f64))
                        .unwrap_or(Ordering::Equal),
                    _ => std::cmp::Ordering::Equal,
                }
            });

            Value::List(sorted)
        }
        other => other.clone(),
    };
    Ok(res)
}

pub fn is_intlist(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("intlist?", "1", args.len()));
    }
    let v = &args[0];
    let res = match v {
        Value::IntList(_) => true,
        Value::List(items) => items.iter().all(|x| matches!(x, Value::Int(_))),
        _ => false,
    };
    Ok(Value::Bool(res))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ints_empty_shape() {
        assert_eq!(ints(&[Value::List(vec![])]).unwrap(), Value::Int(0));
    }

    #[test]
    fn shape_scalar() {
        assert_eq!(shape(&[Value::Int(42)]).unwrap(), Value::IntList(vec![]));
    }

    #[test]
    fn iota_zero() {
        assert_eq!(iota(&[Value::Int(0)]).unwrap(), Value::IntList(vec![]));
    }

    #[test]
    fn shape_and_alloc() {
        // simple vector
        let vec = alloc(&[Value::Int(3)]).unwrap();
        assert_eq!(vec, Value::IntList(vec![0, 0, 0]));
        assert_eq!(shape(&[vec]).unwrap(), Value::Int(3));

        // matrix
        let mat_shape = Value::List(vec![Value::Int(2), Value::Int(3)]);
        let mat = alloc(std::slice::from_ref(&mat_shape)).unwrap();
        assert_eq!(shape(&[mat]).unwrap(), Value::IntList(vec![2, 3]));

        // invalid shape
        let invalid_shape = Value::List(vec![Value::List(vec![Value::Int(2), Value::Int(2)])]);
        let invalid = alloc(std::slice::from_ref(&invalid_shape));
        assert!(matches!(invalid, Err(WqError::DomainError(_))));
    }

    #[test]
    fn shape_atoms_and_empty() {
        assert_eq!(shape(&[Value::Int(5)]).unwrap(), Value::IntList(vec![]));
        assert_eq!(shape(&[Value::Char('a')]).unwrap(), Value::IntList(vec![]));
        assert_eq!(shape(&[Value::List(vec![])]).unwrap(), Value::Int(0));
    }

    #[test]
    fn shape_string_and_mixed_list() {
        let s = Value::List(vec![Value::Char('h'), Value::Char('i')]);
        assert_eq!(shape(std::slice::from_ref(&s)).unwrap(), Value::Int(2));
        let mixed = Value::List(vec![Value::Char('h'), Value::Int(2)]);
        assert_eq!(shape(&[mixed]).unwrap(), Value::Int(2));
    }

    #[test]
    fn where_on_nested_bool_matrix() {
        // ((true;false;false); (false;true;false); (false;false;true))
        let mat = Value::List(vec![
            Value::List(vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(false),
            ]),
            Value::List(vec![
                Value::Bool(false),
                Value::Bool(true),
                Value::Bool(false),
            ]),
            Value::List(vec![
                Value::Bool(false),
                Value::Bool(false),
                Value::Bool(true),
            ]),
        ]);
        let res = wq_where(&[mat]).unwrap();
        assert_eq!(
            res,
            Value::List(vec![
                Value::IntList(vec![0, 0]),
                Value::IntList(vec![1, 1]),
                Value::IntList(vec![2, 2]),
            ])
        );
    }
}
