use std::sync::Arc;

use unicode_width::UnicodeWidthChar as _;
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::wqdb::{CodeLoc, CrashFrame, DebugInfo, SourceFile, Span};

fn clamp_byte_offset(source: &SourceFile, byte_offset: usize) -> usize {
    let mut byte_offset = byte_offset.min(source.text().len());
    while !source.text().is_char_boundary(byte_offset) {
        byte_offset = byte_offset.saturating_sub(1);
    }
    byte_offset
}

pub(super) fn format_span_snippet(
    source: &SourceFile,
    start_byte: usize,
    end_byte: usize,
    color_mode: ColorMode,
) -> String {
    let start_byte = clamp_byte_offset(source, start_byte);
    let mut end_byte = clamp_byte_offset(source, end_byte.max(start_byte.saturating_add(1)));
    if end_byte <= start_byte {
        end_byte = source.text()[start_byte..]
            .chars()
            .next()
            .map_or(start_byte, |ch| start_byte + ch.len_utf8());
    }
    let (line, _) = source.line_col(start_byte);
    let line_text = source.line_text(line);
    let (line_start, line_end_raw) = source.line_bounds(line);
    let line_end = line_end_raw.min(line_start + line_text.len());
    let span_start = start_byte.max(line_start).min(line_end);
    let span_end = end_byte.min(line_end);
    let relative_start = span_start.saturating_sub(line_start);
    let relative_end = span_end.saturating_sub(line_start);
    let use_color = color_mode.should_colorize();
    let mut output = String::new();
    let gutter = format!("  {line} ->");

    let line_display = if use_color {
        let before_span = &line_text[..relative_start];
        let span_text = &line_text[relative_start..relative_end];
        let after_span = &line_text[relative_end..];
        let mut line_display = String::new();
        line_display.push_str(before_span);
        line_display.push_str(&paint(
            span_text,
            TextStyle::new().fg(AnsiColor::Green).bold().underline(),
            color_mode,
        ));
        line_display.push_str(after_span);
        line_display
    } else {
        line_text.to_string()
    };

    output.push_str(&format!("{gutter} {line_display}"));
    output.push('\n');

    if !use_color {
        let source_column = gutter.chars().count() + 1;
        let underline_start =
            source_column + terminal_text_width(&line_text[..relative_start], source_column);
        output.push_str(&" ".repeat(underline_start));
        let width =
            terminal_text_width(&line_text[relative_start..relative_end], underline_start).max(1);
        output.push_str(&"~".repeat(width));
    }
    output
}

fn terminal_text_width(text: &str, start_column: usize) -> usize {
    const TAB_STOP: usize = 8;
    let mut column = start_column;
    for ch in text.chars() {
        if ch == '\t' {
            column += TAB_STOP - column % TAB_STOP;
        } else {
            column += ch.width().unwrap_or(0);
        }
    }
    column - start_column
}

