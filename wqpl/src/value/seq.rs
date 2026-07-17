use std::sync::Arc;

use num_traits::ToPrimitive;
use ordered_float::OrderedFloat;

use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntRangeData {
    start: i64,
    step: i64,
    len: usize,
}

impl IntRangeData {
    pub(crate) fn new(start: i64, step: i64, len: usize) -> Self {
        debug_assert!(step != 0);
        Self { start, step, len }
    }

    pub(crate) fn start(&self) -> i64 {
        self.start
    }

    pub(crate) fn step(&self) -> i64 {
        self.step
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn last_value(&self) -> Option<i64> {
        self.len.checked_sub(1).and_then(|idx| self.get(idx))
    }

    pub(crate) fn get(&self, idx: usize) -> Option<i64> {
        if idx >= self.len {
            return None;
        }
        let idx = i128::try_from(idx).ok()?;
        let value = i128::from(self.start).checked_add(i128::from(self.step).checked_mul(idx)?)?;
        i64::try_from(value).ok()
    }

    pub(crate) fn iter(&self) -> IntRangeIter {
        IntRangeIter {
            next: self.start,
            back: self.last_value().unwrap_or(self.start),
            step: self.step,
            remaining: self.len,
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }

    pub(crate) fn reversed(&self) -> Option<Self> {
        if self.len <= 1 {
            return Some(self.clone());
        }
        Some(Self::new(
            self.last_value()?,
            self.step.checked_neg()?,
            self.len,
        ))
    }
}

#[derive(Clone)]
pub(crate) struct IntRangeIter {
    next: i64,
    back: i64,
    step: i64,
    remaining: usize,
}

pub(crate) enum ExactIntSeqIter<'a> {
    Atom(std::iter::Once<i64>),
    Slice(std::iter::Copied<std::slice::Iter<'a, i64>>),
    Range(IntRangeIter),
}

impl DoubleEndedIterator for IntRangeIter {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let value = self.back;
        self.remaining -= 1;
        if self.remaining > 0 {
            debug_assert!(self.back.checked_sub(self.step).is_some());
            self.back = self.back.wrapping_sub(self.step);
        }
        Some(value)
    }
}

impl Iterator for ExactIntSeqIter<'_> {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Atom(iter) => iter.next(),
            Self::Slice(iter) => iter.next(),
            Self::Range(iter) => iter.next(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::Atom(iter) => iter.size_hint(),
            Self::Slice(iter) => iter.size_hint(),
            Self::Range(iter) => iter.size_hint(),
        }
    }
}

impl ExactSizeIterator for ExactIntSeqIter<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Atom(iter) => iter.len(),
            Self::Slice(iter) => iter.len(),
            Self::Range(iter) => iter.len(),
        }
    }
}

impl DoubleEndedIterator for ExactIntSeqIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Atom(iter) => iter.next_back(),
            Self::Slice(iter) => iter.next_back(),
            Self::Range(iter) => iter.next_back(),
        }
    }
}

pub(crate) enum ValueSeqIter<'a> {
    List(std::iter::Cloned<std::slice::Iter<'a, Value>>),
    Int(ExactIntSeqIter<'a>),
    Float(std::iter::Copied<std::slice::Iter<'a, OrderedFloat<f64>>>),
    Bool(std::iter::Copied<std::slice::Iter<'a, bool>>),
    String(std::str::Chars<'a>),
}

impl Iterator for ValueSeqIter<'_> {
    type Item = Value;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::List(iter) => iter.next(),
            Self::Int(iter) => iter.next().map(Value::Int),
            Self::Float(iter) => iter.next().map(Value::Float),
            Self::Bool(iter) => iter.next().map(Value::Bool),
            Self::String(iter) => iter.next().map(Value::Char),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            Self::List(iter) => iter.size_hint(),
            Self::Int(iter) => iter.size_hint(),
            Self::Float(iter) => iter.size_hint(),
            Self::Bool(iter) => iter.size_hint(),
            Self::String(iter) => iter.size_hint(),
        }
    }
}

impl DoubleEndedIterator for ValueSeqIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::List(iter) => iter.next_back(),
            Self::Int(iter) => iter.next_back().map(Value::Int),
            Self::Float(iter) => iter.next_back().map(Value::Float),
            Self::Bool(iter) => iter.next_back().map(Value::Bool),
            Self::String(iter) => iter.next_back().map(Value::Char),
        }
    }
}

