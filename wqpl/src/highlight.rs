use crate::builtins::{BuiltinPreset, Builtins};
use crate::cas::cas_special_call_name;
use crate::lex::Lexer;
use crate::script::{ScriptItem, ScriptSpan, might_have_script_meta, parse_script_items};
use crate::token::{Token, TokenType};
use crate::value::cas::{CasConst, CasFunction};

pub const ANSI_RESET: &str = "\x1b[0m";

/// Native syntax highlight names plus semantic overlays used by editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightName {
    Comment,
    ConstantBuiltin,
    FunctionBuiltin,
    CasSpecial,
    CasConstant,
    CasFunction,
    CasVariable,
    Keyword,
    KeywordReturn,
    KeywordDebug,
    Number,
    Bool,
    Operator,
    OperatorPipe,
    PunctuationBracket,
    PunctuationBracket1,
    PunctuationBracket2,
    PunctuationBracket3,
    PunctuationBracket4,
    PunctuationBracket5,
    PunctuationBracket6,
    PunctuationDelimiter,
    PunctuationSpecial,
    String,
    StringEscape,
    InvalidString,
    Character,
    InvalidCharacter,
    Tag,
    Variable,
    VariableRefCapture,
    VariableParameter,
    Meta,
}

/// Events produced by the highlighter, compatible with the old tree-sitter
/// event stream so the ANSI renderer needs minimal changes.
#[derive(Debug, Clone)]
pub enum HighlightEvent {
    HighlightStart(HighlightName),
    HighlightEnd,
    Source { start: usize, end: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticHighlightSpan {
    pub span: (usize, usize),
    pub name: HighlightName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorContext {
    Code,
    Comment,
    String,
    Tag,
    FStringText,
    FStringExpr,
    Meta,
}

impl CursorContext {
    pub fn suppresses_completion(self) -> bool {
        matches!(
            self,
            CursorContext::Comment
                | CursorContext::String
                | CursorContext::Tag
                | CursorContext::FStringText
                | CursorContext::Meta
        )
    }
}

pub fn cursor_context_at(src: &str, pos: usize) -> CursorContext {
    if let Some(items) = script_items_with_meta(src) {
        let pos = pos.min(src.len());
        if items.iter().any(|item| match item {
            ScriptItem::Shebang { span } => span_contains_pos(*span, pos),
            ScriptItem::Directive(directive) => span_contains_pos(directive.span(), pos),
            ScriptItem::Code { .. } => false,
        }) {
            return CursorContext::Meta;
        }
    }
    let mut lexer = Lexer::new(src);
    let tokens = lexer.tokenize_recovery();
    cursor_context_from_tokens(src, pos, &tokens)
}

fn cursor_context_from_tokens(src: &str, pos: usize, tokens: &[Token]) -> CursorContext {
    let pos = pos.min(src.len());
    for tok in tokens {
        if matches!(tok.token_type, TokenType::Eof) || pos < tok.byte_start {
            break;
        }
        if pos == tok.byte_start || pos > tok.byte_end {
            continue;
        }
        return match &tok.token_type {
            TokenType::Comment(_) => CursorContext::Comment,
            TokenType::String(_) | TokenType::Character(_) => CursorContext::String,
            TokenType::Error
                if invalid_literal_error(src, tok).is_some_and(|(start, _)| pos >= start) =>
            {
                CursorContext::String
            }
            TokenType::Tag(_) => CursorContext::Tag,
            TokenType::FormatString(parts, open_quote, close_quote) => {
                fstring_cursor_context(pos, tok, parts, *open_quote, *close_quote)
            }
            _ => CursorContext::Code,
        };
    }
    CursorContext::Code
}

fn fstring_cursor_context(
    pos: usize,
    tok: &Token,
    parts: &[crate::token::FmtPart],
    open_quote: usize,
    close_quote: usize,
) -> CursorContext {
    if pos <= open_quote {
        return CursorContext::Code;
    }

    let closed = close_quote < tok.byte_end;
    if closed && pos > close_quote {
        return CursorContext::Code;
    }

    for part in parts {
        match part {
            crate::token::FmtPart::Text { start, end, .. } if pos >= *start && pos <= *end => {
                return CursorContext::FStringText;
            }
            crate::token::FmtPart::Expr { source, start, end } if pos > *start && pos < *end => {
                let inner = cursor_context_at(source, pos - *start);
                return if inner == CursorContext::Code {
                    CursorContext::FStringExpr
                } else {
                    inner
                };
            }
            _ => {}
        }
    }

    if pos > open_quote && (!closed || pos <= close_quote) {
        CursorContext::FStringText
    } else {
        CursorContext::Code
    }
}

#[inline]
pub fn ansi_style_for_name(name: HighlightName) -> (&'static str, &'static str) {
    match name {
        HighlightName::Comment => ("\x1b[3;38;5;249m", ANSI_RESET),
        HighlightName::ConstantBuiltin => ("\x1b[1;38;5;220m", ANSI_RESET),
        HighlightName::FunctionBuiltin => ("\x1b[4;38;5;75m", ANSI_RESET),
        HighlightName::CasSpecial => ("\x1b[1;38;5;177m", ANSI_RESET),
        HighlightName::CasConstant => ("\x1b[1;38;5;220m", ANSI_RESET),
        HighlightName::CasFunction => ("\x1b[1;38;5;75m", ANSI_RESET),
        HighlightName::CasVariable => ("\x1b[38;5;117m", ANSI_RESET),
        HighlightName::Keyword => ("\x1b[1;38;5;177m", ANSI_RESET),
        HighlightName::KeywordReturn => ("\x1b[1;38;5;220m", ANSI_RESET),
        HighlightName::KeywordDebug => ("\x1b[1;38;5;211m", ANSI_RESET),
        HighlightName::Number => ("\x1b[38;5;220m", ANSI_RESET),
        HighlightName::Bool => ("\x1b[1;38;5;220m", ANSI_RESET),
        HighlightName::Operator => ("\x1b[38;5;211m", ANSI_RESET),
        HighlightName::OperatorPipe => ("\x1b[1;38;5;177m", ANSI_RESET),
        HighlightName::PunctuationBracket => ("\x1b[38;5;248m", ANSI_RESET),
        HighlightName::PunctuationBracket1 => ("\x1b[1;38;5;210m", ANSI_RESET),
        HighlightName::PunctuationBracket2 => ("\x1b[38;5;215m", ANSI_RESET),
        HighlightName::PunctuationBracket3 => ("\x1b[1;38;5;222m", ANSI_RESET),
        HighlightName::PunctuationBracket4 => ("\x1b[38;5;114m", ANSI_RESET),
        HighlightName::PunctuationBracket5 => ("\x1b[1;38;5;111m", ANSI_RESET),
        HighlightName::PunctuationBracket6 => ("\x1b[38;5;183m", ANSI_RESET),
        HighlightName::PunctuationDelimiter => ("\x1b[38;5;248m", ANSI_RESET),
        HighlightName::PunctuationSpecial => ("\x1b[1;38;5;177m", ANSI_RESET),
        HighlightName::String => ("\x1b[38;5;113m", ANSI_RESET),
        HighlightName::StringEscape => ("\x1b[1;38;5;81m", ANSI_RESET),
        HighlightName::InvalidString => ("\x1b[4;38;5;210m", ANSI_RESET),
        HighlightName::Character => ("\x1b[38;5;81m", ANSI_RESET),
        HighlightName::InvalidCharacter => ("\x1b[4;38;5;210m", ANSI_RESET),
        HighlightName::Tag => ("\x1b[1;38;5;113m", ANSI_RESET),
        HighlightName::Variable => ("\x1b[38;5;117m", ANSI_RESET),
        HighlightName::VariableRefCapture => ("\x1b[1;38;5;39m", ANSI_RESET),
        HighlightName::VariableParameter => ("\x1b[1;38;5;215m", ANSI_RESET),
        HighlightName::Meta => ("\x1b[1;38;5;228m", ANSI_RESET),
    }
}

fn semantic_ansi_style_for_name(name: HighlightName) -> (&'static str, &'static str) {
    match name {
        HighlightName::Comment => ("\x1b[3;90m", ANSI_RESET),
        HighlightName::ConstantBuiltin => ("\x1b[1;33m", ANSI_RESET),
        HighlightName::FunctionBuiltin => ("\x1b[4;34m", ANSI_RESET),
        HighlightName::CasSpecial => ("\x1b[1;35m", ANSI_RESET),
        HighlightName::CasConstant => ("\x1b[1;33m", ANSI_RESET),
        HighlightName::CasFunction => ("\x1b[1;34m", ANSI_RESET),
        HighlightName::CasVariable => ("\x1b[96m", ANSI_RESET),
        HighlightName::Keyword => ("\x1b[1;35m", ANSI_RESET),
        HighlightName::KeywordReturn => ("\x1b[1;33m", ANSI_RESET),
        HighlightName::KeywordDebug => ("\x1b[1;91m", ANSI_RESET),
        HighlightName::Number => ("\x1b[33m", ANSI_RESET),
        HighlightName::Bool => ("\x1b[1;33m", ANSI_RESET),
        HighlightName::Operator => ("\x1b[91m", ANSI_RESET),
        HighlightName::OperatorPipe => ("\x1b[1;35m", ANSI_RESET),
        HighlightName::PunctuationBracket => ("\x1b[37m", ANSI_RESET),
        HighlightName::PunctuationBracket1 => ("\x1b[1;31m", ANSI_RESET),
        HighlightName::PunctuationBracket2 => ("\x1b[93m", ANSI_RESET),
        HighlightName::PunctuationBracket3 => ("\x1b[1;97m", ANSI_RESET),
        HighlightName::PunctuationBracket4 => ("\x1b[32m", ANSI_RESET),
        HighlightName::PunctuationBracket5 => ("\x1b[1;94m", ANSI_RESET),
        HighlightName::PunctuationBracket6 => ("\x1b[95m", ANSI_RESET),
        HighlightName::PunctuationDelimiter => ("\x1b[37m", ANSI_RESET),
        HighlightName::PunctuationSpecial => ("\x1b[1;35m", ANSI_RESET),
        HighlightName::String => ("\x1b[32m", ANSI_RESET),
        HighlightName::StringEscape => ("\x1b[1;36m", ANSI_RESET),
        HighlightName::InvalidString => ("\x1b[4;31m", ANSI_RESET),
        HighlightName::Character => ("\x1b[36m", ANSI_RESET),
        HighlightName::InvalidCharacter => ("\x1b[4;31m", ANSI_RESET),
        HighlightName::Tag => ("\x1b[1;32m", ANSI_RESET),
        HighlightName::Variable => ("\x1b[96m", ANSI_RESET),
        HighlightName::VariableRefCapture => ("\x1b[1;96m", ANSI_RESET),
        HighlightName::VariableParameter => ("\x1b[1;93m", ANSI_RESET),
        HighlightName::Meta => ("\x1b[1;93m", ANSI_RESET),
    }
}

#[derive(Clone, Copy)]
enum AnsiHighlightPalette {
    Extended,
    Semantic,
}

fn render_ansi_with_palette(
    src: &str,
    events: impl IntoIterator<Item = HighlightEvent>,
    reset: &str,
    palette: AnsiHighlightPalette,
) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len() + 16);
    let mut stack: Vec<HighlightName> = Vec::new();

    for ev in events {
        match ev {
            HighlightEvent::HighlightStart(h) => stack.push(h),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let s = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                if let Some(&name) = stack.last() {
                    let (on, off) = match palette {
                        AnsiHighlightPalette::Extended => ansi_style_for_name(name),
                        AnsiHighlightPalette::Semantic => semantic_ansi_style_for_name(name),
                    };
                    out.push_str(on);
                    out.push_str(s);
                    if reset.is_empty() {
                        out.push_str(off);
                    } else {
                        out.push_str(reset);
                    }
                } else {
                    out.push_str(s);
                }
            }
        }
    }

