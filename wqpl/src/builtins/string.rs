use std::borrow::Cow;
use std::sync::Arc;

use num_bigint::BigInt;
use num_traits::Signed;
use unicode_segmentation::UnicodeSegmentation;

use crate::builtins::{
    BuiltinEnum as BE, BuiltinFnArgs, at_least_arity_error, check_arity, type_mismatch,
};
use crate::value::{IntoWqValue, Value, WqResult, expected_string1};
use crate::wqerror::{Requirement, WqError, WqErrorType};

pub(super) fn to_str(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Str, [1], &args)?;
    let arg = args.into_iter().next().unwrap();
    if arg.is_string() {
        return Ok(arg);
    }
    Ok(arg.to_string().into_wq_value())
}

#[derive(Debug, Clone, Copy)]
enum FormatWidth {
    Fixed(usize),
    Dynamic,
}

#[derive(Debug, Clone, Copy)]
enum FormatPrecision {
    Fixed(usize),
    Dynamic,
}

#[derive(Debug, Default)]
struct FormatSpec {
    fill: Option<char>,
    align: Option<char>,
    sign: Option<char>,
    alt_form: bool,
    zero_pad: bool,
    width: Option<FormatWidth>,
    precision: Option<FormatPrecision>,
    type_spec: Option<char>,
}

fn parse_format_spec(spec: &str) -> WqResult<FormatSpec> {
    let mut chars = spec.chars().peekable();
    let mut result = FormatSpec::default();

    // fill + align
    if let Some(&first) = chars.peek() {
        let has_align = chars.clone().nth(1).is_some_and(|s| "<>^=".contains(s));
        if has_align {
            result.fill = Some(first);
            result.align = Some(chars.clone().nth(1).unwrap());
            chars.next();
            chars.next();
        } else if "<>^=".contains(first) {
            result.align = Some(first);
            chars.next();
        }
    }

    // sign
    if let Some(&c) = chars.peek()
        && "+- ".contains(c)
    {
        result.sign = Some(c);
        chars.next();
    }

    // alt form
    if let Some(&c) = chars.peek()
        && c == '#'
    {
        result.alt_form = true;
        chars.next();
    }

    // zero pad (only if align not already set)
    if result.align.is_none()
        && let Some(&c) = chars.peek()
        && c == '0'
    {
        result.zero_pad = true;
        result.fill = Some('0');
        result.align = Some('=');
        chars.next();
    }

    // width
    if let Some(&c) = chars.peek() {
        if c == '{' {
            chars.next();
            if chars.next() == Some('}') {
                result.width = Some(FormatWidth::Dynamic);
            }
        } else if c.is_ascii_digit() {
            result.width = Some(FormatWidth::Fixed(parse_format_usize(&mut chars, "width")?));
        }
    }

    // precision
    if let Some(&c) = chars.peek()
        && c == '.'
    {
        chars.next();
        if let Some(&c) = chars.peek() {
            if c == '{' {
                chars.next();
                if chars.next() == Some('}') {
                    result.precision = Some(FormatPrecision::Dynamic);
                }
            } else if c.is_ascii_digit() {
                result.precision = Some(FormatPrecision::Fixed(parse_format_usize(
                    &mut chars,
                    "precision",
                )?));
            }
        }
    }

    // type
    if let Some(&c) = chars.peek()
        && "bBoOxXeE,%?".contains(c)
    {
        result.type_spec = Some(c);
        chars.next();
    }

    Ok(result)
}

fn parse_format_usize(
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
    label: &str,
) -> WqResult<usize> {
    let mut num = 0usize;
    while let Some(&c) = chars.peek()
        && c.is_ascii_digit()
    {
        let digit = usize::try_from(c.to_digit(10).expect("ascii digit"))
            .expect("decimal digit fits in usize");
        num = num
            .checked_mul(10)
            .and_then(|n| n.checked_add(digit))
            .ok_or_else(|| {
                WqError::new(WqErrorType::Domain)
                    .src(BE::Fmt)
                    .msg(format!("{label} is too large"))
            })?;
        chars.next();
    }
    Ok(num)
}

fn dynamic_format_usize(value: &Value, arg_idx: usize, label: &str) -> WqResult<usize> {
    let n = value.as_i64().ok_or_else(|| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Fmt)
            .expected(Requirement::INT)
            .at_arg(arg_idx)
            .attach_note(format!("for dynamic {label}"))
            .got1(value)
    })?;
    usize::try_from(n).map_err(|_| {
        WqError::new(WqErrorType::Domain)
            .src(BE::Fmt)
            .expected(Requirement::non_negative(Requirement::INT))
            .at_arg(arg_idx)
            .attach_note(format!("for dynamic {label}"))
            .got1(value)
    })
}

fn add_commas_to_int(s: &str) -> String {
    let mut chars: Vec<char> = s.chars().collect();
    let mut i = chars.len();
    while i > 3 {
        i -= 3;
        chars.insert(i, ',');
    }
    chars.into_iter().collect()
}

