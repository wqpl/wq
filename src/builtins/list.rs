use std::cmp::Ordering;

use crate::{
    builtins::TIL_CACHE,
    value::valuei::{Value, WqError, WqResult},
};

pub fn til(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("til expects 1 argument".to_string()));
    }
    match &args[0] {
        Value::Int(n) => {
            if *n < 0 {
                return Err(WqError::TypeError(
                    "til expects non-negative integer".to_string(),
                ));
            }
            let mut cache = TIL_CACHE.lock().unwrap();
            if let Some(v) = cache.get(n) {
                return Ok(v.clone());
            }
            let items: Vec<i64> = (0..*n).collect();
            let val = Value::IntList(items);
            cache.insert(*n, val.clone());
            Ok(val)
        }
        _ => Err(WqError::TypeError("til only works on integers".to_string())),
    }
}

pub fn range(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 && args.len() != 3 {
        return Err(WqError::ArityError(
            "range expects 2 or 3 arguments".to_string(),
        ));
    }

    // extract start
    let start = match &args[0] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::TypeError(
                "range only works on integers".to_string(),
            ));
        }
    };
    // extract end
    let end = match &args[1] {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::TypeError(
                "range only works on integers".to_string(),
            ));
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
        return Err(WqError::ArityError("count expects 1 argument".to_string()));
    }
    Ok(Value::Int(args[0].len() as i64))
}

pub fn first(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("first expects 1 argument".to_string()));
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
        _ => Err(WqError::TypeError("first only works on lists".to_string())),
    }
}

pub fn last(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("last expects 1 argument".to_string()));
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
        _ => Err(WqError::TypeError("last only works on lists".to_string())),
    }
}

pub fn reverse(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError(
            "reverse expects 1 argument".to_string(),
        ));
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
        _ => Err(WqError::TypeError(
            "reverse only works on lists".to_string(),
        )),
    }
}

pub fn sum(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("sum expects 1 argument".to_string()));
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
        return Err(WqError::ArityError("max expects 1 argument".to_string()));
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
        return Err(WqError::ArityError("min expects 1 argument".to_string()));
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
        return Err(WqError::ArityError("avg expects 1 argument".to_string()));
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
        return Err(WqError::ArityError(
            "take expects 1 or 2 arguments".to_string(),
        ));
    }
    match (&args[0], &args[1]) {
        (Value::Int(n), Value::List(items)) => {
            let n = *n as usize;
            if n > items.len() {
                Ok(Value::List(items.clone()))
            } else {
                Ok(Value::List(items[..n].to_vec()))
            }
        }
        (Value::Int(n), Value::IntList(items)) => {
            let n = *n as usize;
            if n > items.len() {
                Ok(Value::IntList(items.clone()))
            } else {
                Ok(Value::IntList(items[..n].to_vec()))
            }
        }
        _ => Err(WqError::TypeError(
            "take expects integer and list".to_string(),
        )),
    }
}

pub fn drop(args: &[Value]) -> WqResult<Value> {
    if args.len() == 1 {
        let tmp = [Value::Int(1), args[0].clone()];
        return drop(&tmp);
    }
    if args.len() != 2 {
        return Err(WqError::ArityError(
            "drop expects 1 or 2 arguments".to_string(),
        ));
    }
    match (&args[0], &args[1]) {
        (Value::Int(n), Value::List(items)) => {
            let n = *n as usize;
            if n >= items.len() {
                Ok(Value::List(Vec::new()))
            } else {
                Ok(Value::List(items[n..].to_vec()))
            }
        }
        (Value::Int(n), Value::IntList(items)) => {
            let n = *n as usize;
            if n >= items.len() {
                Ok(Value::IntList(Vec::new()))
            } else {
                Ok(Value::IntList(items[n..].to_vec()))
            }
        }
        _ => Err(WqError::TypeError(
            "drop expects integer and list".to_string(),
        )),
    }
}

pub fn where_func(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("where expects 1 argument".to_string()));
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
                        return Err(WqError::TypeError(
                            "where only works on integer or boolean lists".to_string(),
                        ));
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
        _ => Err(WqError::TypeError("where only works on lists".to_string())),
    }
}

pub fn distinct(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError(
            "distinct expects 1 argument".to_string(),
        ));
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
        _ => Err(WqError::TypeError(
            "distinct only works on lists".to_string(),
        )),
    }
}

pub fn sort(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("sort expects 1 argument".to_string()));
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
        _ => Err(WqError::TypeError("sort only works on lists".to_string())),
    }
}

pub fn cat(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::ArityError("cat expects 2 arguments".to_string()));
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
        (a, Value::List(b)) => {
            let mut res = vec![a.clone()];
            res.extend(b.clone());
            Ok(Value::List(res))
        }
        (a, b) => Ok(Value::List(vec![a.clone(), b.clone()])),
    }
}

pub fn flatten(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError(
            "flatten expects 1 argument".to_string(),
        ));
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
        return Err(WqError::ArityError("alloc expects 1 argument".to_string()));
    }
    alloc_shape(&args[0])
}

fn shape_value(v: &Value) -> WqResult<Value> {
    match v {
        Value::IntList(items) => Ok(Value::Int(items.len() as i64)),
        Value::List(items) => {
            if items.is_empty() {
                return Ok(Value::Int(0));
            }
            let mut shapes = Vec::with_capacity(items.len());
            for it in items {
                shapes.push(shape_value(it)?);
            }
            let first = shapes.first().cloned().unwrap();
            let uniform = shapes.iter().all(|s| *s == first);
            let simple = matches!(first, Value::Int(_) | Value::IntList(_));
            if uniform && simple {
                match first {
                    Value::Int(n) => Ok(Value::IntList(vec![items.len() as i64, n])),
                    Value::IntList(dims) => {
                        let mut dims2 = Vec::with_capacity(dims.len() + 1);
                        dims2.push(items.len() as i64);
                        dims2.extend(dims);
                        Ok(Value::IntList(dims2))
                    }
                    _ => unreachable!(),
                }
            } else {
                Ok(Value::List(shapes))
            }
        }
        _ => Err(WqError::TypeError("shape expects a list".to_string())),
    }
}

pub fn shape(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::ArityError("shape expects 1 argument".to_string()));
    }
    shape_value(&args[0])
}

pub fn idx(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::ArityError("idx expects 2 arguments".to_string()));
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
        return Err(WqError::ArityError("in expects 2 arguments".to_string()));
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
        return Err(WqError::ArityError("find expects 2 arguments".to_string()));
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
        let expected_shape = Value::List(vec![Value::IntList(vec![2, 2]), Value::Int(3)]);
        assert_eq!(shape(&[nested]).unwrap(), expected_shape);
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
}
