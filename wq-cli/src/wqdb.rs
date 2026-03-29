#![cfg(not(target_arch = "wasm32"))]

use colored::Colorize;
use wqpl::session::Session;
use wqpl::session::stdio::{
    WqStdinError, wqstderr_print, wqstderr_println, wqstdin_readline, wqstdin_with_highlight_off,
};
use wqpl::value::Excerpt;
use wqpl::vm::Vm;
use wqpl::wqdb::data::{CodeLoc, DebugInfo, DebugLocalsFrame};
use wqpl::wqdb::format_frame;

/// Enter wqdb shell after a crash for inspection.
/// Print a short notice, then reuse the interactive shell.
pub fn enter_wqdb_after_err(s: &mut Session) {
    let host = s.vm_mut();
    wqstderr_println(format!(
        "{}: {}",
        "wqdb".bold().bright_magenta(),
        "error occurred".red(),
    ));
    print_crash_locals(host);
    wqdb_shell(host);
}

fn exec_single_wqdb_cmd(host: &mut Vm, cmd: &str) -> bool {
    let mut it = cmd.split_whitespace();
    match it.next().unwrap_or("") {
        "c" | "continue" => {
            host.dbg_continue();
            true
        }
        "s" | "step" => {
            host.dbg_step_in();
            true
        }
        "n" | "next" | "over" => {
            host.dbg_step_over();
            true
        }
        "fin" | "finish" | "out" => {
            host.dbg_step_out();
            true
        }
        "bf" => {
            set_breakpoint_at_function(host, it.next(), it.next()).unwrap_or_else(wqstderr_println);
            false
        }
        "b" => {
            set_breakpoint_at_pc(host, it.next()).unwrap_or_else(wqstderr_println);
            false
        }
        "ib" => {
            let bps = host.dbg_breakpoints();
            if bps.is_empty() {
                wqstderr_println("no breakpoints");
            } else {
                wqstderr_println(format!(
                    "{:<4}  {:<3}  {:<30}  {}",
                    "id", "en", "location", "function"
                ));
                wqstderr_println(format!("{:-<4}  {:-<3}  {:-<30}  {:-<20}", "", "", "", ""));
            }
            for (id, en, b) in bps {
                let meta = host.debug_info().chunk(b.chunk);
                let en_str = if en { "y" } else { "n" };
                wqstderr_println(format!(
                    "{:<4}  {:<3}  {:<30}  {}",
                    id,
                    en_str,
                    format_breakpoint_loc(host.debug_info(), b),
                    meta.name
                ));
            }
            false
        }
        "bt" => {
            let frames = host.bt_frames();
            let di = host.debug_info();
            for (idx, (loc, name)) in frames.iter().enumerate() {
                let is_current = idx == 0;
                wqstderr_print(format_frame(di, *loc, name, is_current));
            }
            false
        }
        "rs" => {
            if let Some(arg) = it.next() {
                if let Ok(id) = arg.parse::<usize>() {
                    if let Some(new_state) = host.dbg_toggle_break_id(id) {
                        wqstderr_println(format!(
                            "breakpoint {id} -> {}",
                            if new_state { "enabled" } else { "disabled" }
                        ));
                    } else {
                        let here = host.loc();
                        let file_id = host.debug_info().chunk(here.chunk).file_id;
                        let locs = host.debug_info().resolve_line(file_id, id);
                        if locs.is_empty() {
                            wqstderr_println(format!(
                                "no statement at line {id}, nor a valid breakpoint id"
                            ));
                        } else {
                            let mut enabled_count = 0;
                            let mut disabled_count = 0;
                            for l in locs {
                                if host.dbg_toggle_break_loc(l) {
                                    enabled_count += 1;
                                } else {
                                    disabled_count += 1;
                                }
                            }
                            wqstderr_println(format!(
                                "toggled {enabled_count} on, {disabled_count} off at line {id}"
                            ));
                        }
                    }
                } else {
                    wqstderr_println("invalid breakpoint id or line number");
                }
            } else {
                let new_state = host.dbg_toggle_break_all();
                wqstderr_println(format!(
                    "all breakpoints -> {}",
                    if new_state { "enabled" } else { "disabled" }
                ));
            }
            false
        }
        "p" | "peek" => {
            let n = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(3);
            peek_context(host, n);
            false
        }
        "i" | "ins" => {
            let n = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(5);
            peek_instructions(host, n);
            false
        }
        "lb" | "locals" => {
            print_locals(host);
            false
        }
        "gb" | "globals" => {
            let globals = host.dbg_globals();
            if globals.is_empty() {
                wqstderr_println("no globals");
                return false;
            }
            let mut name_w = "name".len();
            let mut value_w = "value".len();
            let mut type_w = "type".len();
            for (name, v) in &globals {
                name_w = name_w.max(name.len());
                value_w = value_w.max(v.to_string().len());
                type_w = type_w.max(v.type_name().len());
            }
            wqstderr_println(format!(
                "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                "name",
                "value",
                "type",
                name_w = name_w,
                value_w = value_w,
                type_w = type_w
            ));
            wqstderr_println(format!(
                "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                "",
                "",
                "",
                name_w = name_w,
                value_w = value_w,
                type_w = type_w
            ));
            for (name, v) in &globals {
                wqstderr_println(format!(
                    "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                    name,
                    v.excerpt(),
                    v.type_name(),
                    name_w = name_w,
                    value_w = value_w,
                    type_w = type_w
                ));
            }
            false
        }
        "h" | "help" => {
            wqstderr_println(include_str!("../../d/wqdb"));
            false
        }
        other => {
            wqstderr_println(format!("unknown wqdb command '{other}', type 'h' for help").as_str());
            false
        }
    }
}

