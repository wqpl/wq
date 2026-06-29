use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{Value, WqResult};

pub(super) fn not(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Not, [1], &args)?;
    args[0].not().map_err(|e| e.src(BE::Not))
}

pub(super) fn and(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::bool_and(BE::And, args)
}

pub(super) fn or(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::bool_or(BE::Or, args)
}

pub(super) fn xor(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::bit_xor(BE::Xor, args)
}

pub(super) fn band(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::bit_and(BE::Band, args)
}

pub(super) fn bor(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::bit_or(BE::Bor, args)
}

pub(super) fn shl(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_shl(args)
}

pub(super) fn shr(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_shr(args)
}
