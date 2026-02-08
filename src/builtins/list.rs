use std::cmp::Ordering;

use crate::{
    builtins::{BuiltinEnum as BE, wqerror_helper::check_arity},
    value::{IntoWqValue, Value, WqResult, cmp::cmp_atom},
    vm::Vm,
    wqerror::{WqError, WqErrorType},
};

use num_bigint::BigInt;

pub fn len(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Len, [1], args)?;
    Ok(args[0].len().into_wq_value())
}

pub fn shape(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Shape, [1], args)?;
    Ok(args[0].shape())
}

pub fn depth(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Depth, [1], args)?;
    Ok(Value::Int(args[0].depth()))
}

pub fn is_uniform(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::UniformQ, [1], args)?;
    Ok(Value::Bool(args[0].is_uniform()))
}

pub fn sum(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    match args {
        [] => Ok(Value::unit()),
        [x] => {
            if x.is_atom() {
                return Ok(x.clone());
            }
            match x {
                Value::IntList(items) => {
                    let mut acc_i: i64 = 0;
                    let mut acc_big: Option<BigInt> = None;
                    for &n in items {
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
                Value::List(items) => {
                    if items.is_empty() {
                        return Ok(Value::Int(0));
                    }
                    let mut acc = items[0].clone();
                    for v in &items[1..] {
                        acc = acc.add(v).map_err(|e| e.into_wqerror().src(BE::Sum))?;
                    }
                    Ok(acc)
                }
                _ => Err(WqError::new(WqErrorType::Domain)
                    .src(BE::Sum)
                    .msg("expected atom or list")
                    .at_arg(0)),
            }
        }
        [a, rest @ ..] => {
            let mut acc = a.clone();
            for v in rest {
                acc = acc.add(v).map_err(|e| e.into_wqerror().src(BE::Sum))?;
            }
            Ok(acc)
        }
    }
}

pub fn min(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BE::Min)
            .msg("expected at least 1 argument"));
    }
    let values: Vec<&Value> = if args.len() == 1 {
        // Single argument: extract immediate elements only
        match &args[0] {
            Value::List(items) => items.iter().collect(),
            Value::IntList(items) => {
                let mut min_int: Option<i64> = None;
                for &item in items {
                    min_int = Some(match min_int {
                        None => item,
                        Some(current) => current.min(item),
                    });
                }
                return Ok(min_int.map(Value::Int).unwrap_or_else(Value::unit));
            }
            Value::Dict(items) => items.values().collect(),
            // If it's an atom, return it directly
            atom => return Ok(atom.clone()),
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

pub fn max(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    if args.is_empty() {
        return Err(WqError::new(WqErrorType::Arity)
            .src(BE::Max)
            .msg("expected at least 1 argument"));
    }
    let values: Vec<&Value> = if args.len() == 1 {
        // Single argument: extract immediate elements only
        match &args[0] {
            Value::List(items) => items.iter().collect(),
            Value::IntList(items) => {
                let mut max_int: Option<i64> = None;
                for &item in items {
                    max_int = Some(match max_int {
                        None => item,
                        Some(current) => current.max(item),
                    });
                }
                return Ok(max_int.map(Value::Int).unwrap_or_else(Value::unit));
            }
            Value::Dict(items) => items.values().collect(),
            // If it's an atom, return it directly
            atom => return Ok(atom.clone()),
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

pub fn flatten(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Flatten, [1], args)?;
    Ok(Value::from_items(args[0].flatten()))
}

pub fn reverse(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Reverse, [1], args)?;
    match &args[0] {
        Value::List(items) => {
            let mut reversed = items.clone();
            reversed.reverse();
            Ok(Value::from_items(reversed))
        }
        Value::IntList(items) => {
            let mut reversed = items.clone();
            reversed.reverse();
            Ok(Value::IntList(reversed))
        }
        Value::Dict(items) => {
            let mut reversed = items.clone();
            reversed.reverse();
            Ok(Value::Dict(reversed))
        }
        v => Ok(v.clone()),
    }
}

pub fn sort(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Sort, [1], args)?;
    let v = &args[0];
    let res = match v {
        Value::IntList(items) => {
            let mut sorted = items.clone();
            sorted.sort();
            Value::IntList(sorted)
        }
        Value::List(items) => {
            let mut sorted = items.clone();
            sorted.sort_by(|a, b| {
                if let (Ok(sa), Ok(sb)) = (a.try_to_string(), b.try_to_string()) {
                    return sa.cmp(&sb);
                }
                cmp_atom(a, b).unwrap_or(Ordering::Equal)
            });
            Value::List(sorted)
        }
        Value::Dict(items) => {
            let mut sorted = items.clone();
            sorted.sort_by(|_ka, va, _kb, vb| {
                if let (Ok(sa), Ok(sb)) = (va.try_to_string(), vb.try_to_string()) {
                    return sa.cmp(&sb);
                }
                cmp_atom(va, vb).unwrap_or(Ordering::Equal)
            });
            Value::Dict(sorted)
        }
        other => other.clone(),
    };
    Ok(res)
}

pub fn filter(vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Filter, [2], args)?;
    let func = &args[0];
    let xs = &args[1];

    match xs {
        Value::IntList(items) => {
            let mut result = Vec::new();
            for &item in items {
                let val = Value::Int(item);
                let pred = vm.call_value(func, std::slice::from_ref(&val))?;
                match pred {
                    Value::Bool(true) => result.push(val),
                    Value::Bool(false) => {}
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Filter)
                            .msg("predicate must return bool"));
                    }
                }
            }
            Ok(Value::from_items(result))
        }
        Value::List(items) => {
            let mut result = Vec::new();
            for item in items {
                let pred = vm.call_value(func, std::slice::from_ref(item))?;
                match pred {
                    Value::Bool(true) => result.push(item.clone()),
                    Value::Bool(false) => {}
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Filter)
                            .msg("predicate must return bool"));
                    }
                }
            }
            Ok(Value::from_items(result))
        }
        Value::Dict(map) => {
            let mut result = Vec::new();
            for value in map.values() {
                let pred = vm.call_value(func, std::slice::from_ref(value))?;
                match pred {
                    Value::Bool(true) => result.push(value.clone()),
                    Value::Bool(false) => {}
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Filter)
                            .msg("predicate must return bool"));
                    }
                }
            }
            Ok(Value::from_items(result))
        }
        other => Ok(other.clone()),
    }
}

