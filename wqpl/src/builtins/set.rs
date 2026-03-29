use std::borrow::Cow;
use std::sync::Arc;

use indexmap::IndexSet;

use crate::builtins::{BuiltinEnum as BE, BuiltinFnArgs, check_arity};
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

pub(super) fn in_(_vm: &mut Vm, args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::InQ, [2], &args)?;
    let elem = &args[0];
    let container = &args[1];
    match container {
        Value::IntList(items) => {
            if let Value::Int(n) = elem {
                Ok(Value::Bool(items.contains(n)))
            } else {
                Ok(Value::Bool(false))
            }
        }
        Value::List(items) => Ok(Value::Bool(items.contains(elem))),
        Value::Dict(map) => {
            if let Value::Tag(s) = elem {
                Ok(Value::Bool(map.contains_key(s.as_ref())))
            } else {
                Ok(Value::Bool(false))
            }
        }
        Value::Set(items) => Ok(Value::Bool(items.contains(elem))),
        atom => Ok(Value::Bool(elem == atom)),
    }
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
