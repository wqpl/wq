use crate::ast::BinaryOperator;
use crate::builtins::{BuiltinEnum, BuiltinFnArgs, at_least_arity_error};
use crate::value::{Value, WqResult, eval_binary};

pub(super) fn fold_binary_op(
    src: BuiltinEnum,
    args: BuiltinFnArgs,
    op: &BinaryOperator,
) -> WqResult<Value> {
    if args.len() < 2 {
        return Err(at_least_arity_error(src, 2, args.len()));
    }
    let mut iter = args.into_iter();
    let init = iter.next().unwrap();
    iter.try_fold(init, |acc, v| {
        eval_binary(op, &acc, &v).map_err(|e| e.src(src))
    })
}
