use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

use crate::builtins::{
    BuiltinEnum as BE, BuiltinFnArgs, check_arity, depth_requirement, type_mismatch,
};
use crate::value::seq::ListStorageSeq;
use crate::value::{IntoWqValue, Value, WqResult};
use crate::wqerror::Requirement;

fn require_explicit_dict_projection(src: BE, args: &[Value]) -> WqResult<()> {
    for (index, value) in args.iter().enumerate() {
        if value.is_dict() {
            return Err(type_mismatch(
                src,
                index,
                Requirement::one_of([Requirement::ATOM, Requirement::LIST]),
                value,
            ));
        }
    }
    Ok(())
}

fn seq_items(v: &Value) -> Vec<Value> {
    if let Some(items) = ListStorageSeq::from_value(v) {
        return items.to_values_vec();
    }

    match v {
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

fn unique_int_items(items: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut seen = IndexSet::new();
    let mut out = Vec::new();
    for item in items {
        if seen.insert(item) {
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

pub(super) fn unique(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Unique, [1], &args)?;
    require_explicit_dict_projection(BE::Unique, &args)?;
    if let Value::IntList(items) = &args[0] {
        return Ok(Value::IntList(Arc::new(unique_int_items(
            items.iter().copied(),
        ))));
    }
    Ok(list_value(unique_items(seq_items(&args[0]))))
}

pub(super) fn union(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Union, [2], &args)?;
    require_explicit_dict_projection(BE::Union, &args)?;
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (&args[0], &args[1]) {
        return Ok(Value::IntList(Arc::new(unique_int_items(
            lhs.iter().chain(rhs.iter()).copied(),
        ))));
    }
    Ok(list_value(unique_items(
        seq_items(&args[0]).into_iter().chain(seq_items(&args[1])),
    )))
}

pub(super) fn intersect(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Intersect, [2], &args)?;
    require_explicit_dict_projection(BE::Intersect, &args)?;
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (&args[0], &args[1]) {
        let rhs: IndexSet<i64> = rhs.iter().copied().collect();
        let mut seen = IndexSet::new();
        return Ok(Value::IntList(Arc::new(
            lhs.iter()
                .copied()
                .filter(|item| rhs.contains(item) && seen.insert(*item))
                .collect(),
        )));
    }
    let rhs = item_set(&args[1]);
    Ok(list_value(unique_items(
        seq_items(&args[0])
            .into_iter()
            .filter(|item| rhs.contains(item)),
    )))
}

pub(super) fn without(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Without, [2], &args)?;
    require_explicit_dict_projection(BE::Without, &args)?;
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (&args[0], &args[1]) {
        let rhs: IndexSet<i64> = rhs.iter().copied().collect();
        return Ok(Value::IntList(Arc::new(
            lhs.iter()
                .copied()
                .filter(|item| !rhs.contains(item))
                .collect(),
        )));
    }
    let rhs = item_set(&args[1]);
    Ok(list_value(
        seq_items(&args[0])
            .into_iter()
            .filter(|item| !rhs.contains(item))
            .collect(),
    ))
}

pub(super) fn symdiff(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Symdiff, [2], &args)?;
    require_explicit_dict_projection(BE::Symdiff, &args)?;
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (&args[0], &args[1]) {
        let lhs_set: IndexSet<i64> = lhs.iter().copied().collect();
        let rhs_set: IndexSet<i64> = rhs.iter().copied().collect();
        let mut seen = IndexSet::new();
        let mut out = Vec::new();
        for item in lhs.iter().copied() {
            if !rhs_set.contains(&item) && seen.insert(item) {
                out.push(item);
            }
        }
        for item in rhs.iter().copied() {
            if !lhs_set.contains(&item) && seen.insert(item) {
                out.push(item);
            }
        }
        return Ok(Value::IntList(Arc::new(out)));
    }
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

pub(super) fn subset(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::SubQ, [2], &args)?;
    require_explicit_dict_projection(BE::SubQ, &args)?;
    Ok(Value::Bool(is_subset(&args[0], &args[1])))
}

pub(super) fn proper_subset(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::PSubQ, [2], &args)?;
    require_explicit_dict_projection(BE::PSubQ, &args)?;
    Ok(Value::Bool(is_proper_subset(&args[0], &args[1])))
}

