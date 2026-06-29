use std::sync::Arc;

use crate::value::seq::IntRangeData;
use crate::value::{Value, WqResult};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) fn make_range(
    start: &Value,
    end: &Value,
    step: Option<&Value>,
    inclusive: bool,
) -> WqResult<Value> {
    match (start, end, step) {
        (Value::Int(s), Value::Int(e), None) => {
            let step = default_int_step(*s, *e);
            make_range_int(*s, *e, step, inclusive)
        }
        (Value::Int(s), Value::Int(e), Some(Value::Int(st))) => {
            make_range_int(*s, *e, *st, inclusive)
        }
        _ => make_range_float(start, end, step, inclusive),
    }
}

pub(crate) fn make_range_from_next(
    start: &Value,
    next: &Value,
    end: &Value,
    inclusive: bool,
) -> WqResult<Value> {
    match (start, next, end) {
        (Value::Int(s), Value::Int(n), Value::Int(e)) => {
            let step = n
                .checked_sub(*s)
                .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("range step too large"))?;
            validate_next_int(*s, *n, *e, inclusive)?;
            make_range_int(*s, *e, step, inclusive)
        }
        _ => make_range_float_from_next(start, next, end, inclusive),
    }
}

fn default_int_step(start: i64, end: i64) -> i64 {
    if end < start { -1 } else { 1 }
}

fn validate_step_int(start: i64, end: i64, step: i64) -> WqResult<()> {
    if step == 0 {
        return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be 0"));
    }
    if start != end && ((end > start && step < 0) || (end < start && step > 0)) {
        return Err(WqError::new(WqErrorType::Domain).msg("range step points away from end"));
    }
    Ok(())
}

fn validate_next_int(start: i64, next: i64, end: i64, inclusive: bool) -> WqResult<()> {
    let step = next
        .checked_sub(start)
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("range step too large"))?;
    validate_step_int(start, end, step)?;
    let in_bounds = if step > 0 {
        if inclusive { next <= end } else { next < end }
    } else if inclusive {
        next >= end
    } else {
        next > end
    };
    if !in_bounds {
        return Err(WqError::new(WqErrorType::Domain).msg("range next point is outside the range"));
    }
    Ok(())
}

fn make_range_int(start: i64, end: i64, step: i64, inclusive: bool) -> WqResult<Value> {
    validate_step_int(start, end, step)?;
    let len = if start == end {
        usize::from(inclusive)
    } else if step > 0 {
        let diff = end.abs_diff(start);
        let step = step as u64;
        let steps = diff / step;
        let len = if inclusive || !diff.is_multiple_of(step) {
            steps.checked_add(1)
        } else {
            Some(steps)
        };
        range_len_to_usize(len)?
    } else {
        let diff = start.abs_diff(end);
        let step = step.unsigned_abs();
        let steps = diff / step;
        let len = if inclusive || !diff.is_multiple_of(step) {
            steps.checked_add(1)
        } else {
            Some(steps)
        };
        range_len_to_usize(len)?
    };
    Ok(Value::IntRange(Arc::new(IntRangeData::new(
        start, step, len,
    ))))
}

fn range_len_to_usize(len: Option<u64>) -> WqResult<usize> {
    len.and_then(|len| usize::try_from(len).ok())
        .ok_or_else(|| WqError::new(WqErrorType::Domain).msg("range too large"))
}

fn numeric_as_f64(value: &Value, label: &str) -> WqResult<f64> {
    let f = match value {
        Value::Int(n) => *n as f64,
        Value::Float(f) => **f,
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg(format!("expected number for range {label}"))
                .got1(value));
        }
    };
    if !f.is_finite() {
        return Err(WqError::new(WqErrorType::Domain)
            .msg(format!("range {label} must be finite"))
            .got1(value));
    }
    Ok(f)
}

fn default_float_step(start: f64, end: f64) -> f64 {
    if end < start { -1.0 } else { 1.0 }
}

