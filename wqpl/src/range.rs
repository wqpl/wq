use std::sync::Arc;

use ordered_float::OrderedFloat;

use crate::value::seq::IntRangeData;
use crate::value::{Excerpt, Value, WqResult};
use crate::wqerror::{Requirement, WqError, WqErrorType};

const SURROGATE_START: u32 = 0xD800;
const SURROGATE_END: u32 = 0xDFFF;
const SURROGATE_LEN: u32 = SURROGATE_END - SURROGATE_START + 1;
const MAX_SCALAR_INDEX: i64 = 0x10F7FF;

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
        (Value::Char(s), Value::Char(e), None) => {
            let step = default_char_step(*s, *e);
            make_range_char(*s, *e, step, inclusive)
        }
        (Value::Char(s), Value::Char(e), Some(Value::Int(st))) => {
            make_range_char(*s, *e, *st, inclusive)
        }
        (Value::Char(_), Value::Char(_), Some(other)) => Err(WqError::new(WqErrorType::Domain)
            .expected(Requirement::INT)
            .attach_note("for a char range step")
            .got1(other)),
        (Value::Char(_), _, _) | (_, Value::Char(_), _) => Err(char_range_kind_err(start, end)),
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
        (Value::Char(s), Value::Char(n), Value::Char(e)) => {
            let step = char_scalar_index(*n) - char_scalar_index(*s);
            validate_next_char(*s, *n, *e, inclusive)?;
            make_range_char(*s, *e, step, inclusive)
        }
        (Value::Char(_), _, _) | (_, Value::Char(_), _) | (_, _, Value::Char(_)) => {
            Err(WqError::new(WqErrorType::Domain)
                .msg("char range start, next, and end must all be chars")
                .attach_note(format!(
                    "got start {} ({})",
                    start.excerpt(),
                    start.category()
                ))
                .attach_note(format!("got next {} ({})", next.excerpt(), next.category()))
                .attach_note(format!("got end {} ({})", end.excerpt(), end.category())))
        }
        _ => make_range_float_from_next(start, next, end, inclusive),
    }
}

fn default_int_step(start: i64, end: i64) -> i64 {
    if end < start { -1 } else { 1 }
}

fn validate_step_int(start: i64, end: i64, step: i64) -> WqResult<()> {
    if step == 0 {
        return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be zero"));
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
        let step = u64::try_from(step).expect("positive i64 step fits in u64");
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

fn char_range_kind_err(start: &Value, end: &Value) -> WqError {
    WqError::new(WqErrorType::Domain)
        .msg("char range endpoints must both be chars")
        .got2(start, end)
}

fn default_char_step(start: char, end: char) -> i64 {
    if char_scalar_index(end) < char_scalar_index(start) {
        -1
    } else {
        1
    }
}

fn char_scalar_index(c: char) -> i64 {
    let code = u32::from(c);
    let index = if code > SURROGATE_END {
        code - SURROGATE_LEN
    } else {
        code
    };
    i64::from(index)
}

fn char_from_scalar_index(index: i64) -> WqResult<char> {
    if !(0..=MAX_SCALAR_INDEX).contains(&index) {
        return Err(WqError::new(WqErrorType::Domain)
            .msg("char range extends beyond valid Unicode scalar values"));
    }
    let mut code = u32::try_from(index).map_err(|_| {
        WqError::new(WqErrorType::Domain)
            .msg("char range extends beyond valid Unicode scalar values")
    })?;
    if code >= SURROGATE_START {
        code += SURROGATE_LEN;
    }
    char::from_u32(code).ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .msg("char range extends beyond valid Unicode scalar values")
    })
}

fn validate_next_char(start: char, next: char, end: char, inclusive: bool) -> WqResult<()> {
    let start = char_scalar_index(start);
    let next = char_scalar_index(next);
    let end = char_scalar_index(end);
    validate_next_int(start, next, end, inclusive)
}

fn make_range_char(start: char, end: char, step: i64, inclusive: bool) -> WqResult<Value> {
    let start = char_scalar_index(start);
    let end = char_scalar_index(end);
    validate_step_int(start, end, step)?;
    let value = make_range_int(start, end, step, inclusive)?;
    let Value::IntRange(range) = value else {
        unreachable!("make_range_int returns IntRange")
    };
    let mut out = String::with_capacity(range.len());
    for index in range.iter() {
        out.push(char_from_scalar_index(index)?);
    }
    Ok(Value::String(Arc::new(out)))
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
                .expected(Requirement::one_of([
                    Requirement::finite(Requirement::INT),
                    Requirement::finite(Requirement::FLOAT),
                ]))
                .attach_note(format!("for range {label}"))
                .got1(value));
        }
    };
    if !f.is_finite() {
        return Err(WqError::new(WqErrorType::Domain)
            .expected(Requirement::one_of([
                Requirement::finite(Requirement::INT),
                Requirement::finite(Requirement::FLOAT),
            ]))
            .attach_note(format!("for range {label}"))
            .got1(value));
    }
    Ok(f)
}

