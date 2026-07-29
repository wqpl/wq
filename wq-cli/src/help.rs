use wqpl::doc::{self, DocRenderTarget};

use crate::repl::editor::WqReplHighlighter;
use crate::{arg, display, note};

const AUTO_FOLD_WIDTH_GUTTER: usize = 4;

pub fn run(
    topic: Option<String>,
    no_pager: bool,
    prefer_doc_topic: bool,
    fold_width: Option<usize>,
) {
    if !prefer_doc_topic && let Some(text) = arg::render_cli_help(topic.as_deref()) {
        print!("{text}");
        return;
    }

    let Some(topic_name) = topic.as_deref() else {
        return;
    };
    let Some(topic) = doc::resolve(topic_name) else {
        eprintln!("unknown help topic '{topic_name}'");
        eprintln!("try `wq help builtins`, `wq help map`, or `wq help @r`");
        std::process::exit(2);
    };

    let highlighter = WqReplHighlighter::new();
    let rendered = render_reference_topic(
        &topic,
        resolve_fold_width(fold_width, display::terminal_width()),
        &highlighter,
    );
    note::print_or_page(&rendered, no_pager);
}

pub(crate) fn render_reference_topic(
    topic: &doc::DocTopic,
    fold_width: Option<usize>,
    highlighter: &WqReplHighlighter,
) -> String {
    let markdown = doc::render_markdown_with_options(
        topic,
        DocRenderTarget::Cli,
        doc::MarkdownRenderOptions { fold_width },
    );
    note::render_markdown_document(&markdown, Some(highlighter))
}

fn resolve_fold_width(explicit: Option<usize>, detected: Option<usize>) -> Option<usize> {
    explicit.or_else(|| auto_fold_width(detected))
}

pub(crate) fn auto_fold_width(detected: Option<usize>) -> Option<usize> {
    detected
        .filter(|width| *width > AUTO_FOLD_WIDTH_GUTTER)
        .map(|width| width - AUTO_FOLD_WIDTH_GUTTER)
}

#[cfg(test)]
mod tests {
    use super::{auto_fold_width, resolve_fold_width};

    #[test]
    fn explicit_fold_width_wins() {
        assert_eq!(resolve_fold_width(Some(60), Some(80)), Some(60));
    }

    #[test]
    fn detected_fold_width_keeps_a_gutter() {
        assert_eq!(resolve_fold_width(None, Some(80)), Some(76));
        assert_eq!(auto_fold_width(Some(5)), Some(1));
        assert_eq!(auto_fold_width(Some(4)), None);
        assert_eq!(resolve_fold_width(None, Some(0)), None);
        assert_eq!(resolve_fold_width(None, None), None);
    }
}
