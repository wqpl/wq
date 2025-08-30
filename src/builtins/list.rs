use super::arity_error;
use crate::{
    builtins::IOTA_CACHE,
    value::{Value, WqError, WqResult},
};
// use std::cmp::Ordering;

pub fn iota(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("iota", "1", args.len()));
    }

    fn iota_dims(dims: &[usize], next: &mut i64) -> Value {
        if dims.is_empty() {
            Value::IntList(Vec::new())
        } else if dims.len() == 1 {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(*next);
                *next += 1;
            }
            Value::IntList(out)
        } else {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(iota_dims(&dims[1..], next));
            }
            Value::List(out)
        }
    }

    fn iota_shape(shape: &Value) -> WqResult<Value> {
        match shape {
            Value::Int(n) => {
                if *n < 0 {
                    Err(WqError::DomainError("`iota`: negative shape".into()))
                } else {
                    let mut cache = IOTA_CACHE.lock().unwrap();
                    if let Some(v) = cache.get(n) {
                        return Ok(v.clone());
                    }
                    let items: Vec<i64> = (0..*n).collect();
                    let val = Value::IntList(items);
                    cache.insert(*n, val.clone());
                    Ok(val)
                }
            }
            Value::IntList(dims) => {
                if dims.iter().any(|&d| d < 0) {
                    return Err(WqError::DomainError("`iota`: negative shape".to_string()));
                }
                if dims.is_empty() {
                    return Ok(Value::Int(0));
                }
                let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
                let mut next = 0i64;
                Ok(iota_dims(&dims, &mut next))
            }
            Value::List(items) => {
                if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
                    if items.is_empty() {
                        Ok(Value::Int(0))
                    } else {
                        let dims: Vec<usize> = items
                            .iter()
                            .map(|v| {
                                if let Value::Int(n) = v {
                                    *n as usize
                                } else {
                                    unreachable!()
                                }
                            })
                            .collect();
                        let mut next = 0i64;
                        Ok(iota_dims(&dims, &mut next))
                    }
                } else {
                    let mut out = Vec::with_capacity(items.len());
                    for v in items {
                        out.push(iota_shape(v)?);
                    }
                    Ok(Value::List(out))
                }
            }
            _ => Err(WqError::TypeError(format!(
                "`iota`: invalid shape, expected int or list, got {}",
                shape.type_name_verbose()
            ))),
        }
    }

    iota_shape(&args[0])
}

