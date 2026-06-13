use std::fmt::Write as _;

use super::model::{DocKind, DocRenderTarget, DocTopic};

pub fn render_markdown(topic: &DocTopic, target: DocRenderTarget) -> String {
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

    out.trim_end().to_string()
}

fn kind_label(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Builtin => "builtin",
        DocKind::Keyword => "keyword",
        DocKind::Syntax => "syntax",
        DocKind::Guide => "guide",
    }
}
