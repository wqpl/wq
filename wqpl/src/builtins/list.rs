use std::cmp::Ordering;
use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::ToPrimitive;
use rayon::prelude::*;
use rayon::slice::ParallelSliceMut;

use crate::builtins::{
    BuiltinEnum as BE, BuiltinFnArgs, at_least_arity_error, check_arity, check_registered_args,
    depth_requirement, type_mismatch,
};
use crate::value::bc::Bc2Stop;
use crate::value::cmp::cmp_atom;
use crate::value::seq::{ExactIntSeq, IntRangeData, ListStorageSeq, ValueSeq};
use crate::value::{Value, WqResult, expected_string1};
use crate::wqerror::{Requirement, WqError, WqErrorType};

fn int_or_bigint(n: BigInt) -> Value {
    n.to_i64()
        .map(Value::Int)
        .unwrap_or_else(|| Value::from_bigint(n))
}

fn sum_int_range(range: &IntRangeData) -> Value {
    let len = range.len();
    if len == 0 {
        return Value::Int(0);
    }

    let len_big = BigInt::from(len);
    let start = BigInt::from(range.start());
    let step = BigInt::from(range.step());
    let last_offset = BigInt::from(len - 1) * step;
    int_or_bigint(len_big * (start * 2 + last_offset) / 2)
}

fn min_int_range(range: &IntRangeData) -> Value {
    if range.len() == 0 {
        return Value::empty_list();
    }
    if range.step() > 0 {
        Value::Int(range.start())
    } else {
        Value::Int(
            range
                .last_value()
                .expect("non-empty int range should have a last value"),
        )
    }
}

fn max_int_range(range: &IntRangeData) -> Value {
    if range.len() == 0 {
        return Value::empty_list();
    }
    if range.step() > 0 {
        Value::Int(
            range
                .last_value()
                .expect("non-empty int range should have a last value"),
        )
    } else {
        Value::Int(range.start())
    }
}

fn sum_exact_int_seq(items: ExactIntSeq<'_>) -> Value {
    const THRESHOLD: usize = 2000;
    match items {
        ExactIntSeq::PackedRange(range) => sum_int_range(range),
        ExactIntSeq::PackedSlice(items) if items.len() > THRESHOLD => {
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
            match acc_big {
                Some(b) => Value::from_bigint(b),
                None => Value::Int(acc_i),
            }
        }
        items => {
            let mut acc_i: i64 = 0;
            let mut acc_big: Option<BigInt> = None;
            for n in items.iter() {
                if let Some(ref mut b) = acc_big {
                    *b += BigInt::from(n);
                } else if let Some(s) = acc_i.checked_add(n) {
                    acc_i = s;
                } else {
                    acc_big = Some(BigInt::from(acc_i) + BigInt::from(n));
                }
            }
            match acc_big {
                Some(b) => Value::from_bigint(b),
                None => Value::Int(acc_i),
            }
        }
    }
}

fn product_exact_int_seq(items: ExactIntSeq<'_>) -> Value {
    if items.len() == 0 {
        return Value::Int(1);
    }

    const THRESHOLD: usize = 2000;
    if let ExactIntSeq::PackedSlice(items) = items
        && items.len() > THRESHOLD
    {
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
        return match acc_big {
            Some(b) => Value::from_bigint(b),
            None => Value::Int(acc_i),
        };
    }

    let mut acc_i: i64 = 1;
    let mut acc_big: Option<BigInt> = None;
    for n in items.iter() {
        if let Some(ref mut b) = acc_big {
            *b *= BigInt::from(n);
        } else if let Some(p) = acc_i.checked_mul(n) {
            acc_i = p;
        } else {
            acc_big = Some(BigInt::from(acc_i) * BigInt::from(n));
        }
    }
    match acc_big {
        Some(b) => Value::from_bigint(b),
        None => Value::Int(acc_i),
    }
}

fn min_exact_int_seq(items: ExactIntSeq<'_>) -> Value {
    match items {
        ExactIntSeq::PackedRange(range) => min_int_range(range),
        ExactIntSeq::PackedSlice(items) if items.len() > 2000 => items
            .par_iter()
            .copied()
            .reduce_with(i64::min)
            .map(Value::Int)
            .unwrap_or_else(Value::empty_list),
        items => items
            .iter()
            .min()
            .map(Value::Int)
            .unwrap_or_else(Value::empty_list),
    }
}