fn apply_alignment(s: &str, width: usize, align: char, fill: char) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let pad = width - len;

    match align {
        '<' => format!("{}{}", s, fill.to_string().repeat(pad)),
        '>' => format!("{}{}", fill.to_string().repeat(pad), s),
        '^' => {
            let left = pad / 2;
            let right = pad - left;
            format!(
                "{}{}{}",
                fill.to_string().repeat(left),
                s,
                fill.to_string().repeat(right)
            )
        }
        '=' => {
            let mut rest = s;
            let mut prefix = String::new();
            if let Some(first) = rest.chars().next()
                && (first == '+' || first == '-' || first == ' ')
            {
                prefix.push(first);
                rest = &rest[first.len_utf8()..];
            }
            if rest.starts_with("0x")
                || rest.starts_with("0X")
                || rest.starts_with("0b")
                || rest.starts_with("0B")
                || rest.starts_with("0o")
                || rest.starts_with("0O")
            {
                prefix.push_str(&rest[..2]);
                rest = &rest[2..];
            }
            format!("{}{}{}", prefix, fill.to_string().repeat(pad), rest)
        }
        _ => s.to_string(),
    }
}

fn format_int(value: &Value, spec: &FormatSpec, precision: Option<usize>) -> WqResult<String> {
    let is_negative = match value {
        Value::Int(n) => *n < 0,
        Value::BigInt(b) => b.sign() == num_bigint::Sign::Minus,
        _ => unreachable!(),
    };

    let mut result = match spec.type_spec {
        Some('x') | Some('X') => match value {
            Value::Int(n) => {
                let abs = n.unsigned_abs();
                let mut h = format!("{:x}", abs);
                if spec.type_spec == Some('X') {
                    h = h.to_uppercase();
                }
                h
            }
            Value::BigInt(b) => {
                let mut h = format!("{:x}", b.abs());
                if spec.type_spec == Some('X') {
                    h = h.to_uppercase();
                }
                h
            }
            _ => unreachable!(),
        },
        Some('b') | Some('B') => match value {
            Value::Int(n) => {
                let abs = n.unsigned_abs();
                let mut b = format!("{:b}", abs);
                if spec.type_spec == Some('B') {
                    b = b.to_uppercase();
                }
                b
            }
            Value::BigInt(bi) => {
                let mut b = format!("{:b}", bi.abs());
                if spec.type_spec == Some('B') {
                    b = b.to_uppercase();
                }
                b
            }
            _ => unreachable!(),
        },
        Some('o') | Some('O') => match value {
            Value::Int(n) => {
                let abs = n.unsigned_abs();
                let mut o = format!("{:o}", abs);
                if spec.type_spec == Some('O') {
                    o = o.to_uppercase();
                }
                o
            }
            Value::BigInt(bi) => {
                let mut o = format!("{:o}", bi.abs());
                if spec.type_spec == Some('O') {
                    o = o.to_uppercase();
                }
                o
            }
            _ => unreachable!(),
        },
        Some(',') => {
            let abs_str = match value {
                Value::Int(n) => {
                    if *n < 0 {
                        n.unsigned_abs().to_string()
                    } else {
                        n.to_string()
                    }
                }
                Value::BigInt(b) => b.abs().to_string(),
                _ => unreachable!(),
            };
            add_commas_to_int(&abs_str)
        }
        _ => match value {
            Value::Int(n) => {
                if *n < 0 {
                    n.unsigned_abs().to_string()
                } else {
                    n.to_string()
                }
            }
            Value::BigInt(b) => b.abs().to_string(),
            _ => unreachable!(),
        },
    };

    if spec.alt_form {
        let prefix = match spec.type_spec {
            Some('x') => "0x",
            Some('X') => "0X",
            Some('b') => "0b",
            Some('B') => "0B",
            Some('o') => "0o",
            Some('O') => "0O",
            _ => "",
        };
        if !prefix.is_empty() {
            result = format!("{}{}", prefix, result);
        }
    }

    if is_negative {
        result = format!("-{}", result);
    } else if spec.sign == Some('+') {
        result = format!("+{}", result);
    } else if spec.sign == Some(' ') {
        result = format!(" {}", result);
    }

    // precision = minimum digits
    if let Some(prec) = precision {
        let mut prefix_len = 0;
        if result.starts_with('+') || result.starts_with('-') || result.starts_with(' ') {
            prefix_len = 1;
        }
        let after_sign = &result[prefix_len..];
        if after_sign.starts_with("0x")
            || after_sign.starts_with("0X")
            || after_sign.starts_with("0b")
            || after_sign.starts_with("0B")
            || after_sign.starts_with("0o")
            || after_sign.starts_with("0O")
        {
            prefix_len += 2;
        }
        let digits = &result[prefix_len..];
        if digits.len() < prec {
            let zeros = "0".repeat(prec - digits.len());
            result = format!("{}{}{}", &result[..prefix_len], zeros, digits);
        }
    }

    Ok(result)
}

