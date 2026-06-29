use std::sync::Arc;

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::builtins::fold::fold_binary_op;
use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::cmp::eval_cmp_chain;
use crate::value::{Value, WqResult, eval_unary};
use crate::wqerror::{WqError, WqErrorType};

fn fold_cmp_op(src: BuiltinEnum, args: &[Value], op: BinaryOperator) -> WqResult<Value> {
    if args.len() < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .src(src)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }
    let ops = vec![op; args.len() - 1];
    eval_cmp_chain(&ops, args).map_err(|e| e.src(src))
}

pub(super) fn op_add(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpAdd, args, &BinaryOperator::Add)
}

pub(super) fn op_sub(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::OpSub)
            .msg("expected 1 or more args, got 0"));
    }
    if args.len() == 1 {
        eval_unary(&UnaryOperator::Negate, &args[0]).map_err(|e| e.src(BuiltinEnum::OpSub))
    } else {
        fold_binary_op(BuiltinEnum::OpSub, args, &BinaryOperator::Subtract)
    }
}

pub(super) fn op_mul(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpMul, args, &BinaryOperator::Multiply)
}

pub(super) fn op_div(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpDiv, args, &BinaryOperator::Divide)
}

pub(super) fn op_divdot(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpDivDot, args, &BinaryOperator::DivideDot)
}

pub(super) fn op_mod(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpMod, args, &BinaryOperator::Modulo)
}

pub(super) fn op_power(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpPower, args, &BinaryOperator::Power)
}

pub(super) fn op_power_dot(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpPowerDot, args, &BinaryOperator::PowerDot)
}

pub(super) fn op_matmul(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpMatmul, args, &BinaryOperator::Matmul)
}

pub(super) fn op_equal(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpEqual, &args, BinaryOperator::Equal)
}

pub(super) fn op_equal_dot(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpEqualDot, &args, BinaryOperator::EqualDot)
}

pub(super) fn op_tilde(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::OpTilde)
            .msg("expected 1 or more args, got 0"));
    }
    if args.len() == 1 {
        eval_unary(&UnaryOperator::Not, &args[0]).map_err(|e| e.src(BuiltinEnum::OpTilde))
    } else {
        fold_cmp_op(BuiltinEnum::OpTilde, &args, BinaryOperator::NotEqual)
    }
}

pub(super) fn op_tilde_dot(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpTildeDot, &args, BinaryOperator::NotEqualDot)
}

pub(super) fn op_lt(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpLt, &args, BinaryOperator::Lt)
}

pub(super) fn op_lte(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpLte, &args, BinaryOperator::Lte)
}

pub(super) fn op_gt(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpGt, &args, BinaryOperator::Gt)
}

pub(super) fn op_gte(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpGte, &args, BinaryOperator::Gte)
}

pub(super) fn op_cat(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.len() < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BuiltinEnum::OpCat)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }

    // Fast path: all unit -> return unit directly
    if args.iter().all(|v| v.is_unit()) {
        return Ok(Value::unit());
    }

    // Fast path: all string-like (String/Char mix, but not all unit)
    if args.iter().all(|v| v.is_string_like()) {
        let mut s = String::new();
        for arg in args {
            s.push_str(&arg.to_rust_string_with_note().expect("valid string"));
        }
        return Ok(Value::String(Arc::from(s)));
    }

    // Fast path: all intlist or int -> build intlist in one pass
    if args
        .iter()
        .all(|v| matches!(v, Value::IntList(_) | Value::Int(_)))
    {
        let mut total_len = 0usize;
        for arg in args.iter() {
            match arg {
                Value::IntList(items) => total_len = total_len.saturating_add(items.len()),
                Value::Int(_) => total_len = total_len.saturating_add(1),
                _ => {}
            }
        }
        let mut res = Vec::with_capacity(total_len);
        for arg in args {
            match arg {
                Value::IntList(items) => res.extend(items.iter().copied()),
                Value::Int(i) => res.push(i),
                _ => {}
            }
        }
        return Ok(Value::IntList(Arc::new(res)));
    }

    // Fallback: cat all values together
    Ok(Value::cat_many(args.to_vec()))
}

pub(super) fn op_floordiv(args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpFloorDiv, args, &BinaryOperator::FloorDiv)
}

pub(super) fn op_sharp(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::OpSharp, [1], &args)?;
    eval_unary(&UnaryOperator::Count, &args[0]).map_err(|e| e.src(BuiltinEnum::OpSharp))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn op_notequal_one_arg_uses_unary_not() {
        assert_eq!(
            op_tilde(BuiltinFnArgs::from(Value::Bool(true))).unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            op_tilde(BuiltinFnArgs::from(Value::Int(1))).unwrap(),
            Value::Int(!1)
        );
    }

    #[test]
    fn op_notequal_multiple_args_still_compares() {
        assert_eq!(
            op_tilde(BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)])).unwrap(),
            Value::Bool(true)
        );
    }
}