pub fn wqdb_shell(host: &mut Vm) {
    if !host.wqdb.batch_cmds.is_empty() {
        let cmds = host.wqdb.batch_cmds.clone();
        let mut should_exit = false;
        for cmd in &cmds {
            let trimmed = cmd.trim();
            if trimmed.is_empty() {
                continue;
            }
            should_exit = exec_single_wqdb_cmd(host, trimmed);
            if should_exit {
                break;
            }
        }
        if !should_exit {
            host.dbg_continue();
        }
        return;
    }

    let mut dbg_line = 1usize;
    print_current_context(host);
    print_navigation_hint(host);
    peek_instructions(host, 1);
    loop {
        #[cfg(not(target_os = "windows"))]
        let prompt = format!(
            "{}[{}] ",
            "wqdb".bold().bright_magenta(),
            dbg_line.to_string().bright_blue()
        );
        #[cfg(target_os = "windows")]
        let prompt = format!("wqdb[{dbg_line}] ");

        let res = wqstdin_with_highlight_off(|| wqstdin_readline(&prompt));
        match res {
            Ok(line) => {
                dbg_line += 1;
                let s = line.trim();
                if s.is_empty() {
                    continue;
                }
                if exec_single_wqdb_cmd(host, s) {
                    break;
                }
            }
            Err(WqStdinError::Interrupted) => continue,
            Err(_) => {
                host.dbg_continue();
                break;
            }
        }
    }
}

fn set_breakpoint_at_pc(host: &mut Vm, pc_arg: Option<&str>) -> Result<(), &'static str> {
    let Some(pc_arg) = pc_arg else {
        return Err("usage: b <pc>");
    };
    let Ok(pc) = pc_arg.parse::<usize>() else {
        return Err("usage: b <pc>");
    };
    let loc = host.loc();
    let (len, name) = {
        let meta = host.debug_info().chunk(loc.chunk);
        (meta.len, meta.name.to_string())
    };
    if pc >= len {
        return Err("pc out of range for current chunk");
    }
    host.dbg_set_break(CodeLoc {
        chunk: loc.chunk,
        pc,
    });
    wqstderr_println(format!("breakpoint set at {name} pc={pc}"));
    Ok(())
}

