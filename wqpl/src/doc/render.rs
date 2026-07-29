use std::fmt::Write as _;

use unicode_width::{UnicodeWidthChar as _, UnicodeWidthStr as _};

use super::model::{DocKind, DocRenderTarget, DocTopic};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkdownRenderOptions {
    pub fold_width: Option<usize>,
}

pub fn render_markdown(topic: &DocTopic, target: DocRenderTarget) -> String {
    render_markdown_with_options(topic, target, MarkdownRenderOptions::default())
}

pub fn render_markdown_with_options(
    topic: &DocTopic,
    target: DocRenderTarget,
    options: MarkdownRenderOptions,
) -> String {
    let mut out = String::new();
    let heading = match target {
        DocRenderTarget::Cli | DocRenderTarget::Web => "#",
        DocRenderTarget::Lsp => "##",
    };
    let _ = writeln!(out, "{} {}", heading, topic.title);
    let _ = writeln!(out);
    let _ = writeln!(out, "_{} · {}_", kind_label(topic.kind), topic.group);
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", topic.summary);

    if let Some(builtin) = topic.builtin {
        let _ = writeln!(out);
        let _ = writeln!(out, "```wq");
        let _ = writeln!(out, "{}", builtin.usage());
        let _ = writeln!(out, "```");
        let _ = writeln!(out);
        let _ = writeln!(out, "arity: `{}`", builtin.arity());
        if let Some(named_args) = builtin.named_args()
            && !named_args.is_empty()
        {
            let _ = writeln!(out);
            let _ = writeln!(out, "named arguments:");
            for arg in named_args {
                let _ = writeln!(
                    out,
                    "- `` `{}:{} ``: {}",
                    arg.name, arg.value_label, arg.summary
                );
            }
        }
        if let Some(canonical) = topic.canonical_builtin
            && canonical != builtin
        {
            let _ = writeln!(out);
            let _ = writeln!(out, "Alias of `{}`.", canonical.name());
        }
    }

    if !topic.details.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "{}", topic.details);
    }

    if !topic.examples.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "## Examples");
        for example in &topic.examples {
            let _ = writeln!(out);
            if !example.title.is_empty() {
                let _ = writeln!(out, "{}", example.title);
                let _ = writeln!(out);
            }
            let _ = writeln!(out, "```wq");
            let _ = writeln!(out, "{}", example.code);
            let _ = writeln!(out, "```");
        }
    }

    if !topic.related.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(out, "Related: {}", topic.related.join(", "));
    }

    let markdown = out.trim_end().to_string();
    if target == DocRenderTarget::Cli
        && let Some(width) = options.fold_width
        && width > 0
    {
        fold_markdown(&markdown, width)
    } else {
        markdown
    }
}

fn kind_label(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Builtin => "builtin",
        DocKind::Keyword => "keyword",
        DocKind::Syntax => "syntax",
        DocKind::Guide => "guide",
    }
}

pub fn fold_markdown(markdown: &str, width: usize) -> String {
    let mut out = String::new();
    let mut paragraph = String::new();
    let mut in_fence = false;

    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            flush_paragraph(&mut out, &mut paragraph, width);
            push_line(&mut out, line);
            in_fence = !in_fence;
        } else if in_fence || line.trim().is_empty() {
            flush_paragraph(&mut out, &mut paragraph, width);
            push_line(&mut out, line);
        } else if let Some(item) = parse_list_item(line) {
            flush_paragraph(&mut out, &mut paragraph, width);
            fold_prefixed_text(
                &mut out,
                &item.prefix,
                &" ".repeat(item.prefix.width()),
                item.body,
                width,
            );
            if has_markdown_hard_break(line) {
                preserve_markdown_hard_break(&mut out);
            }
        } else if should_preserve_line(line) {
            flush_paragraph(&mut out, &mut paragraph, width);
            push_line(&mut out, line);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(line.trim());
            if has_markdown_hard_break(line) {
                flush_paragraph(&mut out, &mut paragraph, width);
                preserve_markdown_hard_break(&mut out);
            }
        }
    }
    flush_paragraph(&mut out, &mut paragraph, width);

    out.trim_end().to_string()
}

