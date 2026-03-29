use std::collections::HashSet;

use crate::builtins::Builtins;
use crate::lexer::Lexer;
use crate::token::{Token, TokenType};

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
        let mut lexer = Lexer::new(src).with_skip_directives(true);
        let tokens = lexer.tokenize_recovery();
        let keyword_spans = Self::keyword_spans_from_tokens(&tokens);
        Self::events_from_tokens(&tokens, &self.builtins, &keyword_spans)
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
                "S" => TokenType::LeftParen,
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

    fn events_from_tokens(
        tokens: &[Token],
        builtins: &Builtins,
        keyword_spans: &HashSet<(usize, usize)>,
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
                                    let inner_events = Self::events_from_tokens(
                                        &inner_tokens,
                                        builtins,
                                        &HashSet::new(),
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
                        let name = Self::name_for_token(tok, builtins, keyword_spans);
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
            | TokenType::DotAmpersandColon
            | TokenType::DotBackslashColon
            | TokenType::DotCaretColon
            | TokenType::DotMinusColon
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
            | TokenType::Ellipsis
            | TokenType::DotAmpersand
            | TokenType::DotBackslash
            | TokenType::DotCaret
            | TokenType::DotMinus
            | TokenType::DotLessThan
            | TokenType::DotLessThanOrEqual
            | TokenType::DotGreaterThan
            | TokenType::DotGreaterThanOrEqual => Some(HighlightName::Operator),

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
    fn test_s_set_keyword_highlight() {
        let src = "S(1,2)";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("S".to_string(), Some(HighlightName::Keyword)));
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
    fn test_s_as_variable_when_not_set() {
        let src = "S + 1";
        let h = Highlighter::new();
        let events = h.highlight(src);
        let regions = named_regions(&events, src);
        assert_eq!(regions[0], ("S".to_string(), Some(HighlightName::Variable)));
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
        // Simulate what TSHelper::colorize does.
        let src = "echo @f\"hello {x + 1}\"";
        let h = Highlighter::new();
        let events = h.highlight(src);

        let mut out = String::new();
        let bytes = src.as_bytes();
        let mut stack: Vec<HighlightName> = Vec::new();
        for ev in events {
            match ev {
                HighlightEvent::HighlightStart(n) => stack.push(n),
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } => {
                    let s = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                    if let Some(&_name) = stack.last() {
                        // In real code this would wrap with ANSI codes.
                        // We add a dummy prefix so strip_ansi has work to do.
                        out.push_str("\x1b[38;5;1m");
                        out.push_str(s);
                        out.push_str("\x1b[0m");
                    } else {
                        out.push_str(s);
                    }
                }
            }
        }
        assert_eq!(strip_ansi(&out), src);
    }
}