fn set_breakpoint_at_function(
    host: &mut Vm,
    name_arg: Option<&str>,
    pc_arg: Option<&str>,
) -> Result<(), String> {
    let Some(fname) = name_arg else {
        return Err("usage: bf <func_name> [pc]".to_string());
    };
    let pc_opt = match pc_arg {
        Some(arg) => Some(
            arg.parse::<usize>()
                .map_err(|_| "usage: bf <func_name> [pc]".to_string())?,
        ),
        None => None,
    };
    let Some(&chunk) = host.debug_info().by_name.get(fname) else {
        return Err(format!("function '{fname}' not found"));
    };
    let meta = host.debug_info().chunk(chunk);
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
    host.dbg_set_break(CodeLoc { chunk, pc });
    wqstderr_println(format!("breakpoint set at {fname} pc={pc}"));
    Ok(())
}

fn format_breakpoint_loc(di: &DebugInfo, loc: CodeLoc) -> String {
    let meta = di.chunk(loc.chunk);
    let span = meta.line_table.context_span_at(loc.pc);
    if span.file_id != u32::MAX
        && let Some(sf) = di.file(span.file_id)
    {
        let (line, col) = sf.line_col(span.start as usize);
        return format!("pc {} ({}:{}:{})", loc.pc, sf.path, line, col);
    }
    format!("pc {}", loc.pc)
}

fn print_crash_locals(host: &mut Vm) {
    let frames = host.dbg_local_frames();
    if frames.is_empty() {
        return;
    }
    wqstderr_println("locals before crash:");
    for (idx, frame) in frames.iter().enumerate() {
        if idx > 0 {
            wqstderr_println("");
        }
        print_frame_locals(host, frame, true);
    }
}

fn print_locals(host: &mut Vm) {
    let locals = host.dbg_locals();
    if locals.is_empty() {
        wqstderr_println("no locals");
        return;
    }
    let frame = DebugLocalsFrame {
        loc: host.loc(),
        name: std::sync::Arc::from(host.debug_info().chunk(host.loc().chunk).name.as_ref()),
        locals,
    };
    print_frame_locals(host, &frame, false);
}

fn print_frame_locals(host: &Vm, frame: &DebugLocalsFrame, include_header: bool) {
    let di = host.debug_info();
    if include_header {
        wqstderr_println(format_loc_hint(di, frame.loc, Some(frame.name.as_ref())));
    }
    if frame.locals.is_empty() {
        wqstderr_println("no locals");
        return;
    }
    let meta = di.chunk(frame.loc.chunk);
    let mut rows: Vec<(String, String, &str)> = Vec::new();
    match &meta.local_names {
        Some(names) => {
            for (i, v) in &frame.locals {
                let name = names
                    .get(*i)
                    .cloned()
                    .unwrap_or_else(|| format!("loc[{i}]"));
                rows.push((name, v.excerpt(), v.type_name()));
            }
        }
        None => {
            for (i, v) in &frame.locals {
                rows.push((format!("loc[{i}]"), v.excerpt(), v.type_name()));
            }
        }
    }
    let mut name_w = "name".len();
    let mut value_w = "value".len();
    let mut type_w = "type".len();
    for (name, value, ty) in &rows {
        name_w = name_w.max(name.len());
        value_w = value_w.max(value.len());
        type_w = type_w.max(ty.len());
    }
    wqstderr_println(format!(
        "{:<name_w$}  {:<value_w$}  {:<type_w$}",
        "name",
        "value",
        "type",
        name_w = name_w,
        value_w = value_w,
        type_w = type_w
    ));
    wqstderr_println(format!(
        "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
        "",
        "",
        "",
        name_w = name_w,
        value_w = value_w,
        type_w = type_w
    ));
    for (name, value, ty) in rows {
        wqstderr_println(format!(
            "{:<name_w$}  {:<value_w$}  {:<type_w$}",
            name,
            value,
            ty,
            name_w = name_w,
            value_w = value_w,
            type_w = type_w
        ));
    }
}

