use std::fmt;
use std::sync::Arc;

use indexmap::IndexMap;
use num_bigint::BigInt;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthStr as _;

use crate::astnode::binary_op_display;
use crate::value::Value;

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{n}"),
            Value::BigInt(n) => write!(f, "{n}"),
            Value::Float(fl) => {
                if fl.is_infinite() && fl.is_sign_positive() {
                    write!(f, "inf")
                } else if fl.is_infinite() && fl.is_sign_negative() {
                    write!(f, "-inf")
                } else if fl.is_nan() {
                    write!(f, "nan")
                } else if fl.fract() == 0.0 {
                    write!(f, "{fl:.1}")
                } else {
                    write!(f, "{fl}")
                }
            }
            Value::Complex(z) => {
                write!(f, "{}", Self::format_complex64(*z, true))
            }
            Value::Fraction(fd) => {
                if *fd.denom() == BigInt::from(1) {
                    write!(f, "{}", fd.numer())
                } else {
                    write!(f, "{}/{}", fd.numer(), fd.denom())
                }
            }
            Value::Char(c) => {
                let esc = escape_str_for_display(&c.to_string());
                write!(f, "\"{esc}\"")
            }
            Value::Tag(s) => write!(f, "`{s}"),
            Value::Bool(b) => write!(f, "{}", if *b { "T" } else { "F" }),
            Value::IntList(items) => {
                if items.is_empty() {
                    return write!(f, "()");
                }
                if items.len() == 1 {
                    write!(f, ",{}", items[0])
                } else {
                    let strs: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    write!(f, "({})", strs.join(";"))
                }
            }
            Value::String(s) => {
                if s.is_empty() {
                    return write!(f, "\"\"");
                }
                let esc = escape_str_for_display(s);
                write!(f, "\"{esc}\"")
            }
            Value::List(items) => {
                // Empty list
                if items.is_empty() {
                    write!(f, "()")
                } else if items.iter().all(|v| matches!(v, Value::Char(_))) {
                    // Non-empty char-only list -> quoted string
                    // Don't call self.to_rust_string()
                    let s: String = items
                        .iter()
                        .map(|v| {
                            if let Value::Char(c) = v {
                                *c
                            } else {
                                unreachable!()
                            }
                        })
                        .collect();
                    let esc = escape_str_for_display(&s);
                    write!(f, "\"{esc}\"")
                } else if items.len() == 1 {
                    let item = &items[0];
                    match item {
                        Value::List(_) | Value::IntList(_) if !item.is_unit() =>
                        // Nest a 1‑element list inside another 1‑element list
                        // renders as ,(,a) instead of the invalid ,,a.
                        {
                            write!(f, ",({})", item)
                        }
                        _ => write!(f, ",{}", item),
                    }
                } else {
                    // General case
                    let items_str: Vec<String> = items.iter().map(|v| v.to_string()).collect();
                    write!(f, "({})", items_str.join(";"))
                }
            }
            Value::Cas(_) => {
                if let Some(s) = self.format_cas() {
                    write!(f, "{s}")
                } else {
                    write!(f, "<cas>")
                }
            }
            Value::Dict(map) => {
                if map.is_empty() {
                    write!(f, "(`)")
                } else {
                    let mut pairs = Vec::new();
                    for (k, v) in &**map {
                        pairs.push(format!("`{k}:{v}"));
                    }
                    write!(f, "({})", pairs.join(";"))
                }
            }

            Value::CompiledFunction(func) => {
                let mut parts: Vec<String> = Vec::new();
                if let Some(p) = &func.params {
                    for name in p.iter() {
                        parts.push(name.clone());
                    }
                }
                if let Some(np) = &func.named_params {
                    for name in np.iter() {
                        parts.push(format!("`{name}"));
                    }
                }
                if parts.is_empty() {
                    write!(f, "{{...}}")
                } else {
                    write!(f, "{{[{}]...}}", parts.join(";"))
                }
            }
            Value::Closure(c) => {
                let mut parts: Vec<String> = Vec::new();
                if let Some(p) = &c.params {
                    for name in p.iter() {
                        parts.push(name.clone());
                    }
                }
                if let Some(np) = &c.named_params {
                    for name in np.iter() {
                        parts.push(format!("`{name}"));
                    }
                }
                if parts.is_empty() {
                    write!(f, "{{...}}")
                } else {
                    write!(f, "{{[{}]...}}", parts.join(";"))
                }
            }
            Value::BuiltinFunction { name, .. } => write!(f, "<bfn '{name}'>"),
            Value::LiftedCallable(data) => {
                let op = data
                    .expr
                    .display_op()
                    .map(|op| binary_op_display(&op))
                    .unwrap_or("expr");
                write!(f, "<fn {op} fn>")
            }
            Value::Stream(_) => write!(f, "<stream>"),
            Value::Algebraic(a) => crate::value::algebraic::fmt_algebraic_human(a, f),
        }
    }
}
pub(crate) fn escape_str_for_display(s: &str) -> String {
    crate::escape::escape_string_inner(s, '"')
}

pub(crate) fn into_wq_string<S: Into<String>>(s: S) -> Value {
    Value::String(Arc::new(s.into()))
}

pub fn format_table_value(val: &Value) -> Option<String> {
    if val.is_atom() || val.is_empty() || matches!(val, Value::Complex(_) | Value::Cas(_)) {
        return None;
    }
    if let Some((headers, rows)) = parse_list_of_dicts(val) {
        return Some(format_table(&headers, &rows));
    }
    if let Some((headers, rows)) = parse_dict_of_dicts(val) {
        return Some(format_table(&headers, &rows));
    }
    if let Some((headers, rows)) = parse_dict_table(val) {
        return Some(format_table(&headers, &rows));
    }
    None
}

