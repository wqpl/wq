//! Wqdb command execution and command-specific output.

mod stop_hooks;
mod tracking;

pub(in crate::wqdb) use tracking::print_symbol_mutation;
use wqpl::style::{AnsiColor, ColorMode, TextStyle, paint};
use wqpl::value::Excerpt;
use wqpl::wqdb::{CodeLoc, DebugInfo, DebugLocalsFrame, DebugResume, StepGranularity};

use super::command::{
    COMMANDS, CommandForm, GRANULARITIES, ParsedCommand, ParsedLine, STOP_HOOK_ACTIONS,
    TRACK_ACTIONS, TRACK_SCOPES, Usage, command_usage_plain, parse_line,
};
use super::host::Host;
use super::render::{
    bold, dim, enabled_marker, format_crash_frame, format_loc_hint, header, help_row,
    render_debug_instruction, render_stop_card, render_table, stop_controls, styled_flag,
    styled_subcommand, title,
};

const CURRENT_LOCATION_UNAVAILABLE: &str = "current location unavailable";

fn wqdb_paint_with_color_mode(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    paint(text, style, color_mode)
}

fn print_wqdb_help(host: &Host<'_, '_>) {
    let color_mode = host.color_mode();
    let usage_width = COMMANDS
        .iter()
        .map(|spec| command_usage_plain(spec).len())
        .max()
        .unwrap_or(0);
    wqdb_println!(
        host,
        format!(
            "{} {}",
            title("wqdb", color_mode),
            dim("=======================================", color_mode)
        )
    );
    for spec in COMMANDS {
        wqdb_println!(host, help_row(spec, usage_width, color_mode));
    }
    wqdb_println!(host, "");
    wqdb_println!(host, bold("stepping granularity", color_mode));
    for granularity in GRANULARITIES {
        wqdb_println!(
            host,
            format!(
                "  {}  {}",
                styled_subcommand(granularity.value, color_mode),
                dim(granularity.description, color_mode)
            )
        );
    }
    wqdb_println!(host, "");
    print_command_forms(
        host,
        "symbol trackers",
        TRACK_SCOPES.iter().chain(TRACK_ACTIONS.iter().skip(1)),
    );
    wqdb_println!(host, "");
    print_command_forms(host, "stop hooks", STOP_HOOK_ACTIONS.iter());
    wqdb_println!(host, "");
    wqdb_println!(host, bold("batch commands", color_mode));
    wqdb_println!(
        host,
        format!(
            "  CLI {}{}{} commands run once at the first debugger stop.",
            styled_flag("-o", color_mode),
            dim("/", color_mode),
            styled_flag("--wqdb-cmd", color_mode)
        )
    );
    wqdb_println!(
        host,
        format!(
            "  Use {} for commands that should run every time execution stops.",
            bold(&Usage::StopHookAdd.to_string(), color_mode)
        )
    );
}

fn print_command_forms<'a>(
    host: &Host<'_, '_>,
    title: &str,
    forms: impl Iterator<Item = &'a CommandForm>,
) {
    let color_mode = host.color_mode();
    wqdb_println!(host, bold(title, color_mode));
    for form in forms {
        wqdb_println!(
            host,
            format!(
                "  {}  {}",
                bold(&form.usage.to_string(), color_mode),
                dim(form.candidate.description, color_mode)
            )
        );
    }
}

pub(in crate::wqdb) fn exec_single_wqdb_cmd(
    host: &mut Host<'_, '_>,
    cmd: &str,
) -> Option<DebugResume> {
    let command = match parse_line(cmd) {
        Ok(ParsedLine::Empty) => return None,
        Ok(ParsedLine::Command(command)) => command,
        Err(error) => {
            wqdb_println!(host, error.to_string());
            return None;
        }
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
        ParsedCommand::Track(command) => {
            if let Err(error) = tracking::execute(host, command) {
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
            if let Some(id) = arg {
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

fn set_step_granularity(host: &mut Host<'_, '_>, granularity: Option<StepGranularity>) {
    let Some(granularity) = granularity else {
        wqdb_println!(
            host,
            format!("stepping granularity: {}", host.step_granularity().as_str())
        );
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
    host: &mut Host<'_, '_>,
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

pub(in crate::wqdb) fn exec_stop_hooks(host: &mut Host<'_, '_>) -> Option<DebugResume> {
    let cmds = host.stop_hook_commands();
    exec_wqdb_cmds(host, &cmds)
}

fn set_breakpoint_at_pc(host: &mut Host<'_, '_>, pc: usize) -> Result<(), String> {
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
    host: &mut Host<'_, '_>,
    fname: &str,
    pc_opt: Option<usize>,
) -> Result<(), String> {
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

pub(in crate::wqdb) fn print_crash_locals(host: &mut Host<'_, '_>) {
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

fn print_locals(host: &mut Host<'_, '_>) {
    let Some(frame) = host.frame_locals(0) else {
        wqdb_println!(host, "no locals");
        return;
    };
    print_frame_locals(host, &frame, false);
}

fn print_frame_locals(host: &Host<'_, '_>, frame: &DebugLocalsFrame, include_header: bool) {
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

pub(in crate::wqdb) fn print_stop_card(host: &Host<'_, '_>) {
    wqdb_println!(host, render_stop_card(host, host.color_mode()));
}

pub(in crate::wqdb) fn print_stop_controls(host: &Host<'_, '_>, granularity: StepGranularity) {
    wqdb_println!(host, stop_controls(granularity, host.color_mode()));
}

fn peek_context(host: &mut Host<'_, '_>, n: usize) {
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

fn peek_instructions(host: &mut Host<'_, '_>, n: usize) {
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

    wqdb_println!(host, header("INST", host.color_mode()));

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
mod tests;
