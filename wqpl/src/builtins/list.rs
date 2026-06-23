use std::cmp::Ordering;
use std::sync::Arc;

use num_bigint::BigInt;
use rayon::prelude::*;
use rayon::slice::ParallelSliceMut;

use crate::builtins::{
    BuiltinEnum as BE, BuiltinFnArgs, check_arity, check_arity_named, type_mismatch,
};
use crate::value::bc::Bc2Stop;
use crate::value::cmp::cmp_atom;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn sum(args: BuiltinFnArgs) -> WqResult<Value> {
    let n = args.len();
    if n == 0 {
        return Ok(Value::unit());
    }
    if n == 1 {
        let is_atom = args[0].is_atom();
        if is_atom {
            return Ok(args.into_iter().next().unwrap());
        }
        let x = args.into_iter().next().unwrap();
        return match &x {
            Value::IntList(items) => {
                const THRESHOLD: usize = 2000;
                if items.len() > THRESHOLD {
                    let (acc_i, acc_big) = items
                        .par_iter()
                        .copied()
                        .fold(
                            || (0i64, None::<BigInt>),
                            |(acc_i, acc_big), n| {
                                if let Some(mut b) = acc_big {
                                    b += BigInt::from(n);
                                    (acc_i, Some(b))
                                } else if let Some(s) = acc_i.checked_add(n) {
                                    (s, None)
                                } else {
                                    (0, Some(BigInt::from(acc_i) + BigInt::from(n)))
                                }
                            },
                        )
                        .reduce(
                            || (0i64, None::<BigInt>),
                            |(a_i, a_b), (b_i, b_b)| match (a_b, b_b) {
                                (Some(mut a), Some(b)) => {
                                    a += b;
                                    (0, Some(a))
                                }
                                (Some(mut a), None) => {
                                    a += BigInt::from(b_i);
                                    (0, Some(a))
                                }
                                (None, Some(mut b)) => {
                                    b += BigInt::from(a_i);
                                    (0, Some(b))
                                }
                                (None, None) => {
                                    if let Some(s) = a_i.checked_add(b_i) {
                                        (s, None)
                                    } else {
                                        (0, Some(BigInt::from(a_i) + BigInt::from(b_i)))
                                    }
                                }
                            },
                        );
                    Ok(match acc_big {
                        Some(b) => Value::from_bigint(b),
                        None => Value::Int(acc_i),
                    })
                } else {
                    let mut acc_i: i64 = 0;
                    let mut acc_big: Option<BigInt> = None;
                    for &n in items.iter() {
                        if let Some(ref mut b) = acc_big {
                            *b += BigInt::from(n);
                        } else if let Some(s) = acc_i.checked_add(n) {
                            acc_i = s;
                        } else {
                            acc_big = Some(BigInt::from(acc_i) + BigInt::from(n));
                        }
                    }
                    Ok(match acc_big {
                        Some(b) => Value::from_bigint(b),
                        None => Value::Int(acc_i),
                    })
                }
            }
            Value::List(items) => {
                if items.is_empty() {
                    return Ok(Value::Int(0));
                }
                let mut acc = items[0].clone();
                for v in &items[1..] {
                    acc = acc.add(v).map_err(|e| e.src(BE::Sum))?;
                }
                Ok(acc)
            }

            _ => Err(WqError::new(WqErrorType::Domain)
                .src(BE::Sum)
                .msg("expected atom or list")
                .at_arg(0)),
        };
    }
    // Multiple args
    let mut iter = args.into_iter();
    let mut acc = iter.next().unwrap();
    for v in iter {
        acc = acc.add(&v).map_err(|e| e.src(BE::Sum))?;
    }
    Ok(acc)
}

