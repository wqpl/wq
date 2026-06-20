use std::sync::Arc;

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::builtins::{
    BuiltinContext, BuiltinEnum as BE, BuiltinFnArgs, check_arity, check_arity_named, type_mismatch,
};
use crate::value::bc::{Bc1Stop, Bc2Stop};
use crate::value::cell::ValueCell;
use crate::value::func::CallableExpr;
use crate::value::{Value, WqResult, eval_binary, eval_unary};
use crate::vm::inst::{Instruction, Operand};
use crate::wqerror::{WqError, WqErrorType};

/// Tiny evaluator for callback bodies made only of args, constants, captured
/// reads, scalar ops, and indexing. Anything that can observe VM state falls
/// back to the normal call path.
#[derive(Clone)]
struct PureCallback {
    result: PureExpr,
}

#[derive(Clone)]
enum PureExpr {
    Arg(usize),
    Const(Value),
    Unary {
        op: UnaryOperator,
        operand: Box<PureExpr>,
    },
    Index {
        target: Box<PureExpr>,
        args: Box<[PureExpr]>,
    },
    Binary {
        op: BinaryOperator,
        left: Box<PureExpr>,
        right: Box<PureExpr>,
    },
}

impl PureCallback {
    fn from_func(func: &Value, arity: usize) -> Option<Self> {
        if let Value::LiftedCallable(data) = func {
            return Some(Self {
                result: PureExpr::from_callable_expr(&data.expr, arity)?,
            });
        }

        let shape = func.as_user_function()?;
        if shape.named_params.is_some() {
            return None;
        }
        match shape.params_len() {
            Some(expected) if expected != arity => return None,
            None if arity > 3 => return None,
            _ => {}
        }
        if usize::from(shape.locals) < arity {
            return None;
        }

        let (last, body) = shape.instructions.split_last()?;
        if !matches!(last, Instruction::Return) {
            return None;
        }

        let captures = shape.captured();
        let mut stack = Vec::new();
        for inst in body {
            match inst {
                Instruction::LoadConst(v) => stack.push(PureExpr::Const((**v).clone())),
                Instruction::LoadLocal(slot) => {
                    stack.push(Self::local_expr(*slot, arity)?);
                }
                Instruction::LoadCapture(slot) => {
                    stack.push(Self::capture_expr(&captures, *slot)?);
                }
                Instruction::UnaryOp(data) => {
                    let operand = Self::operand_expr(&mut stack, &data.operand, arity, &captures)?;
                    stack.push(PureExpr::Unary {
                        op: data.op,
                        operand: Box::new(operand),
                    });
                }
                Instruction::BinaryOp(data) => {
                    let right = Self::operand_expr(&mut stack, &data.right, arity, &captures)?;
                    let left = Self::operand_expr(&mut stack, &data.left, arity, &captures)?;
                    stack.push(PureExpr::Binary {
                        op: data.op,
                        left: Box::new(left),
                        right: Box::new(right),
                    });
                }
                Instruction::Index => {
                    let index = stack.pop()?;
                    let target = stack.pop()?;
                    stack.push(Self::index_expr(target, vec![index]));
                }
                Instruction::Postfix(argc) | Instruction::TailPostfix(argc) if *argc > 0 => {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = stack.pop()?;
                    stack.push(Self::index_expr(target, args));
                }
                Instruction::PostfixLocal(slot, argc)
                | Instruction::TailPostfixLocal(slot, argc)
                    if *argc > 0 =>
                {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = Self::local_expr(*slot, arity)?;
                    stack.push(Self::index_expr(target, args));
                }
                Instruction::PostfixCapture(slot, argc)
                | Instruction::TailPostfixCapture(slot, argc)
                    if *argc > 0 =>
                {
                    let args = Self::index_args(&mut stack, *argc)?;
                    let target = Self::capture_expr(&captures, *slot)?;
                    stack.push(Self::index_expr(target, args));
                }
                _ => return None,
            }
        }

        if stack.len() == 1 {
            Some(Self {
                result: stack.pop()?,
            })
        } else {
            None
        }
    }

    fn local_expr(slot: u16, arity: usize) -> Option<PureExpr> {
        let slot = usize::from(slot);
        if slot < arity {
            Some(PureExpr::Arg(slot))
        } else {
            None
        }
    }

    fn capture_expr(captures: &[ValueCell], slot: u16) -> Option<PureExpr> {
        let cell = captures.get(usize::from(slot))?;
        Some(PureExpr::Const(
            cell.lock().expect("poisoned capture").clone(),
        ))
    }

    fn index_expr(target: PureExpr, args: Vec<PureExpr>) -> PureExpr {
        PureExpr::Index {
            target: Box::new(target),
            args: args.into_boxed_slice(),
        }
    }

    fn index_args(stack: &mut Vec<PureExpr>, argc: usize) -> Option<Vec<PureExpr>> {
        let base = stack.len().checked_sub(argc)?;
        Some(stack.drain(base..).collect())
    }