fn format_float(f: f64, spec: &FormatSpec, precision: Option<usize>) -> WqResult<String> {
    let prec = precision.unwrap_or(6);
    let mut result = match spec.type_spec {
        Some('e') => format!("{:.prec$e}", f, prec = prec),
        Some('E') => format!("{:.prec$E}", f, prec = prec),
        _ => {
            if prec == 0 {
                format!("{:.0}", f)
            } else {
                format!("{:.prec$}", f, prec = prec)
            }
        }
    };

    if spec.type_spec == Some(',') {
        // Add thousands separator to the integer part
        let sign = if result.starts_with('-') || result.starts_with('+') || result.starts_with(' ')
        {
            result.remove(0).to_string()
        } else {
            String::new()
        };
        if let Some(dot_pos) = result.find('.') {
            let int_part = add_commas_to_int(&result[..dot_pos]);
            let frac_part = &result[dot_pos..];
            result = format!("{}{}{}", sign, int_part, frac_part);
        } else {
            result = format!("{}{}", sign, add_commas_to_int(&result));
        }
    }

    if !result.starts_with('-') {
        if spec.sign == Some('+') {
            result = format!("+{}", result);
        } else if spec.sign == Some(' ') {
            result = format!(" {}", result);
        }
    }

    Ok(result)
}

fn format_string(s: &str, _spec: &FormatSpec, precision: Option<usize>) -> WqResult<String> {
    let mut result = s.to_string();
    if let Some(prec) = precision {
        let chars: Vec<char> = result.chars().collect();
        if chars.len() > prec {
            result = chars[..prec].iter().collect();
        }
    }
    Ok(result)
}

fn format_percentage(value: &Value, precision: Option<usize>) -> String {
    let mut result = match value {
        Value::Int(n) => n.checked_mul(100).map_or_else(
            || (BigInt::from(*n) * 100i32).to_string(),
            |pct| pct.to_string(),
        ),
        Value::BigInt(b) => (b.as_ref() * 100i32).to_string(),
        Value::Float(f) => {
            let prec = precision.unwrap_or(0);
            let pct = f * 100.0;
            if prec == 0 {
                format!("{:.0}", pct)
            } else {
                format!("{:.prec$}", pct, prec = prec)
            }
        }
        _ => {
            if let Some(s) = value.try_to_rust_string() {
                s
            } else {
                value.to_string()
            }
        }
    };

    if !result.starts_with('-') {
        // percentage uses the spec sign that was already parsed
    }
    result.push('%');
    result
}

fn format_value(
    value: &Value,
    spec: &FormatSpec,
    width: Option<usize>,
    precision: Option<usize>,
) -> WqResult<String> {
    let mut result = if spec.type_spec == Some('?') {
        if spec.alt_form {
            format!("{:#?}", value)
        } else {
            format!("{:?}", value)
        }
    } else if spec.type_spec == Some('%') {
        let mut pct = format_percentage(value, precision);
        if !pct.starts_with('-') {
            if spec.sign == Some('+') {
                pct = format!("+{}", pct);
            } else if spec.sign == Some(' ') {
                pct = format!(" {}", pct);
            }
        }
        pct
    } else {
        match value {
            Value::Int(_) | Value::BigInt(_) => format_int(value, spec, precision)?,
            Value::Float(f) => format_float(**f, spec, precision)?,
            Value::Char(c) => format_string(&c.to_string(), spec, precision)?,
            _ => {
                if let Some(s) = value.try_to_rust_string() {
                    format_string(&s, spec, precision)?
                } else {
                    let s = value.to_string();
                    format_string(&s, spec, precision)?
                }
            }
        }
    };

    if let Some(w) = width {
        let fill = spec.fill.unwrap_or(' ');
        let align = spec.align.unwrap_or('>');
        result = apply_alignment(&result, w, align, fill);
    }

    Ok(result)
}

