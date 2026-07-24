//! Wqdb command execution and command-specific output.

mod stop_hooks;
mod tracking;

pub(in crate::wqdb) use tracking::print_symbol_mutation;
use tracking::{print_symbol_trackers, track_symbol, untrack_symbol};
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::value::Excerpt;
#[cfg(test)]
use wqpl::wqdb::Span;
use wqpl::wqdb::{CodeLoc, DebugInfo, DebugLocalsFrame, DebugResume, StepGranularity};

use super::command::{
    COMMANDS as WQDB_COMMANDS, Command, GRANULARITIES, ParsedCommand, ParsedLine, Usage,
    command_usage_plain, parse_line, usage_error,
};
use super::host::Host as WqdbHost;
#[cfg(test)]
use super::render::{
    ansi_visible_width as ansi_visible_len, compact_instruction,
    format_expr_stop_card as format_expr_stop_card_with_color_mode,
    format_inst_stop_card as format_inst_stop_card_with_color_mode,
    format_line_stop_card as format_line_stop_card_with_color_mode,
    prompt as wqdb_prompt_with_color_mode, resolved_stop_span,
    unavailable_stop_card as unavailable_stop_card_with_color_mode,
};
use super::render::{
    bold as wqdb_bold, dim as wqdb_dim, enabled_marker, format_crash_frame, format_loc_hint,
    header as wqdb_header, help_row as wqdb_help_row, render_debug_instruction,
    render_stop_card as render_stop_card_with_color_mode, render_table, stop_controls,
    styled_command as styled_command_with_color_mode, styled_flag, styled_required_arg,
    styled_separator, styled_stop_hook_command, styled_subcommand, styled_track_command,
    title as wqdb_title,
};

const CURRENT_LOCATION_UNAVAILABLE: &str = "current location unavailable";

fn wqdb_paint_with_color_mode(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    paint(text, style, color_mode)
}

fn print_wqdb_help(host: &WqdbHost<'_, '_>) {
    let color_mode = host.color_mode();
    let usage_width = WQDB_COMMANDS
        .iter()
        .map(|spec| command_usage_plain(spec).len())
        .max()
        .unwrap_or(0);
    wqdb_println!(
        host,
        format!(
            "{} {}",
            wqdb_title("wqdb", color_mode),
            wqdb_dim("=======================================", color_mode)
        )
    );
    for spec in WQDB_COMMANDS {
        wqdb_println!(host, wqdb_help_row(spec, usage_width, color_mode));
    }
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("stepping granularity", color_mode));
    for granularity in GRANULARITIES {
        wqdb_println!(
            host,
            format!(
                "  {}  {}",
                styled_subcommand(granularity.value, color_mode),
                wqdb_dim(granularity.description, color_mode)
            )
        );
    }
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("track scopes", color_mode));
    let track_name = format!(
        "{} {}",
        styled_command_with_color_mode("track", color_mode),
        styled_required_arg("name", color_mode)
    );
    wqdb_println!(
        host,
        format!(
            "  {} {}",
            track_name,
            wqdb_dim(
                "resolves a current local by name, or a global if no local matches",
                color_mode,
            )
        )
    );
    wqdb_println!(
        host,
        format!(
            "  {} {} {} {} {}",
            styled_track_command("global", "name", color_mode),
            styled_separator(color_mode),
            styled_track_command("local", "name", color_mode),
            styled_separator(color_mode),
            styled_track_command("capture", "slot", color_mode)
        )
    );
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("stop hooks", color_mode));
    wqdb_println!(
        host,
        format!(
            "  {} {} {} {} {} {} {}",
            styled_stop_hook_command(
                "add",
                Some(format!(
                    "{} {}",
                    styled_flag("-o", color_mode),
                    styled_required_arg("cmd", color_mode)
                )),
                color_mode,
            ),
            styled_separator(color_mode),
            styled_stop_hook_command("list", None, color_mode),
            styled_separator(color_mode),
            styled_stop_hook_command(
                "delete",
                Some(styled_required_arg("id|all", color_mode)),
                color_mode,
            ),
            styled_separator(color_mode),
            styled_stop_hook_command("clear", None, color_mode)
        )
    );
    wqdb_println!(host, "");
    wqdb_println!(host, wqdb_bold("batch commands", color_mode));
    wqdb_println!(
        host,
        format!(
            "  CLI {}{}{} commands run once at the first debugger stop.",
            styled_flag("-o", color_mode),
            wqdb_dim("/", color_mode),
            styled_flag("--wqdb-cmd", color_mode)
        )
    );
    wqdb_println!(
        host,
        format!(
            "  Use {} for commands that should run every time execution stops.",
            styled_stop_hook_command(
                "add",
                Some(format!(
                    "{} {}",
                    styled_flag("-o", color_mode),
                    styled_required_arg("cmd", color_mode)
                )),
                color_mode,
            )
        )
    );
}

