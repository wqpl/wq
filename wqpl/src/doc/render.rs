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
        } else if in_fence || should_preserve_line(line) || line.trim().is_empty() {
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

    let mut line = String::new();
    for word in paragraph.split_whitespace() {
        let word_width = word.chars().count();
        let line_width = line.chars().count();
        let separator_width = usize::from(!line.is_empty());
        if !line.is_empty() && line_width + separator_width + word_width > width {
            push_line(out, &line);
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        push_line(out, &line);
    }
    paragraph.clear();
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
        let markdown = "before the code fence wraps\n\n```wq\nthis line is intentionally long and must stay whole\n```\n\n    indented block stays whole";
        let folded = fold_markdown(markdown, 12);

        assert!(folded.contains("before the\ncode fence\nwraps"));
        assert!(folded.contains("this line is intentionally long and must stay whole"));
        assert!(folded.contains("    indented block stays whole"));
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
