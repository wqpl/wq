use std::collections::HashMap;

use crate::builtins::{BuiltinNamedArg, Builtins};
use crate::doc::DocTopic;
use crate::frontend::Frontend;
use crate::highlight::cursor_context_at;
use crate::symbol::DefKind;
use crate::token::{FmtPart, Token, TokenType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Assignment,
    Function,
    Parameter,
    ImplicitParam,
    LoopCounter,
    Builtin,
}

impl CompletionKind {
    fn from_def_kind(kind: DefKind) -> Self {
        match kind {
            DefKind::Assignment => Self::Assignment,
            DefKind::Function => Self::Function,
            DefKind::Parameter => Self::Parameter,
            DefKind::ImplicitParam => Self::ImplicitParam,
            DefKind::LoopCounter => Self::LoopCounter,
            DefKind::Builtin => Self::Builtin,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompletionCandidate {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub documentation: Option<DocTopic>,
    pub named_args: Vec<BuiltinNamedArg>,
}

impl CompletionCandidate {
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
            named_args: Vec::new(),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_documentation(mut self, documentation: DocTopic) -> Self {
        self.documentation = Some(documentation);
        self
    }

    pub fn with_named_args(mut self, named_args: &[BuiltinNamedArg]) -> Self {
        self.named_args.extend_from_slice(named_args);
        self
    }
}

pub fn expression_completion_candidates(
    frontend: &Frontend,
    content: &str,
) -> Vec<CompletionCandidate> {
    let mut seen = HashMap::new();

    if let Ok(index) = frontend.analyze_symbols(content) {
        for def in &index.defs {
            if def.kind == DefKind::Builtin {
                continue;
            }
            let candidate =
                CompletionCandidate::new(def.name.clone(), CompletionKind::from_def_kind(def.kind));
            seen.entry(def.name.clone()).or_insert(candidate);
        }
    } else {
        for name in fallback_assignment_names(frontend, content) {
            seen.entry(name.clone())
                .or_insert_with(|| CompletionCandidate::new(name, CompletionKind::Assignment));
        }
    }

    for candidate in builtin_completion_candidates(frontend.builtins(), true) {
        seen.entry(candidate.label.clone()).or_insert(candidate);
    }

    let mut candidates: Vec<_> = seen.into_values().collect();
    candidates.sort_by(|a, b| a.label.cmp(&b.label));
    candidates
}

pub fn builtin_completion_candidates(
    builtins: &Builtins,
    include_disabled: bool,
) -> Vec<CompletionCandidate> {
    let names = if include_disabled {
        builtins.list_functions_all()
    } else {
        builtins.list_functions()
    };
    let mut candidates = Vec::with_capacity(names.len());
    for name in names {
        let mut candidate = CompletionCandidate::new(name.clone(), CompletionKind::Builtin);
        if let Some(id) = builtins.get_id(&name)
            && let Ok(id) = u16::try_from(id)
            && let Some(usage) = Builtins::usage_from_id(id)
        {
            candidate = candidate.with_detail(usage);
            if let Some(named_args) = Builtins::named_args_from_id(id) {
                candidate = candidate.with_named_args(named_args);
            }
        }
        if let Some(topic) = builtins.doc_for_name(&name) {
            candidate = candidate.with_documentation(topic);
        }
        candidates.push(candidate);
    }
    candidates.sort_by(|a, b| a.label.cmp(&b.label));
    candidates
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltinNamedArgCompletionContext {
    pub builtin_name: String,
    pub prefix: String,
    pub replace_start: usize,
    pub used_names: Vec<String>,
}

pub fn builtin_named_arg_completion_context(
    frontend: &Frontend,
    content: &str,
    byte_offset: usize,
) -> Option<BuiltinNamedArgCompletionContext> {
    let byte_offset = byte_offset.min(content.len());
    if !content.is_char_boundary(byte_offset) {
        return None;
    }

    let tokens = frontend.tokenize_recovery(content);
    let (current_index, current) = tokens.iter().enumerate().find(|(_, token)| {
        token.byte_start < byte_offset
            && byte_offset <= token.byte_end
            && matches!(&token.token_type, TokenType::Tag(_) | TokenType::Backtick)
    })?;
    let prefix_start = current.byte_start.checked_add(1)?;
    let prefix = content.get(prefix_start..byte_offset)?;
    if !prefix.chars().all(is_word_char) {
        return None;
    }

    let mut delimiters = Vec::new();
    for (index, token) in tokens[..current_index].iter().enumerate() {
        match &token.token_type {
            TokenType::LeftBracket => {
                delimiters.push((CompletionDelimiter::Bracket, index));
            }
            TokenType::LeftParen => delimiters.push((CompletionDelimiter::Paren, index)),
            TokenType::LeftBrace => delimiters.push((CompletionDelimiter::Brace, index)),
            TokenType::RightBracket => {
                pop_matching(&mut delimiters, CompletionDelimiter::Bracket);
            }
            TokenType::RightParen => {
                pop_matching(&mut delimiters, CompletionDelimiter::Paren);
            }
            TokenType::RightBrace => {
                pop_matching(&mut delimiters, CompletionDelimiter::Brace);
            }
            _ => {}
        }
    }
    let (delimiter, bracket_index) = delimiters.last()?;
    if *delimiter != CompletionDelimiter::Bracket {
        return None;
    }

    let callee = tokens[..*bracket_index]
        .iter()
        .rev()
        .find(|token| significant_token(token))
        .and_then(|token| match &token.token_type {
            TokenType::Identifier(name) => Some(name.as_str()),
            _ => None,
        })?;
    if !frontend.builtins().is_enabled_name(callee) {
        return None;
    }

    let mut nested = 0usize;
    let mut last_top_level = None;
    let mut used_names = Vec::new();
    let call_tokens = &tokens[*bracket_index + 1..current_index];
    for (index, token) in call_tokens.iter().enumerate() {
        match &token.token_type {
            TokenType::LeftBracket | TokenType::LeftParen | TokenType::LeftBrace => {
                nested += 1;
            }
            TokenType::RightBracket | TokenType::RightParen | TokenType::RightBrace => {
                nested = nested.saturating_sub(1);
            }
            _ if nested == 0 && significant_token(token) => {
                last_top_level = Some(&token.token_type);
                if let TokenType::Tag(name) = &token.token_type
                    && call_tokens[index + 1..]
                        .iter()
                        .find(|next| significant_token(next))
                        .is_some_and(|next| matches!(&next.token_type, TokenType::Colon))
                {
                    used_names.push(name.clone());
                }
            }
            _ => {}
        }
    }
    if last_top_level.is_some_and(|token| !matches!(token, TokenType::Semicolon)) {
        return None;
    }

    Some(BuiltinNamedArgCompletionContext {
        builtin_name: callee.to_string(),
        prefix: prefix.to_string(),
        replace_start: current.byte_start,
        used_names,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompletionDelimiter {
    Bracket,
    Paren,
    Brace,
}

fn pop_matching(delimiters: &mut Vec<(CompletionDelimiter, usize)>, expected: CompletionDelimiter) {
    if delimiters
        .last()
        .is_some_and(|(delimiter, _)| *delimiter == expected)
    {
        delimiters.pop();
    }
}

fn significant_token(token: &Token) -> bool {
    !matches!(
        &token.token_type,
        TokenType::Comment(_) | TokenType::Newline | TokenType::Eof
    )
}

pub fn should_suppress_expression_completion(
    frontend: &Frontend,
    content: &str,
    byte_offset: usize,
) -> bool {
    is_in_no_completion_zone(frontend, content, byte_offset)
        || is_typing_non_ident(frontend, content, byte_offset)
}

fn fallback_assignment_names(frontend: &Frontend, content: &str) -> Vec<String> {
    let tokens = frontend.tokenize_recovery(content);
    let mut names = Vec::new();
    for window in tokens.windows(2) {
        if let (
            Token {
                token_type: TokenType::Identifier(name),
                ..
            },
            Token {
                token_type: TokenType::Colon,
                ..
            },
        ) = (&window[0], &window[1])
        {
            names.push(name.clone());
        }
    }
    names
}

fn is_in_no_completion_zone(frontend: &Frontend, content: &str, byte_offset: usize) -> bool {
    let clamped = byte_offset.min(content.len());

    let line_start = content[..clamped].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line_end = content[clamped..]
        .find('\n')
        .map(|i| clamped + i)
        .unwrap_or(content.len());
    let line = &content[line_start..line_end];
    if line_start == 0 && line.starts_with("#!") {
        return true;
    }
    if line.trim_start().starts_with('\\') {
        return true;
    }

    let tokens = frontend.tokenize_recovery(content);
    let Some(tok) = tokens
        .iter()
        .find(|t| t.byte_start <= byte_offset && byte_offset < t.byte_end)
    else {
        return false;
    };

    match &tok.token_type {
        TokenType::Comment(_) | TokenType::String(_) | TokenType::Character(_) => true,
        TokenType::FormatString(parts, _, _) => {
            let in_expr = parts.iter().any(|p| {
                matches!(p, FmtPart::Expr { start, end, .. } if *start <= byte_offset && byte_offset < *end)
            });
            !in_expr
        }
        TokenType::Error => cursor_context_at(content, byte_offset).suppresses_completion(),
        _ => false,
    }
}

fn is_typing_non_ident(frontend: &Frontend, content: &str, byte_offset: usize) -> bool {
    if let Some(word) = extract_word_at(content, byte_offset)
        && word.chars().next().is_some_and(|c| c.is_numeric())
    {
        return true;
    }

    let tokens = frontend.tokenize_recovery(content);
    if let Some(tok) = tokens.iter().rev().find(|t| {
        t.byte_end <= byte_offset && !matches!(t.token_type, TokenType::Eof | TokenType::Newline)
    }) {
        match &tok.token_type {
            TokenType::Integer(_)
            | TokenType::BigInteger(_)
            | TokenType::Float(_)
            | TokenType::Imaginary(_)
            | TokenType::Character(_)
            | TokenType::String(_) => {
                let between = &content[tok.byte_end..byte_offset];
                if between.chars().all(char::is_whitespace) {
                    return true;
                }
            }
            _ => {}
        }
    }

    false
}

fn extract_word_at(src: &str, offset: usize) -> Option<String> {
    let (start, end) = word_range_at(src, offset)?;
    Some(src[start..end].to_string())
}

fn word_range_at(src: &str, offset: usize) -> Option<(usize, usize)> {
    let offset = offset.min(src.len());
    let at_ident = src[offset..].chars().next().is_some_and(is_word_char);
    let prev_ident = offset > 0 && src[..offset].chars().last().is_some_and(is_word_char);
    if !(at_ident || (offset == src.len() && prev_ident)) {
        return None;
    }

    let start = src[..offset]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, _)| i)
        .unwrap_or(offset);
    let end = src[offset..]
        .char_indices()
        .take_while(|(_, c)| is_word_char(*c))
        .last()
        .map(|(i, c)| offset + i + c.len_utf8())
        .unwrap_or(offset);
    let word = &src[start..end];
    if word.is_empty() {
        None
    } else {
        Some((start, end))
    }
}

fn is_word_char(c: char) -> bool {
    crate::identifier::is_identifier_continue(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_ranges_follow_unicode_identifier_rules() {
        let source = "λe\u{301}?";
        assert_eq!(word_range_at(source, source.len()), Some((0, source.len())));
    }

    #[test]
    fn expression_candidates_include_symbols_and_builtins() {
        let frontend = Frontend::default();
        let candidates = expression_completion_candidates(&frontend, "x:1; f:{[y] y+x}");

        assert!(
            candidates
                .iter()
                .any(|c| { c.label == "x" && c.kind == CompletionKind::Assignment })
        );
        assert!(
            candidates
                .iter()
                .any(|c| { c.label == "f" && c.kind == CompletionKind::Function })
        );
        assert!(candidates.iter().any(|c| {
            c.label == "sum" && c.kind == CompletionKind::Builtin && c.detail.is_some()
        }));
    }

    #[test]
    fn suppression_allows_format_string_exprs_only() {
        let frontend = Frontend::default();
        let text_src = "@f \"hello su\"";
        let text_pos = text_src.find("su").expect("text") + 2;
        let expr_src = "@f \"hello {su}\"";
        let expr_pos = expr_src.find("su").expect("expr") + 2;

        assert!(should_suppress_expression_completion(
            &frontend, text_src, text_pos
        ));
        assert!(!should_suppress_expression_completion(
            &frontend, expr_src, expr_pos
        ));
    }

    #[test]
    fn suppression_blocks_numeric_contexts() {
        let frontend = Frontend::default();

        assert!(should_suppress_expression_completion(&frontend, "123", 3));
        assert!(should_suppress_expression_completion(&frontend, "1 ", 2));
    }

    #[test]
    fn suppression_blocks_completion_inside_invalid_strings() {
        let frontend = Frontend::default();
        let src = r#""\u{}z""#;
        let pos = src.find('z').expect("invalid string contains z") + 1;

        assert!(should_suppress_expression_completion(&frontend, src, pos));
    }

    #[test]
    fn named_arg_context_tracks_nested_builtin_calls_and_used_names() {
        let frontend = Frontend::default();
        let src = "echo[split[\"a,b\";\",\";`ma];`sep:\",\"]";
        let pos = src.find("`ma").expect("partial named argument") + 3;
        let context = builtin_named_arg_completion_context(&frontend, src, pos)
            .expect("split named argument context");

        assert_eq!(context.builtin_name, "split");
        assert_eq!(context.prefix, "ma");
        assert_eq!(&src[context.replace_start..pos], "`ma");
        assert!(context.used_names.is_empty());

        let src = "split[\"a,b\";`max:1;`ma]";
        let pos = src.rfind("`ma").expect("second named argument") + 3;
        let context = builtin_named_arg_completion_context(&frontend, src, pos)
            .expect("split duplicate named argument context");
        assert_eq!(context.used_names, vec!["max"]);
    }

    #[test]
    fn named_arg_context_ignores_non_call_tags_and_strings() {
        let frontend = Frontend::default();
        assert!(builtin_named_arg_completion_context(&frontend, "(`ma:1)", 4).is_none());

        let src = "split[\"`ma\"]";
        let pos = src.find("ma").expect("string contents") + 2;
        assert!(builtin_named_arg_completion_context(&frontend, src, pos).is_none());
    }
}
