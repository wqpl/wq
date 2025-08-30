use super::arity_error;
use crate::value::{Value, WqResult};

pub fn and(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("and", "2", args.len()));
    }
    args[0].and_bool(&args[1])
}

pub fn or(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("or", "2", args.len()));
    }
    args[0].or_bool(&args[1])
}

pub fn not(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("not", "1", args.len()));
    }
    args[0].not_bool()
}

pub fn xor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("xor", "2", args.len()));
    }
    args[0].xor_bool(&args[1])
}

pub fn band(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("band", "2", args.len()));
    }
    args[0].bitand(&args[1])
}

pub fn bor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("bor", "2", args.len()));
    }
    args[0].bitor(&args[1])
}

pub fn bxor(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("bxor", "2", args.len()));
    }
    args[0].bitxor(&args[1])
}

pub fn bnot(args: &[Value]) -> WqResult<Value> {
    if args.len() != 1 {
        return Err(arity_error("bnot", "1", args.len()));
    }
    args[0].bitnot()
}

pub fn shl(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("shl", "2", args.len()));
    }
    args[0].shl(&args[1])
}

pub fn shr(args: &[Value]) -> WqResult<Value> {
    if args.len() != 2 {
        return Err(arity_error("shr", "2", args.len()));
    }
    args[0].shr(&args[1])
}
