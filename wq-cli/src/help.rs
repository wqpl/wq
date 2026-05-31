use std::io::{IsTerminal as _, Write as _};
use std::process::{Command, Stdio};

use wqpl::doc::{self, DocRenderTarget};

use crate::repl::editor::WqReplHighlighter;
use crate::{arg, note};

pub fn run(topic: Option<String>, no_pager: bool) {
    if let Some(text) = arg::render_cli_help(topic.as_deref()) {
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

    let markdown = doc::render_markdown(&topic, DocRenderTarget::Cli);
    let highlighter = WqReplHighlighter::new();
    let rendered = note::render_markdown_document(&markdown, Some(&highlighter));
    print_or_page(&rendered, no_pager);
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
