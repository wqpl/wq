use crate::astnode::BinaryOperator;
use crate::builtins::fold::fold_binary_op;
use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
use crate::value::{Value, WqResult};

pub(super) fn not(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Not, [1], &args)?;
    args[0].not().map_err(|e| e.src(BE::Not))
}

pub(super) fn and(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BE::And, args, &BinaryOperator::BoolAnd)
}

pub(super) fn or(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BE::Or, args, &BinaryOperator::BoolOr)
}

pub(super) fn band(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BE::Band, args, &BinaryOperator::BitAnd)
}

pub(super) fn bor(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BE::Bor, args, &BinaryOperator::BitOr)
}

pub(super) fn xor(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BE::Xor, args, &BinaryOperator::BitXor)
}

pub(super) fn shl(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_shl(args)
}

pub(super) fn shr(args: BuiltinFnArgs) -> WqResult<Value> {
    super::op::op_shr(args)
}
