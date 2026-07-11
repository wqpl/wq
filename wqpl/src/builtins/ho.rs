use std::sync::Arc;

use crate::builtins::{
    BuiltinContext, BuiltinEnum as BE, BuiltinFnArgs, check_arity, check_arity_named, type_mismatch,
};
use crate::value::bc::{Bc1Stop, Bc2Stop};
use crate::value::seq::ValueSeq;
use crate::value::{Value, WqResult};
use crate::vm::pure::PureCallback;
use crate::wqerror::{WqError, WqErrorType};

fn pure_callback(vm: &dyn BuiltinContext, func: &Value, arity: usize) -> Option<PureCallback> {
    if vm.requires_callback_frames() {
        None
    } else {
        PureCallback::compile(func, arity)
    }
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

fn call_fold_func(
    vm: &mut dyn BuiltinContext,
    func: &Value,
    acc: Value,
    item: Value,
) -> WqResult<Value> {
    let mut ca = BuiltinFnArgs::new();
    ca.push(acc);
    ca.push(item);
    vm.call(func, ca)
}

fn predicate_result(src: BE, pred: Value) -> WqResult<bool> {
    match pred {
        Value::Bool(b) => Ok(b),
        _ => Err(WqError::new(WqErrorType::Domain)
            .src(src)
            .msg("predicate must return bool")),
    }
}

/// apply[fs;x]
/// apply each function in fs to x, returning a framed list of results.
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
        _ => Ok(Value::from_items(vec![
            vm.call(fs, BuiltinFnArgs::from(x.clone()))?,
        ])),
    }
}

pub(super) fn apply_discard(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Apply, [2, 2], &args)?;
    let (fs, x) = (&args[0], &args[1]);
    match fs {
        Value::List(items) => {
            for f in items.iter() {
                vm.call(f, BuiltinFnArgs::from(x.clone()))?;
            }
        }
        _ => {
            vm.call(fs, BuiltinFnArgs::from(x.clone()))?;
        }
    }
    Ok(Value::unit())
}

/// map[xs;f;d?]
pub(super) fn map(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Map, [2, 3], &args)?;
    match args.len() {
        2 => {
            let (xs, f) = (&args[0], &args[1]);
            map_impl(vm, xs, f, &Value::Int(1))
        }
        3 => {
            let (xs, f, d) = (&args[0], &args[1], &args[2]);
            map_impl(vm, xs, f, d)
        }
        _ => unreachable!(),
    }
}

pub(super) fn map_discard(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Map, [2, 3], &args)?;
    match args.len() {
        2 => {
            let (xs, f) = (&args[0], &args[1]);
            map_discard_impl(vm, xs, f, &Value::Int(1))
        }
        3 => {
            let (xs, f, d) = (&args[0], &args[1], &args[2]);
            map_discard_impl(vm, xs, f, d)
        }
        _ => unreachable!(),
    }
}

fn map_stop(xs: &Value, d: &Value) -> WqResult<Bc1Stop> {
    let el = match eff_layers(d, xs.depth()) {
        Some(l) => l,
        None => return Err(type_mismatch(BE::Map, 0, "int, inf or -inf", d)),
    };
    Ok(Bc1Stop::AtomOrDepth(el))
}

fn map_impl(vm: &mut dyn BuiltinContext, xs: &Value, f: &Value, d: &Value) -> WqResult<Value> {
    let stop = map_stop(xs, d)?;
    let pure = pure_callback(vm, f, 1);
    let op1 = |v: &Value| call_pure_or_vm1(vm, f, pure.as_ref(), v);
    xs.bc1_until(stop, op1)
        .map_err(|e| e.into_wqerror().src(BE::Map))
}