    out
}

pub fn render_ansi(
    src: &str,
    events: impl IntoIterator<Item = HighlightEvent>,
    reset: &str,
) -> String {
    render_ansi_with_palette(src, events, reset, AnsiHighlightPalette::Extended)
}

/// A lightweight syntax highlighter that works directly on the wq lexer.
pub struct Highlighter {
    builtins: Builtins,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self::new()
    }
}

impl Highlighter {
    pub fn new() -> Self {
        Self::with_builtins(Builtins::new())
    }

    pub fn with_builtins(builtins: Builtins) -> Self {
        Self { builtins }
    }

    pub fn with_preset(preset: BuiltinPreset) -> Self {
        Self::with_builtins(Builtins::with_preset(preset))
    }

    /// Highlight a slice of wq source code.
    pub fn highlight(&self, src: &str) -> Vec<HighlightEvent> {
        self.highlight_with_ref_captures(src, &[])
    }

    pub fn highlight_with_ref_captures(
        &self,
        src: &str,
        ref_capture_spans: &[(usize, usize)],
    ) -> Vec<HighlightEvent> {
        let spans: Vec<_> = ref_capture_spans
            .iter()
            .map(|span| SemanticHighlightSpan {
                span: *span,
                name: HighlightName::VariableRefCapture,
            })
            .collect();
        self.highlight_with_semantic_spans(src, &spans)
    }

    pub fn highlight_with_semantic_spans(
        &self,
        src: &str,
        semantic_spans: &[SemanticHighlightSpan],
    ) -> Vec<HighlightEvent> {
        if let Some(items) = script_items_with_meta(src) {
            return self.highlight_script_items(src, &items, semantic_spans);
        }

        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize_recovery();
        Self::events_from_tokens(src, &tokens, &self.builtins, semantic_spans)
    }

    pub fn highlight_ansi(&self, src: &str) -> String {
        self.highlight_ansi_with_reset(src, "")
    }

