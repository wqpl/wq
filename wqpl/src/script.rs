use std::ops::Range;

use crate::escape::unescape_string_inner;
use crate::lex::Lexer;
use crate::parse::Parser;
use crate::wqerror::WqErrorType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptSpan {
    pub start: usize,
    pub end: usize,
}

impl ScriptSpan {
    pub fn as_range(self) -> Range<usize> {
        self.start..self.end
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptDirective {
    PreludeAlias { span: ScriptSpan },
    LoadEmbeddedOrFile { name: String, span: ScriptSpan },
    LoadPath { path: String, span: ScriptSpan },
    Unknown { text: String, span: ScriptSpan },
}

impl ScriptDirective {
    pub fn span(&self) -> ScriptSpan {
        match self {
            ScriptDirective::PreludeAlias { span }
            | ScriptDirective::LoadEmbeddedOrFile { span, .. }
            | ScriptDirective::LoadPath { span, .. }
            | ScriptDirective::Unknown { span, .. } => *span,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptItem {
    Shebang { span: ScriptSpan },
    Directive(ScriptDirective),
    Code { span: ScriptSpan },
}

impl ScriptItem {
    pub fn span(&self) -> ScriptSpan {
        match self {
            ScriptItem::Shebang { span } | ScriptItem::Code { span } => *span,
            ScriptItem::Directive(directive) => directive.span(),
        }
    }
}

#[derive(Default)]
struct PendingCode {
    start: Option<usize>,
    end: usize,
    has_payload: bool,
}

impl PendingCode {
    fn clear(&mut self) {
        self.start = None;
        self.end = 0;
        self.has_payload = false;
    }

    fn push_line(&mut self, start: usize, end: usize, has_payload: bool) {
        if self.start.is_none() {
            self.start = Some(start);
        }
        self.end = end;
        self.has_payload |= has_payload;
    }

    fn span(&self) -> Option<ScriptSpan> {
        self.start.map(|start| ScriptSpan {
            start,
            end: self.end,
        })
    }
}

pub fn parse_script_items(source: &str) -> Vec<ScriptItem> {
    let mut items = Vec::new();
    let mut pending = PendingCode::default();
    let mut line_index = 0usize;
    let mut line_start = 0usize;

    for (idx, ch) in source.char_indices() {
        if ch == '\n' {
            process_line(
                source,
                line_index,
                line_start,
                idx,
                idx + ch.len_utf8(),
                &mut pending,
                &mut items,
            );
            line_index += 1;
            line_start = idx + ch.len_utf8();
        }
    }

    if line_start < source.len() {
        process_line(
            source,
            line_index,
            line_start,
            source.len(),
            source.len(),
            &mut pending,
            &mut items,
        );
    }

    finish_pending(source, &mut pending, &mut items);
    items
}

pub fn might_have_script_meta(source: &str) -> bool {
    source.contains('\\') || source.starts_with("#!")
}

fn process_line(
    source: &str,
    line_index: usize,
    line_start: usize,
    content_end: usize,
    line_end: usize,
    pending: &mut PendingCode,
    items: &mut Vec<ScriptItem>,
) {
    let raw_line = &source[line_start..content_end];
    let span = ScriptSpan {
        start: line_start,
        end: line_end,
    };

    if line_index == 0 && raw_line.starts_with("#!") && !pending.has_payload {
        pending.clear();
        items.push(ScriptItem::Shebang { span });
        return;
    }

    let trimmed_leading = raw_line.trim_start();
    if !pending.has_payload && trimmed_leading.starts_with('\\') {
        pending.clear();
        items.push(ScriptItem::Directive(parse_legacy_directive(
            raw_line, span,
        )));
        return;
    }

    let trimmed_all = raw_line.trim();
    let has_payload = !trimmed_all.is_empty() && !trimmed_all.starts_with("//");
    pending.push_line(line_start, line_end, has_payload);

    if pending.has_payload
        && let Some(code_span) = pending.span()
        && is_complete_code(&source[code_span.as_range()])
    {
        items.push(ScriptItem::Code { span: code_span });
        pending.clear();
    }
}

fn finish_pending(source: &str, pending: &mut PendingCode, items: &mut Vec<ScriptItem>) {
    let Some(span) = pending.span() else {
        return;
    };
    if source[span.as_range()].trim().is_empty() {
        return;
    }
    items.push(ScriptItem::Code { span });
    pending.clear();
}

pub fn parse_legacy_directive(line: &str, span: ScriptSpan) -> ScriptDirective {
    let trimmed = line.trim();
    if trimmed == r"\p" {
        return ScriptDirective::PreludeAlias { span };
    }
    if let Some(rest) = [r"\load", r"\l"]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))
    {
        let Some(arg) = parse_load_arg(rest) else {
            return ScriptDirective::Unknown {
                text: line.trim_start().to_string(),
                span,
            };
        };
        if arg.starts_with('<') && arg.ends_with('>') && arg.len() >= 2 {
            return ScriptDirective::LoadEmbeddedOrFile {
                name: arg[1..arg.len() - 1].to_string(),
                span,
            };
        }
        if !arg.is_empty() {
            return ScriptDirective::LoadPath {
                path: arg.to_string(),
                span,
            };
        }
    }
    ScriptDirective::Unknown {
        text: line.trim_start().to_string(),
        span,
    }
}

fn parse_load_arg(rest: &str) -> Option<String> {
    let arg = rest.trim();
    if arg.is_empty() {
        return None;
    }
    let mut chars = arg.chars();
    let Some(quote @ ('"' | '\'')) = chars.next() else {
        return Some(arg.to_string());
    };

    let body_start = quote.len_utf8();
    let mut escaped = false;
    for (idx, ch) in arg[body_start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            let body_end = body_start + idx;
            let rest = &arg[body_end + quote.len_utf8()..];
            if !rest.trim().is_empty() {
                return None;
            }
            return unescape_string_inner(&arg[body_start..body_end]).ok();
        }
    }
    None
}

fn is_complete_code(input: &str) -> bool {
    let mut lexer = Lexer::new(input);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => tokens,
        Err(err) => return err.err_type != WqErrorType::Eof,
    };
    let mut parser = Parser::new(tokens, input.to_string());
    match parser.parse() {
        Ok(_) => parser.eof_error().is_none(),
        Err(err) => err.err_type != WqErrorType::Eof,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_text<'a>(source: &'a str, item: &ScriptItem) -> &'a str {
        &source[item.span().as_range()]
    }