impl Iterator for IntRangeIter {
    type Item = i64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }

        let value = self.next;
        self.remaining -= 1;
        if self.remaining > 0 {
            debug_assert!(self.next.checked_add(self.step).is_some());
            self.next = self.next.wrapping_add(self.step);
        }
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for IntRangeIter {
    fn len(&self) -> usize {
        self.remaining
    }
}

/// Borrowed view over every public list representation.
///
/// Use [`ListStorageSeq`] instead when strings must remain single values rather
/// than expand into chars, such as generic list insertion.
pub(crate) enum ValueSeq<'a> {
    List(&'a [Value]),
    IntList(&'a [i64]),
    IntRange(&'a IntRangeData),
    FloatList(&'a [OrderedFloat<f64>]),
    BoolList(&'a [bool]),
    String(&'a str),
}

pub(crate) enum ExactIntSeq<'a> {
    Atom(i64),
    PackedSlice(&'a [i64]),
    PackedRange(&'a IntRangeData),
    General(Vec<i64>),
}

/// Sequence view for list storage variants that should expand as list items.
///
/// Unlike [`ValueSeq`], this intentionally excludes strings: strings are
/// sequence-like for indexing and broadcasting, but list insertion and generic
/// list mutation treat them as atom values unless a string-specific path says
/// otherwise.
pub(crate) enum ListStorageSeq<'a> {
    List(&'a [Value]),
    Int(ExactIntSeq<'a>),
    Float(&'a [OrderedFloat<f64>]),
    Bool(&'a [bool]),
}

impl<'a> ExactIntSeq<'a> {
    pub(crate) fn from_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::Int(i) => Some(Self::Atom(*i)),
            Value::BigInt(b) => b.to_i64().map(Self::Atom),
            Value::IntList(_) | Value::IntRange(_) => Self::from_packed_value(value),
            Value::List(items) => items
                .iter()
                .map(exact_int_atom)
                .collect::<Option<Vec<_>>>()
                .map(Self::General),
            _ => None,
        }
    }

    pub(crate) fn from_native_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::Int(i) => Some(Self::Atom(*i)),
            Value::IntList(_) | Value::IntRange(_) => Self::from_packed_value(value),
            _ => None,
        }
    }

    pub(crate) fn from_packed_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::IntList(items) => Some(Self::PackedSlice(items.as_slice())),
            Value::IntRange(range) => Some(Self::PackedRange(range)),
            _ => None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::Atom(_) => 1,
            Self::PackedSlice(items) => items.len(),
            Self::PackedRange(range) => range.len(),
            Self::General(items) => items.len(),
        }
    }

    pub(crate) fn is_atom(&self) -> bool {
        matches!(self, Self::Atom(_))
    }

    pub(crate) fn is_packed(&self) -> bool {
        matches!(self, Self::PackedSlice(_) | Self::PackedRange(_))
    }

    pub(crate) fn iter(&self) -> ExactIntSeqIter<'_> {
        match self {
            Self::Atom(i) => ExactIntSeqIter::Atom(std::iter::once(*i)),
            Self::PackedSlice(items) => ExactIntSeqIter::Slice(items.iter().copied()),
            Self::General(items) => ExactIntSeqIter::Slice(items.iter().copied()),
            Self::PackedRange(range) => ExactIntSeqIter::Range(range.iter()),
        }
    }

    pub(crate) fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }
}

fn exact_int_atom(value: &Value) -> Option<i64> {
    match value {
        Value::Int(i) => Some(*i),
        Value::BigInt(b) => b.to_i64(),
        _ => None,
    }
}

impl Value {
    pub(crate) fn exact_int_seq(&self) -> Option<ExactIntSeq<'_>> {
        ExactIntSeq::from_value(self)
    }

    pub(crate) fn native_int_seq(&self) -> Option<ExactIntSeq<'_>> {
        ExactIntSeq::from_native_value(self)
    }

    pub(crate) fn packed_int_seq(&self) -> Option<ExactIntSeq<'_>> {
        let seq = ExactIntSeq::from_packed_value(self)?;
        debug_assert!(seq.is_packed());
        Some(seq)
    }
}