    /// Highlight source with named ANSI colors that a semantic CSS palette can
    /// resolve for its current theme.
    pub fn highlight_ansi_semantic(&self, src: &str) -> String {
        render_ansi_with_palette(src, self.highlight(src), "", AnsiHighlightPalette::Semantic)
    }

    pub fn highlight_ansi_with_reset(&self, src: &str, reset: &str) -> String {
        render_ansi(src, self.highlight(src), reset)
    }

    pub fn highlight_ansi_with_ref_captures_and_reset(
        &self,
        src: &str,
        ref_capture_spans: &[(usize, usize)],
        reset: &str,
    ) -> String {
        render_ansi(
            src,
            self.highlight_with_ref_captures(src, ref_capture_spans),
            reset,
        )
    }

    pub fn highlight_ansi_with_semantic_spans_and_reset(
        &self,
        src: &str,
        semantic_spans: &[SemanticHighlightSpan],
        reset: &str,
    ) -> String {
        render_ansi(
            src,
            self.highlight_with_semantic_spans(src, semantic_spans),
            reset,
        )
    }

    fn bracket_name(depth: usize) -> HighlightName {
        match depth % 6 {
            0 => HighlightName::PunctuationBracket1,
            1 => HighlightName::PunctuationBracket2,
            2 => HighlightName::PunctuationBracket3,
            3 => HighlightName::PunctuationBracket4,
            4 => HighlightName::PunctuationBracket5,
            _ => HighlightName::PunctuationBracket6,
        }
    }

    fn bracket_highlight_name(
        token_type: &TokenType,
        paren_depth: &mut usize,
        bracket_depth: &mut usize,
        brace_depth: &mut usize,
    ) -> Option<HighlightName> {
        match token_type {
            TokenType::LeftParen => {
                let name = Self::bracket_name(*paren_depth);
                *paren_depth += 1;
                Some(name)
            }
            TokenType::RightParen => {
                if *paren_depth > 0 {
                    *paren_depth -= 1;
                    Some(Self::bracket_name(*paren_depth))
                } else {
                    Some(HighlightName::PunctuationBracket)
                }
            }
            TokenType::LeftBracket => {
                let name = Self::bracket_name(*bracket_depth);
                *bracket_depth += 1;
                Some(name)
            }
            TokenType::RightBracket => {
                if *bracket_depth > 0 {
                    *bracket_depth -= 1;
                    Some(Self::bracket_name(*bracket_depth))
                } else {
                    Some(HighlightName::PunctuationBracket)
                }
            }
            TokenType::LeftBrace => {
                let name = Self::bracket_name(*brace_depth);
                *brace_depth += 1;
                Some(name)
            }
            TokenType::RightBrace => {
                if *brace_depth > 0 {
                    *brace_depth -= 1;
                    Some(Self::bracket_name(*brace_depth))
                } else {
                    Some(HighlightName::PunctuationBracket)
                }
            }
            _ => None,
        }
    }

    fn highlight_script_items(
        &self,
        src: &str,
        items: &[ScriptItem],
        semantic_spans: &[SemanticHighlightSpan],
    ) -> Vec<HighlightEvent> {
        let mut events = Vec::new();
        let mut cursor = 0usize;

        for item in items {
            let span = item.span();
            if span.start > cursor {
                events.push(HighlightEvent::Source {
                    start: cursor,
                    end: span.start,
                });
            }

            match item {
                ScriptItem::Shebang { .. } | ScriptItem::Directive(_) => {
                    events.push(HighlightEvent::HighlightStart(HighlightName::Meta));
                    events.push(HighlightEvent::Source {
                        start: span.start,
                        end: span.end,
                    });
                    events.push(HighlightEvent::HighlightEnd);
                }
                ScriptItem::Code { .. } => {
                    let source = &src[span.as_range()];
                    let mut lexer = Lexer::new(source);
                    let tokens = lexer.tokenize_recovery();
                    let nested_semantic_spans =
                        Self::nested_semantic_spans(semantic_spans, span.start, span.end);
                    let chunk_events = Self::events_from_tokens(
                        source,
                        &tokens,
                        &self.builtins,
                        &nested_semantic_spans,
                    );
                    push_offset_events(&mut events, chunk_events, span.start);
                }
            }

            cursor = span.end;
        }

        if cursor < src.len() {
            events.push(HighlightEvent::Source {
                start: cursor,
                end: src.len(),
            });
        }

        events
    }