fn map_discard_impl(
    vm: &mut dyn BuiltinContext,
    xs: &Value,
    f: &Value,
    d: &Value,
) -> WqResult<Value> {
    let stop = map_stop(xs, d)?;
    let pure = pure_callback(vm, f, 1);
    let op1 = |v: &Value| call_pure_or_vm1(vm, f, pure.as_ref(), v);
    xs.bc1_for_each_until(stop, op1)
        .map_err(|e| e.into_wqerror().src(BE::Map))?;
    Ok(Value::unit())
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

    if let Some(seq) = ValueSeq::from_value(xs) {
        for item in seq.values() {
            let result = any_all_at_depth(
                vm,
                func,
                &item,
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
        return Ok(!mode_any);
    }

    match xs {
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
            predicate_result(src, pred)
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
        2 => {
            if let Some(seq) = ValueSeq::from_value(&xs) {
                if seq.len() == 0 {
                    return Ok(Value::unit());
                }
                let mut values = seq.values();
                let mut acc = values.next().expect("sequence is non-empty");
                for item in values {
                    acc = call_fold_func(vm, &f, acc, item)?;
                }
                return Ok(acc);
            }

            match xs {
                Value::Dict(map) => {
                    if map.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut val_iter = map.values();
                    let mut acc = val_iter.next().unwrap().clone();
                    for it in val_iter {
                        acc = call_fold_func(vm, &f, acc, it.clone())?;
                    }
                    Ok(acc)
                }
                other => Ok(other),
            }
        }
        3 => {
            let mut acc = iter.next().unwrap();
            if let Some(seq) = ValueSeq::from_value(&xs) {
                for item in seq.values() {
                    acc = call_fold_func(vm, &f, acc, item)?;
                }
                return Ok(acc);
            }

            match xs {
                Value::Dict(map) => {
                    for it in map.values() {
                        acc = call_fold_func(vm, &f, acc, it.clone())?;
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
        2 => {
            if let Some(seq) = ValueSeq::from_value(&xs) {
                if seq.len() == 0 {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(seq.len());
                let mut values = seq.values();
                let mut acc = values.next().expect("sequence is non-empty");
                results.push(acc.clone());
                for item in values {
                    acc = call_fold_func(vm, &f, acc, item)?;
                    results.push(acc.clone());
                }
                return Ok(Value::from_items(results));
            }

            match xs {
                Value::Dict(map) => {
                    if map.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    let mut val_iter = map.values();
                    let mut acc = val_iter.next().unwrap().clone();
                    results.push(acc.clone());
                    for v in val_iter {
                        acc = call_fold_func(vm, &f, acc, v.clone())?;
                        results.push(acc.clone());
                    }
                    Ok(Value::from_items(results))
                }
                other => Ok(other),
            }
        }
        3 => {
            let mut acc = iter.next().unwrap();
            if let Some(seq) = ValueSeq::from_value(&xs) {
                let mut results: Vec<Value> = Vec::with_capacity(seq.len());
                for item in seq.values() {
                    acc = call_fold_func(vm, &f, acc, item)?;
                    results.push(acc.clone());
                }
                return Ok(Value::from_items(results));
            }

            match xs {
                Value::Dict(map) => {
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    for v in map.values() {
                        acc = call_fold_func(vm, &f, acc, v.clone())?;
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
        2 => {
            if let Some(seq) = ValueSeq::from_value(&xs) {
                if seq.len() == 0 {
                    return Ok(Value::unit());
                }
                let mut results: Vec<Value> = Vec::with_capacity(seq.len());
                let mut values = seq.values().rev();
                let mut acc = values.next().expect("sequence is non-empty");
                results.push(acc.clone());
                for item in values {
                    acc = call_fold_func(vm, &f, acc, item)?;
                    results.push(acc.clone());
                }
                results.reverse();
                return Ok(Value::from_items(results));
            }

            match xs {
                Value::Dict(map) => {
                    if map.is_empty() {
                        return Ok(Value::unit());
                    }
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    let mut val_iter = map.values().rev();
                    let mut acc = val_iter.next().unwrap().clone();
                    results.push(acc.clone());
                    for v in val_iter {
                        acc = call_fold_func(vm, &f, acc, v.clone())?;
                        results.push(acc.clone());
                    }
                    results.reverse();
                    Ok(Value::from_items(results))
                }
                other => Ok(other),
            }
        }
        3 => {
            let mut acc = iter.next().unwrap();
            if let Some(seq) = ValueSeq::from_value(&xs) {
                let mut results: Vec<Value> = Vec::with_capacity(seq.len());
                for item in seq.values().rev() {
                    acc = call_fold_func(vm, &f, acc, item)?;
                    results.push(acc.clone());
                }
                results.reverse();
                return Ok(Value::from_items(results));
            }

            match xs {
                Value::Dict(map) => {
                    let mut results: Vec<Value> = Vec::with_capacity(map.len());
                    for v in map.values().rev() {
                        acc = call_fold_func(vm, &f, acc, v.clone())?;
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
    let pure = pure_callback(vm, &func, 1);
    if let Some(seq) = ValueSeq::from_value(&xs) {
        let mut result = Vec::new();
        for item in seq.values() {
            if filter_predicate(vm, &func, pure.as_ref(), &item)? {
                result.push(item);
            }
        }
        return Ok(Value::from_items(result));
    }

    match xs {
        Value::Dict(map) => {
            let mut result = Vec::new();
            for item in map.values() {
                if filter_predicate(vm, &func, pure.as_ref(), item)? {
                    result.push(item.clone());
                }
            }
            Ok(Value::from_items(result))
        }
        other => Ok(other),
    }
}

pub(super) fn filter_discard(vm: &mut dyn BuiltinContext, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Filter, [2], &args)?;
    let mut iter = args.into_iter();
    let xs = iter.next().unwrap();
    let func = iter.next().unwrap();
    let pure = pure_callback(vm, &func, 1);
    if let Some(seq) = ValueSeq::from_value(&xs) {
        for item in seq.values() {
            filter_predicate(vm, &func, pure.as_ref(), &item)?;
        }
        return Ok(Value::unit());
    }

    if let Value::Dict(map) = xs {
        for item in map.values() {
            filter_predicate(vm, &func, pure.as_ref(), item)?;
        }
    }
    Ok(Value::unit())
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
        let pure = pure_callback(vm, f, 2);
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

    // Direct String handling to avoid List<Char> allocation.
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
        l @ Value::List(items) if l.is_string() => {
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
        Value::IntList(_) | Value::IntRange(_) | Value::FloatList(_) | Value::BoolList(_) => {
            let seq = ValueSeq::from_value(&val).expect("guard checked value has list storage");
            let mut chunks = Vec::new();
            let mut current = Vec::new();
            for item in seq.values() {
                let pred = vm.call(&func, BuiltinFnArgs::from(item.clone()))?;
                match pred.try_to_rust_bool() {
                    Some(true) if splits_done < limit => {
                        chunks.push(Value::from_items(std::mem::take(&mut current)));
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
            chunks.push(Value::from_items(current));
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

fn findwith_threshold_reached(results_len: usize, threshold: i64) -> bool {
    usize::try_from(threshold).is_ok_and(|threshold| results_len >= threshold)
}

fn findwith_search(
    vm: &mut dyn BuiltinContext,
    xs: &Value,
    current_depth: i64,
    results: &mut Vec<Value>,
    path: &mut Vec<i64>,
    ctx: &FindWithCtx<'_>,
) -> WqResult<()> {
    if findwith_threshold_reached(results.len(), ctx.threshold) {
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

    if let Some(seq) = ValueSeq::from_value(xs) {
        let mut visit = |idx: usize, item: Value| -> WqResult<bool> {
            if findwith_threshold_reached(results.len(), ctx.threshold) {
                return Ok(true);
            }
            if is_match(vm, &item)? {
                path.push(idx as i64);
                results.push(Value::IntList(Arc::new(path.clone())));
                path.pop();
                if findwith_threshold_reached(results.len(), ctx.threshold) {
                    return Ok(true);
                }
            } else if current_depth < ctx.max_depth {
                path.push(idx as i64);
                findwith_search(vm, &item, current_depth + 1, results, path, ctx)?;
                path.pop();
            }
            Ok(false)
        };

        if ctx.reverse {
            for (item, idx) in seq.values().rev().zip((0..seq.len()).rev()) {
                if visit(idx, item)? {
                    return Ok(());
                }
            }
        } else {
            for (idx, item) in seq.values().enumerate() {
                if visit(idx, item)? {
                    return Ok(());
                }
            }
        }
        return Ok(());
    }

    match xs {
        Value::Dict(map) => {
            let mut visit = |idx: usize, item: &Value| -> WqResult<bool> {
                if findwith_threshold_reached(results.len(), ctx.threshold) {
                    return Ok(true);
                }
                if is_match(vm, item)? {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if findwith_threshold_reached(results.len(), ctx.threshold) {
                        return Ok(true);
                    }
                } else if current_depth < ctx.max_depth {
                    path.push(idx as i64);
                    findwith_search(vm, item, current_depth + 1, results, path, ctx)?;
                    path.pop();
                }
                Ok(false)
            };

            if ctx.reverse {
                for (idx, item) in map.values().enumerate().rev() {
                    if visit(idx, item)? {
                        return Ok(());
                    }
                }
            } else {
                for (idx, item) in map.values().enumerate() {
                    if visit(idx, item)? {
                        return Ok(());
                    }
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
    Ok(Value::List(Arc::new(results)))
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
    Ok(Value::List(Arc::new(results)))
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
                    crate::ast::BinaryOperator::Add,
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
                    crate::ast::BinaryOperator::Add,
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
                    crate::ast::BinaryOperator::Add,
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
                Instruction::LoadCapture(0),
                Instruction::binary_op(
                    crate::ast::BinaryOperator::FloorDiv,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(3))),
                ),
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
                    Operand::Stack,
                    Operand::Capture(1),
                ),
                Instruction::Postfix(1),
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Modulo,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(3))),
                ),
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
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
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(2)),
                Instruction::load_const(Value::Int(0)),
                Instruction::TailPostfix(2),
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
                    crate::ast::BinaryOperator::Add,
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
                    crate::ast::BinaryOperator::Gt,
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
    fn higher_order_builtins_treat_int_range_as_int_list() {
        let mut vm = Vm::new(vec![]);
        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 4)));
        let add = make_fn(
            Some(&["x", "y"]),
            2,
            vec![
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Local(1),
                ),
                Instruction::Return,
            ],
        );
        let gt_two = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Gt,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(2))),
                ),
                Instruction::Return,
            ],
        );
        let eq_three = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Equal,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(3))),
                ),
                Instruction::Return,
            ],
        );

        assert_eq!(
            fold(
                &mut vm,
                BuiltinFnArgs::from(smallvec![range.clone(), add.clone()])
            )
            .unwrap(),
            Value::Int(10)
        );
        assert_eq!(
            scan(
                &mut vm,
                BuiltinFnArgs::from(smallvec![range.clone(), add.clone()])
            )
            .unwrap(),
            Value::IntList(Arc::new(vec![1, 3, 6, 10]))
        );
        assert_eq!(
            rscan(&mut vm, BuiltinFnArgs::from(smallvec![range.clone(), add])).unwrap(),
            Value::IntList(Arc::new(vec![10, 9, 7, 4]))
        );
        assert_eq!(
            filter(
                &mut vm,
                BuiltinFnArgs::from(smallvec![range.clone(), gt_two.clone()])
            )
            .unwrap(),
            Value::IntList(Arc::new(vec![3, 4]))
        );
        assert_eq!(
            all(
                &mut vm,
                BuiltinFnArgs::from(smallvec![range.clone(), gt_two])
            )
            .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            any(
                &mut vm,
                BuiltinFnArgs::from(smallvec![range.clone(), eq_three.clone()])
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            findw(&mut vm, BuiltinFnArgs::from(smallvec![range, eq_three])).unwrap(),
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![2]))]))
        );
    }

    #[test]
    fn higher_order_builtins_treat_strings_as_char_lists() {
        let mut vm = Vm::new(vec![]);
        let text = Value::String(Arc::new("abc".to_owned()));
        let is_b = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Equal,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Char('b'))),
                ),
                Instruction::Return,
            ],
        );
        let take_item = make_fn(
            Some(&["x", "y"]),
            2,
            vec![Instruction::LoadLocal(1), Instruction::Return],
        );

        assert_eq!(
            fold(
                &mut vm,
                BuiltinFnArgs::from(smallvec![text.clone(), take_item])
            )
            .expect("fold succeeds"),
            Value::Char('c')
        );
        assert_eq!(
            filter(
                &mut vm,
                BuiltinFnArgs::from(smallvec![text.clone(), is_b.clone()])
            )
            .expect("filter succeeds"),
            Value::String(Arc::new("b".to_owned()))
        );
        assert_eq!(
            any(
                &mut vm,
                BuiltinFnArgs::from(smallvec![text.clone(), is_b.clone()])
            )
            .expect("any succeeds"),
            Value::Bool(true)
        );
        assert_eq!(
            findw(&mut vm, BuiltinFnArgs::from(smallvec![text, is_b])).expect("findw succeeds"),
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![1]))]))
        );
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
                    crate::ast::BinaryOperator::Add,
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
                    crate::ast::BinaryOperator::Add,
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
                    crate::ast::BinaryOperator::Multiply,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(2))),
                ),
                Instruction::Return,
            ],
        );
        let f = Value::function_composition(crate::ast::BinaryOperator::Add, inc, double);

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
                    crate::ast::BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(1))),
                ),
                Instruction::Return,
            ],
        );
        let f = Value::unary_function_composition(crate::ast::UnaryOperator::Negate, inc);

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
                    crate::ast::BinaryOperator::Add,
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
                    crate::ast::BinaryOperator::Multiply,
                    Operand::Local(0),
                    Operand::Local(1),
                ),
                Instruction::Return,
            ],
        );
        let f = Value::function_composition(crate::ast::BinaryOperator::Add, add, multiply);

        let result =
            zipw(&mut vm, BuiltinFnArgs::from(smallvec![xs, ys, f])).expect("zipw succeeds");
        assert_eq!(result, Value::IntList(Arc::new(vec![9, 17, 27])));
    }
}
