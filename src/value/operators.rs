use std::cmp::Ordering;

use super::Value;

impl Value {
    #[inline]
    fn is_scalar(&self) -> bool {
        !matches!(self, Value::List(_) | Value::IntList(_))
    }

    // Pack a vec result back into Value container, preferring IntList when safe.
    fn pack_bc_result(a: &Value, b: &Value, out: Vec<Value>) -> Value {
        // If not all ints, just return the original list (moves `out` here).
        if !out.iter().all(|v| matches!(v, Value::Int(_))) {
            return Value::List(out);
        }

        // Decide if we want an IntList result.
        let want_intlist = matches!((a, b), (Value::IntList(_), Value::IntList(_)))
            || (matches!(a, Value::IntList(_)) && b.is_scalar())
            || (a.is_scalar() && matches!(b, Value::IntList(_)));

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
    fn bc1<F>(&self, f: F) -> Option<Value>
    where
        F: Fn(&Value) -> Option<Value> + Copy,
    {
        match self {
            Value::List(a) => {
                let mut out = Vec::with_capacity(a.len());
                for x in a {
                    out.push(x.bc1(f)?);
                }
                Some(Value::List(out))
            }
            Value::IntList(a) => {
                let mut out = Vec::with_capacity(a.len());
                for &x in a {
                    out.push(f(&Value::Int(x))?);
                }
                Some(Value::pack_unary_result(self, out))
            }
            _ => f(self),
        }
    }

    /// Generic 2-arg broadcasting, returns None on length mismatch.
    fn bc2<F>(&self, other: &Value, op: F) -> Option<Value>
    where
        F: Fn(&Value, &Value) -> Option<Value> + Copy,
    {
        match (self, other) {
            // elementwise over two lists (Vec<Value>)
            (Value::List(a), Value::List(b)) => {
                if a.is_empty() || b.is_empty() || a.len() != b.len() {
                    return None;
                }
                let mut out = Vec::with_capacity(a.len());
                for (x, y) in a.iter().zip(b.iter()) {
                    out.push(x.bc2(y, op)?);
                }
                Some(Value::List(out))
            }

            // IntList and IntList -> elementwise, must match lengths
            (Value::IntList(a), Value::IntList(b)) => {
                if a.is_empty() || b.is_empty() || a.len() != b.len() {
                    return None;
                }
                let mut out = Vec::with_capacity(a.len());
                for (&x, &y) in a.iter().zip(b.iter()) {
                    out.push(Value::Int(x).bc2(&Value::Int(y), op)?);
                }
                Some(Value::pack_bc_result(self, other, out))
            }

            // IntList and List -> elementwise, must match lengths
            (Value::IntList(a), Value::List(b)) => {
                if a.is_empty() || b.is_empty() || a.len() != b.len() {
                    return None;
                }
                let mut out = Vec::with_capacity(a.len());
                for (&x, y) in a.iter().zip(b.iter()) {
                    out.push(Value::Int(x).bc2(y, op)?);
                }
                Some(Value::List(out))
            }
            (Value::List(a), Value::IntList(b)) => {
                if a.is_empty() || b.is_empty() || a.len() != b.len() {
                    return None;
                }
                let mut out = Vec::with_capacity(a.len());
                for (x, &y) in a.iter().zip(b.iter()) {
                    out.push(x.bc2(&Value::Int(y), op)?);
                }
                Some(Value::List(out))
            }

            // Atom and List / IntList
            (s, Value::List(b)) if s.is_scalar() => {
                if b.is_empty() {
                    return None;
                }
                let mut out = Vec::with_capacity(b.len());
                for y in b {
                    out.push(s.bc2(y, op)?);
                }
                Some(Value::List(out))
            }
            (Value::List(a), s) if s.is_scalar() => {
                if a.is_empty() {
                    return None;
                }
                let mut out = Vec::with_capacity(a.len());
                for x in a {
                    out.push(x.bc2(s, op)?);
                }
                Some(Value::List(out))
            }
            (Value::IntList(a), s) if s.is_scalar() => {
                if a.is_empty() {
                    return None;
                }
                let mut out = Vec::with_capacity(a.len());
                for &x in a {
                    out.push(Value::Int(x).bc2(s, op)?);
                }
                Some(Value::pack_bc_result(self, other, out))
            }
            (s, Value::IntList(b)) if s.is_scalar() => {
                if b.is_empty() {
                    return None;
                }
                let mut out = Vec::with_capacity(b.len());
                for &y in b {
                    out.push(s.bc2(&Value::Int(y), op)?);
                }
                Some(Value::pack_bc_result(self, other, out))
            }

            // Both atoms
            (a, b) => op(a, b),
        }
    }

    // arithmetic ops
    // ==============

    pub fn neg(&self) -> Option<Value> {
        self.bc1(|v| match v {
            Value::Int(n) => n.checked_neg().map(Value::Int),
            Value::Float(f) => Some(Value::Float(-f)),
            _ => None,
        })
    }

    pub fn add(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => x.checked_add(*y).map(Value::Int),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x + y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 + *y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(*x + *y as f64)),
            _ => None, // non-numeric or unsupported
        })
    }

    pub fn subtract(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => x.checked_sub(*y).map(Value::Int),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x - y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 - y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(x - *y as f64)),
            _ => None,
        })
    }

    pub fn multiply(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => x.checked_mul(*y).map(Value::Int),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x * y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 * y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(x * *y as f64)),
            _ => None,
        })
    }

    pub fn divide(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(_), Value::Int(0)) => None,
            (Value::Float(_), Value::Int(0)) => None,
            (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,

            (Value::Int(x), Value::Int(y)) => Some(Value::Float(*x as f64 / *y as f64)),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x / y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 / y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(x / *y as f64)),
            _ => None,
        })
    }

    pub fn divide_dot(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            // (Value::Int(_), Value::Int(0)) => None,
            // allow y==0.0`
            // (Value::Float(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,
            (Value::Int(x), Value::Int(y)) => Some(Value::Float(*x as f64 / *y as f64)),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x / y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 / *y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(*x / *y as f64)),
            _ => None,
        })
    }

    pub fn modulo(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(_), Value::Int(0)) => None,
            (Value::Float(_), Value::Int(0)) => None,
            (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,

            (Value::Int(x), Value::Int(y)) => Some(Value::Int(x % y)),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x % y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 % y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(x % *y as f64)),
            _ => None,
        })
    }

    pub fn modulo_dot(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            // (Value::Int(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Int(0)) => None,
            // (Value::Float(_), Value::Float(y)) if *y == 0.0 => None,
            (Value::Int(x), Value::Int(y)) => Some(Value::Float(*x as f64 % *y as f64)),
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x % y)),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float(*x as f64 % y)),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(x % *y as f64)),
            _ => None,
        })
    }

    pub fn power(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) if *y >= 0 => x.checked_pow(*y as u32).map(Value::Int),
            (Value::Int(x), Value::Int(y)) /* y < 0 */ => {
                // handle 0.pow(negative) explicitly
                if *x == 0 {
                    None // or Some(Value::Float(f64::INFINITY)) depending on semantics
                } else {
                    Some(Value::Float((*x as f64).powf(*y as f64)))
                }
            }
            (Value::Float(x), Value::Float(y)) => Some(Value::Float(x.powf(*y))),
            (Value::Int(x), Value::Float(y)) => Some(Value::Float((*x as f64).powf(*y))),
            (Value::Float(x), Value::Int(y)) => Some(Value::Float(x.powf(*y as f64))),
            _ => None,
        })
    }

    // bitwise ops
    // ===========

    pub fn bitand(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(Value::Int(x & y)),
            _ => None,
        })
    }

    pub fn bitor(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(Value::Int(x | y)),
            _ => None,
        })
    }

    pub fn bitxor(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(Value::Int(x ^ y)),
            _ => None,
        })
    }

    pub fn bitnot(&self) -> Option<Value> {
        self.bc1(|v| match v {
            Value::Int(x) => Some(Value::Int(!x)),
            _ => None,
        })
    }

    // Shifts (reject negative shift counts)

    pub fn shl(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(s)) if *s >= 0 => {
                Some(Value::Int(x.wrapping_shl(*s as u32)))
            }
            _ => None,
        })
    }

    pub fn shr(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Int(x), Value::Int(s)) if *s >= 0 => {
                Some(Value::Int(x.wrapping_shr(*s as u32)))
            }
            _ => None,
        })
    }

    // Logical

    pub fn and_bool(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a && *b)),
            _ => None,
        })
    }

    pub fn or_bool(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a || *b)),
            _ => None,
        })
    }

    pub fn xor_bool(&self, other: &Value) -> Option<Value> {
        self.bc2(other, |a, b| match (a, b) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a ^ *b)),
            _ => None,
        })
    }

    pub fn not_bool(&self) -> Option<Value> {
        self.bc1(|v| match v {
            Value::Bool(b) => Some(Value::Bool(!b)),
            _ => None,
        })
    }

    // Comparison

    #[inline]
    fn cmp_atom(a: &Value, b: &Value) -> Option<Ordering> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
            (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
            (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
            (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
            (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
            (Value::Symbol(x), Value::Symbol(y)) => Some(x.cmp(y)),
            // (Value::List(a), Value::List(b)) => Value::compare_lists(a, b),
            (Value::Null, Value::Null) => Some(Ordering::Equal),
            _ => None,
        }
    }

    #[inline]
    fn cmp_pred<F>(&self, other: &Value, pred: F) -> Option<Value>
    where
        F: Fn(Ordering) -> bool + Copy,
    {
        self.bc2(other, |a, b| {
            let ord = Self::cmp_atom(a, b)?;
            Some(Value::Bool(pred(ord)))
        })
    }

    pub fn eq(&self, other: &Value) -> Option<Value> {
        match self.cmp_pred(other, |o| o == Ordering::Equal) {
            Some(v) => Some(v),
            None => Some(Value::Bool(false)),
        }
    }

    pub fn neq(&self, other: &Value) -> Option<Value> {
        match self.cmp_pred(other, |o| o != Ordering::Equal) {
            Some(v) => Some(v),
            None => Some(Value::Bool(true)),
        }
    }

    pub fn lt(&self, other: &Value) -> Option<Value> {
        self.cmp_pred(other, |o| o == Ordering::Less)
    }

    pub fn leq(&self, other: &Value) -> Option<Value> {
        self.cmp_pred(other, |o| o != Ordering::Greater)
    }

    pub fn gt(&self, other: &Value) -> Option<Value> {
        self.cmp_pred(other, |o| o == Ordering::Greater)
    }

    pub fn geq(&self, other: &Value) -> Option<Value> {
        self.cmp_pred(other, |o| o != Ordering::Less)
    }

    // pub fn min_value(&self, other: &Value) -> Option<Value> {
    //     self.bc2(other, |a, b| {
    //         match Self::cmp_atom(a, b)? {
    //                 Ordering::Greater => Some(b.clone()),
    //                 _ /* Less|Equal  */ => Some(a.clone()),
    //             }
    //     })
    // }

    // pub fn max_value(&self, other: &Value) -> Option<Value> {
    //     self.bc2(other, |a, b| {
    //         match Self::cmp_atom(a, b)? {
    //                 Ordering::Less => Some(b.clone()),
    //                 _ /* Greater|Equal */ => Some(a.clone()),
    //             }
    //     })
    // }
}
