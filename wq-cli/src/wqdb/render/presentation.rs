use unicode_width::UnicodeWidthChar as _;
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::wqdb::{CodeLoc, DebugInfo, DebugInstruction, InstructionClass, Span, StepGranularity};

use super::format_span_snippet;
use crate::wqdb::command::{CommandSpec, UsageArg, command_usage_plain};
use crate::wqdb::host::Host;

pub(in crate::wqdb) fn command_usage_styled(spec: &CommandSpec, color_mode: ColorMode) -> String {
    let mut usage = String::new();
    for (idx, alias) in spec.aliases.iter().enumerate() {
        if idx > 0 {
            usage.push_str(&format!(" {} ", styled_separator(color_mode)));
        }
        usage.push_str(&styled_command(alias, color_mode));
    }
    for arg in spec.args {
        usage.push(' ');
        usage.push_str(&styled_usage_arg(*arg, color_mode));
    }
    usage
}

fn styled_usage_arg(arg: UsageArg, color_mode: ColorMode) -> String {
    match arg {
        UsageArg::Required(name) => styled_required_arg(name, color_mode),
        UsageArg::Optional(name) => styled_optional_arg(name, color_mode),
    }
}

pub(in crate::wqdb) fn styled_command(text: &str, color_mode: ColorMode) -> String {
    paint(text, TextStyle::new().fg(AnsiColor::Green), color_mode)
}

pub(in crate::wqdb) fn styled_subcommand(text: &str, color_mode: ColorMode) -> String {
    color(text, AnsiColor::BrightCyan, color_mode)
}

pub(in crate::wqdb) fn styled_flag(text: &str, color_mode: ColorMode) -> String {
    color(text, AnsiColor::BrightMagenta, color_mode)
}

pub(in crate::wqdb) fn styled_required_arg(name: &str, color_mode: ColorMode) -> String {
    format!(
        "{}{}{}",
        dim("<", color_mode),
        color(name, AnsiColor::BrightYellow, color_mode),
        dim(">", color_mode)
    )
}

fn styled_optional_arg(name: &str, color_mode: ColorMode) -> String {
    format!(
        "{}{}{}",
        dim("[", color_mode),
        color(name, AnsiColor::BrightYellow, color_mode),
        dim("]", color_mode)
    )
}

pub(in crate::wqdb) fn styled_separator(color_mode: ColorMode) -> String {
    dim("|", color_mode)
}

pub(in crate::wqdb) fn title(text: &str, color_mode: ColorMode) -> String {
    paint(
        text,
        TextStyle::new().fg(AnsiColor::BrightMagenta).bold(),
        color_mode,
    )
}

pub(in crate::wqdb) fn bold(text: &str, color_mode: ColorMode) -> String {
    paint(text, TextStyle::new().bold(), color_mode)
}

pub(in crate::wqdb) fn header(text: &str, color_mode: ColorMode) -> String {
    paint(text, TextStyle::new().bold().underline(), color_mode)
}

pub(in crate::wqdb) fn dim(text: &str, color_mode: ColorMode) -> String {
    color(text, AnsiColor::BrightBlack, color_mode)
}

pub(in crate::wqdb) fn color(text: &str, ansi_color: AnsiColor, color_mode: ColorMode) -> String {
    paint(text, TextStyle::new().fg(ansi_color), color_mode)
}

pub(in crate::wqdb) fn render_debug_instruction(
    instruction: &DebugInstruction,
    color_mode: ColorMode,
) -> String {
    let color = match instruction.class {
        InstructionClass::Load => AnsiColor::Red,
        InstructionClass::Store => AnsiColor::Green,
        InstructionClass::Call => AnsiColor::Blue,
        InstructionClass::Jump => AnsiColor::Yellow,
        InstructionClass::Stack => AnsiColor::BrightBlack,
        InstructionClass::Operator => AnsiColor::Magenta,
        InstructionClass::Indexing => AnsiColor::Purple,
        InstructionClass::Construct => AnsiColor::BrightRed,
        InstructionClass::Try => AnsiColor::BrightYellow,
    };
    let mut opcode_style = TextStyle::new().fg(color);
    if instruction.is_special {
        opcode_style = opcode_style.bold().italic();
    }
    let mut rendered = paint(&instruction.opcode, opcode_style, color_mode);
    rendered.push_str(&instruction.operands);
    if !instruction.annotations.is_empty() {
        rendered.push_str("  ");
        rendered.push_str(&dim(
            &format!("// {}", instruction.annotations.join("; ")),
            color_mode,
        ));
    }
    rendered
}