pub(super) fn product(args: BuiltinFnArgs) -> WqResult<Value> {
    let n = args.len();
    if n == 0 {
        return Ok(Value::unit());
    }
    if n == 1 {
        let is_atom = args[0].is_atom();
        if is_atom {
            return Ok(args.into_iter().next().unwrap());
        }
        let x = args.into_iter().next().unwrap();
        return match &x {
            Value::IntList(items) => {
                if items.is_empty() {
                    return Ok(Value::Int(1));
                }
                const THRESHOLD: usize = 2000;
                if items.len() > THRESHOLD {
                    let (acc_i, acc_big) = items
                        .par_iter()
                        .copied()
                        .fold(
                            || (1i64, None::<BigInt>),
                            |(acc_i, acc_big), n| {
                                if let Some(mut b) = acc_big {
                                    b *= BigInt::from(n);
                                    (acc_i, Some(b))
                                } else if let Some(p) = acc_i.checked_mul(n) {
                                    (p, None)
                                } else {
                                    (1, Some(BigInt::from(acc_i) * BigInt::from(n)))
                                }
                            },
                        )
                        .reduce(
                            || (1i64, None::<BigInt>),
                            |(a_i, a_b), (b_i, b_b)| match (a_b, b_b) {
                                (Some(mut a), Some(b)) => {
                                    a *= b;
                                    (1, Some(a))
                                }
                                (Some(mut a), None) => {
                                    a *= BigInt::from(b_i);
                                    (1, Some(a))
                                }
                                (None, Some(mut b)) => {
                                    b *= BigInt::from(a_i);
                                    (1, Some(b))
                                }
                                (None, None) => {
                                    if let Some(p) = a_i.checked_mul(b_i) {
                                        (p, None)
                                    } else {
                                        (1, Some(BigInt::from(a_i) * BigInt::from(b_i)))
                                    }
                                }
                            },
                        );
                    Ok(match acc_big {
                        Some(b) => Value::from_bigint(b),
                        None => Value::Int(acc_i),
                    })
                } else {
                    let mut acc_i: i64 = 1;
                    let mut acc_big: Option<BigInt> = None;
                    for &n in items.iter() {
                        if let Some(ref mut b) = acc_big {
                            *b *= BigInt::from(n);
                        } else if let Some(p) = acc_i.checked_mul(n) {
                            acc_i = p;
                        } else {
                            acc_big = Some(BigInt::from(acc_i) * BigInt::from(n));
                        }
                    }
                    Ok(match acc_big {
                        Some(b) => Value::from_bigint(b),
                        None => Value::Int(acc_i),
                    })
                }
            }
            Value::List(items) => {
                if items.is_empty() {
                    return Ok(Value::Int(1));
                }
                let mut acc = items[0].clone();
                for v in &items[1..] {
                    acc = acc.multiply(v).map_err(|e| e.src(BE::Product))?;
                }
                Ok(acc)
            }

            _ => Err(WqError::new(WqErrorType::Domain)
                .src(BE::Product)
                .msg("expected atom or list")
                .at_arg(0)),
        };
    }
    // Multiple args
    let mut iter = args.into_iter();
    let mut acc = iter.next().unwrap();
    for v in iter {
        acc = acc.multiply(&v).map_err(|e| e.src(BE::Product))?;
    }
    Ok(acc)
}

pub(super) fn min(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BE::Min)
            .msg("expected at least 1 argument"));
    }
    if args.len() == 1 && args[0].is_atom() {
        return Ok(args.into_iter().next().unwrap());
    }
    let values: Vec<&Value> = if args.len() == 1 {
        // Single argument: extract immediate elements only
        match &args[0] {
            Value::List(items) => items.iter().collect(),
            Value::IntList(items) => {
                const THRESHOLD: usize = 2000;
                if items.len() > THRESHOLD {
                    let min_int = items.par_iter().copied().reduce_with(|a, b| a.min(b));
                    return Ok(min_int.map(Value::Int).unwrap_or_else(Value::unit));
                }
                let mut min_int: Option<i64> = None;
                for &item in items.iter() {
                    min_int = Some(match min_int {
                        None => item,
                        Some(current) => current.min(item),
                    });
                }
                return Ok(min_int.map(Value::Int).unwrap_or_else(Value::unit));
            }
            Value::Dict(items) => items.values().collect(),

            _atom => return Ok(args.into_iter().next().unwrap()),
        }
    } else {
        // Multiple arguments: compare them directly
        args.iter().collect()
    };
    if values.is_empty() {
        return Ok(Value::unit());
    }
    // Filter to only atoms (skip nested lists/dicts)
    let mut min_val: Option<&Value> = None;
    for val in values {
        // Only consider atoms
        match val {
            v if !v.is_atom() => continue,
            atom => {
                min_val = Some(match min_val {
                    None => atom,
                    Some(current) => {
                        if let Some(ord) = cmp_atom(atom, current) {
                            if ord == Ordering::Less { atom } else { current }
                        } else {
                            return Err(WqError::new(WqErrorType::Domain).src(BE::Min).msg(
                                format!(
                                    "cannot compare {} and {}",
                                    atom.type_name(),
                                    current.type_name()
                                ),
                            ));
                        }
                    }
                });
            }
        }
    }
    Ok(min_val.cloned().unwrap_or_else(Value::unit))
}

