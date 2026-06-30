use std::collections::HashMap;

use crate::builtins::Builtins;
use crate::doc::DocTopic;
use crate::session::Session;
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
}

impl CompletionCandidate {
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
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
}

pub fn expression_completion_candidates(
    session: &Session,
    content: &str,
) -> Vec<CompletionCandidate> {
    let mut seen = HashMap::new();

    if let Ok(index) = session.analyze_symbols(content) {
        for def in &index.defs {
            if def.kind == DefKind::Builtin {
                continue;
            }
            let candidate =
                CompletionCandidate::new(def.name.clone(), CompletionKind::from_def_kind(def.kind));
            seen.entry(def.name.clone()).or_insert(candidate);
        }
    } else {
        for name in fallback_assignment_names(session, content) {
            seen.entry(name.clone())
                .or_insert_with(|| CompletionCandidate::new(name, CompletionKind::Assignment));
        }
    }

    for candidate in builtin_completion_candidates(session.builtins(), true) {
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
        }
        if let Some(topic) = builtins.doc_for_name(&name) {
            candidate = candidate.with_documentation(topic);
        }
        candidates.push(candidate);
    }
    candidates.sort_by(|a, b| a.label.cmp(&b.label));
    candidates
}

pub fn should_suppress_expression_completion(
    session: &Session,
    content: &str,
    byte_offset: usize,
) -> bool {
    is_in_no_completion_zone(session, content, byte_offset)
        || is_typing_non_ident(session, content, byte_offset)
}

fn fallback_assignment_names(session: &Session, content: &str) -> Vec<String> {
    let tokens = session.tokenize_recovery(content);
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

fn is_in_no_completion_zone(session: &Session, content: &str, byte_offset: usize) -> bool {
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

    let tokens = session.tokenize_recovery(content);
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
        _ => false,
    }
}

fn is_typing_non_ident(session: &Session, content: &str, byte_offset: usize) -> bool {
    if let Some(word) = extract_word_at(content, byte_offset)
        && word.chars().next().is_some_and(|c| c.is_numeric())
    {
        return true;
    }

    let tokens = session.tokenize_recovery(content);
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
    c.is_alphanumeric() || c == '_' || c == '?'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expression_candidates_include_symbols_and_builtins() {
        let session = Session::new();
        let candidates = expression_completion_candidates(&session, "x:1; f:{[y] y+x}");

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
        let session = Session::new();
        let text_src = "@f \"hello su\"";
        let text_pos = text_src.find("su").expect("text") + 2;
        let expr_src = "@f \"hello {su}\"";
        let expr_pos = expr_src.find("su").expect("expr") + 2;

        assert!(should_suppress_expression_completion(
            &session, text_src, text_pos
        ));
        assert!(!should_suppress_expression_completion(
            &session, expr_src, expr_pos
        ));
    }

    #[test]
    fn suppression_blocks_numeric_contexts() {
        let session = Session::new();

        assert!(should_suppress_expression_completion(&session, "123", 3));
        assert!(should_suppress_expression_completion(&session, "1 ", 2));
    }
}