    fn operand_expr(
        stack: &mut Vec<PureExpr>,
        operand: &Operand,
        arity: usize,
        captures: &[ValueCell],
    ) -> Option<PureExpr> {
        match operand {
            Operand::Stack => stack.pop(),
            Operand::Const(v) => Some(PureExpr::Const((**v).clone())),
            Operand::Local(slot) => Self::local_expr(*slot, arity),
            Operand::Capture(slot) => Self::capture_expr(captures, *slot),
            Operand::Var(_) | Operand::Self_ => None,
        }
    }

    fn eval(&self, args: &[&Value]) -> WqResult<Option<Value>> {
        self.result.eval(args)
    }
}

impl PureExpr {
    fn from_callable_expr(expr: &CallableExpr, arity: usize) -> Option<Self> {
        match expr {
            CallableExpr::Const(value) => Some(Self::Const(value.clone())),
            CallableExpr::Call(value) => {
                PureCallback::from_func(value, arity).map(|callback| callback.result)
            }
            CallableExpr::Unary { op, operand } => Some(Self::Unary {
                op: *op,
                operand: Box::new(Self::from_callable_expr(operand, arity)?),
            }),
            CallableExpr::Binary { op, left, right } => Some(Self::Binary {
                op: *op,
                left: Box::new(Self::from_callable_expr(left, arity)?),
                right: Box::new(Self::from_callable_expr(right, arity)?),
            }),
        }
    }

    fn eval(&self, args: &[&Value]) -> WqResult<Option<Value>> {
        match self {
            Self::Arg(slot) => {
                let arg = args.get(*slot).ok_or_else(|| {
                    WqError::new(WqErrorType::Vm).msg("pure callback argument missing")
                })?;
                Ok(Some((**arg).clone()))
            }
            Self::Const(v) => Ok(Some(v.clone())),
            Self::Unary { op, operand } => {
                let Some(value) = operand.eval(args)? else {
                    return Ok(None);
                };
                eval_unary(op, &value).map(Some)
            }
            Self::Index {
                target,
                args: index_args,
            } => {
                let Some(target) = target.eval(args)? else {
                    return Ok(None);
                };
                if target.is_callable() {
                    return Ok(None);
                }
                let mut resolved_args = Vec::with_capacity(index_args.len());
                for index_arg in index_args {
                    let Some(value) = index_arg.eval(args)? else {
                        return Ok(None);
                    };
                    resolved_args.push(value);
                }
                let result = if resolved_args.len() == 1 {
                    target.index(
                        resolved_args
                            .first()
                            .expect("single index argument should exist"),
                    )
                } else {
                    target.index_many(&resolved_args)
                };
                match result {
                    Some(value) => Ok(Some(value)),
                    None => Err(pure_index_err(&target, &resolved_args)),
                }
            }
            Self::Binary { op, left, right } => {
                let Some(left) = left.eval(args)? else {
                    return Ok(None);
                };
                let Some(right) = right.eval(args)? else {
                    return Ok(None);
                };
                eval_binary(op, &left, &right).map(Some)
            }
        }
    }
}

fn pure_index_err(target: &Value, args: &[Value]) -> WqError {
    let index = if args.len() == 1 {
        args.first()
            .expect("single index argument should exist")
            .clone()
    } else {
        Value::from_items(args.to_vec())
    };
    WqError::new(WqErrorType::Index)
        .src("pure callback")
        .msg(format!("invalid index '{index}'"))
        .got1(target)
}

#[inline]
fn call_pure_or_vm1(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    pure: Option<&PureCallback>,
    arg: &Value,
) -> WqResult<Value> {
    if let Some(pure) = pure
        && let Some(value) = pure.eval(&[arg])?
    {
        return Ok(value);
    }
    vm.call(func, BuiltinFnArgs::from(arg.clone()))
}

#[inline]
fn call_pure_or_vm2(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    pure: Option<&PureCallback>,
    left: &Value,
    right: &Value,
) -> WqResult<Value> {
    if let Some(pure) = pure
        && let Some(value) = pure.eval(&[left, right])?
    {
        return Ok(value);
    }
    let mut ca = BuiltinFnArgs::new();
    ca.push(left.clone());
    ca.push(right.clone());
    vm.call(func, ca)
}

fn filter_predicate(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    pure: Option<&PureCallback>,
    value: &Value,
) -> WqResult<bool> {
    match call_pure_or_vm1(vm, func, pure, value)? {
        Value::Bool(b) => Ok(b),
        _ => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Filter)
            .msg("predicate must return bool")),
    }
}

/// apply[fs;x] — apply each function in fs to x, returning a list of results.
/// If fs is a single function (not a list), returns f[x] unwrapped.
pub(super) fn apply(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Apply, [2, 2], &args)?;
    let (fs, x) = (&args[0], &args[1]);
    match fs {
        Value::List(items) => {
            let mut results = Vec::with_capacity(items.len());
            for f in items.iter() {
                results.push(vm.call(f, BuiltinFnArgs::from(x.clone()))?);
            }
            Ok(Value::from_items(results))
        }
        _ => vm.call(fs, BuiltinFnArgs::from(x.clone())),
    }
}

