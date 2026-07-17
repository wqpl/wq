use num_traits::ToPrimitive;

use crate::value::Value;
use crate::wqerror::{Bound, Requirement, WqError, WqErrorType};

pub(crate) fn expected_numeric1(value: &Value) -> WqError {
    expected1(Requirement::NUMBER, value)
}

pub(crate) fn expected_numeric2(lhs: &Value, rhs: &Value) -> WqError {
    expected2(Requirement::NUMBER, lhs, rhs)
}

pub(crate) fn expected_integer1(value: &Value) -> WqError {
    expected1(Requirement::INT, value)
}

pub(crate) fn expected_integer2(lhs: &Value, rhs: &Value) -> WqError {
    expected2(Requirement::INT, lhs, rhs)
}

pub(crate) fn expected_bool1(value: &Value) -> WqError {
    expected1(Requirement::BOOL, value)
}

pub(crate) fn expected_bool2(lhs: &Value, rhs: &Value) -> WqError {
    expected2(Requirement::BOOL, lhs, rhs)
}

pub(crate) fn expected_string1(value: &Value) -> WqError {
    let error = WqError::new(WqErrorType::Domain).expected(Requirement::one_of([
        Requirement::CHAR,
        Requirement::STRING,
    ]));
    let Value::List(items) = value else {
        return error.got1(value);
    };
    if let Some((index, item)) = items
        .iter()
        .enumerate()
        .find(|(_, item)| !matches!(item, Value::Char(_)))
    {
        error.got_at_index(item, index)
    } else {
        error.got1(value)
    }
}

pub(crate) fn expected_bytes1(value: &Value) -> WqError {
    let byte = Requirement::int_range(Bound::Included(0), Bound::Included(255));
    let error = WqError::new(WqErrorType::Domain)
        .expected(Requirement::one_of([byte.clone(), Requirement::list(byte)]));
    match value {
        Value::IntList(_) | Value::IntRange(_) => {
            let invalid = value
                .packed_int_seq()
                .expect("list<int> and int-range are packed int sequences")
                .iter()
                .enumerate()
                .find(|(_, item)| u8::try_from(*item).is_err());
            if let Some((index, item)) = invalid {
                error.got_at_index(&Value::Int(item), index)
            } else {
                error.got1(value)
            }
        }
        Value::List(items) => {
            let invalid = items.iter().enumerate().find(|(_, item)| !is_byte(item));
            if let Some((index, item)) = invalid {
                error.got_at_index(item, index)
            } else {
                error.got1(value)
            }
        }
        _ => error.got1(value),
    }
}

fn expected1(expected: Requirement, value: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .expected(expected)
        .got1(value)
}

fn expected2(expected: Requirement, lhs: &Value, rhs: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .expected(expected)
        .got2(lhs, rhs)
}

fn is_byte(value: &Value) -> bool {
    match value {
        Value::Int(value) => u8::try_from(*value).is_ok(),
        Value::BigInt(value) => value.to_u8().is_some(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn string_error_reports_the_first_non_char_item() {
        let value = Value::List(Arc::new(vec![Value::Char('a'), Value::Int(2)]));

        let error = expected_string1(&value);

        assert_eq!(error.msg.as_deref(), Some("expected char or string"));
        assert_eq!(error.notes.as_slice(), ["at index 1", "got 2 (int)"]);
    }

    #[test]
    fn byte_error_reports_the_first_out_of_range_item() {
        let value = Value::IntList(Arc::new(vec![0, 256, -1]));

        let error = expected_bytes1(&value);

        assert_eq!(
            error.msg.as_deref(),
            Some("expected int from 0 through 255 or list of ints from 0 through 255")
        );
        assert_eq!(error.notes.as_slice(), ["at index 1", "got 256 (int)"]);
    }
}