pub(in crate::wqdb) fn exec_single_wqdb_cmd(
    host: &mut WqdbHost<'_, '_>,
    cmd: &str,
) -> Option<DebugResume> {
    let command = match parse_line(cmd) {
        ParsedLine::Empty => return None,
        ParsedLine::Unknown(name) => {
            wqdb_println!(
                host,
                format!("unknown wqdb command '{name}', type 'h' for help")
            );
            return None;
        }
        ParsedLine::Command(command) => command,
    };
    match command {
        ParsedCommand::Continue => Some(DebugResume::Continue),
        ParsedCommand::StepIn => Some(DebugResume::StepIn),
        ParsedCommand::StepOver => Some(DebugResume::StepOver),
        ParsedCommand::Finish => Some(DebugResume::StepOut),
        ParsedCommand::Granularity(arg) => {
            set_step_granularity(host, arg);
            None
        }
        ParsedCommand::BreakFunction { name, pc } => {
            if let Err(error) = set_breakpoint_at_function(host, name, pc) {
                wqdb_println!(host, error);
            }
            None
        }
        ParsedCommand::BreakPc(pc) => {
            if let Err(error) = set_breakpoint_at_pc(host, pc) {
                wqdb_println!(host, error);
            }
            None
        }
        ParsedCommand::Track { target, name } => {
            if let Err(error) = track_symbol(host, target, name) {
                wqdb_println!(host, error);
            }
            None
        }
        ParsedCommand::Tracks => {
            print_symbol_trackers(host);
            None
        }
        ParsedCommand::Untrack(arg) => {
            if let Err(error) = untrack_symbol(host, arg) {
                wqdb_println!(host, error);
            }
            None
        }
        ParsedCommand::StopHook(command) => {
            if let Err(error) = stop_hooks::execute(host, command) {
                wqdb_println!(host, error);
            }
            None
        }
        ParsedCommand::Breakpoints => {
            let bps = host.breakpoints();
            if bps.is_empty() {
                wqdb_println!(host, "no breakpoints");
            } else {
                let rows = bps
                    .iter()
                    .map(|(id, enabled, breakpoint)| {
                        let function = host
                            .debug_info()
                            .get_chunk(breakpoint.chunk)
                            .map_or("<?>", |meta| meta.name.as_ref());
                        vec![
                            id.to_string(),
                            enabled_marker(*enabled).to_string(),
                            format_breakpoint_loc(host.debug_info(), *breakpoint),
                            function.to_string(),
                        ]
                    })
                    .collect::<Vec<_>>();
                wqdb_println!(
                    host,
                    render_table(
                        &["id", "en", "location", "function"],
                        &rows,
                        &[4, 3, 30, 20],
                    )
                );
            }
            None
        }
        ParsedCommand::Backtrace => {
            let frames = host.backtrace();
            if frames.is_empty() {
                wqdb_println!(host, "no backtrace");
            }
            for (idx, frame) in frames.iter().enumerate() {
                wqdb_println!(host, format_crash_frame(frame, idx == 0, host.color_mode()));
            }
            None
        }
        ParsedCommand::ResetBreakpoints(arg) => {
            if let Some(arg) = arg {
                if let Ok(id) = arg.parse::<usize>() {
                    if let Some(new_state) = host.toggle_breakpoint_by_id(id) {
                        wqdb_println!(
                            host,
                            format!(
                                "breakpoint {id} -> {}",
                                if new_state { "enabled" } else { "disabled" }
                            )
                        );
                    } else {
                        let Some(file_id) = host
                            .location()
                            .and_then(|here| host.debug_info().get_chunk(here.chunk))
                            .map(|metadata| metadata.file_id)
                        else {
                            wqdb_println!(
                                host,
                                format!(
                                    "breakpoint {id} not found; current location unavailable for line lookup"
                                )
                            );
                            return None;
                        };
                        let locs = host.debug_info().resolve_line(file_id, id);
                        if locs.is_empty() {
                            wqdb_println!(
                                host,
                                format!("no statement at line {id}, nor a valid breakpoint id")
                            );
                        } else {
                            let mut enabled_count = 0;
                            let mut disabled_count = 0;
                            for l in locs {
                                if host.toggle_breakpoint_at(l) {
                                    enabled_count += 1;
                                } else {
                                    disabled_count += 1;
                                }
                            }
                            wqdb_println!(
                                host,
                                format!(
                                    "toggled {enabled_count} on, {disabled_count} off at line {id}"
                                )
                            );
                        }
                    }
                } else {
                    wqdb_println!(host, "invalid breakpoint id or line number");
                }
            } else {
                let new_state = host.toggle_all_breakpoints();
                wqdb_println!(
                    host,
                    format!(
                        "all breakpoints -> {}",
                        if new_state { "enabled" } else { "disabled" }
                    )
                );
            }
            None
        }
        ParsedCommand::Peek(n) => {
            peek_context(host, n);
            None
        }
        ParsedCommand::Instructions(n) => {
            peek_instructions(host, n);
            None
        }
        ParsedCommand::Locals => {
            print_locals(host);
            None
        }
        ParsedCommand::Globals => {
            let globals = host.globals();
            if globals.is_empty() {
                wqdb_println!(host, "no globals");
                return None;
            }
            let rows = globals
                .iter()
                .map(|(name, value)| {
                    vec![
                        name.clone(),
                        value.excerpt(),
                        value.debug_kind().to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            wqdb_println!(host, render_table(&["name", "value", "kind"], &rows, &[]));
            None
        }
        ParsedCommand::Help => {
            print_wqdb_help(host);
            None
        }
    }
}

fn set_step_granularity(host: &mut WqdbHost<'_, '_>, arg: Option<&str>) {
    let Some(arg) = arg else {
        wqdb_println!(
            host,
            format!("stepping granularity: {}", host.step_granularity().as_str())
        );
        return;
    };
    let Some(granularity) = StepGranularity::parse(arg) else {
        wqdb_println!(host, usage_error(Usage::Command(Command::Granularity)));
        return;
    };
    host.set_step_granularity(granularity);
    wqdb_println!(
        host,
        format!("stepping granularity -> {}", granularity.as_str())
    );
    print_stop_card(host);
    print_stop_controls(host, granularity);
}

pub(in crate::wqdb) fn exec_wqdb_cmds(
    host: &mut WqdbHost<'_, '_>,
    cmds: &[String],
) -> Option<DebugResume> {
    for cmd in cmds {
        let trimmed = cmd.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(action) = exec_single_wqdb_cmd(host, trimmed) {
            return Some(action);
        }
    }
    None
}

pub(in crate::wqdb) fn exec_stop_hooks(host: &mut WqdbHost<'_, '_>) -> Option<DebugResume> {
    let cmds = host.stop_hook_commands();
    exec_wqdb_cmds(host, &cmds)
}

fn set_breakpoint_at_pc(host: &mut WqdbHost<'_, '_>, pc_arg: Option<&str>) -> Result<(), String> {
    let Some(pc_arg) = pc_arg else {
        return Err(usage_error(Usage::Command(Command::BreakPc)));
    };
    let Ok(pc) = pc_arg.parse::<usize>() else {
        return Err(usage_error(Usage::Command(Command::BreakPc)));
    };
    let Some(loc) = host.location() else {
        return Err(CURRENT_LOCATION_UNAVAILABLE.to_string());
    };
    let (len, name) = {
        let Some(meta) = host.debug_info().get_chunk(loc.chunk) else {
            return Err(CURRENT_LOCATION_UNAVAILABLE.to_string());
        };
        (meta.len, meta.name.to_string())
    };
    if pc >= len {
        return Err("pc out of range for current chunk".to_string());
    }
    host.set_breakpoint(CodeLoc {
        chunk: loc.chunk,
        pc,
    });
    wqdb_println!(host, format!("breakpoint set at {name} pc={pc}"));
    Ok(())
}

fn set_breakpoint_at_function(
    host: &mut WqdbHost<'_, '_>,
    name_arg: Option<&str>,
    pc_arg: Option<&str>,
) -> Result<(), String> {
    let Some(fname) = name_arg else {
        return Err(usage_error(Usage::Command(Command::BreakFunction)));
    };
    let pc_opt = match pc_arg {
        Some(arg) => Some(
            arg.parse::<usize>()
                .map_err(|_| usage_error(Usage::Command(Command::BreakFunction)))?,
        ),
        None => None,
    };
    let Some(chunk) = host.debug_info().function_chunk(fname) else {
        return Err(format!("function '{fname}' not found"));
    };
    let Some(meta) = host.debug_info().get_chunk(chunk) else {
        return Err(format!(
            "debug information for function '{fname}' is unavailable"
        ));
    };
    let pc = if let Some(pc) = pc_opt {
        if pc >= meta.len {
            return Err(format!("pc out of range for function '{fname}'"));
        }
        pc
    } else {
        (0..meta.len)
            .find(|&p| meta.line_table.is_stmt(p))
            .unwrap_or(0)
    };
    host.set_breakpoint(CodeLoc { chunk, pc });
    wqdb_println!(host, format!("breakpoint set at {fname} pc={pc}"));
    Ok(())
}

fn format_breakpoint_loc(di: &DebugInfo, loc: CodeLoc) -> String {
    let Some(resolved) = di.resolve_location(loc) else {
        return format!("pc {} (location unavailable)", loc.pc);
    };
    if let Some(source) = resolved.source {
        return format!(
            "pc {} ({}:{}:{})",
            loc.pc, source.path, source.line, source.column
        );
    }
    format!("pc {}", loc.pc)
}

pub(in crate::wqdb) fn print_crash_locals(host: &mut WqdbHost<'_, '_>) {
    let frames = (0..host.backtrace().len())
        .filter_map(|index| host.frame_locals(index))
        .collect::<Vec<_>>();
    if frames.is_empty() {
        return;
    }
    wqdb_println!(host, "locals before crash:");
    for (idx, frame) in frames.iter().enumerate() {
        if idx > 0 {
            wqdb_println!(host, "");
        }
        print_frame_locals(host, frame, true);
    }
}

fn print_locals(host: &mut WqdbHost<'_, '_>) {
    let Some(frame) = host.frame_locals(0) else {
        wqdb_println!(host, "no locals");
        return;
    };
    print_frame_locals(host, &frame, false);
}

fn print_frame_locals(host: &WqdbHost<'_, '_>, frame: &DebugLocalsFrame, include_header: bool) {
    let di = host.debug_info();
    if include_header {
        wqdb_println!(
            host,
            format_loc_hint(di, frame.loc, Some(frame.name.as_ref()))
        );
    }
    if frame.locals.is_empty() {
        wqdb_println!(host, "no locals");
        return;
    }
    let local_names = di
        .get_chunk(frame.loc.chunk)
        .and_then(|meta| meta.local_names.as_ref());
    let mut rows = Vec::new();
    match local_names {
        Some(names) => {
            for (i, v) in &frame.locals {
                let name = names
                    .get(*i)
                    .cloned()
                    .unwrap_or_else(|| format!("loc[{i}]"));
                rows.push(vec![name, v.excerpt(), v.debug_kind().as_str().to_string()]);
            }
        }
        None => {
            for (i, v) in &frame.locals {
                rows.push(vec![
                    format!("loc[{i}]"),
                    v.excerpt(),
                    v.debug_kind().as_str().to_string(),
                ]);
            }
        }
    }
    wqdb_println!(host, render_table(&["name", "value", "kind"], &rows, &[]));
}

pub(in crate::wqdb) fn print_stop_card(host: &WqdbHost<'_, '_>) {
    wqdb_println!(
        host,
        render_stop_card_with_color_mode(host, host.color_mode())
    );
}

pub(in crate::wqdb) fn print_stop_controls(host: &WqdbHost<'_, '_>, granularity: StepGranularity) {
    wqdb_println!(host, stop_controls(granularity, host.color_mode()));
}

fn peek_context(host: &mut WqdbHost<'_, '_>, n: usize) {
    let di = host.debug_info();
    let Some(loc) = host.location() else {
        wqdb_println!(host, CURRENT_LOCATION_UNAVAILABLE);
        return;
    };
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        wqdb_println!(host, CURRENT_LOCATION_UNAVAILABLE);
        return;
    };
    // Prefer a span for the next statement if current pc has no span yet
    let mut span = meta.line_table.context_span_at(loc.pc);
    if span.file_id == u32::MAX {
        for pc in loc.pc..meta.len {
            if meta.line_table.is_stmt(pc) {
                span = meta.line_table.context_span_at(pc);
                break;
            }
        }
    }
    if let Some(sf) = di.file(span.file_id) {
        let (l, _) = sf.line_col(span.start);
        // Clamp 1-based line numbers within [1, total]
        let total = sf.line_count();
        let lo_ln = if l > n { l - n } else { 1 };
        let hi_ln = if l + n <= total { l + n } else { total };
        for ln in lo_ln..=hi_ln {
            if ln == l {
                wqdb_println!(
                    host,
                    wqdb_paint_with_color_mode(
                        &format!("{:>4} -> {}", ln, sf.line_text(ln)),
                        TextStyle::new().fg(AnsiColor::Green).bold(),
                        host.color_mode(),
                    )
                );
            } else {
                wqdb_println!(host, format!("{:>4}    {}", ln, sf.line_text(ln)));
            }
        }
    } else {
        wqdb_println!(host, "no source available");
    }
}

fn peek_instructions(host: &mut WqdbHost<'_, '_>, n: usize) {
    let di = host.debug_info();
    let Some(loc) = host.location() else {
        wqdb_println!(host, CURRENT_LOCATION_UNAVAILABLE);
        return;
    };
    let Some(meta) = di.get_chunk(loc.chunk).filter(|meta| loc.pc < meta.len) else {
        wqdb_println!(host, CURRENT_LOCATION_UNAVAILABLE);
        return;
    };
    let len = meta.len;
    if len == 0 {
        wqdb_println!(host, "no instructions");
        return;
    }

    wqdb_println!(host, wqdb_header("INST", host.color_mode()));

    let start = loc.pc.saturating_sub(n);
    let end = (loc.pc + n).min(len.saturating_sub(1));
    for pc in start..=end {
        let text = host
            .instruction_at(pc)
            .map(|instruction| render_debug_instruction(&instruction, host.color_mode()))
            .unwrap_or_else(|| "<unavailable>".to_string());
        let prefix = if pc == loc.pc {
            wqdb_paint_with_color_mode(
                &format!("{pc:>4} -> "),
                TextStyle::new().fg(AnsiColor::Green).bold(),
                host.color_mode(),
            )
        } else {
            format!("{pc:>4}    ")
        };
        wqdb_println!(host, format!("{prefix}{text}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop_card_debug_info() -> (DebugInfo, CodeLoc) {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "first\n  total:price*qty\nlast\n");
        let chunk = di.new_chunk("calc", file_id, 5);
        assert!(di.set_statement_span(
            CodeLoc { chunk, pc: 2 },
            Span {
                file_id,
                start: 6,
                end: 23,
            },
        ));
        assert!(di.set_exact_span(
            CodeLoc { chunk, pc: 2 },
            Span {
                file_id,
                start: 14,
                end: 23,
            },
        ));
        (di, CodeLoc { chunk, pc: 2 })
    }

    #[test]
    fn command_styles_use_explicit_style_renderer() {
        assert_eq!(
            styled_command_with_color_mode("continue", ColorMode::Always),
            "\x1b[32mcontinue\x1b[0m"
        );
        assert_eq!(
            wqdb_paint_with_color_mode(
                "INST",
                TextStyle::new().bold().underline(),
                ColorMode::Never,
            ),
            "INST"
        );
    }

    #[test]
    fn command_help_rows_are_indented() {
        let usage_width = WQDB_COMMANDS
            .iter()
            .map(|spec| command_usage_plain(spec).len())
            .max()
            .expect("wqdb commands");
        let row = wqdb_help_row(&WQDB_COMMANDS[0], usage_width, ColorMode::Never);

        assert!(row.starts_with("  "));
    }

    #[test]
    fn prompt_keeps_the_active_granularity_visible() {
        assert_eq!(
            wqdb_prompt_with_color_mode(StepGranularity::Expr, 3, ColorMode::Never),
            "wqdb[expr:3] "
        );
    }

    #[test]
    fn unavailable_stop_cards_name_the_active_mode() {
        assert_eq!(
            unavailable_stop_card_with_color_mode(StepGranularity::Line, ColorMode::Never),
            "LINE  current location unavailable"
        );
        assert_eq!(
            unavailable_stop_card_with_color_mode(StepGranularity::Expr, ColorMode::Never),
            "EXPR  current location unavailable"
        );
        assert_eq!(
            unavailable_stop_card_with_color_mode(StepGranularity::Inst, ColorMode::Never),
            "INST  current location unavailable"
        );
    }

    #[test]
    fn stale_locations_render_without_panicking() {
        let di = DebugInfo::default();
        let loc = CodeLoc {
            chunk: wqpl::wqdb::ChunkId(u32::MAX),
            pc: 7,
        };

        assert_eq!(
            format_breakpoint_loc(&di, loc),
            "pc 7 (location unavailable)"
        );
        assert_eq!(
            format_loc_hint(&di, loc, Some("calc")),
            "pc 7 in calc (location unavailable)"
        );
        assert_eq!(resolved_stop_span(&di, loc), (Span::NONE, false));
    }

    #[test]
    fn line_stop_card_is_source_first_and_preserves_indentation() {
        let (di, loc) = stop_card_debug_info();

        let rendered = format_line_stop_card_with_color_mode(&di, loc, "calc", 1, ColorMode::Never);

        assert_eq!(
            rendered,
            "LINE  demo.wq:2 in calc\n   1    first\n   2 ->   total:price*qty\n   3    last"
        );
    }

    #[test]
    fn expression_stop_card_focuses_the_exact_span() {
        let (di, loc) = stop_card_debug_info();

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            Some("BinaryOp(Multiply)"),
            ColorMode::Never,
        );

        assert_eq!(
            rendered,
            "EXPR  demo.wq:2:9 in calc\n  2 ->   total:price*qty\n               ~~~~~~~~~\npc 2  BinaryOp(Multiply)"
        );
    }

    #[test]
    fn expression_stop_card_preserves_pretty_instruction_color() {
        let (di, loc) = stop_card_debug_info();
        let instruction = "\x1b[35mBinaryOp\x1b[0m(Multiply)";

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            Some(instruction),
            ColorMode::Always,
        );

        assert!(
            rendered.ends_with(&format!("\x1b[90mpc 2  \x1b[0m{instruction}")),
            "card was: {rendered:?}"
        );
    }

    #[test]
    fn instruction_stop_card_leads_with_disassembly_then_source() {
        let (di, loc) = stop_card_debug_info();
        let instructions = vec![
            (0, "LoadLocal(0)".to_string()),
            (1, "LoadLocal(1)".to_string()),
            (2, "BinaryOp(Multiply)".to_string()),
            (3, "StoreLocal(2)".to_string()),
        ];

        let rendered = format_inst_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            5,
            &instructions,
            ColorMode::Never,
        );

        assert_eq!(
            rendered,
            "INST  calc  pc 2/4\n   0    LoadLocal(0)\n   1    LoadLocal(1)\n   2 -> BinaryOp(Multiply)\n   3    StoreLocal(2)\n\nSOURCE  demo.wq:2:9\n  2 ->   total:price*qty\n               ~~~~~~~~~"
        );
    }

    #[test]
    fn instruction_stop_card_colors_prefix_without_overriding_opcode() {
        let (di, loc) = stop_card_debug_info();
        let instruction = "\x1b[35mBinaryOp\x1b[0m(Multiply)";

        let rendered = format_inst_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            5,
            &[(2, instruction.to_string())],
            ColorMode::Always,
        );

        assert!(
            rendered.contains(&format!("\x1b[1;32m   2 -> \x1b[0m{instruction}")),
            "card was: {rendered:?}"
        );
    }

    #[test]
    fn compact_instruction_preserves_complete_ansi_sequences() {
        let instruction = format!("\x1b[31mLoadConst\x1b[0m({})", "x".repeat(140));

        let compact = compact_instruction(&instruction);

        assert!(compact.starts_with("\x1b[31mLoadConst\x1b[0m("));
        assert!(compact.ends_with("…\x1b[0m"));
        assert_eq!(ansi_visible_len(&compact), 120);
    }

    #[test]
    fn expression_stop_card_clamps_a_multiline_span() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "x:(1;\n  2)\n");
        let chunk = di.new_chunk("calc", file_id, 1);
        assert!(di.set_exact_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id,
                start: 2,
                end: 10,
            },
        ));

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 0 },
            "calc",
            Some("MakeList(2)"),
            ColorMode::Never,
        );

        assert_eq!(
            rendered,
            "EXPR  demo.wq:1:3 in calc\n  1 -> x:(1;\n         ~~~\npc 0  MakeList(2)"
        );
    }

    #[test]
    fn stop_cards_keep_instruction_context_when_source_is_unavailable() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "");
        let chunk = di.new_chunk("calc", file_id, 1);
        let loc = CodeLoc { chunk, pc: 0 };

        let expr = format_expr_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            Some("Return"),
            ColorMode::Never,
        );
        let inst = format_inst_stop_card_with_color_mode(
            &di,
            loc,
            "calc",
            1,
            &[(0, "Return".to_string())],
            ColorMode::Never,
        );

        assert_eq!(
            expr,
            "EXPR  pc 0 in calc\n  source unavailable\npc 0  Return"
        );
        assert!(
            inst.ends_with("\n\nSOURCE  unavailable"),
            "card was: {inst}"
        );
    }

    #[test]
    fn expression_stop_card_reports_display_columns() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "α:1\n");
        let chunk = di.new_chunk("calc", file_id, 1);
        assert!(di.set_exact_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id,
                start: 3,
                end: 4,
            },
        ));

        let rendered = format_expr_stop_card_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 0 },
            "calc",
            None,
            ColorMode::Never,
        );

        assert!(
            rendered.starts_with("EXPR  demo.wq:1:3 in calc"),
            "card was: {rendered}"
        );
    }

    #[test]
    fn line_stop_card_omits_a_phantom_line_after_final_newline() {
        let mut di = DebugInfo::default();
        let file_id = di.new_file("demo.wq", "a:1\nb:2\n");
        let chunk = di.new_chunk("calc", file_id, 1);
        assert!(di.set_statement_span(
            CodeLoc { chunk, pc: 0 },
            Span {
                file_id,
                start: 4,
                end: 7,
            },
        ));

        let rendered = format_line_stop_card_with_color_mode(
            &di,
            CodeLoc { chunk, pc: 0 },
            "calc",
            2,
            ColorMode::Never,
        );

        assert!(!rendered.contains("\n   3"), "card was: {rendered}");
    }
}
