use crate::value::valuei::{Value, WqError, WqResult};

pub fn keys(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "keys expects 1 argument".to_string(),
        ));
    }
    match &args[0] {
        Value::Dict(map) => {
            let mut ks: Vec<String> = map.keys().cloned().collect();
            ks.sort();
            let list = ks.into_iter().map(Value::Symbol).collect();
            Ok(Value::List(list))
        }
        _ => Err(WqError::TypeError("keys only works on dicts".to_string())),
    }
}
