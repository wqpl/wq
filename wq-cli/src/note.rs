use std::io::{IsTerminal as _, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use terminal_size::{Width, terminal_size};
use unicode_width::UnicodeWidthStr as _;
use wqpl::format::{FormatConfig, Formatter};
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};

use crate::repl::editor::WqReplHighlighter;

#[derive(Debug, Clone)]
pub enum Segment {
    Markdown(String),
    Heading { level: u8, text: String },
    CodeFence { lang: String, code: String },
    Newlines(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SoftBreakMode {
    Space,
    Newline,
}

fn strip_frontmatter(content: &str) -> &str {
    let mut rest = content;

    // Skip optional shebang line
    if rest.starts_with("#!") {
        if let Some(idx) = rest.find('\n') {
            rest = &rest[idx + 1..];
        } else {
            return "";
        }
    }

    let trimmed = rest.trim_start();
    if !trimmed.starts_with("---") {
        return rest;
    }

    let after_open = &trimmed[3..];
    let mut pos = 0;
    while pos < after_open.len() {
        let slice = &after_open[pos..];
        let nl_idx = slice.find('\n').unwrap_or(slice.len());
        let line = slice[..nl_idx].trim_end();
        if line == "---" {
            let body = if nl_idx < slice.len() {
                &slice[nl_idx + 1..]
            } else {
                ""
            };
            return body;
        }
        pos += nl_idx + 1;
    }

    rest
}

fn split_markdown_blocks(segments: Vec<Segment>) -> Vec<Segment> {
    let mut result = Vec::new();
    for seg in segments {
        match seg {
            Segment::Markdown(text) => {
                let normalized = text.replace("\r\n", "\n");
                let mut block = String::new();
                let mut blank_lines = 0;
                for line in normalized.split('\n') {
                    if line.trim().is_empty() {
                        blank_lines += 1;
                        continue;
                    }
                    if blank_lines > 0 {
                        if !block.is_empty() {
                            result.push(Segment::Markdown(block.trim().to_string()));
                            result.push(Segment::Newlines(blank_lines));
                            block.clear();
                        }
                        blank_lines = 0;
                    } else if !block.is_empty() {
                        block.push('\n');
                    }
                    block.push_str(line);
                }
                if !block.trim().is_empty() {
                    result.push(Segment::Markdown(block.trim().to_string()));
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
    format_heading_with_color_mode(level, text, ColorMode::Auto)
}

fn format_heading_with_color_mode(level: u8, text: &str, color_mode: ColorMode) -> String {
    let prefix = "#".repeat(level as usize);
    let colored = match level {
        1 => note_paint_with_color_mode(
            &prefix,
            TextStyle::new().fg(AnsiColor::Magenta).bold(),
            color_mode,
        ),
        2 => note_paint_with_color_mode(
            &prefix,
            TextStyle::new().fg(AnsiColor::Blue).bold(),
            color_mode,
        ),
        3 => note_paint_with_color_mode(
            &prefix,
            TextStyle::new().fg(AnsiColor::Cyan).bold(),
            color_mode,
        ),
        _ => note_paint_with_color_mode(&prefix, TextStyle::new().dimmed().bold(), color_mode),
    };
    format!(
        "{} {}",
        colored,
        note_paint_with_color_mode(text, TextStyle::new().bold(), color_mode)
    )
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
        note_dim("+"),
        note_dim(lang),
        note_dim(&"-".repeat(top_dashes))
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
            out.push_str(&format!("{} {}\x1b[0m\n", note_dim("|"), line));
        } else {
            out.push_str(&format!("{} {}\n", note_dim("|"), line));
        }
    }
    out.push_str(&format!(
        "{}{}",
        note_dim("+"),
        note_dim(&"-".repeat(bottom_dashes))
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

fn render_terminal_with_soft_break_mode(md: &str, soft_break_mode: SoftBreakMode) -> String {
    let parser = Parser::new(md);
    let mut out = String::new();

    let mut _in_paragraph = false;
    let mut in_strong = false;
    let mut in_em = false;
    let mut in_link = false;
    let mut in_list: Vec<bool> = Vec::new();
    let mut item_continuation_indent: Vec<String> = Vec::new();
    let mut item_number: Vec<usize> = Vec::new();

    for event in parser {
        match event {
            Event::Start(Tag::Paragraph) => _in_paragraph = true,
            Event::End(TagEnd::Paragraph) => {
                _in_paragraph = false;
                if !item_continuation_indent.is_empty() {
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
                let indent = "  ".repeat(in_list.len().saturating_sub(1));
                let marker = if *in_list.last().unwrap_or(&false) {
                    format!("{}. ", *item_number.last().unwrap_or(&1))
                } else {
                    "• ".to_string()
                };
                item_continuation_indent.push(format!(
                    "{}{}",
                    indent,
                    " ".repeat(marker.chars().count())
                ));
                out.push_str(&format!("{}{}", indent, note_dim(&marker)));
            }
            Event::End(TagEnd::Item) => {
                item_continuation_indent.pop();
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
                out.push_str(&format!("`{}`", note_dim(&code)));
            }
            Event::SoftBreak => match soft_break_mode {
                SoftBreakMode::Space => out.push(' '),
                SoftBreakMode::Newline => {
                    push_terminal_newline(
                        &mut out,
                        item_continuation_indent.last().map(String::as_str),
                    );
                }
            },
            Event::HardBreak => {
                push_terminal_newline(
                    &mut out,
                    item_continuation_indent.last().map(String::as_str),
                );
            }
            Event::Rule => out.push_str(&format!("{}\n", note_dim(&"─".repeat(40)))),
            Event::Html(html) => out.push_str(&html),
            _ => {}
        }
    }

    out.trim_end().to_string()
}

fn push_terminal_newline(out: &mut String, indent: Option<&str>) {
    out.push('\n');
    if let Some(indent) = indent {
        out.push_str(indent);
    }
}

fn apply_text_style(text: &str, strong: bool, em: bool, link: bool) -> String {
    apply_text_style_with_color_mode(text, strong, em, link, ColorMode::Auto)
}

fn apply_text_style_with_color_mode(
    text: &str,
    strong: bool,
    em: bool,
    link: bool,
    color_mode: ColorMode,
) -> String {
    if strong && em {
        note_paint_with_color_mode(text, TextStyle::new().bold().italic(), color_mode)
    } else if strong {
        note_paint_with_color_mode(text, TextStyle::new().bold(), color_mode)
    } else if em {
        note_paint_with_color_mode(text, TextStyle::new().italic(), color_mode)
    } else if link {
        note_paint_with_color_mode(
            text,
            TextStyle::new().fg(AnsiColor::Blue).underline(),
            color_mode,
        )
    } else {
        text.to_string()
    }
}

fn note_dim(text: &str) -> String {
    note_paint(text, TextStyle::new().dimmed())
}

fn note_paint(text: &str, style: TextStyle) -> String {
    note_paint_with_color_mode(text, style, ColorMode::Auto)
}

fn note_paint_with_color_mode(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    paint(text, style, color_mode)
}

pub(crate) fn render_markdown_document(
    md: &str,
    highlighter: Option<&WqReplHighlighter>,
) -> String {
    render_markdown_document_with_soft_break_mode(md, highlighter, SoftBreakMode::Space)
}

pub(crate) fn render_markdown_document_with_soft_break_mode(
    md: &str,
    highlighter: Option<&WqReplHighlighter>,
    soft_break_mode: SoftBreakMode,
) -> String {
    let mut out = String::new();
    let mut segments = split_markdown_blocks(parse_segments(md))
        .into_iter()
        .peekable();
    while let Some(segment) = segments.next() {
        let is_newlines = matches!(segment, Segment::Newlines(_));
        match segment {
            Segment::Markdown(md) => {
                out.push_str(&render_terminal_with_soft_break_mode(&md, soft_break_mode));
            }
            Segment::Heading { level, text } => {
                out.push_str(&format_heading(level, &text));
            }
            Segment::CodeFence { lang, code } => {
                out.push_str(&format_code_fence(&lang, &code, highlighter));
            }
            Segment::Newlines(count) => {
                out.push_str(&"\n".repeat(count));
            }
        }
        if !is_newlines
            && matches!(segments.peek(), Some(segment) if !matches!(segment, Segment::Newlines(_)))
        {
            out.push_str("\n\n");
        }
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

pub fn run_markdown(path: &Path, no_pager: bool) {
    let content = std::fs::read_to_string(path).unwrap_or_else(|e| {
        eprintln!("Cannot read {}: {e}", path.display());
        std::process::exit(1);
    });

    let highlighter = WqReplHighlighter::new();
    let rendered = render_markdown_document(strip_frontmatter(&content), Some(&highlighter));
    print_or_page(&rendered, no_pager);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render_terminal(md: &str) -> String {
        render_terminal_with_soft_break_mode(md, SoftBreakMode::Space)
    }

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn test_strip_frontmatter_basic() {
        let content = "---\nbuiltins = \"all\"\n---\n# Hello\n";
        assert_eq!(strip_frontmatter(content), "# Hello\n");
    }

    #[test]
    fn test_strip_frontmatter_no_frontmatter() {
        let content = "# Hello\n";
        assert_eq!(strip_frontmatter(content), "# Hello\n");
    }

    #[test]
    fn test_strip_frontmatter_with_shebang() {
        let content = "#!/usr/bin/env wq\n---\nbuiltins = \"all\"\n---\n# Hello\n";
        assert_eq!(strip_frontmatter(content), "# Hello\n");
    }

    #[test]
    fn test_strip_frontmatter_dash_in_escaped_value() {
        let content = "---\ndesc = \"foo\\n---\\nbar\"\n---\n# Hello\n";
        assert_eq!(strip_frontmatter(content), "# Hello\n");
    }

    #[test]
    fn test_strip_frontmatter_no_trailing_newline() {
        let content = "---\nbuiltins = \"all\"\n---";
        assert_eq!(strip_frontmatter(content), "");
    }

    #[test]
    fn test_strip_frontmatter_keeps_markdown_segments() {
        let content = "---\nbuiltins = \"all\"\n---\n# Title\n\nText.\n\n```wq\n1+1\n```\n\n## Sub\n\nMore text.\n"
            .replace("\r\n", "\n");
        let segments = split_markdown_blocks(parse_segments(strip_frontmatter(&content)));
        assert!(!segments.is_empty());
        assert!(matches!(segments[0], Segment::Heading { level: 1, .. }));
        let has_code = segments
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
    fn heading_and_link_styles_use_explicit_renderer() {
        assert_eq!(
            format_heading_with_color_mode(1, "Title", ColorMode::Always),
            "\x1b[1;35m#\x1b[0m \x1b[1mTitle\x1b[0m"
        );
        assert_eq!(
            apply_text_style_with_color_mode("wq", false, false, true, ColorMode::Never),
            "wq"
        );
    }

    #[test]
    fn test_render_terminal_list() {
        let md = "- a\n- b\n";
        let out = render_terminal(md);
        assert!(out.contains('a'));
        assert!(out.contains('b'));
    }

    #[test]
    fn test_render_terminal_list_soft_break_is_inline() {
        let md = "- alpha\n  beta\n";
        let out = render_terminal(md);

        assert!(out.contains("alpha beta"));
    }

    #[test]
    fn markdown_newline_runs_render_one_newline_shorter() {
        for (markdown, expected) in [
            ("a.\nb.", "a. b."),
            ("a.\r\nb.", "a. b."),
            ("a.\n\nb.", "a.\nb."),
            ("a.\n\n\nb.", "a.\n\nb."),
            ("a.\n\n\n\nb.", "a.\n\n\nb."),
        ] {
            assert_eq!(render_markdown_document(markdown, None), expected);
        }
    }

    #[test]
    fn explicit_markdown_hard_break_still_renders_a_newline() {
        assert_eq!(render_terminal("a.  \nb."), "a.\nb.");
        assert_eq!(
            render_terminal("x  \n``alpha``  \n``beta``"),
            "x\n`alpha`\n`beta`"
        );
    }

    #[test]
    fn rendered_reference_soft_breaks_can_use_terminal_newlines() {
        assert_eq!(
            render_terminal_with_soft_break_mode("a.\nb.", SoftBreakMode::Newline),
            "a.\nb."
        );
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
        let out = format_code_fence_with_width("wq", "f[(1;2;3;4;5;6;7;8;9;10)]", None, Some(12));
        let visible = strip_ansi(&out);

        assert!(visible.contains("| f[(1;2;3;4;\n"), "got: {out:?}");
        assert!(visible.contains("|     5;6;7;8;\n"), "got: {out:?}");
        assert!(visible.contains("|     9;10)]\n"), "got: {out:?}");
        assert!(
            !visible.contains("| f[(1;2;3;4;5;6;7;8;9;10)]\n"),
            "got: {out:?}"
        );
    }

    #[test]
    fn test_format_code_fence_preserves_non_wq() {
        let out = format_code_fence_with_width("python", "print('abcdef')", None, Some(6));

        assert!(
            strip_ansi(&out).contains("| print('abcdef')\n"),
            "got: {out:?}"
        );
    }

    #[test]
    fn test_format_code_fence_uses_terminal_width_for_wide_text() {
        assert_eq!(terminal_width("界"), 2);

        let wide_text = "界".repeat(25);
        let out = format_code_fence_with_width("python", &wide_text, None, Some(80));
        let visible = strip_ansi(&out);
        let mut lines = visible.lines();
        let top = lines.next().expect("top fence line");
        let bottom = visible.lines().last().expect("bottom fence line");

        assert_eq!(top, format!("+ python {}", "-".repeat(43)));
        assert_eq!(bottom, format!("+{}", "-".repeat(51)));
    }
}
