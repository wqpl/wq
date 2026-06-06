use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity, fold_value};
use crate::value::{Value, WqResult};
use crate::vm::Vm;

pub(super) fn not(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Not, [1], &args)?;
    args[0].not_bool().map_err(|e| e.src(BE::Not))
}

pub(super) fn and(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::And, args, Value::and_bool)
}

pub(super) fn or(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Or, args, Value::or_bool)
}

pub(super) fn xor(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Xor, args, Value::xor_bool)
}

pub(super) fn bnot(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Bnot, [1], &args)?;
    args[0].bnot().map_err(|e| e.src(BE::Bnot))
}

pub(super) fn band(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Band, args, Value::band)
}

pub(super) fn bor(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Bor, args, Value::bor)
}

pub(super) fn bxor(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Bxor, args, Value::bxor)
}

pub(super) fn shl(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Shl, args, Value::shl)
}

pub(super) fn shr(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_value(BE::Shr, args, Value::shr)
}
