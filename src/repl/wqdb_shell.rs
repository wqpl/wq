#[cfg(not(target_arch = "wasm32"))]
use colored::Colorize;

use crate::{
    desserts::icedtea::create_boxed_text,
    wqdb::{CodeLoc, DebugHost, format_frame},
};

use super::stdio::{StdinError, stderr_println, stdin_readline, stdin_with_highlight_off};

pub fn wqdb_shell(host: &mut dyn DebugHost) {
    let mut dbg_line = 1usize;
    // Print current context
    // stderr_println(&format!(
    //     "{}[{}] ",
    //     "wqdb".bold().bright_magenta(),
    //     dbg_line.to_string().bright_blue()
    // ));
    print_current_context(host);

    loop {
        #[cfg(not(target_arch = "wasm32"))]
        let prompt = format!(
            "{}[{}] ",
            "wqdb".bold().bright_magenta(),
            dbg_line.to_string().bright_blue()
        );

        #[cfg(any(target_arch = "wasm32", target_os = "windows"))]
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
                                    stderr_println(&format!("no statement at line {ln}"));
                                } else {
                                    for l in &locs {
                                        host.dbg_set_break(*l);
                                    }
                                    stderr_println(&format!(
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
                                stderr_println(&format!(
                                    "{}:{} (pc {} in {})",
                                    sf.path, l, b.pc, meta.name
                                ));
                            } else {
                                stderr_println(&format!(
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
                        for (loc, name) in frames {
                            eprint!("{}", format_frame(di, loc, &name));
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
                    "locals" | "lb" => {
                        // Print current frame locals using debug names when available
                        let locals = host.dbg_locals();
                        if locals.is_empty() {
                            stderr_println("no locals");
                            continue;
                        }
                        let di = host.di();
                        let loc = host.loc();
                        let meta = di.chunk(loc.chunk);
                        if let Some(names) = &meta.local_names {
                            for (i, v) in locals {
                                let name =
                                    names.get(i).cloned().unwrap_or_else(|| format!("loc[{i}]"));
                                #[cfg(not(target_arch = "wasm32"))]
                                stderr_println(&format!(
                                    "{}: {} {}",
                                    name.red().bold(),
                                    v.to_string().yellow(),
                                    v.type_name_verbose().green().underline()
                                ));
                                #[cfg(target_arch = "wasm32")]
                                stderr_println(&format!("{name}: {v} {}", v.type_name_verbose()));
                            }
                        } else {
                            for (i, v) in locals {
                                // stderr_println(&format!(
                                //     "loc[{i}] = {v}  ({})",
                                //     v.type_name_verbose()
                                // ));
                                #[cfg(not(target_arch = "wasm32"))]
                                stderr_println(&format!(
                                    "loc[{i}]: {} {}",
                                    v.to_string().yellow(),
                                    v.type_name_verbose().green().underline()
                                ));
                                #[cfg(target_arch = "wasm32")]
                                stderr_println(&format!("loc[{i}]: {v} {}", v.type_name_verbose()));
                            }
                        }
                    }
                    "globals" | "gb" => {
                        // Print global environment variables
                        let globals = host.dbg_globals();
                        if globals.is_empty() {
                            stderr_println("no globals");
                        } else {
                            // Sort by name for stable output
                            let mut pairs = globals;
                            pairs.sort_by(|a, b| a.0.cmp(&b.0));
                            for (name, v) in pairs.into_iter() {
                                #[cfg(not(target_arch = "wasm32"))]
                                stderr_println(&format!(
                                    "{}: {} {}",
                                    name.red().bold(),
                                    v.to_string().yellow(),
                                    v.type_name_verbose().green().underline()
                                ));
                                #[cfg(target_arch = "wasm32")]
                                stderr_println(&format!("{name}: {v} {}", v.type_name_verbose()));
                            }
                        }
                    }
                    "h" | "help" => {
                        stderr_println(
                            create_boxed_text(include_str!("../../d/wqdbman"), 2).as_str(),
                        );
                    }
                    other => {
                        stderr_println(
                            format!("I do not recognize command `{other}`. Type `h` for help.")
                                .as_str(),
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
        eprint!("{}", format_frame(di, loc, &name));
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
        eprint!("{}", format_frame(di, nl, &name));
    } else {
        // Fallback to previous behavior
        eprint!("{}", format_frame(di, loc, &name));
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
                #[cfg(not(target_arch = "wasm32"))]
                stderr_println(
                    &format!("{:>4} -> {}", ln, sf.line_snippet(ln).trim())
                        .green()
                        .bold()
                        .to_string(),
                );
                #[cfg(target_arch = "wasm32")]
                stderr_println(&format!("{:>4} -> {}", ln, sf.line_snippet(ln).trim()).to_string());
            } else {
                stderr_println(&format!("{:>4}    {}", ln, sf.line_snippet(ln).trim()));
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
            #[cfg(not(target_arch = "wasm32"))]
            {
                use colored::Colorize;
                stderr_println(&format!("{pc:>4} -> {text}").green().bold().to_string());
            }
            #[cfg(target_arch = "wasm32")]
            {
                stderr_println(&format!("{pc:>4} -> {text}"));
            }
        } else {
            stderr_println(&format!("{pc:>4}    {text}"));
        }
    }
}
