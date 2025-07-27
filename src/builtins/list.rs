use std::cmp::Ordering;

use crate::{
    builtins::TIL_CACHE,
    value::valuei::{Value, WqError, WqResult},
};

pub fn til(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "til expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
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
        return Err(WqError::FnArgCountMismatchError(
            "count expects 1 argument".to_string(),
        ));
    }
    Ok(Value::Int(args[0].len() as i64))
}

pub fn first(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "first expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
            "last expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
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
        return Err(WqError::FnArgCountMismatchError(
            "sum expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
            "max expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
            "min expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
            "avg expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
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
        return Err(WqError::FnArgCountMismatchError(
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
        return Err(WqError::FnArgCountMismatchError(
            "where expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
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
        return Err(WqError::FnArgCountMismatchError(
            "sort expects 1 argument".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
            "cat expects 2 arguments".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
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

pub fn alloc(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "alloc expects 1 argument".to_string(),
        ));
    }
    match args[0] {
        Value::Int(n) if n >= 0 => Ok(Value::IntList(vec![0; n as usize])),
        Value::Int(_) => Err(WqError::DomainError(
            "alloc length must be non-negative".to_string(),
        )),
        _ => Err(WqError::TypeError("alloc expects an integer".into())),
    }
}

pub fn idx(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "idx expects 2 arguments".to_string(),
        ));
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
        return Err(WqError::FnArgCountMismatchError(
            "in expects 2 arguments".to_string(),
        ));
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
        assert_eq!(
            in_list(&[Value::Int(4), lst]).unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn in_int_list() {
        let lst = Value::IntList(vec![1, 2, 3]);
        assert_eq!(
            in_list(&[Value::Int(2), lst.clone()]).unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            in_list(&[Value::Int(4), lst]).unwrap(),
            Value::Bool(false)
        );
    }
}
