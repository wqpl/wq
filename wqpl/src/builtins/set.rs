use std::sync::Arc;

use indexmap::IndexSet;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity, type_mismatch};
use crate::value::{IntoWqValue, Value, WqResult};
use crate::vm::Vm;

fn seq_items(v: &Value) -> Vec<Value> {
    match v {
        Value::IntList(items) => items.iter().copied().map(Value::Int).collect(),
        Value::List(items) => items.iter().cloned().collect(),
        Value::String(s) => s.chars().map(Value::Char).collect(),
        Value::Dict(map) => map.keys().cloned().map(Value::Tag).collect(),
        atom => vec![atom.clone()],
    }
}

fn unique_items(items: impl IntoIterator<Item = Value>) -> Vec<Value> {
    let mut seen = IndexSet::new();
    let mut out = Vec::new();
    for item in items {
        if seen.insert(item.clone()) {
            out.push(item);
        }
    }
    out
}

fn item_set(v: &Value) -> IndexSet<Value> {
    seq_items(v).into_iter().collect()
}

fn list_value(items: Vec<Value>) -> Value {
    Value::from_items(items)
}

pub(super) fn unique(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Unique, [1], &args)?;
    Ok(list_value(unique_items(seq_items(&args[0]))))
}

pub(super) fn r#union(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Union, [2], &args)?;
    Ok(list_value(unique_items(
        seq_items(&args[0]).into_iter().chain(seq_items(&args[1])),
    )))
}

pub(super) fn intersect(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Intersect, [2], &args)?;
    let rhs = item_set(&args[1]);
    Ok(list_value(unique_items(
        seq_items(&args[0])
            .into_iter()
            .filter(|item| rhs.contains(item)),
    )))
}

pub(super) fn without(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Without, [2], &args)?;
    let rhs = item_set(&args[1]);
    Ok(list_value(
        seq_items(&args[0])
            .into_iter()
            .filter(|item| !rhs.contains(item))
            .collect(),
    ))
}

pub(super) fn symdiff(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Symdiff, [2], &args)?;
    let lhs = item_set(&args[0]);
    let rhs = item_set(&args[1]);
    let out = seq_items(&args[0])
        .into_iter()
        .filter(|item| !rhs.contains(item))
        .chain(
            seq_items(&args[1])
                .into_iter()
                .filter(|item| !lhs.contains(item)),
        );
    Ok(list_value(unique_items(out)))
}

pub(super) fn subset(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::SubsetQ, [2], &args)?;
    Ok(Value::Bool(is_subset(&args[0], &args[1])))
}

pub(super) fn proper_subset(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::ProperSubsetQ, [2], &args)?;
    Ok(Value::Bool(is_proper_subset(&args[0], &args[1])))
}

pub(super) fn superset(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::SupersetQ, [2], &args)?;
    Ok(Value::Bool(is_subset(&args[1], &args[0])))
}

pub(super) fn proper_superset(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::ProperSupersetQ, [2], &args)?;
    Ok(Value::Bool(is_proper_subset(&args[1], &args[0])))
}

pub(super) fn member(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::MemberQ, [2], &args)?;
    let rhs = item_set(&args[1]);
    let results: Vec<Value> = seq_items(&args[0])
        .into_iter()
        .map(|item| Value::Bool(rhs.contains(&item)))
        .collect();
    if args[0].is_atom() {
        Ok(results
            .into_iter()
            .next()
            .expect("atom has one member result"))
    } else {
        Ok(Value::from_items(results))
    }
}

fn is_subset(lhs: &Value, rhs: &Value) -> bool {
    let rhs = item_set(rhs);
    item_set(lhs).iter().all(|item| rhs.contains(item))
}

fn is_proper_subset(lhs: &Value, rhs: &Value) -> bool {
    let lhs = item_set(lhs);
    let rhs = item_set(rhs);
    lhs.len() < rhs.len() && lhs.iter().all(|item| rhs.contains(item))
}

pub(super) fn carproduct(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Cart, [2], &args)?;
    let a = seq_items(&args[0]);
    let b = seq_items(&args[1]);
    let mut result = Vec::new();
    for av in &a {
        for bv in &b {
            result.push(Value::List(Arc::new(vec![av.clone(), bv.clone()])));
        }
    }
    Ok(Value::List(Arc::new(result)))
}

fn eff_layers(raw_d: &Value, total_depth: i64) -> Option<i64> {
    match raw_d {
        Value::Int(n) if *n >= 0 => Some((*n).min(total_depth)),
        Value::Int(n) => Some((total_depth + *n).max(0)),
        Value::Float(n) if n.is_infinite() && n.is_sign_positive() => Some(total_depth),
        Value::Float(n) if n.is_infinite() && n.is_sign_negative() => Some(0),
        _ => None,
    }
}