    fn events_from_tokens(
        src: &str,
        tokens: &[Token],
        builtins: &Builtins,
        semantic_spans: &[SemanticHighlightSpan],
    ) -> Vec<HighlightEvent> {
        let mut events = Vec::with_capacity(tokens.len() * 2);
        let mut overlay_spans = Self::cas_semantic_spans_from_tokens(tokens);
        overlay_spans.extend_from_slice(semantic_spans);
        let mut last_end: usize = 0;
        let mut paren_depth: usize = 0;
        let mut bracket_depth: usize = 0;
        let mut brace_depth: usize = 0;

        for tok in tokens {
            // Emit un-highlighted gap (whitespace) between previous token end and
            // current token start. The lexer skips whitespace so it never gets a
            // token of its own.
            if tok.byte_start > last_end {
                events.push(HighlightEvent::Source {
                    start: last_end,
                    end: tok.byte_start,
                });
            }

            if let Some(name) = Self::bracket_highlight_name(
                &tok.token_type,
                &mut paren_depth,
                &mut bracket_depth,
                &mut brace_depth,
            ) {
                events.push(HighlightEvent::HighlightStart(name));
                events.push(HighlightEvent::Source {
                    start: tok.byte_start,
                    end: tok.byte_end,
                });
                events.push(HighlightEvent::HighlightEnd);
            } else {
                match &tok.token_type {
                    TokenType::FormatString(parts, open_quote, close_quote) => {
                        // @f prefix (and any whitespace between @f and the quote)
                        if *open_quote > tok.byte_start {
                            events.push(HighlightEvent::HighlightStart(HighlightName::Keyword));
                            events.push(HighlightEvent::Source {
                                start: tok.byte_start,
                                end: *open_quote,
                            });
                            events.push(HighlightEvent::HighlightEnd);
                        }
                        // opening quote
                        events.push(HighlightEvent::HighlightStart(HighlightName::String));
                        events.push(HighlightEvent::Source {
                            start: *open_quote,
                            end: open_quote + 1,
                        });
                        events.push(HighlightEvent::HighlightEnd);

                        let mut inner_last = open_quote + 1;
                        for part in parts {
                            match part {
                                crate::token::FmtPart::Text { start, end, .. } => {
                                    if *start > inner_last {
                                        events.push(HighlightEvent::Source {
                                            start: inner_last,
                                            end: *start,
                                        });
                                    }
                                    Self::push_string_events(
                                        &mut events,
                                        src,
                                        *start,
                                        *end,
                                        HighlightName::String,
                                        true,
                                    );
                                    inner_last = *end;
                                }
                                crate::token::FmtPart::Expr { source, start, end } => {
                                    if *start > inner_last {
                                        events.push(HighlightEvent::Source {
                                            start: inner_last,
                                            end: *start,
                                        });
                                    }
                                    let mut inner_lexer = Lexer::new(source);
                                    let inner_tokens = inner_lexer.tokenize_recovery();
                                    let inner_semantic_spans =
                                        Self::nested_semantic_spans(&overlay_spans, *start, *end);
                                    let inner_events = Self::events_from_tokens(
                                        source,
                                        &inner_tokens,
                                        builtins,
                                        &inner_semantic_spans,
                                    );
                                    for ev in inner_events {
                                        match ev {
                                            HighlightEvent::Source { start: s, end: e } => {
                                                events.push(HighlightEvent::Source {
                                                    start: start + s,
                                                    end: start + e,
                                                });
                                            }
                                            other => events.push(other),
                                        }
                                    }
                                    inner_last = *end;
                                }
                            }
                        }
                        // closing quote
                        if *close_quote >= inner_last && *close_quote < tok.byte_end {
                            events.push(HighlightEvent::HighlightStart(HighlightName::String));
                            events.push(HighlightEvent::Source {
                                start: *close_quote,
                                end: close_quote + 1,
                            });
                            events.push(HighlightEvent::HighlightEnd);
                        }
                        // anything after the closing quote (shouldn't happen, but gap-fill)
                        if tok.byte_end > close_quote + 1 {
                            events.push(HighlightEvent::Source {
                                start: close_quote + 1,
                                end: tok.byte_end,
                            });
                        }
                    }
                    TokenType::String(_) => {
                        let is_raw = src[tok.byte_start..tok.byte_end].starts_with("@l");
                        if Self::string_token_is_invalid(src, tok) {
                            Self::push_highlighted_source(
                                &mut events,
                                tok.byte_start,
                                tok.byte_end,
                                HighlightName::InvalidString,
                            );
                        } else {
                            Self::push_string_events(
                                &mut events,
                                src,
                                tok.byte_start,
                                tok.byte_end,
                                HighlightName::String,
                                !is_raw,
                            );
                        }
                    }
                    TokenType::Character(_) => {
                        if Self::character_token_is_valid(src, tok) {
                            Self::push_string_events(
                                &mut events,
                                src,
                                tok.byte_start,
                                tok.byte_end,
                                HighlightName::Character,
                                true,
                            );
                        } else {
                            Self::push_highlighted_source(
                                &mut events,
                                tok.byte_start,
                                tok.byte_end,
                                HighlightName::InvalidCharacter,
                            );
                        }
                    }
                    TokenType::Error => {
                        if let Some((start, name)) = invalid_literal_error(src, tok) {
                            if tok.byte_start < start {
                                events.push(HighlightEvent::Source {
                                    start: tok.byte_start,
                                    end: start,
                                });
                            }
                            Self::push_highlighted_source(&mut events, start, tok.byte_end, name);
                        } else {
                            events.push(HighlightEvent::Source {
                                start: tok.byte_start,
                                end: tok.byte_end,
                            });
                        }
                    }
                    _ => {
                        let name = Self::semantic_name_for_token(tok, &overlay_spans)
                            .or_else(|| Self::name_for_token(tok, builtins));
                        match name {
                            Some(n) => {
                                events.push(HighlightEvent::HighlightStart(n));
                                events.push(HighlightEvent::Source {
                                    start: tok.byte_start,
                                    end: tok.byte_end,
                                });
                                events.push(HighlightEvent::HighlightEnd);
                            }
                            None => {
                                events.push(HighlightEvent::Source {
                                    start: tok.byte_start,
                                    end: tok.byte_end,
                                });
                            }
                        }
                    }
                }
            }

            last_end = tok.byte_end;
        }

        events
    }

    fn push_string_events(
        events: &mut Vec<HighlightEvent>,
        src: &str,
        start: usize,
        end: usize,
        name: HighlightName,
        highlight_escapes: bool,
    ) {
        events.push(HighlightEvent::HighlightStart(name));

        let mut cursor = start;
        if highlight_escapes {
            let mut search_start = start;
            while let Some(relative_start) = src[search_start..end].find('\\') {
                let escape_start = search_start + relative_start;
                let Some(len) = crate::escape::valid_escape_sequence_len(&src[escape_start..end])
                else {
                    search_start = escape_start + 1;
                    continue;
                };
                let escape_end = escape_start + len;

                if escape_start > cursor {
                    events.push(HighlightEvent::Source {
                        start: cursor,
                        end: escape_start,
                    });
                }
                events.push(HighlightEvent::HighlightStart(HighlightName::StringEscape));
                events.push(HighlightEvent::Source {
                    start: escape_start,
                    end: escape_end,
                });
                events.push(HighlightEvent::HighlightEnd);

                cursor = escape_end;
                search_start = escape_end;
            }
        }

        if cursor < end {
            events.push(HighlightEvent::Source { start: cursor, end });
        }
        events.push(HighlightEvent::HighlightEnd);
    }

    fn push_highlighted_source(
        events: &mut Vec<HighlightEvent>,
        start: usize,
        end: usize,
        name: HighlightName,
    ) {
        events.push(HighlightEvent::HighlightStart(name));
        events.push(HighlightEvent::Source { start, end });
        events.push(HighlightEvent::HighlightEnd);
    }

    fn character_token_is_valid(src: &str, tok: &Token) -> bool {
        let Some(token_src) = src.get(tok.byte_start..tok.byte_end) else {
            return false;
        };
        let mut lexer = Lexer::new(token_src);
        let Ok(tokens) = lexer.tokenize() else {
            return false;
        };
        matches!(
            tokens.as_slice(),
            [
                Token {
                    token_type: TokenType::Character(_),
                    ..
                },
                Token {
                    token_type: TokenType::Eof,
                    ..
                }
            ]
        )
    }

    fn string_token_is_invalid(src: &str, tok: &Token) -> bool {
        let Some(token_src) = src.get(tok.byte_start..tok.byte_end) else {
            return true;
        };
        let mut lexer = Lexer::new(token_src);
        let valid_token = lexer.tokenize().is_ok_and(|tokens| {
            matches!(
                tokens.as_slice(),
                [
                    Token {
                        token_type: TokenType::String(_),
                        ..
                    },
                    Token {
                        token_type: TokenType::Eof,
                        ..
                    }
                ]
            )
        });
        !valid_token
    }