    #[test]
    fn splits_code_and_load_directives() {
        let source = "a:1\n\\l lib.wq\nb\n";
        let items = parse_script_items(source);

        assert_eq!(items.len(), 3);
        assert_eq!(item_text(source, &items[0]), "a:1\n");
        assert!(matches!(
            &items[1],
            ScriptItem::Directive(ScriptDirective::LoadPath { path, .. }) if path == "lib.wq"
        ));
        assert_eq!(item_text(source, &items[2]), "b\n");
    }

    #[test]
    fn parses_prelude_alias_and_embedded_load() {
        let source = "\\p\n\\load <prelude>\n";
        let items = parse_script_items(source);

        assert!(matches!(
            items.first(),
            Some(ScriptItem::Directive(ScriptDirective::PreludeAlias { .. }))
        ));
        assert!(matches!(
            items.get(1),
            Some(ScriptItem::Directive(ScriptDirective::LoadEmbeddedOrFile { name, .. }))
                if name == "prelude"
        ));
    }

    #[test]
    fn parses_quoted_load_args() {
        let source = "\\load \"dir/lib file.wq\"\n\\load '<prelude>'\n";
        let items = parse_script_items(source);

        assert!(matches!(
            items.first(),
            Some(ScriptItem::Directive(ScriptDirective::LoadPath { path, .. }))
                if path == "dir/lib file.wq"
        ));
        assert!(matches!(
            items.get(1),
            Some(ScriptItem::Directive(ScriptDirective::LoadEmbeddedOrFile { name, .. }))
                if name == "prelude"
        ));
    }

    #[test]
    fn directive_inside_incomplete_code_stays_code() {
        let source = "$[true;\n\\l nope.wq\n;1]\n";
        let items = parse_script_items(source);

        assert!(
            items
                .iter()
                .all(|item| matches!(item, ScriptItem::Code { .. })),
            "inner directive line should not become a directive: {items:#?}",
        );
        assert!(item_text(source, &items[0]).contains("\\l nope.wq"));
    }

    #[test]
    fn shebang_is_a_script_item() {
        let source = "#!/usr/bin/env wq\n1+1\n";
        let items = parse_script_items(source);

        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], ScriptItem::Shebang { .. }));
        assert_eq!(item_text(source, &items[1]), "1+1\n");
    }
}
