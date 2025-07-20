use crate::value::{Value, WqError, WqResult};
use rand::Rng;
use std::collections::HashMap;

/// builtin functions
pub struct Builtins {
    functions: HashMap<String, fn(&[Value]) -> WqResult<Value>>,
}

impl Builtins {
    pub fn new() -> Self {
        let mut builtins = Builtins {
            functions: HashMap::new(),
        };
        builtins.register_functions();
        builtins
    }

    fn register_functions(&mut self) {
        // Arithmetic functions
        self.functions.insert("abs".to_string(), abs);
        self.functions.insert("neg".to_string(), neg);
        self.functions.insert("signum".to_string(), signum);
        self.functions.insert("sqrt".to_string(), sqrt);
        self.functions.insert("exp".to_string(), exp);
        self.functions.insert("log".to_string(), log);
        self.functions.insert("floor".to_string(), floor);
        self.functions.insert("ceiling".to_string(), ceiling);
        self.functions.insert("rand".to_string(), rand);
        self.functions.insert("echo".to_string(), echo);

        // Math functions
        self.functions.insert("sin".into(), sin);
        self.functions.insert("cos".into(), cos);
        self.functions.insert("tan".into(), tan);
        self.functions.insert("sinh".into(), sinh);
        self.functions.insert("cosh".into(), cosh);
        self.functions.insert("tanh".into(), tanh);

        // List functions
        self.functions.insert("count".to_string(), count);
        self.functions.insert("first".to_string(), first);
        self.functions.insert("last".to_string(), last);
        self.functions.insert("reverse".to_string(), reverse);
        self.functions.insert("sum".to_string(), sum);
        self.functions.insert("max".to_string(), max);
        self.functions.insert("min".to_string(), min);
        self.functions.insert("avg".to_string(), avg);

        // Generation functions
        self.functions.insert("til".to_string(), til);
        self.functions.insert("range".to_string(), range);

        // Type functions
        self.functions.insert("type".to_string(), type_of);
        self.functions.insert("string".to_string(), to_string);

        // List manipulation
        self.functions.insert("take".to_string(), take);
        self.functions.insert("drop".to_string(), drop);
        self.functions.insert("where".to_string(), where_func);
        self.functions.insert("distinct".to_string(), distinct);
        self.functions.insert("sort".to_string(), sort);
        self.functions.insert("cat".to_string(), cat);
        self.functions.insert("flatten".to_string(), flatten);

        // Logical functions
        self.functions.insert("and".to_string(), and);
        self.functions.insert("or".to_string(), or);
        self.functions.insert("not".to_string(), not);
    }

    pub fn call(&self, name: &str, args: &[Value]) -> WqResult<Value> {
        if let Some(func) = self.functions.get(name) {
            func(args)
        } else {
            Err(WqError::DomainError(format!(
                "Unknown builtin function: {name}",
            )))
        }
    }

    pub fn has_function(&self, name: &str) -> bool {
        self.functions.contains_key(name)
    }

    pub fn list_functions(&self) -> Vec<String> {
        self.functions.keys().cloned().collect()
    }
}

impl Default for Builtins {
    fn default() -> Self {
        Self::new()
    }
}

// Arithmetic functions
fn abs(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "abs expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(n.abs())),
        Value::Float(f) => Ok(Value::Float(f.abs())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| abs(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("abs only works on numbers".to_string())),
    }
}

fn neg(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "neg expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(-n)),
        Value::Float(f) => Ok(Value::Float(-f)),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| neg(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("neg only works on numbers".to_string())),
    }
}

fn signum(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "signum expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(if *n > 0 {
            1
        } else if *n < 0 {
            -1
        } else {
            0
        })),
        Value::Float(f) => Ok(Value::Float(if *f > 0.0 {
            1.0
        } else if *f < 0.0 {
            -1.0
        } else {
            0.0
        })),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| signum(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(
            "signum only works on numbers".to_string(),
        )),
    }
}

fn sqrt(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "sqrt expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n < 0 {
                Err(WqError::DomainError("sqrt of negative number".to_string()))
            } else {
                Ok(Value::Float((*n as f64).sqrt()))
            }
        }
        Value::Float(f) => {
            if *f < 0.0 {
                Err(WqError::DomainError("sqrt of negative number".to_string()))
            } else {
                Ok(Value::Float(f.sqrt()))
            }
        }
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| sqrt(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("sqrt only works on numbers".to_string())),
    }
}