impl<'a> ListStorageSeq<'a> {
    pub(crate) fn from_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::List(items) => Some(Self::List(items.as_slice())),
            Value::IntList(_) | Value::IntRange(_) => {
                Some(Self::Int(ExactIntSeq::from_packed_value(value)?))
            }
            Value::FloatList(items) => Some(Self::Float(items.as_slice())),
            Value::BoolList(items) => Some(Self::Bool(items.as_slice())),
            _ => None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::List(items) => items.len(),
            Self::Int(items) => items.len(),
            Self::Float(items) => items.len(),
            Self::Bool(items) => items.len(),
        }
    }

    pub(crate) fn values(&self) -> ValueSeqIter<'_> {
        match self {
            Self::List(items) => ValueSeqIter::List(items.iter().cloned()),
            Self::Int(items) => ValueSeqIter::Int(items.iter()),
            Self::Float(items) => ValueSeqIter::Float(items.iter().copied()),
            Self::Bool(items) => ValueSeqIter::Bool(items.iter().copied()),
        }
    }

    pub(crate) fn extend_values(&self, out: &mut Vec<Value>) {
        match self {
            Self::List(items) => out.extend(items.iter().cloned()),
            Self::Int(items) => out.extend(items.iter().map(Value::Int)),
            Self::Float(items) => out.extend(items.iter().copied().map(Value::Float)),
            Self::Bool(items) => out.extend(items.iter().copied().map(Value::Bool)),
        }
    }

    pub(crate) fn to_values_vec(&self) -> Vec<Value> {
        let mut out = Vec::with_capacity(self.len());
        self.extend_values(&mut out);
        out
    }
}

