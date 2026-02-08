use crate::value::Value;

impl Value {
    pub fn cat(self, other: Value) -> Value {
        match (self, other) {
            (Value::IntList(mut a), Value::IntList(b)) => {
                a.extend(b);
                Value::IntList(a)
            }
            (Value::IntList(mut a), Value::Int(bv)) => {
                a.push(bv);
                Value::IntList(a)
            }
            (Value::Int(av), Value::IntList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(av);
                res.extend(b);
                Value::IntList(res)
            }
            (Value::IntList(a), Value::List(b)) => {
                let mut res: Vec<Value> = a.into_iter().map(Value::Int).collect();
                res.extend(b);
                Value::List(res)
            }
            (Value::List(mut a), Value::IntList(b)) => {
                a.extend(b.iter().copied().map(Value::Int));
                Value::List(a)
            }
            (Value::List(mut a), Value::List(b)) => {
                a.extend(b);
                Value::List(a)
            }
            (Value::List(mut a), b) => {
                a.push(b);
                Value::List(a)
            }
            (Value::IntList(a), b) => {
                let mut res: Vec<Value> = a.into_iter().map(Value::Int).collect();
                res.push(b);
                Value::List(res)
            }
            (a, Value::List(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b);
                Value::List(res)
            }
            (a, Value::IntList(b)) => {
                let mut res = Vec::with_capacity(b.len() + 1);
                res.push(a);
                res.extend(b.iter().copied().map(Value::Int));
                Value::List(res)
            }
            (Value::Int(a), Value::Int(b)) => Value::IntList(vec![a, b]),
            (a, b) => Value::List(vec![a, b]),
        }
    }

    pub fn flatten(&self) -> Vec<Value> {
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
                Value::IntList(items) => {
                    out.extend(items.iter().copied().map(Value::Int));
                }
                other => out.push(other.clone()),
            }
        }
        out
    }
}
