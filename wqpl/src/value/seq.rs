use std::sync::Arc;

use crate::value::Value;

pub(crate) enum ValueSeq<'a> {
    List(&'a [Value]),
    IntList(&'a [i64]),
    String(&'a str),
}

impl<'a> ValueSeq<'a> {
    pub(crate) fn from_value(value: &'a Value) -> Option<Self> {
        match value {
            Value::List(items) => Some(Self::List(items.as_slice())),
            Value::IntList(items) => Some(Self::IntList(items.as_slice())),
            Value::String(s) => Some(Self::String(s.as_str())),
            _ => None,
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            Self::List(items) => items.len(),
            Self::IntList(items) => items.len(),
            Self::String(s) => s.chars().count(),
        }
    }

    pub(crate) fn get(&self, idx: usize) -> Option<Value> {
        match self {
            Self::List(items) => items.get(idx).cloned(),
            Self::IntList(items) => items.get(idx).copied().map(Value::Int),
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
}

pub(crate) struct ValueSeqBuilder {
    state: ValueSeqBuilderState,
}

enum ValueSeqBuilderState {
    Empty { capacity: usize },
    Int(Vec<i64>),
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
}