pub(super) fn max(args: BuiltinFnArgs) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BE::Max)
            .msg("expected at least 1 argument"));
    }
    if args.len() == 1 && args[0].is_atom() {
        return Ok(args.into_iter().next().unwrap());
    }
    let values: Vec<&Value> = if args.len() == 1 {
        // Single argument: extract immediate elements only
        match &args[0] {
            Value::List(items) => items.iter().collect(),
            Value::IntList(items) => {
                const THRESHOLD: usize = 2000;
                if items.len() > THRESHOLD {
                    let max_int = items.par_iter().copied().reduce_with(|a, b| a.max(b));
                    return Ok(max_int.map(Value::Int).unwrap_or_else(Value::unit));
                }
                let mut max_int: Option<i64> = None;
                for &item in items.iter() {
                    max_int = Some(match max_int {
                        None => item,
                        Some(current) => current.max(item),
                    });
                }
                return Ok(max_int.map(Value::Int).unwrap_or_else(Value::unit));
            }
            Value::Dict(items) => items.values().collect(),

            _atom => return Ok(args.into_iter().next().unwrap()),
        }
    } else {
        // Multiple arguments: compare them directly
        args.iter().collect()
    };
    if values.is_empty() {
        return Ok(Value::unit());
    }
    // Filter to only atoms (skip nested lists/dicts)
    let mut max_val: Option<&Value> = None;
    for val in values {
        // Only consider atoms (not List or Dict)
        match val {
            v if !v.is_atom() => continue,
            atom => {
                max_val = Some(match max_val {
                    None => atom,
                    Some(current) => {
                        if let Some(ord) = cmp_atom(atom, current) {
                            if ord == Ordering::Greater {
                                atom
                            } else {
                                current
                            }
                        } else {
                            return Err(WqError::new(WqErrorType::Domain).src(BE::Max).msg(
                                format!(
                                    "cannot compare {} and {}",
                                    atom.type_name(),
                                    current.type_name()
                                ),
                            ));
                        }
                    }
                });
            }
        }
    }
    Ok(max_val.cloned().unwrap_or_else(Value::unit))
}

pub(super) fn flatten(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Flatten, [1], &args)?;
    Ok(Value::from_items(args[0].flatten()))
}

pub(super) fn reverse(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Reverse, [1], &args)?;
    let v = args.into_iter().next().unwrap();
    match &v {
        Value::List(items) => {
            let mut reversed: Vec<Value> = items.iter().cloned().collect();
            reversed.reverse();
            Ok(Value::from_items(reversed))
        }
        Value::IntList(items) => {
            let mut reversed = Arc::clone(items);
            Arc::make_mut(&mut reversed).reverse();
            Ok(Value::IntList(reversed))
        }
        Value::Dict(items) => {
            let mut reversed = items.clone();
            Arc::make_mut(&mut reversed).reverse();
            Ok(Value::Dict(reversed))
        }

        _ => Ok(v),
    }
}

pub(super) fn sort(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Sort, [1], &args)?;
    let v = args.into_iter().next().unwrap();
    let res = match &v {
        Value::IntList(items) => {
            let mut sorted = Arc::clone(items);
            let slice = Arc::make_mut(&mut sorted);
            if slice.len() > 2000 {
                slice.par_sort();
            } else {
                slice.sort();
            }
            Value::IntList(sorted)
        }
        Value::List(items) => {
            let mut sorted: Vec<Value> = items.iter().cloned().collect();
            let cmp = |a: &Value, b: &Value| {
                if let (Ok(sa), Ok(sb)) =
                    (a.to_rust_string_with_note(), b.to_rust_string_with_note())
                {
                    return sa.cmp(&sb);
                }
                cmp_atom(a, b).unwrap_or(Ordering::Equal)
            };
            if sorted.len() > 2000 {
                sorted.par_sort_by(cmp);
            } else {
                sorted.sort_by(cmp);
            }
            Value::from_items(sorted)
        }
        Value::Dict(items) => {
            let mut sorted = items.clone();
            Arc::make_mut(&mut sorted).sort_by(|_ka, va, _kb, vb| {
                if let (Ok(sa), Ok(sb)) =
                    (va.to_rust_string_with_note(), vb.to_rust_string_with_note())
                {
                    return sa.cmp(&sb);
                }
                cmp_atom(va, vb).unwrap_or(Ordering::Equal)
            });
            Value::Dict(sorted)
        }

        _ => v,
    };
    Ok(res)
}

pub(crate) fn parse_maxsplit(val: Option<&Value>, src: BE) -> WqResult<Option<usize>> {
    match val {
        None => Ok(None),
        Some(Value::Int(n)) if *n >= 0 => Ok(Some(*n as usize)),
        Some(Value::Float(f)) if f.is_infinite() && f.is_sign_positive() => Ok(None),
        Some(other) => Err(WqError::new(WqErrorType::Domain)
            .src(src)
            .msg("expected int>=0 or inf for `m")
            .got1(other)),
    }
}

fn split_string_by_delim(s: &str, delim: &str, maxsplit: Option<usize>) -> Value {
    if let Some(n) = maxsplit {
        Value::value_from_str_chunks(s.splitn(n + 1, delim).map(str::to_string).collect())
    } else {
        Value::value_from_str_chunks(s.split(delim).map(str::to_string).collect())
    }
}