fn contains_shallow(elem: &Value, container: &Value) -> bool {
    match container {
        Value::IntList(items) => {
            if let Value::Int(n) = elem {
                items.contains(n)
            } else {
                false
            }
        }
        Value::List(items) => items.contains(elem),
        Value::String(s) => matches!(elem, Value::Char(ch) if s.contains(*ch)),
        Value::Dict(map) => {
            if let Value::Tag(s) = elem {
                map.contains_key(s.as_ref())
            } else {
                false
            }
        }
        atom => elem == atom,
    }
}

fn contains_at_depth(elem: &Value, container: &Value, depth: i64) -> bool {
    if depth <= 0 || container.is_atom() {
        return contains_shallow(elem, container);
    }

    match container {
        Value::List(items) => items
            .iter()
            .any(|item| contains_at_depth(elem, item, depth - 1)),
        Value::Dict(map) => map
            .values()
            .any(|item| contains_at_depth(elem, item, depth - 1)),
        Value::IntList(_) | Value::String(_) => contains_shallow(elem, container),
        atom => contains_shallow(elem, atom),
    }
}

fn parse_depth_arg(src: BE, container: &Value, depth: &Value) -> WqResult<i64> {
    eff_layers(depth, container.depth())
        .ok_or_else(|| type_mismatch(src, 2, "int, inf or -inf", depth))
}

fn contains_with_optional_depth(
    src: BE,
    elem: &Value,
    container: &Value,
    depth: Option<&Value>,
) -> WqResult<Value> {
    let found = match depth {
        Some(depth) => contains_at_depth(elem, container, parse_depth_arg(src, container, depth)?),
        None => contains_shallow(elem, container),
    };
    Ok(Value::Bool(found))
}

pub(super) fn in_(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::InQ, [2, 3], &args)?;
    let elem = &args[0];
    let container = &args[1];
    contains_with_optional_depth(BE::InQ, elem, container, args.get_pos(2))
}

pub(super) fn has(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::HasQ, [2, 3], &args)?;
    let container = &args[0];
    let elem = &args[1];
    contains_with_optional_depth(BE::HasQ, elem, container, args.get_pos(2))
}