pub(in crate::wqdb) fn styled_track_command(
    scope: &str,
    arg: &str,
    color_mode: ColorMode,
) -> String {
    format!(
        "{} {} {}",
        styled_command("track", color_mode),
        styled_subcommand(scope, color_mode),
        styled_required_arg(arg, color_mode)
    )
}

pub(in crate::wqdb) fn styled_stop_hook_command(
    action: &str,
    suffix: Option<String>,
    color_mode: ColorMode,
) -> String {
    match suffix {
        Some(suffix) => format!(
            "{} {} {suffix}",
            styled_command("stop-hook", color_mode),
            styled_subcommand(action, color_mode)
        ),
        None => format!(
            "{} {}",
            styled_command("stop-hook", color_mode),
            styled_subcommand(action, color_mode)
        ),
    }
}

pub(in crate::wqdb) fn help_row(
    spec: &CommandSpec,
    usage_width: usize,
    color_mode: ColorMode,
) -> String {
    let usage = command_usage_styled(spec, color_mode);
    let padding = usage_width - command_usage_plain(spec).len();
    format!("  {usage}{:padding$}  {}", "", spec.summary)
}

pub(in crate::wqdb) fn prompt(
    granularity: StepGranularity,
    line: usize,
    color_mode: ColorMode,
) -> String {
    let title = paint(
        "wqdb",
        TextStyle::new().fg(AnsiColor::BrightMagenta).bold(),
        color_mode,
    );
    let granularity = paint(
        granularity.as_str(),
        TextStyle::new().fg(AnsiColor::BrightCyan),
        color_mode,
    );
    let line = paint(
        &line.to_string(),
        TextStyle::new().fg(AnsiColor::BrightBlue),
        color_mode,
    );
    format!("{title}[{granularity}:{line}] ")
}

fn mode_header(label: &str, detail: &str, color_mode: ColorMode) -> String {
    let label = paint(
        label,
        TextStyle::new().fg(AnsiColor::BrightCyan).bold(),
        color_mode,
    );
    format!("{label}  {detail}")
}

pub(in crate::wqdb) fn resolved_stop_span(di: &DebugInfo, loc: CodeLoc) -> (Span, bool) {
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        return (Span::NONE, false);
    };
    if let Some(span) = meta.line_table.exact_pc_span.get(loc.pc)
        && span.file_id != u32::MAX
    {
        return (*span, true);
    }
    if meta.line_table.is_stmt(loc.pc)
        && let Some(span) = meta.line_table.pc_to_stmt_span.get(loc.pc)
        && span.file_id != u32::MAX
    {
        return (*span, true);
    }
    let span = meta.line_table.context_span_at(loc.pc);
    if span.file_id != u32::MAX {
        return (span, false);
    }
    for &pc in &meta.line_table.stmt_pcs {
        if pc >= loc.pc {
            let span = meta.line_table.span_at(pc);
            if span.file_id != u32::MAX {
                return (span, false);
            }
        }
    }
    (Span::NONE, false)
}

