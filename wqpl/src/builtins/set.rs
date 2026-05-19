use std::borrow::Cow;
use std::sync::Arc;

use indexmap::IndexSet;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity, type_mismatch};
use crate::value::{IntoWqValue, Value, WqResult};
use crate::vm::Vm;

fn to_value_vec(v: &Value) -> Vec<Value> {
    match v {
        Value::IntList(items) => items.iter().copied().map(Value::Int).collect(),
        Value::List(items) => items.iter().cloned().collect(),
        Value::Dict(map) => map.keys().cloned().map(Value::Tag).collect(),
        Value::Set(items) => items.iter().cloned().collect(),
        atom => vec![atom.clone()],
    }
}

/// Borrow the inner `IndexSet` if `v` is already a `Value::Set`,
/// otherwise convert `v` into a temporary singleton set.
fn as_set_or_collect(v: &Value) -> Cow<'_, IndexSet<Value>> {
    match v {
        Value::Set(s) => Cow::Borrowed(s),
        other => Cow::Owned(to_value_vec(other).into_iter().collect()),
    }
}

pub(super) fn carproduct(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Carproduct, [2], &args)?;
    let a = to_value_vec(&args[0]);
    let b = to_value_vec(&args[1]);
    let mut result = IndexSet::new();
    for av in &a {
        for bv in &b {
            result.insert(Value::List(Arc::new(vec![av.clone(), bv.clone()])));
        }
    }
    Ok(Value::Set(Arc::new(result)))
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
        Value::Dict(map) => {
            if let Value::Tag(s) = elem {
                map.contains_key(s.as_ref())
            } else {
                false
            }
        }
        Value::Set(items) => items.contains(elem),
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
        Value::Set(items) => items
            .iter()
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
    let a = as_set_or_collect(&args[0]);
    let b = as_set_or_collect(&args[1]);
    for v in a.iter() {
        if b.contains(v) {
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
        Value::Dict(map) => {
            if let Value::Tag(s) = elem {
                Ok(Value::Int(if map.contains_key(s.as_ref()) { 1 } else { 0 }))
            } else {
                Ok(Value::Int(0))
            }
        }
        Value::Set(items) => Ok(Value::Int(if items.contains(elem) { 1 } else { 0 })),
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
        let res = carproduct(&mut vm, BuiltinFnArgs::from(smallvec![a, b])).unwrap();
        let mut expected = IndexSet::new();
        expected.insert(Value::List(Arc::new(vec![Value::Int(1), Value::Int(3)])));
        expected.insert(Value::List(Arc::new(vec![Value::Int(1), Value::Int(4)])));
        expected.insert(Value::List(Arc::new(vec![Value::Int(2), Value::Int(3)])));
        expected.insert(Value::List(Arc::new(vec![Value::Int(2), Value::Int(4)])));
        assert_eq!(res, Value::Set(Arc::new(expected)));
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
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            in_(&mut vm, BuiltinFnArgs::from(smallvec![Value::Int(5), list])).unwrap(),
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
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            has(&mut vm, BuiltinFnArgs::from(smallvec![list, Value::Int(5)])).unwrap(),
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
            .unwrap(),
            Value::Bool(false)
        );
        assert_eq!(
            in_(
                &mut vm,
                BuiltinFnArgs::from(smallvec![Value::Int(2), nested.clone(), Value::Int(1)])
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            has(
                &mut vm,
                BuiltinFnArgs::from(smallvec![nested.clone(), Value::Int(4), Value::Int(1)])
            )
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            has(
                &mut vm,
                BuiltinFnArgs::from(smallvec![nested, Value::Int(5), Value::Int(1)])
            )
            .unwrap(),
            Value::Bool(false)
        );
    }

    #[test]
    fn disjoint_basic() {
        let mut vm = Vm::new(vec![]);
        let a = Value::IntList(Arc::new(vec![1, 2]));
        let b = Value::IntList(Arc::new(vec![3, 4]));
        assert_eq!(
            disjoint(&mut vm, BuiltinFnArgs::from(smallvec![a.clone(), b])).unwrap(),
            Value::Bool(true)
        );

        let c = Value::IntList(Arc::new(vec![2, 3]));
        assert_eq!(
            disjoint(&mut vm, BuiltinFnArgs::from(smallvec![a, c])).unwrap(),
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
            multiplicity(&mut vm, BuiltinFnArgs::from(smallvec![Value::Int(1), list])).unwrap(),
            Value::Int(2)
        );
    }
}
