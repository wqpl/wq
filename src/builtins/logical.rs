use super::arity_error;
use crate::value::{Value, WqError, WqResult};

pub fn and(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("and", "2", args.len()));
    }
    args[0].and_bool(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`and`: expected bools, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn or(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("or", "2", args.len()));
    }
    args[0].or_bool(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`or`: expected bools, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn not(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("not", "1", args.len()));
    }
    args[0].not_bool().ok_or_else(|| {
        WqError::TypeError(format!(
            "`not`: expected bool, got {}",
            args[0].type_name_verbose()
        ))
    })
}

pub fn xor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("xor", "2", args.len()));
    }
    args[0].xor_bool(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`xor`: expected bools, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn band(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("band", "2", args.len()));
    }
    args[0].bitand(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`band`: expected ints, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn bor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("bor", "2", args.len()));
    }
    args[0].bitor(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`bor`: expected ints, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn bxor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("bxor", "2", args.len()));
    }
    args[0].bitxor(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`bxor`: expected ints, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn bnot(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("bnot", "1", args.len()));
    }
    args[0].bitnot().ok_or_else(|| {
        WqError::TypeError(format!(
            "`bnot`: expected int, got {}",
            args[0].type_name_verbose()
        ))
    })
}

pub fn shl(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("shl", "2", args.len()));
    }
    args[0].shl(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`shl`: expected ints, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}

pub fn shr(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("shr", "2", args.len()));
    }
    args[0].shr(&args[1]).ok_or_else(|| {
        WqError::TypeError(format!(
            "`shr`: expected ints, got {} and {}",
            args[0].type_name_verbose(),
            args[1].type_name_verbose()
        ))
    })
}