impl<'a> ValueSeq<'a> {
    pub(crate) fn from_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::List(items) => Some(Self::List(items.as_slice())),
            Value::IntList(items) => Some(Self::IntList(items.as_slice())),
            Value::IntRange(range) => Some(Self::IntRange(range)),
            Value::FloatList(items) => Some(Self::FloatList(items.as_slice())),
            Value::BoolList(items) => Some(Self::BoolList(items.as_slice())),
            Value::String(s) => Some(Self::String(s.as_str())),
            _ => None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::List(items) => items.len(),
            Self::IntList(items) => items.len(),
            Self::IntRange(range) => range.len(),
            Self::FloatList(items) => items.len(),
            Self::BoolList(items) => items.len(),
            Self::String(s) => s.chars().count(),
        }
    }

    pub(crate) fn get(&self, idx: usize) -> Option<Value> {
        match self {
            Self::List(items) => items.get(idx).cloned(),
            Self::IntList(items) => items.get(idx).copied().map(Value::Int),
            Self::IntRange(range) => range.get(idx).map(Value::Int),
            Self::FloatList(items) => items.get(idx).copied().map(Value::Float),
            Self::BoolList(items) => items.get(idx).copied().map(Value::Bool),
            Self::String(s) => s.chars().nth(idx).map(Value::Char),
        }
    }

    pub(crate) fn gather(&self, indices: &[usize]) -> Option<Value> {
        match self {
            Self::List(items) => {
                let mut out = Vec::with_capacity(indices.len());
                for &idx in indices {
                    out.push(items.get(idx)?.clone());
                }
                Some(Value::from_items(out))
            }
            Self::IntList(items) => {
                let mut out = Vec::with_capacity(indices.len());
                for &idx in indices {
                    out.push(*items.get(idx)?);
                }
                Some(Value::IntList(Arc::new(out)))
            }
            Self::IntRange(range) => {
                let mut out = Vec::with_capacity(indices.len());
                for &idx in indices {
                    out.push(range.get(idx)?);
                }
                Some(Value::IntList(Arc::new(out)))
            }
            Self::FloatList(items) => {
                let mut out = Vec::with_capacity(indices.len());
                for &idx in indices {
                    out.push(*items.get(idx)?);
                }
                Some(Value::FloatList(Arc::new(out)))
            }
            Self::BoolList(items) => {
                let mut out = Vec::with_capacity(indices.len());
                for &idx in indices {
                    out.push(*items.get(idx)?);
                }
                Some(Value::BoolList(Arc::new(out)))
            }
            Self::String(s) => {
                let chars = s.chars().collect::<Vec<_>>();
                let mut out = String::with_capacity(indices.len());
                for &idx in indices {
                    out.push(*chars.get(idx)?);
                }
                Some(Value::String(Arc::new(out)))
            }
        }
    }

    pub(crate) fn values(&self) -> ValueSeqIter<'_> {
        match self {
            Self::List(items) => ValueSeqIter::List(items.iter().cloned()),
            Self::IntList(items) => {
                ValueSeqIter::Int(ExactIntSeqIter::Slice(items.iter().copied()))
            }
            Self::IntRange(range) => ValueSeqIter::Int(ExactIntSeqIter::Range(range.iter())),
            Self::FloatList(items) => ValueSeqIter::Float(items.iter().copied()),
            Self::BoolList(items) => ValueSeqIter::Bool(items.iter().copied()),
            Self::String(s) => ValueSeqIter::String(s.chars()),
        }
    }

    pub(crate) fn eq_values(&self, other: &Self) -> bool {
        self.len() == other.len() && self.values().zip(other.values()).all(|(x, y)| x == y)
    }
}

pub(crate) struct ValueSeqBuilder {
    state: ValueSeqBuilderState,
}

enum ValueSeqBuilderState {
    Empty { capacity: usize },
    Int(Vec<i64>),
    Float(Vec<OrderedFloat<f64>>),
    Bool(Vec<bool>),
    String { value: String, item_capacity: usize },
    General(Vec<Value>),
}

