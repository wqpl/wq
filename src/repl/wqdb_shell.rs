use crate::{
    colored::Colorize,
    repl::stdio::{StdinError, stderr_println, stdin_readline, stdin_with_highlight_off},
    wqdb::{CodeLoc, DebugHost, format_frame},
};

/// Enter the wqdb shell after a crash to allow post-mortem inspection.
/// Prints a short notice, then reuses the interactive shell.
pub fn wqdb_shell_after_crash(host: &mut dyn DebugHost) {
    stderr_println(format!(
        "{}: {}",
        "wqdb".bold().bright_magenta(),
        "error occurred".red(),
    ));
    // Reuse the regular shell loop; stepping/continue won’t resume execution after crash
    wqdb_shell(host);
}

pub fn wqdb_shell(host: &mut dyn DebugHost) {
    let mut dbg_line = 1usize;
    // Print current context
    // stderr_println(&format!(
    //     "{}[{}] ",
    //     "wqdb".bold().bright_magenta(),
    //     dbg_line.to_string().bright_blue()
    // ));
    print_current_context(host);
    peek_instructions(host, 1);
    loop {
        let prompt = format!(
            "{}[{}] ",
            "wqdb".bold().bright_magenta(),
            dbg_line.to_string().bright_blue()
        );
        #[cfg(target_os = "windows")]
        let prompt = format!("wqdb[{dbg_line}] ");

        let res = stdin_with_highlight_off(|| stdin_readline(&prompt));
        match res {
            Ok(line) => {
                dbg_line += 1;
                let s = line.trim();
                if s.is_empty() {
                    continue;
                }
                let mut it = s.split_whitespace();
                match it.next().unwrap_or("") {
                    "c" | "continue" => {
                        host.dbg_continue();
                        break;
                    }
                    "s" | "step" => {
                        host.dbg_step_in();
                        break;
                    }
                    "n" | "next" => {
                        host.dbg_step_over();
                        break;
                    }
                    "fin" | "finish" => {
                        host.dbg_step_out();
                        break;
                    }
                    // set breakpoint by function name (first stmt) or by name + pc
                    // "bf" => {
                    //     let name = it.next();
                    //     let pc_opt = it.next().and_then(|x| x.parse::<usize>().ok());
                    //     if let Some(fname) = name {
                    //         if let Some(&chunk) = host.di().by_name.get(fname) {
                    //             let meta = host.di().chunk(chunk);
                    //             let pc = if let Some(pc) = pc_opt {
                    //                 pc
                    //             } else {
                    //                 (0..meta.len)
                    //                     .find(|&p| meta.line_table.is_stmt(p))
                    //                     .unwrap_or(0)
                    //             };
                    //             host.dbg_set_break(crate::debug::CodeLoc { chunk, pc });
                    //             stderr_println(&format!("breakpoint set at {fname} pc={pc}"));
                    //         } else {
                    //             stderr_println(&format!("function '{fname}' not found"));
                    //         }
                    //     } else {
                    //         stderr_println("usage: bf <func_name> [pc]");
                    //     }
                    // }
                    "b" => {
                        if let Some(n) = it.next() {
                            if let Ok(ln) = n.parse::<usize>() {
                                let here = host.loc();
                                let file_id = host.di().chunk(here.chunk).file_id;
                                let locs = host.di().resolve_line(file_id, ln);
                                if locs.is_empty() {
                                    stderr_println(format!("no statement at line {ln}"));
                                } else {
                                    for l in &locs {
                                        host.dbg_set_break(*l);
                                    }
                                    stderr_println(format!(
                                        "set {} breakpoint(s) at line {ln}",
                                        locs.len()
                                    ));
                                }
                            } else {
                                stderr_println("usage: b <line>");
                            }
                        } else {
                            stderr_println("usage: b <line>");
                        }
                    }
                    "ib" => {
                        let bps = host.dbg_breakpoints();
                        if bps.is_empty() {
                            stderr_println("no breakpoints");
                        }
                        for b in bps {
                            let meta = host.di().chunk(b.chunk);
                            let span = meta.line_table.span_at(b.pc);
                            if let Some(sf) = host.di().file(span.file_id) {
                                let (l, _) = sf.line_col(span.start as usize);
                                stderr_println(format!(
                                    "{}:{} (pc {} in {})",
                                    sf.path, l, b.pc, meta.name
                                ));
                            } else {
                                stderr_println(format!(
                                    "chunk {:?} pc {} ({})",
                                    meta.id, b.pc, meta.name
                                ));
                            }
                        }
                    }
                    // save/load breakpoints to a simple text file: each line "<name> <pc>"
                    // "bp_save" => {
                    //     if let Some(path) = it.next() {
                    //         use std::io::Write;
                    //         match std::fs::File::create(path) {
                    //             Ok(mut f) => {
                    //                 for b in host.dbg_breakpoints() {
                    //                     let meta = host.di().chunk(b.chunk);
                    //                     let _ = writeln!(f, "{} {}", meta.name, b.pc);
                    //                 }
                    //                 stderr_println(&format!("breakpoints saved to {path}"));
                    //             }
                    //             Err(e) => stderr_println(&format!("cannot save breakpoints: {e}")),
                    //         }
                    //     } else {
                    //         stderr_println("usage: bp_save <path>");
                    //     }
                    // }
                    // "bp_load" => {
                    //     if let Some(path) = it.next() {
                    //         match std::fs::read_to_string(path) {
                    //             Ok(content) => {
                    //                 let mut count = 0usize;
                    //                 for (i, line) in content.lines().enumerate() {
                    //                     let mut parts = line.split_whitespace();
                    //                     let Some(name) = parts.next() else { continue };
                    //                     let pc = parts
                    //                         .next()
                    //                         .and_then(|x| x.parse::<usize>().ok())
                    //                         .unwrap_or(0);
                    //                     if let Some(&chunk) = host.di().by_name.get(name) {
                    //                         host.dbg_set_break(crate::debug::CodeLoc { chunk, pc });
                    //                         count += 1;
                    //                     } else {
                    //                         let _ = i; // ignore unknown functions
                    //                     }
                    //                 }
                    //                 stderr_println(&format!("loaded {count} breakpoints"));
                    //             }
                    //             Err(e) => stderr_println(&format!("cannot load breakpoints: {e}")),
                    //         }
                    //     } else {
                    //         stderr_println("usage: bp_load <path>");
                    //     }
                    // }
                    "bt" => {
                        // print a simple backtrace from the current paused state
                        let frames = host.bt_frames();
                        let di = host.di();
                        let count = frames.len();
                        for (idx, (loc, name)) in frames.iter().enumerate() {
                            let is_last = idx + 1 == count;
                            eprint!("{}", format_frame(di, *loc, name, is_last));
                        }
                    }
                    "rs" => {
                        host.dbg_reset_breaks();
                        stderr_println("breakpoints cleared");
                    }
                    "p" | "peek" => {
                        let n = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(3);
                        peek_context(host, n);
                    }
                    "i" | "ins" => {
                        let n = it.next().and_then(|x| x.parse::<usize>().ok()).unwrap_or(5);
                        peek_instructions(host, n);
                    }
                    "lb" | "locals" => {
                        // Print current frame locals using debug names when available
                        let locals = host.dbg_locals();
                        if locals.is_empty() {
                            stderr_println("no locals");
                            continue;
                        }

                        let di = host.di();
                        let loc = host.loc();
                        let meta = di.chunk(loc.chunk);

                        let mut rows: Vec<(String, String, &str)> = Vec::new();

                        match &meta.local_names {
                            Some(names) => {
                                for (i, v) in locals {
                                    let name = names
                                        .get(i)
                                        .cloned()
                                        .unwrap_or_else(|| format!("loc[{i}]"));
                                    rows.push((name, v.to_string(), v.type_name()));
                                }
                            }
                            None => {
                                for (i, v) in locals {
                                    rows.push((format!("loc[{i}]"), v.to_string(), v.type_name()));
                                }
                            }
                        }

                        // Measure column widths
                        let mut name_w = "name".len();
                        let mut value_w = "value".len();
                        let mut type_w = "type".len();

                        for (name, value, ty) in &rows {
                            name_w = name_w.max(name.len());
                            value_w = value_w.max(value.len());
                            type_w = type_w.max(ty.len());
                        }

                        // Print header and rule
                        stderr_println(format!(
                            "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                            "name",
                            "value",
                            "type",
                            name_w = name_w,
                            value_w = value_w,
                            type_w = type_w
                        ));
                        stderr_println(format!(
                            "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                            "",
                            "",
                            "",
                            name_w = name_w,
                            value_w = value_w,
                            type_w = type_w
                        ));

                        // Print rows
                        for (name, value, ty) in rows {
                            stderr_println(format!(
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
                    "gb" | "globals" => {
                        // Print global environment variables
                        let globals = host.dbg_globals();
                        if globals.is_empty() {
                            stderr_println("no globals");
                            continue;
                        }

                        // Compute widths
                        let mut name_w = "name".len();
                        let mut value_w = "value".len();
                        let mut type_w = "type".len();

                        for (name, v) in &globals {
                            name_w = name_w.max(name.len());
                            value_w = value_w.max(v.to_string().len());
                            type_w = type_w.max(v.type_name().len());
                        }

                        // Print header
                        stderr_println(format!(
                            "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                            "name",
                            "value",
                            "type",
                            name_w = name_w,
                            value_w = value_w,
                            type_w = type_w
                        ));
                        stderr_println(format!(
                            "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                            "",
                            "",
                            "",
                            name_w = name_w,
                            value_w = value_w,
                            type_w = type_w
                        ));

                        // Print rows
                        for (name, v) in &globals {
                            stderr_println(format!(
                                "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                                name,
                                v.to_string(),
                                v.type_name(),
                                name_w = name_w,
                                value_w = value_w,
                                type_w = type_w
                            ));
                        }
                    }

                    "h" | "help" => {
                        stderr_println(include_str!("../../d/wqdbman"));
                    }
                    other => {
                        stderr_println(
                            format!("unknown wqdb command '{other}', type 'h' for help").as_str(),
                        );
                    }
                }
            }
            Err(StdinError::Interrupted) => continue,
            Err(_) => {
                host.dbg_continue();
                break;
            }
        }
    }
}

fn print_current_context(host: &mut dyn DebugHost) {
    let di = host.di();
    let loc = host.loc();
    let name = di.chunk(loc.chunk).name.to_string();
    // If current PC does not have a mapped span yet (e.g., pc 0), try to
    // present the next statement span to show a useful context.
    let meta = di.chunk(loc.chunk);
    let span_here = meta.line_table.span_at(loc.pc);
    if span_here.file_id != u32::MAX {
        eprint!("{}", format_frame(di, loc, &name, true));
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
        eprint!("{}", format_frame(di, nl, &name, true));
    } else {
        // Fallback to previous behavior
        eprint!("{}", format_frame(di, loc, &name, true));
    }
}

fn peek_context(host: &mut dyn DebugHost, n: usize) {
    let di = host.di();
    let loc = host.loc();
    let meta = di.chunk(loc.chunk);
    // Prefer a span for the next statement if current pc has no span yet
    let mut span = meta.line_table.span_at(loc.pc);
    if span.file_id == u32::MAX {
        for pc in loc.pc..meta.len {
            if meta.line_table.is_stmt(pc) {
                span = meta.line_table.span_at(pc);
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
                stderr_println(
                    format!("{:>4} -> {}", ln, sf.line_snippet(ln).trim())
                        .green()
                        .bold()
                        .to_string(),
                );
            } else {
                stderr_println(format!("{:>4}    {}", ln, sf.line_snippet(ln).trim()));
            }
        }
    } else {
        stderr_println("no source available");
    }
}

fn peek_instructions(host: &mut dyn DebugHost, n: usize) {
    let di = host.di();
    let loc = host.loc();
    let meta = di.chunk(loc.chunk);
    let len = meta.len;
    if len == 0 {
        stderr_println("no instructions");
        return;
    }
    let start = loc.pc.saturating_sub(n);
    let end = (loc.pc + n).min(len.saturating_sub(1));
    for pc in start..=end {
        let text = host
            .dbg_ins_at(pc)
            .unwrap_or_else(|| "<unavailable>".to_string());
        if pc == loc.pc {
            stderr_println(format!("{pc:>4} -> {text}").green().bold().to_string());
        } else {
            stderr_println(format!("{pc:>4}    {text}"));
        }
    }
}