fn print_navigation_hint(host: &Vm) {
    let di = host.debug_info();
    let hints = host.dbg_step_hints();
    if let Some(prev) = hints.previous {
        wqstderr_println(format!("previously:  {}", format_loc_hint(di, prev, None)));
    }
    if let Some(step) = hints.step {
        wqstderr_println(format!("step in:     {}", format_loc_hint(di, step, None)));
    }
    if let Some(next) = hints.next {
        wqstderr_println(format!("over (next): {}", format_loc_hint(di, next, None)));
    }
    if let Some(finish) = hints.finish {
        wqstderr_println(format!(
            "finish to:   {}",
            format_loc_hint(di, finish, None)
        ));
    }
}

fn format_loc_hint(di: &DebugInfo, loc: CodeLoc, name_hint: Option<&str>) -> String {
    let meta = di.chunk(loc.chunk);
    let span = meta.line_table.context_span_at(loc.pc);
    if span.file_id != u32::MAX
        && let Some(sf) = di.file(span.file_id)
    {
        let (line, col) = sf.line_col(span.start as usize);
        let name = name_hint.unwrap_or(meta.name.as_ref());
        return format!("{}:{}:{} in {}", sf.path, line, col, name);
    }
    format!(
        "pc {} in {}",
        loc.pc,
        name_hint.unwrap_or(meta.name.as_ref())
    )
}

fn print_current_context(host: &mut Vm) {
    let di = host.debug_info();
    let loc = host.loc();
    let name = di.chunk(loc.chunk).name.to_string();
    // If current PC does not have a mapped span yet (e.g., pc 0),
    // Try to present the next statement span to show a useful context.
    let meta = di.chunk(loc.chunk);
    let span_here = meta.line_table.context_span_at(loc.pc);
    if span_here.file_id != u32::MAX {
        wqstderr_print(format_frame(di, loc, &name, true));
        return;
    }
    // Find next statement at or after current pc
    let mut next_loc = None;
    for pc in loc.pc..meta.len {
        if meta.line_table.is_stmt(pc) {
            next_loc = Some(CodeLoc {
                chunk: loc.chunk,
                pc,
            });
            break;
        }
    }
    if let Some(nl) = next_loc {
        wqstderr_print(format_frame(di, nl, &name, true));
    } else {
        // Fallback to previous behavior
        wqstderr_print(format_frame(di, loc, &name, true));
    }
}

fn peek_context(host: &mut Vm, n: usize) {
    let di = host.debug_info();
    let loc = host.loc();
    let meta = di.chunk(loc.chunk);
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
        let (l, _) = sf.line_col(span.start as usize);
        // Clamp 1-based line numbers within [1, total]
        let total = sf.line_starts.len();
        let lo_ln = if l > n { l - n } else { 1 };
        let hi_ln = if l + n <= total { l + n } else { total };
        for ln in lo_ln..=hi_ln {
            if ln == l {
                wqstderr_println(
                    format!("{:>4} -> {}", ln, sf.line_snippet(ln).trim())
                        .green()
                        .bold()
                        .to_string(),
                );
            } else {
                wqstderr_println(format!("{:>4}    {}", ln, sf.line_snippet(ln).trim()));
            }
        }
    } else {
        wqstderr_println("no source available");
    }
}

