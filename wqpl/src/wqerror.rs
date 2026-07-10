use std::sync::Arc;

use indexmap::IndexMap;

use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::value::{Excerpt as _, Value};
use crate::wqdb::data::{CodeLoc, DebugInfo};

#[derive(Debug, Clone, PartialEq)]
pub struct SourceCtx {
    pub text: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WqError {
    pub err_type: WqErrorType,
    pub src: Option<String>,
    pub msg: Option<String>,
    pub notes: Vec<String>,
    /// Byte span of the offending token(s) in the source, if known.
    pub span: Option<(usize, usize)>,
    /// Source context (text + path) for rendering the span snippet.
    pub source_ctx: Option<Box<SourceCtx>>,
}

impl WqError {
    pub(crate) fn new(err_type: WqErrorType) -> Self {
        Self {
            err_type,
            src: None,
            msg: None,
            notes: Vec::new(),
            span: None,
            source_ctx: None,
        }
    }

    pub(crate) fn src(mut self, d: impl std::fmt::Display) -> Self {
        self.src = Some(d.to_string());
        self
    }

    pub(crate) fn msg(mut self, d: impl std::fmt::Display) -> Self {
        self.msg = Some(d.to_string());
        self
    }

    pub(crate) fn span(mut self, span: Option<(usize, usize)>) -> Self {
        self.span = span;
        self
    }

    pub(crate) fn source_ctx(mut self, text: impl Into<String>, path: impl Into<String>) -> Self {
        self.source_ctx = Some(Box::new(SourceCtx {
            text: text.into(),
            path: path.into(),
        }));
        self
    }

    pub(crate) fn attach_note(mut self, d: impl std::fmt::Display) -> Self {
        self.notes.push(d.to_string());
        self
    }

    pub(crate) fn got1(mut self, v: &Value) -> Self {
        self = self.attach_note(format!("got '{}' ({})", v.excerpt(), v.type_name()));
        self
    }

    pub(crate) fn got2(mut self, lhs: &Value, rhs: &Value) -> Self {
        self = self.attach_note(format!("got lhs '{}' ({})", lhs.excerpt(), lhs.type_name()));
        self = self.attach_note(format!("got rhs '{}' ({})", rhs.excerpt(), rhs.type_name()));
        self
    }

    pub(crate) fn unexpected_element(mut self, v: &Value, i: usize) -> Self {
        self = self.attach_note(format!(
            "unexpected element '{}' ({}) at [{i}]",
            v.excerpt(),
            v.type_name()
        ));
        self
    }

    pub(crate) fn at_arg(mut self, pos: usize) -> Self {
        self = self.attach_note(format!("at arg[{}]", pos));
        self
    }

    /// Convert this error into the stable wq-side error payload used by `@t`.
    pub(crate) fn to_wq_value(
        &self,
        debug_info: &DebugInfo,
        backtrace: &[(CodeLoc, Arc<str>)],
    ) -> Value {
        value_dict([
            ("version", Value::Int(1)),
            ("kind", Value::Tag(Arc::from(self.err_type.name()))),
            (
                "message",
                string_value(self.msg.as_deref().unwrap_or_else(|| self.err_type.name())),
            ),
            (
                "source",
                self.src
                    .as_deref()
                    .map(string_value)
                    .unwrap_or_else(Value::unit),
            ),
            ("span", self.wq_span_value(debug_info, backtrace)),
            (
                "notes",
                Value::List(Arc::new(
                    self.notes.iter().map(string_value).collect::<Vec<_>>(),
                )),
            ),
            ("data", Value::Dict(Arc::new(IndexMap::new()))),
            ("stack", wq_stack_value(debug_info, backtrace)),
            ("cause", Value::unit()),
        ])
    }