pub(super) fn superset(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::SuperQ, [2], &args)?;
    require_explicit_dict_projection(BE::SuperQ, &args)?;
    Ok(Value::Bool(is_subset(&args[1], &args[0])))
}

pub(super) fn proper_superset(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::PSuperQ, [2], &args)?;
    require_explicit_dict_projection(BE::PSuperQ, &args)?;
    Ok(Value::Bool(is_proper_subset(&args[1], &args[0])))
}

pub(super) fn member(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::MemberQ, [2], &args)?;
    require_explicit_dict_projection(BE::MemberQ, &args)?;
    if let Value::IntList(rhs) = &args[1] {
        let rhs: IndexSet<i64> = rhs.iter().copied().collect();
        return match &args[0] {
            Value::Int(n) => Ok(Value::Bool(rhs.contains(n))),
            Value::IntList(lhs) => Ok(Value::List(Arc::new(
                lhs.iter()
                    .map(|item| Value::Bool(rhs.contains(item)))
                    .collect(),
            ))),
            lhs => Ok(member_result(
                lhs,
                seq_items(lhs)
                    .into_iter()
                    .map(|item| Value::Bool(matches!(item, Value::Int(n) if rhs.contains(&n))))
                    .collect(),
            )),
        };
    }
    let rhs = item_set(&args[1]);
    let results: Vec<Value> = seq_items(&args[0])
        .into_iter()
        .map(|item| Value::Bool(rhs.contains(&item)))
        .collect();
    Ok(member_result(&args[0], results))
}

fn member_result(lhs: &Value, results: Vec<Value>) -> Value {
    if lhs.is_atom() {
        results
            .into_iter()
            .next()
            .expect("atom has one member result")
    } else {
        Value::from_items(results)
    }
}

fn is_subset(lhs: &Value, rhs: &Value) -> bool {
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (lhs, rhs) {
        let rhs: IndexSet<i64> = rhs.iter().copied().collect();
        return lhs.iter().all(|item| rhs.contains(item));
    }
    let rhs = item_set(rhs);
    item_set(lhs).iter().all(|item| rhs.contains(item))
}

fn is_proper_subset(lhs: &Value, rhs: &Value) -> bool {
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (lhs, rhs) {
        let lhs: IndexSet<i64> = lhs.iter().copied().collect();
        let rhs: IndexSet<i64> = rhs.iter().copied().collect();
        return lhs.len() < rhs.len() && lhs.iter().all(|item| rhs.contains(item));
    }
    let lhs = item_set(lhs);
    let rhs = item_set(rhs);
    lhs.len() < rhs.len() && lhs.iter().all(|item| rhs.contains(item))
}

pub(super) fn carproduct(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Cart, [2], &args)?;
    require_explicit_dict_projection(BE::Cart, &args)?;
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
    if let Some(items) = ListStorageSeq::from_value(container) {
        return items.values().any(|item| &item == elem);
    }

    match container {
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

    if let Some(items) = ListStorageSeq::from_value(container) {
        return items
            .values()
            .any(|item| contains_at_depth(elem, &item, depth - 1));
    }

    match container {
        Value::Dict(map) => map
            .values()
            .any(|item| contains_at_depth(elem, item, depth - 1)),
        Value::String(_) => contains_shallow(elem, container),
        atom => contains_shallow(elem, atom),
    }
}

fn parse_depth_arg(src: BE, container: &Value, depth: &Value) -> WqResult<i64> {
    eff_layers(depth, container.depth())
        .ok_or_else(|| type_mismatch(src, 2, depth_requirement(), depth))
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

pub(super) fn in_(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::InQ, [2, 3], &args)?;
    let elem = &args[0];
    let container = &args[1];
    contains_with_optional_depth(BE::InQ, elem, container, args.get_pos(2))
}

pub(super) fn has(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::HasQ, [2, 3], &args)?;
    let container = &args[0];
    let elem = &args[1];
    contains_with_optional_depth(BE::HasQ, elem, container, args.get_pos(2))
}

