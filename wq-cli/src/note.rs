use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use colored::Colorize as _;
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use terminal_size::{Width, terminal_size};
use unicode_width::UnicodeWidthStr as _;
use wqpl::format::{FormatConfig, Formatter};
use wqpl::session::Session;
use wqpl::session::dbglog::set_debug_log_flags;
use wqpl::session::stdio::{WqStdinError, set_wqstdin, wqstdin_add_history, wqstdin_readline};

use crate::arg::RuntimeFlags;
use crate::load::eval_inline_with_load;
use crate::msg::{MsgType, print_load_error, system_msg_err, system_msg_out};
use crate::repl::editor::WqReplHighlighter;
use crate::repl::input::RustylineInput;
use crate::{apply_builtins_flag, apply_interpreter_flag, wqdb_pause_handler};

#[derive(Debug, Clone)]
pub enum Segment {
    Markdown(String),
    Heading { level: u8, text: String },
    CodeFence { lang: String, code: String },
}

#[derive(Debug, Clone, Default)]
pub struct NotebookConfig {
    pub builtins: Option<String>,
    pub interpreter: Option<String>,
    pub wqdb: Option<bool>,
    pub wqdb_cmds: Option<Vec<String>>,
    pub dry: Option<bool>,
    pub no_bt: Option<bool>,
    pub print: Option<bool>,
    pub stack_size: Option<usize>,
    pub debug: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Notebook {
    pub config: NotebookConfig,
    pub segments: Vec<Segment>,
}

pub fn parse_notebook(content: &str) -> Result<Notebook, String> {
    let (frontmatter, body) = split_frontmatter(content);

    let config = if let Some(frontmatter) = frontmatter {
        parse_frontmatter(frontmatter)?
    } else {
        NotebookConfig::default()
    };

    let segments = split_markdown_blocks(parse_segments(body));

    Ok(Notebook { config, segments })
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let mut rest = content;

    // Skip optional shebang line
    if rest.starts_with("#!") {
        if let Some(idx) = rest.find('\n') {
            rest = &rest[idx + 1..];
        } else {
            return (None, "");
        }
    }

    let trimmed = rest.trim_start();
    if !trimmed.starts_with("---") {
        return (None, rest);
    }

    let after_open = &trimmed[3..];
    let mut pos = 0;
    while pos < after_open.len() {
        let slice = &after_open[pos..];
        let nl_idx = slice.find('\n').unwrap_or(slice.len());
        let line = slice[..nl_idx].trim_end();
        if line == "---" {
            let frontmatter = after_open[..pos].trim();
            let body = if nl_idx < slice.len() {
                &slice[nl_idx + 1..]
            } else {
                ""
            };
            return (Some(frontmatter), body);
        }
        pos += nl_idx + 1;
    }

    (None, rest)
}

fn parse_frontmatter(frontmatter: &str) -> Result<NotebookConfig, String> {
    let table: toml::Table = frontmatter
        .parse()
        .map_err(|e| format!("TOML parse error: {e}"))?;
    let mut config = NotebookConfig::default();

    if let Some(v) = table.get("builtins").and_then(|v| v.as_str()) {
        config.builtins = Some(v.to_string());
    }
    if let Some(v) = table.get("interpreter").and_then(|v| v.as_str()) {
        config.interpreter = Some(v.to_string());
    }
    if let Some(v) = table.get("wqdb").and_then(|v| v.as_bool()) {
        config.wqdb = Some(v);
    }
    if let Some(v) = table.get("wqdb_cmds").and_then(|v| v.as_array()) {
        let mut cmds = Vec::new();
        for item in v {
            let s = item
                .as_str()
                .ok_or("wqdb_cmds must be an array of strings")?;
            cmds.push(s.to_string());
        }
        config.wqdb_cmds = Some(cmds);
    }
    if let Some(v) = table.get("dry").and_then(|v| v.as_bool()) {
        config.dry = Some(v);
    }
    if let Some(v) = table.get("no_bt").and_then(|v| v.as_bool()) {
        config.no_bt = Some(v);
    }
    if let Some(v) = table.get("print").and_then(|v| v.as_bool()) {
        config.print = Some(v);
    }
    if let Some(v) = table.get("stack_size").and_then(|v| v.as_integer()) {
        config.stack_size = Some(v as usize);
    }
    if let Some(v) = table.get("debug").and_then(|v| v.as_str()) {
        config.debug = Some(v.to_string());
    }

    Ok(config)
}

fn split_markdown_blocks(segments: Vec<Segment>) -> Vec<Segment> {
    let mut result = Vec::new();
    for seg in segments {
        match seg {
            Segment::Markdown(text) => {
                let normalized = text.replace("\r\n", "\n");
                for block in normalized.split("\n\n") {
                    let trimmed = block.trim();
                    if !trimmed.is_empty() {
                        result.push(Segment::Markdown(trimmed.to_string()));
                    }
                }
            }
            other => result.push(other),
        }
    }
    result
}

fn parse_segments(markdown: &str) -> Vec<Segment> {
    let parser = Parser::new(markdown);
    let mut segments = Vec::new();
    let mut pos = 0usize;

    let mut heading_level = 0u8;
    let mut heading_text: Option<String> = None;

    let mut code_lang: Option<String> = None;
    let mut code_text = String::new();

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                if pos < range.start {
                    let text = markdown[pos..range.start].trim();
                    if !text.is_empty() {
                        segments.push(Segment::Markdown(text.to_string()));
                    }
                }
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(l) => l.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
                code_lang = Some(lang);
                code_text.clear();
                pos = range.end;
            }
            Event::Text(text) => {
                if code_lang.is_some() {
                    code_text.push_str(&text);
                } else if heading_level > 0 {
                    heading_text.get_or_insert_with(String::new).push_str(&text);
                }
            }
            Event::Code(code) if heading_level > 0 => {
                heading_text.get_or_insert_with(String::new).push_str(&code);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(lang) = code_lang.take() {
                    segments.push(Segment::CodeFence {
                        lang,
                        code: code_text.trim_end().to_string(),
                    });
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                if pos < range.start {
                    let text = markdown[pos..range.start].trim();
                    if !text.is_empty() {
                        segments.push(Segment::Markdown(text.to_string()));
                    }
                }
                heading_level = level as u8;
                heading_text = None;
                pos = range.end;
            }
            Event::End(TagEnd::Heading(_)) if heading_level > 0 => {
                let text = heading_text.take().unwrap_or_default().trim().to_string();
                segments.push(Segment::Heading {
                    level: heading_level,
                    text,
                });
                heading_level = 0;
            }
            _ => {}
        }
    }

    if pos < markdown.len() {
        let text = markdown[pos..].trim();
        if !text.is_empty() {
            segments.push(Segment::Markdown(text.to_string()));
        }
    }

    segments
}