fn split_string_by_whitespace(s: &str, maxsplit: Option<usize>) -> Value {
    let parts: Vec<String> = s.split_whitespace().map(str::to_string).collect();
    if let Some(n) = maxsplit {
        if parts.len() <= n + 1 {
            Value::value_from_str_chunks(parts)
        } else {
            let mut chunks: Vec<String> = parts.into_iter().take(n).collect();
            let remaining: Vec<&str> = s.split_whitespace().skip(n).collect();
            chunks.push(remaining.join(" "));
            Value::value_from_str_chunks(chunks)
        }
    } else {
        Value::value_from_str_chunks(parts)
    }
}

pub(super) fn split(args: BuiltinFnArgs) -> WqResult<Value> {
    const MAXSPLIT_ARG: &str = "m";
    const DELIM_ARG: usize = 1;

    check_arity_named(BE::Split, [1, 2], &args, &[MAXSPLIT_ARG])?;
    let data = &args[0];
    let delim = args.get_pos(DELIM_ARG);
    let maxsplit = parse_maxsplit(args.named(MAXSPLIT_ARG), BE::Split)?;

    match data {
        // String: whitespace or delim split
        Value::String(s) => {
            if let Some(d) = delim {
                let d_str = d
                    .to_rust_string_with_note()
                    .map_err(|e| e.src(BE::Split).at_arg(DELIM_ARG))?;
                Ok(split_string_by_delim(s, &d_str, maxsplit))
            } else {
                Ok(split_string_by_whitespace(s, maxsplit))
            }
        }
        // Char list: whitespace or delim split
        v @ Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))) => {
            let s = v
                .to_rust_string_with_note()
                .map_err(|e| e.src(BE::Split).at_arg(0))?;
            if let Some(d) = delim {
                let d_str = d
                    .to_rust_string_with_note()
                    .map_err(|e| e.src(BE::Split).at_arg(DELIM_ARG))?;
                Ok(split_string_by_delim(&s, &d_str, maxsplit))
            } else {
                Ok(split_string_by_whitespace(&s, maxsplit))
            }
        }
        // IntList split
        Value::IntList(items) => {
            let mut chunks = Vec::new();
            let mut current = Vec::new();
            let limit = maxsplit.unwrap_or(usize::MAX);
            let mut splits_done = 0;
            for &item in items.iter() {
                if delim.is_some_and(|d| Value::Int(item) == *d) && splits_done < limit {
                    chunks.push(Value::IntList(Arc::new(std::mem::take(&mut current))));
                    splits_done += 1;
                } else {
                    current.push(item);
                }
            }
            chunks.push(Value::IntList(Arc::new(current)));
            Ok(Value::List(Arc::new(chunks)))
        }
        // List split
        Value::List(items) => {
            let mut chunks = Vec::new();
            let mut current = Vec::new();
            let limit = maxsplit.unwrap_or(usize::MAX);
            let mut splits_done = 0;
            for item in items.iter() {
                if delim.is_some_and(|d| item == d) && splits_done < limit {
                    chunks.push(Value::List(Arc::new(std::mem::take(&mut current))));
                    splits_done += 1;
                } else {
                    current.push(item.clone());
                }
            }
            chunks.push(Value::List(Arc::new(current)));
            Ok(Value::List(Arc::new(chunks)))
        }
        other => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Split)
            .msg("expected string or list")
            .at_arg(0)
            .got1(other)),
    }
}

/// Find element in nested structure
/// find[xs;elem] - find first occurrence, depth 1
/// find[xs;elem;threshold] - find up to threshold occurrences, depth 1
/// find[xs;elem;threshold;depth] - find up to threshold occurrences at
/// specified depth
pub(super) fn find(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Find, [2, 3, 4], &args)?;

    let (xs, elem, threshold, depth) = parse_find_args(&args, BE::Find)?;

    let mut results = Vec::new();
    let mut path = Vec::new();
    let ctx = FindCtx {
        elem,
        max_depth: depth,
        threshold,
        reverse: false,
    };
    find_search(xs, 0, &mut results, &mut path, &ctx);
    if results.is_empty() {
        Ok(Value::unit())
    } else if results.len() == 1 {
        Ok(results.into_iter().next().unwrap())
    } else {
        Ok(Value::List(Arc::new(results)))
    }
}

