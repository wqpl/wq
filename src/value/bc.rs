use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

impl Value {
    // Pack a vec result back into Value container, preferring IntList when safe.
    fn pack_bc_result(a: &Value, b: &Value, out: Vec<Value>) -> Value {
        // If not all ints, just return the original list (moves `out` here).
        if !out.iter().all(|v| matches!(v, Value::Int(_))) {
            return Value::List(out);
        }

        // Decide if we want an IntList result.
        let want_intlist = matches!((a, b), (Value::IntList(_), Value::IntList(_)))
            || (matches!(a, Value::IntList(_)) && b.is_atom())
            || (a.is_atom() && matches!(b, Value::IntList(_)));

        if want_intlist {
            // Now we can safely consume `out` exactly once.
            let ints = out
                .into_iter()
                .map(|v| match v {
                    Value::Int(i) => i,
                    _ => unreachable!(),
                })
                .collect::<Vec<i64>>();
            Value::IntList(ints)
        } else {
            Value::List(out)
        }
    }

    fn pack_unary_result(src: &Value, out: Vec<Value>) -> Value {
        let all_ints = out.iter().all(|v| matches!(v, Value::Int(_)));
        if all_ints && matches!(src, Value::IntList(_)) {
            Value::IntList(
                out.into_iter()
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
            Value::List(out)
        }
    }

    /// Generic 1-arg broadcasting
    pub fn bc1<F>(&self, f: F) -> WqResult<Value>
    where
        F: Fn(&Value) -> WqResult<Value> + Copy,
    {
        match self {
            Value::List(a) => {
                let mut out = Vec::with_capacity(a.len());
                for x in a {
                    out.push(x.bc1(f)?);
                }
                Ok(Value::List(out))
            }
            Value::IntList(a) => {
                let mut out = Vec::with_capacity(a.len());
                for &x in a {
                    out.push(f(&Value::Int(x))?);
                }
                Ok(Value::pack_unary_result(self, out))
            }
            _ => f(self),
        }
    }

    /// Generic 2-arg broadcasting, returns None on length mismatch.
    pub fn bc2<F>(&self, other: &Value, op: F) -> WqResult<Value>
    where
        F: Fn(&Value, &Value) -> WqResult<Value> + Copy,
    {
        match (self, other) {
            // elementwise over two lists (Vec<Value>)
            (Value::List(a), Value::List(b)) => {
                if a.len() != b.len() {
                    return Err(WqError::LengthError(format!(
                        "length mismatch ({}, {})",
                        a.len(),
                        b.len()
                    )));
                }
                if a.is_empty() || b.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(a.len());
                for (x, y) in a.iter().zip(b.iter()) {
                    out.push(x.bc2(y, op)?);
                }
                Ok(Value::List(out))
            }

            // IntList and IntList -> elementwise, must match lengths
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() != b.len() {
                    return Err(WqError::LengthError(format!(
                        "length mismatch ({}, {})",
                        a.len(),
                        b.len()
                    )));
                }
                if a.is_empty() || b.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(a.len());
                for (&x, &y) in a.iter().zip(b.iter()) {
                    out.push(Value::Int(x).bc2(&Value::Int(y), op)?);
                }
                Ok(Value::pack_bc_result(self, other, out))
            }

            // IntList and List -> elementwise, must match lengths
            (Value::IntList(a), Value::List(b)) => {
                if a.len() != b.len() {
                    return Err(WqError::LengthError(format!(
                        "length mismatch ({}, {})",
                        a.len(),
                        b.len()
                    )));
                }
                if a.is_empty() || b.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(a.len());
                for (&x, y) in a.iter().zip(b.iter()) {
                    out.push(Value::Int(x).bc2(y, op)?);
                }
                Ok(Value::List(out))
            }
            (Value::List(a), Value::IntList(b)) => {
                if a.len() != b.len() {
                    return Err(WqError::LengthError(format!(
                        "length mismatch ({}, {})",
                        a.len(),
                        b.len()
                    )));
                }
                if a.is_empty() || b.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(a.len());
                for (x, &y) in a.iter().zip(b.iter()) {
                    out.push(x.bc2(&Value::Int(y), op)?);
                }
                Ok(Value::List(out))
            }

            // Atom and List / IntList
            (s, Value::List(b)) if s.is_atom() => {
                if b.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(b.len());
                for y in b {
                    out.push(s.bc2(y, op)?);
                }
                Ok(Value::List(out))
            }
            (Value::List(a), s) if s.is_atom() => {
                if a.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(a.len());
                for x in a {
                    out.push(x.bc2(s, op)?);
                }
                Ok(Value::List(out))
            }
            (Value::IntList(a), s) if s.is_atom() => {
                if a.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(a.len());
                for &x in a {
                    out.push(Value::Int(x).bc2(s, op)?);
                }
                Ok(Value::pack_bc_result(self, other, out))
            }
            (s, Value::IntList(b)) if s.is_atom() => {
                if b.is_empty() {
                    return Ok(Value::IntList(vec![]));
                }
                let mut out = Vec::with_capacity(b.len());
                for &y in b {
                    out.push(s.bc2(&Value::Int(y), op)?);
                }
                Ok(Value::pack_bc_result(self, other, out))
            }

            // Both atoms
            (a, b) => op(a, b),
        }
    }
}
