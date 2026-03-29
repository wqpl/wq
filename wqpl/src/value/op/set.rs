use std::sync::Arc;

use crate::value::{Value, WqResult, expected_set2, with_sets_as_refs};

fn set_binary_op(
    a: &Value,
    b: &Value,
    op: impl FnOnce(&indexmap::IndexSet<Value>, &indexmap::IndexSet<Value>) -> indexmap::IndexSet<Value>,
) -> Option<Value> {
    with_sets_as_refs(a, b, |a_set, b_set| Value::Set(Arc::new(op(a_set, b_set))))
}

impl Value {
    pub(crate) fn set_intersection(&self, other: &Value) -> WqResult<Value> {
        set_binary_op(self, other, |x, y| x.intersection(y).cloned().collect())
            .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_union(&self, other: &Value) -> WqResult<Value> {
        set_binary_op(self, other, |x, y| x.union(y).cloned().collect())
            .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_sym_diff(&self, other: &Value) -> WqResult<Value> {
        set_binary_op(self, other, |x, y| {
            x.symmetric_difference(y).cloned().collect()
        })
        .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_difference(&self, other: &Value) -> WqResult<Value> {
        set_binary_op(self, other, |x, y| x.difference(y).cloned().collect())
            .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_subset(&self, other: &Value) -> WqResult<Value> {
        with_sets_as_refs(self, other, |a, b| {
            Value::Bool(a.is_subset(b) && a.len() < b.len())
        })
        .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_subset_eq(&self, other: &Value) -> WqResult<Value> {
        with_sets_as_refs(self, other, |a, b| Value::Bool(a.is_subset(b)))
            .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_superset(&self, other: &Value) -> WqResult<Value> {
        with_sets_as_refs(self, other, |a, b| {
            Value::Bool(a.is_superset(b) && a.len() > b.len())
        })
        .ok_or_else(|| expected_set2(self, other))
    }

    pub(crate) fn set_superset_eq(&self, other: &Value) -> WqResult<Value> {
        with_sets_as_refs(self, other, |a, b| Value::Bool(a.is_superset(b)))
            .ok_or_else(|| expected_set2(self, other))
    }
}
