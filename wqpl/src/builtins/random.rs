use num_traits::ToPrimitive as _;

use crate::builtins::{BuiltinContext, BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::{Value, WqResult};
use crate::wqerror::{Requirement, WqError, WqErrorType};

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
            .expected(Requirement::phrase(
                "int in the signed 64-bit range",
                "ints in the signed 64-bit range",
            ))
            .at_arg(0)
            .got1(&args[0])
    })?;
    Ok(Value::rng(seed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_reports_the_seed_range_and_value() {
        let error = rng(BuiltinFnArgs::from(Value::Bool(true))).expect_err("bool seed should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected int in the signed 64-bit range")
        );
        assert_eq!(error.notes.as_ref(), &["at argument 1", "got T (bool)"]);
    }
}