/// map[xs;f;d?]
pub(super) fn map(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    #[inline]
    fn eff_layers(raw_d: &Value, total_depth: i64) -> Option<i64> {
        match raw_d {
            // non-negative: go min(d, D) layers
            Value::Int(n) if *n >= 0 => Some((*n).min(total_depth)),
            // negative: "cut |d| from xs.depth()" -> L = max(0, D + d)
            Value::Int(n) => Some((total_depth + *n).max(0)),
            // +inf: go fully (atoms only) -> L = D
            Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(total_depth),
            // -inf: apply at root -> L = 0
            Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
            _ => None,
        }
    }

    fn _map(vm: &mut dyn BuiltinContext, xs: &Value, f: &Value, d: &Value) -> WqResult<Value> {
        let el = match eff_layers(d, xs.depth()) {
            Some(l) => l,
            None => return Err(type_mismatch(BE::Map, 0, "int, inf or -inf", d)),
        };
        // atoms are always leaves; stop after traversing L layers from the root
        let stop = Bc1Stop::AtomOrDepth(el);
        let pure = PureCallback::from_func(f, 1);
        let op1 = |v: &Value| call_pure_or_vm1(vm, f, pure.as_ref(), v);
        xs.bc1_until(stop, op1)
            .map_err(|e| e.into_wqerror().src(BE::Map))
    }

    check_arity(BE::Map, [2, 3], &args)?;
    match args.len() {
        2 => {
            let (xs, f) = (&args[0], &args[1]);
            _map(vm, xs, f, &Value::Int(1))
        }
        3 => {
            let (xs, f, d) = (&args[0], &args[1], &args[2]);
            _map(vm, xs, f, d)
        }
        _ => unreachable!(),
    }
}

#[inline]
fn eff_layers(raw_d: &Value, total_depth: i64) -> Option<i64> {
    match raw_d {
        Value::Int(n) if *n >= 0 => Some((*n).min(total_depth)),
        Value::Int(n) => Some((total_depth + *n).max(0)),
        Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(total_depth),
        Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
        _ => None,
    }
}

fn any_all_at_depth(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    xs: &Value,
    depth_from_root: i64,
    max_depth: i64,
    mode_any: bool,
    src: BE,
) -> WqResult<bool> {
    if depth_from_root >= max_depth || xs.is_atom() {
        let pred = vm.call(func, BuiltinFnArgs::from(xs.clone()))?;
        return match pred {
            Value::Bool(b) => Ok(b),
            _ => Err(WqError::new(WqErrorType::Domain)
                .src(src)
                .msg("predicate must return bool")),
        };
    }

    match xs {
        Value::List(items) => {
            for item in items.iter() {
                let result = any_all_at_depth(
                    vm,
                    func,
                    item,
                    depth_from_root + 1,
                    max_depth,
                    mode_any,
                    src,
                )?;
                if mode_any && result {
                    return Ok(true);
                }
                if !mode_any && !result {
                    return Ok(false);
                }
            }
            Ok(!mode_any)
        }
        Value::IntList(items) => {
            for &item in items.iter() {
                let pred = vm.call(func, BuiltinFnArgs::from(Value::Int(item)))?;
                let result = match pred {
                    Value::Bool(b) => b,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(src)
                            .msg("predicate must return bool"));
                    }
                };
                if mode_any && result {
                    return Ok(true);
                }
                if !mode_any && !result {
                    return Ok(false);
                }
            }
            Ok(!mode_any)
        }
        Value::Dict(map) => {
            for item in map.values() {
                let result = any_all_at_depth(
                    vm,
                    func,
                    item,
                    depth_from_root + 1,
                    max_depth,
                    mode_any,
                    src,
                )?;
                if mode_any && result {
                    return Ok(true);
                }
                if !mode_any && !result {
                    return Ok(false);
                }
            }
            Ok(!mode_any)
        }
        other => {
            let pred = vm.call(func, BuiltinFnArgs::from(other.clone()))?;
            match pred {
                Value::Bool(b) => Ok(b),
                _ => Err(WqError::new(WqErrorType::Domain)
                    .src(src)
                    .msg("predicate must return bool")),
            }
        }
    }
}

/// any[xs;f;d?]
pub(super) fn any(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Any, [2, 3], &args)?;
    let (xs, f, d) = match args.len() {
        2 => (&args[0], &args[1], &Value::Int(1)),
        3 => (&args[0], &args[1], &args[2]),
        _ => unreachable!(),
    };
    let max_depth = match eff_layers(d, xs.depth()) {
        Some(l) => l,
        None => return Err(type_mismatch(BE::Any, 0, "int, inf or -inf", d)),
    };
    Ok(Value::Bool(any_all_at_depth(
        vm,
        f,
        xs,
        0,
        max_depth,
        true,
        BE::Any,
    )?))
}

/// all[xs;f;d?]
pub(super) fn all(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::All, [2, 3], &args)?;
    let (xs, f, d) = match args.len() {
        2 => (&args[0], &args[1], &Value::Int(1)),
        3 => (&args[0], &args[1], &args[2]),
        _ => unreachable!(),
    };
    let max_depth = match eff_layers(d, xs.depth()) {
        Some(l) => l,
        None => return Err(type_mismatch(BE::All, 0, "int, inf or -inf", d)),
    };
    Ok(Value::Bool(any_all_at_depth(
        vm,
        f,
        xs,
        0,
        max_depth,
        false,
        BE::All,
    )?))
}

