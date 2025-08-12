use std::cmp::Ordering;

use super::arity_error;
use crate::{
    builtins::TIL_CACHE,
    value::{Value, WqError, WqResult},
};

fn til_dims(dims: &[usize], next: &mut i64) -> Value {
    if dims.len() == 1 {
        let mut out = Vec::with_capacity(dims[0]);
        for _ in 0..dims[0] {
            out.push(*next);
            *next += 1;
        }
        Value::IntList(out)
    } else {
        let mut out = Vec::with_capacity(dims[0]);
        for _ in 0..dims[0] {
            out.push(til_dims(&dims[1..], next));
        }
        Value::List(out)
    }
}

fn til_shape(shape: &Value) -> WqResult<Value> {
    match shape {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::TypeError(
                    "til expects non-negative integer".into(),
                ))
            } else {
                let mut cache = TIL_CACHE.lock().unwrap();
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
                return Err(WqError::DomainError(
                    "til length must be non-negative".to_string(),
                ));
            }
            let dims: Vec<usize> = dims.iter().map(|&d| d as usize).collect();
            let mut next = 0i64;
            Ok(til_dims(&dims, &mut next))
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
                let mut next = 0i64;
                Ok(til_dims(&dims, &mut next))
            } else {
                let mut out = Vec::with_capacity(items.len());
                for v in items {
                    out.push(til_shape(v)?);
                }
                Ok(Value::List(out))
            }
        }
        _ => Err(WqError::TypeError("til expects an integer shape".into())),
    }
}

pub fn til(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("til", "1 argument", args.len()));
    }
    til_shape(&args[0])
}

pub fn range(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 && args.len() != 3 {
        return Err(arity_error("range", "2 or 3 arguments", args.len()));
    }

    // extract start
    let start = match &args[0] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::TypeError(format!(
                "range expects integers, got {}",
                args[0].type_name()
            )));
        }
    };
    // extract end
    let end = match &args[1] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::TypeError(format!(
                "range expects integers, got {}",
                args[1].type_name()
            )));
        }
    };
    // extract optional step (default = 1)
    let step = if args.len() == 3 {
        match &args[2] {
            Value::Int(n) => *n,
            _ => {
                return Err(WqError::TypeError(
                    "range step must be an integer".to_string(),
                ));
            }
        }
    } else {
        1
    };
    // step must not be zero
    if step == 0 {
        return Err(WqError::TypeError("range step cannot be zero".to_string()));
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
        return Err(arity_error("count", "1 argument", args.len()));
    }
    Ok(Value::Int(args[0].len() as i64))
}

pub fn first(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("first", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(items[0].clone())
            }
        }
        Value::IntList(items) => {
            if items.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Int(items[0]))
            }
        }
        _ => Err(WqError::TypeError(format!(
            "first expects a list, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn last(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("last", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(items[items.len() - 1].clone())
            }
        }
        Value::IntList(items) => {
            if items.is_empty() {
                Ok(Value::Null)
            } else {
                Ok(Value::Int(items[items.len() - 1]))
            }
        }
        _ => Err(WqError::TypeError(format!(
            "last expects a list, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn reverse(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("reverse", "1 argument", args.len()));
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
            "reverse expects a list, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn sum(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("sum", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                return Ok(Value::Int(0));
            }
            let mut result = items[0].clone();
            for item in items.iter().skip(1) {
                result = result
                    .add(item)
                    .ok_or_else(|| WqError::TypeError("Cannot sum these types".to_string()))?;
            }
            Ok(result)
        }
        Value::IntList(items) => {
            if items.is_empty() {
                return Ok(Value::Int(0));
            }
            let sum: i64 = items.iter().sum();
            Ok(Value::Int(sum))
        }
        _ => Ok(args[0].clone()),
    }
}

pub fn max(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("max", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                return Ok(Value::Null);
            }
            let mut result = &items[0];
            for item in items.iter().skip(1) {
                match (result, item) {
                    (Value::Int(a), Value::Int(b)) => {
                        if b > a {
                            result = item;
                        }
                    }
                    (Value::Float(a), Value::Float(b)) => {
                        if b > a {
                            result = item;
                        }
                    }
                    (Value::Int(a), Value::Float(b)) => {
                        if *b > *a as f64 {
                            result = item;
                        }
                    }
                    (Value::Float(a), Value::Int(b)) => {
                        if *b as f64 > *a {
                            result = item;
                        }
                    }
                    _ => return Err(WqError::TypeError("Cannot compare these types".to_string())),
                }
            }
            Ok(result.clone())
        }
        Value::IntList(items) => {
            if items.is_empty() {
                return Ok(Value::Null);
            }
            let max = items.iter().max().cloned().unwrap();
            Ok(Value::Int(max))
        }
        _ => Ok(args[0].clone()),
    }
}