fn validate_step_float(start: f64, end: f64, step: f64) -> WqResult<()> {
    if step == 0.0 {
        return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be 0"));
    }
    if !step.is_finite() {
        return Err(WqError::new(WqErrorType::Domain).msg("range step must be finite"));
    }
    if start != end && ((end > start && step < 0.0) || (end < start && step > 0.0)) {
        return Err(WqError::new(WqErrorType::Domain).msg("range step points away from end"));
    }
    Ok(())
}

fn validate_next_float(start: f64, next: f64, end: f64, inclusive: bool) -> WqResult<f64> {
    let step = next - start;
    validate_step_float(start, end, step)?;
    let in_bounds = if step > 0.0 {
        if inclusive { next <= end } else { next < end }
    } else if inclusive {
        next >= end
    } else {
        next > end
    };
    if !in_bounds {
        return Err(WqError::new(WqErrorType::Domain).msg("range next point is outside the range"));
    }
    Ok(step)
}

fn make_range_float(
    start: &Value,
    end: &Value,
    step: Option<&Value>,
    inclusive: bool,
) -> WqResult<Value> {
    let start_f = numeric_as_f64(start, "start")?;
    let end_f = numeric_as_f64(end, "end")?;
    let step_f = match step {
        Some(value) => numeric_as_f64(value, "step")?,
        None => default_float_step(start_f, end_f),
    };
    make_range_float_with_step(start_f, end_f, step_f, inclusive)
}

fn make_range_float_from_next(
    start: &Value,
    next: &Value,
    end: &Value,
    inclusive: bool,
) -> WqResult<Value> {
    let start_f = numeric_as_f64(start, "start")?;
    let next_f = numeric_as_f64(next, "next")?;
    let end_f = numeric_as_f64(end, "end")?;
    let step_f = validate_next_float(start_f, next_f, end_f, inclusive)?;
    make_range_float_with_step(start_f, end_f, step_f, inclusive)
}

fn make_range_float_with_step(start: f64, end: f64, step: f64, inclusive: bool) -> WqResult<Value> {
    validate_step_float(start, end, step)?;
    let mut items = Vec::new();
    const MAX_ITER: usize = 10_000_000;
    if step > 0.0 {
        for i in 0..MAX_ITER {
            let cur = start + i as f64 * step;
            if if inclusive { cur > end } else { cur >= end } {
                break;
            }
            items.push(Value::float(cur));
        }
    } else {
        for i in 0..MAX_ITER {
            let cur = start + i as f64 * step;
            if if inclusive { cur < end } else { cur <= end } {
                break;
            }
            items.push(Value::float(cur));
        }
    }
    Ok(Value::List(Arc::new(items)))
}

#[inline]
pub(crate) fn range_alloc_len(value: &Value) -> usize {
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
        let value =
            make_range(&Value::Int(1), &Value::Int(5), None, false).expect("valid integer range");

        assert!(matches!(value, Value::IntRange(_)));
        assert_eq!(value.to_string(), "(1;2;3;4)");
        assert_eq!(value, Value::IntList(Arc::new(vec![1, 2, 3, 4])));
        assert_eq!(value.index(&Value::Int(2)), Some(Value::Int(3)));
    }

    #[test]
    fn two_point_range_infers_descending_step() {
        let value =
            make_range(&Value::Int(5), &Value::Int(1), None, false).expect("valid integer range");
        assert_eq!(value.to_string(), "(5;4;3;2)");
    }

    #[test]
    fn three_point_range_uses_next_point() {
        let value = make_range_from_next(&Value::Int(1), &Value::Int(3), &Value::Int(10), false)
            .expect("valid integer range");
        assert_eq!(value.to_string(), "(1;3;5;7;9)");
    }

    #[test]
    fn three_point_range_rejects_next_past_half_open_end() {
        let err = make_range_from_next(&Value::Int(1), &Value::Int(3), &Value::Int(3), false)
            .expect_err("next is not emitted");
        assert!(err.to_string().contains("range next point is outside"));
    }
}