pub(crate) fn format_heading(level: u8, text: &str) -> String {
    let prefix = "#".repeat(level as usize);
    let colored = match level {
        1 => prefix.magenta().bold(),
        2 => prefix.blue().bold(),
        3 => prefix.cyan().bold(),
        _ => prefix.dimmed().bold(),
    };
    format!("{} {}", colored, text.bold())
}

pub(crate) fn format_code_fence(
    lang: &str,
    code: &str,
    highlighter: Option<&WqReplHighlighter>,
) -> String {
    format_code_fence_with_width(lang, code, highlighter, None)
}

const DEFAULT_CODE_FENCE_WRAP_WIDTH: usize = 100;
const CODE_FENCE_PREFIX_WIDTH: usize = 2;
const MIN_DETECTED_CODE_FENCE_WRAP_WIDTH: usize = 20;

fn format_code_fence_with_width(
    lang: &str,
    code: &str,
    highlighter: Option<&WqReplHighlighter>,
    wrap_width: Option<usize>,
) -> String {
    let mut out = String::new();
    let is_wq = is_wq_fence_lang(lang);
    let code_width = code_fence_wrap_width(wrap_width);
    let display_source = if is_wq {
        smart_wrap_wq_code(code, code_width).unwrap_or_else(|| code.to_string())
    } else {
        code.to_string()
    };

    let max_line_len = display_source
        .lines()
        .map(terminal_width)
        .max()
        .unwrap_or(0);
    let lang_width = terminal_width(lang);
    let top_dashes = (max_line_len.saturating_sub(lang_width + 1)).max(40);
    let bottom_dashes = lang_width + 2 + top_dashes;
    out.push_str(&format!(
        "{} {} {}\n",
        "+".dimmed(),
        lang.dimmed(),
        "-".repeat(top_dashes).dimmed()
    ));

    let display_code = if is_wq {
        highlighter
            .map(|h| h.highlight_text(&display_source))
            .unwrap_or_else(|| display_source.clone())
    } else {
        display_source
    };

    for line in display_code.lines() {
        if is_wq {
            out.push_str(&format!("{} {}\x1b[0m\n", "|".dimmed(), line));
        } else {
            out.push_str(&format!("{} {}\n", "|".dimmed(), line));
        }
    }
    out.push_str(&format!(
        "{}{}",
        "+".dimmed(),
        "-".repeat(bottom_dashes).dimmed()
    ));
    out
}