fn flush_paragraph(out: &mut String, paragraph: &mut String, width: usize) {
    if paragraph.is_empty() {
        return;
    }

    fold_prefixed_text(out, "", "", paragraph, width);
    paragraph.clear();
}

fn fold_prefixed_text(
    out: &mut String,
    first_prefix: &str,
    continuation_prefix: &str,
    text: &str,
    width: usize,
) {
    let mut line = first_prefix.to_string();
    let mut line_width = first_prefix.width();
    let mut has_content = false;
    let continuation_width = continuation_prefix.width();

    for token in fold_tokens(text) {
        let separator_width = usize::from(token.space_before && has_content);
        let token_width = token.visible_width();
        if has_content && token.space_before && line_width + separator_width + token_width > width {
            push_hard_break_line(out, &line);
            line.clear();
            line.push_str(continuation_prefix);
            line_width = continuation_width;
            has_content = false;
        }

        if let FoldTokenKind::InlineCode {
            markdown,
            delimiter,
            body,
        } = token.kind
            && token_width > width.saturating_sub(continuation_width)
        {
            let payload_width = width.saturating_sub(continuation_width + 2).max(1);
            let chunks = split_code_body(body, payload_width);
            if chunks.is_empty() {
                line.push_str(markdown);
                line_width += token_width;
                has_content = true;
                continue;
            }
            for (index, chunk) in chunks.iter().enumerate() {
                line.push_str(delimiter);
                line.push_str(chunk);
                line.push_str(delimiter);
                line_width += chunk.width() + 2;
                has_content = true;
                if index + 1 < chunks.len() {
                    push_hard_break_line(out, &line);
                    line.clear();
                    line.push_str(continuation_prefix);
                    line_width = continuation_width;
                    has_content = false;
                }
            }
            continue;
        }

        if token.space_before && has_content {
            line.push(' ');
            line_width += 1;
        }
        line.push_str(token.markdown());
        line_width += token_width;
        has_content = true;
    }

    if has_content || !first_prefix.is_empty() {
        push_line(out, &line);
    }
}

#[derive(Debug, Clone, Copy)]
struct FoldToken<'a> {
    kind: FoldTokenKind<'a>,
    space_before: bool,
}

