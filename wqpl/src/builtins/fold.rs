use crate::astnode::BinaryOperator;
use crate::builtins::{BuiltinEnum, BuiltinFnArgs};
use crate::value::{Value, WqResult, eval_binary};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn fold_binary_op(
    src: BuiltinEnum,
    args: BuiltinFnArgs,
    op: &BinaryOperator,
) -> WqResult<Value> {
    if args.len() < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .src(src)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }
    let mut iter = args.into_iter();
    let init = iter.next().unwrap();
    iter.try_fold(init, |acc, v| {
        eval_binary(op, &acc, &v).map_err(|e| e.src(src))
    })
}
