use super::arity_error;
use crate::value::valuei::{Value, WqError, WqResult};

pub fn keys(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("keys", "1 argument", args.len()));
    }
    match &args[0] {
        Value::Dict(map) => {
            let mut ks: Vec<String> = map.keys().cloned().collect();
            ks.sort();
            let list = ks.into_iter().map(Value::Symbol).collect();
            Ok(Value::List(list))
        }
        _ => Err(WqError::TypeError(format!(
            "keys expects dict, got {}",
            args[0].type_name()
        ))),
    }
}