/// rfind[xs;elem] - find last occurrence, depth 1
/// rfind[xs;elem;threshold] - find up to threshold occurrences from the right,
/// depth 1 rfind[xs;elem;threshold;depth] - find up to threshold occurrences
/// from the right at specified depth
pub(super) fn rfind(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::RFind, [2, 3, 4], &args)?;

    let (xs, elem, threshold, depth) = parse_find_args(&args, BE::RFind)?;

    let mut results = Vec::new();
    let mut path = Vec::new();
    let ctx = FindCtx {
        elem,
        max_depth: depth,
        threshold,
        reverse: true,
    };
    find_search(xs, 0, &mut results, &mut path, &ctx);
    if results.is_empty() {
        Ok(Value::unit())
    } else if results.len() == 1 {
        Ok(results.into_iter().next().unwrap())
    } else {
        Ok(Value::List(Arc::new(results)))
    }
}

fn parse_find_args(args: &[Value], src: BE) -> WqResult<(&Value, &Value, i64, i64)> {
    let (xs, elem, threshold, depth) = match args.len() {
        2 => (&args[0], &args[1], 1i64, 1i64),
        3 => {
            let threshold = match &args[2] {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(src)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            (&args[0], &args[1], threshold, 1)
        }
        4 => {
            let threshold = match &args[2] {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(src)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            let depth = match &args[3] {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(src)
                        .msg("depth must be non-negative int or inf")
                        .at_arg(3));
                }
            };
            (&args[0], &args[1], threshold, depth)
        }
        _ => unreachable!(),
    };
    Ok((xs, elem, threshold, depth))
}

struct FindCtx<'a> {
    elem: &'a Value,
    max_depth: i64,
    threshold: i64,
    reverse: bool,
}

