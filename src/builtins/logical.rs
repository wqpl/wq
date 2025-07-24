use crate::value::valuei::{Value, WqError, WqResult};

pub fn and(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "and expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .and_bool(&args[1])
        .ok_or_else(|| WqError::TypeError("and only works on booleans".to_string()))
}

pub fn or(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "or expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .or_bool(&args[1])
        .ok_or_else(|| WqError::TypeError("or only works on booleans".to_string()))
}

pub fn not(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "not expects 1 argument".to_string(),
        ));
    }
    args[0]
        .not_bool()
        .ok_or_else(|| WqError::TypeError("not only works on booleans".to_string()))
}

pub fn xor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "xor expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .xor_bool(&args[1])
        .ok_or_else(|| WqError::TypeError("xor only works on booleans".to_string()))
}

pub fn band(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "band expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .bitand(&args[1])
        .ok_or_else(|| WqError::TypeError("band expects integers".to_string()))
}

pub fn bor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "bor expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .bitor(&args[1])
        .ok_or_else(|| WqError::TypeError("bor expects integers".to_string()))
}

pub fn bxor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "bxor expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .bitxor(&args[1])
        .ok_or_else(|| WqError::TypeError("bxor expects integers".to_string()))
}

pub fn bnot(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(WqError::FnArgCountMismatchError(
            "bnot expects 1 argument".to_string(),
        ));
    }
    args[0]
        .bitnot()
        .ok_or_else(|| WqError::TypeError("bnot expects integers".to_string()))
}

pub fn shl(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "shl expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .shl(&args[1])
        .ok_or_else(|| WqError::TypeError("shl expects integers".to_string()))
}

pub fn shr(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(WqError::FnArgCountMismatchError(
            "shr expects 2 arguments".to_string(),
        ));
    }
    args[0]
        .shr(&args[1])
        .ok_or_else(|| WqError::TypeError("shr expects integers".to_string()))
}