fn max_exact_int_seq(items: ExactIntSeq<'_>) -> Value {
    match items {
        ExactIntSeq::PackedRange(range) => max_int_range(range),
        ExactIntSeq::PackedSlice(items) if items.len() > 2000 => items
            .par_iter()
            .copied()
            .reduce_with(i64::max)
            .map(Value::Int)
            .unwrap_or_else(Value::empty_list),
        items => items
            .iter()
            .max()
            .map(Value::Int)
            .unwrap_or_else(Value::empty_list),
    }
}

pub(super) fn sum(args: BuiltinFnArgs) -> WqResult<Value> {
    let n = args.len();
    if n == 0 {
        return Ok(Value::empty_list());
    }
    if n == 1 {
        let is_atom = args[0].is_atom();
        if is_atom {
            return Ok(args.into_iter().next().unwrap());
        }
        let x = args.into_iter().next().unwrap();
        return match &x {
            Value::IntList(_) | Value::IntRange(_) => Ok(sum_exact_int_seq(
                x.packed_int_seq()
                    .expect("guard checked value is packed int sequence"),
            )),
            Value::FloatList(items) => {
                let mut acc = 0.0;
                for item in items.iter() {
                    acc += item.0;
                }
                Ok(Value::float(acc))
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
                .expected(Requirement::one_of([Requirement::ATOM, Requirement::LIST]))
                .at_arg(0)
                .got1(&x)),
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
        return Ok(Value::empty_list());
    }
    if n == 1 {
        let is_atom = args[0].is_atom();
        if is_atom {
            return Ok(args.into_iter().next().unwrap());
        }
        let x = args.into_iter().next().unwrap();
        return match &x {
            Value::IntList(_) | Value::IntRange(_) => Ok(product_exact_int_seq(
                x.packed_int_seq()
                    .expect("guard checked value is packed int sequence"),
            )),
            Value::FloatList(items) => {
                if items.is_empty() {
                    return Ok(Value::Int(1));
                }
                let mut acc = 1.0;
                for item in items.iter() {
                    acc *= item.0;
                }
                Ok(Value::float(acc))
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
                .expected(Requirement::one_of([Requirement::ATOM, Requirement::LIST]))
                .at_arg(0)
                .got1(&x)),
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

fn extreme_values(
    values: impl IntoIterator<Item = Value>,
    desired: Ordering,
    src: BE,
) -> WqResult<Value> {
    let mut extreme: Option<Value> = None;
    for value in values {
        if !value.is_atom() {
            continue;
        }
        extreme = Some(match extreme {
            None => value,
            Some(current) => match cmp_atom(&value, &current) {
                Some(ordering) if ordering == desired => value,
                Some(_) => current,
                None => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(src)
                        .msg("values are not comparable")
                        .got2(&value, &current));
                }
            },
        });
    }
    Ok(extreme.unwrap_or_else(Value::empty_list))
}

fn extreme(args: BuiltinFnArgs, desired: Ordering, src: BE) -> WqResult<Value> {
    if args.is_empty() {
        return Err(at_least_arity_error(src, 1, args.len()));
    }
    if args.len() != 1 {
        return extreme_values(args, desired, src);
    }
    if args[0].is_atom() {
        return Ok(args.into_iter().next().expect("one argument"));
    }

    match &args[0] {
        Value::IntList(_) | Value::IntRange(_) => {
            let items = args[0]
                .packed_int_seq()
                .expect("guard checked value is packed int sequence");
            Ok(if desired == Ordering::Less {
                min_exact_int_seq(items)
            } else {
                max_exact_int_seq(items)
            })
        }
        Value::FloatList(items) => {
            let item = if desired == Ordering::Less {
                items.iter().copied().min()
            } else {
                items.iter().copied().max()
            };
            Ok(item.map(Value::Float).unwrap_or_else(Value::empty_list))
        }
        Value::Dict(items) => extreme_values(items.values().cloned(), desired, src),
        value => {
            if let Some(items) = ValueSeq::from_value(value) {
                extreme_values(items.values(), desired, src)
            } else {
                Ok(value.clone())
            }
        }
    }
}

pub(super) fn min(args: BuiltinFnArgs) -> WqResult<Value> {
    extreme(args, Ordering::Less, BE::Min)
}

pub(super) fn max(args: BuiltinFnArgs) -> WqResult<Value> {
    extreme(args, Ordering::Greater, BE::Max)
}

pub(super) fn flatten(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Flatten, [1], &args)?;
    Ok(Value::from_items(args[0].flatten()))
}

pub(super) fn reverse(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Reverse, [1], &args)?;
    let v = args.into_iter().next().unwrap();
    Ok(match v {
        Value::IntRange(range) => {
            if let Some(reversed) = range.reversed() {
                Value::IntRange(Arc::new(reversed))
            } else {
                let mut items = range.to_vec();
                items.reverse();
                Value::IntList(Arc::new(items))
            }
        }
        Value::IntList(mut items) => {
            Arc::make_mut(&mut items).reverse();
            Value::IntList(items)
        }
        Value::FloatList(mut items) => {
            Arc::make_mut(&mut items).reverse();
            Value::FloatList(items)
        }
        Value::BoolList(mut items) => {
            Arc::make_mut(&mut items).reverse();
            Value::BoolList(items)
        }
        Value::List(mut items) => {
            let items = Arc::make_mut(&mut items);
            items.reverse();
            Value::from_items(std::mem::take(items))
        }
        Value::String(items) => Value::String(Arc::new(items.chars().rev().collect::<String>())),
        Value::Dict(mut items) => {
            Arc::make_mut(&mut items).reverse();
            Value::Dict(items)
        }
        other => other,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    Value,
    Key,
}

fn parse_sort_by(args: &BuiltinFnArgs) -> WqResult<SortBy> {
    match args.named("by") {
        None => Ok(SortBy::Value),
        Some(Value::Tag(mode)) if mode.as_ref() == "value" => Ok(SortBy::Value),
        Some(Value::Tag(mode)) if mode.as_ref() == "key" => Ok(SortBy::Key),
        Some(value) => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Sort)
            .expected(Requirement::one_of([
                Requirement::literal("`value"),
                Requirement::literal("`key"),
            ]))
            .at_named_arg("by")
            .got1(value)),
    }
}