fn exp(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "exp expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Float((*n as f64).exp())),
        Value::Float(f) => Ok(Value::Float(f.exp())),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| exp(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("exp only works on numbers".to_string())),
    }
}

fn log(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "log expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => {
            if *n <= 0 {
                Err(WqError::DomainError(
                    "log of non-positive number".to_string(),
                ))
            } else {
                Ok(Value::Float((*n as f64).ln()))
            }
        }
        Value::Float(f) => {
            if *f <= 0.0 {
                Err(WqError::DomainError(
                    "log of non-positive number".to_string(),
                ))
            } else {
                Ok(Value::Float(f.ln()))
            }
        }
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| log(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError("log only works on numbers".to_string())),
    }
}

fn floor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "floor expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(f.floor() as i64)),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> = items.iter().map(|v| floor(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(
            "floor only works on numbers".to_string(),
        )),
    }
}

fn ceiling(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "ceiling expects 1 argument".to_string(),
        ));
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => Ok(Value::Int(f.ceil() as i64)),
        Value::List(items) => {
            let result: WqResult<Vec<Value>> =
                items.iter().map(|v| ceiling(&[v.clone()])).collect();
            Ok(Value::List(result?))
        }
        _ => Err(WqError::TypeError(
            "ceiling only works on numbers".to_string(),
        )),
    }
}

// Math Functions
macro_rules! bind_math {
    ($name:ident, $func:path) => {
        pub fn $name(args: &[Value]) -> WqResult<Value> {
            if args.len() != 1 {
                return Err(WqError::FnArgCountMismatchError(
                    stringify!($name).to_string() + " expects 1 argument",
                ));
            }
            match &args[0] {
                Value::Int(n) => Ok(Value::Float($func(*n as f64))),
                Value::Float(f) => Ok(Value::Float($func(*f))),
                Value::List(items) => {
                    let result: WqResult<Vec<Value>> =
                        items.iter().map(|v| $name(&[v.clone()])).collect();
                    Ok(Value::List(result?))
                }
                other => Err(WqError::TypeError(
                    stringify!($name).to_string()
                        + " only works on numbers or lists of numbers, got "
                        + other.type_name(),
                )),
            }
        }
    };
}

bind_math!(sin, f64::sin);
bind_math!(cos, f64::cos);
bind_math!(tan, f64::tan);
bind_math!(sinh, f64::sinh);
bind_math!(cosh, f64::cosh);
bind_math!(tanh, f64::tanh);

// List functions
fn count(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "count expects 1 argument".to_string(),
        ));
    }
    Ok(Value::Int(args[0].len() as i64))
}

fn first(args: &[Value]) -> WqResult<Value> {
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
        _ => Err(WqError::TypeError("first only works on lists".to_string())),
    }
}

fn last(args: &[Value]) -> WqResult<Value> {
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
        _ => Err(WqError::TypeError("last only works on lists".to_string())),
    }
}

fn reverse(args: &[Value]) -> WqResult<Value> {
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
        _ => Err(WqError::TypeError(
            "reverse only works on lists".to_string(),
        )),
    }
}

fn sum(args: &[Value]) -> WqResult<Value> {
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
        _ => Ok(args[0].clone()),
    }
}

fn max(args: &[Value]) -> WqResult<Value> {
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
        _ => Ok(args[0].clone()),
    }
}

fn min(args: &[Value]) -> WqResult<Value> {
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
        _ => Ok(args[0].clone()),
    }
}

fn avg(args: &[Value]) -> WqResult<Value> {
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
        _ => Ok(args[0].clone()),
    }
}

// Generation functions
fn til(args: &[Value]) -> WqResult<Value> {
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
            let items: Vec<Value> = (0..*n).map(Value::Int).collect();
            Ok(Value::List(items))
        }
        _ => Err(WqError::TypeError("til only works on integers".to_string())),
    }
}

fn range(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "range expects 2 arguments".to_string(),
        ));
    }
    match (&args[0], &args[1]) {
        (Value::Int(start), Value::Int(end)) => {
            let items: Vec<Value> = (*start..*end).map(Value::Int).collect();
            Ok(Value::List(items))
        }
        _ => Err(WqError::TypeError(
            "range only works on integers".to_string(),
        )),
    }
}

