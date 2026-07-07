use std::sync::Arc;

use crate::value::Value;
use crate::value::seq::ListStorageSeq;

impl Value {
    pub(crate) fn cat(self, other: Value) -> Value {
        // Fast path: if both sides are char-sequences (String, List<Char>, or Char),
        // produce a unified String result. This also handles mixed String/List<Char>
        // concatenation which would otherwise fall through to generic List arms.

        if self.is_string() && other.is_string() {
            if self.is_unit() && other.is_unit() {
                return Value::unit();
            }

            let mut s = self.to_rust_string_with_note().expect("valid string");
            s.push_str(&other.to_rust_string_with_note().expect("valid string"));
            return Value::String(Arc::from(s));
        }

        if let (Some(a), Some(b)) = (self.native_int_seq(), other.native_int_seq()) {
            let mut res = Vec::with_capacity(a.len() + b.len());
            res.extend(a.iter());
            res.extend(b.iter());
            return Value::IntList(Arc::new(res));
        }

        if let Some(a) = self.packed_int_seq()
            && let Value::List(b) = &other
        {
            let mut res: Vec<Value> = a.iter().map(Value::Int).collect();
            res.extend(b.iter().cloned());
            return Value::List(Arc::new(res));
        }

        if let Value::List(a) = &self
            && let Some(b) = other.packed_int_seq()
        {
            let mut res: Vec<Value> = Vec::with_capacity(a.len() + b.len());
            res.extend(a.iter().cloned());
            res.extend(b.iter().map(Value::Int));
            return Value::List(Arc::new(res));
        }

        if let Some(a) = self.packed_int_seq() {
            let mut res: Vec<Value> = a.iter().map(Value::Int).collect();
            res.push(other);
            return Value::List(Arc::new(res));
        }

        if let Some(b) = other.packed_int_seq() {
            let mut res = Vec::with_capacity(b.len() + 1);
            res.push(self);
            res.extend(b.iter().map(Value::Int));
            return Value::List(Arc::new(res));
        }

        match (self, other) {
            (Value::Float(a), Value::Float(b)) => Value::FloatList(Arc::new(vec![a, b])),
            (Value::FloatList(mut a), Value::FloatList(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().copied());
                Value::FloatList(a)
            }
            (Value::FloatList(mut a), Value::Float(b)) => {
                Arc::make_mut(&mut a).push(b);
                Value::FloatList(a)
            }
            (Value::Float(a), Value::FloatList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().copied());
                Value::FloatList(Arc::new(res))
            }
            (Value::FloatList(a), Value::List(b)) => {
                let mut res: Vec<Value> = a.iter().copied().map(Value::Float).collect();
                res.extend(b.iter().cloned());
                Value::List(Arc::new(res))
            }
            (Value::List(mut a), Value::FloatList(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().copied().map(Value::Float));
                Value::List(a)
            }
            (Value::FloatList(a), b) => {
                let mut res: Vec<Value> = a.iter().copied().map(Value::Float).collect();
                res.push(b);
                Value::List(Arc::new(res))
            }
            (a, Value::FloatList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().copied().map(Value::Float));
                Value::List(Arc::new(res))
            }
            (Value::Bool(a), Value::Bool(b)) => Value::BoolList(Arc::new(vec![a, b])),
            (Value::BoolList(mut a), Value::BoolList(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().copied());
                Value::BoolList(a)
            }
            (Value::BoolList(mut a), Value::Bool(b)) => {
                Arc::make_mut(&mut a).push(b);
                Value::BoolList(a)
            }
            (Value::Bool(a), Value::BoolList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().copied());
                Value::BoolList(Arc::new(res))
            }
            (Value::BoolList(a), Value::List(b)) => {
                let mut res: Vec<Value> = a.iter().copied().map(Value::Bool).collect();
                res.extend(b.iter().cloned());
                Value::List(Arc::new(res))
            }
            (Value::List(mut a), Value::BoolList(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().copied().map(Value::Bool));
                Value::List(a)
            }
            (Value::BoolList(a), b) => {
                let mut res: Vec<Value> = a.iter().copied().map(Value::Bool).collect();
                res.push(b);
                Value::List(Arc::new(res))
            }
            (a, Value::BoolList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().copied().map(Value::Bool));
                Value::List(Arc::new(res))
            }
            (Value::List(mut a), Value::List(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().cloned());
                Value::List(a)
            }

            (Value::List(mut a), b) => {
                Arc::make_mut(&mut a).push(b);
                Value::List(a)
            }

            (a, Value::List(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().cloned());
                Value::List(Arc::new(res))
            }

            (a, b) => Value::List(Arc::new(vec![a, b])),
        }
    }

    /// Concatenate many values at once, pre-allocating the target collection
    /// based on total length rather than growing incrementally.
    pub(crate) fn cat_many(values: Vec<Value>) -> Value {
        if values.is_empty() {
            return Value::unit();
        }
        if values.len() == 1 {
            return values.into_iter().next().expect("len==1");
        }
        if values.iter().all(|v| v.is_unit()) {
            return Value::unit();
        }

        // All string-like: pre-allocate a single String buffer.
        if values.iter().all(|v| v.is_string()) {
            let strings: Vec<String> = values
                .into_iter()
                .filter_map(|v| v.to_rust_string_with_note().ok())
                .collect();
            let total_len: usize = strings.iter().map(|s| s.len()).sum();
            let mut s = String::with_capacity(total_len);
            for part in strings {
                s.push_str(&part);
            }
            return Value::String(Arc::from(s));
        }

        // All Int / IntList: pre-allocate a single IntList.
        if values.iter().all(|v| v.native_int_seq().is_some()) {
            let total_len: usize = values.iter().map(|v| v.len()).sum();
            let mut res = Vec::with_capacity(total_len);
            for v in values {
                let items = v
                    .native_int_seq()
                    .expect("all values are native int sequences");
                res.extend(items.iter());
            }
            return Value::IntList(Arc::new(res));
        }

        // All Float / FloatList: pre-allocate a single FloatList.
        if values
            .iter()
            .all(|v| matches!(v, Value::Float(_) | Value::FloatList(_)))
        {
            let total_len: usize = values.iter().map(|v| v.len()).sum();
            let mut res = Vec::with_capacity(total_len);
            for v in values {
                match v {
                    Value::Float(f) => res.push(f),
                    Value::FloatList(items) => res.extend(items.iter().copied()),
                    _ => unreachable!(),
                }
            }
            return Value::FloatList(Arc::new(res));
        }

        // All Bool / BoolList: pre-allocate a single BoolList.
        if values
            .iter()
            .all(|v| matches!(v, Value::Bool(_) | Value::BoolList(_)))
        {
            let total_len: usize = values.iter().map(|v| v.len()).sum();
            let mut res = Vec::with_capacity(total_len);
            for v in values {
                match v {
                    Value::Bool(b) => res.push(b),
                    Value::BoolList(items) => res.extend(items.iter().copied()),
                    _ => unreachable!(),
                }
            }
            return Value::BoolList(Arc::new(res));
        }

        // All generic or packed list storage: pre-allocate a single List.
        if values
            .iter()
            .all(|v| ListStorageSeq::from_value(v).is_some())
        {
            let total_len: usize = values.iter().map(|v| v.len()).sum();
            let mut res: Vec<Value> = Vec::with_capacity(total_len);
            for v in values {
                let items = ListStorageSeq::from_value(&v)
                    .expect("all values are generic or packed list storage");
                items.extend_values(&mut res);
            }
            return Value::List(Arc::new(res));
        }

        // Fallback: fold using the existing cat logic.
        values
            .into_iter()
            .reduce(|acc, v| acc.cat(v))
            .unwrap_or_else(Value::unit)
    }

    pub(crate) fn flatten(&self) -> Vec<Value> {
        let mut out = Vec::new();
        let mut stack: Vec<&Value> = vec![self];
        while let Some(cur) = stack.pop() {
            match cur {
                Value::List(items) => {
                    // push in reverse to preserve original order
                    for v in items.iter().rev() {
                        stack.push(v);
                    }
                }
                Value::String(s) => {
                    out.extend(s.chars().map(Value::Char));
                }
                items if items.packed_int_seq().is_some() => {
                    let items = items.packed_int_seq().expect("checked packed int sequence");
                    out.extend(items.iter().map(Value::Int));
                }
                Value::BoolList(items) => {
                    out.extend(items.iter().copied().map(Value::Bool));
                }
                Value::FloatList(items) => {
                    out.extend(items.iter().copied().map(Value::Float));
                }

                other => out.push(other.clone()),
            }
        }
        out
    }
}
