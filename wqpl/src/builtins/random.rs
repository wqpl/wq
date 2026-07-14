use num_traits::ToPrimitive as _;

use crate::builtins::{BuiltinContext, BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn rand(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Rand, [0, 1, 2], &args)?;
    vm.draw_default_random(&args)
}

pub(super) fn rng(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::Rng, [1], &args)?;
    let seed = match &args[0] {
        Value::Int(seed) => Some(*seed),
        Value::BigInt(seed) => seed.to_i64(),
        _ => None,
    }
    .ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BuiltinEnum::Rng)
            .msg("expected int seed in the signed 64-bit range")
            .at_arg(0)
    })?;
    Ok(Value::rng(seed))
}