fn is_wq_fence_lang(lang: &str) -> bool {
    lang == "wq" || lang.starts_with("wq ")
}

fn code_fence_wrap_width(explicit: Option<usize>) -> usize {
    explicit
        .filter(|width| *width > 0)
        .or_else(detected_code_fence_wrap_width)
        .unwrap_or(DEFAULT_CODE_FENCE_WRAP_WIDTH)
}

fn detected_code_fence_wrap_width() -> Option<usize> {
    terminal_size()
        .map(|(Width(width), _)| usize::from(width).saturating_sub(CODE_FENCE_PREFIX_WIDTH))
        .filter(|width| *width >= MIN_DETECTED_CODE_FENCE_WRAP_WIDTH)
}

fn smart_wrap_wq_code(code: &str, width: usize) -> Option<String> {
    if code.lines().all(|line| terminal_width(line) <= width) {
        return None;
    }

    let formatter = Formatter::new(FormatConfig {
        max_width: width,
        wrap_only: true,
        ..FormatConfig::default()
    });
    formatter
        .format_script(code)
        .ok()
        .filter(|formatted| formatted != code)
}

fn terminal_width(s: &str) -> usize {
    s.width()
}

pub(crate) fn render_terminal(md: &str) -> String {
    let parser = Parser::new(md);
    let mut out = String::new();

    let mut _in_paragraph = false;
    let mut in_strong = false;
    let mut in_em = false;
    let mut in_link = false;
    let mut in_list: Vec<bool> = Vec::new();
    let mut in_item = false;
    let mut item_number: Vec<usize> = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => _in_paragraph = true,
            Event::End(TagEnd::Paragraph) => {
                _in_paragraph = false;
                if in_item {
                    out.push('\n');
                } else {
                    out.push('\n');
                    out.push('\n');
                }
            }
            Event::Start(Tag::Strong) => in_strong = true,
            Event::End(TagEnd::Strong) => in_strong = false,
            Event::Start(Tag::Emphasis) => in_em = true,
            Event::End(TagEnd::Emphasis) => in_em = false,

            Event::Start(Tag::Link { .. }) => in_link = true,
            Event::End(TagEnd::Link) => in_link = false,
            Event::Start(Tag::List(start)) => {
                in_list.push(start.is_some());
                item_number.push(start.unwrap_or(1) as usize);
            }
            Event::End(TagEnd::List(_)) => {
                in_list.pop();
                item_number.pop();
                out.push('\n');
            }
            Event::Start(Tag::Item) => {
                in_item = true;
                let indent = "  ".repeat(in_list.len().saturating_sub(1));
                let marker = if *in_list.last().unwrap_or(&false) {
                    format!("{}. ", *item_number.last().unwrap_or(&1))
                } else {
                    "• ".to_string()
                };
                out.push_str(&format!("{}{}", indent, marker.dimmed()));
            }
            Event::End(TagEnd::Item) => {
                in_item = false;
                out.push('\n');
                if let Some(num) = item_number.last_mut() {
                    *num += 1;
                }
            }
            Event::Text(text) => {
                let s = apply_text_style(&text, in_strong, in_em, in_link);
                out.push_str(&s);
            }
            Event::Code(code) => {
                out.push_str(&format!("`{}`", code.dimmed()));
            }
            Event::SoftBreak => out.push('\n'),
            Event::HardBreak => out.push('\n'),
            Event::Rule => out.push_str(&format!("{}\n", "─".repeat(40).dimmed())),
            Event::Html(html) => out.push_str(&html),
            _ => {}
        }
    }

    out.trim_end().to_string()
}