    fn nested_semantic_spans(
        semantic_spans: &[SemanticHighlightSpan],
        start: usize,
        end: usize,
    ) -> Vec<SemanticHighlightSpan> {
        semantic_spans
            .iter()
            .filter_map(|semantic_span| {
                let (span_start, span_end) = semantic_span.span;
                if span_start < end && start < span_end {
                    Some(SemanticHighlightSpan {
                        span: (
                            span_start.saturating_sub(start),
                            span_end.min(end).saturating_sub(start),
                        ),
                        name: semantic_span.name,
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    fn semantic_name_for_token(
        tok: &Token,
        semantic_spans: &[SemanticHighlightSpan],
    ) -> Option<HighlightName> {
        semantic_spans
            .iter()
            .find(|semantic_span| {
                let (start, end) = semantic_span.span;
                start < tok.byte_end && tok.byte_start < end
            })
            .map(|semantic_span| semantic_span.name)
    }

    fn cas_semantic_spans_from_tokens(tokens: &[Token]) -> Vec<SemanticHighlightSpan> {
        let mut spans = Vec::new();
        let mut i = 0usize;

        while i < tokens.len() {
            if tokens[i].token_type != TokenType::AtSymbolic {
                i += 1;
                continue;
            }

            i += 1;
            let mut started = false;
            let mut paren_depth = 0usize;
            let mut bracket_depth = 0usize;
            let mut brace_depth = 0usize;

            while i < tokens.len() {
                let tok = &tokens[i];
                if Self::cas_quote_should_stop(
                    tok,
                    started,
                    paren_depth,
                    bracket_depth,
                    brace_depth,
                ) {
                    break;
                }

                if !matches!(tok.token_type, TokenType::Comment(_) | TokenType::Newline) {
                    started = true;
                }

                if let Some(name) = Self::cas_name_for_token(tok) {
                    spans.push(SemanticHighlightSpan {
                        span: (tok.byte_start, tok.byte_end),
                        name,
                    });
                }

                Self::advance_cas_quote_depths(
                    &tok.token_type,
                    &mut paren_depth,
                    &mut bracket_depth,
                    &mut brace_depth,
                );

                i += 1;
            }
        }

        spans
    }

    fn cas_quote_should_stop(
        tok: &Token,
        started: bool,
        paren_depth: usize,
        bracket_depth: usize,
        brace_depth: usize,
    ) -> bool {
        if matches!(tok.token_type, TokenType::Eof) {
            return true;
        }
        if !started {
            return false;
        }
        if paren_depth != 0 || bracket_depth != 0 || brace_depth != 0 {
            return false;
        }
        matches!(
            tok.token_type,
            TokenType::Newline
                | TokenType::Semicolon
                | TokenType::Pipe
                | TokenType::PipeDot
                | TokenType::PipePipe
                | TokenType::PipePipeDot
                | TokenType::RightParen
                | TokenType::RightBracket
                | TokenType::RightBrace
        )
    }

    fn advance_cas_quote_depths(
        token_type: &TokenType,
        paren_depth: &mut usize,
        bracket_depth: &mut usize,
        brace_depth: &mut usize,
    ) {
        match token_type {
            TokenType::LeftParen => *paren_depth += 1,
            TokenType::LeftBracket => *bracket_depth += 1,
            TokenType::LeftBrace => *brace_depth += 1,
            TokenType::RightParen => *paren_depth = paren_depth.saturating_sub(1),
            TokenType::RightBracket => *bracket_depth = bracket_depth.saturating_sub(1),
            TokenType::RightBrace => *brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
    }

    fn cas_name_for_token(tok: &Token) -> Option<HighlightName> {
        match &tok.token_type {
            TokenType::Inf => Some(HighlightName::CasConstant),
            TokenType::Identifier(name) => Some(Self::cas_name_for_identifier(name)),
            _ => None,
        }
    }

    fn cas_name_for_identifier(name: &str) -> HighlightName {
        if cas_special_call_name(name) {
            HighlightName::CasSpecial
        } else if CasConst::from_name(name).is_some() {
            HighlightName::CasConstant
        } else if CasFunction::from_name(name).is_some() {
            HighlightName::CasFunction
        } else {
            HighlightName::CasVariable
        }
    }

    fn name_for_token(tok: &Token, builtins: &Builtins) -> Option<HighlightName> {
        match &tok.token_type {
            TokenType::Comment(_) => Some(HighlightName::Comment),

            TokenType::Integer(_)
            | TokenType::BigInteger(_)
            | TokenType::Float(_)
            | TokenType::Imaginary(_) => Some(HighlightName::Number),

            TokenType::Inf => Some(HighlightName::ConstantBuiltin),
            TokenType::True | TokenType::False => Some(HighlightName::Bool),

            TokenType::String(_) => Some(HighlightName::String),
            TokenType::Character(_) => Some(HighlightName::Character),
            TokenType::Tag(_) => Some(HighlightName::Tag),

            TokenType::Identifier(name) => {
                if builtins.is_enabled_name(name) {
                    Some(HighlightName::FunctionBuiltin)
                } else {
                    Some(HighlightName::Variable)
                }
            }
            TokenType::Keyword(_) => Some(HighlightName::Keyword),

            TokenType::AtReturn => Some(HighlightName::KeywordReturn),
            TokenType::AtDebug => Some(HighlightName::KeywordDebug),
            TokenType::AtBreak
            | TokenType::AtContinue
            | TokenType::AtDepth(_)
            | TokenType::AtPause
            | TokenType::AtSymbolic
            | TokenType::AtTry
            | TokenType::AtImport => Some(HighlightName::Keyword),

            TokenType::Dollar | TokenType::DollarDot | TokenType::DollarDollar => {
                Some(HighlightName::Keyword)
            }

            TokenType::PlusColon
            | TokenType::MinusColon
            | TokenType::MultiplyColon
            | TokenType::DivideColon
            | TokenType::DivideDotColon
            | TokenType::ModuloColon
            | TokenType::PowerColon
            | TokenType::PowerDotColon
            | TokenType::CommaColon
            | TokenType::FloorDivColon
            | TokenType::Plus
            | TokenType::Minus
            | TokenType::Multiply
            | TokenType::Power
            | TokenType::PowerDot
            | TokenType::Divide
            | TokenType::DivideDot
            | TokenType::Modulo
            | TokenType::Matmul
            | TokenType::FloorDiv
            | TokenType::Equal
            | TokenType::EqualDot
            | TokenType::NotEqual
            | TokenType::NotEqualDot
            | TokenType::LessThan
            | TokenType::LessThanOrEqual
            | TokenType::GreaterThan
            | TokenType::GreaterThanOrEqual
            | TokenType::Sharp
            | TokenType::Range
            | TokenType::RangeInclusive
            | TokenType::Ellipsis => Some(HighlightName::Operator),

            TokenType::Pipe | TokenType::PipeDot | TokenType::PipePipe | TokenType::PipePipeDot => {
                Some(HighlightName::OperatorPipe)
            }

            TokenType::Colon | TokenType::Comma | TokenType::Semicolon => {
                Some(HighlightName::PunctuationDelimiter)
            }

            TokenType::LeftParen
            | TokenType::RightParen
            | TokenType::LeftBracket
            | TokenType::RightBracket
            | TokenType::LeftBrace
            | TokenType::RightBrace => Some(HighlightName::PunctuationBracket),

            TokenType::Apostrophe | TokenType::Backtick | TokenType::Bang => {
                Some(HighlightName::PunctuationSpecial)
            }

            TokenType::FormatString(_, _, _) => None, // handled in events_from_tokens

            TokenType::Error => None,

            TokenType::Newline | TokenType::Eof => None,
        }
    }
}

fn script_items_with_meta(src: &str) -> Option<Vec<ScriptItem>> {
    if !might_have_script_meta(src) {
        return None;
    }
    let items = parse_script_items(src);
    has_script_meta(&items).then_some(items)
}

fn has_script_meta(items: &[ScriptItem]) -> bool {
    items
        .iter()
        .any(|item| matches!(item, ScriptItem::Shebang { .. } | ScriptItem::Directive(_)))
}

fn span_contains_pos(span: ScriptSpan, pos: usize) -> bool {
    span.start <= pos && pos < span.end
}

fn push_offset_events(
    events: &mut Vec<HighlightEvent>,
    chunk_events: Vec<HighlightEvent>,
    offset: usize,
) {
    for event in chunk_events {
        match event {
            HighlightEvent::Source { start, end } => events.push(HighlightEvent::Source {
                start: start + offset,
                end: end + offset,
            }),
            other => events.push(other),
        }
    }
}

fn invalid_literal_error(src: &str, tok: &Token) -> Option<(usize, HighlightName)> {
    let token_src = src.get(tok.byte_start..tok.byte_end)?;
    let trimmed = token_src.trim_start_matches(|ch: char| ch.is_whitespace() && ch != '\n');
    let name = if trimmed.starts_with('"') || trimmed.starts_with("@f") || trimmed.starts_with("@l")
    {
        HighlightName::InvalidString
    } else {
        return None;
    };
    Some((tok.byte_end - trimmed.len(), name))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reconstruct the source string from a sequence of HighlightEvents to
    /// verify that every byte is accounted for and no gaps/overlaps exist.
    fn reconstruct(events: &[HighlightEvent], src: &str) -> String {
        let bytes = src.as_bytes();
        let mut out = String::new();
        for ev in events {
            if let HighlightEvent::Source { start, end } = ev {
                out.push_str(std::str::from_utf8(&bytes[*start..*end]).unwrap_or(""));
            }
        }
        out
    }

    #[test]
    fn test_simple_expression_covers_whole_source() {
        let src = "x: 1";
        let h = Highlighter::new();
        let events = h.highlight(src);
        assert_eq!(reconstruct(&events, src), src);
    }

    #[test]
    fn test_format_string_covers_whole_source() {
        let src = "echo@f\"hello {x}\"";
        let h = Highlighter::new();
        let events = h.highlight(src);
        assert_eq!(reconstruct(&events, src), src);
    }

    #[test]
    fn test_format_string_with_spaces_covers_whole_source() {
        let src = "echo @f\"a {1+2} b\"";
        let h = Highlighter::new();
        let events = h.highlight(src);
        assert_eq!(reconstruct(&events, src), src);
    }

    #[test]
    fn test_multiple_spaces_covers_whole_source() {
        let src = "a   +   b";
        let h = Highlighter::new();
        let events = h.highlight(src);
        assert_eq!(reconstruct(&events, src), src);
    }

    #[test]
    fn test_comment_covers_whole_source() {
        let src = "x:1 // hello";
        let h = Highlighter::new();
        let events = h.highlight(src);
        assert_eq!(reconstruct(&events, src), src);
    }

    #[test]
    fn test_script_directive_covers_whole_source_as_meta() {
        let src = "a:1\n\\l ./lib.wq\nb:2\n";
        let h = Highlighter::new();
        let events = h.highlight(src);

        assert_eq!(reconstruct(&events, src), src);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, HighlightEvent::HighlightStart(HighlightName::Meta)))
        );
    }

    #[test]
    fn test_cursor_context_in_script_directive_is_meta() {
        let src = "a:1\n\\l ./lib.wq\nb:2\n";
        let pos = src.find("lib").expect("directive has lib path");

        assert_eq!(cursor_context_at(src, pos), CursorContext::Meta);
    }

    /// Strip ANSI escape sequences so we can compare visible text.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                // skip until 'm' or end of CSI sequence
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn extended_fg_index(style: &str) -> u8 {
        let marker = "38;5;";
        let start = style
            .find(marker)
            .expect("style has an extended foreground")
            + marker.len();
        let end = style[start..]
            .find('m')
            .map(|offset| start + offset)
            .expect("extended foreground has a terminator");
        style[start..end]
            .parse()
            .expect("extended foreground index is numeric")
    }

    fn xterm_rgb(index: u8) -> [u8; 3] {
        if index >= 232 {
            let level = 8 + (index - 232) * 10;
            return [level; 3];
        }
        let value = index - 16;
        let channel = |step: u8| if step == 0 { 0 } else { 55 + step * 40 };
        [
            channel(value / 36),
            channel((value % 36) / 6),
            channel(value % 6),
        ]
    }

    fn relative_luminance(rgb: [u8; 3]) -> f64 {
        let linear = |channel: u8| {
            let channel = f64::from(channel) / 255.0;
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgb[0]) + 0.7152 * linear(rgb[1]) + 0.0722 * linear(rgb[2])
    }

    fn contrast_ratio(left: [u8; 3], right: [u8; 3]) -> f64 {
        let left = relative_luminance(left);
        let right = relative_luminance(right);
        (left.max(right) + 0.05) / (left.min(right) + 0.05)
    }

    /// Collect contiguous (text, highlight_name) pairs from events.
    fn named_regions(events: &[HighlightEvent], src: &str) -> Vec<(String, Option<HighlightName>)> {
        let bytes = src.as_bytes();
        let mut stack: Vec<HighlightName> = Vec::new();
        let mut out = Vec::new();
        for ev in events {
            match ev {
                HighlightEvent::HighlightStart(n) => stack.push(*n),
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } => {
                    let s = std::str::from_utf8(&bytes[*start..*end]).unwrap_or("");
                    out.push((s.to_string(), stack.last().copied()));
                }
            }
        }
        out
    }

    fn assert_region(regions: &[(String, Option<HighlightName>)], text: &str, name: HighlightName) {
        assert!(
            regions
                .iter()
                .any(|(region_text, region_name)| region_text == text && *region_name == Some(name)),
            "expected {text:?} to have highlight {name:?}; regions: {regions:?}"
        );
    }

    fn regions_with_name(
        regions: &[(String, Option<HighlightName>)],
        name: HighlightName,
    ) -> Vec<&str> {
        regions
            .iter()
            .filter_map(|(text, region_name)| (*region_name == Some(name)).then_some(text.as_str()))
            .collect()
    }

    #[test]
    fn valid_string_and_character_escapes_use_special_highlight() {
        let src = r#""prefix\nsuffix" "\r" "\t" "\0" "\\" "\"" "\'" "\x41" "\u{1f4a9}""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert_eq!(
            regions_with_name(&regions, HighlightName::StringEscape),
            vec![
                r"\n",
                r"\r",
                r"\t",
                r"\0",
                r"\\",
                r#"\""#,
                r"\'",
                r"\x41",
                r"\u{1f4a9}",
            ]
        );
    }

    #[test]
    fn invalid_and_unknown_escapes_do_not_use_special_highlight() {
        let src = r#""\q" "\x" "\x4" "\xGG" "\u" "\u1234" "\U0001f980" "\u{}" "\u{d800}" "\u{110000}" "\u{0000000}" "\N{}" "\N{NOT A NAME}""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert!(regions_with_name(&regions, HighlightName::StringEscape).is_empty());
    }

    #[test]
    fn malformed_hex_and_unicode_escapes_mark_the_whole_string_invalid() {
        let src = r#""ok" "\x41" "\x" "\x4" "\xGG" "\u1234" "\U0001f980" "\u{}z" "\u{d800}" "\N{}" "\N{NOT A NAME}" @f"\x" "\q""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert_region(&regions, r#""ok""#, HighlightName::String);
        assert_region(&regions, r"\x41", HighlightName::StringEscape);
        assert_region(&regions, r#""\q""#, HighlightName::String);
        assert_eq!(
            regions_with_name(&regions, HighlightName::InvalidString),
            vec![
                r#""\x""#,
                r#""\x4""#,
                r#""\xGG""#,
                r#""\u1234""#,
                r#""\U0001f980""#,
                r#""\u{}z""#,
                r#""\u{d800}""#,
                r#""\N{}""#,
                r#""\N{NOT A NAME}""#,
                r#"@f"\x""#
            ]
        );
    }

    #[test]
    fn invalid_string_cursor_context_stays_inside_the_string() {
        let src = r#""\u{}z""#;
        let pos = src.find('z').expect("invalid string contains z") + 1;

        assert_eq!(cursor_context_at(src, pos), CursorContext::String);
    }

    #[test]
    fn quoted_literals_distinguish_char_and_string_regions() {
        let src = r#""a" "\n" "\N{SNOWMAN}" "ab" "é" "\N{KEYCAP DIGIT ONE}""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert_region(&regions, r#""a""#, HighlightName::Character);
        assert_region(&regions, r#""ab""#, HighlightName::String);
        assert_region(&regions, r#""é""#, HighlightName::String);
        assert_region(&regions, r"\n", HighlightName::StringEscape);
        assert_region(&regions, r"\N{SNOWMAN}", HighlightName::StringEscape);
        assert_region(
            &regions,
            r"\N{KEYCAP DIGIT ONE}",
            HighlightName::StringEscape,
        );
    }

    #[test]
    fn invalid_unicode_name_stays_in_string_context() {
        let src = r#""\N{NOT A NAME}""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert_eq!(
            regions_with_name(&regions, HighlightName::InvalidString),
            vec![src]
        );
        assert_eq!(cursor_context_at(src, src.len() - 1), CursorContext::String);
    }

    #[test]
    fn raw_string_escapes_remain_plain_string_content() {
        let src = r#"@l"\n\x41\u{41}""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert!(regions_with_name(&regions, HighlightName::StringEscape).is_empty());
        assert_region(&regions, src, HighlightName::String);
    }

    #[test]
    fn format_string_text_highlights_only_valid_escapes() {
        let src = r#"@f"line\n \x41 \u{10ffff} {x} \q""#;
        let h = Highlighter::new();
        let regions = named_regions(&h.highlight(src), src);

        assert_eq!(
            regions_with_name(&regions, HighlightName::StringEscape),
            vec![r"\n", r"\x41", r"\u{10ffff}"]
        );
    }

    #[test]
    fn test_w_loop_keyword_highlight() {
        let src = "W[1;2]";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("W".to_string(), Some(HighlightName::Keyword)));
    }

    #[test]
    fn test_n_loop_keyword_highlight() {
        let src = "N[3;x]";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("N".to_string(), Some(HighlightName::Keyword)));
    }

    #[test]
    fn reserved_control_names_are_always_keywords() {
        let src = "W N B A and O or";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(
            regions
                .iter()
                .filter_map(|(text, name)| (*name == Some(HighlightName::Keyword)).then_some(text))
                .collect::<Vec<_>>(),
            ["W", "N", "B", "A", "and", "O", "or"]
        );
    }

    #[test]
    fn test_cas_quote_highlights_cas_names() {
        let src = "@s limit[sin[x]/x;pi]+root[t^3-t-1;t;1;2]+e+oo+_oo+undef";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);

        assert_region(&regions, "limit", HighlightName::CasSpecial);
        assert_region(&regions, "root", HighlightName::CasSpecial);
        assert_region(&regions, "sin", HighlightName::CasFunction);
        assert_region(&regions, "pi", HighlightName::CasConstant);
        assert_region(&regions, "e", HighlightName::CasConstant);
        assert_region(&regions, "oo", HighlightName::CasConstant);
        assert_region(&regions, "_oo", HighlightName::CasConstant);
        assert_region(&regions, "undef", HighlightName::CasConstant);
        assert_region(&regions, "x", HighlightName::CasVariable);
        assert_region(&regions, "t", HighlightName::CasVariable);
    }

    #[test]
    fn test_cas_quote_highlight_stops_at_expression_boundary() {
        let src = "@s inf; pi";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);

        assert_region(&regions, "inf", HighlightName::CasConstant);
        assert_region(&regions, "pi", HighlightName::Variable);
    }