pub(super) fn disjoint(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::DisjointQ, [2], &args)?;
    let b = item_set(&args[1]);
    for v in seq_items(&args[0]) {
        if b.contains(&v) {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

pub(super) fn multiplicity(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Multiplicity, [2], &args)?;
    let elem = &args[0];
    let container = &args[1];
    match container {
        Value::IntList(items) => {
            if let Value::Int(n) = elem {
                Ok(items.iter().filter(|&&x| x == *n).count().into_wq_value())
            } else {
                Ok(Value::Int(0))
            }
        }
        Value::List(items) => Ok(items.iter().filter(|x| *x == elem).count().into_wq_value()),
        Value::String(s) => {
            if let Value::Char(ch) = elem {
                Ok(s.chars().filter(|c| c == ch).count().into_wq_value())
            } else {
                Ok(Value::Int(0))
            }
        }
        Value::Dict(map) => {
            if let Value::Tag(s) = elem {
                Ok(Value::Int(if map.contains_key(s.as_ref()) { 1 } else { 0 }))
            } else {
                Ok(Value::Int(0))
            }
        }
        atom => Ok(Value::Int(if elem == atom { 1 } else { 0 })),
    }
}

#[cfg(test)]
mod tests {
    use smallvec::smallvec;

    use super::*;
    use crate::vm::Vm;

    #[test]
    fn carproduct_basic() {
        let mut vm = Vm::new(vec![]);
        let a = Value::IntList(Arc::new(vec![1, 2]));
        let b = Value::IntList(Arc::new(vec![3, 4]));
        let res = carproduct(&mut vm, BuiltinFnArgs::from(smallvec![a, b]))
            .expect("cartesian product should succeed");
        assert_eq!(
            res,
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(1), Value::Int(3)])),
                Value::List(Arc::new(vec![Value::Int(1), Value::Int(4)])),
                Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
                Value::List(Arc::new(vec![Value::Int(2), Value::Int(4)])),
            ]))
        );
    }

    #[test]
    fn list_set_algebra_preserves_first_seen_order() {
        let mut vm = Vm::new(vec![]);
        let a = Value::IntList(Arc::new(vec![2, 1, 2, 3]));
        let b = Value::IntList(Arc::new(vec![3, 4, 1]));

        assert_eq!(
            unique(&mut vm, BuiltinFnArgs::from(smallvec![a.clone()]))
                .expect("unique should succeed"),
            Value::IntList(Arc::new(vec![2, 1, 3]))
        );
        assert_eq!(
            r#union(
                &mut vm,
                BuiltinFnArgs::from(smallvec![a.clone(), b.clone()])
            )
            .expect("union should succeed"),
            Value::IntList(Arc::new(vec![2, 1, 3, 4]))
        );
        assert_eq!(
            intersect(
                &mut vm,
                BuiltinFnArgs::from(smallvec![a.clone(), b.clone()])
            )
            .expect("intersect should succeed"),
            Value::IntList(Arc::new(vec![1, 3]))
        );
        assert_eq!(
            without(
                &mut vm,
                BuiltinFnArgs::from(smallvec![a.clone(), b.clone()])
            )
            .expect("without should succeed"),
            Value::IntList(Arc::new(vec![2, 2]))
        );
        assert_eq!(
            symdiff(&mut vm, BuiltinFnArgs::from(smallvec![a, b])).expect("symdiff should succeed"),
            Value::IntList(Arc::new(vec![2, 4]))
        );
    }

    #[test]
    fn list_set_predicates_ignore_multiplicity() {
        let mut vm = Vm::new(vec![]);
        let small = Value::IntList(Arc::new(vec![1, 1, 2]));
        let large = Value::IntList(Arc::new(vec![1, 2, 3]));

        assert_eq!(
            subset(
                &mut vm,
                BuiltinFnArgs::from(smallvec![small.clone(), large.clone()])
            )
            .expect("subset should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            proper_subset(
                &mut vm,
                BuiltinFnArgs::from(smallvec![small.clone(), large.clone()])
            )
            .expect("proper subset should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            superset(
                &mut vm,
                BuiltinFnArgs::from(smallvec![large.clone(), small.clone()])
            )
            .expect("superset should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            proper_superset(&mut vm, BuiltinFnArgs::from(smallvec![large, small]))
                .expect("proper superset should succeed"),
            Value::Bool(true)
        );
    }

    #[test]
    fn member_returns_shape_like_membership() {
        let mut vm = Vm::new(vec![]);
        let haystack = Value::IntList(Arc::new(vec![1, 3]));
        assert_eq!(
            member(
                &mut vm,
                BuiltinFnArgs::from(smallvec![
                    Value::IntList(Arc::new(vec![1, 2, 3])),
                    haystack.clone()
                ])
            )
            .expect("member should succeed"),
            Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(true),
            ]))
        );
        assert_eq!(
            member(
                &mut vm,
                BuiltinFnArgs::from(smallvec![Value::Int(2), haystack])
            )
            .expect("scalar member should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn in_basic() {
        let mut vm = Vm::new(vec![]);
        let list = Value::IntList(Arc::new(vec![1, 2, 3]));
        assert_eq!(
            in_(
                &mut vm,
                BuiltinFnArgs::from(smallvec![Value::Int(2), list.clone()])
            )
            .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            in_(&mut vm, BuiltinFnArgs::from(smallvec![Value::Int(5), list]))
                .expect("membership should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn has_basic() {
        let mut vm = Vm::new(vec![]);
        let list = Value::IntList(Arc::new(vec![1, 2, 3]));
        assert_eq!(
            has(
                &mut vm,
                BuiltinFnArgs::from(smallvec![list.clone(), Value::Int(2)])
            )
            .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            has(&mut vm, BuiltinFnArgs::from(smallvec![list, Value::Int(5)]))
                .expect("membership should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn in_and_has_search_at_requested_depth() {
        let mut vm = Vm::new(vec![]);
        let nested = Value::List(Arc::new(vec![
            Value::IntList(Arc::new(vec![1, 2])),
            Value::IntList(Arc::new(vec![3, 4])),
        ]));
        assert_eq!(
            in_(
                &mut vm,
                BuiltinFnArgs::from(smallvec![Value::Int(2), nested.clone()])
            )
            .expect("membership should succeed"),
            Value::Bool(false)
        );
        assert_eq!(
            in_(
                &mut vm,
                BuiltinFnArgs::from(smallvec![Value::Int(2), nested.clone(), Value::Int(1)])
            )
            .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            has(
                &mut vm,
                BuiltinFnArgs::from(smallvec![nested.clone(), Value::Int(4), Value::Int(1)])
            )
            .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            has(
                &mut vm,
                BuiltinFnArgs::from(smallvec![nested, Value::Int(5), Value::Int(1)])
            )
            .expect("membership should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn disjoint_basic() {
        let mut vm = Vm::new(vec![]);
        let a = Value::IntList(Arc::new(vec![1, 2]));
        let b = Value::IntList(Arc::new(vec![3, 4]));
        assert_eq!(
            disjoint(&mut vm, BuiltinFnArgs::from(smallvec![a.clone(), b]))
                .expect("disjoint should succeed"),
            Value::Bool(true)
        );

        let c = Value::IntList(Arc::new(vec![2, 3]));
        assert_eq!(
            disjoint(&mut vm, BuiltinFnArgs::from(smallvec![a, c]))
                .expect("disjoint should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn multiplicity_basic() {
        let mut vm = Vm::new(vec![]);
        let list = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(1),
            Value::Int(3),
        ]));
        assert_eq!(
            multiplicity(&mut vm, BuiltinFnArgs::from(smallvec![Value::Int(1), list]))
                .expect("multiplicity should succeed"),
            Value::Int(2)
        );
    }
}
