use std::sync::Arc;

use super::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnpackPathSegment {
    Index(i64),
    Key(Arc<str>),
}

#[derive(Debug)]
pub(crate) struct UnpackPathError {
    pub(crate) index: Value,
    pub(crate) target: Value,
}

pub(crate) fn extract_path(
    source: &Value,
    path: &[UnpackPathSegment],
) -> Result<Value, UnpackPathError> {
    let mut value = source.clone();
    for segment in path {
        let index = match segment {
            UnpackPathSegment::Index(index) => Value::Int(*index),
            UnpackPathSegment::Key(key) => Value::Tag(key.clone()),
        };
        value = value.index(&index).ok_or_else(|| UnpackPathError {
            index,
            target: value,
        })?;
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_nested_list_and_dict_paths() {
        let source = Value::from_items(vec![
            Value::Int(1),
            Value::Dict(Arc::new(
                [(Arc::from("key"), Value::Int(2))].into_iter().collect(),
            )),
        ]);
        let path = [
            UnpackPathSegment::Index(1),
            UnpackPathSegment::Key(Arc::from("key")),
        ];

        assert_eq!(
            extract_path(&source, &path).expect("valid path"),
            Value::Int(2)
        );
    }

    #[test]
    fn reports_the_failing_index_and_target() {
        let source = Value::from_items(vec![Value::Int(1)]);

        let err = extract_path(&source, &[UnpackPathSegment::Index(1)])
            .expect_err("out-of-range path should fail");

        assert_eq!(err.index, Value::Int(1));
        assert_eq!(err.target, source);
    }
}