pub(super) fn disjoint(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::DisjointQ, [2], &args)?;
    require_explicit_dict_projection(BE::DisjointQ, &args)?;
    if let (Value::IntList(lhs), Value::IntList(rhs)) = (&args[0], &args[1]) {
        let rhs: IndexSet<i64> = rhs.iter().copied().collect();
        return Ok(Value::Bool(!lhs.iter().any(|item| rhs.contains(item))));
    }
    let b = item_set(&args[1]);
    for v in seq_items(&args[0]) {
        if b.contains(&v) {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}

pub(super) fn counts(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Counts, [1], &args)?;
    require_explicit_dict_projection(BE::Counts, &args)?;
    if let Value::IntList(items) = &args[0] {
        let mut counts = IndexMap::<i64, i64>::new();
        for item in items.iter().copied() {
            counts
                .entry(item)
                .and_modify(|count| *count += 1)
                .or_insert(1);
        }
        return Ok(count_int_pairs(counts));
    }

    let mut counts = IndexMap::<Value, i64>::new();
    for item in seq_items(&args[0]) {
        counts
            .entry(item)
            .and_modify(|count| *count += 1)
            .or_insert(1);
    }
    Ok(Value::List(Arc::new(
        counts
            .into_iter()
            .map(|(item, count)| Value::List(Arc::new(vec![item, Value::Int(count)])))
            .collect(),
    )))
}

fn count_int_pairs(counts: IndexMap<i64, i64>) -> Value {
    Value::List(Arc::new(
        counts
            .into_iter()
            .map(|(item, count)| Value::List(Arc::new(vec![Value::Int(item), Value::Int(count)])))
            .collect(),
    ))
}

