use std::{
    cell::RefCell,
    collections::HashSet,
    env,
    fmt::Write as _,
    fs,
    io::{Read, Write as _, stdout},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use wqpl::{
    apps::{
        evaluator::{Evaluator, default::DefaultEvaluator},
        formatter::{FormatConfig, Formatter},
    },
    builtins::{BuiltinPreset, Builtins},
    helpers::box_mode::format_boxed,
    hotchoco,
    stdio::{
        StdinError, WqStdin, stdin_add_history, stdin_highlight_enabled, stdin_readline,
        stdin_set_highlight,
    },
    value::Value,
    wqerror::WqErrorType,
};
use wqpl::{debug_flags::DebugFlags, interpreters::InterpreterKind};

use crate::daydream::{Command, ExecSource, FmtOpts, ParseOutcome, RuntimeFlags, parse_args};
use wqpl::wqdb::DebugHost;

/// Callback for wqdb pause hook - called by the VM when debugger pauses
fn wqdb_pause_handler(host: &mut dyn DebugHost) {
    crate::wqdb_shell::wqdb_shell(host);
}

/// Enter wqdb shell after an error occurs
fn enter_wqdb_after_err(eval: &mut DefaultEvaluator) {
    crate::wqdb_shell::wqdb_shell_after_err(eval.vm_mut());
}
use crate::tshelper::TSHelper;

use colored::Colorize;
use rand::RngExt;
use rustyline::{Editor, error::ReadlineError, history::FileHistory};

pub fn main() {
    match parse_args(env::args_os().skip(1)) {
        ParseOutcome::ShowHelp => {
            println!("{}", include_str!("../../../d/usage"));
        }
        ParseOutcome::ShowVersion => {
            println!("wq {}", env!("CARGO_PKG_VERSION"));
        }
        ParseOutcome::Error { msg, code } => {
            eprintln!("{msg}");
            std::process::exit(code);
        }
        ParseOutcome::Succeed(parsed) => {
            let rt = parsed.runtime;
            match parsed.command {
                Command::Fmt { script, opts } => {
                    format_script(&script, opts);
                }
                Command::Exec(ExecSource::Inline(src)) => {
                    exec_cmd(&src, rt);
                }
                Command::Exec(ExecSource::Stdin) => {
                    let mut input = String::new();
                    let _ = std::io::stdin().read_to_string(&mut input);
                    exec_cmd(&input, rt);
                }
                Command::Script(path) => {
                    exec_script(&path, rt);
                }
                Command::Repl => {
                    enter_repl(rt);
                }
            }
        }
    }
}

fn enter_repl(rtflags: RuntimeFlags) {
    let mut evaluator = DefaultEvaluator::new();
    evaluator.set_pause_callback(Some(wqdb_pause_handler));
    let mut time_mode = false;
    let mut xray_mode = false;
    let mut box_mode = false;
    evaluator.set_debug_flags(rtflags.debug_flags);
    evaluator.set_bt_mode(rtflags.bt);
    evaluator.set_wqdb(rtflags.wqdb);
    apply_builtins_flag(&mut evaluator, &rtflags);
    apply_interpreter_flag(&mut evaluator, &rtflags);
    evaluator.set_stdin(Box::new(RustylineInput::new().unwrap()));
    let mut line_number = 1;
    let mut buffer = String::new();
    // one-time controls for next input
    let mut oneshot_time = false;
    let mut oneshot_debug: Option<DebugFlags> = None;
    let mut oneshot_wqdb = false;
    // Unified loader state for directive lines handled by hotchoco
    let repl_loading = RefCell::new(HashSet::new());
    const WQ_VERSIOH: &str = env!("CARGO_PKG_VERSION");
    const RUSTC_VER: &str = env!("RUSTC_VERSION");
    const RUSTC_HOST: &str = env!("RUSTC_HOST");
    const RUSTC_LLVM_VERSION: &str = env!("RUSTC_LLVM_VERSION");
    const BUILD_OPT_LEVEL: &str = env!("BUILD_OPT_LEVEL");
    let cwd = match env::current_dir() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => "?".into(),
    };
    println!(
        "{} {} {} {}",
        format!("wq {WQ_VERSIOH}").magenta(),
        format!("(o{BUILD_OPT_LEVEL})").dimmed(),
        "(c)tttiw (l)MIT".blue(),
        "!highlight !help !exit".green()
    );

    // let host_label = "host:".dimmed();
    // let rustc_label = "rustc:".dimmed();
    // let cwd_label = "cwd:".dimmed();
    // let interp_label = "interpreter:".dimmed();
    // let builtins_label = "builtins:".dimmed();
    let label_width = 14;

    println!(
        "{}",
        &format!(
            "{:>label_width$} {RUSTC_HOST}\n{:>label_width$} {RUSTC_VER} [llvm {RUSTC_LLVM_VERSION}]\n{:>label_width$} {cwd}",
            "host:".green().dimmed(),
            "rustc:".blue().dimmed(),
            "cwd:".red().dimmed()
        ),
    );
    println!(
        "{:>label_width$} {}",
        "interpreter:".yellow().dimmed(),
        evaluator.interpreter_name()
    );

    println!(
        "{:>label_width$} {}",
        "builtins:".yellow().dimmed(),
        evaluator.builtins_preset().name()
    );

    loop {
        let prompt = if buffer.is_empty() {
            if cfg!(windows) {
                format!("wq[{line_number}] ")
            } else {
                format!("{}[{}] ", "wq".magenta(), line_number.to_string().blue())
            }
        } else {
            let indent = " ".repeat(line_number.to_string().len());
            if cfg!(windows) {
                format!("{} {} ", indent, "...")
            } else {
                format!("{} {} ", indent, "...".magenta())
            }
        };
        // Read input
        let readline = stdin_readline(&prompt);
        match readline {
            Ok(line) => {
                let input = line.trim_end();
                if !input.is_empty() {
                    stdin_add_history(input);
                }
                // Handle repl commands only if buffer is empty
                if buffer.is_empty() {
                    match input {
                        "!exit" | "!e" | "!!" => {
                            system_msg_printer::stdout(
                                "bye..".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            break;
                        }
                        "!bye" => {
                            system_msg_printer::stdout(
                                "bye".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            break;
                        }
                        "!goodbye" => {
                            print_goodbye();
                            break;
                        }
                        "!highlight" | "!hl" => {
                            stdin_set_highlight(!stdin_highlight_enabled());
                            continue;
                        }
                        cmd if cmd == "!bfn" || cmd == "!" || cmd.starts_with("!bfn ") => {
                            let mut parts = cmd.split_whitespace();
                            let _ = parts.next();
                            let names = BuiltinPreset::names().join(", ");
                            if let Some(preset) = parts.next() {
                                match BuiltinPreset::from_name(preset) {
                                    Some(preset) => {
                                        evaluator.set_builtins_preset(preset);
                                        system_msg_printer::stdout(
                                            format!("bfn -> {}", preset.name()),
                                            system_msg_printer::MsgType::Info,
                                        );
                                    }
                                    None => {
                                        system_msg_printer::stderr(
                                            format!(
                                                "unknown bfn preset '{preset}'; available: {names}"
                                            ),
                                            system_msg_printer::MsgType::Error,
                                        );
                                    }
                                }
                            } else {
                                let current = evaluator.builtins_preset();
                                let names = BuiltinPreset::names().join(", ");
                                system_msg_printer::stderr(
                                    format!(
                                        "active preset: {}\navailable: {names}",
                                        current.name().bold().underline()
                                    ),
                                    system_msg_printer::MsgType::Info,
                                );
                                dump_builtins(evaluator.builtins());
                            }
                            continue;
                        }
                        "!gb" | "!g" => {
                            match evaluator.get_environment() {
                                Some(env) => {
                                    let mut name_w = "name".len();
                                    let mut value_w = "value".len();
                                    let mut type_w = "type".len();
                                    for (name, v) in env {
                                        name_w = name_w.max(name.len());
                                        value_w = value_w.max(v.to_string().len());
                                        type_w = type_w.max(v.type_name().len());
                                    }
                                    eprintln!(
                                        "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                                        "name",
                                        "value",
                                        "type",
                                        name_w = name_w,
                                        value_w = value_w,
                                        type_w = type_w
                                    );
                                    eprintln!(
                                        "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                                        "",
                                        "",
                                        "",
                                        name_w = name_w,
                                        value_w = value_w,
                                        type_w = type_w
                                    );
                                    // Print rows
                                    for (name, v) in env {
                                        eprintln!(
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
                                None => {
                                    system_msg_printer::stdout(
                                        "no global bindings".to_string(),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                            }
                            continue;
                        }
                        "!reset" | "!r" => {
                            evaluator.reset_session();
                            system_msg_printer::stdout(
                                "session reset".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!box" | "!b" => {
                            box_mode = !box_mode;
                            system_msg_printer::stdout(
                                format!(
                                    "boxed display -> {}",
                                    (if box_mode { "on" } else { "off" }).underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!xray" | "!x" => {
                            xray_mode = !xray_mode;
                            system_msg_printer::stdout(
                                format!(
                                    "xray -> {}",
                                    (if xray_mode { "on" } else { "off" }).underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd == "!i" || cmd == "!interpreter" => {
                            let current = evaluator.interpreter_name();
                            let mut list = Vec::new();
                            for name in InterpreterKind::names() {
                                if *name == current {
                                    list.push(format!("{name} (current)"));
                                } else {
                                    list.push((*name).to_string());
                                }
                            }
                            system_msg_printer::stdout(
                                format!("interpreters: {}", list.join(", ")),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd.starts_with("!i ") || cmd.starts_with("!interpreter ") => {
                            let mut parts = cmd.split_whitespace();
                            let _ = parts.next();
                            if let Some(name) = parts.next() {
                                match evaluator.set_interpreter_by_name(name) {
                                    Ok(selected) => {
                                        system_msg_printer::stdout(
                                            format!("interpreter set to {selected}"),
                                            system_msg_printer::MsgType::Info,
                                        );
                                    }
                                    Err(err) => {
                                        let list = InterpreterKind::names().join(", ");
                                        system_msg_printer::stderr(
                                            format!("{err}; available: {list}"),
                                            system_msg_printer::MsgType::Error,
                                        );
                                    }
                                }
                            } else {
                                system_msg_printer::stderr(
                                    "missing interpreter name".to_string(),
                                    system_msg_printer::MsgType::Error,
                                );
                            }
                            continue;
                        }

                        "!time" | "!t" => {
                            time_mode = !time_mode;
                            system_msg_printer::stdout(
                                format!(
                                    "time mode -> {}",
                                    (if time_mode { "on" } else { "off" }).underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!t." | "!time." => {
                            oneshot_time = true;
                            system_msg_printer::stdout(
                                "time will be shown for next eval".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!wqdb" | "!w" => {
                            evaluator.set_wqdb(!evaluator.is_wqdb_enabled());
                            system_msg_printer::stdout(
                                format!(
                                    "wqdb -> {}",
                                    (if evaluator.is_wqdb_enabled() {
                                        "on"
                                    } else {
                                        "off"
                                    })
                                    .underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!wqdb." | "!w." => {
                            oneshot_wqdb = true;
                            system_msg_printer::stdout(
                                "wqdb will be on for next eval".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd.starts_with("!help ")
                            || cmd.starts_with("!h ")
                            || cmd == "!help"
                            || cmd == "!h" =>
                        {
                            // Show usage and arity for a builtin when provided: `!h <name>`
                            let mut parts = cmd.split_whitespace();
                            let _ = parts.next(); // skip !h/!help
                            if let Some(name) = parts.next() {
                                let b = evaluator.builtins();
                                if b.is_enabled_name(name) {
                                    if let Some(id) = b.get_id(name) {
                                        let id = id as u16;
                                        let usage = Builtins::usage_from_id(id).unwrap_or("?");
                                        let arity = Builtins::arity_from_id(id).unwrap_or("?");
                                        println!("{usage} (arity {arity})");
                                    } else {
                                        system_msg_printer::stdout(
                                            format!("unknown builtin '{name}'"),
                                            system_msg_printer::MsgType::Info,
                                        );
                                    }
                                } else {
                                    system_msg_printer::stdout(
                                        format!("unknown builtin '{name}'"),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                            } else {
                                println!("{}", include_str!("../../../d/refcard"));
                            }
                            continue;
                        }
                        "!debug" => {
                            system_msg_printer::stdout(
                                debug_help_table(evaluator.get_debug_flags()),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!d" => {
                            let next = if evaluator.get_debug_flags().is_empty() {
                                DebugFlags::from_alias(1).expect("debug alias 1 exists")
                            } else {
                                DebugFlags::empty()
                            };
                            evaluator.set_debug_flags(next);
                            system_msg_printer::stdout(
                                format!("debug flags -> {}", format_debug_flags(next).underline()),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd.starts_with("!d.") || cmd.starts_with("!debug.") => {
                            let rest = cmd
                                .split_once('.')
                                .map(|(_, rest)| rest.trim())
                                .unwrap_or_default();
                            match DebugFlags::parse(rest) {
                                Ok(flags) => {
                                    oneshot_debug = Some(flags);
                                    system_msg_printer::stdout(
                                        format!(
                                            "debug flags will be {} for next eval",
                                            format_debug_flags(flags).underline()
                                        ),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                                Err(e) => {
                                    system_msg_printer::stderr(
                                        e,
                                        system_msg_printer::MsgType::Error,
                                    );
                                }
                            }
                            continue;
                        }
                        cmd if cmd.starts_with("!d ") || cmd.starts_with("!debug ") => {
                            let rest = cmd
                                .split_once(' ')
                                .map(|(_, rest)| rest.trim())
                                .unwrap_or_default();
                            match DebugFlags::parse(rest) {
                                Ok(flags) => {
                                    evaluator.set_debug_flags(flags);
                                    system_msg_printer::stdout(
                                        format!(
                                            "debug flags -> {}",
                                            format_debug_flags(flags).underline()
                                        ),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                                Err(e) => {
                                    system_msg_printer::stderr(
                                        e,
                                        system_msg_printer::MsgType::Error,
                                    );
                                }
                            }
                            continue;
                        }
                        cmd if cmd.starts_with("!d") => {
                            let rest = cmd.strip_prefix("!d").unwrap_or_default().trim();
                            match DebugFlags::parse(rest) {
                                Ok(flags) => {
                                    evaluator.set_debug_flags(flags);
                                    system_msg_printer::stdout(
                                        format!(
                                            "debug flags -> {}",
                                            format_debug_flags(flags).underline()
                                        ),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                                Err(e) => {
                                    system_msg_printer::stderr(
                                        e,
                                        system_msg_printer::MsgType::Error,
                                    );
                                }
                            }
                            continue;
                        }
                        "" => {
                            // Empty line; continue
                            continue;
                        }
                        _ => {}
                    }
                }
                let mut input_for_eval = input;
                if buffer.is_empty() {
                    if let Some((first, rest)) = input_for_eval.split_once('\n') {
                        if first.trim_start().starts_with("#!") {
                            if rest.trim().is_empty() {
                                continue;
                            }
                            input_for_eval = rest;
                        }
                    } else if input_for_eval.trim_start().starts_with("#!") {
                        continue;
                    }
                    let t = input_for_eval.trim_start();
                    if t.starts_with("!") {
                        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                        match hotchoco::repl_eval_inline(
                            &mut evaluator,
                            input_for_eval,
                            &cwd,
                            &repl_loading,
                            false,
                            rtflags.bt,
                        ) {
                            Ok(report) => {
                                // let has_eval_code = input_for_eval.lines().any(|line| {
                                //     let trimmed = line.trim_start();
                                //     if trimmed.is_empty() || trimmed.starts_with("//") {
                                //         return false;
                                //     }
                                //     if trimmed.starts_with("#!") || trimmed.starts_with("!") {
                                //         return false;
                                //     }
                                //     true
                                // });
                                print_load_report(&report);
                                // if has_eval_code {
                                if let Some(result) = report.result {
                                    let resstr = if box_mode {
                                        format_boxed(&result)
                                    } else {
                                        format!("{result}")
                                    };
                                    system_msg_printer::stdout(
                                        resstr,
                                        system_msg_printer::MsgType::Success,
                                    );
                                    if xray_mode && !result.is_atom() {
                                        let info = xray_info(&result);
                                        system_msg_printer::stdout(
                                            info,
                                            system_msg_printer::MsgType::Info,
                                        );
                                    }
                                    // }
                                }
                            }
                            Err(err) => {
                                // Only treat EOF as a signal to continue buffering multi-line input
                                if let hotchoco::LoadErrorKind::Eval(_, we) = &err.kind
                                    && we.err_type == WqErrorType::Eof
                                {
                                    buffer.push_str(input_for_eval);
                                    buffer.push('\n');
                                    continue;
                                }
                                print_load_error(&err, &mut evaluator, rtflags.bt);
                            }
                        }
                        // move on to next prompt
                        continue;
                    }
                }
                buffer.push_str(input_for_eval);
                let src_eval = buffer.trim();
                // Prepare for one-time cmds
                let prev_dbg_flags = evaluator.get_debug_flags();
                if let Some(flags) = oneshot_debug.take() {
                    evaluator.set_debug_flags(flags);
                }
                if oneshot_wqdb {
                    evaluator.set_wqdb(true);
                }
                let start_t = if time_mode || oneshot_time {
                    Some(Instant::now())
                } else {
                    None
                };
                // eval
                // Ensure interactive inputs map to a unique source label per iteration
                let source_label = format!("wq[{}]", line_number);
                evaluator.dbg_set_source(&source_label, src_eval);
                evaluator.dbg_set_offset(0);
                // Note whether wqdb was active for this evaluation (persistent or one-time)
                let wqdb_active_for_eval = evaluator.is_wqdb_enabled() || oneshot_wqdb;
                let attempt = evaluator.eval_string(src_eval);
                // reset one-time cmds and wqdb
                if oneshot_wqdb {
                    evaluator.set_wqdb(false);
                    oneshot_wqdb = false;
                }
                // reset one-time dbg level
                evaluator.set_debug_flags(prev_dbg_flags);
                // handle eval result
                match attempt {
                    Ok(result) => {
                        let resstr = if box_mode {
                            format_boxed(&result)
                        } else {
                            format!("{result}")
                        };
                        system_msg_printer::stdout(resstr, system_msg_printer::MsgType::Success);
                        // X-Ray list info
                        if xray_mode && !result.is_atom() {
                            let info = xray_info(&result);
                            system_msg_printer::stdout(info, system_msg_printer::MsgType::Info);
                        }
                        if let Some(st) = start_t {
                            system_msg_printer::stdout(
                                format!("time elapsed: {:?}", st.elapsed()),
                                system_msg_printer::MsgType::Info,
                            );
                            // reset one-time time mode
                            oneshot_time = false;
                        }
                        buffer.clear();
                        line_number += 1;
                    }
                    Err(error) => {
                        if error.err_type == WqErrorType::Eof {
                            buffer.push('\n');
                            // one-time time consumed
                            oneshot_time = false;
                            continue;
                        } else {
                            system_msg_printer::stderr(
                                format!("{error}"),
                                system_msg_printer::MsgType::Error,
                            );
                            // Only show backtrace for runtime errors; skip for parse/EOF errors
                            if rtflags.bt && error.err_type.is_runtime() {
                                evaluator.dbg_print_bt();
                            }
                            if wqdb_active_for_eval && error.err_type.is_runtime() {
                                enter_wqdb_after_err(&mut evaluator);
                            }
                            if let Some(st) = start_t {
                                let d = st.elapsed();
                                system_msg_printer::stdout(
                                    format!("time elapsed: {d:?}"),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                            // one-time time consumed
                            oneshot_time = false;
                            buffer.clear();
                            line_number += 1;
                        }
                    }
                }
            }
            Err(StdinError::Eof) => {
                break;
            }
            Err(StdinError::Interrupted) => {
                if !buffer.is_empty() {
                    // Cancel current multi-line input
                    buffer.clear();
                    oneshot_time = false;
                    oneshot_debug = None;
                    oneshot_wqdb = false;
                }
                continue;
            }
            Err(StdinError::Other(error)) => {
                system_msg_printer::stderr(
                    format!("Error reading input: {error}"),
                    system_msg_printer::MsgType::Error,
                );
                break;
            }
        }
    }
}

fn exec_script<P: AsRef<Path>>(filename: P, rtflags: RuntimeFlags) {
    let mut evaluator = DefaultEvaluator::new();
    evaluator.set_pause_callback(Some(wqdb_pause_handler));
    evaluator.set_debug_flags(rtflags.debug_flags);
    evaluator.set_stdin(Box::new(RustylineInput::new().unwrap()));
    evaluator.set_wqdb(rtflags.wqdb);
    apply_builtins_flag(&mut evaluator, &rtflags);
    apply_interpreter_flag(&mut evaluator, &rtflags);
    let loading = RefCell::new(HashSet::new());
    match hotchoco::repl_load_script(&mut evaluator, filename, &loading, true, rtflags.bt) {
        Ok(report) => {
            if rtflags.print
                && let Some(result) = report.result
            {
                println!("{result}");
            }
        }
        Err(err) => {
            print_load_error(&err, &mut evaluator, rtflags.bt);
            if evaluator.is_wqdb_enabled() && err.is_runtime() {
                enter_wqdb_after_err(&mut evaluator);
            }
        }
    }
}

fn exec_cmd(content: &str, rtflags: RuntimeFlags) {
    let mut evaluator = DefaultEvaluator::new();
    evaluator.set_pause_callback(Some(wqdb_pause_handler));
    evaluator.set_debug_flags(rtflags.debug_flags);
    evaluator.set_stdin(Box::new(RustylineInput::new().unwrap()));
    evaluator.set_wqdb(rtflags.wqdb);
    apply_builtins_flag(&mut evaluator, &rtflags);
    apply_interpreter_flag(&mut evaluator, &rtflags);
    let loading = RefCell::new(HashSet::new());
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match hotchoco::repl_eval_inline(&mut evaluator, content, &cwd, &loading, true, rtflags.bt) {
        Ok(report) => {
            if rtflags.print
                && let Some(result) = report.result
            {
                println!("{result}");
            }
        }
        Err(err) => {
            print_load_error(&err, &mut evaluator, rtflags.bt);
            if evaluator.is_wqdb_enabled() && err.is_runtime() {
                enter_wqdb_after_err(&mut evaluator);
            }
        }
    }
}

fn format_script<P: AsRef<Path>>(filename: P, opts: FmtOpts) {
    let path = filename.as_ref();
    match fs::read_to_string(path) {
        Ok(content) => {
            let fmt = Formatter::new(FormatConfig {
                indent_size: 2,
                nlcd: opts.nlcd,
                no_bracket_calls: opts.nbc,
                one_line_wizard: opts.olw,
            });
            match fmt.format_script(&content) {
                Ok(out) => println!("{out}"),
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            }
        }
        Err(err) => {
            eprintln!("Cannot read {}: {err}", path.display());
            std::process::exit(1);
        }
    }
}

fn print_goodbye() {
    let mut rng = rand::rng();
    let mut stdout = stdout();
    let frames = if rng.random_bool(0.5) {
        [";D", ";D", ";D", ";D", ";)"]
    } else {
        [":D", ":D", ":D", ":D", ":)"]
    };
    print!("{}", "\u{258D} goodbye! ".cyan());
    stdout.flush().unwrap();
    thread::sleep(Duration::from_millis(250));
    for &face in &frames {
        print!("\r{}", format!("\u{258D} goodbye! {face}").cyan());
        stdout.flush().unwrap();
        thread::sleep(Duration::from_millis(300));
    }
    print!("\r{}", "\u{258D} goodbye!        ".cyan());
    println!();
}

fn dump_builtins(builtins: &Builtins) {
    let mut funcs = builtins.list_functions();
    funcs.sort();
    let max_len = funcs.iter().map(|s| s.len()).max().unwrap_or(0);
    let columns = 6;
    for (i, func) in funcs.iter().enumerate() {
        print!("{func:<max_len$} ");
        if (i + 1) % columns == 0 {
            println!();
        }
    }
    println!();
}

fn apply_interpreter_flag(evaluator: &mut DefaultEvaluator, rtflags: &RuntimeFlags) {
    if let Some(name) = rtflags.interpreter.as_deref()
        && let Err(err) = evaluator.set_interpreter_by_name(name)
    {
        let list = InterpreterKind::names().join(", ");
        eprintln!("{err}; available: {list}");
        std::process::exit(2);
    }
}

fn apply_builtins_flag(evaluator: &mut DefaultEvaluator, rtflags: &RuntimeFlags) {
    if let Some(preset) = rtflags.builtins.as_deref() {
        match BuiltinPreset::from_name(preset) {
            Some(preset) => evaluator.set_builtins_preset(preset),
            None => {
                let names = BuiltinPreset::names().join(", ");
                eprintln!("unknown builtin preset '{preset}'; available: {names}");
                std::process::exit(2);
            }
        }
    }
}

fn format_debug_flags(flags: DebugFlags) -> String {
    let names = flags.display_names();
    if names.is_empty() {
        "off".to_string()
    } else {
        names.join(",")
    }
}

fn debug_help_table(active: DebugFlags) -> String {
    let rows = [
        ("active", format_debug_flags(active)),
        ("0", "off".to_string()),
        ("1", "inst".to_string()),
        ("2", "inst,ast".to_string()),
        ("3", "inst,ast,token".to_string()),
        ("4", "inst,ast,token,wqdb-1,wqdb-2".to_string()),
    ];
    let left_w = rows.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    let right_w = rows.iter().map(|(_, r)| r.len()).max().unwrap_or(0);
    let rule = format!("+-{:-<left_w$}-+-{:-<right_w$}-+", "", "");
    let mut out = String::new();
    out.push_str(&rule);
    out.push('\n');
    out.push_str(&format!("| {:<left_w$} | {:<right_w$} |", "spec", "flags"));
    out.push('\n');
    out.push_str(&rule);
    out.push('\n');
    for (left, right) in rows {
        out.push_str(&format!("| {:<left_w$} | {:<right_w$} |", left, right));
        out.push('\n');
    }
    out.push_str(&rule);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_debug_spec_supports_named_flags() {
        let flags = DebugFlags::parse("inst,token,wqdb-1").unwrap();
        assert!(flags.contains(DebugFlags::INST));
        assert!(flags.contains(DebugFlags::TOKEN));
        assert!(flags.contains(DebugFlags::WQDB_1));
        assert!(!flags.contains(DebugFlags::AST));
    }

    #[test]
    fn parse_debug_spec_supports_oneshot_default_alias() {
        let flags = DebugFlags::parse("").unwrap();
        assert_eq!(flags, DebugFlags::from_alias(1).unwrap());
    }

    #[test]
    fn parse_debug_spec_rejects_invalid_numeric_alias() {
        assert!(DebugFlags::parse("7").is_err());
    }

    #[test]
    fn debug_alias_table_lists_supported_aliases() {
        let table = debug_help_table(DebugFlags::empty());
        assert!(table.contains("| 0      | off"));
        assert!(table.contains("| 4      | inst,ast,token,wqdb-1,wqdb-2 |"));
    }

    #[test]
    fn debug_help_table_is_tabular() {
        let table = debug_help_table(DebugFlags::from_names(["token", "inst"]));
        assert!(table.contains("| spec   |"));
        assert!(table.contains("active"));
        assert!(table.contains("token,inst"));
        assert!(table.contains("inst,ast,token,wqdb-1,wqdb-2"));
    }

    #[test]
    fn parse_debug_spec_supports_compact_alias_shortcut() {
        let flags = DebugFlags::parse("1").unwrap();
        assert_eq!(flags, DebugFlags::from_alias(1).unwrap());
    }
}

fn xray_info(v: &Value) -> String {
    fn two_col_item_values(pairs: &[(&str, String)], gutter: usize) -> String {
        if pairs.is_empty() {
            return String::new();
        }
        // Max key widths per column (even = left, odd = right)
        let left_key_w = pairs
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 0)
            .map(|(_, (k, _))| k.len())
            .max()
            .unwrap_or(0);
        let right_key_w = pairs
            .iter()
            .enumerate()
            .filter(|(i, _)| i % 2 == 1)
            .map(|(_, (k, _))| k.len())
            .max()
            .unwrap_or(0);
        // Label widths (including ": ")
        let left_label_w = left_key_w + 2; // ": "
        let right_label_w = right_key_w + 2;
        // Build left cells with aligned value starts
        let mut left_cells: Vec<String> = Vec::new();
        for (i, (k, v)) in pairs.iter().enumerate() {
            if i % 2 == 0 {
                let label = format!("{}: ", k);
                left_cells.push(format!("{:<lw$}{}", label, v, lw = left_label_w));
            }
        }
        let left_col_w = left_cells.iter().map(|s| s.len()).max().unwrap_or(0);
        let mut out = String::new();
        let rows = pairs.len().div_ceil(2);
        for r in 0..rows {
            let li = 2 * r;
            let ri = li + 1;
            // left cell: label-padded (values aligned) then cell-padded (column width)
            let left_label = format!("{}: ", pairs[li].0);
            let left_cell = format!("{:<lw$}{}", left_label, pairs[li].1, lw = left_label_w);
            let left_pad = format!("{:<cw$}", left_cell, cw = left_col_w);
            if ri < pairs.len() {
                // right cell: align value start within the right column
                let right_label = format!("{}: ", pairs[ri].0);
                let right_cell = format!("{:<rw$}{}", right_label, pairs[ri].1, rw = right_label_w);
                let _ = writeln!(out, "{}{}{}", left_pad, " ".repeat(gutter), right_cell);
            } else {
                let _ = writeln!(out, "{}", left_pad);
            }
        }
        out
    }

    let pairs = [
        ("count", format!("{}", v.len())),
        ("depth", format!("{}", v.depth())),
        ("shape", format!("{}", v.shape())),
        ("axes", format!("{}", v.axes())),
        ("uniform?", format!("{}", v.is_uniform())),
    ];
    two_col_item_values(&pairs, 4)
}

pub struct RustylineInput {
    rl: Editor<TSHelper, FileHistory>,
}

impl RustylineInput {
    pub fn new() -> rustyline::Result<Self> {
        let mut rl: Editor<TSHelper, _> = Editor::new()?;
        rl.set_helper(Some(TSHelper::new()));
        Ok(Self { rl })
    }
}

impl WqStdin for RustylineInput {
    fn readline(&mut self, prompt: &str) -> Result<String, StdinError> {
        match self.rl.readline(prompt) {
            Ok(line) => Ok(line),
            Err(ReadlineError::Eof) => Err(StdinError::Eof),
            Err(ReadlineError::Interrupted) => Err(StdinError::Interrupted),
            Err(e) => Err(StdinError::Other(e.to_string())),
        }
    }

    fn add_history(&mut self, line: &str) {
        let _ = self.rl.add_history_entry(line);
    }

    fn set_highlight(&mut self, on: bool) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_enabled(on);
        }
    }

    fn highlight_enabled(&self) -> bool {
        self.rl.helper().map(|h| h.enabled()).unwrap_or(true)
    }
}

fn print_load_report(report: &hotchoco::LoadReport) {
    for w in &report.warnings {
        system_msg_printer::stderr(format!("warning: {w}"), system_msg_printer::MsgType::Info);
    }
    if report.new_bindings.is_empty() && report.overridden.is_empty() {
        system_msg_printer::stderr(
            format!("no new bindings from '{}'", report.label),
            system_msg_printer::MsgType::Info,
        );
        return;
    }
    if !report.new_bindings.is_empty() {
        system_msg_printer::stderr(
            format!(
                "new bindings from '{}': {}",
                report.label,
                report.new_bindings.join(", ")
            ),
            system_msg_printer::MsgType::Info,
        );
    }
    if !report.overridden.is_empty() {
        system_msg_printer::stderr(
            format!(
                "overridden bindings from '{}': {}",
                report.label,
                report.overridden.join(", ")
            ),
            system_msg_printer::MsgType::Info,
        );
    }
}

fn print_load_error<R: Evaluator>(err: &hotchoco::LoadError, evaluator: &mut R, bt: bool) {
    match &err.kind {
        hotchoco::LoadErrorKind::Cycle(path) => {
            system_msg_printer::stderr(
                format!("[hotchoco] cannot load {}: cycling", path.display()),
                system_msg_printer::MsgType::Error,
            );
        }
        hotchoco::LoadErrorKind::Io(path, e) => {
            system_msg_printer::stderr(
                format!("[hotchoco] cannot load {}: {}", path.display(), e),
                system_msg_printer::MsgType::Error,
            );
        }
        hotchoco::LoadErrorKind::Eval(label, e) => {
            system_msg_printer::stderr(
                format!("[hotchoco] eval error at {label}\n{e}"),
                system_msg_printer::MsgType::Error,
            );
            if bt && e.err_type.is_runtime() {
                evaluator.dbg_print_bt();
            }
        }
        hotchoco::LoadErrorKind::Directive(cmd) => {
            system_msg_printer::stderr(
                format!("[hotchoco] unknown directive: {cmd}"),
                system_msg_printer::MsgType::Error,
            );
        }
    }
    if !err.stack.is_empty() {
        system_msg_printer::stderr(
            format!("[hotchoco] import stack: {}", err.stack.join(" -> ")),
            system_msg_printer::MsgType::Info,
        );
    }
}

mod system_msg_printer {
    use colored::Colorize;

    pub enum MsgType {
        Info,
        Error,
        Success,
    }

    fn format_msg(msg: impl Into<String>, msg_type: MsgType) -> String {
        let msg = msg.into();
        let mut lines = msg.lines();
        let mut formatted = String::new();
        if let Some(first) = lines.next() {
            formatted.push_str(&format!("\u{258D} {first}\n"));
        }
        for line in lines {
            formatted.push_str(&format!("  {line}\n"));
        }
        if formatted.ends_with('\n') {
            formatted.pop();
        }
        match msg_type {
            MsgType::Info => formatted.cyan().to_string(),
            MsgType::Error => formatted.red().to_string(),
            MsgType::Success => formatted,
        }
    }

    pub fn stdout(msg: impl Into<String>, msg_type: MsgType) {
        println!("{}", format_msg(msg, msg_type));
    }

    pub fn stderr(msg: impl Into<String>, msg_type: MsgType) {
        eprintln!("{}", format_msg(msg, msg_type));
    }
}
