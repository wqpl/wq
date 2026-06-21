use std::fmt;
use std::sync::Arc;

use indexmap::IndexMap;
use num_bigint::BigInt;
use unicode_segmentation::UnicodeSegmentation as _;
use unicode_width::UnicodeWidthChar as _;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TableStyle {
    Plain,
    Markdown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TableFormatOptions {
    pub columns: Option<Vec<String>>,
    pub limit: Option<usize>,
    pub max_cell_width: Option<usize>,
    pub missing: String,
    pub style: TableStyle,
}

impl Default for TableFormatOptions {
    fn default() -> Self {
        Self {
            columns: None,
            limit: None,
            max_cell_width: None,
            missing: String::new(),
            style: TableStyle::Plain,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TableData {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

pub fn format_table_value(val: &Value) -> Option<String> {
    format_table_value_with_options(val, &TableFormatOptions::default())
        .ok()
        .flatten()
}

pub fn format_table_value_with_options(
    val: &Value,
    opts: &TableFormatOptions,
) -> Result<Option<String>, String> {
    if val.is_atom() || val.is_empty() || matches!(val, Value::Complex(_) | Value::Cas(_)) {
        return Ok(None);
    }
    let table = parse_list_of_dicts(val)
        .or_else(|| parse_dict_of_dicts(val))
        .or_else(|| parse_dict_table(val));
    table
        .as_ref()
        .map(|table| format_table(table, opts).map(Some))
        .unwrap_or(Ok(None))
}

fn parse_dict_table(val: &Value) -> Option<TableData> {
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

fn parse_list_of_dicts(val: &Value) -> Option<TableData> {
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
        return Some(TableData {
            headers,
            rows: data,
        });
    }
    None
}

fn parse_dict_of_lists(val: &Value) -> Option<TableData> {
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
        return Some(TableData {
            headers,
            rows: data,
        });
    }
    None
}

fn parse_dict_of_dicts(val: &Value) -> Option<TableData> {
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
        return Some(TableData {
            headers,
            rows: data,
        });
    }
    None
}

fn format_table(table: &TableData, opts: &TableFormatOptions) -> Result<String, String> {
    let prepared = prepare_table(table, opts)?;
    Ok(match opts.style {
        TableStyle::Plain => format_plain_table(&prepared),
        TableStyle::Markdown => format_markdown_table(&prepared),
    })
}

struct PreparedTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    numeric_columns: Vec<bool>,
    omitted_rows: usize,
}

fn prepare_table(table: &TableData, opts: &TableFormatOptions) -> Result<PreparedTable, String> {
    let indices = if let Some(columns) = &opts.columns {
        let mut indices = Vec::with_capacity(columns.len());
        for name in columns {
            let Some(idx) = table.headers.iter().position(|header| header == name) else {
                return Err(format!("table column `{name}` was not found"));
            };
            indices.push(idx);
        }
        indices
    } else {
        (0..table.headers.len()).collect()
    };

    let mut headers = Vec::with_capacity(indices.len());
    for idx in &indices {
        headers.push(format_table_cell(&table.headers[*idx], opts));
    }

    let row_limit = opts.limit.unwrap_or(table.rows.len()).min(table.rows.len());
    let omitted_rows = table.rows.len().saturating_sub(row_limit);
    let mut rows = Vec::with_capacity(row_limit);
    for source_row in table.rows.iter().take(row_limit) {
        let mut row = Vec::with_capacity(indices.len());
        for idx in &indices {
            let raw = source_row.get(*idx).map(String::as_str).unwrap_or("");
            row.push(format_table_cell(raw, opts));
        }
        rows.push(row);
    }

    let numeric_columns = (0..headers.len())
        .map(|idx| {
            let mut seen = false;
            for row in &rows {
                let Some(cell) = row.get(idx) else {
                    continue;
                };
                if cell == &opts.missing || cell.is_empty() {
                    continue;
                }
                seen = true;
                if !is_numeric_cell(cell) {
                    return false;
                }
            }
            seen
        })
        .collect();

    Ok(PreparedTable {
        headers,
        rows,
        numeric_columns,
        omitted_rows,
    })
}

fn format_table_cell(cell: &str, opts: &TableFormatOptions) -> String {
    let value = if cell.is_empty() {
        opts.missing.clone()
    } else {
        cell.to_string()
    };
    if let Some(max_width) = opts.max_cell_width {
        truncate_display(&value, max_width)
    } else {
        value
    }
}

fn format_plain_table(prepared: &PreparedTable) -> String {
    let mut render_rows = Vec::new();
    if !prepared.headers.is_empty() {
        render_rows.push(prepared.headers.clone());
    }
    render_rows.extend_from_slice(&prepared.rows);
    let ncols = render_rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; ncols];
    for row in &render_rows {
        for (j, cell) in row.iter().enumerate() {
            let width = display_width(cell);
            if widths[j] < width {
                widths[j] = width;
            }
        }
    }
    let mut lines = Vec::with_capacity(render_rows.len());
    for (row_idx, row) in render_rows.into_iter().enumerate() {
        let mut parts = Vec::new();
        for (j, cell) in row.iter().enumerate() {
            let padding = widths[j].saturating_sub(display_width(cell));
            let align_right =
                row_idx > 0 && prepared.numeric_columns.get(j).copied().unwrap_or(false);
            if align_right {
                parts.push(format!("{}{}", " ".repeat(padding), cell));
            } else {
                parts.push(format!("{}{}", cell, " ".repeat(padding)));
            }
        }
        lines.push(parts.join(" ").trim_end().to_string());
    }
    if prepared.omitted_rows > 0 {
        lines.push(format!("... {} more rows", prepared.omitted_rows));
    }
    lines.join("\n")
}

fn format_markdown_table(table: &PreparedTable) -> String {
    let headers: Vec<String> = table
        .headers
        .iter()
        .map(|cell| escape_markdown_cell(cell))
        .collect();
    let rows: Vec<Vec<String>> = table
        .rows
        .iter()
        .map(|row| row.iter().map(|cell| escape_markdown_cell(cell)).collect())
        .collect();
    let ncols = headers.len();
    let mut widths = vec![0usize; ncols];
    for (idx, cell) in headers.iter().enumerate() {
        widths[idx] = widths[idx].max(display_width(cell));
    }
    for row in &rows {
        for (idx, cell) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(display_width(cell));
        }
    }

    let mut lines = Vec::with_capacity(rows.len() + 3);
    lines.push(markdown_row(&headers, &widths, &table.numeric_columns, false));
    let separator: Vec<String> = (0..ncols)
        .map(|idx| {
            let width = widths[idx].max(3);
            if table.numeric_columns.get(idx).copied().unwrap_or(false) {
                format!("{}:", "-".repeat(width.saturating_sub(1)))
            } else {
                "-".repeat(width)
            }
        })
        .collect();
    lines.push(markdown_row(
        &separator,
        &widths,
        &table.numeric_columns,
        false,
    ));
    for row in rows {
        lines.push(markdown_row(&row, &widths, &table.numeric_columns, true));
    }
    if table.omitted_rows > 0 {
        lines.push(format!("... {} more rows", table.omitted_rows));
    }
    lines.join("\n")
}