pub(in crate::wqdb) fn format_line_stop_card(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    radius: usize,
    color_mode: ColorMode,
) -> String {
    let (span, _) = resolved_stop_span(di, loc);
    let Some(source) = di.file(span.file_id) else {
        return mode_header(
            "LINE",
            &format!("pc {} in {name}\n  source unavailable", loc.pc),
            color_mode,
        );
    };
    let (line, _) = source.line_col(span.start);
    let mut out = mode_header(
        "LINE",
        &format!("{}:{line} in {name}", source.path()),
        color_mode,
    );
    let total = source
        .line_count()
        .saturating_sub(usize::from(source.text().ends_with('\n')))
        .max(1);
    let first = line.saturating_sub(radius).max(1);
    let last = line.saturating_add(radius).min(total);
    for current in first..=last {
        out.push('\n');
        let source_line = source.line_text(current);
        if current == line {
            out.push_str(&paint(
                &format!("{current:>4} -> {source_line}"),
                TextStyle::new().fg(AnsiColor::Green).bold(),
                color_mode,
            ));
        } else {
            out.push_str(&format!("{current:>4}    {source_line}"));
        }
    }
    out
}

pub(in crate::wqdb) fn format_expr_stop_card(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    instruction: Option<&str>,
    color_mode: ColorMode,
) -> String {
    let (span, _) = resolved_stop_span(di, loc);
    let Some(source) = di.file(span.file_id) else {
        let mut out = mode_header("EXPR", &format!("pc {} in {name}", loc.pc), color_mode);
        out.push_str("\n  source unavailable");
        if let Some(instruction) = instruction {
            out.push('\n');
            out.push_str(&paint(
                &format!("pc {}  ", loc.pc),
                TextStyle::new().fg(AnsiColor::BrightBlack),
                color_mode,
            ));
            out.push_str(&compact_instruction(instruction));
        }
        return out;
    };
    let (line, col) = source.display_line_col(span.start);
    let mut out = mode_header(
        "EXPR",
        &format!("{}:{line}:{col} in {name}", source.path()),
        color_mode,
    );
    out.push('\n');
    out.push_str(
        format_span_snippet(source, span.start, span.end, color_mode).trim_end_matches('\n'),
    );
    if let Some(instruction) = instruction {
        out.push('\n');
        out.push_str(&paint(
            &format!("pc {}  ", loc.pc),
            TextStyle::new().fg(AnsiColor::BrightBlack),
            color_mode,
        ));
        out.push_str(&compact_instruction(instruction));
    }
    out
}

pub(in crate::wqdb) fn compact_instruction(instruction: &str) -> String {
    const LIMIT: usize = 120;
    let instruction = instruction.replace(['\n', '\r'], " ");
    if ansi_visible_width(&instruction) <= LIMIT {
        return instruction;
    }
    let mut compact = String::with_capacity(instruction.len().min(LIMIT * 2));
    let mut visible = 0usize;
    let mut in_escape = false;
    for ch in instruction.chars() {
        if in_escape {
            compact.push(ch);
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            compact.push(ch);
            in_escape = true;
        } else {
            let width = ch.width().unwrap_or(0);
            if visible + width > LIMIT - 1 {
                compact.push('…');
                break;
            }
            compact.push(ch);
            visible += width;
        }
    }
    if instruction.contains('\x1b') {
        compact.push_str("\x1b[0m");
    }
    compact
}

pub(in crate::wqdb) fn ansi_visible_width(text: &str) -> usize {
    let mut visible = 0usize;
    let mut in_escape = false;
    for ch in text.chars() {
        if in_escape {
            if ch == 'm' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            in_escape = true;
        } else {
            visible += ch.width().unwrap_or(0);
        }
    }
    visible
}

