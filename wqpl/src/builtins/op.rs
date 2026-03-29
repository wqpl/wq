use std::sync::Arc;

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::builtins::{BuiltinEnum, BuiltinFnArgs, check_arity};
use crate::value::cmp::eval_cmp_chain;
use crate::value::{Value, WqResult, eval_binary, eval_unary};
use crate::vm::Vm;
use crate::wqerror::{WqError, WqErrorType};

fn fold_binary_op(src: BuiltinEnum, args: BuiltinFnArgs, op: &BinaryOperator) -> WqResult<Value> {
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

fn fold_cmp_op(src: BuiltinEnum, args: &[Value], op: BinaryOperator) -> WqResult<Value> {
    if args.len() < 2 {
        return Err(WqError::new(WqErrorType::Arity)
            .src(src)
            .msg(format!("expected 2 or more args, got {}", args.len())));
    }
    let ops = vec![op; args.len() - 1];
    eval_cmp_chain(&ops, args).map_err(|e| e.src(src))
}

pub(super) fn op_add(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpAdd, args, &BinaryOperator::Add)
}

pub(super) fn op_sub(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
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

pub(super) fn op_mul(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpMul, args, &BinaryOperator::Multiply)
}

pub(super) fn op_div(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpDiv, args, &BinaryOperator::Divide)
}

pub(super) fn op_divdot(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpDivDot, args, &BinaryOperator::DivideDot)
}

pub(super) fn op_mod(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpMod, args, &BinaryOperator::Modulo)
}

pub(super) fn op_power(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpPower, args, &BinaryOperator::Power)
}

pub(super) fn op_powerdot(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpPowerDot, args, &BinaryOperator::PowerDot)
}

pub(super) fn op_matmul(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpMatmul, args, &BinaryOperator::Matmul)
}

pub(super) fn op_equal(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpEqual, &args, BinaryOperator::Equal)
}

pub(super) fn op_equaldot(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpEqualDot, &args, BinaryOperator::EqualDot)
}

pub(super) fn op_notequal(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpNotEqual, &args, BinaryOperator::NotEqual)
}

pub(super) fn op_notequaldot(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(
        BuiltinEnum::OpNotEqualDot,
        &args,
        BinaryOperator::NotEqualDot,
    )
}

pub(super) fn op_lt(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpLt, &args, BinaryOperator::Lt)
}

pub(super) fn op_lte(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpLte, &args, BinaryOperator::Lte)
}

pub(super) fn op_gt(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpGt, &args, BinaryOperator::Gt)
}

pub(super) fn op_gte(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_cmp_op(BuiltinEnum::OpGte, &args, BinaryOperator::Gte)
}

pub(super) fn op_cat(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
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

pub(super) fn op_booland(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpBoolAnd, args, &BinaryOperator::BoolAnd)
}

pub(super) fn op_boolor(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpBoolOr, args, &BinaryOperator::BoolOr)
}

pub(super) fn op_bitand(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpBitAnd, args, &BinaryOperator::BitAnd)
}

pub(super) fn op_bitor(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpBitOr, args, &BinaryOperator::BitOr)
}

pub(super) fn op_shl(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpShl, args, &BinaryOperator::Shl)
}

pub(super) fn op_shr(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpShr, args, &BinaryOperator::Shr)
}

pub(super) fn op_bitxor(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpBitXor, args, &BinaryOperator::BitXor)
}

pub(super) fn op_floordiv(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    fold_binary_op(BuiltinEnum::OpFloorDiv, args, &BinaryOperator::FloorDiv)
}

pub(super) fn op_sharp(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BuiltinEnum::OpSharp, [1], &args)?;
    eval_unary(&UnaryOperator::Count, &args[0]).map_err(|e| e.src(BuiltinEnum::OpSharp))
}