    #[test]
    fn test_w_loop_with_block_comment_gap() {
        let src = "W/*comment*/[1;2]";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("W".to_string(), Some(HighlightName::Keyword)));
    }

    #[test]
    fn test_reserved_keyword_before_line_comment() {
        let src = "W//comment\n[1;2]";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("W".to_string(), Some(HighlightName::Keyword)));
    }

    #[test]
    fn test_rainbow_brackets_paren_nesting() {
        let src = "((()))";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions: Vec<_> = named_regions(&events, src)
            .into_iter()
            .filter(|(s, _)| !s.is_empty())
            .collect();
        assert_eq!(
            regions,
            vec![
                ("(".to_string(), Some(HighlightName::PunctuationBracket1)),
                ("(".to_string(), Some(HighlightName::PunctuationBracket2)),
                ("(".to_string(), Some(HighlightName::PunctuationBracket3)),
                (")".to_string(), Some(HighlightName::PunctuationBracket3)),
                (")".to_string(), Some(HighlightName::PunctuationBracket2)),
                (")".to_string(), Some(HighlightName::PunctuationBracket1)),
            ]
        );
    }

    #[test]
    fn test_rainbow_brackets_mixed_types() {
        let src = "([]{})";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions: Vec<_> = named_regions(&events, src)
            .into_iter()
            .filter(|(s, _)| !s.is_empty())
            .collect();
        assert_eq!(
            regions,
            vec![
                ("(".to_string(), Some(HighlightName::PunctuationBracket1)),
                ("[".to_string(), Some(HighlightName::PunctuationBracket1)),
                ("]".to_string(), Some(HighlightName::PunctuationBracket1)),
                ("{".to_string(), Some(HighlightName::PunctuationBracket1)),
                ("}".to_string(), Some(HighlightName::PunctuationBracket1)),
                (")".to_string(), Some(HighlightName::PunctuationBracket1)),
            ]
        );
    }

    #[test]
    fn test_rainbow_brackets_depth_cycles() {
        let src = "((((((()))))))";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions: Vec<_> = named_regions(&events, src)
            .into_iter()
            .filter(|(s, _)| !s.is_empty())
            .collect();
        let opens: Vec<_> = [
            HighlightName::PunctuationBracket1,
            HighlightName::PunctuationBracket2,
            HighlightName::PunctuationBracket3,
            HighlightName::PunctuationBracket4,
            HighlightName::PunctuationBracket5,
            HighlightName::PunctuationBracket6,
            HighlightName::PunctuationBracket1,
        ]
        .iter()
        .map(|n| ("(".to_string(), Some(*n)))
        .collect();
        let closes: Vec<_> = [
            HighlightName::PunctuationBracket1,
            HighlightName::PunctuationBracket6,
            HighlightName::PunctuationBracket5,
            HighlightName::PunctuationBracket4,
            HighlightName::PunctuationBracket3,
            HighlightName::PunctuationBracket2,
            HighlightName::PunctuationBracket1,
        ]
        .iter()
        .map(|n| (")".to_string(), Some(*n)))
        .collect();
        let expected: Vec<_> = opens.into_iter().chain(closes).collect();
        assert_eq!(regions, expected);
    }

    #[test]
    fn test_rainbow_brackets_unmatched_close() {
        let src = ")}";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions: Vec<_> = named_regions(&events, src)
            .into_iter()
            .filter(|(s, _)| !s.is_empty())
            .collect();
        assert_eq!(
            regions,
            vec![
                (")".to_string(), Some(HighlightName::PunctuationBracket)),
                ("}".to_string(), Some(HighlightName::PunctuationBracket)),
            ]
        );
    }

    #[test]
    fn test_colorize_roundtrip() {
        let src = "echo @f\"hello {x + 1}\"";
        let h = Highlighter::new();
        let out = h.highlight_ansi(src);

        assert!(out.contains("\x1b[38;5;220m1"));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn ansi_styles_preserve_semantic_and_structural_cues() {
        assert_eq!(
            ansi_style_for_name(HighlightName::FunctionBuiltin).0,
            "\x1b[4;38;5;75m"
        );
        assert_eq!(
            ansi_style_for_name(HighlightName::Operator).0,
            "\x1b[38;5;211m"
        );
        assert_eq!(
            ansi_style_for_name(HighlightName::PunctuationDelimiter).0,
            "\x1b[38;5;248m"
        );
        assert_eq!(
            ansi_style_for_name(HighlightName::PunctuationBracket1).0,
            "\x1b[1;38;5;210m"
        );
        assert_eq!(
            ansi_style_for_name(HighlightName::PunctuationBracket2).0,
            "\x1b[38;5;215m"
        );
    }

    #[test]
    fn fixed_ansi_palette_is_readable_on_repl_input_background() {
        let names = [
            HighlightName::Comment,
            HighlightName::ConstantBuiltin,
            HighlightName::FunctionBuiltin,
            HighlightName::CasSpecial,
            HighlightName::CasConstant,
            HighlightName::CasFunction,
            HighlightName::CasVariable,
            HighlightName::Keyword,
            HighlightName::KeywordReturn,
            HighlightName::KeywordDebug,
            HighlightName::Number,
            HighlightName::Bool,
            HighlightName::Operator,
            HighlightName::OperatorPipe,
            HighlightName::PunctuationBracket,
            HighlightName::PunctuationBracket1,
            HighlightName::PunctuationBracket2,
            HighlightName::PunctuationBracket3,
            HighlightName::PunctuationBracket4,
            HighlightName::PunctuationBracket5,
            HighlightName::PunctuationBracket6,
            HighlightName::PunctuationDelimiter,
            HighlightName::PunctuationSpecial,
            HighlightName::String,
            HighlightName::StringEscape,
            HighlightName::InvalidString,
            HighlightName::Character,
            HighlightName::InvalidCharacter,
            HighlightName::Tag,
            HighlightName::Variable,
            HighlightName::VariableRefCapture,
            HighlightName::VariableParameter,
            HighlightName::Meta,
        ];
        let repl_background = [48, 48, 48];

        for name in names {
            let index = extended_fg_index(ansi_style_for_name(name).0);
            let contrast = contrast_ratio(xterm_rgb(index), repl_background);
            assert!(
                contrast >= 4.5,
                "{name:?} uses xterm color {index} at only {contrast:.2}:1 contrast"
            );
        }
    }

    #[test]
    fn semantic_ansi_uses_named_terminal_colors() {
        let src = "echo @f\"hello {x + 1}\"";
        let h = Highlighter::new();
        let out = h.highlight_ansi_semantic(src);

        assert!(out.contains("\x1b[33m1"));
        assert!(!out.contains("\x1b[38;5;"));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn ref_capture_spans_use_special_highlight() {
        let src = "a:1; f:'{[] a}; f[]";
        let h = Highlighter::new();
        let events = h.highlight_with_ref_captures(src, &[(12, 13)]);
        let regions = named_regions(&events, src);

        assert!(regions.contains(&("a".to_string(), Some(HighlightName::VariableRefCapture))));

        let out = h.highlight_ansi_with_ref_captures_and_reset(src, &[(12, 13)], "");
        assert!(out.contains("\x1b[1;38;5;39ma"));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn semantic_spans_emit_variable_parameter() {
        let src = "f:{[x] x+1}";
        let h = Highlighter::new();
        let spans = [
            SemanticHighlightSpan {
                span: (4, 5),
                name: HighlightName::VariableParameter,
            },
            SemanticHighlightSpan {
                span: (7, 8),
                name: HighlightName::VariableParameter,
            },
        ];
        let regions = named_regions(&h.highlight_with_semantic_spans(src, &spans), src);

        assert!(regions.contains(&("x".to_string(), Some(HighlightName::VariableParameter))));
    }

    #[test]
    fn cursor_context_tracks_multiline_string() {
        let src = "\"hello\nsu";
        assert_eq!(cursor_context_at(src, src.len()), CursorContext::String);
    }

    #[test]
    fn cursor_context_leaves_closed_multiline_string() {
        let src = "\"hello\nworld\" su";
        let pos = src.len();
        assert_eq!(cursor_context_at(src, pos), CursorContext::Code);
    }

    #[test]
    fn cursor_context_tracks_nested_block_comment() {
        let src = "1 /* outer /* inner */ st";
        let pos = src.len();
        assert_eq!(cursor_context_at(src, pos), CursorContext::Comment);
    }

    #[test]
    fn cursor_context_distinguishes_fstring_text_and_expr() {
        let src = "@f \"hello {ec}\"";
        let text_pos = src.find("hello").expect("text") + 2;
        let expr_pos = src.find("ec").expect("expr") + 2;

        assert_eq!(cursor_context_at(src, text_pos), CursorContext::FStringText);
        assert_eq!(cursor_context_at(src, expr_pos), CursorContext::FStringExpr);
    }

    #[test]
    fn cursor_context_suppresses_string_inside_fstring_expr() {
        let src = "@f\"{foo[\"bar\"]}\"";
        let pos = src.find("bar").expect("inner string") + 2;

        assert_eq!(cursor_context_at(src, pos), CursorContext::String);
    }
}
