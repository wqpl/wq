use std::sync::Arc;

use crate::value::Value;

impl Value {
    pub(crate) fn cat(self, other: Value) -> Value {
        // Fast path: if both sides are char-sequences (String, List<Char>, or Char),
        // produce a unified String result. This also handles mixed String/List<Char>
        // concatenation which would otherwise fall through to generic List arms.

        if self.is_string_like() && other.is_string_like() {
            if self.is_unit() && other.is_unit() {
                return Value::unit();
            }

            let mut s = self.to_rust_string_with_note().expect("valid string");
            s.push_str(&other.to_rust_string_with_note().expect("valid string"));
            return Value::String(Arc::from(s));
        }

        match (self, other) {
            (Value::IntRange(a), Value::IntRange(b)) => {
                let mut res = Vec::with_capacity(a.len() + b.len());
                res.extend(a.iter());
                res.extend(b.iter());
                Value::IntList(Arc::new(res))
            }
            (Value::IntRange(a), Value::IntList(b)) => {
                let mut res = Vec::with_capacity(a.len() + b.len());
                res.extend(a.iter());
                res.extend(b.iter().copied());
                Value::IntList(Arc::new(res))
            }
            (Value::IntList(mut a), Value::IntRange(b)) => {
                Arc::make_mut(&mut a).extend(b.iter());
                Value::IntList(a)
            }
            (Value::IntList(mut a), Value::IntList(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().copied());
                Value::IntList(a)
            }
            (Value::IntRange(a), Value::Int(bv)) => {
                let mut res = Vec::with_capacity(a.len() + 1);
                res.extend(a.iter());
                res.push(bv);
                Value::IntList(Arc::new(res))
            }
            (Value::IntList(mut a), Value::Int(bv)) => {
                Arc::make_mut(&mut a).push(bv);
                Value::IntList(a)
            }
            (Value::Int(av), Value::IntRange(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(av);
                res.extend(b.iter());
                Value::IntList(Arc::new(res))
            }
            (Value::Int(av), Value::IntList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(av);
                res.extend(b.iter().copied());
                Value::IntList(Arc::new(res))
            }
            (Value::IntRange(a), Value::List(b)) => {
                let mut res: Vec<Value> = a.iter().map(Value::Int).collect();
                res.extend(b.iter().cloned());
                Value::List(Arc::new(res))
            }
            (Value::IntList(a), Value::List(b)) => {
                let mut res: Vec<Value> = a.iter().copied().map(Value::Int).collect();
                res.extend(b.iter().cloned());
                Value::List(Arc::new(res))
            }
            (Value::List(mut a), Value::IntRange(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().map(Value::Int));
                Value::List(a)
            }
            (Value::List(mut a), Value::IntList(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().copied().map(Value::Int));
                Value::List(a)
            }
            (Value::List(mut a), Value::List(b)) => {
                Arc::make_mut(&mut a).extend(b.iter().cloned());
                Value::List(a)
            }

            (Value::List(mut a), b) => {
                Arc::make_mut(&mut a).push(b);
                Value::List(a)
            }

            (Value::IntList(a), b) => {
                let mut res: Vec<Value> = a.iter().copied().map(Value::Int).collect();
                res.push(b);
                Value::List(Arc::new(res))
            }
            (Value::IntRange(a), b) => {
                let mut res: Vec<Value> = a.iter().map(Value::Int).collect();
                res.push(b);
                Value::List(Arc::new(res))
            }
            (a, Value::List(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().cloned());
                Value::List(Arc::new(res))
            }
            (a, Value::IntRange(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().map(Value::Int));
                Value::List(Arc::new(res))
            }
            (a, Value::IntList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().copied().map(Value::Int));
                Value::List(Arc::new(res))
            }

            (Value::Int(a), Value::Int(b)) => Value::IntList(Arc::new(vec![a, b])),
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
        if values.iter().all(|v| v.is_string_like()) {
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
        if values
            .iter()
            .all(|v| matches!(v, Value::Int(_) | Value::IntList(_) | Value::IntRange(_)))
        {
            let total_len: usize = values.iter().map(|v| v.len()).sum();
            let mut res = Vec::with_capacity(total_len);
            for v in values {
                match v {
                    Value::Int(i) => res.push(i),
                    Value::IntList(l) => res.extend(l.iter().copied()),
                    Value::IntRange(l) => res.extend(l.iter()),
                    _ => unreachable!(),
                }
            }
            return Value::IntList(Arc::new(res));
        }

        // All List / IntList / Set: pre-allocate a single List.
        if values
            .iter()
            .all(|v| matches!(v, Value::List(_) | Value::IntList(_) | Value::IntRange(_)))
        {
            let total_len: usize = values.iter().map(|v| v.len()).sum();
            let mut res: Vec<Value> = Vec::with_capacity(total_len);
            for v in values {
                match v {
                    Value::List(l) => res.extend(l.iter().cloned()),
                    Value::IntList(l) => res.extend(l.iter().copied().map(Value::Int)),
                    Value::IntRange(l) => res.extend(l.iter().map(Value::Int)),

                    _ => unreachable!(),
                }
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
                Value::IntList(items) => {
                    out.extend(items.iter().copied().map(Value::Int));
                }
                Value::IntRange(items) => {
                    out.extend(items.iter().map(Value::Int));
                }

                other => out.push(other.clone()),
            }
        }
        out
    }
}