/// fold[xs;f;acc?]
pub(super) fn fold(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Fold, [2, 3], &args)?;
    let n = args.len();
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let f = iter.next().unwrap();

    match n {
        2 => match xs {
            Value::IntList(items) => {
                if items.is_empty() {
                    return Ok(Value::unit());
                }
                let mut acc = Value::Int(items[0]);
                for &x in &items[1..] {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(Value::Int(x));
                    acc = vm.call(&f, ca)?;
                }
                Ok(acc)
            }
            Value::List(items) => {
                if items.is_empty() {
                    return Ok(Value::unit());
                }
                let mut list_iter = items.iter();
                let mut acc = list_iter.next().unwrap().clone();
                for it in list_iter {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(it.clone());
                    acc = vm.call(&f, ca)?;
                }
                Ok(acc)
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    return Ok(Value::unit());
                }
                let mut val_iter = map.values();
                let mut acc = val_iter.next().unwrap().clone();
                for it in val_iter {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(it.clone());
                    acc = vm.call(&f, ca)?;
                }
                Ok(acc)
            }
            other => Ok(other),
        },
        3 => {
            let mut acc = iter.next().unwrap();
            match xs {
                Value::IntList(items) => {
                    for &x in items.iter() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(Value::Int(x));
                        acc = vm.call(&f, ca)?;
                    }
                    Ok(acc)
                }
                Value::List(items) => {
                    for it in items.iter() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(it.clone());
                        acc = vm.call(&f, ca)?;
                    }
                    Ok(acc)
                }
                Value::Dict(map) => {
                    for it in map.values() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(it.clone());
                        acc = vm.call(&f, ca)?;
                    }
                    Ok(acc)
                }
                other => Ok(other),
            }
        }
        _ => unreachable!(),
    }
}

/// scan[xs;f;acc?]
pub(super) fn scan(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Scan, [2, 3], &args)?;
    let n = args.len();
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let f = iter.next().unwrap();

    match n {
        2 => match xs {
            Value::IntList(xs) => {
                if xs.is_empty() {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                let mut acc = Value::Int(xs[0]);
                results.push(acc.clone());
                for &x in &xs[1..] {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(Value::Int(x));
                    acc = vm.call(&f, ca)?;
                    results.push(acc.clone());
                }
                Ok(Value::from_items(results))
            }
            Value::List(xs) => {
                if xs.is_empty() {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                let mut acc = xs[0].clone();
                results.push(acc.clone());
                for x in &xs[1..] {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(x.clone());
                    acc = vm.call(&f, ca)?;
                    results.push(acc.clone());
                }
                Ok(Value::from_items(results))
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(map.len());
                let mut val_iter = map.values();
                let mut acc = val_iter.next().unwrap().clone();
                results.push(acc.clone());
                for v in val_iter {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(v.clone());
                    acc = vm.call(&f, ca)?;
                    results.push(acc.clone());
                }
                Ok(Value::from_items(results))
            }
            other => Ok(other),
        },
        3 => {
            let mut acc = iter.next().unwrap();
            match xs {
                Value::IntList(xs) => {
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    for &x in xs.iter() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(Value::Int(x));
                        acc = vm.call(&f, ca)?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                Value::List(xs) => {
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    for x in xs.iter() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(x.clone());
                        acc = vm.call(&f, ca)?;
                        results.push(acc.clone());
                    }
                    Ok(Value::List(Arc::new(results)))
                }
                Value::Dict(map) => {
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    for v in map.values() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(v.clone());
                        acc = vm.call(&f, ca)?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                other => Ok(other),
            }
        }
        _ => unreachable!(),
    }
}

/// rscan[xs;f;acc?]
pub(super) fn rscan(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::RScan, [2, 3], &args)?;
    let n = args.len();
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let f = iter.next().unwrap();

    match n {
        2 => match xs {
            Value::IntList(xs) => {
                if xs.is_empty() {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                let mut acc = Value::Int(*xs.last().unwrap());
                results.push(acc.clone());
                for &x in xs.iter().rev().skip(1) {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(Value::Int(x));
                    acc = vm.call(&f, ca)?;
                    results.push(acc.clone());
                }
                results.reverse();
                Ok(Value::from_items(results))
            }
            Value::List(xs) => {
                if xs.is_empty() {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                let mut acc = xs.last().unwrap().clone();
                results.push(acc.clone());
                for x in xs.iter().rev().skip(1) {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(x.clone());
                    acc = vm.call(&f, ca)?;
                    results.push(acc.clone());
                }
                results.reverse();
                Ok(Value::from_items(results))
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(map.len());
                let mut val_iter = map.values().rev();
                let mut acc = val_iter.next().unwrap().clone();
                results.push(acc.clone());
                for v in val_iter {
                    let mut ca = BuiltinFnArgs::new();
                    ca.push(acc);
                    ca.push(v.clone());
                    acc = vm.call(&f, ca)?;
                    results.push(acc.clone());
                }
                results.reverse();
                Ok(Value::from_items(results))
            }
            other => Ok(other),
        },
        3 => {
            let mut acc = iter.next().unwrap();
            match xs {
                Value::IntList(xs) => {
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    for &x in xs.iter().rev() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(Value::Int(x));
                        acc = vm.call(&f, ca)?;
                        results.push(acc.clone());
                    }
                    results.reverse();
                    Ok(Value::from_items(results))
                }
                Value::List(xs) => {
                    let mut results: Vec<Value> = Vec::with_capacity(xs.len());
                    for x in xs.iter().rev() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(x.clone());
                        acc = vm.call(&f, ca)?;
                        results.push(acc.clone());
                    }
                    results.reverse();
                    Ok(Value::List(Arc::new(results)))
                }
                Value::Dict(map) => {
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    for v in map.values().rev() {
                        let mut ca = BuiltinFnArgs::new();
                        ca.push(acc);
                        ca.push(v.clone());
                        acc = vm.call(&f, ca)?;
                        results.push(acc.clone());
                    }
                    results.reverse();
                    Ok(Value::from_items(results))
                }
                other => Ok(other),
            }
        }
        _ => unreachable!(),
    }
}

/// filter[xs;f]
pub(super) fn filter(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Filter, [2], &args)?;
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let func = iter.next().unwrap();
    let pure = PureCallback::from_func(&func, 1);
    match xs {
        Value::IntList(items) => {
            let mut result = Vec::new();
            for &item in items.iter() {
                let val = Value::Int(item);
                if filter_predicate(vm, &func, pure.as_ref(), &val)? {
                    result.push(val);
                }
            }
            Ok(Value::from_items(result))
        }
        Value::List(items) => {
            let mut result = Vec::new();
            for item in items.iter() {
                if filter_predicate(vm, &func, pure.as_ref(), item)? {
                    result.push(item.clone());
                }
            }
            Ok(Value::from_items(result))
        }
        Value::Dict(map) => {
            let mut result = Vec::new();
            for value in map.values() {
                if filter_predicate(vm, &func, pure.as_ref(), value)? {
                    result.push(value.clone());
                }
            }
            Ok(Value::from_items(result))
        }
        other => Ok(other),
    }
}