impl<'a> FoldToken<'a> {
    fn markdown(self) -> &'a str {
        match self.kind {
            FoldTokenKind::Text(text) => text,
            FoldTokenKind::InlineCode { markdown, .. } => markdown,
        }
    }

    fn visible_width(self) -> usize {
        match self.kind {
            FoldTokenKind::Text(text) => text.width(),
            FoldTokenKind::InlineCode { body, .. } => body.width() + 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum FoldTokenKind<'a> {
    Text(&'a str),
    InlineCode {
        markdown: &'a str,
        delimiter: &'a str,
        body: &'a str,
    },
}

fn fold_tokens(text: &str) -> Vec<FoldToken<'_>> {
    let mut tokens = Vec::new();
    let mut position = 0;
    while position < text.len() {
        let mut space_before = false;
        while let Some(ch) = text[position..].chars().next()
            && ch.is_whitespace()
        {
            position += ch.len_utf8();
            space_before = true;
        }
        if position == text.len() {
            break;
        }

        if text.as_bytes()[position] == b'`' {
            let delimiter_len = backtick_run_len(text, position);
            let body_start = position + delimiter_len;
            if let Some(body_end) = find_closing_backticks(text, body_start, delimiter_len) {
                let markdown_end = body_end + delimiter_len;
                tokens.push(FoldToken {
                    kind: FoldTokenKind::InlineCode {
                        markdown: &text[position..markdown_end],
                        delimiter: &text[position..body_start],
                        body: &text[body_start..body_end],
                    },
                    space_before,
                });
                position = markdown_end;
                continue;
            }
        }

        let start = position;
        while let Some(ch) = text[position..].chars().next() {
            if ch.is_whitespace() || ch == '`' && position > start {
                break;
            }
            position += ch.len_utf8();
        }
        tokens.push(FoldToken {
            kind: FoldTokenKind::Text(&text[start..position]),
            space_before,
        });
    }
    tokens
}

fn backtick_run_len(text: &str, start: usize) -> usize {
    text.as_bytes()[start..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

fn find_closing_backticks(text: &str, mut position: usize, delimiter_len: usize) -> Option<usize> {
    while position < text.len() {
        if text.as_bytes()[position] != b'`' {
            position += text[position..]
                .chars()
                .next()
                .expect("position is before the end of text")
                .len_utf8();
            continue;
        }
        let run_len = backtick_run_len(text, position);
        if run_len == delimiter_len {
            return Some(position);
        }
        position += run_len;
    }
    None
}

fn split_code_body(body: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    let mut chunk_width = 0;

    for word in body.split_whitespace() {
        let word_width = word.width();
        if word_width > width {
            if !chunk.is_empty() {
                chunks.push(std::mem::take(&mut chunk));
                chunk_width = 0;
            }
            let mut word_chunk = String::new();
            let mut word_chunk_width = 0;
            for ch in word.chars() {
                let char_width = ch.width().unwrap_or(0);
                if !word_chunk.is_empty() && word_chunk_width + char_width > width {
                    chunks.push(std::mem::take(&mut word_chunk));
                    word_chunk_width = 0;
                }
                word_chunk.push(ch);
                word_chunk_width += char_width;
            }
            if !word_chunk.is_empty() {
                chunk_width = word_chunk_width;
                chunk = word_chunk;
            }
        } else if chunk.is_empty() {
            chunk.push_str(word);
            chunk_width = word_width;
        } else if chunk_width + 1 + word_width <= width {
            chunk.push(' ');
            chunk.push_str(word);
            chunk_width += 1 + word_width;
        } else {
            chunks.push(std::mem::take(&mut chunk));
            chunk.push_str(word);
            chunk_width = word_width;
        }
    }
    if !chunk.is_empty() {
        chunks.push(chunk);
    }
    chunks
}

struct ListItem<'a> {
    prefix: String,
    body: &'a str,
}

fn parse_list_item(line: &str) -> Option<ListItem<'_>> {
    let indent_len = line.bytes().take_while(|byte| *byte == b' ').count();
    if indent_len > 3 || line.as_bytes().get(indent_len) == Some(&b'\t') {
        return None;
    }

    let rest = &line[indent_len..];
    if rest.starts_with("- ") || rest.starts_with("* ") {
        let body_start = indent_len + 2;
        return Some(ListItem {
            prefix: line[..body_start].to_string(),
            body: line[body_start..].trim_start(),
        });
    }

    let marker_end = rest
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .filter(|idx| *idx > 0)?;
    let marker = rest.as_bytes().get(marker_end)?;
    if !matches!(marker, b'.' | b')') || rest.as_bytes().get(marker_end + 1) != Some(&b' ') {
        return None;
    }

    let body_start = indent_len + marker_end + 2;
    Some(ListItem {
        prefix: line[..body_start].to_string(),
        body: line[body_start..].trim_start(),
    })
}

fn should_preserve_line(line: &str) -> bool {
    if line.starts_with([' ', '\t']) {
        return true;
    }

    let trimmed = line.trim_start();
    trimmed.starts_with('#')
        || trimmed.starts_with('|')
        || trimmed.starts_with('+')
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
}

fn has_markdown_hard_break(line: &str) -> bool {
    line.ends_with("  ")
}

fn preserve_markdown_hard_break(out: &mut String) {
    if out.ends_with('\n') {
        out.truncate(out.len() - 1);
        out.push_str("  \n");
    }
}

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn push_hard_break_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push_str("  \n");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::Builtins;
    use crate::doc::{DocKind, DocRenderTarget, DocTopic};

    #[test]
    fn folds_plain_markdown_paragraphs() {
        let markdown = "# Topic\n\nalpha beta gamma delta epsilon";
        let folded = fold_markdown(markdown, 18);

        assert_eq!(folded, "# Topic\n\nalpha beta gamma  \ndelta epsilon");
    }

    #[test]
    fn folding_preserves_code_fences_and_blocks() {
        let markdown = "before the code fence wraps\n\n```wq\n- this line is intentionally long and must stay whole\n```\n\n    indented block stays whole";
        let folded = fold_markdown(markdown, 12);

        assert!(folded.contains("before the  \ncode fence  \nwraps"));
        assert!(folded.contains("- this line is intentionally long and must stay whole"));
        assert!(folded.contains("    indented block stays whole"));
    }

    #[test]
    fn folds_markdown_list_items() {
        let markdown = "- alpha beta gamma delta epsilon\n- short";
        let folded = fold_markdown(markdown, 16);

        assert_eq!(
            folded,
            "- alpha beta  \n  gamma delta  \n  epsilon\n- short"
        );
    }

    #[test]
    fn folds_ordered_and_nested_list_items() {
        let markdown = "12. alpha beta gamma delta\n  - alpha beta gamma";
        let folded = fold_markdown(markdown, 16);

        assert_eq!(
            folded,
            "12. alpha beta  \n    gamma delta\n  - alpha beta  \n    gamma"
        );
    }

    #[test]
    fn inline_code_prefers_starting_on_a_fresh_line() {
        let folded = fold_markdown("alpha ``beta gamma``", 12);

        assert_eq!(folded, "alpha  \n``beta gamma``");
    }

    #[test]
    fn folding_keeps_attached_punctuation_with_inline_code() {
        let folded = fold_markdown("alpha beta `gamma`.", 17);

        assert_eq!(folded, "alpha beta  \n`gamma`.");
    }

    #[test]
    fn inline_code_splits_when_it_cannot_fit_a_full_line() {
        let folded = fold_markdown("x ``alpha beta gamma`` y", 10);

        assert_eq!(folded, "x  \n``alpha``  \n``beta``  \n``gamma`` y");

        let folded = fold_markdown("x ``abcdefghijkl``", 8);
        assert_eq!(folded, "x  \n``abcdef``  \n``ghijkl``");
    }

    #[test]
    fn fold_width_uses_terminal_columns() {
        assert_eq!(fold_markdown("界 a", 3), "界  \na");
        assert_eq!(fold_markdown("e\u{301} x", 3), "e\u{301} x");
        assert_eq!(fold_markdown("x `界界界`", 6), "x  \n`界界`  \n`界`");
    }

    #[test]
    fn folding_preserves_source_soft_and_hard_break_semantics() {
        assert_eq!(fold_markdown("alpha\nbeta", 80), "alpha beta");
        assert_eq!(fold_markdown("alpha  \nbeta", 80), "alpha  \nbeta");
    }

    #[test]
    fn fold_option_only_applies_to_cli_target() {
        let topic = DocTopic {
            id: "sample".to_string(),
            title: "Sample".to_string(),
            kind: DocKind::Guide,
            group: "misc".to_string(),
            aliases: Vec::new(),
            summary: "alpha beta gamma delta".to_string(),
            details: String::new(),
            examples: Vec::new(),
            related: Vec::new(),
            builtin: None,
            canonical_builtin: None,
        };
        let options = MarkdownRenderOptions {
            fold_width: Some(12),
        };

        let cli = render_markdown_with_options(&topic, DocRenderTarget::Cli, options);
        let web = render_markdown_with_options(&topic, DocRenderTarget::Web, options);

        assert!(cli.contains("alpha beta  \ngamma delta"));
        assert!(web.contains("alpha beta gamma delta"));
    }

    #[test]
    fn builtin_docs_render_named_arguments_from_registry_metadata() {
        let builtins = Builtins::new();
        let topic = builtins.doc_for_name("split").expect("split docs");

        let markdown = render_markdown(&topic, DocRenderTarget::Web);

        assert!(markdown.contains("named arguments:"));
        assert!(markdown.contains("- `` `max:n ``: maximum number of splits"));
        assert!(!markdown.contains("opts?"));
    }
}