pub fn range(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 && args.len() != 3 {
        return Err(arity_error("rg", "2 or 3", args.len()));
    }

    // extract start
    let start = match &args[0] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::TypeError(format!(
                "`rg`: invalid start, expected int, got {}",
                args[0].type_name_verbose()
            )));
        }
    };
    // extract end
    let end = match &args[1] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::TypeError(format!(
                "`rg`: invalid end, expected int, got {}",
                args[1].type_name_verbose()
            )));
        }
    };
    // extract optional step (default = 1)
    let step = if args.len() == 3 {
        match &args[2] {
            Value::Int(n) => *n,
            _ => {
                return Err(WqError::TypeError(format!(
                    "`rg`: invalid step, expected int, got {}",
                    args[2].type_name_verbose()
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

pub fn count(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("count", "1", args.len()));
    }
    Ok(Value::Int(args[0].len() as i64))
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
        _ => Err(WqError::TypeError(format!(
            "`reverse`: expected list at arg0, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

// pub fn sum(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("sum", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Int(0));
//             }
//             let mut result = items[0].clone();
//             for item in items.iter().skip(1) {
//                 result = result.add(item).ok_or_else(|| {
//                     WqError::TypeError("`sum`: cannot compute sum of provided list".to_string())
//                 })?;
//             }
//             Ok(result)
//         }
//         Value::IntList(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Int(0));
//             }
//             let sum: i64 = items.iter().sum();
//             Ok(Value::Int(sum))
//         }
//         _ => Ok(args[0].clone()),
//     }
// }

// pub fn max(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("max", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Null);
//             }
//             let mut result = &items[0];
//             for item in items.iter().skip(1) {
//                 match (result, item) {
//                     (Value::Int(a), Value::Int(b)) => {
//                         if b > a {
//                             result = item;
//                         }
//                     }
//                     (Value::Float(a), Value::Float(b)) => {
//                         if b > a {
//                             result = item;
//                         }
//                     }
//                     (Value::Int(a), Value::Float(b)) => {
//                         if *b > *a as f64 {
//                             result = item;
//                         }
//                     }
//                     (Value::Float(a), Value::Int(b)) => {
//                         if *b as f64 > *a {
//                             result = item;
//                         }
//                     }
//                     _ => {
//                         return Err(WqError::TypeError(
//                             "`max`: cannot compare provided types".to_string(),
//                         ));
//                     }
//                 }
//             }
//             Ok(result.clone())
//         }
//         Value::IntList(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Null);
//             }
//             let max = items.iter().max().cloned().unwrap();
//             Ok(Value::Int(max))
//         }
//         _ => Ok(args[0].clone()),
//     }
// }

// pub fn min(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("min", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Null);
//             }
//             let mut result = &items[0];
//             for item in items.iter().skip(1) {
//                 match (result, item) {
//                     (Value::Int(a), Value::Int(b)) => {
//                         if b < a {
//                             result = item;
//                         }
//                     }
//                     (Value::Float(a), Value::Float(b)) => {
//                         if b < a {
//                             result = item;
//                         }
//                     }
//                     (Value::Int(a), Value::Float(b)) => {
//                         if *b < *a as f64 {
//                             result = item;
//                         }
//                     }
//                     (Value::Float(a), Value::Int(b)) => {
//                         if (*b as f64) < *a {
//                             result = item;
//                         }
//                     }
//                     _ => {
//                         return Err(WqError::TypeError(
//                             "`min`: cannot compare provided types".to_string(),
//                         ));
//                     }
//                 }
//             }
//             Ok(result.clone())
//         }
//         Value::IntList(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Null);
//             }
//             let min = items.iter().min().cloned().unwrap();
//             Ok(Value::Int(min))
//         }
//         _ => Ok(args[0].clone()),
//     }
// }

// pub fn avg(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("avg", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Null);
//             }
//             let sum_result = sum(args)?;
//             let count = items.len() as f64;
//             match sum_result {
//                 Value::Int(n) => Ok(Value::Float(n as f64 / count)),
//                 Value::Float(f) => Ok(Value::Float(f / count)),
//                 _ => Err(WqError::TypeError(
//                     "`avg`: cannot compute average of provided list".to_string(),
//                 )),
//             }
//         }
//         Value::IntList(items) => {
//             if items.is_empty() {
//                 return Ok(Value::Null);
//             }
//             let sum: i64 = items.iter().sum();
//             let count = items.len() as f64;
//             Ok(Value::Float(sum as f64 / count))
//         }
//         _ => Ok(args[0].clone()),
//     }
// }

// pub fn take(args: &[Value]) -> WqResult<Value> {
//     if args.len() == 1 {
//         let tmp = [Value::Int(1), args[0].clone()];
//         return take(&tmp);
//     }
//     if args.len() != 2 {
//         return Err(arity_error("take", "1 or 2", args.len()));
//     }
//     match (&args[0], &args[1]) {
//         (Value::Int(n), Value::List(items)) => {
//             let n = *n as usize;
//             if n >= items.len() {
//                 Ok(Value::List(items.clone()))
//             } else {
//                 let mut result = Vec::with_capacity(n);
//                 for item in items.iter().take(n) {
//                     result.push(item.clone());
//                 }
//                 Ok(Value::List(result))
//             }
//         }
//         (Value::Int(n), Value::IntList(items)) => {
//             let n = *n as usize;
//             if n >= items.len() {
//                 Ok(Value::IntList(items.clone()))
//             } else {
//                 let mut result = Vec::with_capacity(n);
//                 for &item in items.iter().take(n) {
//                     result.push(item);
//                 }
//                 Ok(Value::IntList(result))
//             }
//         }
//         _ => Err(WqError::TypeError(format!(
//             "`take`: expected int and list, got {} and {}",
//             &args[0].type_name_verbose(),
//             &args[1].type_name_verbose()
//         ))),
//     }
// }

// pub fn drop(args: &[Value]) -> WqResult<Value> {
//     if args.len() == 1 {
//         let tmp = [Value::Int(1), args[0].clone()];
//         return drop(&tmp);
//     }
//     if args.len() != 2 {
//         return Err(arity_error("drop", "1 or 2", args.len()));
//     }
//     match (&args[0], &args[1]) {
//         (Value::Int(n), Value::List(items)) => {
//             let n = *n as usize;
//             if n >= items.len() {
//                 Ok(Value::List(Vec::new()))
//             } else {
//                 let remaining = items.len() - n;
//                 let mut result = Vec::with_capacity(remaining);
//                 for item in items.iter().skip(n) {
//                     result.push(item.clone());
//                 }
//                 Ok(Value::List(result))
//             }
//         }
//         (Value::Int(n), Value::IntList(items)) => {
//             let n = *n as usize;
//             if n >= items.len() {
//                 Ok(Value::IntList(Vec::new()))
//             } else {
//                 let remaining = items.len() - n;
//                 let mut result = Vec::with_capacity(remaining);
//                 for &item in items.iter().skip(n) {
//                     result.push(item);
//                 }
//                 Ok(Value::IntList(result))
//             }
//         }
//         _ => Err(WqError::TypeError(format!(
//             "`drop`: expected int and list, got {} and {}",
//             &args[0].type_name_verbose(),
//             &args[1].type_name_verbose()
//         ))),
//     }
// }

pub fn wq_where(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("where", "1", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            let mut indices = Vec::new();
            for (i, item) in items.iter().enumerate() {
                match item {
                    Value::Int(n) => {
                        if *n != 0 {
                            indices.push(Value::Int(i as i64));
                        }
                    }
                    Value::Bool(b) => {
                        if *b {
                            indices.push(Value::Int(i as i64));
                        }
                    }
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "`where`: expected provided list to contain (only ints) or (only bools), got {} in the list",
                            item.type_name_verbose()
                        )));
                    }
                }
            }
            Ok(Value::List(indices))
        }
        Value::IntList(items) => {
            let mut indices = Vec::new();
            for (i, n) in items.iter().enumerate() {
                if *n != 0 {
                    indices.push(Value::Int(i as i64));
                }
            }
            Ok(Value::List(indices))
        }
        _ => Err(WqError::TypeError(format!(
            "`where`: expected list, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

// pub fn distinct(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("distinct", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             let mut seen = Vec::new();
//             for item in items {
//                 if !seen.contains(item) {
//                     seen.push(item.clone());
//                 }
//             }
//             Ok(Value::List(seen))
//         }
//         Value::IntList(items) => {
//             let mut seen: Vec<i64> = Vec::new();
//             for &x in items {
//                 if !seen.contains(&x) {
//                     seen.push(x);
//                 }
//             }
//             Ok(Value::IntList(seen))
//         }
//         _ => Err(WqError::TypeError(format!(
//             "`distinct`: expected list, got {}",
//             args[0].type_name_verbose()
//         ))),
//     }
// }

// pub fn sort(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("sort", "1", args.len()));
//     }
//     match &args[0] {
//         Value::List(items) => {
//             let mut sorted = items.clone();
//             sorted.sort_by(|a, b| match (a, b) {
//                 (Value::Int(x), Value::Int(y)) => x.cmp(y),
//                 (Value::Float(x), Value::Float(y)) => {
//                     x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal)
//                 }
//                 (Value::Int(x), Value::Float(y)) => (*x as f64)
//                     .partial_cmp(y)
//                     .unwrap_or(std::cmp::Ordering::Equal),
//                 (Value::Float(x), Value::Int(y)) => x
//                     .partial_cmp(&(*y as f64))
//                     .unwrap_or(std::cmp::Ordering::Equal),
//                 (Value::Char(x), Value::Char(y)) => x.cmp(y),
//                 (Value::Char(x), Value::Int(y)) => (*x as i64).cmp(y),
//                 (Value::Int(x), Value::Char(y)) => x.cmp(&(*y as i64)),
//                 (Value::Char(x), Value::Float(y)) => ((*x as u8) as f64)
//                     .partial_cmp(y)
//                     .unwrap_or(Ordering::Equal),
//                 (Value::Float(x), Value::Char(y)) => x
//                     .partial_cmp(&((*y as u8) as f64))
//                     .unwrap_or(Ordering::Equal),
//                 _ => std::cmp::Ordering::Equal,
//             });
//             Ok(Value::List(sorted))
//         }
//         Value::IntList(items) => {
//             let mut sorted = items.clone();
//             sorted.sort();
//             Ok(Value::IntList(sorted))
//         }
//         _ => Err(WqError::TypeError(format!(
//             "`sort`: expected list, got {}",
//             args[0].type_name_verbose()
//         ))),
//     }
// }

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

pub fn alloc(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("alloc", "1", args.len()));
    }

    fn alloc_dims(dims: &[usize]) -> Value {
        if dims.len() == 1 {
            Value::IntList(vec![0; dims[0]])
        } else {
            let mut out = Vec::with_capacity(dims[0]);
            for _ in 0..dims[0] {
                out.push(alloc_dims(&dims[1..]));
            }
            Value::List(out)
        }
    }

    fn alloc_shape(shape: &Value) -> WqResult<Value> {
        match shape {
            Value::Int(n) => {
                if *n < 0 {
                    Err(WqError::DomainError("`alloc`: negative length".to_string()))
                } else {
                    Ok(Value::IntList(vec![0; *n as usize]))
                }
            }
            Value::IntList(dims) => {
                if dims.iter().any(|&d| d < 0) {
                    return Err(WqError::DomainError("`alloc`: negative length".to_string()));
                }
                let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
                Ok(alloc_dims(&dims))
            }
            Value::List(items) => {
                if items.iter().all(|v| matches!(v, Value::Int(n) if *n >= 0)) {
                    let dims: Vec<usize> = items
                        .iter()
                        .map(|v| {
                            if let Value::Int(n) = v {
                                *n as usize
                            } else {
                                unreachable!()
                            }
                        })
                        .collect();
                    Ok(alloc_dims(&dims))
                } else {
                    let mut out = Vec::with_capacity(items.len());
                    for v in items {
                        out.push(alloc_shape(v)?);
                    }
                    Ok(Value::List(out))
                }
            }
            _ => Err(WqError::TypeError(format!(
                "`alloc`: invalid shape, expected int or list, got {}",
                shape.type_name_verbose()
            ))),
        }
    }

    alloc_shape(&args[0])
}

pub fn shape(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("shape", "1", args.len()));
    }

    fn shape_vec(v: &Value) -> Option<Vec<i64>> {
        match v {
            Value::IntList(items) => Some(vec![items.len() as i64]),
            Value::List(items) => {
                if items.is_empty() {
                    Some(vec![0])
                } else {
                    let first = shape_vec(&items[0])?;
                    for it in &items[1..] {
                        let s = shape_vec(it)?;
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
            _ => Some(vec![]),
        }
    }

    match shape_vec(&args[0]) {
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

// pub fn idx(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 2 {
//         return Err(arity_error("idx", "2", args.len()));
//     }
//     let indices = match &args[1] {
//         Value::IntList(idxs) => idxs,
//         _ => {
//             return Err(WqError::TypeError(
//                 "idx expects an integer list of indices".to_string(),
//             ));
//         }
//     };
//     let list_len = args[0].len() as i64;
//     for &i in indices {
//         if i < 0 || i >= list_len {
//             return Err(WqError::IndexError("idx out of bounds".into()));
//         }
//     }
//     match &args[0] {
//         Value::IntList(items) => {
//             let mut out = Vec::with_capacity(indices.len());
//             for &i in indices {
//                 out.push(items[i as usize]);
//             }
//             Ok(Value::IntList(out))
//         }
//         Value::List(items) => {
//             let mut out = Vec::with_capacity(indices.len());
//             for &i in indices {
//                 out.push(items[i as usize].clone());
//             }
//             Ok(Value::List(out))
//         }
//         _ => Err(WqError::TypeError(
//             "idx expects a list as the first arg".to_string(),
//         )),
//     }
// }

// pub fn wq_in(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 2 {
//         return Err(arity_error("in", "2", args.len()));
//     }
//     match &args[1] {
//         Value::List(items) => Ok(Value::Bool(items.contains(&args[0]))),
//         Value::IntList(items) => match &args[0] {
//             Value::Int(n) => Ok(Value::Bool(items.contains(n))),
//             _ => Err(WqError::TypeError(
//                 "in expects an integer when searching an integer list".to_string(),
//             )),
//         },
//         _ => Err(WqError::TypeError(
//             "in expects a list as the second arg".to_string(),
//         )),
//     }
// }

// pub fn find(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 2 {
//         return Err(arity_error("find", "2", args.len()));
//     }
//     let target = &args[0];
//     match &args[1] {
//         Value::List(items) => {
//             for (i, item) in items.iter().enumerate() {
//                 if item == target {
//                     return Ok(Value::Int(i as i64));
//                 }
//             }
//             Ok(Value::Null)
//         }
//         Value::IntList(items) => match target {
//             Value::Int(n) => {
//                 for (i, item) in items.iter().enumerate() {
//                     if item == n {
//                         return Ok(Value::Int(i as i64));
//                     }
//                 }
//                 Ok(Value::Null)
//             }
//             _ => Err(WqError::TypeError(
//                 "find expects an integer when searching an integer list".to_string(),
//             )),
//         },
//         _ => Err(WqError::TypeError(
//             "find expects a list as the second arg".to_string(),
//         )),
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iota_empty_shape() {
        assert_eq!(iota(&[Value::List(vec![])]).unwrap(), Value::Int(0));
    }

    #[test]
    fn shape_scalar() {
        assert_eq!(shape(&[Value::Int(42)]).unwrap(), Value::IntList(vec![]));
    }

    #[test]
    fn iota_zero() {
        assert_eq!(iota(&[Value::Int(0)]).unwrap(), Value::IntList(vec![]));
    }

    // #[test]
    // fn in_list_with_value() {
    //     let lst = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    //     assert_eq!(
    //         wq_in(&[Value::Int(2), lst.clone()]).unwrap(),
    //         Value::Bool(true)
    //     );
    //     assert_eq!(wq_in(&[Value::Int(4), lst]).unwrap(), Value::Bool(false));
    // }

    // #[test]
    // fn in_int_list() {
    //     let lst = Value::IntList(vec![1, 2, 3]);
    //     assert_eq!(
    //         wq_in(&[Value::Int(2), lst.clone()]).unwrap(),
    //         Value::Bool(true)
    //     );
    //     assert_eq!(wq_in(&[Value::Int(4), lst]).unwrap(), Value::Bool(false));
    // }

    #[test]
    fn shape_and_alloc() {
        // simple vector
        let vec = alloc(&[Value::Int(3)]).unwrap();
        assert_eq!(vec, Value::IntList(vec![0, 0, 0]));
        assert_eq!(shape(&[vec]).unwrap(), Value::Int(3));

        // matrix
        let mat_shape = Value::List(vec![Value::Int(2), Value::Int(3)]);
        let mat = alloc(&[mat_shape.clone()]).unwrap();
        assert_eq!(shape(&[mat]).unwrap(), Value::IntList(vec![2, 3]));

        // heterogeneous shape
        let nested_shape = Value::List(vec![
            Value::List(vec![Value::Int(2), Value::Int(2)]),
            Value::Int(3),
        ]);
        let nested = alloc(&[nested_shape.clone()]).unwrap();
        assert_eq!(shape(&[nested]).unwrap(), Value::Int(2));
    }

    // #[test]
    // fn find_in_list() {
    //     let lst = Value::List(vec![
    //         Value::Int(1),
    //         Value::Int(2),
    //         Value::Int(3),
    //         Value::Int(2),
    //     ]);
    //     assert_eq!(find(&[Value::Int(2), lst.clone()]).unwrap(), Value::Int(1));
    //     assert_eq!(find(&[Value::Int(4), lst]).unwrap(), Value::Null);
    // }

    // #[test]
    // fn find_in_int_list() {
    //     let lst = Value::IntList(vec![1, 2, 3, 2]);
    //     assert_eq!(find(&[Value::Int(2), lst.clone()]).unwrap(), Value::Int(1));
    //     assert_eq!(find(&[Value::Int(4), lst]).unwrap(), Value::Null);
    // }

    #[test]
    fn shape_atoms_and_empty() {
        assert_eq!(shape(&[Value::Int(5)]).unwrap(), Value::IntList(vec![]));
        assert_eq!(shape(&[Value::Char('a')]).unwrap(), Value::IntList(vec![]));
        assert_eq!(shape(&[Value::List(vec![])]).unwrap(), Value::Int(0));
    }

    #[test]
    fn shape_string_and_mixed_list() {
        let s = Value::List(vec![Value::Char('h'), Value::Char('i')]);
        assert_eq!(shape(&[s.clone()]).unwrap(), Value::Int(2));
        let mixed = Value::List(vec![Value::Char('h'), Value::Int(2)]);
        assert_eq!(shape(&[mixed]).unwrap(), Value::Int(2));
    }
}