impl ValueSeqBuilder {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            state: ValueSeqBuilderState::Empty { capacity },
        }
    }

    pub(crate) fn push(&mut self, value: Value) {
        let state = std::mem::replace(&mut self.state, ValueSeqBuilderState::Empty { capacity: 0 });
        self.state = match (state, value) {
            (ValueSeqBuilderState::Empty { capacity }, Value::Int(i)) => {
                let mut items = Vec::with_capacity(capacity);
                items.push(i);
                ValueSeqBuilderState::Int(items)
            }
            (ValueSeqBuilderState::Empty { capacity }, Value::Bool(b)) => {
                let mut items = Vec::with_capacity(capacity);
                items.push(b);
                ValueSeqBuilderState::Bool(items)
            }
            (ValueSeqBuilderState::Empty { capacity }, Value::Float(f)) => {
                let mut items = Vec::with_capacity(capacity);
                items.push(f);
                ValueSeqBuilderState::Float(items)
            }
            (ValueSeqBuilderState::Empty { capacity }, Value::Char(c)) => {
                let mut s = String::with_capacity(capacity);
                s.push(c);
                ValueSeqBuilderState::String {
                    value: s,
                    item_capacity: capacity,
                }
            }
            (ValueSeqBuilderState::Empty { capacity }, value) => {
                let mut items = Vec::with_capacity(capacity);
                items.push(value);
                ValueSeqBuilderState::General(items)
            }

            (ValueSeqBuilderState::Int(mut items), Value::Int(i)) => {
                items.push(i);
                ValueSeqBuilderState::Int(items)
            }
            (ValueSeqBuilderState::Int(items), value) => {
                let mut out = Vec::with_capacity(items.capacity().max(items.len() + 1));
                out.extend(items.into_iter().map(Value::Int));
                out.push(value);
                ValueSeqBuilderState::General(out)
            }

            (ValueSeqBuilderState::Float(mut items), Value::Float(f)) => {
                items.push(f);
                ValueSeqBuilderState::Float(items)
            }
            (ValueSeqBuilderState::Float(items), value) => {
                let mut out = Vec::with_capacity(items.capacity().max(items.len() + 1));
                out.extend(items.into_iter().map(Value::Float));
                out.push(value);
                ValueSeqBuilderState::General(out)
            }

            (ValueSeqBuilderState::Bool(mut items), Value::Bool(b)) => {
                items.push(b);
                ValueSeqBuilderState::Bool(items)
            }
            (ValueSeqBuilderState::Bool(items), value) => {
                let mut out = Vec::with_capacity(items.capacity().max(items.len() + 1));
                out.extend(items.into_iter().map(Value::Bool));
                out.push(value);
                ValueSeqBuilderState::General(out)
            }

            (
                ValueSeqBuilderState::String {
                    mut value,
                    item_capacity,
                },
                Value::Char(c),
            ) => {
                value.push(c);
                ValueSeqBuilderState::String {
                    value,
                    item_capacity,
                }
            }
            (
                ValueSeqBuilderState::String {
                    value: s,
                    item_capacity,
                },
                value,
            ) => {
                let char_len = s.chars().count();
                let mut out = Vec::with_capacity(item_capacity.max(char_len + 1));
                out.extend(s.chars().map(Value::Char));
                out.push(value);
                ValueSeqBuilderState::General(out)
            }

            (ValueSeqBuilderState::General(mut items), value) => {
                items.push(value);
                ValueSeqBuilderState::General(items)
            }
        };
    }

    pub(crate) fn finish(self) -> Value {
        match self.state {
            ValueSeqBuilderState::Empty { .. } => Value::unit(),
            ValueSeqBuilderState::Int(items) => Value::IntList(Arc::new(items)),
            ValueSeqBuilderState::Float(items) => Value::FloatList(Arc::new(items)),
            ValueSeqBuilderState::Bool(items) => Value::BoolList(Arc::new(items)),
            ValueSeqBuilderState::String { value, .. } => Value::String(Arc::new(value)),
            ValueSeqBuilderState::General(items) => Value::List(Arc::new(items)),
        }
    }

    pub(crate) fn from_items(items: Vec<Value>) -> Value {
        let mut builder = Self::with_capacity(items.len());
        for item in items {
            builder.push(item);
        }
        builder.finish()
    }
}

#[cfg(test)]
mod tests {
    use num_bigint::BigInt;

    use super::*;

    #[test]
    fn builder_preserves_empty_unit() {
        assert_eq!(
            ValueSeqBuilder::from_items(vec![]),
            Value::IntList(Arc::new(vec![]))
        );
    }

