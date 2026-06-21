//! Wrap-only formatter.
//!
//! This pass preserves source spelling and spacing except when inserting a
//! newline after a semicolon separator. It is intentionally narrower than the
//! full CST formatter: parse first, then only add line breaks at token positions
//! that are valid separators in wq source.

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use crate::token::{Token, TokenType};

pub(super) fn wrap_source(src: &str, tokens: &[Token], width: usize, indent_size: usize) -> String {
    let width = width.max(1);
    let breakpoints = collect_breakpoints(tokens);
    if breakpoints.is_empty() {
        return src.to_string();
    }

    let mut out = String::new();
    let mut line_start = 0usize;
    for (idx, ch) in src.char_indices() {
        if ch == '\n' {
            wrap_line(
                &mut out,
                src,
                line_start,
                idx,
                &breakpoints,
                width,
                indent_size,
            );
            out.push('\n');
            line_start = idx + ch.len_utf8();
        }
    }
    if line_start < src.len() {
        wrap_line(
            &mut out,
            src,
            line_start,
            src.len(),
            &breakpoints,
            width,
            indent_size,
        );
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Breakpoint {
    pos: usize,
    depth: usize,
}

fn collect_breakpoints(tokens: &[Token]) -> Vec<Breakpoint> {
    let mut breakpoints = Vec::new();
    let mut depth = 0usize;
    for token in tokens {
        if matches!(&token.token_type, TokenType::Eof) {
            break;
        }

        if is_close(&token.token_type) {
            depth = depth.saturating_sub(1);
        }

        if is_open(&token.token_type) {
            depth += 1;
        }

        if matches!(&token.token_type, TokenType::Semicolon) {
            breakpoints.push(Breakpoint {
                pos: token.byte_end,
                depth,
            });
        }
    }
    breakpoints
}

fn wrap_line(
    out: &mut String,
    src: &str,
    line_start: usize,
    line_end: usize,
    breakpoints: &[Breakpoint],
    width: usize,
    indent_size: usize,
) {
    let line = &src[line_start..line_end];
    if line.width() <= width {
        out.push_str(line);
        return;
    }

    let line_breakpoints: Vec<Breakpoint> = breakpoints
        .iter()
        .copied()
        .filter(|breakpoint| line_start < breakpoint.pos && breakpoint.pos < line_end)
        .collect();
    if line_breakpoints.is_empty() {
        out.push_str(line);
        return;
    }

    let base_indent = leading_indent_width(line);
    let mut current_start = line_start;
    let mut pending_indent = 0usize;

    loop {
        if pending_indent + src[current_start..line_end].width() <= width {
            break;
        }
        let Some(breakpoint) =
            choose_breakpoint(src, current_start, line_end, pending_indent, width, &line_breakpoints)
        else {
            break;
        };
        out.push_str(&src[current_start..breakpoint.pos]);
        out.push('\n');
        pending_indent = base_indent + indent_size * breakpoint.depth;
        for _ in 0..pending_indent {
            out.push(' ');
        }
        current_start = skip_horizontal_whitespace(src, breakpoint.pos, line_end);
    }

    out.push_str(&src[current_start..line_end]);
}

fn choose_breakpoint(
    src: &str,
    current_start: usize,
    line_end: usize,
    pending_indent: usize,
    width: usize,
    breakpoints: &[Breakpoint],
) -> Option<Breakpoint> {
    let mut last_fitting = None;
    for breakpoint in breakpoints
        .iter()
        .copied()
        .filter(|breakpoint| current_start < breakpoint.pos && breakpoint.pos < line_end)
    {
        let segment_width = pending_indent + src[current_start..breakpoint.pos].width();
        if segment_width <= width {
            last_fitting = Some(breakpoint);
        } else {
            return last_fitting.or(Some(breakpoint));
        }
    }
    last_fitting
}

fn skip_horizontal_whitespace(src: &str, mut pos: usize, end: usize) -> usize {
    while pos < end {
        let Some(ch) = src[pos..end].chars().next() else {
            break;
        };
        if ch == '\n' || !ch.is_whitespace() {
            break;
        }
        pos += ch.len_utf8();
    }
    pos
}

fn leading_indent_width(line: &str) -> usize {
    let mut width = 0usize;
    for ch in line.chars() {
        if ch == ' ' || ch == '\t' {
            width += ch.width().unwrap_or(0);
        } else if ch != '\r' {
            break;
        }
    }
    width
}

fn is_open(token: &TokenType) -> bool {
    matches!(
        token,
        TokenType::LeftParen | TokenType::LeftBracket | TokenType::LeftBrace
    )
}

fn is_close(token: &TokenType) -> bool {
    matches!(
        token,
        TokenType::RightParen | TokenType::RightBracket | TokenType::RightBrace
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::Lexer;

    fn wrap(src: &str, width: usize) -> String {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("lex succeeds");
        wrap_source(src, &tokens, width, 2)
    }

    #[test]
    fn leaves_short_source_untouched() {
        assert_eq!(wrap("f[(1; 2; 3)]", 80), "f[(1; 2; 3)]");
    }

    #[test]
    fn wraps_after_semicolon_without_normalizing_source() {
        assert_eq!(
            wrap("f[(1; 2; 3; 4; 5)]", 8),
            "f[(1; 2;\n    3;\n    4;\n    5)]"
        );
    }

    #[test]
    fn uses_display_width() {
        assert_eq!("界".width(), 2);
    }
}