pub(super) fn sort(args: BuiltinFnArgs) -> WqResult<Value> {
    check_registered_args(BE::Sort, &args)?;
    let sort_by = parse_sort_by(&args)?;
    let v = args.into_iter().next().unwrap();
    if sort_by == SortBy::Key && !matches!(v, Value::Dict(_)) {
        return Err(type_mismatch(BE::Sort, 0, Requirement::DICT, &v));
    }
    let res = match v {
        Value::IntRange(range) if range.step() > 0 => Value::IntRange(range),
        Value::IntRange(range) => {
            if let Some(sorted) = range.reversed() {
                Value::IntRange(Arc::new(sorted))
            } else {
                let mut sorted = range.to_vec();
                sorted.sort();
                Value::IntList(Arc::new(sorted))
            }
        }
        Value::IntList(mut items) => {
            let sorted = Arc::make_mut(&mut items);
            if sorted.len() > 2000 {
                sorted.par_sort();
            } else {
                sorted.sort();
            }
            Value::IntList(items)
        }
        Value::FloatList(mut items) => {
            let slice = Arc::make_mut(&mut items);
            if slice.len() > 2000 {
                slice.par_sort();
            } else {
                slice.sort();
            }
            Value::FloatList(items)
        }
        Value::List(mut items) => {
            let sorted = Arc::make_mut(&mut items);
            let cmp = |a: &Value, b: &Value| {
                if let (Some(sa), Some(sb)) = (a.try_to_rust_string(), b.try_to_rust_string()) {
                    return sa.cmp(&sb);
                }
                cmp_atom(a, b).unwrap_or(Ordering::Equal)
            };
            if sorted.len() > 2000 {
                sorted.par_sort_by(cmp);
            } else {
                sorted.sort_by(cmp);
            }
            Value::from_items(std::mem::take(sorted))
        }
        Value::String(items) => {
            let mut chars = items.chars().collect::<Vec<_>>();
            chars.sort();
            Value::String(Arc::new(chars.into_iter().collect()))
        }
        Value::Dict(mut items) => {
            match sort_by {
                SortBy::Key => Arc::make_mut(&mut items).sort_keys(),
                SortBy::Value => {
                    Arc::make_mut(&mut items).sort_by(|_ka, va, _kb, vb| {
                        if let (Some(sa), Some(sb)) =
                            (va.try_to_rust_string(), vb.try_to_rust_string())
                        {
                            return sa.cmp(&sb);
                        }
                        cmp_atom(va, vb).unwrap_or(Ordering::Equal)
                    });
                }
            }
            Value::Dict(items)
        }

        other => other,
    };
    Ok(res)
}

