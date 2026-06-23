use std::sync::Arc;

use crate::value::seq::IntRangeData;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(super) fn make_range(
    start: &Value,
    end: &Value,
    step: Option<&Value>,
    inclusive: bool,
) -> WqResult<Value> {
    match (start, end, step) {
        (Value::Int(s), Value::Int(e), None) => make_range_int(*s, *e, 1, inclusive),
        (Value::Int(s), Value::Int(e), Some(Value::Int(st))) => {
            make_range_int(*s, *e, *st, inclusive)
        }
        _ => make_range_float(start, end, step, inclusive),
    }
}

fn make_range_int(start: i64, end: i64, step: i64, inclusive: bool) -> WqResult<Value> {
    if step == 0 {
        return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be 0"));
    }
    let len = if step > 0 && end >= start {
        let diff = end.abs_diff(start);
        let steps = diff / step as u64;
        let len = if inclusive || !diff.is_multiple_of(step as u64) {
            steps.checked_add(1)
        } else {
            Some(steps)
        };
        range_len_to_usize(len)?
    } else if step < 0 && start >= end {
        let diff = start.abs_diff(end);
        let steps = diff / step.unsigned_abs();
        let len = if inclusive || !diff.is_multiple_of(step.unsigned_abs()) {
            steps.checked_add(1)
        } else {
            Some(steps)
        };
        range_len_to_usize(len)?
    } else {
        0
    };
    Ok(Value::IntRange(Arc::new(IntRangeData::new(
        start, step, len,
    ))))
}

fn range_len_to_usize(len: Option<u64>) -> WqResult<usize> {
    len.and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("range too large"))
}

fn make_range_float(
    start: &Value,
    end: &Value,
    step: Option<&Value>,
    inclusive: bool,
) -> WqResult<Value> {
    let start_f = match start {
        Value::Int(n) => *n as f64,
        Value::Float(f) => **f,
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("expected number for range start")
                .got1(start));
        }
    };
    let end_f = match end {
        Value::Int(n) => *n as f64,
        Value::Float(f) => **f,
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("expected number for range end")
                .got1(end));
        }
    };
    let step_f = match step {
        Some(Value::Int(n)) => *n as f64,
        Some(Value::Float(f)) => **f,
        Some(other) => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("expected number for range step")
                .got1(other));
        }
        None => 1.0,
    };
    if step_f == 0.0 {
        return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be 0"));
    }
    let mut items = Vec::new();
    const MAX_ITER: usize = 10_000_000;
    if step_f > 0.0 {
        for i in 0..MAX_ITER {
            let cur = start_f + i as f64 * step_f;
            if if inclusive { cur > end_f } else { cur >= end_f } {
                break;
            }
            items.push(Value::float(cur));
        }
    } else {
        for i in 0..MAX_ITER {
            let cur = start_f + i as f64 * step_f;
            if if inclusive { cur < end_f } else { cur <= end_f } {
                break;
            }
            items.push(Value::float(cur));
        }
    }
    Ok(Value::List(Arc::new(items)))
}

#[inline]
pub(super) fn range_alloc_len(value: &Value) -> usize {
    match value {
        Value::IntRange(items) => items.len(),
        Value::IntList(items) => items.len(),
        Value::List(items) => items.len(),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_range_stays_virtual_but_displays_as_list() {
        let value = make_range(&Value::Int(1), &Value::Int(5), None, false)
            .expect("valid integer range");

        assert!(matches!(value, Value::IntRange(_)));
        assert_eq!(value.to_string(), "(1;2;3;4)");
        assert_eq!(value, Value::IntList(Arc::new(vec![1, 2, 3, 4])));
        assert_eq!(value.index(&Value::Int(2)), Some(Value::Int(3)));
    }
}
