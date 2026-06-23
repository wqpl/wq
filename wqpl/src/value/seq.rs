use std::sync::Arc;

use num_traits::ToPrimitive;

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

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn get(&self, idx: usize) -> Option<i64> {
        if idx >= self.len {
            return None;
        }
        let idx = i128::try_from(idx).ok()?;
        let value = i128::from(self.start).checked_add(i128::from(self.step).checked_mul(idx)?)?;
        i64::try_from(value).ok()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = i64> + '_ {
        (0..self.len).filter_map(|idx| self.get(idx))
    }

    pub(crate) fn to_vec(&self) -> Vec<i64> {
        self.iter().collect()
    }
}

pub(crate) enum ValueSeq<'a> {
    List(&'a [Value]),
    IntList(&'a [i64]),
    IntRange(&'a IntRangeData),
    BoolList(&'a [bool]),
    String(&'a str),
}

pub(crate) enum ExactIntSeq<'a> {
    Scalar(i64),
    PackedSlice(&'a [i64]),
    PackedRange(&'a IntRangeData),
    General(Vec<i64>),
}

impl<'a> ExactIntSeq<'a> {
    pub(crate) fn from_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::Int(i) => Some(Self::Scalar(*i)),
            Value::BigInt(b) => b.to_i64().map(Self::Scalar),
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
            Value::Int(i) => Some(Self::Scalar(*i)),
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
            Self::Scalar(_) => 1,
            Self::PackedSlice(items) => items.len(),
            Self::PackedRange(range) => range.len(),
            Self::General(items) => items.len(),
        }
    }

    pub(crate) fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar(_))
    }

    pub(crate) fn is_packed(&self) -> bool {
        matches!(self, Self::PackedSlice(_) | Self::PackedRange(_))
    }

    pub(crate) fn iter(&self) -> Box<dyn Iterator<Item = i64> + '_> {
        match self {
            Self::Scalar(i) => Box::new(std::iter::once(*i)),
            Self::PackedSlice(items) => Box::new(items.iter().copied()),
            Self::PackedRange(range) => Box::new(range.iter()),
            Self::General(items) => Box::new(items.iter().copied()),
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

impl<'a> ValueSeq<'a> {
    pub(crate) fn from_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::List(items) => Some(Self::List(items.as_slice())),
            Value::IntList(items) => Some(Self::IntList(items.as_slice())),
            Value::IntRange(range) => Some(Self::IntRange(range)),
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
            Self::BoolList(items) => items.len(),
            Self::String(s) => s.chars().count(),
        }
    }

    pub(crate) fn get(&self, idx: usize) -> Option<Value> {
        match self {
            Self::List(items) => items.get(idx).cloned(),
            Self::IntList(items) => items.get(idx).copied().map(Value::Int),
            Self::IntRange(range) => range.get(idx).map(Value::Int),
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

    pub(crate) fn values(&self) -> Box<dyn Iterator<Item = Value> + '_> {
        match self {
            Self::List(items) => Box::new(items.iter().cloned()),
            Self::IntList(items) => Box::new(items.iter().copied().map(Value::Int)),
            Self::IntRange(range) => Box::new(range.iter().map(Value::Int)),
            Self::BoolList(items) => Box::new(items.iter().copied().map(Value::Bool)),
            Self::String(s) => Box::new(s.chars().map(Value::Char)),
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
    Bool(Vec<bool>),
    String(String),
    General(Vec<Value>),
}

impl ValueSeqBuilder {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            state: ValueSeqBuilderState::Empty { capacity },
        }
    }

    pub(crate) fn push(&mut self, value: Value) {
        let state = std::mem::replace(
            &mut self.state,
            ValueSeqBuilderState::Empty { capacity: 0 },
        );
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
            (ValueSeqBuilderState::Empty { capacity }, Value::Char(c)) => {
                let mut s = String::with_capacity(capacity);
                s.push(c);
                ValueSeqBuilderState::String(s)
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
                let mut out = Vec::with_capacity(items.len() + 1);
                out.extend(items.into_iter().map(Value::Int));
                out.push(value);
                ValueSeqBuilderState::General(out)
            }

            (ValueSeqBuilderState::Bool(mut items), Value::Bool(b)) => {
                items.push(b);
                ValueSeqBuilderState::Bool(items)
            }
            (ValueSeqBuilderState::Bool(items), value) => {
                let mut out = Vec::with_capacity(items.len() + 1);
                out.extend(items.into_iter().map(Value::Bool));
                out.push(value);
                ValueSeqBuilderState::General(out)
            }

            (ValueSeqBuilderState::String(mut s), Value::Char(c)) => {
                s.push(c);
                ValueSeqBuilderState::String(s)
            }
            (ValueSeqBuilderState::String(s), value) => {
                let mut out = Vec::with_capacity(s.chars().count() + 1);
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
            ValueSeqBuilderState::Bool(items) => Value::BoolList(Arc::new(items)),
            ValueSeqBuilderState::String(s) => Value::String(Arc::new(s)),
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
    fn builder_promotes_homogeneous_scalars() {
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::Int(1), Value::Int(2)]),
            Value::IntList(Arc::new(vec![1, 2]))
        );
        assert_eq!(
            ValueSeqBuilder::from_items(vec![Value::Bool(true), Value::Bool(false)]),
            Value::BoolList(Arc::new(vec![true, false]))
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
    fn int_range_reads_without_materializing() {
        let range = IntRangeData::new(2, 3, 4);
        assert_eq!(range.get(0), Some(2));
        assert_eq!(range.get(3), Some(11));
        assert_eq!(range.get(4), None);
        assert_eq!(range.to_vec(), vec![2, 5, 8, 11]);
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
    fn exact_int_seq_preserves_source_shape() {
        let scalar = Value::Int(7);
        let seq = scalar.exact_int_seq().expect("int is an exact int sequence");
        assert!(seq.is_scalar());
        assert!(!seq.is_packed());
        assert_eq!(seq.to_vec(), vec![7]);

        let packed = Value::IntRange(Arc::new(IntRangeData::new(1, 2, 3)));
        let seq = packed
            .exact_int_seq()
            .expect("range is an exact int sequence");
        assert!(!seq.is_scalar());
        assert!(seq.is_packed());
        assert_eq!(seq.to_vec(), vec![1, 3, 5]);

        let general = Value::List(Arc::new(vec![
            Value::Int(1),
            Value::from_bigint(BigInt::from(2)),
        ]));
        let seq = general
            .exact_int_seq()
            .expect("list<int> is an exact int sequence");
        assert!(!seq.is_scalar());
        assert!(!seq.is_packed());
        assert_eq!(seq.to_vec(), vec![1, 2]);
    }
}