pub(super) fn fmt(args: BuiltinFnArgs) -> WqResult<Value> {
    // Append Value to an output Vec<Value::Char(..)>, avoiding extra allocations
    // when possible.
    fn push_value_as_chars(out: &mut Vec<Value>, v: &Value) {
        match v.as_rust_char_slice() {
            Some(Cow::Borrowed(s)) => out.extend_from_slice(s),
            Some(Cow::Owned(mut v)) => out.append(&mut v),
            None => out.extend(v.to_string().chars().map(Value::Char)),
        }
    }

    fn push_str_as_chars(out: &mut Vec<Value>, s: &str) {
        out.extend(s.chars().map(Value::Char));
    }

    fn char_at(fmt_chars: &[Value], idx: usize) -> Option<char> {
        match fmt_chars.get(idx) {
            Some(Value::Char(c)) => Some(*c),
            _ => None,
        }
    }

    fn collect_chars(fmt_chars: &[Value], start: usize, end: usize) -> String {
        fmt_chars[start..end]
            .iter()
            .map(|v| match v {
                Value::Char(c) => *c,
                _ => unreachable!(),
            })
            .collect()
    }

    fn read_format_spec(fmt_chars: &[Value], open: usize) -> WqResult<(String, usize)> {
        let mut brace_depth = 0usize;
        let mut j = open + 2;
        while j < fmt_chars.len() {
            match fmt_chars[j] {
                Value::Char('{') => brace_depth += 1,
                Value::Char('}') if brace_depth > 0 => brace_depth -= 1,
                Value::Char(']') if brace_depth == 0 => {
                    if char_at(fmt_chars, j + 1) != Some('}') {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Fmt)
                            .msg("expected '}' after format specifier")
                            .at_arg(0)
                            .attach_note(format!("at template position {open}")));
                    }
                    return Ok((collect_chars(fmt_chars, open + 2, j), j + 2));
                }
                _ => {}
            }
            j += 1;
        }
        Err(WqError::new(WqErrorType::Domain)
            .src(BE::Fmt)
            .msg("unterminated format specifier")
            .at_arg(0)
            .attach_note(format!("at template position {open}")))
    }

    fn count_placeholders(fmt_chars: &[Value]) -> WqResult<usize> {
        let mut i = 0usize;
        let mut count = 0usize;

        while i < fmt_chars.len() {
            let ch = match fmt_chars[i] {
                Value::Char(c) => c,
                _ => unreachable!(),
            };

            if ch == '{' {
                match fmt_chars.get(i + 1) {
                    Some(Value::Char('{')) => i += 2,
                    Some(Value::Char('[')) => {
                        let (spec_str, next_i) = read_format_spec(fmt_chars, i)?;
                        let spec = parse_format_spec(&spec_str)?;
                        if spec
                            .width
                            .is_some_and(|w| matches!(w, FormatWidth::Dynamic))
                        {
                            count += 1;
                        }
                        if spec
                            .precision
                            .is_some_and(|p| matches!(p, FormatPrecision::Dynamic))
                        {
                            count += 1;
                        }
                        count += 1; // main value
                        i = next_i;
                    }
                    Some(Value::Char('}')) => {
                        count += 1;
                        i += 2;
                    }
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Fmt)
                            .msg(
                                "unescaped '{'; use '{{' for a literal, '{}' for a placeholder, or '{[spec]}' for a formatted placeholder",
                            )
                            .at_arg(0)
                            .attach_note(format!("at template position {i}")));
                    }
                }
            } else if ch == '}' {
                match fmt_chars.get(i + 1) {
                    Some(Value::Char('}')) => i += 2,
                    _ => {
                        return Err(WqError::new(WqErrorType::Domain)
                            .src(BE::Fmt)
                            .msg("unescaped '}'; use '}}' for a literal")
                            .at_arg(0)
                            .attach_note(format!("at template position {i}")));
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
        Some(s) => s.as_rust_char_slice().ok_or_else(|| {
            WqError::new(WqErrorType::Domain)
                .src(BE::Fmt)
                .expected(Requirement::one_of([
                    Requirement::CHAR,
                    Requirement::STRING,
                ]))
                .at_arg(0)
        })?,
        None => return Err(at_least_arity_error(BE::Fmt, 1, 0)),
    };
    // Pre-count placeholders for arity errors
    let needed = count_placeholders(&fmt_chars)?;
    let provided = args.len().saturating_sub(1);
    if provided != needed {
        let noun = if needed == 1 {
            "replacement argument"
        } else {
            "replacement arguments"
        };
        return Err(WqError::new(WqErrorType::Arity)
            .src(BE::Fmt)
            .msg(format!("expected {needed} {noun}, got {provided}")));
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
                Some(Value::Char('[')) => {
                    let (spec_str, next_i) = read_format_spec(&fmt_chars, i)?;
                    let spec = parse_format_spec(&spec_str)?;

                    let width = match spec.width {
                        Some(FormatWidth::Dynamic) => {
                            let w = &args[arg_idx + 1];
                            arg_idx += 1;
                            Some(dynamic_format_usize(w, arg_idx, "width")?)
                        }
                        Some(FormatWidth::Fixed(n)) => Some(n),
                        None => None,
                    };

                    let precision = match spec.precision {
                        Some(FormatPrecision::Dynamic) => {
                            let p = &args[arg_idx + 1];
                            arg_idx += 1;
                            Some(dynamic_format_usize(p, arg_idx, "precision")?)
                        }
                        Some(FormatPrecision::Fixed(n)) => Some(n),
                        None => None,
                    };

                    let value = &args[arg_idx + 1];
                    arg_idx += 1;

                    let formatted = format_value(value, &spec, width, precision)?;
                    push_str_as_chars(&mut out, &formatted);

                    i = next_i;
                }
                Some(Value::Char('}')) => {
                    push_value_as_chars(&mut out, &args[arg_idx + 1]);
                    arg_idx += 1;
                    i += 2;
                }
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::Fmt)
                        .msg(
                            "unescaped '{'; use '{{' for a literal, '{}' for a placeholder, or '{[spec]}' for a formatted placeholder",
                        )
                        .at_arg(0)
                        .attach_note(format!("at template position {i}")));
                }
            }
        } else if ch == '}' {
            match fmt_chars.get(i + 1) {
                Some(Value::Char('}')) => {
                    out.push(Value::Char('}'));
                    i += 2;
                }
                _ => {
                    return Err(WqError::new(WqErrorType::Domain)
                        .src(BE::Fmt)
                        .msg("unescaped '}'; use '}}' for a literal")
                        .at_arg(0)
                        .attach_note(format!("at template position {i}")));
                }
            }
        } else {
            out.push(Value::Char(ch));
            i += 1;
        }
    }
    let result: String = out
        .iter()
        .map(|v| match v {
            Value::Char(c) => *c,
            _ => unreachable!(),
        })
        .collect();
    Ok(Value::String(Arc::new(result)))
}