/// zipw[xs;ys;f;d?]
pub(super) fn zipw(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    #[inline]
    fn eff_layers_2(raw_d: &Value, dx: i64, dy: i64) -> Option<i64> {
        let dmax = dx.max(dy);
        match raw_d {
            // non-negative: go min(d, Dmax) layers
            Value::Int(n) if *n >= 0 => Some((*n).min(dmax)),
            // negative: cut |d| from max depth, L = max(0, Dmax + d)
            Value::Int(n) => Some((dmax + *n).max(0)),
            // +inf: go fully
            Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(dmax),
            // -inf: treat as atom (apply at root)
            Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
            _ => None,
        }
    }

    fn _zipw(
        vm: &mut dyn BuiltinContext,
        xs: &Value,
        ys: &Value,
        f: &Value,
        d: &Value,
    ) -> WqResult<Value> {
        let el = match eff_layers_2(d, xs.depth(), ys.depth()) {
            Some(l) => l,
            None => return Err(type_mismatch(BE::ZipW, 0, "int, inf or -inf", d)),
        };
        // atoms are always leaves; stop after traversing L layers from the root
        let stop = Bc2Stop::BothAtomOrDepth(el);
        let pure = PureCallback::from_func(f, 2);
        let op2 = |a: &Value, b: &Value| call_pure_or_vm2(vm, f, pure.as_ref(), a, b);
        xs.bc2_until(ys, stop, op2)
            .map_err(|e| e.into_wqerror().src(BE::ZipW))
    }

    check_arity(BE::ZipW, [3, 4], &args)?;
    match args.len() {
        3 => {
            let (xs, ys, f) = (&args[0], &args[1], &args[2]);
            _zipw(vm, xs, ys, f, &Value::Int(1))
        }
        4 => {
            let (xs, ys, f, d) = (&args[0], &args[1], &args[2], &args[3]);
            _zipw(vm, xs, ys, f, d)
        }
        _ => unreachable!(),
    }
}

