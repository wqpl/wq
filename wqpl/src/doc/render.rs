use std::fmt::Write as _;

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

fn fold_markdown(markdown: &str, width: usize) -> String {
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
                &" ".repeat(item.prefix.chars().count()),
                item.body,
                width,
            );
        } else if should_preserve_line(line) {
            flush_paragraph(&mut out, &mut paragraph, width);
            push_line(&mut out, line);
        } else {
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(line.trim());
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
    let mut line_width = first_prefix.chars().count();
    let mut has_word = false;

    for word in text.split_whitespace() {
        let word_width = word.chars().count();
        let separator_width = usize::from(has_word);
        if has_word && line_width + separator_width + word_width > width {
            push_line(out, &line);
            line.clear();
            line.push_str(continuation_prefix);
            line_width = continuation_prefix.chars().count();
            has_word = false;
        }
        if has_word {
            line.push(' ');
            line_width += 1;
        }
        line.push_str(word);
        line_width += word_width;
        has_word = true;
    }

    if has_word || !first_prefix.is_empty() {
        push_line(out, &line);
    }
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

fn push_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doc::{DocKind, DocRenderTarget, DocTopic};

    #[test]
    fn folds_plain_markdown_paragraphs() {
        let markdown = "# Topic\n\nalpha beta gamma delta epsilon";
        let folded = fold_markdown(markdown, 18);

        assert_eq!(folded, "# Topic\n\nalpha beta gamma\ndelta epsilon");
    }

    #[test]
    fn folding_preserves_code_fences_and_blocks() {
        let markdown = "before the code fence wraps\n\n```wq\n- this line is intentionally long and must stay whole\n```\n\n    indented block stays whole";
        let folded = fold_markdown(markdown, 12);

        assert!(folded.contains("before the\ncode fence\nwraps"));
        assert!(folded.contains("- this line is intentionally long and must stay whole"));
        assert!(folded.contains("    indented block stays whole"));
    }

    #[test]
    fn folds_markdown_list_items() {
        let markdown = "- alpha beta gamma delta epsilon\n- short";
        let folded = fold_markdown(markdown, 16);

        assert_eq!(
            folded,
            "- alpha beta\n  gamma delta\n  epsilon\n- short"
        );
    }

    #[test]
    fn folds_ordered_and_nested_list_items() {
        let markdown = "12. alpha beta gamma delta\n  - alpha beta gamma";
        let folded = fold_markdown(markdown, 16);

        assert_eq!(
            folded,
            "12. alpha beta\n    gamma delta\n  - alpha beta\n    gamma"
        );
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

        assert!(cli.contains("alpha beta\ngamma delta"));
        assert!(web.contains("alpha beta gamma delta"));
    }
}