fn string_arg(args: &BuiltinFnArgs, index: usize, builtin: BE) -> WqResult<String> {
    args[index]
        .try_to_rust_string()
        .ok_or_else(|| expected_string1(&args[index]).src(builtin).at_arg(index))
}

fn transformed_string(input: &Value, output: String) -> Value {
    if matches!(input, Value::Char(_)) {
        let mut chars = output.chars();
        if let (Some(value), None) = (chars.next(), chars.next()) {
            return Value::Char(value);
        }
    }
    Value::String(Arc::new(output))
}

fn selector_error(builtin: BE, value: &Value, selectors: &[&'static str]) -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(builtin)
        .expected(Requirement::one_of(
            selectors
                .iter()
                .map(|selector| Requirement::literal(format!("`{selector}"))),
        ))
        .at_arg(1)
        .got1(value)
}

pub(super) fn unicode(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Unicode, [0, 2], &args)?;
    if args.is_empty() {
        return Ok(crate::unicode::VERSION_STRING.into_wq_value());
    }

    let selector = match &args[1] {
        Value::Tag(selector) => selector.as_ref(),
        value => {
            return Err(selector_error(
                BE::Unicode,
                value,
                &["name", "from_name", "xid_start", "xid_continue"],
            ));
        }
    };

    match selector {
        "name" => {
            let name = match &args[0] {
                Value::Char(value) => crate::unicode::character_name(*value),
                value => {
                    let value = value
                        .try_to_rust_string()
                        .ok_or_else(|| expected_string1(value).src(BE::Unicode).at_arg(0))?;
                    crate::unicode::named_sequence_name(&value).map(str::to_string)
                }
            };
            Ok(name.map_or_else(Value::empty_list, IntoWqValue::into_wq_value))
        }
        "from_name" => {
            let name = string_arg(&args, 0, BE::Unicode)?;
            Ok(
                crate::unicode::lookup_name(&name).map_or_else(Value::empty_list, |value| {
                    let mut chars = value.chars();
                    match (chars.next(), chars.next()) {
                        (Some(value), None) => Value::Char(value),
                        _ => Value::String(Arc::new(value)),
                    }
                }),
            )
        }
        "xid_start" | "xid_continue" => {
            let Value::Char(value) = &args[0] else {
                return Err(type_mismatch(BE::Unicode, 0, Requirement::CHAR, &args[0]));
            };
            let result = if selector == "xid_start" {
                crate::unicode::is_xid_start(*value)
            } else {
                crate::unicode::is_xid_continue(*value)
            };
            Ok(Value::Bool(result))
        }
        _ => Err(selector_error(
            BE::Unicode,
            &args[1],
            &["name", "from_name", "xid_start", "xid_continue"],
        )),
    }
}

pub(super) fn normalize(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Normalize, [1, 2], &args)?;
    let form = match args.get(1) {
        None => crate::unicode::NormalizationForm::Nfc,
        Some(Value::Tag(form)) if form.as_ref() == "nfc" => crate::unicode::NormalizationForm::Nfc,
        Some(Value::Tag(form)) if form.as_ref() == "nfd" => crate::unicode::NormalizationForm::Nfd,
        Some(Value::Tag(form)) if form.as_ref() == "nfkc" => {
            crate::unicode::NormalizationForm::Nfkc
        }
        Some(Value::Tag(form)) if form.as_ref() == "nfkd" => {
            crate::unicode::NormalizationForm::Nfkd
        }
        Some(value) => {
            return Err(selector_error(
                BE::Normalize,
                value,
                &["nfc", "nfd", "nfkc", "nfkd"],
            ));
        }
    };
    let input = string_arg(&args, 0, BE::Normalize)?;
    Ok(transformed_string(
        &args[0],
        crate::unicode::normalize(&input, form),
    ))
}