///splitw[xs;f;`m]
pub(super) fn splitw(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    const MAXSPLIT_ARG: &str = "m";
    check_arity_named(BE::SplitW, [2], &args, &[MAXSPLIT_ARG])?;
    let maxsplit = crate::builtins::list::parse_maxsplit(args.named(MAXSPLIT_ARG), BE::SplitW)?;
    let mut iter = args.into_iter();
    let val = iter.next().unwrap();
    let func = iter.next().unwrap();
    let limit = maxsplit.unwrap_or(usize::MAX);
    let mut splits_done = 0;

    // Direct String handling — avoid List<Char> allocation.
    if let Value::String(s) = &val {
        let mut chunks = Vec::new();
        let mut current = String::new();
        for c in s.chars() {
            let ch_val = Value::Char(c);
            let pred = vm.call(&func, BuiltinFnArgs::from(ch_val))?;
            match pred.try_to_rust_bool() {
                Some(true) if splits_done < limit => {
                    chunks.push(current);
                    current = String::new();
                    splits_done += 1;
                }
                Some(true) => current.push(c),
                Some(false) => current.push(c),
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::SplitW)
                        .msg("predicate must return bool"));
                }
            }
        }
        chunks.push(current);
        return Ok(Value::value_from_str_chunks(chunks));
    }

    // Normalize String to List<Char> for uniform handling
    match &val {
        l @ Value::List(items) if l.is_string_like() => {
            let mut chunks = Vec::new();
            let mut current = String::new();
            for item in items.iter() {
                let pred = vm.call(&func, BuiltinFnArgs::from(item.clone()))?;
                match pred.try_to_rust_bool() {
                    Some(true) if splits_done < limit => {
                        chunks.push(current);
                        current = String::new();
                        splits_done += 1;
                    }
                    Some(true) => {
                        let Value::Char(ch) = item else {
                            unreachable!()
                        };
                        current.push(*ch);
                    }
                    Some(false) => {
                        let Value::Char(ch) = item else {
                            unreachable!()
                        };
                        current.push(*ch);
                    }
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::SplitW)
                            .msg("predicate must return bool"));
                    }
                }
            }
            chunks.push(current);
            Ok(Value::value_from_str_chunks(chunks))
        }
        Value::IntList(items) => {
            let mut chunks = Vec::new();
            let mut current = Vec::new();
            for &item in items.iter() {
                let value = Value::Int(item);
                let pred = vm.call(&func, BuiltinFnArgs::from(value))?;
                match pred.try_to_rust_bool() {
                    Some(true) if splits_done < limit => {
                        chunks.push(Value::IntList(Arc::new(std::mem::take(&mut current))));
                        splits_done += 1;
                    }
                    Some(true) => current.push(item),
                    Some(false) => current.push(item),
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::SplitW)
                            .msg("predicate must return bool"));
                    }
                }
            }
            chunks.push(Value::IntList(Arc::new(current)));
            Ok(Value::List(Arc::new(chunks)))
        }
        Value::List(items) => {
            let mut chunks = Vec::new();
            let mut current = Vec::new();
            for item in items.iter() {
                let pred = vm.call(&func, BuiltinFnArgs::from(item.clone()))?;
                match pred.try_to_rust_bool() {
                    Some(true) if splits_done < limit => {
                        chunks.push(Value::List(Arc::new(std::mem::take(&mut current))));
                        splits_done += 1;
                    }
                    Some(true) => current.push(item.clone()),
                    Some(false) => current.push(item.clone()),
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::SplitW)
                            .msg("predicate must return bool"));
                    }
                }
            }
            chunks.push(Value::List(Arc::new(current)));
            Ok(Value::List(Arc::new(chunks)))
        }
        other => Err(WqError::new(WqErrorType::Domain)
            .src(BE::SplitW)
            .msg("expected string or list")
            .at_arg(1)
            .got1(other)),
    }
}

struct FindWithCtx<'a> {
    func: &'a Value,
    max_depth: i64,
    threshold: i64,
    reverse: bool,
    src: BE,
}

fn findwith_search(
    vm: &mut dyn BuiltinContext,
    xs: &Value,
    current_depth: i64,
    results: &mut Vec<Value>,
    path: &mut Vec<i64>,
    ctx: &FindWithCtx<'_>,
) -> WqResult<()> {
    if results.len() >= ctx.threshold as usize {
        return Ok(());
    }

    let is_match = |vm: &mut dyn BuiltinContext, item: &Value| -> WqResult<bool> {
        let pred = vm.call(ctx.func, BuiltinFnArgs::from(item.clone()))?;
        match pred {
            Value::Bool(b) => Ok(b),
            _ => Err(WqError::new(WqErrorType::Domain)
                .src(ctx.src)
                .msg("predicate must return bool")),
        }
    };

    match xs {
        Value::List(items) => {
            let indices: Vec<usize> = if ctx.reverse {
                (0..items.len()).rev().collect()
            } else {
                (0..items.len()).collect()
            };
            for idx in indices {
                if results.len() >= ctx.threshold as usize {
                    return Ok(());
                }
                let item = &items[idx];
                if is_match(vm, item)? {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if results.len() >= ctx.threshold as usize {
                        return Ok(());
                    }
                } else if current_depth < ctx.max_depth {
                    path.push(idx as i64);
                    findwith_search(vm, item, current_depth + 1, results, path, ctx)?;
                    path.pop();
                }
            }
        }
        Value::IntList(items) => {
            let indices: Vec<usize> = if ctx.reverse {
                (0..items.len()).rev().collect()
            } else {
                (0..items.len()).collect()
            };
            for idx in indices {
                if results.len() >= ctx.threshold as usize {
                    return Ok(());
                }
                let item_val = Value::Int(items[idx]);
                if is_match(vm, &item_val)? {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if results.len() >= ctx.threshold as usize {
                        return Ok(());
                    }
                }
            }
        }
        Value::Dict(map) => {
            let values: Vec<_> = map.values().collect();
            let indices: Vec<usize> = if ctx.reverse {
                (0..values.len()).rev().collect()
            } else {
                (0..values.len()).collect()
            };
            for idx in indices {
                if results.len() >= ctx.threshold as usize {
                    return Ok(());
                }
                let item = values[idx];
                if is_match(vm, item)? {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if results.len() >= ctx.threshold as usize {
                        return Ok(());
                    }
                } else if current_depth < ctx.max_depth {
                    path.push(idx as i64);
                    findwith_search(vm, item, current_depth + 1, results, path, ctx)?;
                    path.pop();
                }
            }
        }
        _ => {
            if is_match(vm, xs)? {
                results.push(Value::IntList(Arc::new(path.clone())));
            }
        }
    }
    Ok(())
}

