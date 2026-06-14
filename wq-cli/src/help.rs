use std::io::{IsTerminal as _, Write as _};
use std::process::{Command, Stdio};

use terminal_size::{Width, terminal_size};
use wqpl::doc::{self, DocRenderTarget};

use crate::repl::editor::WqReplHighlighter;
use crate::{arg, note};

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

    let markdown = doc::render_markdown_with_options(
        &topic,
        DocRenderTarget::Cli,
        doc::MarkdownRenderOptions {
            fold_width: resolve_fold_width(fold_width, detected_terminal_width()),
        },
    );
    let highlighter = WqReplHighlighter::new();
    let rendered = note::render_markdown_document(&markdown, Some(&highlighter));
    print_or_page(&rendered, no_pager);
}

fn resolve_fold_width(explicit: Option<usize>, detected: Option<usize>) -> Option<usize> {
    explicit.or_else(|| detected.filter(|width| *width > 0))
}

fn detected_terminal_width() -> Option<usize> {
    terminal_size().map(|(Width(width), _)| width as usize)
}

pub(crate) fn print_or_page(text: &str, no_pager: bool) {
    if no_pager || !std::io::stdout().is_terminal() {
        println!("{text}");
        return;
    }

    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less -R".to_string());
    let mut parts = pager.split_whitespace();
    let Some(program) = parts.next() else {
        println!("{text}");
        return;
    };
    let args: Vec<&str> = parts.collect();

    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        println!("{text}");
        return;
    };

    if let Some(stdin) = child.stdin.as_mut()
        && stdin.write_all(text.as_bytes()).is_err()
    {
        println!("{text}");
        return;
    }

    if child.wait().is_err() {
        println!("{text}");
    }
}

#[cfg(test)]
mod tests {
    use super::resolve_fold_width;

    #[test]
    fn explicit_fold_width_wins() {
        assert_eq!(resolve_fold_width(Some(60), Some(80)), Some(60));
    }

    #[test]
    fn detected_fold_width_is_used_when_explicit_is_absent() {
        assert_eq!(resolve_fold_width(None, Some(80)), Some(80));
        assert_eq!(resolve_fold_width(None, Some(0)), None);
        assert_eq!(resolve_fold_width(None, None), None);
    }
}
