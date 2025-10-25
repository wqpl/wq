use std::borrow::Cow;

use crate::{
    builtins::{
        BuiltinEnum as BE,
        wqerr_ext::{check_arity, type_mismatch},
    },
    value::{IntoWqValue, Value, WqResult},
    vm::Vm,
    wqerr::{WqErr, WqErrType},
};

use unicode_segmentation::UnicodeSegmentation;

pub fn to_str(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Str, [1], args)?;
    let arg = &args[0];
    if arg.is_str() {
        return Ok(arg.clone());
    }
    Ok(arg.to_string().into_wq_value())
}

pub fn fmt(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    // Append Value to an output Vec<Value::Char(..)>, avoiding extra allocations when possible.
    fn push_value_as_chars(out: &mut Vec<Value>, v: &Value) {
        match v.as_char_list() {
            Some(Cow::Borrowed(s)) => out.extend_from_slice(s),
            Some(Cow::Owned(mut v)) => out.append(&mut v),
            None => out.extend(v.to_string().chars().map(Value::Char)),
        }
    }

    // Count "{}" placeholders in a list-of-chars format, respecting "{{" and "}}".
    // Replace count_placeholders(...) with this:
    fn count_placeholders(fmt_chars: &[Value]) -> WqResult<usize> {
        let mut i = 0usize;
        let mut count = 0usize;

        while i < fmt_chars.len() {
            let ch = match fmt_chars[i] {
                Value::Char(c) => c,
                _ => unreachable!(), // fmt validated earlier to be list<char>
            };

            if ch == '{' {
                match fmt_chars.get(i + 1) {
                    Some(Value::Char('{')) => i += 2, // "{{" -> literal
                    Some(Value::Char('}')) => {
                        // "{}" -> placeholder
                        count += 1;
                        i += 2;
                    }
                    _ => {
                        return Err(WqErr::new(WqErrType::Domain)
                            .src(BE::Fmt)
                            .msg("unescaped '{'; use '{{' for a literal or '{}' for a placeholder")
                            .attach_note(format!("at template pos {i}"))
                            .at_arg(0));
                    }
                }
            } else if ch == '}' {
                match fmt_chars.get(i + 1) {
                    Some(Value::Char('}')) => i += 2, // "}}" -> literal
                    _ => {
                        return Err(WqErr::new(WqErrType::Domain)
                            .src(BE::Fmt)
                            .msg("unescaped '{'; use '{{' for a literal or '{}' for a placeholder")
                            .attach_note(format!("at template pos {i}"))
                            .at_arg(0));
                    }
                }
            } else {
                i += 1;
            }
        }

        Ok(count)
    }

    // Runtime check
    let fmt_chars = match args.first() {
        Some(s) => s.as_char_list().ok_or_else(|| {
            WqErr::new(WqErrType::Domain)
                .src(BE::Fmt)
                .msg("expected char or list<char>")
                .at_arg(0)
        })?,
        None => return Err(WqErr::new(WqErrType::Arity).msg("expected at least 1 arg, got 0")),
    };
    // Pre-count placeholders for arity errors
    let needed = count_placeholders(&fmt_chars)?;
    let provided = args.len().saturating_sub(1);
    if provided != needed {
        return Err(WqErr::new(WqErrType::Arity)
            .src(BE::Fmt)
            .msg(format!("expected {needed}, got {provided}")));
    }

    // Format
    let mut out: Vec<Value> = Vec::with_capacity(fmt_chars.len() + 16);
    let mut i = 0usize;
    let mut arg_idx = 0usize;
    while i < fmt_chars.len() {
        let ch = match fmt_chars[i] {
            Value::Char(c) => c,
            _ => unreachable!(),
        };
        if ch == '{' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('{')) => {
                    out.push(Value::Char('{'));
                    i += 2;
                }
                Some(Value::Char('}')) => {
                    // "{}" -> substitute next argument
                    // safe because of the pre-check using `needed`.
                    push_value_as_chars(&mut out, &args[arg_idx + 1]);
                    arg_idx += 1;
                    i += 2;
                }
                _ => unreachable!(),
            }
        } else if ch == '}' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('}')) => {
                    out.push(Value::Char('}'));
                    i += 2;
                }
                _ => unreachable!(),
            }
        } else {
            out.push(Value::Char(ch));
            i += 1;
        }
    }
    Ok(Value::List(out))
}

/// Count grapheme clusters (user-perceived characters)
pub fn graphemes(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Graphemes, [1], args)?;
    let s = args[0].try_to_string().map_err(|e| e.src(BE::Graphemes))?;
    let count = s.graphemes(true).count();
    Ok(count.into_wq_value())
}

/// Split str by Unicode word boundaries
pub fn words(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Words, [1], args)?;
    let s = args[0].try_to_string().map_err(|e| e.src(BE::Words))?;
    let res = s
        .split_word_bounds()
        .filter(|w| !w.trim().is_empty())
        .map(|v| v.into_wq_value())
        .collect();
    Ok(Value::List(res))
}

pub fn trim(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::Trim, [1], args)?;
    let s = args[0].try_to_string().map_err(|e| e.src(BE::Trim))?;
    Ok(s.trim().into_wq_value())
}

pub fn trim_start(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::TrimS, [1], args)?;
    let s = args[0].try_to_string().map_err(|e| e.src(BE::TrimS))?;
    Ok(s.trim_start().into_wq_value())
}

pub fn trim_end(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::TrimE, [1], args)?;
    let s = args[0].try_to_string().map_err(|e| e.src(BE::TrimE))?;
    Ok(s.trim_end().into_wq_value())
}

/// Check if a character is whitespace
pub fn is_whitespace(_vm: &mut Vm, args: &[Value]) -> WqResult<Value> {
    check_arity(BE::WsQ, [1], args)?;
    match &args[0] {
        Value::Char(c) => Ok(Value::Bool(c.is_whitespace())),
        v => Err(type_mismatch(BE::WsQ, 0, "char", v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Vm;

    #[test]
    fn interpolation() {
        let test = Value::List("x = {}".chars().map(Value::Char).collect());
        let mut vm = Vm::new(vec![]);
        let res = fmt(&mut vm, &[test, Value::Int(5)]).unwrap();
        assert_eq!(res, Value::List("x = 5".chars().map(Value::Char).collect()));
    }

    #[test]
    fn escape_braces() {
        let test = Value::List("{{}}".chars().map(Value::Char).collect());
        let mut vm = Vm::new(vec![]);
        let res = fmt(&mut vm, &[test]).unwrap();
        assert_eq!(res, Value::List("{}".chars().map(Value::Char).collect()));
    }

    #[test]
    fn test_graphemes() {
        let mut vm = Vm::new(vec![]);
        // Simple ASCII
        let input = Value::List("hello".chars().map(Value::Char).collect());
        assert_eq!(graphemes(&mut vm, &[input]).unwrap(), Value::Int(5));

        // Multi-byte characters
        let input = Value::List("café".chars().map(Value::Char).collect());
        assert_eq!(graphemes(&mut vm, &[input]).unwrap(), Value::Int(4));

        // Grapheme clusters
        let input = Value::List("👨‍👩‍👧‍👦".chars().map(Value::Char).collect());
        assert_eq!(graphemes(&mut vm, &[input]).unwrap(), Value::Int(1));
    }
}
