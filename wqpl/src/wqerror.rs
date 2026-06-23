use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::value::{Excerpt as _, Value};

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
    let b = byte_pos.min(src.len());
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
                TextStyle::new()
                    .fg(AnsiColor::Green)
                    .bold()
                    .underline(),
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
                    writeln!(output, "{}{}", prefix, first).expect("writing to string must not fail");
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
}
