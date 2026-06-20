use std::collections::HashSet;

use crate::builtins::Builtins;
use crate::lex::Lexer;
use crate::script::{ScriptItem, ScriptSpan, parse_script_items};
use crate::token::{Token, TokenType};

pub const ANSI_RESET: &str = "\x1b[0m";

/// Capture names that mirror `highlights.scm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightName {
    Comment,
    Constant,
    ConstantBuiltin,
    Function,
    FunctionCall,
    FunctionBuiltin,
    Keyword,
    KeywordReturn,
    KeywordDebug,
    Module,
    Number,
    Boolean,
    Operator,
    OperatorPipe,
    Property,
    PropertyBuiltin,
    Punctuation,
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
    StringSpecial,
    Tag,
    Type,
    TypeBuiltin,
    Variable,
    VariableOuter,
    VariableRefCapture,
    VariableBuiltin,
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
        HighlightName::Constant => ("\x1b[38;5;220m", ANSI_RESET),
        HighlightName::ConstantBuiltin => ("\x1b[1;38;5;220m", ANSI_RESET),
        HighlightName::Function => ("\x1b[38;5;75m", ANSI_RESET),
        HighlightName::FunctionCall => ("\x1b[1;38;5;75m", ANSI_RESET),
        HighlightName::FunctionBuiltin => ("\x1b[4;38;5;213m", ANSI_RESET),
        HighlightName::Keyword => ("\x1b[38;5;199m", ANSI_RESET),
        HighlightName::KeywordReturn => ("\x1b[38;5;220m", ANSI_RESET),
        HighlightName::KeywordDebug => ("\x1b[38;5;210m", ANSI_RESET),
        HighlightName::Number => ("\x1b[38;5;220m", ANSI_RESET),
        HighlightName::Boolean => ("\x1b[38;5;220m", ANSI_RESET),
        HighlightName::Operator => ("\x1b[38;5;208m", ANSI_RESET),
        HighlightName::OperatorPipe => ("\x1b[38;5;170m", ANSI_RESET),
        HighlightName::Punctuation => ("\x1b[38;5;245m", ANSI_RESET),
        HighlightName::PunctuationBracket => ("\x1b[38;5;245m", ANSI_RESET),
        HighlightName::PunctuationBracket1 => ("\x1b[38;5;203m", ANSI_RESET),
        HighlightName::PunctuationBracket2 => ("\x1b[38;5;215m", ANSI_RESET),
        HighlightName::PunctuationBracket3 => ("\x1b[38;5;222m", ANSI_RESET),
        HighlightName::PunctuationBracket4 => ("\x1b[38;5;114m", ANSI_RESET),
        HighlightName::PunctuationBracket5 => ("\x1b[38;5;111m", ANSI_RESET),
        HighlightName::PunctuationBracket6 => ("\x1b[38;5;183m", ANSI_RESET),
        HighlightName::PunctuationDelimiter => ("\x1b[38;5;243m", ANSI_RESET),
        HighlightName::PunctuationSpecial => ("\x1b[38;5;170m", ANSI_RESET),
        HighlightName::String => ("\x1b[38;5;113m", ANSI_RESET),
        HighlightName::Tag => ("\x1b[38;5;113m", ANSI_RESET),
        HighlightName::Variable => ("\x1b[38;5;117m", ANSI_RESET),
        HighlightName::VariableOuter => ("\x1b[38;5;199m", ANSI_RESET),
        HighlightName::VariableRefCapture => ("\x1b[38;5;39m", ANSI_RESET),
        HighlightName::VariableBuiltin => ("\x1b[4;38;5;213m", ANSI_RESET),
        HighlightName::VariableParameter => ("\x1b[38;5;215m", ANSI_RESET),
        HighlightName::Meta => ("\x1b[38;5;228m", ANSI_RESET),
        _ => (ANSI_RESET, ANSI_RESET),
    }
}