pub fn min(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("min", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                return Ok(Value::Null);
            }
            let mut result = &items[0];
            for item in items.iter().skip(1) {
                match (result, item) {
                    (Value::Int(a), Value::Int(b)) => {
                        if b < a {
                            result = item;
                        }
                    }
                    (Value::Float(a), Value::Float(b)) => {
                        if b < a {
                            result = item;
                        }
                    }
                    (Value::Int(a), Value::Float(b)) => {
                        if *b < *a as f64 {
                            result = item;
                        }
                    }
                    (Value::Float(a), Value::Int(b)) => {
                        if (*b as f64) < *a {
                            result = item;
                        }
                    }
                    _ => return Err(WqError::TypeError("Cannot compare these types".to_string())),
                }
            }
            Ok(result.clone())
        }
        Value::IntList(items) => {
            if items.is_empty() {
                return Ok(Value::Null);
            }
            let min = items.iter().min().cloned().unwrap();
            Ok(Value::Int(min))
        }
        _ => Ok(args[0].clone()),
    }
}

pub fn avg(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("avg", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            if items.is_empty() {
                return Ok(Value::Null);
            }
            let sum_result = sum(args)?;
            let count = items.len() as f64;
            match sum_result {
                Value::Int(n) => Ok(Value::Float(n as f64 / count)),
                Value::Float(f) => Ok(Value::Float(f / count)),
                _ => Err(WqError::TypeError("Cannot compute average".to_string())),
            }
        }
        Value::IntList(items) => {
            if items.is_empty() {
                return Ok(Value::Null);
            }
            let sum: i64 = items.iter().sum();
            let count = items.len() as f64;
            Ok(Value::Float(sum as f64 / count))
        }
        _ => Ok(args[0].clone()),
    }
}

pub fn take(args: &[Value]) -> WqResult<Value> {
    if args.len() == 1 {
        let tmp = [Value::Int(1), args[0].clone()];
        return take(&tmp);
    }
    if args.len() != 2 {
        return Err(arity_error("take", "1 or 2 arguments", args.len()));
    }
    match (&args[0], &args[1]) {
        (Value::Int(n), Value::List(items)) => {
            let n = *n as usize;
            if n >= items.len() {
                Ok(Value::List(items.clone()))
            } else {
                let mut result = Vec::with_capacity(n);
                for item in items.iter().take(n) {
                    result.push(item.clone());
                }
                Ok(Value::List(result))
            }
        }
        (Value::Int(n), Value::IntList(items)) => {
            let n = *n as usize;
            if n >= items.len() {
                Ok(Value::IntList(items.clone()))
            } else {
                let mut result = Vec::with_capacity(n);
                for &item in items.iter().take(n) {
                    result.push(item);
                }
                Ok(Value::IntList(result))
            }
        }
        _ => Err(WqError::TypeError(format!(
            "take expects integer and list, got {} and {}",
            &args[0].type_name(),
            &args[1].type_name()
        ))),
    }
}

pub fn drop(args: &[Value]) -> WqResult<Value> {
    if args.len() == 1 {
        let tmp = [Value::Int(1), args[0].clone()];
        return drop(&tmp);
    }
    if args.len() != 2 {
        return Err(arity_error("drop", "1 or 2 arguments", args.len()));
    }
    match (&args[0], &args[1]) {
        (Value::Int(n), Value::List(items)) => {
            let n = *n as usize;
            if n >= items.len() {
                Ok(Value::List(Vec::new()))
            } else {
                let remaining = items.len() - n;
                let mut result = Vec::with_capacity(remaining);
                for item in items.iter().skip(n) {
                    result.push(item.clone());
                }
                Ok(Value::List(result))
            }
        }
        (Value::Int(n), Value::IntList(items)) => {
            let n = *n as usize;
            if n >= items.len() {
                Ok(Value::IntList(Vec::new()))
            } else {
                let remaining = items.len() - n;
                let mut result = Vec::with_capacity(remaining);
                for &item in items.iter().skip(n) {
                    result.push(item);
                }
                Ok(Value::IntList(result))
            }
        }
        _ => Err(WqError::TypeError(format!(
            "drop expects integer and list, got {} and {}",
            &args[0].type_name(),
            &args[1].type_name()
        ))),
    }
}

pub fn where_func(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("where", "1 argument", args.len()));
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
                            "where expects integer or boolean lists, got {}",
                            item.type_name()
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
            "where expects a list, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn distinct(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("distinct", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            let mut seen = Vec::new();
            for item in items {
                if !seen.contains(item) {
                    seen.push(item.clone());
                }
            }
            Ok(Value::List(seen))
        }
        Value::IntList(items) => {
            let mut seen: Vec<i64> = Vec::new();
            for &x in items {
                if !seen.contains(&x) {
                    seen.push(x);
                }
            }
            Ok(Value::IntList(seen))
        }
        _ => Err(WqError::TypeError(format!(
            "distinct expects a list, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn sort(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("sort", "1 argument", args.len()));
    }
    match &args[0] {
        Value::List(items) => {
            let mut sorted = items.clone();
            sorted.sort_by(|a, b| match (a, b) {
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
            });
            Ok(Value::List(sorted))
        }
        Value::IntList(items) => {
            let mut sorted = items.clone();
            sorted.sort();
            Ok(Value::IntList(sorted))
        }
        _ => Err(WqError::TypeError(format!(
            "sort expects a list, got {}",
            args[0].type_name()
        ))),
    }
}

pub fn cat(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("cat", "2 arguments", args.len()));
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
        (a, b) => Ok(Value::List(vec![a.clone(), b.clone()])),
    }
}

