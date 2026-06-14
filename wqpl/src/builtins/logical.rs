use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{Value, WqResult};

pub(super) fn not(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Not, [1], &args)?;
    args[0].not().map_err(|e| e.src(BE::Not))
}

pub(super) fn and(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_bool_and(args)
}

pub(super) fn or(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_bool_or(args)
}

pub(super) fn xor(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_xor(args)
}

pub(super) fn band(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_bit_and(args)
}

pub(super) fn bor(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_bit_or(args)
}

pub(super) fn shl(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_shl(args)
}

pub(super) fn shr(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_shr(args)
}