pub(super) fn change_case(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Case, [2], &args)?;
    let mode = match &args[1] {
        Value::Tag(mode) if mode.as_ref() == "lower" => crate::unicode::CaseMode::Lower,
        Value::Tag(mode) if mode.as_ref() == "upper" => crate::unicode::CaseMode::Upper,
        Value::Tag(mode) if mode.as_ref() == "fold" => crate::unicode::CaseMode::Fold,
        value => return Err(selector_error(BE::Case, value, &["lower", "upper", "fold"])),
    };
    let input = string_arg(&args, 0, BE::Case)?;
    Ok(transformed_string(
        &args[0],
        crate::unicode::change_case(&input, mode),
    ))
}

/// Split into extended grapheme clusters.
pub(super) fn graphemes(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Graphemes, [1], &args)?;
    let value = string_arg(&args, 0, BE::Graphemes)?;
    Ok(Value::from_items(
        value
            .graphemes(true)
            .map(|cluster| {
                let mut chars = cluster.chars();
                match (chars.next(), chars.next()) {
                    (Some(value), None) => Value::Char(value),
                    _ => Value::String(Arc::new(cluster.to_string())),
                }
            })
            .collect(),
    ))
}

pub(super) fn trim(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Trim, [1], &args)?;
    let value = string_arg(&args, 0, BE::Trim)?;
    Ok(value
        .trim_matches(crate::unicode::is_whitespace)
        .into_wq_value())
}

pub(super) fn trim_left(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::LTrim, [1], &args)?;
    let value = string_arg(&args, 0, BE::LTrim)?;
    Ok(value
        .trim_start_matches(crate::unicode::is_whitespace)
        .into_wq_value())
}

pub(super) fn trim_right(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::RTrim, [1], &args)?;
    let value = string_arg(&args, 0, BE::RTrim)?;
    Ok(value
        .trim_end_matches(crate::unicode::is_whitespace)
        .into_wq_value())
}

pub(super) fn is_whitespace(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::WhitespaceQ, [1], &args)?;
    match &args[0] {
        Value::Char(value) => Ok(Value::Bool(crate::unicode::is_whitespace(*value))),
        value => Err(type_mismatch(BE::WhitespaceQ, 0, Requirement::CHAR, value)),
    }
}

pub(super) fn term_width(args: BuiltinFnArgs) -> WqResult<Value> {
    check_arity(BE::Termwidth, [1], &args)?;
    let value = string_arg(&args, 0, BE::Termwidth)?;
    if value.chars().any(char::is_control) {
        return Err(WqError::new(WqErrorType::Domain)
            .src(BE::Termwidth)
            .expected(Requirement::phrase(
                "char or string without control characters",
                "chars or strings without control characters",
            ))
            .at_arg(0)
            .got1(&args[0]));
    }
    Ok(crate::unicode::terminal_width(&value).into_wq_value())
}

#[cfg(test)]
mod tests {
    use smallvec::smallvec;

    use super::*;

    #[test]
    fn interpolation() {
        let test = "x = {}".into_wq_value();
        let res = fmt(BuiltinFnArgs::from(smallvec![test, Value::Int(5)])).unwrap();
        assert_eq!(res, "x = 5".into_wq_value());
    }

    #[test]
    fn escape_braces() {
        let test = "{{}}".into_wq_value();
        let res = fmt(BuiltinFnArgs::from(test)).unwrap();
        assert_eq!(res, "{}".into_wq_value());
    }

    #[test]
    fn test_graphemes() {
        assert_eq!(
            graphemes(BuiltinFnArgs::from("hello".into_wq_value())).unwrap(),
            "hello".into_wq_value()
        );
        assert_eq!(
            graphemes(BuiltinFnArgs::from("e\u{301}x".into_wq_value())).unwrap(),
            Value::List(Arc::new(vec!["e\u{301}".into_wq_value(), Value::Char('x')]))
        );
    }