pub fn flatten(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("flatten", "1 argument", args.len()));
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
                Err(WqError::DomainError(
                    "alloc length must be non-negative".to_string(),
                ))
            } else {
                Ok(Value::IntList(vec![0; *n as usize]))
            }
        }
        Value::IntList(dims) => {
            if dims.iter().any(|&d| d < 0) {
                return Err(WqError::DomainError(
                    "alloc length must be non-negative".to_string(),
                ));
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
        _ => Err(WqError::TypeError("alloc expects an integer shape".into())),
    }
}

pub fn alloc(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("alloc", "1 argument", args.len()));
    }
    alloc_shape(&args[0])
}

fn shape_value(v: &Value) -> WqResult<Value> {
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

    match shape_vec(v) {
        Some(dims) => match dims.as_slice() {
            [] => Ok(Value::Int(0)),
            [d] => Ok(Value::Int(*d)),
            _ => Ok(Value::IntList(dims)),
        },
        None => Ok(Value::Int(v.len() as i64)),
    }
}

pub fn shape(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("shape", "1 argument", args.len()));
    }
    shape_value(&args[0])
}

pub fn idx(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("idx", "2 arguments", args.len()));
    }
    let indices = match &args[1] {
        Value::IntList(idxs) => idxs,
        _ => {
            return Err(WqError::TypeError(
                "idx expects an integer list of indices".to_string(),
            ));
        }
    };
    let list_len = args[0].len() as i64;
    for &i in indices {
        if i < 0 || i >= list_len {
            return Err(WqError::IndexError("idx out of bounds".into()));
        }
    }
    match &args[0] {
        Value::IntList(items) => {
            let mut out = Vec::with_capacity(indices.len());
            for &i in indices {
                out.push(items[i as usize]);
            }
            Ok(Value::IntList(out))
        }
        Value::List(items) => {
            let mut out = Vec::with_capacity(indices.len());
            for &i in indices {
                out.push(items[i as usize].clone());
            }
            Ok(Value::List(out))
        }
        _ => Err(WqError::TypeError(
            "idx expects a list as the first argument".to_string(),
        )),
    }
}

pub fn in_list(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("in", "2 arguments", args.len()));
    }
    match &args[1] {
        Value::List(items) => Ok(Value::Bool(items.contains(&args[0]))),
        Value::IntList(items) => match &args[0] {
            Value::Int(n) => Ok(Value::Bool(items.contains(n))),
            _ => Err(WqError::TypeError(
                "in expects an integer when searching an integer list".to_string(),
            )),
        },
        _ => Err(WqError::TypeError(
            "in expects a list as the second argument".to_string(),
        )),
    }
}

pub fn find(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("find", "2 arguments", args.len()));
    }
    let target = &args[0];
    match &args[1] {
        Value::List(items) => {
            for (i, item) in items.iter().enumerate() {
                if item == target {
                    return Ok(Value::Int(i as i64));
                }
            }
            Ok(Value::Null)
        }
        Value::IntList(items) => match target {
            Value::Int(n) => {
                for (i, item) in items.iter().enumerate() {
                    if item == n {
                        return Ok(Value::Int(i as i64));
                    }
                }
                Ok(Value::Null)
            }
            _ => Err(WqError::TypeError(
                "find expects an integer when searching an integer list".to_string(),
            )),
        },
        _ => Err(WqError::TypeError(
            "find expects a list as the second argument".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_list_with_value() {
        let lst = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(
            in_list(&[Value::Int(2), lst.clone()]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(in_list(&[Value::Int(4), lst]).unwrap(), Value::Bool(false));
    }

    #[test]
    fn in_int_list() {
        let lst = Value::IntList(vec![1, 2, 3]);
        assert_eq!(
            in_list(&[Value::Int(2), lst.clone()]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(in_list(&[Value::Int(4), lst]).unwrap(), Value::Bool(false));
    }

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

    #[test]
    fn find_in_list() {
        let lst = Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
        ]);
        assert_eq!(find(&[Value::Int(2), lst.clone()]).unwrap(), Value::Int(1));
        assert_eq!(find(&[Value::Int(4), lst]).unwrap(), Value::Null);
    }

    #[test]
    fn find_in_int_list() {
        let lst = Value::IntList(vec![1, 2, 3, 2]);
        assert_eq!(find(&[Value::Int(2), lst.clone()]).unwrap(), Value::Int(1));
        assert_eq!(find(&[Value::Int(4), lst]).unwrap(), Value::Null);
    }

    #[test]
    fn shape_atoms_and_empty() {
        assert_eq!(shape(&[Value::Int(5)]).unwrap(), Value::Int(0));
        assert_eq!(shape(&[Value::Char('a')]).unwrap(), Value::Int(0));
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