fn find_search(
    xs: &Value,
    current_depth: i64,
    results: &mut Vec<Value>,
    path: &mut Vec<i64>,
    ctx: &FindCtx<'_>,
) {
    if results.len() >= ctx.threshold as usize {
        return;
    }
    match xs {
        Value::List(items) => {
            let indices: Vec<usize> = if ctx.reverse {
                (0..items.len()).rev().collect()
            } else {
                (0..items.len()).collect()
            };
            for idx in indices {
                if results.len() >= ctx.threshold as usize {
                    return;
                }
                let item = &items[idx];
                let is_match = ctx.elem == item;
                if is_match {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if results.len() >= ctx.threshold as usize {
                        return;
                    }
                }
                if !is_match && current_depth < ctx.max_depth {
                    path.push(idx as i64);
                    find_search(item, current_depth + 1, results, path, ctx);
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
                    return;
                }
                let item_val = Value::Int(items[idx]);
                if ctx.elem == &item_val {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if results.len() >= ctx.threshold as usize {
                        return;
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
                    return;
                }
                let item = values[idx];
                let is_match = ctx.elem == item;
                if is_match {
                    path.push(idx as i64);
                    results.push(Value::IntList(Arc::new(path.clone())));
                    path.pop();
                    if results.len() >= ctx.threshold as usize {
                        return;
                    }
                }
                if !is_match && current_depth < ctx.max_depth {
                    path.push(idx as i64);
                    find_search(item, current_depth + 1, results, path, ctx);
                    path.pop();
                }
            }
        }

        _ => {
            if ctx.elem == xs {
                results.push(Value::IntList(Arc::new(path.clone())));
            }
        }
    }
}

pub(super) fn zip(args: BuiltinFnArgs) -> WqResult<Value> {
    #[inline]
    fn eff_layers_2(raw_d: &Value, dx: i64, dy: i64) -> Option<i64> {
        let dmax = dx.max(dy);
        match raw_d {
            Value::Int(n) if *n >= 0 => Some((*n).min(dmax)),
            Value::Int(n) => Some((dmax + *n).max(0)),
            Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(dmax),
            Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
            _ => None,
        }
    }

    fn _zip(xs: &Value, ys: &Value, d: &Value) -> WqResult<Value> {
        let el = match eff_layers_2(d, xs.depth(), ys.depth()) {
            Some(l) => l,
            None => return Err(type_mismatch(BE::Zip, 0, "int, inf or -inf", d)),
        };
        let stop = Bc2Stop::BothAtomOrDepth(el);
        let op2 = |a: &Value, b: &Value| Ok(Value::List(Arc::new(vec![a.clone(), b.clone()])));
        xs.bc2_until(ys, stop, op2)
            .map_err(|e| e.into_wqerror().src(BE::Zip))
    }

    check_arity(BE::Zip, [2, 3], &args)?;
    match args.len() {
        2 => _zip(&args[0], &args[1], &Value::Int(1)),
        3 => _zip(&args[0], &args[1], &args[2]),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use smallvec::smallvec;

    use super::*;
    use crate::builtins::listgen::*;
    use crate::value::IntoWqValue as _;
    use crate::value::access::{insert_in_place, pop_in_place, remove_in_place};
    use crate::vm::Vm;

    #[test]
    fn alloc_with_fill_value() {
        let vec = alloc(BuiltinFnArgs::from(smallvec![Value::Int(3), Value::Int(9)])).unwrap();
        assert_eq!(vec, Value::IntList(Arc::new(vec![9, 9, 9])));

        let shape = Value::List(Arc::new(vec![Value::Int(2), Value::Int(2)]));
        let filled = alloc(BuiltinFnArgs::from(smallvec![shape, Value::Char('x')])).unwrap();
        assert_eq!(
            filled,
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Char('x'), Value::Char('x')])),
                Value::List(Arc::new(vec![Value::Char('x'), Value::Char('x')])),
            ]))
        );
    }

    #[test]
    fn pop_in_place_updates_lists_and_atoms() {
        let mut ints = Value::IntList(Arc::new(vec![1, 2, 3, 4]));
        assert_eq!(
            pop_in_place(&mut ints, 2).unwrap(),
            Value::IntList(Arc::new(vec![3, 4]))
        );
        assert_eq!(ints, Value::IntList(Arc::new(vec![1, 2])));

        let mut items = Value::List(Arc::new(vec![
            Value::Char('a'),
            Value::Char('b'),
            Value::Char('c'),
        ]));
        assert_eq!(pop_in_place(&mut items, 1).unwrap(), Value::Char('c'));
        assert_eq!(
            items,
            Value::List(Arc::new(vec![Value::Char('a'), Value::Char('b')]))
        );

        let mut atom = Value::Int(7);
        assert_eq!(pop_in_place(&mut atom, 1).unwrap(), Value::Int(7));
        assert_eq!(atom, Value::unit());
    }

    #[test]
    fn pop_in_place_updates_dicts() {
        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("b".into(), Value::Int(2));
        map.insert("c".into(), Value::Int(3));
        let mut dict = Value::Dict(Arc::new(map));

        assert_eq!(
            pop_in_place(&mut dict, 2).unwrap(),
            Value::IntList(Arc::new(vec![2, 3]))
        );

        let mut expected = IndexMap::new();
        expected.insert("a".into(), Value::Int(1));
        assert_eq!(dict, Value::Dict(Arc::new(expected)));
    }

    #[test]
    fn split_variants_cover_strings_and_lists() {
        // Whitespace split
        assert_eq!(
            split(BuiltinFnArgs::from("a\nb\n".into_wq_value())).unwrap(),
            Value::List(Arc::new(vec!["a".into_wq_value(), "b".into_wq_value(),]))
        );
        assert_eq!(
            split(BuiltinFnArgs::from("  a \t b  c ".into_wq_value())).unwrap(),
            Value::List(Arc::new(vec![
                "a".into_wq_value(),
                "b".into_wq_value(),
                "c".into_wq_value(),
            ]))
        );
        // Delim split
        assert_eq!(
            split(BuiltinFnArgs::from(vec![
                "a,b,c".into_wq_value(),
                ",".into_wq_value()
            ]))
            .unwrap(),
            Value::List(Arc::new(vec![
                "a".into_wq_value(),
                "b".into_wq_value(),
                "c".into_wq_value(),
            ]))
        );
        // IntList split via named arg
        assert_eq!(
            split(BuiltinFnArgs::from(vec![
                Value::IntList(Arc::new(vec![1, 2, 3, 2, 4])),
                Value::Int(2)
            ]))
            .unwrap(),
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1])),
                Value::IntList(Arc::new(vec![3])),
                Value::IntList(Arc::new(vec![4])),
            ]))
        );
    }

    #[test]
    fn split_maxsplit_works() {
        // String whitespace with maxsplit
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec!["a b c".into_wq_value()],
                vec![(Arc::from("m"), Value::Int(1))],
            ))
            .unwrap(),
            Value::List(Arc::new(vec!["a".into_wq_value(), "b c".into_wq_value()]))
        );
        // String delim with maxsplit
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec!["a,b,c".into_wq_value(), ",".into_wq_value()],
                vec![(Arc::from("m"), Value::Int(1)),],
            ))
            .unwrap(),
            Value::List(Arc::new(vec!["a".into_wq_value(), "b,c".into_wq_value()]))
        );
        // IntList with maxsplit
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec![Value::IntList(Arc::new(vec![1, 2, 3, 2, 4])), Value::Int(2)],
                vec![(Arc::from("m"), Value::Int(1)),],
            ))
            .unwrap(),
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1])),
                Value::IntList(Arc::new(vec![3, 2, 4])),
            ]))
        );
        // List with maxsplit 0 (no splits)
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec![
                    Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3),])),
                    Value::Int(2)
                ],
                vec![(Arc::from("m"), Value::Int(0)),],
            ),)
            .unwrap(),
            Value::List(Arc::new(vec![Value::List(Arc::new(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
            ]))]))
        );
    }

    #[test]
    fn remove_in_place_updates_lists_and_dicts() {
        let mut ints = Value::IntList(Arc::new(vec![10, 20, 30, 40]));
        assert_eq!(
            remove_in_place(&mut ints, &Value::IntList(Arc::new(vec![1, 3]))).unwrap(),
            Value::IntList(Arc::new(vec![20, 40]))
        );
        assert_eq!(ints, Value::IntList(Arc::new(vec![10, 30])));

        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("b".into(), Value::Int(2));
        map.insert("c".into(), Value::Int(3));
        let mut dict = Value::Dict(Arc::new(map));
        assert_eq!(
            remove_in_place(
                &mut dict,
                &Value::List(Arc::new(vec![Value::Tag("b".into()), Value::Int(0)])),
            )
            .unwrap(),
            Value::IntList(Arc::new(vec![2, 1]))
        );

        let mut expected = IndexMap::new();
        expected.insert("c".into(), Value::Int(3));
        assert_eq!(dict, Value::Dict(Arc::new(expected)));
    }

    #[test]
    fn insert_in_place_handles_strings_and_lists() {
        let mut text = "ac".into_wq_value();
        insert_in_place(&mut text, &"b".into_wq_value(), Some(&Value::Int(1))).unwrap();
        assert_eq!(text, "abc".into_wq_value());

        let mut list = Value::IntList(Arc::new(vec![1, 4]));
        insert_in_place(
            &mut list,
            &Value::IntList(Arc::new(vec![2, 3])),
            Some(&Value::Int(1)),
        )
        .unwrap();
        assert_eq!(list, Value::IntList(Arc::new(vec![1, 2, 3, 4])));

        let mut pairwise = Value::IntList(Arc::new(vec![1, 4]));
        insert_in_place(
            &mut pairwise,
            &Value::IntList(Arc::new(vec![2, 3])),
            Some(&Value::IntList(Arc::new(vec![1, 2]))),
        )
        .unwrap();
        assert_eq!(pairwise, Value::IntList(Arc::new(vec![1, 2, 4, 3])));

        let mut broadcast = Value::IntList(Arc::new(vec![1, 4]));
        insert_in_place(
            &mut broadcast,
            &Value::Int(9),
            Some(&Value::IntList(Arc::new(vec![1, 2]))),
        )
        .unwrap();
        assert_eq!(broadcast, Value::IntList(Arc::new(vec![1, 9, 4, 9])));

        let mut between = Value::IntList(Arc::new(vec![1, 2, 3]));
        insert_in_place(&mut between, &Value::Int(999), None).unwrap();
        assert_eq!(between, Value::IntList(Arc::new(vec![1, 999, 2, 999, 3])));
    }

    #[test]
    fn insert_in_place_handles_dicts() {
        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        map.insert("d".into(), Value::Int(4));
        let mut dict = Value::Dict(Arc::new(map));

        let mut xs = IndexMap::new();
        xs.insert("b".into(), Value::Int(2));
        xs.insert("c".into(), Value::Int(3));

        let mut expected = IndexMap::new();
        expected.insert("a".into(), Value::Int(1));
        expected.insert("b".into(), Value::Int(2));
        expected.insert("c".into(), Value::Int(3));
        expected.insert("d".into(), Value::Int(4));

        insert_in_place(&mut dict, &Value::Dict(Arc::new(xs)), Some(&Value::Int(1))).unwrap();
        assert_eq!(dict, Value::Dict(Arc::new(expected)));

        let mut map = IndexMap::new();
        map.insert("a".into(), Value::Int(1));
        let mut dict = Value::Dict(Arc::new(map));

        let mut dsts = IndexMap::new();
        dsts.insert("b".into(), Value::Int(1));
        dsts.insert("c".into(), Value::Int(1));

        let mut expected = IndexMap::new();
        expected.insert("a".into(), Value::Int(1));
        expected.insert("b".into(), Value::Int(9));
        expected.insert("c".into(), Value::Int(9));

        insert_in_place(
            &mut dict,
            &Value::Int(9),
            Some(&Value::Dict(Arc::new(dsts))),
        )
        .unwrap();
        assert_eq!(dict, Value::Dict(Arc::new(expected)));
    }

    #[test]
    fn where_on_nested_bool_matrix() {
        let flat = Value::BoolList(Arc::new(vec![false, true, true]));
        let res = wq_where(BuiltinFnArgs::from(flat)).expect("where should accept bool-list");
        assert_eq!(res, Value::IntList(Arc::new(vec![1, 2])));

        let mat = Value::List(Arc::new(vec![
            Value::BoolList(Arc::new(vec![true, false, false])),
            Value::BoolList(Arc::new(vec![false, true, false])),
            Value::BoolList(Arc::new(vec![false, false, true])),
        ]));
        let res = wq_where(BuiltinFnArgs::from(mat))
            .expect("where should accept nested bool-list rows");
        assert_eq!(
            res,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![0, 0])),
                Value::IntList(Arc::new(vec![1, 1])),
                Value::IntList(Arc::new(vec![2, 2])),
            ]))
        );
    }

    #[test]
    fn test_find_basic() {
        // Simple list - find first occurrence
        let list = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![list, Value::Int(2)])).unwrap();
        assert_eq!(result, Value::IntList(Arc::new(vec![1])));

        // Not found - return unit
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = find(BuiltinFnArgs::from(smallvec![list, Value::Int(5)])).unwrap();
        assert_eq!(result, Value::unit());
    }

    #[test]
    fn test_find_with_threshold() {
        // Find multiple occurrences
        let list = Value::List(Arc::new(vec![
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
            Value::Int(2),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![
            list,
            Value::Int(2),
            Value::Int(2)
        ]))
        .unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![0])),
                Value::IntList(Arc::new(vec![2]))
            ]))
        );

        // Find all occurrences with inf
        let list = Value::List(Arc::new(vec![
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
            Value::Int(2),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![
            list,
            Value::Int(2),
            Value::float(f64::INFINITY)
        ]))
        .unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![0])),
                Value::IntList(Arc::new(vec![2])),
                Value::IntList(Arc::new(vec![3]))
            ]))
        );
    }

    #[test]
    fn test_find_nested() {
        let _vm = Vm::new(vec![]);

        // Nested structure: (2;(2;3);((4;5);6))
        let nested = Value::List(Arc::new(vec![
            Value::Int(2),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(4), Value::Int(5)])),
                Value::Int(6),
            ])),
        ]));

        // Find at depth 1 (default)
        let result = find(BuiltinFnArgs::from(smallvec![
            nested.clone(),
            Value::Int(2)
        ]))
        .unwrap();
        assert_eq!(result, Value::IntList(Arc::new(vec![0])));

        // Find at depth 2 with inf threshold
        let result = find(BuiltinFnArgs::from(smallvec![
            nested.clone(),
            Value::Int(2),
            Value::float(f64::INFINITY),
            Value::Int(2),
        ]))
        .unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![0])),
                Value::IntList(Arc::new(vec![1, 0]))
            ]))
        );
    }

    #[test]
    fn test_find_intlist() {
        // IntList support
        let list = Value::IntList(Arc::new(vec![1, 2, 3, 2, 4]));
        let result = find(BuiltinFnArgs::from(smallvec![
            list,
            Value::Int(2),
            Value::float(f64::INFINITY)
        ]))
        .unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1])),
                Value::IntList(Arc::new(vec![3]))
            ]))
        );
    }

    #[test]
    fn test_find_sublist() {
        // Find a sub-list: find[(2;3);((1;2);(2;3))] should return (1)
        let target = Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)]));
        let list = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![list, target.clone()])).unwrap();
        assert_eq!(result, Value::IntList(Arc::new(vec![1])));

        // Find multiple sub-lists with threshold
        let list = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
            Value::Int(5),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![
            list,
            target.clone(),
            Value::float(f64::INFINITY)
        ]))
        .unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![0])),
                Value::IntList(Arc::new(vec![2]))
            ]))
        );

        // Find sub-list at depth 2
        let nested = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
                Value::Int(4),
            ])),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![
            nested,
            target.clone(),
            Value::float(f64::INFINITY),
            Value::Int(2),
        ]))
        .unwrap();
        assert_eq!(result, Value::IntList(Arc::new(vec![1, 0])));
    }

    #[test]
    fn product_basic() {
        assert_eq!(
            product(BuiltinFnArgs::from(Value::IntList(Arc::new(vec![2, 3, 4])))).unwrap(),
            Value::Int(24)
        );
        assert_eq!(
            product(BuiltinFnArgs::from(Value::IntList(Arc::new(vec![])))).unwrap(),
            Value::Int(1)
        );
        assert_eq!(
            product(BuiltinFnArgs::from(Value::Int(5))).unwrap(),
            Value::Int(5)
        );
        assert_eq!(
            product(BuiltinFnArgs::from(smallvec![
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ]))
            .unwrap(),
            Value::Int(24)
        );
    }

    #[test]
    fn sum_min_max_intlist_consistency() {
        let list = Value::IntList(Arc::new(vec![5, 1, 9, 3, 7]));
        assert_eq!(
            sum(BuiltinFnArgs::from(list.clone())).unwrap(),
            Value::Int(25)
        );
        assert_eq!(
            min(BuiltinFnArgs::from(list.clone())).unwrap(),
            Value::Int(1)
        );
        assert_eq!(max(BuiltinFnArgs::from(list)).unwrap(), Value::Int(9));
    }
}