    fn wq_span_value(&self, debug_info: &DebugInfo, backtrace: &[(CodeLoc, Arc<str>)]) -> Value {
        if let (Some((start, end)), Some(source)) = (self.span, self.source_ctx.as_deref()) {
            return span_value(&source.path, &source.text, start, end);
        }

        let Some((loc, _)) = backtrace.first() else {
            return Value::unit();
        };
        let Some(chunk) = debug_info.chunk_opt(loc.chunk) else {
            return Value::unit();
        };
        let span = chunk.line_table.context_span_at(loc.pc);
        let Some(file) = debug_info.file(span.file_id) else {
            return Value::unit();
        };
        span_value(&file.path, &file.text, span.start, span.end)
    }
}

fn wq_stack_value(debug_info: &DebugInfo, backtrace: &[(CodeLoc, Arc<str>)]) -> Value {
    let frames = backtrace
        .iter()
        .map(|(loc, name)| {
            let location = debug_info.chunk_opt(loc.chunk).and_then(|chunk| {
                let span = chunk.line_table.context_span_at(loc.pc);
                debug_info.file(span.file_id).map(|file| {
                    let byte = clamp_byte_boundary(&file.text, span.start);
                    (Arc::clone(&file.path), byte, file.display_line_col(byte))
                })
            });
            let (path, byte, line, column) = if let Some((path, byte, (line, column))) = location {
                (
                    string_value(&path),
                    usize_value(byte),
                    usize_value(line),
                    usize_value(column),
                )
            } else {
                (Value::unit(), Value::unit(), Value::unit(), Value::unit())
            };
            value_dict([
                ("function", string_value(name)),
                ("path", path),
                ("line", line),
                ("column", column),
                ("byte", byte),
            ])
        })
        .collect();
    Value::List(Arc::new(frames))
}

fn span_value(path: &str, source: &str, start: usize, end: usize) -> Value {
    let start = clamp_byte_boundary(source, start);
    let end = clamp_byte_boundary(source, end);
    let (start_line, start_column) = byte_to_line_col(source, start);
    let (end_line, end_column) = byte_to_line_col(source, end);
    value_dict([
        ("path", string_value(path)),
        (
            "start",
            Value::IntList(Arc::new(vec![
                usize_to_i64(start_line),
                usize_to_i64(start_column),
                usize_to_i64(start),
            ])),
        ),
        (
            "end",
            Value::IntList(Arc::new(vec![
                usize_to_i64(end_line),
                usize_to_i64(end_column),
                usize_to_i64(end),
            ])),
        ),
    ])
}

fn value_dict<const N: usize>(entries: [(&str, Value); N]) -> Value {
    let mut values = IndexMap::with_capacity(N);
    for (key, value) in entries {
        values.insert(Arc::from(key), value);
    }
    Value::Dict(Arc::new(values))
}

fn string_value(value: impl AsRef<str>) -> Value {
    Value::String(Arc::new(value.as_ref().to_string()))
}

fn usize_value(value: usize) -> Value {
    Value::Int(usize_to_i64(value))
}

fn usize_to_i64(value: usize) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn clamp_byte_boundary(source: &str, byte: usize) -> usize {
    let mut byte = byte.min(source.len());
    while !source.is_char_boundary(byte) {
        byte -= 1;
    }
    byte
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WqErrorType {
    Vm,

    Eof,
    Syntax,
    NotBound,
    Index,
    Call,
    Arity,

    Domain,
    Length,

    // NumericOverflow,
    ZeroDiv,

    Io,
    Encode,
    Exec,
    Raise,
    Recursion,
}

impl WqErrorType {
    pub const fn is_runtime(&self) -> bool {
        !self.is_compile_time()
    }

    pub const fn is_compile_time(&self) -> bool {
        use WqErrorType as W;
        matches!(self, W::Eof | W::Syntax)
    }

    pub const fn name(&self) -> &'static str {
        use WqErrorType as W;
        match self {
            W::Vm => "vm",
            W::Eof => "eof",
            W::Syntax => "syntax",
            W::NotBound => "not-bound",
            W::Index => "index",
            W::Call => "call",
            W::Arity => "arity",
            W::Domain => "domain",
            W::Length => "length",
            W::ZeroDiv => "zero-div",
            W::Io => "io",
            W::Encode => "encoding",
            W::Exec => "exec",
            W::Raise => "raise",
            W::Recursion => "recursion",
        }
    }

    pub const fn to_code(&self) -> u16 {
        use WqErrorType::*;
        match self {
            Vm => 1,
            Eof | Syntax => 2,
            NotBound | Index | Call | Arity | Domain | Length | ZeroDiv | Io | Encode | Exec
            | Raise | Recursion => 3,
        }
    }
}

impl std::fmt::Display for WqErrorType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Convert a byte offset into (1-based) line and column within `src`.
pub(crate) fn byte_to_line_col(src: &str, byte_pos: usize) -> (usize, usize) {
    let b = clamp_byte_boundary(src, byte_pos);
    let prefix = &src[..b];
    let line = prefix.chars().filter(|&c| c == '\n').count() + 1;
    let last_nl = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = src[last_nl..b].chars().count() + 1;
    (line, col)
}

/// Render a source snippet for a byte span, aligned with backtrace style.
fn render_span_snippet(
    src: &str,
    path: &str,
    start: usize,
    end: usize,
    color_mode: ColorMode,
) -> String {
    let (line, col) = byte_to_line_col(src, start);
    let line_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = src[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(src.len());
    let src_line = &src[line_start..line_end];
    let rel_start = start.saturating_sub(line_start);
    let rel_end = end.min(line_end).saturating_sub(line_start);
    let width = src_line[rel_start..rel_end].chars().count().max(1);

    let use_color = color_mode.should_colorize();
    let mut out = String::new();
    use std::fmt::Write;

    // Header line: at path:line:col
    writeln!(out, "at {path}:{line}:{col}").unwrap();

    let prefix = format!("{:>4} -> ", line);
    if use_color {
        let before = &src_line[..rel_start];
        let span_text = &src_line[rel_start..rel_end];
        let after = &src_line[rel_end..];
        write!(
            out,
            "{}{}{}{}",
            prefix,
            before,
            paint(
                span_text,
                TextStyle::new().fg(AnsiColor::Green).bold().underline(),
                color_mode,
            ),
            after,
        )
        .unwrap();
    } else {
        write!(out, "{}{}", prefix, src_line).unwrap();
    }

    if !use_color {
        let pointer_start = col.saturating_sub(1) + prefix.chars().count();
        write!(out, "\n{}{}", " ".repeat(pointer_start), "~".repeat(width)).unwrap();
    }
    out
}

impl std::fmt::Display for WqError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render_with_color_mode(ColorMode::Auto))
    }
}