fn apply_text_style(text: &str, strong: bool, em: bool, link: bool) -> String {
    if strong && em {
        text.bold().italic().to_string()
    } else if strong {
        text.bold().to_string()
    } else if em {
        text.italic().to_string()
    } else if link {
        text.underline().blue().to_string()
    } else {
        text.to_string()
    }
}

pub(crate) fn render_markdown_document(
    md: &str,
    highlighter: Option<&WqReplHighlighter>,
) -> String {
    let mut out = String::new();
    for segment in split_markdown_blocks(parse_segments(md)) {
        match segment {
            Segment::Markdown(md) => {
                out.push_str(&render_terminal(&md));
            }
            Segment::Heading { level, text } => {
                out.push_str(&format_heading(level, &text));
            }
            Segment::CodeFence { lang, code } => {
                out.push_str(&format_code_fence(&lang, &code, highlighter));
            }
        }
        out.push_str("\n\n");
    }
    out.trim_end().to_string()
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

pub fn run_notebook(path: &Path, mut rtflags: RuntimeFlags, interactive: bool) {
    let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {e}", path.display());
        std::process::exit(1);
    });

    let notebook = match parse_notebook(&content) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("Notebook parse error: {e}");
            std::process::exit(1);
        }
    };

    // Apply preamble config to rtflags
    if let Some(v) = &notebook.config.builtins {
        rtflags.builtins = Some(v.clone());
    }
    if let Some(v) = &notebook.config.interpreter {
        rtflags.interpreter = Some(v.clone());
    }
    if let Some(v) = notebook.config.wqdb {
        rtflags.wqdb = v;
    }
    if let Some(v) = &notebook.config.wqdb_cmds {
        rtflags.wqdb_cmds = v.clone();
    }
    if let Some(v) = notebook.config.dry {
        rtflags.dry = v;
    }
    if let Some(v) = notebook.config.no_bt {
        rtflags.bt = !v;
    }
    if let Some(v) = notebook.config.print {
        rtflags.print = v;
    }
    if let Some(v) = notebook.config.stack_size {
        rtflags.stack_size_mebibyte = v;
    }
    if let Some(v) = &notebook.config.debug {
        match wqpl::session::dbglog::DebugLogFlags::parse(v) {
            Ok(flags) => rtflags.debug_flags = flags,
            Err(e) => {
                eprintln!("Invalid debug flags in preamble: {e}");
                std::process::exit(2);
            }
        }
    }

    let mut session = Session::new();
    session.set_pause_callback(Some(wqdb_pause_handler));
    set_debug_log_flags(rtflags.debug_flags);
    session.set_bt_mode(rtflags.bt);
    set_wqstdin(Box::new(RustylineInput::new().unwrap()));
    session.set_wqdb(rtflags.wqdb);
    if !rtflags.wqdb_cmds.is_empty() {
        session.set_wqdb_batch_cmds(rtflags.wqdb_cmds.clone());
    }
    session.set_dry_mode(rtflags.dry);
    apply_builtins_flag(&mut session, &rtflags);
    apply_interpreter_flag(&mut session, &rtflags);

    let highlighter = WqReplHighlighter::new();
    if interactive {
        run_interactive(session, notebook, rtflags, path, &highlighter);
    } else {
        run_non_interactive(session, notebook, rtflags, &highlighter);
    }
}