pub fn rand(args: &[Value]) -> WqResult<Value> {
    let mut rng = rand::thread_rng();
    match args.len() {
        0 => Ok(Value::Float(rng.r#gen())),
        1 => match &args[0] {
            Value::Int(n) if *n > 0 => Ok(Value::Int(rng.gen_range(0..*n))),
            Value::Float(f) if *f > 0.0 => Ok(Value::Float(rng.gen_range(0.0..*f))),
            v => Err(WqError::DomainError(format!(
                "expected positive int or float, got {}",
                v.type_name()
            ))),
        },
        2 => match (&args[0], &args[1]) {
            (Value::Int(a), Value::Int(b)) if a < b => Ok(Value::Int(rng.gen_range(*a..*b))),
            (a, b) => {
                let af = match a {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "expected numbers, got {}",
                            a.type_name()
                        )));
                    }
                };
                let bf = match b {
                    Value::Int(n) => *n as f64,
                    Value::Float(f) => *f,
                    _ => {
                        return Err(WqError::TypeError(format!(
                            "expected numbers, got {}",
                            b.type_name()
                        )));
                    }
                };
                if af < bf {
                    Ok(Value::Float(rng.gen_range(af..bf)))
                } else {
                    Err(WqError::RuntimeError(format!(
                        "require a < b, got {af} >= {bf}"
                    )))
                }
            }
        },
        other => Err(WqError::FnArgCountMismatchError(format!(
            "rand expects 0, 1 or 2 arguments, got {other}"
        ))),
    }
}

// Type functions
fn type_of(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "type expects 1 argument".to_string(),
        ));
    }
    Ok(Value::Symbol(args[0].type_name().to_string()))
}

fn to_string(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "string expects 1 argument".to_string(),
        ));
    }
    Ok(Value::Symbol(args[0].to_string()))
}

// List manipulation
fn take(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "take expects 2 arguments".to_string(),
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
        _ => Err(WqError::TypeError(
            "take expects integer and list".to_string(),
        )),
    }
}

fn drop(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "drop expects 2 arguments".to_string(),
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
        _ => Err(WqError::TypeError(
            "drop expects integer and list".to_string(),
        )),
    }
}

fn where_func(args: &[Value]) -> WqResult<Value> {
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
                    _ => {
                        return Err(WqError::TypeError(
                            "where only works on integer lists".to_string(),
                        ));
                    }
                }
            }
            Ok(Value::List(indices))
        }
        _ => Err(WqError::TypeError("where only works on lists".to_string())),
    }
}

fn distinct(args: &[Value]) -> WqResult<Value> {
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
        _ => Err(WqError::TypeError(
            "distinct only works on lists".to_string(),
        )),
    }
}

fn sort(args: &[Value]) -> WqResult<Value> {
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
                _ => std::cmp::Ordering::Equal,
            });
            Ok(Value::List(sorted))
        }
        _ => Err(WqError::TypeError("sort only works on lists".to_string())),
    }
}

fn cat(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "cat expects 2 arguments".to_string(),
        ));
    }

    let left = &args[0];
    let right = &args[1];

    match (left, right) {
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

fn flatten(args: &[Value]) -> WqResult<Value> {
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
            other => out.push(other.clone()),
        }
    }

    let mut result = Vec::new();
    flatten_value(&args[0], &mut result);
    Ok(Value::List(result))
}

// Logical functions
fn and(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "and expects 2 arguments".to_string(),
        ));
    }
    match (&args[0], &args[1]) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a && *b)),
        _ => Err(WqError::TypeError("and only works on booleans".to_string())),
    }
}

fn or(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "or expects 2 arguments".to_string(),
        ));
    }
    match (&args[0], &args[1]) {
        (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(*a || *b)),
        _ => Err(WqError::TypeError("or only works on booleans".to_string())),
    }
}

fn not(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "not expects 1 argument".to_string(),
        ));
    }
    match &args[0] {
        Value::Bool(n) => Ok(Value::Bool(!(*n))),
        _ => Err(WqError::TypeError("not only works on booleans".to_string())),
    }
}

fn echo(args: &[Value]) -> WqResult<Value> {
    for arg in args {
        println!("{arg}");
    }
    Ok(Value::Null)
}
