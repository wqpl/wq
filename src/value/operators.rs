use super::valuei::Value;

impl Value {
    pub fn add(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a + *b as f64)),
            (Value::IntList(vec), Value::Int(b)) => {
                Some(Value::IntList(vec.iter().map(|x| x + b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                Some(Value::IntList(vec.iter().map(|x| a + x).collect()))
            }
            (Value::Float(a), Value::IntList(vec)) => {
                Some(Value::List(vec.iter().map(|&x| Value::Float(a + x as f64)).collect()))
            }
            (Value::IntList(vec), Value::Float(b)) => {
                Some(Value::List(vec.iter().map(|&x| Value::Float(x as f64 + b)).collect()))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter().zip(b.iter()).map(|(x, y)| x + y).collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()] + b[i % b.len()])
                            .collect(),
                    ))
                }
            }
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.add(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.add(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.add(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.add(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    fn is_scalar(&self) -> bool {
        matches!(
            self,
            Value::Int(_) | Value::Float(_) | Value::Char(_) | Value::Symbol(_) | Value::Bool(_)
        )
    }


    pub fn subtract(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a - b)),
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a - *b as f64)),
            (Value::IntList(vec), Value::Int(b)) => {
                Some(Value::IntList(vec.iter().map(|x| x - b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                Some(Value::IntList(vec.iter().map(|x| a - x).collect()))
            }
            (Value::Float(a), Value::IntList(vec)) => {
                Some(Value::List(vec.iter().map(|&x| Value::Float(a - x as f64)).collect()))
            }
            (Value::IntList(vec), Value::Float(b)) => {
                Some(Value::List(vec.iter().map(|&x| Value::Float(x as f64 - b)).collect()))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter().zip(b.iter()).map(|(x, y)| x - y).collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()] - b[i % b.len()])
                            .collect(),
                    ))
                }
            }
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.subtract(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.subtract(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.subtract(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.subtract(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn multiply(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a * b)),
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a * *b as f64)),
            (Value::IntList(vec), Value::Int(b)) => {
                Some(Value::IntList(vec.iter().map(|x| x * b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                Some(Value::IntList(vec.iter().map(|x| a * x).collect()))
            }
            (Value::Float(a), Value::IntList(vec)) => {
                Some(Value::List(vec.iter().map(|&x| Value::Float(a * x as f64)).collect()))
            }
            (Value::IntList(vec), Value::Float(b)) => {
                Some(Value::List(vec.iter().map(|&x| Value::Float(x as f64 * b)).collect()))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter().zip(b.iter()).map(|(x, y)| x * y).collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()] * b[i % b.len()])
                            .collect(),
                    ))
                }
            }
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.multiply(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.multiply(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.multiply(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.multiply(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn divide(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else if a % b == 0 {
                    Some(Value::Int(a / b))
                } else {
                    Some(Value::Float(*a as f64 / *b as f64))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(a / b))
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(*a as f64 / b))
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Float(a / *b as f64))
                }
            }
            (Value::IntList(items), Value::Int(b)) => {
                if *b == 0 {
                    return None;
                }
                let mut out = Vec::with_capacity(items.len());
                let mut all_int = true;
                for &x in items {
                    match Value::Int(x).divide(other) {
                        Some(Value::Int(n)) => out.push(Value::Int(n)),
                        Some(Value::Float(f)) => {
                            all_int = false;
                            out.push(Value::Float(f));
                        }
                        _ => return None,
                    }
                }
                if all_int {
                    let ints: Vec<i64> = out
                        .into_iter()
                        .map(|v| match v { Value::Int(n) => n, _ => unreachable!() })
                        .collect();
                    Some(Value::IntList(ints))
                } else {
                    Some(Value::List(out))
                }
            }
            (Value::Int(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else if vec.iter().any(|x| *x == 0) {
                    None
                } else {
                    Some(Value::IntList(vec.iter().map(|x| a / x).collect()))
                }
            }
            (Value::Float(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else if vec.iter().any(|&x| x == 0) {
                    None
                } else {
                    let out = vec.iter().map(|&x| Value::Float(a / x as f64)).collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(vec), Value::Float(b)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else if *b == 0.0 {
                    None
                } else {
                    let out = vec.iter().map(|&x| Value::Float(x as f64 / b)).collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    if b.iter().any(|&x| x == 0) {
                        return None;
                    }
                    Some(Value::IntList(a.iter().zip(b.iter()).map(|(x, y)| x / y).collect()))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    for i in 0..max_len {
                        let right = b[i % b.len()];
                        if right == 0 {
                            return None;
                        }
                    }
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| {
                                let left = a[i % a.len()];
                                let right = b[i % b.len()];
                                left / right
                            })
                            .collect(),
                    ))
                }
            }
            // Scalar-vector operations
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.divide(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.divide(scalar)).collect();
                result.map(Value::List)
            }
            // Vector-vector operations
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.divide(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    // Cycle the shorter vector
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.divide(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn divide_dot(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Some(Value::Float(if *a >= 0 { f64::INFINITY } else { f64::NEG_INFINITY }))
                } else if a % b == 0 {
                    Some(Value::Int(a / b))
                } else {
                    Some(Value::Float(*a as f64 / *b as f64))
                }
            }
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a / b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a / *b as f64)),
            (Value::IntList(items), Value::Int(b)) => {
                if *b == 0 {
                    let out = items
                        .iter()
                        .map(|&x| Value::Float(if x >= 0 { f64::INFINITY } else { f64::NEG_INFINITY }))
                        .collect();
                    Some(Value::List(out))
                } else {
                    self.divide(other)
                }
            }
            (Value::Int(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else {
                    let mut out = Vec::with_capacity(vec.len());
                    let mut all_int = true;
                    for &x in vec {
                        let val = if x == 0 {
                            all_int = false;
                            Value::Float(if *a >= 0 { f64::INFINITY } else { f64::NEG_INFINITY })
                        } else if a % x == 0 {
                            Value::Int(a / x)
                        } else {
                            all_int = false;
                            Value::Float(*a as f64 / x as f64)
                        };
                        out.push(val);
                    }
                    if all_int {
                        let ints: Vec<i64> = out.into_iter().map(|v| match v { Value::Int(n) => n, _ => unreachable!() }).collect();
                        Some(Value::IntList(ints))
                    } else {
                        Some(Value::List(out))
                    }
                }
            }
            (Value::Float(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else {
                    let out = vec
                        .iter()
                        .map(|&x| {
                            if x == 0 {
                                Value::Float(if *a >= 0.0 { f64::INFINITY } else { f64::NEG_INFINITY })
                            } else {
                                Value::Float(a / x as f64)
                            }
                        })
                        .collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(vec), Value::Float(b)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else {
                    let out = vec
                        .iter()
                        .map(|&x| {
                            if *b == 0.0 {
                                Value::Float(if x >= 0 { f64::INFINITY } else { f64::NEG_INFINITY })
                            } else {
                                Value::Float(x as f64 / b)
                            }
                        })
                        .collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    let mut out = Vec::with_capacity(a.len());
                    let mut all_int = true;
                    for (&x, &y) in a.iter().zip(b.iter()) {
                        let val = if y == 0 {
                            all_int = false;
                            Value::Float(if x >= 0 { f64::INFINITY } else { f64::NEG_INFINITY })
                        } else if x % y == 0 {
                            Value::Int(x / y)
                        } else {
                            all_int = false;
                            Value::Float(x as f64 / y as f64)
                        };
                        out.push(val);
                    }
                    if all_int {
                        Some(Value::IntList(out.into_iter().map(|v| if let Value::Int(n) = v { n } else { unreachable!() }).collect()))
                    } else {
                        Some(Value::List(out))
                    }
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let mut out = Vec::with_capacity(max_len);
                    let mut all_int = true;
                    for i in 0..max_len {
                        let left = a[i % a.len()];
                        let right = b[i % b.len()];
                        let val = if right == 0 {
                            all_int = false;
                            Value::Float(if left >= 0 { f64::INFINITY } else { f64::NEG_INFINITY })
                        } else if left % right == 0 {
                            Value::Int(left / right)
                        } else {
                            all_int = false;
                            Value::Float(left as f64 / right as f64)
                        };
                        out.push(val);
                    }
                    if all_int {
                        Some(Value::IntList(out.into_iter().map(|v| if let Value::Int(n) = v { n } else { unreachable!() }).collect()))
                    } else {
                        Some(Value::List(out))
                    }
                }
            }
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.divide_dot(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.divide_dot(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> = a.iter().zip(b.iter()).map(|(x, y)| x.divide_dot(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.divide_dot(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn modulo(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Int(a % b))
                }
            }
            (Value::Float(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(a % b))
                }
            }
            (Value::Int(a), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    Some(Value::Float(*a as f64 % b))
                }
            }
            (Value::Float(a), Value::Int(b)) => {
                if *b == 0 {
                    None
                } else {
                    Some(Value::Float(a % *b as f64))
                }
            }
            (Value::IntList(items), Value::Int(b)) => {
                if *b == 0 {
                    return None;
                }
                Some(Value::IntList(items.iter().map(|x| x % b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else if vec.iter().any(|x| *x == 0) {
                    None
                } else {
                    Some(Value::IntList(vec.iter().map(|x| a % x).collect()))
                }
            }
            (Value::Float(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    None
                } else if vec.iter().any(|&x| x == 0) {
                    None
                } else {
                    let out = vec.iter().map(|&x| Value::Float(a % x as f64)).collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(vec), Value::Float(b)) => {
                if *b == 0.0 {
                    None
                } else {
                    let out = vec.iter().map(|&x| Value::Float(x as f64 % b)).collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    if b.iter().any(|&x| x == 0) {
                        return None;
                    }
                    Some(Value::IntList(a.iter().zip(b.iter()).map(|(x, y)| x % y).collect()))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    for i in 0..max_len {
                        if b[i % b.len()] == 0 {
                            return None;
                        }
                    }
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| {
                                let left = a[i % a.len()];
                                let right = b[i % b.len()];
                                left % right
                            })
                            .collect(),
                    ))
                }
            }
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.modulo(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.modulo(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.modulo(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.modulo(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    pub fn modulo_dot(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    Some(Value::nan())
                } else {
                    Some(Value::Int(a % b))
                }
            }
            (Value::Float(a), Value::Float(b)) => Some(Value::Float(a % b)),
            (Value::Int(a), Value::Float(b)) => Some(Value::Float(*a as f64 % b)),
            (Value::Float(a), Value::Int(b)) => Some(Value::Float(a % *b as f64)),
            (Value::IntList(items), Value::Int(b)) => {
                let out = items
                    .iter()
                    .map(|&x| {
                        if *b == 0 {
                            Value::nan()
                        } else {
                            Value::Int(x % b)
                        }
                    })
                    .collect();
                Some(Value::List(out))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    Some(Value::List(Vec::new()))
                } else {
                    let mut out = Vec::with_capacity(vec.len());
                    for &x in vec {
                        if x == 0 {
                            out.push(Value::nan());
                        } else {
                            out.push(Value::Int(a % x));
                        }
                    }
                    Some(Value::List(out))
                }
            }
            (Value::Float(a), Value::IntList(vec)) => {
                if vec.is_empty() {
                    None
                } else {
                    let out = vec
                        .iter()
                        .map(|&x| {
                            if x == 0 {
                                Value::nan()
                            } else {
                                Value::Float(a % x as f64)
                            }
                        })
                        .collect();
                    Some(Value::List(out))
                }
            }
            (Value::IntList(vec), Value::Float(b)) => {
                let out = vec
                    .iter()
                    .map(|&x| {
                        if *b == 0.0 {
                            Value::nan()
                        } else {
                            Value::Float(x as f64 % b)
                        }
                    })
                    .collect();
                Some(Value::List(out))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    let out = a
                        .iter()
                        .zip(b.iter())
                        .map(|(x, y)| if *y == 0 { Value::nan() } else { Value::Int(x % y) })
                        .collect();
                    Some(Value::List(out))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let out = (0..max_len)
                        .map(|i| {
                            let left = a[i % a.len()];
                            let right = b[i % b.len()];
                            if right == 0 {
                                Value::nan()
                            } else {
                                Value::Int(left % right)
                            }
                        })
                        .collect();
                    Some(Value::List(out))
                }
            }
            (scalar, Value::List(vec)) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.modulo_dot(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar) if scalar.is_scalar() => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.modulo_dot(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> = a.iter().zip(b.iter()).map(|(x, y)| x.modulo_dot(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.modulo_dot(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Bitwise AND with broadcasting support
    pub fn bitand(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a & b)),
            (Value::IntList(vec), Value::Int(b)) => {
                Some(Value::IntList(vec.iter().map(|x| x & b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                Some(Value::IntList(vec.iter().map(|x| a & x).collect()))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter().zip(b.iter()).map(|(x, y)| x & y).collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()] & b[i % b.len()])
                            .collect(),
                    ))
                }
            }
            (scalar @ Value::Int(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.bitand(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Int(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.bitand(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.bitand(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.bitand(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Bitwise OR with broadcasting support
    pub fn bitor(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a | b)),
            (Value::IntList(vec), Value::Int(b)) => {
                Some(Value::IntList(vec.iter().map(|x| x | b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                Some(Value::IntList(vec.iter().map(|x| a | x).collect()))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter().zip(b.iter()).map(|(x, y)| x | y).collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()] | b[i % b.len()])
                            .collect(),
                    ))
                }
            }
            (scalar @ Value::Int(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.bitor(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Int(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.bitor(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.bitor(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.bitor(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Bitwise XOR with broadcasting support
    pub fn bitxor(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a ^ b)),
            (Value::IntList(vec), Value::Int(b)) => {
                Some(Value::IntList(vec.iter().map(|x| x ^ b).collect()))
            }
            (Value::Int(a), Value::IntList(vec)) => {
                Some(Value::IntList(vec.iter().map(|x| a ^ x).collect()))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter().zip(b.iter()).map(|(x, y)| x ^ y).collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()] ^ b[i % b.len()])
                            .collect(),
                    ))
                }
            }
            (scalar @ Value::Int(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.bitxor(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Int(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.bitxor(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.bitxor(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.bitxor(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Bitwise NOT with broadcasting support
    pub fn bitnot(&self) -> Option<Value> {
        match self {
            Value::Int(a) => Some(Value::Int(!a)),
            Value::IntList(vec) => Some(Value::IntList(vec.iter().map(|x| !x).collect())),
            Value::List(items) => {
                let result: Option<Vec<Value>> = items.iter().map(|v| v.bitnot()).collect();
                result.map(Value::List)
            }
            _ => None,
        }
    }

    /// Left shift with broadcasting support
    pub fn shl(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a.wrapping_shl(*b as u32))),
            (Value::IntList(vec), Value::Int(b)) => {
                let shift = *b as u32;
                Some(Value::IntList(
                    vec.iter().map(|x| x.wrapping_shl(shift)).collect(),
                ))
            }
            (Value::Int(a), Value::IntList(vec)) => Some(Value::IntList(
                vec.iter().map(|x| a.wrapping_shl(*x as u32)).collect(),
            )),
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter()
                            .zip(b.iter())
                            .map(|(x, y)| x.wrapping_shl(*y as u32))
                            .collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()].wrapping_shl(b[i % b.len()] as u32))
                            .collect(),
                    ))
                }
            }
            (scalar @ Value::Int(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.shl(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Int(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.shl(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.shl(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.shl(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Right shift with broadcasting support
    pub fn shr(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => Some(Value::Int(a.wrapping_shr(*b as u32))),
            (Value::IntList(vec), Value::Int(b)) => {
                let shift = *b as u32;
                Some(Value::IntList(
                    vec.iter().map(|x| x.wrapping_shr(shift)).collect(),
                ))
            }
            (Value::Int(a), Value::IntList(vec)) => Some(Value::IntList(
                vec.iter().map(|x| a.wrapping_shr(*x as u32)).collect(),
            )),
            (Value::IntList(a), Value::IntList(b)) => {
                if a.len() == b.len() {
                    Some(Value::IntList(
                        a.iter()
                            .zip(b.iter())
                            .map(|(x, y)| x.wrapping_shr(*y as u32))
                            .collect(),
                    ))
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    Some(Value::IntList(
                        (0..max_len)
                            .map(|i| a[i % a.len()].wrapping_shr(b[i % b.len()] as u32))
                            .collect(),
                    ))
                }
            }
            (scalar @ Value::Int(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.shr(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Int(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.shr(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.shr(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.shr(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical AND with broadcasting support
    pub fn and_bool(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a && *b)),
            (scalar @ Value::Bool(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.and_bool(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Bool(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.and_bool(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.and_bool(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.and_bool(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical OR with broadcasting support
    pub fn or_bool(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a || *b)),
            (scalar @ Value::Bool(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.or_bool(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Bool(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.or_bool(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.or_bool(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.or_bool(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical XOR with broadcasting support
    pub fn xor_bool(&self, other: &Value) -> Option<Value> {
        match (self, other) {
            (Value::Bool(a), Value::Bool(b)) => Some(Value::Bool(*a ^ *b)),
            (scalar @ Value::Bool(_), Value::List(vec)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| scalar.xor_bool(x)).collect();
                result.map(Value::List)
            }
            (Value::List(vec), scalar @ Value::Bool(_)) => {
                let result: Option<Vec<Value>> = vec.iter().map(|x| x.xor_bool(scalar)).collect();
                result.map(Value::List)
            }
            (Value::List(a), Value::List(b)) => {
                if a.len() == b.len() {
                    let result: Option<Vec<Value>> =
                        a.iter().zip(b.iter()).map(|(x, y)| x.xor_bool(y)).collect();
                    result.map(Value::List)
                } else if a.is_empty() || b.is_empty() {
                    None
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Option<Vec<Value>> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            left.xor_bool(right)
                        })
                        .collect();
                    result.map(Value::List)
                }
            }
            _ => None,
        }
    }

    /// Logical NOT with broadcasting support
    pub fn not_bool(&self) -> Option<Value> {
        match self {
            Value::Bool(b) => Some(Value::Bool(!b)),
            Value::List(items) => {
                let result: Option<Vec<Value>> = items.iter().map(|v| v.not_bool()).collect();
                result.map(Value::List)
            }
            _ => None,
        }
    }

    fn compare_values(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
        match (a, b) {
            (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
            (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
            (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
            (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
            (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
            (Value::Char(x), Value::Char(y)) => Some(x.cmp(y)),
            (Value::List(a), Value::List(b)) => Value::compare_lists(a, b),
            (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
            _ => None,
        }
    }

    fn compare_lists(a: &[Value], b: &[Value]) -> Option<std::cmp::Ordering> {
        let len = a.len().min(b.len());
        for i in 0..len {
            match Value::compare_values(&a[i], &b[i])? {
                std::cmp::Ordering::Equal => continue,
                ord => return Some(ord),
            }
        }
        Some(a.len().cmp(&b.len()))
    }

    /// Helper for vectorized comparisons
    fn cmp_broadcast<F>(&self, other: &Value, cmp: F) -> Value
    where
        F: Fn(&Value, &Value) -> bool + Copy,
    {
        match (self, other) {
            (Value::List(_), Value::List(_)) if self.is_string() && other.is_string() => {
                Value::Bool(cmp(self, other))
            }
            (Value::IntList(a), Value::IntList(b)) => {
                if a.is_empty() || b.is_empty() {
                    Value::List(Vec::new())
                } else if a.len() == b.len() {
                    let result: Vec<Value> = a
                        .iter()
                        .zip(b.iter())
                        .map(|(x, y)| Value::Bool(cmp(&Value::Int(*x), &Value::Int(*y))))
                        .collect();
                    Value::List(result)
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Vec<Value> = (0..max_len)
                        .map(|i| {
                            let left = Value::Int(a[i % a.len()]);
                            let right = Value::Int(b[i % b.len()]);
                            Value::Bool(cmp(&left, &right))
                        })
                        .collect();
                    Value::List(result)
                }
            }
            (Value::IntList(a), b) => {
                let result: Vec<Value> = a
                    .iter()
                    .map(|x| Value::Bool(cmp(&Value::Int(*x), b)))
                    .collect();
                Value::List(result)
            }
            (a, Value::IntList(b)) => {
                let result: Vec<Value> = b
                    .iter()
                    .map(|y| Value::Bool(cmp(a, &Value::Int(*y))))
                    .collect();
                Value::List(result)
            }
            (Value::List(a), Value::List(b)) => {
                if a.is_empty() || b.is_empty() {
                    Value::List(Vec::new())
                } else if a.len() == b.len() {
                    let result: Vec<Value> = a
                        .iter()
                        .zip(b.iter())
                        .map(|(x, y)| Value::Bool(cmp(x, y)))
                        .collect();
                    Value::List(result)
                } else {
                    let max_len = a.len().max(b.len());
                    let result: Vec<Value> = (0..max_len)
                        .map(|i| {
                            let left = &a[i % a.len()];
                            let right = &b[i % b.len()];
                            Value::Bool(cmp(left, right))
                        })
                        .collect();
                    Value::List(result)
                }
            }
            (Value::List(a), b) => {
                let result: Vec<Value> = a.iter().map(|x| Value::Bool(cmp(x, b))).collect();
                Value::List(result)
            }
            (a, Value::List(b)) => {
                let result: Vec<Value> = b.iter().map(|y| Value::Bool(cmp(a, y))).collect();
                Value::List(result)
            }
            (a, b) => Value::Bool(cmp(a, b)),
        }
    }

    /// Comparison operations
    pub fn equals(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(Value::compare_values(a, b), Some(std::cmp::Ordering::Equal))
        })
    }

    pub fn not_equals(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            !matches!(Value::compare_values(a, b), Some(std::cmp::Ordering::Equal))
        })
    }

    pub fn less_than(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(Value::compare_values(a, b), Some(std::cmp::Ordering::Less))
        })
    }

    pub fn less_than_or_equal(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(
                Value::compare_values(a, b),
                Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
            )
        })
    }

    pub fn greater_than(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(
                Value::compare_values(a, b),
                Some(std::cmp::Ordering::Greater)
            )
        })
    }

    pub fn greater_than_or_equal(&self, other: &Value) -> Value {
        self.cmp_broadcast(other, |a, b| {
            matches!(
                Value::compare_values(a, b),
                Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
            )
        })
    }
}
