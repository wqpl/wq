use super::arity_error;
use crate::{
    value::{Value, WqResult},
    wqerror::WqError,
};

pub fn keys(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("keys", "1", args.len()));
    }
    match &args[0] {
        Value::Dict(map) => {
            let ks: Vec<String> = map.keys().cloned().collect();
            // dont sort
            // ks.sort();
            let list = ks.into_iter().map(Value::Symbol).collect();
            Ok(Value::List(list))
        }
        _ => Err(WqError::DomainError(format!(
            "`keys`: expected dict, got {}",
            args[0].type_name_verbose()
        ))),
    }
}

// pub fn values(args: &[Value]) -> WqResult<Value> {
//     if args.len() != 1 {
//         return Err(arity_error("values", "1", args.len()));
//     }
//     match &args[0] {
//         Value::Dict(map) => {
//             let ks: Vec<Value> = map.values().cloned().collect();
//             // ks.sort();
//             let list = ks.into_iter().collect();
//             Ok(Value::List(list))
//         }
//         _ => Err(WqError::DomainError(format!(
//             "`keys`: expected dict, got {}",
//             args[0].type_name_verbose()
//         ))),
//     }
// }