fn default_float_step(start: f64, end: f64) -> f64 {
    if end < start { -1.0 } else { 1.0 }
}

fn validate_step_float(start: f64, end: f64, step: f64) -> WqResult<()> {
    if step == 0.0 {
        return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be zero"));
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
            items.push(OrderedFloat(cur));
        }
    } else {
        for i in 0..MAX_ITER {
            let cur = start + i as f64 * step;
            if if inclusive { cur < end } else { cur <= end } {
                break;
            }
            items.push(OrderedFloat(cur));
        }
    }
    Ok(Value::FloatList(Arc::new(items)))
}

#[inline]
pub(crate) fn range_alloc_len(value: &Value) -> usize {
    match value {
        Value::IntRange(items) => items.len(),
        Value::IntList(items) => items.len(),
        Value::FloatList(items) => items.len(),
        Value::BoolList(items) => items.len(),
        Value::List(items) => items.len(),
        Value::String(s) => s.chars().count(),
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
    fn float_range_uses_packed_float_list() {
        let value = make_range(&Value::float(2.5), &Value::float(5.5), None, false)
            .expect("valid float range");

        assert!(matches!(value, Value::FloatList(_)));
        assert_eq!(
            value,
            Value::FloatList(Arc::new(vec![
                OrderedFloat(2.5),
                OrderedFloat(3.5),
                OrderedFloat(4.5),
            ]))
        );

        let singleton = make_range(&Value::float(2.5), &Value::float(2.5), None, true)
            .expect("valid inclusive float range");
        assert_eq!(
            singleton,
            Value::FloatList(Arc::new(vec![OrderedFloat(2.5)]))
        );
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

    #[test]
    fn char_range_returns_string() {
        let value = make_range(&Value::Char('a'), &Value::Char('d'), None, false)
            .expect("valid char range");
        assert_eq!(value, Value::String(Arc::new("abc".to_string())));
    }

    #[test]
    fn char_range_descends() {
        let value = make_range(&Value::Char('d'), &Value::Char('a'), None, false)
            .expect("valid char range");
        assert_eq!(value, Value::String(Arc::new("dcb".to_string())));
    }

    #[test]
    fn char_range_uses_next_point() {
        let value = make_range_from_next(
            &Value::Char('a'),
            &Value::Char('c'),
            &Value::Char('h'),
            false,
        )
        .expect("valid char range");
        assert_eq!(value, Value::String(Arc::new("aceg".to_string())));
    }

    #[test]
    fn char_range_builtin_step_is_integer() {
        let value = make_range(
            &Value::Char('a'),
            &Value::Char('h'),
            Some(&Value::Int(2)),
            false,
        )
        .expect("valid char range");
        assert_eq!(value, Value::String(Arc::new("aceg".to_string())));
    }

    #[test]
    fn char_range_reports_an_int_step_requirement() {
        let error = make_range(
            &Value::Char('a'),
            &Value::Char('h'),
            Some(&Value::float(2.0)),
            false,
        )
        .expect_err("float step should fail");

        assert_eq!(error.msg.as_deref(), Some("expected int"));
        assert_eq!(
            error.notes.as_slice(),
            ["for a char range step", "got 2.0 (float)"]
        );
    }

    #[test]
    fn numeric_range_reports_the_exact_supported_types() {
        let error = make_range(&Value::Bool(true), &Value::Int(3), None, false)
            .expect_err("bool start should fail");

        assert_eq!(
            error.msg.as_deref(),
            Some("expected finite int or finite float")
        );
        assert_eq!(error.notes.as_slice(), ["for range start", "got T (bool)"]);
    }

    #[test]
    fn char_range_skips_surrogate_gap() {
        let value = make_range(
            &Value::Char('\u{D7FE}'),
            &Value::Char('\u{E001}'),
            None,
            true,
        )
        .expect("valid char range");
        assert_eq!(
            value,
            Value::String(Arc::new(
                ['\u{D7FE}', '\u{D7FF}', '\u{E000}', '\u{E001}']
                    .into_iter()
                    .collect()
            ))
        );
    }
}