fn parse_dict_table(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    let Value::Dict(map) = val else {
        return None;
    };
    let mut wrapped: IndexMap<Arc<str>, Value> = IndexMap::new();
    for (k, v) in map.iter() {
        if is_table_column_value(v) {
            wrapped.insert(k.clone(), v.clone());
        } else {
            wrapped.insert(k.clone(), Value::List(Arc::new(vec![v.clone()])));
        }
    }
    let wrapped_val = Value::Dict(Arc::new(wrapped));
    parse_dict_of_lists(&wrapped_val)
}

fn is_table_column_value(value: &Value) -> bool {
    match value {
        Value::IntList(_) => true,
        Value::List(items) => !is_char_list(items),
        _ => false,
    }
}

fn is_char_list(items: &[Value]) -> bool {
    !items.is_empty() && items.iter().all(|item| matches!(item, Value::Char(_)))
}

fn parse_list_of_dicts(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    if let Value::List(rows) = val
        && rows
            .iter()
            .all(|r| matches!(r, Value::Dict(_)) && !r.is_atom())
    {
        let mut headers: Vec<String> = Vec::new();
        for row in rows.iter() {
            if let Value::Dict(map) = row {
                for k in map.keys() {
                    if !headers.contains(&k.to_string()) {
                        headers.push(k.to_string());
                    }
                }
            }
        }
        let mut data = Vec::new();
        for row in rows.iter() {
            if let Value::Dict(map) = row {
                let mut r = Vec::new();
                for h in &headers {
                    if let Some(v) = map.get(h.as_str()) {
                        r.push(v.to_string());
                    } else {
                        r.push(String::new());
                    }
                }
                data.push(r);
            }
        }
        return Some((headers, data));
    }
    None
}

fn parse_dict_of_lists(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    if let Value::Dict(map) = val
        && map.values().all(is_table_column_value)
    {
        let headers: Vec<String> = map.keys().map(|k| k.to_string()).collect();
        let nrows = map
            .values()
            .filter_map(|v| match v {
                Value::List(items) => Some(items.len()),
                Value::IntList(items) => Some(items.len()),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        let mut data = Vec::new();
        for i in 0..nrows {
            let mut row = Vec::new();
            for h in &headers {
                if let Some(value) = map.get(h.as_str()) {
                    match value {
                        Value::List(items) => {
                            if let Some(v) = items.get(i) {
                                row.push(v.to_string());
                            } else {
                                row.push(String::new());
                            }
                        }
                        Value::IntList(items) => {
                            if let Some(v) = items.get(i) {
                                row.push(v.to_string());
                            } else {
                                row.push(String::new());
                            }
                        }
                        _ => row.push(String::new()),
                    }
                } else {
                    row.push(String::new());
                }
            }
            data.push(row);
        }
        return Some((headers, data));
    }
    None
}

fn parse_dict_of_dicts(val: &Value) -> Option<(Vec<String>, Vec<Vec<String>>)> {
    if let Value::Dict(map) = val
        && map
            .values()
            .all(|v| matches!(v, Value::Dict(_)) && !v.is_atom())
    {
        let row_names: Vec<String> = map.keys().map(|k| k.to_string()).collect();
        let mut columns: Vec<String> = Vec::new();
        for v in map.values() {
            if let Value::Dict(inner) = v {
                for k in inner.keys() {
                    if !columns.contains(&k.to_string()) {
                        columns.push(k.to_string());
                    }
                }
            }
        }
        let mut headers = Vec::with_capacity(columns.len() + 1);
        headers.push("row".to_string());
        headers.extend(columns.clone());
        let mut data = Vec::new();
        for row_name in &row_names {
            let mut row = Vec::new();
            row.push(row_name.clone());
            if let Some(Value::Dict(inner)) = map.get(row_name.as_str()) {
                for col in &columns {
                    if let Some(v) = inner.get(col.as_str()) {
                        row.push(v.to_string());
                    } else {
                        row.push(String::new());
                    }
                }
            } else {
                for _ in &columns {
                    row.push(String::new());
                }
            }
            data.push(row);
        }
        return Some((headers, data));
    }
    None
}

fn format_table(headers: &[String], rows: &[Vec<String>]) -> String {
    let mut table = Vec::new();
    if !headers.is_empty() {
        table.push(headers.to_vec());
    }
    table.extend_from_slice(rows);
    let ncols = table.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; ncols];
    for row in &table {
        for (j, cell) in row.iter().enumerate() {
            let width = display_width(cell);
            if widths[j] < width {
                widths[j] = width;
            }
        }
    }
    let mut lines = Vec::with_capacity(table.len());
    for row in table {
        let mut parts = Vec::new();
        for (j, cell) in row.iter().enumerate() {
            let padding = widths[j].saturating_sub(display_width(cell));
            parts.push(format!("{}{}", cell, " ".repeat(padding)));
        }
        lines.push(parts.join(" ").trim_end().to_string());
    }
    lines.join("\n")
}

fn display_width(text: &str) -> usize {
    let mut visible = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for code_ch in chars.by_ref() {
                if ('@'..='~').contains(&code_ch) {
                    break;
                }
            }
        } else {
            visible.push(ch);
        }
    }
    visible.as_str().width()
}

pub trait Excerpt {
    fn excerpt(&self) -> String;
}

impl<T: std::fmt::Display> Excerpt for T {
    fn excerpt(&self) -> String {
        let s = self.to_string();
        let mut g = s.graphemes(true);
        let head: String = g.by_ref().take(20).collect();
        if g.next().is_some() {
            format!("{head}...")
        } else {
            head
        }
    }
}