pub(crate) fn parse_maxsplit(val: Option<&Value>, src: BE) -> WqResult<Option<usize>> {
    let requirement = || {
        Requirement::one_of([
            Requirement::non_negative(Requirement::INT),
            Requirement::literal("inf"),
        ])
    };
    match val {
        None => Ok(None),
        Some(v @ Value::Int(n)) if *n >= 0 => usize::try_from(*n).map(Some).map_err(|_| {
            WqError::new(WqErrorType::Domain)
                .src(src)
                .expected(requirement())
                .at_named_arg("max")
                .got1(v)
        }),
        Some(Value::Float(f)) if f.is_infinite() && f.is_sign_positive() => Ok(None),
        Some(other) => Err(WqError::new(WqErrorType::Domain)
            .src(src)
            .expected(requirement())
            .at_named_arg("max")
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
    let split = || {
        s.split(crate::unicode::is_whitespace)
            .filter(|part| !part.is_empty())
    };
    let parts: Vec<String> = split().map(str::to_string).collect();
    if let Some(n) = maxsplit {
        if parts.len() <= n + 1 {
            Value::value_from_str_chunks(parts)
        } else {
            let mut chunks: Vec<String> = parts.into_iter().take(n).collect();
            let remaining: Vec<&str> = split().skip(n).collect();
            chunks.push(remaining.join(" "));
            Value::value_from_str_chunks(chunks)
        }
    } else {
        Value::value_from_str_chunks(parts)
    }
}

pub(super) fn split(args: BuiltinFnArgs) -> WqResult<Value> {
    const MAXSPLIT_ARG: &str = "max";
    const DELIM_ARG: usize = 1;

    check_registered_args(BE::Split, &args)?;
    let data = &args[0];
    let delim = args.get_pos(DELIM_ARG);
    let maxsplit = parse_maxsplit(args.named(MAXSPLIT_ARG), BE::Split)?;

    match data {
        // String: whitespace or delim split
        Value::String(s) => {
            if let Some(d) = delim {
                let d_str = d
                    .try_to_rust_string()
                    .ok_or_else(|| expected_string1(d).src(BE::Split).at_arg(DELIM_ARG))?;
                Ok(split_string_by_delim(s, &d_str, maxsplit))
            } else {
                Ok(split_string_by_whitespace(s, maxsplit))
            }
        }
        // Char list: whitespace or delim split
        v @ Value::List(items) if items.iter().all(|v| matches!(v, Value::Char(_))) => {
            let s = v
                .try_to_rust_string()
                .ok_or_else(|| expected_string1(v).src(BE::Split).at_arg(0))?;
            if let Some(d) = delim {
                let d_str = d
                    .try_to_rust_string()
                    .ok_or_else(|| expected_string1(d).src(BE::Split).at_arg(DELIM_ARG))?;
                Ok(split_string_by_delim(&s, &d_str, maxsplit))
            } else {
                Ok(split_string_by_whitespace(&s, maxsplit))
            }
        }
        value if ListStorageSeq::from_value(value).is_some() => {
            let items = ListStorageSeq::from_value(value)
                .expect("guard checked value has non-string list storage");
            let mut chunks = Vec::new();
            let mut current = Vec::new();
            let limit = maxsplit.unwrap_or(usize::MAX);
            let mut splits_done = 0;
            for item in items.values() {
                if delim.is_some_and(|d| item == *d) && splits_done < limit {
                    chunks.push(Value::from_items(std::mem::take(&mut current)));
                    splits_done += 1;
                } else {
                    current.push(item);
                }
            }
            chunks.push(Value::from_items(current));
            Ok(Value::List(Arc::new(chunks)))
        }
        other => Err(WqError::new(WqErrorType::Domain)
            .src(BE::Split)
            .expected(Requirement::one_of([
                Requirement::STRING,
                Requirement::LIST,
            ]))
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
    Ok(Value::List(Arc::new(results)))
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
    Ok(Value::List(Arc::new(results)))
}

fn parse_find_args(args: &[Value], src: BE) -> WqResult<(&Value, &Value, i64, i64)> {
    let (xs, elem, threshold, depth) = match args.len() {
        2 => (&args[0], &args[1], 1i64, 1i64),
        3 => {
            let threshold = parse_non_negative_int_or_inf(&args[2], src, 2)?;
            (&args[0], &args[1], threshold, 1)
        }
        4 => {
            let threshold = parse_non_negative_int_or_inf(&args[2], src, 2)?;
            let depth = parse_non_negative_int_or_inf(&args[3], src, 3)?;
            (&args[0], &args[1], threshold, depth)
        }
        _ => unreachable!(),
    };
    Ok((xs, elem, threshold, depth))
}

pub(super) fn parse_non_negative_int_or_inf(
    value: &Value,
    src: BE,
    position: usize,
) -> WqResult<i64> {
    match value {
        Value::Int(n) if *n >= 0 => Ok(*n),
        Value::Float(f) if f.is_infinite() && f.is_sign_positive() => Ok(i64::MAX),
        other => Err(WqError::new(WqErrorType::Domain)
            .src(src)
            .expected(Requirement::one_of([
                Requirement::non_negative(Requirement::INT),
                Requirement::literal("inf"),
            ]))
            .at_arg(position)
            .got1(other)),
    }
}

struct FindCtx<'a> {
    elem: &'a Value,
    max_depth: i64,
    threshold: i64,
    reverse: bool,
}

fn find_threshold_reached(results_len: usize, threshold: i64) -> bool {
    usize::try_from(threshold).is_ok_and(|threshold| results_len >= threshold)
}

fn find_search(
    xs: &Value,
    current_depth: i64,
    results: &mut Vec<Value>,
    path: &mut Vec<i64>,
    ctx: &FindCtx<'_>,
) {
    if find_threshold_reached(results.len(), ctx.threshold) {
        return;
    }

    if let Some(seq) = ValueSeq::from_value(xs) {
        let visit = |idx: usize, item: Value, results: &mut Vec<Value>, path: &mut Vec<i64>| {
            if find_threshold_reached(results.len(), ctx.threshold) {
                return;
            }
            let is_match = ctx.elem == &item;
            if is_match {
                path.push(idx as i64);
                results.push(Value::IntList(Arc::new(path.clone())));
                path.pop();
                if find_threshold_reached(results.len(), ctx.threshold) {
                    return;
                }
            }
            if !is_match && current_depth < ctx.max_depth {
                path.push(idx as i64);
                find_search(&item, current_depth + 1, results, path, ctx);
                path.pop();
            }
        };
        if ctx.reverse {
            for (item, idx) in seq.values().rev().zip((0..seq.len()).rev()) {
                visit(idx, item, results, path);
                if find_threshold_reached(results.len(), ctx.threshold) {
                    break;
                }
            }
        } else {
            for (idx, item) in seq.values().enumerate() {
                visit(idx, item, results, path);
                if find_threshold_reached(results.len(), ctx.threshold) {
                    break;
                }
            }
        }
        return;
    }

    match xs {
        Value::Dict(map) => {
            let visit =
                |idx: usize, item: &Value, results: &mut Vec<Value>, path: &mut Vec<i64>| {
                    if find_threshold_reached(results.len(), ctx.threshold) {
                        return;
                    }
                    let is_match = ctx.elem == item;
                    if is_match {
                        path.push(idx as i64);
                        results.push(Value::IntList(Arc::new(path.clone())));
                        path.pop();
                        if find_threshold_reached(results.len(), ctx.threshold) {
                            return;
                        }
                    }
                    if !is_match && current_depth < ctx.max_depth {
                        path.push(idx as i64);
                        find_search(item, current_depth + 1, results, path, ctx);
                        path.pop();
                    }
                };
            if ctx.reverse {
                for (item, idx) in map.values().rev().zip((0..map.len()).rev()) {
                    visit(idx, item, results, path);
                    if find_threshold_reached(results.len(), ctx.threshold) {
                        break;
                    }
                }
            } else {
                for (idx, item) in map.values().enumerate() {
                    visit(idx, item, results, path);
                    if find_threshold_reached(results.len(), ctx.threshold) {
                        break;
                    }
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
            None => {
                return Err(type_mismatch(BE::Zip, 2, depth_requirement(), d));
            }
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

    fn sample_unsorted_dict() -> Value {
        Value::Dict(Arc::new(IndexMap::from([
            (Arc::from("b"), Value::Int(2)),
            (Arc::from("a"), Value::Int(3)),
            (Arc::from("c"), Value::Int(1)),
        ])))
    }

    #[test]
    fn sort_dict_defaults_to_values_and_accepts_explicit_axes() {
        let by_value = Value::Dict(Arc::new(IndexMap::from([
            (Arc::from("c"), Value::Int(1)),
            (Arc::from("b"), Value::Int(2)),
            (Arc::from("a"), Value::Int(3)),
        ])));
        assert_eq!(
            sort(BuiltinFnArgs::from(sample_unsorted_dict())).expect("default sort succeeds"),
            by_value
        );

        assert_eq!(
            sort(BuiltinFnArgs::with_named(
                smallvec![sample_unsorted_dict()],
                vec![(Arc::from("by"), Value::Tag(Arc::from("value")))]
            ))
            .expect("value sort succeeds"),
            by_value
        );

        assert_eq!(
            sort(BuiltinFnArgs::with_named(
                smallvec![sample_unsorted_dict()],
                vec![(Arc::from("by"), Value::Tag(Arc::from("key")))]
            ))
            .expect("key sort succeeds"),
            Value::Dict(Arc::new(IndexMap::from([
                (Arc::from("a"), Value::Int(3)),
                (Arc::from("b"), Value::Int(2)),
                (Arc::from("c"), Value::Int(1)),
            ])))
        );
    }

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
        assert_eq!(atom, Value::empty_list());
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
        assert_eq!(
            split(BuiltinFnArgs::from(vec![
                Value::BoolList(Arc::new(vec![true, false, true])),
                Value::Bool(false),
            ]))
            .unwrap(),
            Value::List(Arc::new(vec![
                Value::BoolList(Arc::new(vec![true])),
                Value::BoolList(Arc::new(vec![true])),
            ]))
        );
    }

    #[test]
    fn split_maxsplit_works() {
        // String whitespace with maxsplit
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec!["a b c".into_wq_value()],
                vec![(Arc::from("max"), Value::Int(1))],
            ))
            .unwrap(),
            Value::List(Arc::new(vec!["a".into_wq_value(), "b c".into_wq_value()]))
        );
        // String delim with maxsplit
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec!["a,b,c".into_wq_value(), ",".into_wq_value()],
                vec![(Arc::from("max"), Value::Int(1)),],
            ))
            .unwrap(),
            Value::List(Arc::new(vec!["a".into_wq_value(), "b,c".into_wq_value()]))
        );
        // IntList with maxsplit
        assert_eq!(
            split(BuiltinFnArgs::with_named(
                smallvec![Value::IntList(Arc::new(vec![1, 2, 3, 2, 4])), Value::Int(2)],
                vec![(Arc::from("max"), Value::Int(1)),],
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
                vec![(Arc::from("max"), Value::Int(0)),],
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
    fn split_uses_the_unicode_white_space_property() {
        assert_eq!(
            split(BuiltinFnArgs::from("a\u{85}b\u{3000}c".into_wq_value())).unwrap(),
            Value::List(Arc::new(vec![
                "a".into_wq_value(),
                "b".into_wq_value(),
                "c".into_wq_value(),
            ]))
        );
        assert_eq!(
            split(BuiltinFnArgs::from("a\u{180e}b".into_wq_value())).unwrap(),
            Value::List(Arc::new(vec!["a\u{180e}b".into_wq_value()]))
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
        let res =
            wq_where(BuiltinFnArgs::from(mat)).expect("where should accept nested bool-list rows");
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
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![1]))]))
        );

        // Not found returns an empty path-result frame.
        let list = Value::List(Arc::new(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
        let result = find(BuiltinFnArgs::from(smallvec![list, Value::Int(5)])).unwrap();
        assert_eq!(result, Value::empty_list());
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
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![0]))]))
        );

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
        // Find a sub-list and keep the outer path-result frame.
        let target = Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)]));
        let list = Value::List(Arc::new(vec![
            Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
        ]));
        let result = find(BuiltinFnArgs::from(smallvec![list, target.clone()])).unwrap();
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![1]))]))
        );

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
        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![1, 0]))]))
        );
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

    #[test]
    fn aggregates_treat_int_range_as_int_list() {
        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(2, 3, 4)));
        assert_eq!(
            sum(BuiltinFnArgs::from(range.clone())).expect("sum succeeds"),
            Value::Int(26)
        );
        assert_eq!(
            product(BuiltinFnArgs::from(range.clone())).expect("product succeeds"),
            Value::Int(880)
        );
        assert_eq!(
            min(BuiltinFnArgs::from(range.clone())).expect("min succeeds"),
            Value::Int(2)
        );
        assert_eq!(
            max(BuiltinFnArgs::from(range)).expect("max succeeds"),
            Value::Int(11)
        );

        let descending = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(7, -2, 4)));
        assert_eq!(
            sum(BuiltinFnArgs::from(descending.clone())).expect("sum succeeds"),
            Value::Int(16)
        );
        assert_eq!(
            min(BuiltinFnArgs::from(descending.clone())).expect("min succeeds"),
            Value::Int(1)
        );
        assert_eq!(
            max(BuiltinFnArgs::from(descending)).expect("max succeeds"),
            Value::Int(7)
        );
    }

    #[test]
    fn range_sum_promotes_when_formula_overflows_i64() {
        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(
            i64::MAX - 2,
            1,
            3,
        )));
        let expected =
            BigInt::from(i64::MAX - 2) + BigInt::from(i64::MAX - 1) + BigInt::from(i64::MAX);
        assert_eq!(
            sum(BuiltinFnArgs::from(range)).expect("sum succeeds"),
            Value::from_bigint(expected)
        );
    }

    #[test]
    fn list_builtins_treat_int_range_as_int_list() {
        let descending = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(3, -1, 3)));
        let reversed = reverse(BuiltinFnArgs::from(descending.clone())).unwrap();
        assert!(matches!(
            &reversed,
            Value::IntRange(range) if range.start() == 1 && range.step() == 1 && range.len() == 3
        ));
        assert_eq!(reversed, Value::IntList(Arc::new(vec![1, 2, 3])));

        let sorted = sort(BuiltinFnArgs::from(descending)).unwrap();
        assert!(matches!(
            &sorted,
            Value::IntRange(range) if range.start() == 1 && range.step() == 1 && range.len() == 3
        ));
        assert_eq!(sorted, Value::IntList(Arc::new(vec![1, 2, 3])));

        let range = Value::IntRange(Arc::new(crate::value::seq::IntRangeData::new(1, 1, 5)));
        assert_eq!(
            split(BuiltinFnArgs::from(smallvec![range.clone(), Value::Int(3)])).unwrap(),
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1, 2])),
                Value::IntList(Arc::new(vec![4, 5])),
            ]))
        );
        assert_eq!(
            find(BuiltinFnArgs::from(smallvec![range.clone(), Value::Int(3)])).unwrap(),
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![2]))]))
        );
        assert_eq!(
            rfind(BuiltinFnArgs::from(smallvec![range, Value::Int(3)])).unwrap(),
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![2]))]))
        );
    }

    #[test]
    fn list_builtins_treat_strings_as_char_lists() {
        let text = "cba".into_wq_value();
        assert_eq!(
            reverse(BuiltinFnArgs::from(text.clone())).expect("reverse succeeds"),
            "abc".into_wq_value()
        );
        assert_eq!(
            sort(BuiltinFnArgs::from(text.clone())).expect("sort succeeds"),
            "abc".into_wq_value()
        );
        assert_eq!(
            min(BuiltinFnArgs::from(text.clone())).expect("min succeeds"),
            Value::Char('a')
        );
        assert_eq!(
            max(BuiltinFnArgs::from(text.clone())).expect("max succeeds"),
            Value::Char('c')
        );
        assert_eq!(
            find(BuiltinFnArgs::from(smallvec![text, Value::Char('b')])).expect("find succeeds"),
            Value::List(Arc::new(vec![Value::IntList(Arc::new(vec![1]))]))
        );
    }

    #[test]
    fn zip_reports_invalid_depth_at_the_third_argument() {
        let error = zip(BuiltinFnArgs::from(smallvec![
            Value::IntList(Arc::new(vec![1])),
            Value::IntList(Arc::new(vec![2])),
            Value::Char('x'),
        ]))
        .expect_err("char depth should fail");

        assert_eq!(error.msg.as_deref(), Some("expected int, inf, or -inf"));
        assert_eq!(
            error.notes.as_ref(),
            &["at argument 3", "got \"x\" (char)", "usage: zip[xs;ys;d?]",]
        );
    }

    #[test]
    fn min_reports_the_actual_count_when_no_arguments_are_given() {
        let error = min(BuiltinFnArgs::new()).expect_err("min without arguments should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected at least 1 argument, got 0")
        );
        assert_eq!(error.notes.as_slice(), ["min[xs], min[xs;ys+]"]);
    }
}
