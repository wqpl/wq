use crate::{
    builtins::{BuiltinEnum as BE, wqerr_ext::check_arity, fold_value},
    value::{Value, WqResult},
    vm::Vm,
};

pub fn not(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Not, [1], args)?;
    args[0]
        .not_bool()
        .map_err(|e| e.into_wqerror().src(BE::Not))
}

pub fn and(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fold_value(BE::And, args, Value::and_bool)
}

pub fn or(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fold_value(BE::Or, args, Value::or_bool)
}

pub fn xor(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fold_value(BE::Xor, args, Value::xor_bool)
}

pub fn bnot(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Bnot, [1], args)?;

    args[0].bnot().map_err(|e| e.into_wqerror().src(BE::Bnot))
}

pub fn band(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fold_value(BE::Band, args, Value::band)
}

pub fn bor(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fold_value(BE::Bor, args, Value::bor)
}

pub fn bxor(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    fold_value(BE::Bxor, args, Value::bxor)
}

pub fn shl(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Shl, [2], args)?;
    args[0]
        .shl(&args[1])
        .map_err(|e| e.into_wqerror().src(BE::Shl))
}

pub fn shr(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Shr, [2], args)?;
    args[0]
        .shr(&args[1])
        .map_err(|e| e.into_wqerror().src(BE::Shr))
}