/// findw[xs;f;threshold?;d?]
pub(super) fn findw(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::FindW, [2, 3, 4], &args)?;
    let n = args.len();
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let func = iter.next().unwrap();

    let (threshold, depth) = match n {
        2 => (1i64, 1i64),
        3 => {
            let threshold = match &iter.next().unwrap() {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::FindW)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            (threshold, 1)
        }
        4 => {
            let thresh_val = iter.next().unwrap();
            let depth_val = iter.next().unwrap();
            let threshold = match &thresh_val {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::FindW)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            let depth = match &depth_val {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::FindW)
                        .msg("depth must be non-negative int or inf")
                        .at_arg(3));
                }
            };
            (threshold, depth)
        }
        _ => unreachable!(),
    };

    let mut results = Vec::new();
    let mut path = Vec::new();
    let ctx = FindWithCtx {
        func: &func,
        max_depth: depth,
        threshold,
        reverse: false,
        src: BE::FindW,
    };
    findwith_search(vm, &xs, 0, &mut results, &mut path, &ctx)?;
    if results.is_empty() {
        Ok(Value::unit())
    } else if results.len() == 1 {
        Ok(results.into_iter().next().unwrap())
    } else {
        Ok(Value::List(Arc::new(results)))
    }
}

/// rfindw[xs;f;threshold?;d?]
pub(super) fn rfindw(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::RFindW, [2, 3, 4], &args)?;
    let n = args.len();
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let func = iter.next().unwrap();

    let (threshold, depth) = match n {
        2 => (1i64, 1i64),
        3 => {
            let threshold = match &iter.next().unwrap() {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::RFindW)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            (threshold, 1)
        }
        4 => {
            let thresh_val = iter.next().unwrap();
            let depth_val = iter.next().unwrap();
            let threshold = match &thresh_val {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::RFindW)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            let depth = match &depth_val {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::RFindW)
                        .msg("depth must be non-negative int or inf")
                        .at_arg(3));
                }
            };
            (threshold, depth)
        }
        _ => unreachable!(),
    };

    let mut results = Vec::new();
    let mut path = Vec::new();
    let ctx = FindWithCtx {
        func: &func,
        max_depth: depth,
        threshold,
        reverse: true,
        src: BE::RFindW,
    };
    findwith_search(vm, &xs, 0, &mut results, &mut path, &ctx)?;
    if results.is_empty() {
        Ok(Value::unit())
    } else if results.len() == 1 {
        Ok(results.into_iter().next().unwrap())
    } else {
        Ok(Value::List(Arc::new(results)))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use smallvec::smallvec;

    use super::*;
    use crate::value::func::{ClosureData, FunctionData};
    use crate::vm::Vm;
    use crate::vm::inst::{Instruction, Operand};

    fn make_fn(params: Option<&[&str]>, locals: u16, instructions: Vec<Instruction>) -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: params.map(|names| {
                Arc::<[String]>::from(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            }),
            named_params: None,
            locals,
            instructions: instructions.into(),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    fn make_closure(
        params: Option<&[&str]>,
        locals: u16,
        captures: Vec<Value>,
        instructions: Vec<Instruction>,
    ) -> Value {
        Value::Closure(Arc::new(ClosureData {
            params: params.map(|names| {
                Arc::<[String]>::from(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            }),
            named_params: None,
            locals,
            captured: Arc::from(
                captures
                    .into_iter()
                    .map(|value| Arc::new(Mutex::new(value)))
                    .collect::<Vec<_>>(),
            ),
            instructions: instructions.into(),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    #[test]
    fn map_pure_fast_path_correctness() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        // map[1..4;{x+1}] should use the pure fast-path and still return (2;3;4)
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let f = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(1)),
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Stack,
                    Operand::Stack,
                ),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![2, 3, 4])));
    }

    #[test]
    fn map_pure_fast_path_embedded_operands() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let f = make_fn(
            None,
            3,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(1))),
                ),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![2, 3, 4])));
    }

    #[test]
    fn map_pure_fast_path_accepts_captured_operands() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let f = make_closure(
            Some(&["x"]),
            1,
            vec![Value::Int(10)],
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Capture(0),
                ),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![11, 12, 13])));
    }

    #[test]
    fn map_pure_fast_path_accepts_captured_indexing() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::IntList(Arc::new((0..9).collect()));
        let grid = Value::List(Arc::new(vec![
            Value::IntList(Arc::new(vec![1, 2, 3])),
            Value::IntList(Arc::new(vec![4, 5, 6])),
            Value::IntList(Arc::new(vec![7, 8, 9])),
        ]));
        let f = make_closure(
            Some(&["x"]),
            1,
            vec![grid, Value::Int(0), Value::Int(0)],
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::FloorDiv,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(3))),
                ),
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Stack,
                    Operand::Capture(1),
                ),
                Instruction::PostfixCapture(0, 1),
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Modulo,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(3))),
                ),
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Stack,
                    Operand::Capture(2),
                ),
                Instruction::TailPostfix(1),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(
            result,
            Value::IntList(Arc::new(vec![1, 2, 3, 4, 5, 6, 7, 8, 9]))
        );
    }

    #[test]
    fn map_pure_fast_path_accepts_multi_arg_local_indexing() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::List(Arc::new(vec![
            Value::IntList(Arc::new(vec![1, 2, 3])),
            Value::IntList(Arc::new(vec![4, 5, 6])),
        ]));
        let f = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::load_const(Value::Int(2)),
                Instruction::load_const(Value::Int(0)),
                Instruction::TailPostfixLocal(0, 2),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![3, 1])),
                Value::IntList(Arc::new(vec![6, 4])),
            ]))
        );
    }

    #[test]
    fn map_pure_fast_path_accepts_multi_arg_stack_indexing() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::List(Arc::new(vec![
            Value::IntList(Arc::new(vec![1, 2, 3])),
            Value::IntList(Arc::new(vec![4, 5, 6])),
        ]));
        let f = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(1)),
                Instruction::load_const(Value::Int(0)),
                Instruction::TailPostfix(2),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![2, 1])),
                Value::IntList(Arc::new(vec![5, 4])),
            ]))
        );
    }

    #[test]
    fn map_pure_fast_path_falls_back_for_callable_postfix_target() {
        let mut vm = Vm::new(vec![]);
        let inc = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(1))),
                ),
                Instruction::Return,
            ],
        );
        let xs = Value::List(Arc::new(vec![inc]));
        let f = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(1)),
                Instruction::TailPostfix(1),
                Instruction::Return,
            ],
        );
        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![2])));
    }

    #[test]
    fn filter_pure_fast_path_correctness() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        // filter[1..5;{x>2}] should use the pure fast-path and still return (3;4)
        let xs = Value::IntList(Arc::new(vec![1, 2, 3, 4]));
        let f = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Gt,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(2))),
                ),
                Instruction::Return,
            ],
        );
        let result =
            filter(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("filter succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![3, 4])));
    }

    #[test]
    fn zipw_pure_fast_path_correctness() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        // zipw[(1;2;3);(4;5;6);{x+y}] should use the pure fast-path
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let ys = Value::IntList(Arc::new(vec![4, 5, 6]));
        let f = make_fn(
            Some(&["x", "y"]),
            2,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Local(1),
                ),
                Instruction::Return,
            ],
        );
        let result =
            zipw(&mut vm, BuiltinFnArgs::from(smallvec![xs, ys, f])).expect("zipw succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![5, 7, 9])));
    }

    #[test]
    fn map_pure_fast_path_accepts_callable_expr() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let inc = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(1))),
                ),
                Instruction::Return,
            ],
        );
        let double = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Multiply,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(2))),
                ),
                Instruction::Return,
            ],
        );
        let f = Value::function_composition(crate::astnode::BinaryOperator::Add, inc, double);

        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![4, 7, 10])));
    }

    #[test]
    fn map_pure_fast_path_accepts_unary_callable_expr() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let inc = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(1))),
                ),
                Instruction::Return,
            ],
        );
        let f = Value::unary_function_composition(crate::astnode::UnaryOperator::Negate, inc);

        let result = map(&mut vm, BuiltinFnArgs::from(smallvec![xs, f])).expect("map succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![-2, -3, -4])));
    }

    #[test]
    fn zipw_pure_fast_path_accepts_callable_expr() {
        let mut vm = Vm::new(vec![]);
        vm.max_call_depth = 0;
        let xs = Value::IntList(Arc::new(vec![1, 2, 3]));
        let ys = Value::IntList(Arc::new(vec![4, 5, 6]));
        let add = make_fn(
            Some(&["x", "y"]),
            2,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Local(1),
                ),
                Instruction::Return,
            ],
        );
        let multiply = make_fn(
            Some(&["x", "y"]),
            2,
            vec![
                Instruction::binary_op(
                    crate::astnode::BinaryOperator::Multiply,
                    Operand::Local(0),
                    Operand::Local(1),
                ),
                Instruction::Return,
            ],
        );
        let f = Value::function_composition(crate::astnode::BinaryOperator::Add, add, multiply);

        let result =
            zipw(&mut vm, BuiltinFnArgs::from(smallvec![xs, ys, f])).expect("zipw succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![9, 17, 27])));
    }
}