fn format_frame(
    debug_info: &DebugInfo,
    location: CodeLoc,
    name: &str,
    is_current: bool,
    color_mode: ColorMode,
) -> String {
    let metadata = match debug_info
        .get_chunk(location.chunk)
        .filter(|metadata| location.pc < metadata.len)
    {
        Some(metadata) => metadata,
        None => {
            let bullet = if is_current { '*' } else { '+' };
            let gutter = paint(
                "| ",
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            );
            let mut output = paint(
                &format!("{bullet} at {name}, ?:?:?"),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            );
            output.push('\n');
            output.push_str(&gutter);
            output.push_str(&format!("{:>4} -> ?", "?"));
            return output;
        }
    };
    let mut span = metadata.line_table.span_at(location.pc);
    if span.file_id == u32::MAX {
        span = metadata.line_table.context_span_at(location.pc);
    }
    let bullet = if is_current { '*' } else { '+' };
    let gutter = paint(
        "| ",
        TextStyle::new().fg(AnsiColor::BrightYellow),
        color_mode,
    );
    let use_inline_underline = color_mode.should_colorize();
    if span.file_id == u32::MAX {
        let mut first_statement_pc = None;
        for pc in 0..metadata.len {
            if metadata.line_table.is_stmt(pc) {
                first_statement_pc = Some(pc);
                break;
            }
        }
        if let Some(pc) = first_statement_pc {
            let first_span = metadata.line_table.span_at(pc);
            if first_span.file_id != u32::MAX {
                span = first_span;
            }
        }
        if span.file_id == u32::MAX {
            if let Some(source) = debug_info.file(metadata.file_id) {
                let mut output = paint(
                    &format!("{bullet} at {name}, {}:?:?", source.path()),
                    TextStyle::new().fg(AnsiColor::BrightYellow),
                    color_mode,
                );
                output.push('\n');
                output.push_str(&gutter);
                output.push_str(&format!("{:>4} -> ?", "?"));
                return output;
            }
            let mut output = paint(
                &format!("{bullet} at {name}, ?:?:?"),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            );
            output.push('\n');
            output.push_str(&gutter);
            output.push_str(&format!("{:>4} -> ?", "?"));
            return output;
        }
    }
    if let Some(source) = debug_info.file(span.file_id) {
        let start_byte = clamp_byte_offset(source, span.start);
        let mut end_byte = clamp_byte_offset(source, span.end.max(start_byte.saturating_add(1)));
        if end_byte <= start_byte {
            end_byte = source.text()[start_byte..]
                .chars()
                .next()
                .map_or(start_byte, |ch| start_byte + ch.len_utf8());
        }
        let (line, column) = source.line_col(start_byte);
        let end_lookup = end_byte
            .saturating_sub(1)
            .min(source.text().len().saturating_sub(1));
        let (end_line, _) = source.line_col(end_lookup);
        let mut output = paint(
            &format!("{bullet} at {name}, {}:{line}:{column}", source.path()),
            TextStyle::new().fg(AnsiColor::BrightYellow),
            color_mode,
        );
        output.push('\n');
        let total_lines = source.line_count();
        let first_line = if line > 1 { line - 1 } else { 1 };
        let last_line = if end_line < total_lines {
            end_line + 1
        } else {
            total_lines
        };
        for current_line in first_line..=last_line {
            let line_text = source.line_text(current_line);
            let (line_start, line_end_raw) = source.line_bounds(current_line);
            let line_end = line_end_raw.min(line_start + line_text.len());
            let span_start = start_byte.max(line_start);
            let span_end = end_byte.min(line_end);
            let has_overlap = span_start < line_end && span_end > line_start;
            output.push_str(&gutter);
            if current_line == line {
                let marker = if line_text.is_empty() {
                    format!("{current_line:>4} ->")
                } else {
                    format!("{current_line:>4} -> ")
                };
                output.push_str(&paint(
                    &marker,
                    TextStyle::new().fg(AnsiColor::Green).bold(),
                    color_mode,
                ));
            } else if line_text.is_empty() {
                output.push_str(&format!("{current_line:>4}"));
            } else {
                output.push_str(&format!("{current_line:>4}    "));
            }
            if has_overlap && use_inline_underline {
                let relative_start = span_start - line_start;
                let relative_end = span_end - line_start;
                output.push_str(&line_text[..relative_start]);
                output.push_str(&paint(
                    &line_text[relative_start..relative_end],
                    TextStyle::new().fg(AnsiColor::Green).bold().underline(),
                    color_mode,
                ));
                output.push_str(&line_text[relative_end..]);
            } else {
                output.push_str(line_text);
            }
            output.push('\n');
            if has_overlap && !use_inline_underline {
                let pointer_start = if current_line == line {
                    column.saturating_sub(1)
                } else {
                    0
                };
                let pointer_width = source.text()[span_start..span_end].chars().count().max(1);
                output.push_str(&gutter);
                output.push_str("        ");
                output.push_str(&paint(
                    &format!("{}{}", " ".repeat(pointer_start), "~".repeat(pointer_width)),
                    TextStyle::new().fg(AnsiColor::Green).bold(),
                    color_mode,
                ));
                output.push('\n');
            }
        }
        if output.ends_with('\n') {
            output.truncate(output.len() - 1);
        }
        output
    } else {
        let mut output = paint(
            &format!(
                "{bullet} at {}, chunk {:?}, pc {}",
                name, metadata.id, location.pc
            ),
            TextStyle::new().fg(AnsiColor::Yellow),
            color_mode,
        );
        output.push('\n');
        output.push_str(&gutter);
        output.push_str(&format!("{:>4} -> ?", "?"));
        output
    }
}

pub(super) fn format_crash_frame(
    frame: &CrashFrame,
    is_current: bool,
    color_mode: ColorMode,
) -> String {
    match frame {
        CrashFrame::TailCallsOmitted => {
            let bullet = if is_current { '*' } else { '+' };
            paint(
                &format!("{bullet} at {}", frame.function()),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            )
        }
        CrashFrame::Located {
            function,
            location,
            source: None,
            ..
        } => {
            let bullet = if is_current { '*' } else { '+' };
            paint(
                &format!("{bullet} at {function}, ?:?:? (pc {})", location.pc),
                TextStyle::new().fg(AnsiColor::BrightYellow),
                color_mode,
            )
        }
        CrashFrame::Located {
            function,
            location,
            source: Some(source),
            ..
        } => {
            let mut debug_info = DebugInfo::default();
            let file_id = debug_info.new_file(Arc::clone(&source.path), Arc::clone(&source.source));
            let chunk =
                debug_info.new_chunk(Arc::clone(function), file_id, location.pc.saturating_add(1));
            if !debug_info.set_exact_span(
                CodeLoc {
                    chunk,
                    pc: location.pc,
                },
                Span {
                    file_id,
                    start: source.span.start,
                    end: source.span.end,
                },
            ) {
                let bullet = if is_current { '*' } else { '+' };
                return paint(
                    &format!("{bullet} at {function}, ?:?:? (pc {})", location.pc),
                    TextStyle::new().fg(AnsiColor::BrightYellow),
                    color_mode,
                );
            }
            format_frame(
                &debug_info,
                CodeLoc {
                    chunk,
                    pc: location.pc,
                },
                function,
                is_current,
                color_mode,
            )
        }
    }
}