pub fn render_ansi(
    src: &str,
    events: impl IntoIterator<Item = HighlightEvent>,
    reset: &str,
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
                    let (on, off) = ansi_style_for_name(name);
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
        Self {
            builtins: Builtins::new(),
        }
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
        let keyword_spans = Self::keyword_spans_from_tokens(&tokens);
        Self::events_from_tokens(&tokens, &self.builtins, &keyword_spans, semantic_spans)
    }

    pub fn highlight_ansi(&self, src: &str) -> String {
        self.highlight_ansi_with_reset(src, "")
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

    /// Mirror the parser's special-treatment logic for `W`, `N`, and `S`:
    /// an Identifier is a keyword only when it is immediately followed
    /// (allowing only comments in between, exactly as the parser does) by
    /// the correct bracket type.
    fn keyword_spans_from_tokens(tokens: &[Token]) -> HashSet<(usize, usize)> {
        let mut spans = HashSet::new();
        for (i, tok) in tokens.iter().enumerate() {
            let TokenType::Identifier(name) = &tok.token_type else {
                continue;
            };
            let target = match name.as_str() {
                "W" | "N" => TokenType::LeftBracket,

                _ => continue,
            };
            let mut j = i + 1;
            while j < tokens.len() && matches!(tokens[j].token_type, TokenType::Comment(_)) {
                j += 1;
            }
            if j < tokens.len() && tokens[j].token_type == target {
                spans.insert((tok.byte_start, tok.byte_end));
            }
        }
        spans
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
                    let keyword_spans = Self::keyword_spans_from_tokens(&tokens);
                    let nested_semantic_spans =
                        Self::nested_semantic_spans(semantic_spans, span.start, span.end);
                    let chunk_events = Self::events_from_tokens(
                        &tokens,
                        &self.builtins,
                        &keyword_spans,
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
        tokens: &[Token],
        builtins: &Builtins,
        keyword_spans: &HashSet<(usize, usize)>,
        semantic_spans: &[SemanticHighlightSpan],
    ) -> Vec<HighlightEvent> {
        let mut events = Vec::with_capacity(tokens.len() * 2);
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
                                    events.push(HighlightEvent::HighlightStart(
                                        HighlightName::String,
                                    ));
                                    events.push(HighlightEvent::Source {
                                        start: *start,
                                        end: *end,
                                    });
                                    events.push(HighlightEvent::HighlightEnd);
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
                                        Self::nested_semantic_spans(semantic_spans, *start, *end);
                                    let inner_events = Self::events_from_tokens(
                                        &inner_tokens,
                                        builtins,
                                        &HashSet::new(),
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
                    _ => {
                        let name = Self::semantic_name_for_identifier(tok, semantic_spans)
                            .or_else(|| Self::name_for_token(tok, builtins, keyword_spans));
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

    fn semantic_name_for_identifier(
        tok: &Token,
        semantic_spans: &[SemanticHighlightSpan],
    ) -> Option<HighlightName> {
        if !matches!(tok.token_type, TokenType::Identifier(_)) {
            return None;
        }
        semantic_spans
            .iter()
            .find(|semantic_span| {
                let (start, end) = semantic_span.span;
                start < tok.byte_end && tok.byte_start < end
            })
            .map(|semantic_span| semantic_span.name)
    }

    fn name_for_token(
        tok: &Token,
        builtins: &Builtins,
        keyword_spans: &HashSet<(usize, usize)>,
    ) -> Option<HighlightName> {
        match &tok.token_type {
            TokenType::Comment(_) => Some(HighlightName::Comment),

            TokenType::Integer(_)
            | TokenType::BigInteger(_)
            | TokenType::Float(_)
            | TokenType::Imaginary(_) => Some(HighlightName::Number),

            TokenType::Inf => Some(HighlightName::ConstantBuiltin),
            TokenType::True | TokenType::False => Some(HighlightName::Boolean),

            TokenType::String(_) => Some(HighlightName::String),
            TokenType::Character(_) => Some(HighlightName::String),
            TokenType::Tag(_) => Some(HighlightName::Tag),

            TokenType::Identifier(name) => {
                if keyword_spans.contains(&(tok.byte_start, tok.byte_end)) {
                    Some(HighlightName::Keyword)
                } else if builtins.is_known_name(name) {
                    Some(HighlightName::FunctionBuiltin)
                } else {
                    Some(HighlightName::Variable)
                }
            }

            TokenType::AtReturn => Some(HighlightName::KeywordReturn),
            TokenType::AtDebug => Some(HighlightName::KeywordDebug),
            TokenType::AtAssert
            | TokenType::AtBreak
            | TokenType::AtContinue
            | TokenType::AtDepth(_)
            | TokenType::AtPause
            | TokenType::AtSymbolic
            | TokenType::AtTry => Some(HighlightName::Keyword),

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
            | TokenType::BoolAndColon
            | TokenType::BoolOrColon
            | TokenType::BitAndColon
            | TokenType::BitOrColon
            | TokenType::ShlColon
            | TokenType::ShrColon
            | TokenType::BitXorColon
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
            | TokenType::BoolAnd
            | TokenType::BoolOr
            | TokenType::BitAnd
            | TokenType::BitOr
            | TokenType::Shl
            | TokenType::Shr
            | TokenType::BitXor
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
    if !src.contains('!') {
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
        let src = "a:1\n!l ./lib.wq\nb:2\n";
        let h = Highlighter::new();
        let events = h.highlight(src);

        assert_eq!(reconstruct(&events, src), src);
        assert!(events.iter().any(|event| matches!(
            event,
            HighlightEvent::HighlightStart(HighlightName::Meta)
        )));
    }

    #[test]
    fn test_cursor_context_in_script_directive_is_meta() {
        let src = "a:1\n!l ./lib.wq\nb:2\n";
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
    fn test_w_as_variable_when_not_loop() {
        let src = "W + 1";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("W".to_string(), Some(HighlightName::Variable)));
    }

    #[test]
    fn test_n_as_variable_when_not_loop() {
        let src = "N + 1";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("N".to_string(), Some(HighlightName::Variable)));
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
    fn test_w_loop_with_line_comment_not_keyword() {
        // Line comment swallows the rest of the line, so '[' is never seen.
        let src = "W//comment\n[1;2]";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("W".to_string(), Some(HighlightName::Variable)));
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
    fn ref_capture_spans_use_special_highlight() {
        let src = "a:1; f:'{[] a}; f[]";
        let h = Highlighter::new();
        let events = h.highlight_with_ref_captures(src, &[(12, 13)]);
        let regions = named_regions(&events, src);

        assert!(regions.contains(&("a".to_string(), Some(HighlightName::VariableRefCapture))));

        let out = h.highlight_ansi_with_ref_captures_and_reset(src, &[(12, 13)], "");
        assert!(out.contains("\x1b[38;5;39ma"));
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