impl WqError {
    pub fn render_with_color_mode(&self, color_mode: ColorMode) -> String {
        let mut output = String::new();
        use std::fmt::Write;

        let err_type = paint(
            self.err_type.name(),
            TextStyle::new().bold().underline(),
            color_mode,
        );
        write!(output, "{err_type}: ").expect("writing to string must not fail");
        if let Some(m) = &self.msg {
            write!(output, "{m}").expect("writing to string must not fail");
        }
        writeln!(output).expect("writing to string must not fail");
        if let Some(s) = &self.src {
            writeln!(output, "- from {s}").expect("writing to string must not fail");
        }
        if let (Some((start, end)), Some(ctx)) = (self.span, self.source_ctx.as_ref()) {
            let snippet = render_span_snippet(&ctx.text, &ctx.path, start, end, color_mode);
            let prefix = "- ";
            let cont = " ".repeat(prefix.chars().count());
            let mut lines = snippet.lines();
            if let Some(first) = lines.next() {
                writeln!(output, "{}{}", prefix, first).expect("writing to string must not fail");
            }
            for line in lines {
                writeln!(output, "{}{}", cont, line).expect("writing to string must not fail");
            }
        }
        if !self.notes.is_empty() {
            for note in self.notes.iter() {
                let prefix = "- ";
                let cont = " ".repeat(prefix.chars().count());
                let mut lines = note.lines();
                if let Some(first) = lines.next() {
                    writeln!(output, "{}{}", prefix, first)
                        .expect("writing to string must not fail");
                }
                for line in lines {
                    writeln!(output, "{}{}", cont, line).expect("writing to string must not fail");
                }
            }
        }
        output.trim_end_matches('\n').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::ColorMode;

    #[test]
    fn render_with_color_mode_never_uses_plain_pointer() {
        let err = WqError::new(WqErrorType::Syntax)
            .msg("unexpected token")
            .span(Some((2, 3)))
            .source_ctx("x\n1+2\n", "<test>");

        let rendered = err.render_with_color_mode(ColorMode::Never);

        assert!(rendered.contains("syntax: unexpected token"));
        assert!(rendered.contains("at <test>:2:1"));
        assert!(rendered.contains("   2 -> 1+2"));
        assert!(rendered.contains("        ~"));
        assert!(!rendered.contains("\x1b["));
    }

    #[test]
    fn render_with_color_mode_always_styles_error_and_span() {
        let err = WqError::new(WqErrorType::Syntax)
            .msg("unexpected token")
            .span(Some((2, 3)))
            .source_ctx("x\n1+2\n", "<test>");

        let rendered = err.render_with_color_mode(ColorMode::Always);

        assert!(rendered.contains("\x1b[1;4msyntax\x1b[0m: unexpected token"));
        assert!(rendered.contains("\x1b[1;4;32m1\x1b[0m+2"));
        assert!(!rendered.contains("        ~"));
    }

    #[test]
    fn wq_value_has_versioned_stable_fields_and_source_span() {
        let err = WqError::new(WqErrorType::Domain)
            .src("test")
            .msg("bad value")
            .attach_note("got int")
            .span(Some((2, 3)))
            .source_ctx("x\n1+2\n", "<test>");

        let value = err.to_wq_value(&DebugInfo::default(), &[]);
        let Value::Dict(error) = value else {
            panic!("expected error dict");
        };

        assert_eq!(
            error.keys().map(AsRef::as_ref).collect::<Vec<_>>(),
            vec![
                "version", "kind", "message", "source", "span", "notes", "data", "stack", "cause"
            ]
        );
        assert_eq!(error.get("version"), Some(&Value::Int(1)));
        assert_eq!(error.get("kind"), Some(&Value::Tag(Arc::from("domain"))));
        let Some(Value::Dict(span)) = error.get("span") else {
            panic!("expected span dict");
        };
        assert_eq!(
            span.get("start"),
            Some(&Value::IntList(Arc::new(vec![2, 1, 2])))
        );
        assert_eq!(
            span.get("end"),
            Some(&Value::IntList(Arc::new(vec![2, 2, 3])))
        );
    }

    #[test]
    fn wq_value_clamps_malformed_utf8_byte_spans() {
        let err = WqError::new(WqErrorType::Domain)
            .span(Some((1, usize::MAX)))
            .source_ctx("éx", "<test>");

        let value = err.to_wq_value(&DebugInfo::default(), &[]);
        let Value::Dict(error) = value else {
            panic!("expected error dict");
        };
        let Some(Value::Dict(span)) = error.get("span") else {
            panic!("expected span dict");
        };

        assert_eq!(
            span.get("start"),
            Some(&Value::IntList(Arc::new(vec![1, 1, 0])))
        );
        assert_eq!(
            span.get("end"),
            Some(&Value::IntList(Arc::new(vec![1, 3, 3])))
        );
    }
}