pub(in crate::wqdb) fn format_inst_stop_card(
    di: &DebugInfo,
    loc: CodeLoc,
    name: &str,
    instruction_len: usize,
    instructions: &[(usize, String)],
    color_mode: ColorMode,
) -> String {
    let last_pc = instruction_len.saturating_sub(1);
    let mut out = mode_header(
        "INST",
        &format!("{name}  pc {}/{last_pc}", loc.pc),
        color_mode,
    );
    for (pc, instruction) in instructions {
        out.push('\n');
        let prefix = if *pc == loc.pc {
            paint(
                &format!("{pc:>4} -> "),
                TextStyle::new().fg(AnsiColor::Green).bold(),
                color_mode,
            )
        } else {
            format!("{pc:>4}    ")
        };
        out.push_str(&prefix);
        out.push_str(&compact_instruction(instruction));
    }

    let (span, is_precise) = resolved_stop_span(di, loc);
    if let Some(source) = di.file(span.file_id) {
        let (line, col) = source.display_line_col(span.start);
        out.push_str("\n\n");
        out.push_str(&mode_header(
            if is_precise { "SOURCE" } else { "CONTEXT" },
            &format!("{}:{line}:{col}", source.path()),
            color_mode,
        ));
        out.push('\n');
        out.push_str(
            format_span_snippet(source, span.start, span.end, color_mode).trim_end_matches('\n'),
        );
    } else {
        out.push_str("\n\n");
        out.push_str(&mode_header("SOURCE", "unavailable", color_mode));
    }
    out
}

pub(in crate::wqdb) fn render_stop_card(host: &Host<'_, '_>, color_mode: ColorMode) -> String {
    let granularity = host.step_granularity();
    let Some(loc) = host.location() else {
        return unavailable_stop_card(granularity, color_mode);
    };
    let di = host.debug_info();
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        return unavailable_stop_card(granularity, color_mode);
    };
    let name = meta.name.as_ref();
    match granularity {
        StepGranularity::Line => format_line_stop_card(di, loc, name, 2, color_mode),
        StepGranularity::Expr => format_expr_stop_card(
            di,
            loc,
            name,
            host.instruction_at(loc.pc)
                .as_ref()
                .map(|instruction| render_debug_instruction(instruction, color_mode))
                .as_deref(),
            color_mode,
        ),
        StepGranularity::Inst => {
            let start = loc.pc.saturating_sub(3);
            let end = loc.pc.saturating_add(3).min(meta.len.saturating_sub(1));
            let instructions = (start..=end)
                .filter_map(|pc| {
                    host.instruction_at(pc)
                        .map(|instruction| (pc, render_debug_instruction(&instruction, color_mode)))
                })
                .collect::<Vec<_>>();
            format_inst_stop_card(di, loc, name, meta.len, &instructions, color_mode)
        }
    }
}

pub(in crate::wqdb) fn unavailable_stop_card(
    granularity: StepGranularity,
    color_mode: ColorMode,
) -> String {
    let label = match granularity {
        StepGranularity::Line => "LINE",
        StepGranularity::Expr => "EXPR",
        StepGranularity::Inst => "INST",
    };
    mode_header(label, "current location unavailable", color_mode)
}

pub(in crate::wqdb) fn stop_controls(
    granularity: StepGranularity,
    color_mode: ColorMode,
) -> String {
    paint(
        &format!(
            "[n] next {} [s] step in [fin] step out [c] continue [g] <line|expr|inst>",
            granularity.as_str()
        ),
        TextStyle::new().dimmed(),
        color_mode,
    )
}

pub(in crate::wqdb) fn format_loc_hint(
    di: &DebugInfo,
    loc: CodeLoc,
    name_hint: Option<&str>,
) -> String {
    let Some(resolved) = di.resolve_location(loc) else {
        return format!(
            "pc {} in {} (location unavailable)",
            loc.pc,
            name_hint.unwrap_or("<?>")
        );
    };
    let name = name_hint.unwrap_or(resolved.function.as_ref());
    if let Some(source) = resolved.source {
        return format!(
            "{}:{}:{} in {}",
            source.path, source.line, source.column, name
        );
    }
    format!("pc {} in {}", loc.pc, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_instructions_count_terminal_columns() {
        let instruction = format!("\x1b[31mLoadConst\x1b[0m({})", "界".repeat(80));

        let compact = compact_instruction(&instruction);

        assert!(compact.ends_with("…\x1b[0m"));
        assert!(ansi_visible_width(&compact) <= 120);
    }
}