fn markdown_row(
    row: &[String],
    widths: &[usize],
    numeric_columns: &[bool],
    align_numbers: bool,
) -> String {
    let mut parts = Vec::new();
    for (idx, cell) in row.iter().enumerate() {
        let width = widths.get(idx).copied().unwrap_or_default();
        let padding = width.saturating_sub(display_width(cell));
        if align_numbers && numeric_columns.get(idx).copied().unwrap_or(false) {
            parts.push(format!(" {}{} ", " ".repeat(padding), cell));
        } else {
            parts.push(format!(" {}{} ", cell, " ".repeat(padding)));
        }
    }
    format!("|{}|", parts.join("|"))
}

fn escape_markdown_cell(cell: &str) -> String {
    cell.replace('|', "\\|")
}

fn is_numeric_cell(cell: &str) -> bool {
    let visible = visible_text(cell);
    let text = visible.trim();
    !text.is_empty() && text.parse::<f64>().is_ok()
}

fn truncate_display(text: &str, max_width: usize) -> String {
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }

    let target = max_width - 1;
    let mut out = String::new();
    let mut used = 0usize;
    for grapheme in text.graphemes(true) {
        let width = grapheme_width(grapheme);
        if used + width > target {
            break;
        }
        out.push_str(grapheme);
        used += width;
    }
    out.push('…');
    out
}

fn grapheme_width(grapheme: &str) -> usize {
    if grapheme == "\n" {
        return 0;
    }
    grapheme
        .chars()
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>()
}

fn display_width(text: &str) -> usize {
    visible_text(text).as_str().width()
}

fn visible_text(text: &str) -> String {
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
    visible
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