fn peek_instructions(host: &mut Vm, n: usize) {
    let di = host.debug_info();
    let loc = host.loc();
    let meta = di.chunk(loc.chunk);
    let len = meta.len;
    if len == 0 {
        wqstderr_println("no instructions");
        return;
    }

    wqstderr_println("INST".bold().underline().to_string());

    let start = loc.pc.saturating_sub(n);
    let end = (loc.pc + n).min(len.saturating_sub(1));
    for pc in start..=end {
        let text = host
            .dbg_ins_at(pc)
            .unwrap_or_else(|| "<unavailable>".to_string());
        if pc == loc.pc {
            wqstderr_println(format!("{pc:>4} -> {text}").green().bold().to_string());
        } else {
            wqstderr_println(format!("{pc:>4}    {text}"));
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use std::sync::Arc;

//     use wqpl::{
//         value::Value,
//         wqdb::model::{CodeLoc, DebugInfo, DebugLocalsFrame, DebugStepHints,
// Span},     };

//     use super::{format_breakpoint_loc, set_breakpoint_at_function,
// set_breakpoint_at_pc};

//     struct TestHost {
//         di: DebugInfo,
//         loc: CodeLoc,
//         breaks: Vec<CodeLoc>,
//     }

//     impl TestHost {
//         fn new() -> Self {
//             let mut di = DebugInfo::default();
//             let file_id = di.new_file("<test>", "a\nb\nc\n");
//             let chunk = di.new_chunk("<repl>", file_id, 8);
//             {
//                 let table = &mut di.chunk_mut(chunk).line_table;
//                 table.set_exact_span(
//                     5,
//                     Span {
//                         file_id,
//                         start: 2,
//                         end: 3,
//                     },
//                 );
//             }
//             Self {
//                 di,
//                 loc: CodeLoc { chunk, pc: 2 },
//                 breaks: Vec::new(),
//             }
//         }
//     }

//     #[test]
//     fn b_command_sets_breakpoint_at_exact_pc_in_current_chunk() {
//         let mut host = TestHost::new();

//         set_breakpoint_at_pc(&mut host, Some("5")).expect("pc breakpoint
// should be accepted");

//         assert_eq!(
//             host.breaks,
//             vec![CodeLoc {
//                 chunk: host.loc.chunk,
//                 pc: 5
//             }]
//         );
//     }

//     #[test]
//     fn b_command_rejects_out_of_range_pc() {
//         let mut host = TestHost::new();

//         let err = set_breakpoint_at_pc(&mut host, Some("8")).expect_err("pc
// should be rejected");

//         assert_eq!(err, "pc out of range for current chunk");
//         assert!(host.breaks.is_empty());
//     }

//     #[test]
//     fn breakpoint_locations_include_exact_pc_and_source_hint() {
//         let host = TestHost::new();

//         assert_eq!(
//             format_breakpoint_loc(
//                 &host.di,
//                 CodeLoc {
//                     chunk: host.loc.chunk,
//                     pc: 5
//                 }
//             ),
//             "pc 5 (<test>:2:1)"
//         );
//     }

//     #[test]
//     fn bf_command_sets_breakpoint_at_exact_pc_in_named_function() {
//         let mut host = TestHost::new();
//         let chunk = host.loc.chunk;
//         host.di.by_name.insert(Arc::from("demo"), chunk);

//         set_breakpoint_at_function(&mut host, Some("demo"), Some("5"))
//             .expect("function breakpoint should be accepted");

//         assert_eq!(host.breaks, vec![CodeLoc { chunk, pc: 5 }]);
//     }

//     #[test]
//     fn bf_command_rejects_out_of_range_pc() {
//         let mut host = TestHost::new();
//         let chunk = host.loc.chunk;
//         host.di.by_name.insert(Arc::from("demo"), chunk);

//         let err = set_breakpoint_at_function(&mut host, Some("demo"),
// Some("8"))             .expect_err("pc should be rejected");

//         assert_eq!(err, "pc out of range for function 'demo'");
//         assert!(host.breaks.is_empty());
//     }

//     #[test]
//     fn bf_command_rejects_invalid_pc_text() {
//         let mut host = TestHost::new();
//         let chunk = host.loc.chunk;
//         host.di.by_name.insert(Arc::from("demo"), chunk);

//         let err = set_breakpoint_at_function(&mut host, Some("demo"),
// Some("wat"))             .expect_err("invalid pc text should be rejected");

//         assert_eq!(err, "usage: bf <func_name> [pc]");
//         assert!(host.breaks.is_empty());
//     }
// }
