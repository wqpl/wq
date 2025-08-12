use super::arity_error;
use crate::value::{Value, WqError, WqResult};

pub fn and(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("and", "2 arguments", args.len()));
    }
    args[0].and_bool(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "and expects booleans, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn or(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("or", "2 arguments", args.len()));
    }
    args[0].or_bool(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "or expects booleans, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn not(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("not", "1 argument", args.len()));
    }
    args[0].not_bool().ok_or_else(|| {
        WqError::TypeError(format!(
            "not expects a boolean, got {}",
            args[0].type_name()
        ))
    })
}

pub fn xor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("xor", "2 arguments", args.len()));
    }
    args[0].xor_bool(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "xor expects booleans, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn band(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("band", "2 arguments", args.len()));
    }
    args[0].bitand(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "band expects integers, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn bor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("bor", "2 arguments", args.len()));
    }
    args[0].bitor(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "bor expects integers, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn bxor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("bxor", "2 arguments", args.len()));
    }
    args[0].bitxor(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "bxor expects integers, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn bnot(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("bnot", "1 argument", args.len()));
    }
    args[0].bitnot().ok_or_else(|| {
        WqError::TypeError(format!(
            "bnot expects an integer, got {}",
            args[0].type_name()
        ))
    })
}

pub fn shl(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("shl", "2 arguments", args.len()));
    }
    args[0].shl(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "shl expects integers, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}

pub fn shr(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("shr", "2 arguments", args.len()));
    }
    args[0].shr(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "shr expects integers, got {} and {}",
            args[0].type_name(),
            args[1].type_name()
        ))
    })
}