fn run_non_interactive(
    mut session: Session,
    notebook: Notebook,
    rtflags: RuntimeFlags,
    highlighter: &WqReplHighlighter,
) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let loading = RefCell::new(HashSet::new());

    for segment in &notebook.segments {
        match segment {
            Segment::Markdown(md) => {
                println!("{}", render_terminal(md));
            }
            Segment::Heading { level, text } => {
                println!("{}", format_heading(*level, text));
            }
            Segment::CodeFence { lang, code } => {
                if lang == "wq" || lang.starts_with("wq ") {
                    println!("{}", format_code_fence(lang, code, Some(highlighter)));
                    if !rtflags.dry {
                        match eval_inline_with_load(&mut session, code, &cwd, &loading, false) {
                            Ok(report) => {
                                for w in &report.warnings {
                                    eprintln!("warning: {w}");
                                }
                                if rtflags.print
                                    && let Some(result) = report.result
                                {
                                    println!("{}", result);
                                }
                            }
                            Err(err) => {
                                print_load_error(&err, &mut session);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        println!("{}", "dry run: skipped".dimmed());
                    }
                } else {
                    println!("{}", format_code_fence(lang, code, Some(highlighter)));
                }
            }
        }
        println!();
    }
}

fn run_interactive(
    mut session: Session,
    notebook: Notebook,
    rtflags: RuntimeFlags,
    path: &Path,
    highlighter: &WqReplHighlighter,
) {
    println!(
        "{} {}",
        "wq notebook".magenta(),
        path.display().to_string().dimmed()
    );
    println!(
        "{}",
        "Commands: [n]ext [p]rev [r]un [s]kip [g]oto [c]hapters [l]ist [q]uit".dimmed()
    );
    println!();

    let segments = notebook.segments;
    let total = segments.len().saturating_sub(1);
    let mut pos = 0usize;
    let mut last_shown_pos: Option<usize> = None;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let loading = RefCell::new(HashSet::new());

    loop {
        // Show the current segment if we haven't shown it yet.
        if pos < segments.len() && last_shown_pos != Some(pos) {
            println!("{}", format!("[{pos}/{total}]").dimmed());
            match &segments[pos] {
                Segment::Markdown(md) => {
                    println!("{}", render_terminal(md));
                }
                Segment::Heading { level, text } => {
                    println!("{}", format_heading(*level, text));
                }
                Segment::CodeFence { lang, code } => {
                    println!("{}", format_code_fence(lang, code, Some(highlighter)));
                }
            }
            last_shown_pos = Some(pos);
            println!();
        }

        if pos >= segments.len() {
            println!("\n{}", "== The End ==".green());
            break;
        }

        // Determine prompt based on current segment type
        let (chapter_count, segment_count) = {
            let chapter_count = segments[..=pos]
                .iter()
                .filter(|s| matches!(s, Segment::Heading { .. }))
                .count()
                .max(1);
            let last_heading_pos = segments[..=pos]
                .iter()
                .enumerate()
                .filter(|(_, s)| matches!(s, Segment::Heading { .. }))
                .map(|(i, _)| i)
                .next_back()
                .unwrap_or(0);
            let segment_count = pos - last_heading_pos + 1;
            (chapter_count, segment_count)
        };

        let prompt_str = format!("wq[{chapter_count}][{segment_count}] ");
        let prompt = match &segments[pos] {
            Segment::CodeFence { lang, .. } if lang == "wq" || lang.starts_with("wq ") => {
                if cfg!(windows) {
                    prompt_str
                } else {
                    format!("{} ", prompt_str.cyan())
                }
            }
            _ => {
                if cfg!(windows) {
                    prompt_str
                } else {
                    format!("{} ", prompt_str.magenta())
                }
            }
        };

        match wqstdin_readline(&prompt) {
            Ok(line) => {
                println!();
                let input = line.trim();
                if !input.is_empty() {
                    wqstdin_add_history(input);
                }

                match input {
                    "n" | "next" => {
                        pos += 1;
                    }
                    "s" | "skip" => {
                        pos += 1;
                    }
                    "p" | "prev" => {
                        pos = pos.saturating_sub(1);
                    }
                    "r" | "run" => {
                        if let Segment::CodeFence { code, .. } = &segments[pos] {
                            if !rtflags.dry {
                                match eval_inline_with_load(
                                    &mut session,
                                    code,
                                    &cwd,
                                    &loading,
                                    false,
                                ) {
                                    Ok(report) => {
                                        if let Some(result) = report.result {
                                            system_msg_err(format!("{result}"), MsgType::Success);
                                        }
                                        for w in &report.warnings {
                                            system_msg_err(format!("warning: {w}"), MsgType::Info);
                                        }
                                        println!();
                                    }
                                    Err(err) => {
                                        print_load_error(&err, &mut session);
                                        println!();
                                    }
                                }
                            } else {
                                system_msg_out("dry run: skipped".to_string(), MsgType::Info);
                            }
                        } else {
                            system_msg_out("not at a code cell".to_string(), MsgType::Info);
                        }
                    }
                    "q" | "quit" | "!!" => {
                        system_msg_out("bye..".to_string(), MsgType::Info);
                        break;
                    }
                    "c" | "chapters" => {
                        system_msg_err(format!("segment {pos} / {total}"), MsgType::Info);
                        for (i, seg) in segments.iter().enumerate() {
                            if let Segment::Heading { level, text } = seg {
                                let marker = if i == pos { ">" } else { " " };
                                println!(
                                    "{} [{:>3}] {} {}",
                                    marker,
                                    i,
                                    "  ".repeat((*level as usize).saturating_sub(1)),
                                    text.dimmed()
                                );
                            }
                        }
                    }
                    "l" | "list" => {
                        for (i, seg) in segments.iter().enumerate() {
                            let marker = if i == pos { ">" } else { " " };
                            match seg {
                                Segment::Heading { level, text } => {
                                    println!(
                                        "{} [{:>3}] H{} {}",
                                        marker,
                                        i,
                                        "#".repeat(*level as usize),
                                        text.dimmed()
                                    );
                                }
                                Segment::Markdown(text) => {
                                    let preview: String =
                                        text.chars().filter(|c| !c.is_control()).take(40).collect();
                                    println!("{} [{:>3}] M {}", marker, i, preview.dimmed());
                                }
                                Segment::CodeFence { lang, .. } => {
                                    println!("{} [{:>3}] C <{}>", marker, i, lang.dimmed());
                                }
                            }
                        }
                    }
                    cmd if cmd.starts_with("g ") || cmd.starts_with("goto ") => {
                        let idx = cmd
                            .split_whitespace()
                            .nth(1)
                            .and_then(|s| s.parse::<usize>().ok());
                        if let Some(i) = idx {
                            if i < segments.len() {
                                pos = i;
                            } else {
                                system_msg_err(
                                    format!(
                                        "index out of bounds (valid: 0-{})",
                                        segments.len().saturating_sub(1)
                                    ),
                                    MsgType::Error,
                                );
                            }
                        } else {
                            system_msg_err(
                                format!(
                                    "usage: goto <index> (valid: 0-{})",
                                    segments.len().saturating_sub(1)
                                ),
                                MsgType::Error,
                            );
                        }
                    }
                    cmd if cmd.starts_with('!') => {
                        if cmd == "!reset" || cmd == "!r" {
                            session.reset_session();
                            system_msg_out("session reset".to_string(), MsgType::Info);
                        } else if cmd == "!gb" || cmd == "!g" {
                            let env = session.env_vars();
                            if env.is_empty() {
                                system_msg_out("no global bindings".to_string(), MsgType::Info);
                            } else {
                                let mut name_w = "name".len();
                                let mut value_w = "value".len();
                                let mut type_w = "type".len();
                                for (name, v) in env.clone() {
                                    name_w = name_w.max(name.len());
                                    value_w = value_w.max(v.to_string().len());
                                    type_w = type_w.max(v.type_name().len());
                                }
                                println!(
                                    "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                                    "name",
                                    "value",
                                    "type",
                                    name_w = name_w,
                                    value_w = value_w,
                                    type_w = type_w
                                );
                                println!(
                                    "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                                    "",
                                    "",
                                    "",
                                    name_w = name_w,
                                    value_w = value_w,
                                    type_w = type_w
                                );
                                for (name, v) in env.clone() {
                                    println!(
                                        "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                                        name,
                                        v.to_string(),
                                        v.type_name(),
                                        name_w = name_w,
                                        value_w = value_w,
                                        type_w = type_w
                                    );
                                }
                            }
                        } else {
                            match eval_inline_with_load(&mut session, cmd, &cwd, &loading, false) {
                                Ok(report) => {
                                    if let Some(result) = report.result {
                                        println!("{}", result);
                                    }
                                    for w in &report.warnings {
                                        system_msg_err(format!("warning: {w}"), MsgType::Info);
                                    }
                                }
                                Err(err) => {
                                    system_msg_err(format!("{err:?}"), MsgType::Error);
                                }
                            }
                        }
                    }
                    "" => {}
                    cmd if cmd == "e" || cmd == "exec" => {
                        system_msg_err("usage: e <code> | exec <code>".to_string(), MsgType::Error);
                    }
                    cmd if cmd.starts_with("e ") || cmd.starts_with("exec ") => {
                        let code = cmd.split_once(' ').map(|x| x.1).unwrap_or("").trim();
                        if code.is_empty() {
                            system_msg_err(
                                "usage: e <code> | exec <code>".to_string(),
                                MsgType::Error,
                            );
                        } else if !rtflags.dry {
                            match eval_inline_with_load(&mut session, code, &cwd, &loading, false) {
                                Ok(report) => {
                                    if let Some(result) = report.result {
                                        system_msg_err(format!("{result}"), MsgType::Success);
                                    }
                                    for w in &report.warnings {
                                        system_msg_err(format!("warning: {w}"), MsgType::Info);
                                    }
                                    println!();
                                }
                                Err(err) => {
                                    print_load_error(&err, &mut session);
                                    println!();
                                }
                            }
                        } else {
                            system_msg_out("dry run: skipped".to_string(), MsgType::Info);
                        }
                    }
                    _ => {
                        system_msg_err(
                            "unknown command (try: n r s p c l g e q)".to_string(),
                            MsgType::Error,
                        );
                    }
                }
            }
            Err(WqStdinError::Eof) => break,
            Err(WqStdinError::Interrupted) => continue,
            Err(WqStdinError::Other(e)) => {
                system_msg_err(format!("input error: {e}"), MsgType::Error);
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_frontmatter_basic() {
        let content = "---\nbuiltins = \"all\"\n---\n# Hello\n";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some("builtins = \"all\""));
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn test_split_frontmatter_no_frontmatter() {
        let content = "# Hello\n";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, None);
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn test_split_frontmatter_with_shebang() {
        let content = "#!/usr/bin/env wq\n---\nbuiltins = \"all\"\n---\n# Hello\n";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some("builtins = \"all\""));
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn test_split_frontmatter_dash_in_escaped_value() {
        // Literal newlines inside basic TOML strings are invalid;
        // users should use \n escapes or multi-line strings.
        // This test ensures a frontmatter without raw --- works.
        let content = "---\ndesc = \"foo\\n---\\nbar\"\n---\n# Hello\n";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some("desc = \"foo\\n---\\nbar\""));
        assert_eq!(body, "# Hello\n");
    }

    #[test]
    fn test_split_frontmatter_no_trailing_newline() {
        let content = "---\nbuiltins = \"all\"\n---";
        let (fm, body) = split_frontmatter(content);
        assert_eq!(fm, Some("builtins = \"all\""));
        assert_eq!(body, "");
    }

    #[test]
    fn test_parse_frontmatter_wqdb_cmds() {
        let fm = "wqdb_cmds = [\"bt\", \"c\"]\nprint = true";
        let config = parse_frontmatter(fm).unwrap();
        assert_eq!(
            config.wqdb_cmds,
            Some(vec!["bt".to_string(), "c".to_string()])
        );
        assert_eq!(config.print, Some(true));
    }

    #[test]
    fn test_parse_notebook_happy_path() {
        let content = "---\nbuiltins = \"all\"\n---\n# Title\n\nText.\n\n```wq\n1+1\n```\n\n## Sub\n\nMore text.\n"
            .replace("\r\n", "\n");
        let nb = parse_notebook(&content).unwrap();
        assert_eq!(nb.config.builtins, Some("all".to_string()));
        assert!(!nb.segments.is_empty());
        assert!(matches!(nb.segments[0], Segment::Heading { level: 1, .. }));
        let has_code = nb
            .segments
            .iter()
            .any(|s| matches!(s, Segment::CodeFence { lang, .. } if lang == "wq"));
        assert!(has_code);
    }

    #[test]
    fn test_parse_segments_heading_and_code() {
        let md = "# Hello\n\n```wq\n1+1\n```\n\nWorld\n";
        let segs = parse_segments(md);
        assert_eq!(segs.len(), 3);
        assert!(matches!(&segs[0], Segment::Heading { level: 1, text } if text == "Hello"));
        assert!(
            matches!(&segs[1], Segment::CodeFence { lang, code } if lang == "wq" && code == "1+1")
        );
        assert!(matches!(&segs[2], Segment::Markdown(text) if text == "World"));
    }

    #[test]
    fn test_render_terminal_bold() {
        let md = "**hello**";
        let out = render_terminal(md);
        assert!(out.contains("hello"));
    }

    #[test]
    fn test_render_terminal_list() {
        let md = "- a\n- b\n";
        let out = render_terminal(md);
        assert!(out.contains('a'));
        assert!(out.contains('b'));
    }

    // #[test]
    // fn test_format_code_fence_non_wq_no_ansi_reset() {
    //     let hl = WqReplHighlighter::new();
    //     let out = format_code_fence("python", "print(1)", Some(&hl));
    //     // Non-wq fences should not have trailing ANSI reset
    //     dbg!(&out);
    //     assert!(!out.contains("\x1b[0m"));
    // }

    #[test]
    fn test_format_code_fence_wq_has_ansi_reset() {
        let hl = WqReplHighlighter::new();
        let out = format_code_fence("wq", "1+1", Some(&hl));
        // Wq fences should have ANSI reset per line
        assert!(out.contains("\x1b[0m"));
    }

    #[test]
    fn test_format_code_fence_wraps_long_wq_at_formatter_breaks() {
        let out = format_code_fence_with_width(
            "wq",
            "f[(1;2;3;4;5;6;7;8;9;10)]",
            None,
            Some(12),
        );

        assert!(out.contains("| f[(1;2;3;4;\x1b[0m\n"), "got: {out:?}");
        assert!(out.contains("|     5;6;7;8;\x1b[0m\n"), "got: {out:?}");
        assert!(out.contains("|     9;10)]\x1b[0m\n"), "got: {out:?}");
        assert!(
            !out.contains("| f[(1;2;3;4;5;6;7;8;9;10)]\x1b[0m\n"),
            "got: {out:?}"
        );
    }

    #[test]
    fn test_format_code_fence_preserves_non_wq() {
        let out = format_code_fence_with_width("python", "print('abcdef')", None, Some(6));

        assert!(out.contains("| print('abcdef')\n"), "got: {out:?}");
    }

    #[test]
    fn test_format_code_fence_uses_terminal_width_for_wide_text() {
        assert_eq!(terminal_width("界"), 2);

        let wide_text = "界".repeat(25);
        let out = format_code_fence_with_width("python", &wide_text, None, Some(80));
        let mut lines = out.lines();
        let top = lines.next().expect("top fence line");
        let bottom = out.lines().last().expect("bottom fence line");

        assert_eq!(top, format!("+ python {}", "-".repeat(43)));
        assert_eq!(bottom, format!("+{}", "-".repeat(51)));
    }
}