pub(super) fn multiplicity(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Multiplicity, [2], &args)?;
    let elem = &args[0];
    let container = &args[1];
    if let Some(items) = ListStorageSeq::from_value(container) {
        return Ok(items
            .values()
            .filter(|item| item == elem)
            .count()
            .into_wq_value());
    }

    match container {
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
    use crate::value::seq::IntRangeData;

    #[test]
    fn set_algebra_requires_explicit_dict_projection() {
        let dict = Value::Dict(Arc::new(IndexMap::from([(Arc::from("a"), Value::Int(1))])));

        let unique_error = unique(BuiltinFnArgs::from(dict.clone()))
            .expect_err("unique should reject an implicit dict projection");
        assert_eq!(unique_error.msg.as_deref(), Some("expected atom or list"));
        assert_eq!(
            unique_error.notes.as_slice(),
            ["at argument 1", "got (`a:1) (dict)", "usage: unique[xs]"]
        );

        let union_error = union(BuiltinFnArgs::from(smallvec![dict, Value::empty_list()]))
            .expect_err("union should reject an implicit dict projection");
        assert_eq!(union_error.msg.as_deref(), Some("expected atom or list"));
        assert_eq!(
            union_error.notes.as_slice(),
            ["at argument 1", "got (`a:1) (dict)", "usage: union[xs;ys]"]
        );
    }

    #[test]
    fn carproduct_basic() {
        let a = Value::IntList(Arc::new(vec![1, 2]));
        let b = Value::IntList(Arc::new(vec![3, 4]));
        let res = carproduct(BuiltinFnArgs::from(smallvec![a, b]))
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
        let a = Value::IntList(Arc::new(vec![2, 1, 2, 3]));
        let b = Value::IntList(Arc::new(vec![3, 4, 1]));

        assert_eq!(
            unique(BuiltinFnArgs::from(smallvec![a.clone()])).expect("unique should succeed"),
            Value::IntList(Arc::new(vec![2, 1, 3]))
        );
        assert_eq!(
            union(BuiltinFnArgs::from(smallvec![a.clone(), b.clone()]))
                .expect("union should succeed"),
            Value::IntList(Arc::new(vec![2, 1, 3, 4]))
        );
        assert_eq!(
            intersect(BuiltinFnArgs::from(smallvec![a.clone(), b.clone()]))
                .expect("intersect should succeed"),
            Value::IntList(Arc::new(vec![1, 3]))
        );
        assert_eq!(
            without(BuiltinFnArgs::from(smallvec![a.clone(), b.clone()]))
                .expect("without should succeed"),
            Value::IntList(Arc::new(vec![2, 2]))
        );
        assert_eq!(
            symdiff(BuiltinFnArgs::from(smallvec![a, b])).expect("symdiff should succeed"),
            Value::IntList(Arc::new(vec![2, 4]))
        );
    }

    #[test]
    fn intlist_fast_paths_match_list_contracts() {
        let a_int = Value::IntList(Arc::new(vec![2, 1, 2, 3]));
        let b_int = Value::IntList(Arc::new(vec![3, 4, 1]));
        let a_list = Value::List(Arc::new(vec![
            Value::Int(2),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
        ]));
        let b_list = Value::List(Arc::new(vec![Value::Int(3), Value::Int(4), Value::Int(1)]));

        assert_eq!(
            unique(BuiltinFnArgs::from(smallvec![a_int.clone()]))
                .expect("int-list unique should succeed"),
            unique(BuiltinFnArgs::from(smallvec![a_list.clone()]))
                .expect("list unique should succeed")
        );
        assert_eq!(
            union(BuiltinFnArgs::from(smallvec![a_int.clone(), b_int.clone()]))
                .expect("int-list union should succeed"),
            union(BuiltinFnArgs::from(smallvec![
                a_list.clone(),
                b_list.clone()
            ]))
            .expect("list union should succeed")
        );
        assert_eq!(
            intersect(BuiltinFnArgs::from(smallvec![a_int.clone(), b_int.clone()]))
                .expect("int-list intersect should succeed"),
            intersect(BuiltinFnArgs::from(smallvec![
                a_list.clone(),
                b_list.clone()
            ]))
            .expect("list intersect should succeed")
        );
        assert_eq!(
            without(BuiltinFnArgs::from(smallvec![a_int.clone(), b_int.clone()]))
                .expect("int-list without should succeed"),
            without(BuiltinFnArgs::from(smallvec![
                a_list.clone(),
                b_list.clone()
            ]))
            .expect("list without should succeed")
        );
        assert_eq!(
            symdiff(BuiltinFnArgs::from(smallvec![a_int, b_int]))
                .expect("int-list symdiff should succeed"),
            symdiff(BuiltinFnArgs::from(smallvec![a_list, b_list]))
                .expect("list symdiff should succeed")
        );
    }

    #[test]
    fn int_range_set_items_match_list_contracts() {
        let range = Value::IntRange(Arc::new(IntRangeData::new(2, 1, 4)));

        assert_eq!(
            unique(BuiltinFnArgs::from(smallvec![range.clone()]))
                .expect("range unique should succeed"),
            Value::IntList(Arc::new(vec![2, 3, 4, 5]))
        );
        assert_eq!(
            in_(BuiltinFnArgs::from(smallvec![Value::Int(4), range]))
                .expect("range membership should succeed"),
            Value::Bool(true)
        );
    }

    #[test]
    fn counts_returns_first_seen_value_counts() {
        let xs = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::String(Arc::new("a".to_string())),
            Value::Int(1),
            Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
            Value::String(Arc::new("a".to_string())),
        ]));

        assert_eq!(
            counts(BuiltinFnArgs::from(smallvec![xs])).expect("counts should succeed"),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
                Value::List(Arc::new(vec![
                    Value::String(Arc::new("a".to_string())),
                    Value::Int(2)
                ])),
                Value::List(Arc::new(vec![
                    Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
                    Value::Int(1)
                ])),
            ]))
        );
    }

    #[test]
    fn counts_intlist_uses_first_seen_order() {
        assert_eq!(
            counts(BuiltinFnArgs::from(smallvec![Value::IntList(Arc::new(
                vec![2, 1, 2, 3, 1, 2]
            ))]))
            .expect("counts should succeed"),
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])),
                Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)])),
                Value::List(Arc::new(vec![Value::Int(3), Value::Int(1)])),
            ]))
        );
    }

    #[test]
    fn unique_uses_value_hash_contract_for_complex_values() {
        let zero = Value::from_complex64(num_complex::Complex64::new(0.0, 0.0));
        let signed_zero = Value::from_complex64(num_complex::Complex64::new(-0.0, -0.0));
        let nan = Value::from_complex64(num_complex::Complex64::new(f64::NAN, 0.0));
        let same_nan = Value::from_complex64(num_complex::Complex64::new(
            f64::from_bits(0x7ff8_0000_0000_0001),
            -0.0,
        ));

        assert_eq!(
            unique(BuiltinFnArgs::from(smallvec![Value::List(Arc::new(vec![
                zero.clone(),
                signed_zero,
                nan.clone(),
                same_nan,
            ]))]))
            .expect("unique should succeed"),
            Value::List(Arc::new(vec![zero, nan]))
        );
    }

    #[test]
    fn list_set_predicates_ignore_multiplicity() {
        let small = Value::IntList(Arc::new(vec![1, 1, 2]));
        let large = Value::IntList(Arc::new(vec![1, 2, 3]));

        assert_eq!(
            subset(BuiltinFnArgs::from(smallvec![small.clone(), large.clone()]))
                .expect("subset should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            proper_subset(BuiltinFnArgs::from(smallvec![small.clone(), large.clone()]))
                .expect("proper subset should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            superset(BuiltinFnArgs::from(smallvec![large.clone(), small.clone()]))
                .expect("superset should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            proper_superset(BuiltinFnArgs::from(smallvec![large, small]))
                .expect("proper superset should succeed"),
            Value::Bool(true)
        );
    }

    #[test]
    fn member_returns_shape_like_membership() {
        let haystack = Value::IntList(Arc::new(vec![1, 3]));
        assert_eq!(
            member(BuiltinFnArgs::from(smallvec![
                Value::IntList(Arc::new(vec![1, 2, 3])),
                haystack.clone()
            ]))
            .expect("member should succeed"),
            Value::List(Arc::new(vec![
                Value::Bool(true),
                Value::Bool(false),
                Value::Bool(true),
            ]))
        );
        assert_eq!(
            member(BuiltinFnArgs::from(smallvec![Value::Int(2), haystack]))
                .expect("atom member should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn member_non_int_atom_with_intlist_rhs_returns_atom_bool() {
        assert_eq!(
            member(BuiltinFnArgs::from(smallvec![
                Value::Tag(Arc::from("x")),
                Value::IntList(Arc::new(vec![1, 2, 3])),
            ]))
            .expect("atom member should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn in_basic() {
        let list = Value::IntList(Arc::new(vec![1, 2, 3]));
        assert_eq!(
            in_(BuiltinFnArgs::from(smallvec![Value::Int(2), list.clone()]))
                .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            in_(BuiltinFnArgs::from(smallvec![Value::Int(5), list]))
                .expect("membership should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn has_basic() {
        let list = Value::IntList(Arc::new(vec![1, 2, 3]));
        assert_eq!(
            has(BuiltinFnArgs::from(smallvec![list.clone(), Value::Int(2)]))
                .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            has(BuiltinFnArgs::from(smallvec![list, Value::Int(5)]))
                .expect("membership should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn in_and_has_search_at_requested_depth() {
        let nested = Value::List(Arc::new(vec![
            Value::IntList(Arc::new(vec![1, 2])),
            Value::IntList(Arc::new(vec![3, 4])),
        ]));
        assert_eq!(
            in_(BuiltinFnArgs::from(smallvec![
                Value::Int(2),
                nested.clone()
            ]))
            .expect("membership should succeed"),
            Value::Bool(false)
        );
        assert_eq!(
            in_(BuiltinFnArgs::from(smallvec![
                Value::Int(2),
                nested.clone(),
                Value::Int(1)
            ]))
            .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            has(BuiltinFnArgs::from(smallvec![
                nested.clone(),
                Value::Int(4),
                Value::Int(1)
            ]))
            .expect("membership should succeed"),
            Value::Bool(true)
        );
        assert_eq!(
            has(BuiltinFnArgs::from(smallvec![
                nested,
                Value::Int(5),
                Value::Int(1)
            ]))
            .expect("membership should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn disjoint_basic() {
        let a = Value::IntList(Arc::new(vec![1, 2]));
        let b = Value::IntList(Arc::new(vec![3, 4]));
        assert_eq!(
            disjoint(BuiltinFnArgs::from(smallvec![a.clone(), b]))
                .expect("disjoint should succeed"),
            Value::Bool(true)
        );

        let c = Value::IntList(Arc::new(vec![2, 3]));
        assert_eq!(
            disjoint(BuiltinFnArgs::from(smallvec![a, c])).expect("disjoint should succeed"),
            Value::Bool(false)
        );
    }

    #[test]
    fn multiplicity_basic() {
        let list = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::Int(2),
            Value::Int(1),
            Value::Int(3),
        ]));
        assert_eq!(
            multiplicity(BuiltinFnArgs::from(smallvec![Value::Int(1), list]))
                .expect("multiplicity should succeed"),
            Value::Int(2)
        );
    }
}