/// Find element in nested structure
/// find[elem;xs] - find first occurrence, depth 1
/// find[elem;threshold;xs] - find up to threshold occurrences, depth 1
/// find[elem;threshold;depth;xs] - find up to threshold occurrences at specified depth
pub fn find(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Find, [2, 3, 4], args)?;

    fn search_recursive(
        elem: &Value,
        xs: &Value,
        current_depth: i64,
        max_depth: i64,
        threshold: i64,
        results: &mut Vec<Value>,
        path: &mut Vec<i64>,
    ) {
        if results.len() >= threshold as usize {
            return;
        }
        match xs {
            Value::List(items) => {
                for (idx, item) in items.iter().enumerate() {
                    if results.len() >= threshold as usize {
                        return;
                    }
                    // Check if current item matches
                    let is_match = elem == item;
                    if is_match {
                        path.push(idx as i64);
                        let index_path = Value::IntList(path.clone());
                        results.push(index_path);
                        path.pop();
                        if results.len() >= threshold as usize {
                            return;
                        }
                    }
                    // Recurse deeper if max depth hasn't been reached and item didn't match
                    // (don't recurse into matched items)
                    if !is_match && current_depth < max_depth {
                        path.push(idx as i64);
                        search_recursive(
                            elem,
                            item,
                            current_depth + 1,
                            max_depth,
                            threshold,
                            results,
                            path,
                        );
                        path.pop();
                    }
                }
            }
            Value::IntList(items) => {
                for (idx, &item) in items.iter().enumerate() {
                    if results.len() >= threshold as usize {
                        return;
                    }
                    let item_val = Value::Int(item);
                    // Check if current item matches
                    if elem == &item_val {
                        path.push(idx as i64);
                        let index_path = Value::IntList(path.clone());
                        results.push(index_path);
                        path.pop();
                        if results.len() >= threshold as usize {
                            return;
                        }
                    }
                    // IntList items are atoms, can't recurse deeper
                }
            }
            Value::Dict(map) => {
                for (idx, item) in map.values().enumerate() {
                    if results.len() >= threshold as usize {
                        return;
                    }
                    // Check if current item matches
                    let is_match = elem == item;
                    if is_match {
                        path.push(idx as i64);
                        let index_path = Value::IntList(path.clone());
                        results.push(index_path);
                        path.pop();
                        if results.len() >= threshold as usize {
                            return;
                        }
                    }
                    if !is_match && current_depth < max_depth {
                        path.push(idx as i64);
                        search_recursive(
                            elem,
                            item,
                            current_depth + 1,
                            max_depth,
                            threshold,
                            results,
                            path,
                        );
                        path.pop();
                    }
                }
            }
            _ => {
                if elem == xs {
                    let index_path = Value::IntList(path.clone());
                    results.push(index_path);
                }
            }
        }
    }

    let (elem, depth, threshold, xs) = match args.len() {
        2 => (&args[0], 1i64, 1i64, &args[1]),
        3 => {
            let threshold = match &args[1] {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::Find)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(1));
                }
            };
            (&args[0], 1, threshold, &args[2])
        }
        4 => {
            let threshold = match &args[1] {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::Find)
                        .msg("threshold must be non-negative int or inf")
                        .at_arg(1));
                }
            };
            let depth = match &args[2] {
                Value::Int(n) if *n >= 0 => *n,
                Value::Float(f) if f.is_infinite() && f.is_sign_positive() => i64::MAX,
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::Find)
                        .msg("depth must be non-negative int or inf")
                        .at_arg(2));
                }
            };
            (&args[0], depth, threshold, &args[3])
        }
        _ => unreachable!(),
    };

    let mut results = Vec::new();
    let mut path = Vec::new();
    search_recursive(elem, xs, 0, depth, threshold, &mut results, &mut path);
    if results.is_empty() {
        Ok(Value::unit())
    } else if results.len() == 1 {
        Ok(results.into_iter().next().unwrap())
    } else {
        Ok(Value::List(results))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{builtins::list_gen::*, vm::Vm};

    #[test]
    fn shape_scalar() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            shape(&mut vm, &[Value::Int(42)]).unwrap(),
            Value::IntList(vec![])
        );
    }

    #[test]
    fn shape_and_alloc() {
        let mut vm = Vm::new(vec![]);
        // simple vector
        let vec = alloc(&mut vm, &[Value::Int(3)]).unwrap();
        assert_eq!(vec, Value::IntList(vec![0, 0, 0]));
        assert_eq!(shape(&mut vm, &[vec]).unwrap(), Value::IntList(vec![3]));

        // matrix
        let mat_shape = Value::List(vec![Value::Int(2), Value::Int(3)]);
        let mat = alloc(&mut vm, std::slice::from_ref(&mat_shape)).unwrap();
        assert_eq!(shape(&mut vm, &[mat]).unwrap(), Value::IntList(vec![2, 3]));

        // invalid shape
        let invalid_shape = Value::List(vec![Value::List(vec![Value::Int(2), Value::Int(2)])]);
        let invalid = alloc(&mut vm, std::slice::from_ref(&invalid_shape));
        assert!(invalid.is_err());
    }

    #[test]
    fn shape_atoms_and_empty() {
        let mut vm = Vm::new(vec![]);
        assert_eq!(
            shape(&mut vm, &[Value::Int(5)]).unwrap(),
            Value::IntList(vec![])
        );
        assert_eq!(
            shape(&mut vm, &[Value::Char('a')]).unwrap(),
            Value::IntList(vec![])
        );
        assert_eq!(
            shape(&mut vm, &[Value::List(vec![])]).unwrap(),
            Value::IntList(vec![0])
        );
    }

    #[test]
    fn shape_string_and_mixed_list() {
        let mut vm = Vm::new(vec![]);
        let s = Value::List(vec![Value::Char('h'), Value::Char('i')]);
        assert_eq!(
            shape(&mut vm, std::slice::from_ref(&s)).unwrap(),
            Value::IntList(vec![2])
        );
        let mixed = Value::List(vec![Value::Char('h'), Value::Int(2)]);
        assert_eq!(shape(&mut vm, &[mixed]).unwrap(), Value::IntList(vec![2]));
    }

    #[test]
    fn where_on_nested_bool_matrix() {
        let mut vm = Vm::new(vec![]);
        // ((true;false;false); (false;true;false); (false;false;true))
        let mat = Value::List(vec![
            Value::List(vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(false),
            ]),
            Value::List(vec![
                Value::Bool(false),
                Value::Bool(true),
                Value::Bool(false),
            ]),
            Value::List(vec![
                Value::Bool(false),
                Value::Bool(false),
                Value::Bool(true),
            ]),
        ]);
        let res = wq_where(&mut vm, &[mat]).unwrap();
        assert_eq!(
            res,
            Value::List(vec![
                Value::IntList(vec![0, 0]),
                Value::IntList(vec![1, 1]),
                Value::IntList(vec![2, 2]),
            ])
        );
    }

    #[test]
    fn test_find_basic() {
        let mut vm = Vm::new(vec![]);

        // Simple list - find first occurrence
        let list = Value::List(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
        ]);
        let result = find(&mut vm, &[Value::Int(2), list]).unwrap();
        assert_eq!(result, Value::IntList(vec![1]));

        // Not found - return unit
        let list = Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        let result = find(&mut vm, &[Value::Int(5), list]).unwrap();
        assert_eq!(result, Value::unit());
    }

    #[test]
    fn test_find_with_threshold() {
        let mut vm = Vm::new(vec![]);

        // Find multiple occurrences
        let list = Value::List(vec![
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
            Value::Int(2),
        ]);
        let result = find(&mut vm, &[Value::Int(2), Value::Int(2), list]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::IntList(vec![0]), Value::IntList(vec![2])])
        );

        // Find all occurrences with inf
        let list = Value::List(vec![
            Value::Int(2),
            Value::Int(3),
            Value::Int(2),
            Value::Int(2),
        ]);
        let result = find(&mut vm, &[Value::Int(2), Value::Float(f64::INFINITY), list]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::IntList(vec![0]),
                Value::IntList(vec![2]),
                Value::IntList(vec![3])
            ])
        );
    }

    #[test]
    fn test_find_nested() {
        let mut vm = Vm::new(vec![]);

        // Nested structure: (2;(2;3);((4;5);6))
        let nested = Value::List(vec![
            Value::Int(2),
            Value::List(vec![Value::Int(2), Value::Int(3)]),
            Value::List(vec![
                Value::List(vec![Value::Int(4), Value::Int(5)]),
                Value::Int(6),
            ]),
        ]);

        // Find at depth 1 (default)
        let result = find(&mut vm, &[Value::Int(2), nested.clone()]).unwrap();
        assert_eq!(result, Value::IntList(vec![0]));

        // Find at depth 2 with inf threshold
        let result = find(
            &mut vm,
            &[
                Value::Int(2),
                Value::Float(f64::INFINITY),
                Value::Int(2),
                nested.clone(),
            ],
        )
        .unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::IntList(vec![0]), Value::IntList(vec![1, 0])])
        );
    }

    #[test]
    fn test_find_intlist() {
        let mut vm = Vm::new(vec![]);

        // IntList support
        let list = Value::IntList(vec![1, 2, 3, 2, 4]);
        let result = find(&mut vm, &[Value::Int(2), Value::Float(f64::INFINITY), list]).unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::IntList(vec![1]), Value::IntList(vec![3])])
        );
    }

    #[test]
    fn test_find_sublist() {
        let mut vm = Vm::new(vec![]);

        // Find a sub-list: find[(2;3);((1;2);(2;3))] should return (1)
        let target = Value::List(vec![Value::Int(2), Value::Int(3)]);
        let list = Value::List(vec![
            Value::List(vec![Value::Int(1), Value::Int(2)]),
            Value::List(vec![Value::Int(2), Value::Int(3)]),
        ]);
        let result = find(&mut vm, &[target.clone(), list]).unwrap();
        assert_eq!(result, Value::IntList(vec![1]));

        // Find multiple sub-lists with threshold
        let list = Value::List(vec![
            Value::List(vec![Value::Int(2), Value::Int(3)]),
            Value::Int(5),
            Value::List(vec![Value::Int(2), Value::Int(3)]),
        ]);
        let result = find(
            &mut vm,
            &[target.clone(), Value::Float(f64::INFINITY), list],
        )
        .unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::IntList(vec![0]), Value::IntList(vec![2])])
        );

        // Find sub-list at depth 2
        let nested = Value::List(vec![
            Value::Int(1),
            Value::List(vec![
                Value::List(vec![Value::Int(2), Value::Int(3)]),
                Value::Int(4),
            ]),
        ]);
        let result = find(
            &mut vm,
            &[
                target.clone(),
                Value::Float(f64::INFINITY),
                Value::Int(2),
                nested,
            ],
        )
        .unwrap();
        assert_eq!(result, Value::IntList(vec![1, 0]));
    }
}