    #[test]
    fn unicode_properties_include_xid() {
        assert_eq!(
            unicode(BuiltinFnArgs::from(vec![
                Value::Char('λ'),
                Value::Tag("xid_start".into())
            ]))
            .unwrap(),
            Value::Bool(true)
        );
        assert_eq!(
            unicode(BuiltinFnArgs::from(vec![
                Value::Char('\u{301}'),
                Value::Tag("xid_continue".into())
            ]))
            .unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn unicode_names_round_trip_named_sequences() {
        let value = unicode(BuiltinFnArgs::from(vec![
            "KEYCAP DIGIT ONE".into_wq_value(),
            Value::Tag("from_name".into()),
        ]))
        .unwrap();
        assert_eq!(value, "1\u{fe0f}\u{20e3}".into_wq_value());
        assert_eq!(
            unicode(BuiltinFnArgs::from(vec![value, Value::Tag("name".into())])).unwrap(),
            "KEYCAP DIGIT ONE".into_wq_value()
        );
    }

    #[test]
    fn case_and_normalization_preserve_char_shape_when_possible() {
        assert_eq!(
            change_case(BuiltinFnArgs::from(vec![
                Value::Char('A'),
                Value::Tag("lower".into())
            ]))
            .unwrap(),
            Value::Char('a')
        );
        assert_eq!(
            change_case(BuiltinFnArgs::from(vec![
                Value::Char('ß'),
                Value::Tag("upper".into())
            ]))
            .unwrap(),
            "SS".into_wq_value()
        );
        assert_eq!(
            normalize(BuiltinFnArgs::from(Value::Char('é'))).unwrap(),
            Value::Char('é')
        );
    }

    #[test]
    fn terminal_width_rejects_controls() {
        let error = term_width(BuiltinFnArgs::from(Value::Char('\t')))
            .expect_err("tab width is context-dependent");
        assert!(error.to_string().contains("without control characters"));
    }

    // Format spec tests

    fn run_fmt(template: &str, args: &[Value]) -> String {
        let t = template.into_wq_value();
        let mut all_args = vec![t];
        all_args.extend_from_slice(args);
        let res = fmt(BuiltinFnArgs::from(all_args)).unwrap();
        res.try_to_rust_string().unwrap_or_default()
    }

    #[test]
    fn fmt_rejects_negative_dynamic_width() {
        let err = fmt(BuiltinFnArgs::from(vec![
            "{[{}]}".into_wq_value(),
            Value::Int(-1),
            Value::Int(42),
        ]))
        .expect_err("negative dynamic width should fail");

        assert!(
            err.to_string().contains("expected non-negative int"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fmt_rejects_static_width_that_overflows_usize() {
        let template = format!("{{[{}]}}", "9".repeat(100));
        let err = fmt(BuiltinFnArgs::from(vec![
            template.into_wq_value(),
            Value::Int(42),
        ]))
        .expect_err("oversized static width should fail");

        assert!(
            err.to_string().contains("width is too large"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn fmt_right_align() {
        assert_eq!(run_fmt("{[>10]}", &[Value::Int(42)]), "        42");
    }

    #[test]
    fn fmt_left_align() {
        assert_eq!(run_fmt("{[<10]}", &[Value::Int(42)]), "42        ");
    }

    #[test]
    fn fmt_center_align() {
        assert_eq!(run_fmt("{[^10]}", &[Value::Int(42)]), "    42    ");
    }

    #[test]
    fn fmt_fill_and_center() {
        assert_eq!(run_fmt("{[*^10]}", &[Value::Int(42)]), "****42****");
    }

    #[test]
    fn fmt_zero_pad() {
        assert_eq!(run_fmt("{[08]}", &[Value::Int(42)]), "00000042");
    }

    #[test]
    fn fmt_float_precision() {
        assert_eq!(run_fmt("{[.2]}", &[Value::float(4.14159)]), "4.14");
    }

    #[test]
    fn fmt_string_truncate() {
        assert_eq!(run_fmt("{[.5]}", &["hello world".into_wq_value()]), "hello");
    }

    #[test]
    fn fmt_hex_lower() {
        assert_eq!(run_fmt("{[x]}", &[Value::Int(255)]), "ff");
    }

    #[test]
    fn fmt_i64_min_magnitude() {
        assert_eq!(
            run_fmt("{[x]}", &[Value::Int(i64::MIN)]),
            "-8000000000000000"
        );
        assert_eq!(
            run_fmt("{[,]}", &[Value::Int(i64::MIN)]),
            "-9,223,372,036,854,775,808"
        );
        assert_eq!(
            run_fmt("{}", &[Value::Int(i64::MIN)]),
            "-9223372036854775808"
        );
    }

    #[test]
    fn fmt_hex_upper() {
        assert_eq!(run_fmt("{[X]}", &[Value::Int(255)]), "FF");
    }

    #[test]
    fn fmt_hex_with_prefix() {
        assert_eq!(run_fmt("{[#x]}", &[Value::Int(255)]), "0xff");
        assert_eq!(run_fmt("{[#X]}", &[Value::Int(255)]), "0XFF");
    }

    #[test]
    fn fmt_hex_zero_pad_with_prefix() {
        assert_eq!(run_fmt("{[#08x]}", &[Value::Int(123)]), "0x00007b");
        assert_eq!(run_fmt("{[#08X]}", &[Value::Int(123)]), "0X00007B");
        assert_eq!(run_fmt("{[#08x]}", &[Value::Int(-123)]), "-0x0007b");
    }

    #[test]
    fn fmt_binary_zero_pad_with_prefix() {
        assert_eq!(run_fmt("{[#08b]}", &[Value::Int(5)]), "0b000101");
    }

    #[test]
    fn fmt_octal_zero_pad_with_prefix() {
        assert_eq!(run_fmt("{[#08o]}", &[Value::Int(83)]), "0o000123");
    }

    #[test]
    fn fmt_hex_precision_with_prefix() {
        assert_eq!(run_fmt("{[#.6x]}", &[Value::Int(123)]), "0x00007b");
        assert_eq!(run_fmt("{[#.6x]}", &[Value::Int(-123)]), "-0x00007b");
    }

    #[test]
    fn fmt_binary() {
        assert_eq!(run_fmt("{[b]}", &[Value::Int(5)]), "101");
        assert_eq!(run_fmt("{[B]}", &[Value::Int(5)]), "101");
        assert_eq!(run_fmt("{[#b]}", &[Value::Int(5)]), "0b101");
    }

    #[test]
    fn fmt_thousands_separator() {
        assert_eq!(run_fmt("{[,]}", &[Value::Int(1234567)]), "1,234,567");
    }

    #[test]
    fn fmt_force_plus() {
        assert_eq!(run_fmt("{[+]}", &[Value::Int(42)]), "+42");
        assert_eq!(run_fmt("{[+]}", &[Value::Int(-42)]), "-42");
    }

    #[test]
    fn fmt_space_sign() {
        assert_eq!(run_fmt("{[ ]}", &[Value::Int(42)]), " 42");
        assert_eq!(run_fmt("{[ ]}", &[Value::Int(-42)]), "-42");
    }

    #[test]
    fn fmt_sign_aware_zero_fill() {
        assert_eq!(run_fmt("{[0=+10]}", &[Value::Int(123)]), "+000000123");
        assert_eq!(run_fmt("{[0=+10]}", &[Value::Int(-123)]), "-000000123");
    }

    #[test]
    fn fmt_zero_pad_implicit_equal() {
        assert_eq!(run_fmt("{[+010]}", &[Value::Int(123)]), "+000000123");
        assert_eq!(run_fmt("{[010]}", &[Value::Int(-123)]), "-000000123");
    }

    #[test]
    fn fmt_dynamic_width() {
        assert_eq!(
            run_fmt("{[{}]}", &[Value::Int(10), Value::Int(42)]),
            "        42"
        );
    }

    #[test]
    fn fmt_dynamic_precision() {
        assert_eq!(
            run_fmt("{[.{}]}", &[Value::Int(2), Value::float(4.14159)]),
            "4.14"
        );
    }

    #[test]
    fn fmt_scientific() {
        let res = run_fmt("{[e]}", &[Value::float(1234.5)]);
        assert!(res.starts_with("1.234500"), "got {res}");
        assert!(res.ends_with("e3"), "got {res}");
    }

    #[test]
    fn fmt_mixed_simple_and_formatted() {
        assert_eq!(
            run_fmt("{} {[>5]}", &[Value::Int(1), Value::Int(2)]),
            "1     2"
        );
    }

    #[test]
    fn fmt_percentage_int() {
        assert_eq!(run_fmt("{[%]}", &[Value::Int(5)]), "500%");
        assert_eq!(
            run_fmt("{[%]}", &[Value::Int(i64::MAX)]),
            "922337203685477580700%"
        );
        assert_eq!(
            run_fmt("{[%]}", &[Value::Int(i64::MIN)]),
            "-922337203685477580800%"
        );
    }

    #[test]
    fn fmt_percentage_float() {
        assert_eq!(run_fmt("{[%]}", &[Value::float(0.5)]), "50%");
        assert_eq!(run_fmt("{[.2%]}", &[Value::float(0.1234)]), "12.34%");
    }

    #[test]
    fn fmt_percentage_with_sign_and_width() {
        assert_eq!(run_fmt("{[+%]}", &[Value::float(0.5)]), "+50%");
        assert_eq!(run_fmt("{[ >8%]}", &[Value::float(0.5)]), "     50%");
    }

    #[test]
    fn fmt_debug() {
        assert_eq!(run_fmt("{[?]}", &[Value::Int(42)]), "Int(42)");
        assert_eq!(run_fmt("{[?]}", &[Value::float(4.14)]), "Float(4.14)");
        assert_eq!(
            run_fmt(
                "{[?]}",
                &[Value::List(Arc::new(vec![Value::Int(1), Value::Int(2)]))]
            ),
            "List([Int(1), Int(2)])"
        );
    }

    #[test]
    fn fmt_debug_pretty() {
        assert_eq!(run_fmt("{[#?]}", &[Value::Int(42)]), "Int(\n    42,\n)");
    }

    #[test]
    fn fmt_debug_with_width() {
        assert_eq!(run_fmt("{[>10?]}", &[Value::Int(42)]), "   Int(42)");
        assert_eq!(run_fmt("{[<10?]}", &[Value::Int(42)]), "Int(42)   ");
    }
}