    #[test]
    fn builder_promotes_homogeneous_atoms() {
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::Int(1), Value::Int(2)]),
            Value::IntList(Arc::new(vec![1, 2]))
        );
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::Bool(true), Value::Bool(false)]),
            Value::BoolList(Arc::new(vec![true, false]))
        );
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::float(1.5), Value::float(2.5)]),
            Value::FloatList(Arc::new(vec![OrderedFloat(1.5), OrderedFloat(2.5)]))
        );
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::Char('a'), Value::Char('b')]),
            Value::String(Arc::new("ab".to_owned()))
        );
    }

    #[test]
    fn builder_widens_mixed_values() {
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::Int(1), Value::Char('a')]),
            Value::List(Arc::new(vec![Value::Int(1), Value::Char('a')]))
        );
    }

    #[test]
    fn builder_preserves_reserved_capacity_when_widening() {
        let mut builder = ValueSeqBuilder::with_capacity(32);
        builder.push(Value::Int(1));
        builder.push(Value::Char('a'));
        let Value::List(items) = builder.finish() else {
            unreachable!("mixed values should use general list storage");
        };
        assert!(items.capacity() >= 32);
    }

    #[test]
    fn int_range_reads_without_materializing() {
        let range = IntRangeData::new(2, 3, 4);
        assert_eq!(range.start(), 2);
        assert_eq!(range.step(), 3);
        assert_eq!(range.get(0), Some(2));
        assert_eq!(range.get(3), Some(11));
        assert_eq!(range.get(4), None);
        assert_eq!(range.last_value(), Some(11));
        assert_eq!(range.to_vec(), vec![2, 5, 8, 11]);

        let mut iter = range.iter();
        assert_eq!(iter.next(), Some(2));
        assert_eq!(iter.next_back(), Some(11));
        assert_eq!(iter.collect::<Vec<_>>(), vec![5, 8]);
    }

    #[test]
    fn int_range_gathers_as_intlist() {
        let value = Value::IntRange(Arc::new(IntRangeData::new(10, -2, 4)));
        let seq = ValueSeq::from_value(&value).expect("range is sequence-like");
        assert_eq!(
            seq.gather(&[0, 2, 3]),
            Some(Value::IntList(Arc::new(vec![10, 6, 4])))
        );
    }

    #[test]
    fn bool_list_reads_without_widening() {
        let value = Value::BoolList(Arc::new(vec![true, false, true]));
        let seq = ValueSeq::from_value(&value).expect("bool-list is sequence-like");

        assert_eq!(seq.get(1), Some(Value::Bool(false)));
        assert_eq!(
            seq.gather(&[0, 2]),
            Some(Value::BoolList(Arc::new(vec![true, true])))
        );
    }

    #[test]
    fn list_storage_seq_expands_list_storage_but_excludes_strings() {
        let bools = Value::BoolList(Arc::new(vec![true, false]));
        let seq = ListStorageSeq::from_value(&bools).expect("bool-list is list storage");
        assert_eq!(
            seq.to_values_vec(),
            vec![Value::Bool(true), Value::Bool(false)]
        );

        let ints = Value::IntRange(Arc::new(IntRangeData::new(2, 3, 3)));
        let seq = ListStorageSeq::from_value(&ints).expect("int-range is list storage");
        assert_eq!(
            seq.to_values_vec(),
            vec![Value::Int(2), Value::Int(5), Value::Int(8)]
        );

        let string = Value::String(Arc::new("ab".to_owned()));
        assert!(ListStorageSeq::from_value(&string).is_none());
    }

    #[test]
    fn exact_int_seq_preserves_source_shape() {
        let atom = Value::Int(7);
        let seq = atom.exact_int_seq().expect("int is an exact int sequence");
        assert!(seq.is_atom());
        assert!(!seq.is_packed());
        assert_eq!(seq.to_vec(), vec![7]);

        let packed = Value::IntRange(Arc::new(IntRangeData::new(1, 2, 3)));
        let seq = packed
            .exact_int_seq()
            .expect("range is an exact int sequence");
        assert!(!seq.is_atom());
        assert!(seq.is_packed());
        assert_eq!(seq.to_vec(), vec![1, 3, 5]);

        let general = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::from_bigint(BigInt::from(2)),
        ]));
        let seq = general
            .exact_int_seq()
            .expect("int-list is an exact int sequence");
        assert!(!seq.is_atom());
        assert!(!seq.is_packed());
        assert_eq!(seq.to_vec(), vec![1, 2]);
    }
}
