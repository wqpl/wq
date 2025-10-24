#![cfg(not(target_arch = "wasm32"))]

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
    apps::formatter::{FormatConfig, Formatter},
    builtins::Builtins,
    colored::Colorize,
    daydream::{Command, ExecSource, FmtOpts, ParseOutcome, RuntimeFlags, parse_args},
    hotchoco,
    repl::{
        VmEvaluator,
        box_mode::format_boxed,
        enter_wqdb_post_mortem,
        repl_engine::ReplEngine,
        stdio::{
            ReplStdin, StdinError, stdin_add_history, stdin_highlight_enabled, stdin_readline,
            stdin_set_highlight,
        },
        tshelper::TSHelper,
    },
    value::Value,
    wqerr::WqErrType,
};

use rand::Rng;
use rustyline::{Editor, error::ReadlineError, history::FileHistory};

pub fn main() {
    match parse_args(env::args_os().skip(1)) {
        ParseOutcome::ShowHelp => {
            println!("{}", include_str!("../d/usage"));
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
    let mut vm = VmEvaluator::new();
    let mut time_mode = false;
    let mut xray_mode = false;
    let mut box_mode = false;
    vm.set_debug_level(rtflags.debug_level);
    vm.set_bt_mode(rtflags.bt);
    vm.set_wqdb(rtflags.wqdb);
    vm.set_stdin(Box::new(RustylineInput::new().unwrap()));
    let mut line_number = 1;
    let mut buffer = String::new();
    // one-time controls for next input
    let mut oneshot_time = false;
    let mut oneshot_debug: Option<u8> = None;
    let mut oneshot_wqdb = false;
    // Unified loader state for directive lines handled by hotchoco
    let repl_loading = RefCell::new(HashSet::new());
    const WQ_VERSIOH: &str = env!("CARGO_PKG_VERSION");
    const GIT_REV: &str = env!("GIT_REV");
    const RUSTC_VER: &str = env!("RUSTC_VERSION");
    const RUSTC_HOST: &str = env!("RUSTC_HOST");
    const RUSTC_LLVM_VERSION: &str = env!("RUSTC_LLVM_VERSION");
    const BUILD_OPT_LEVEL: &str = env!("BUILD_OPT_LEVEL");
    let cwd = match env::current_dir() {
        Ok(path) => path.to_string_lossy().into_owned(),
        Err(_) => "?".into(),
    };
    println!(
        "{} {} {}",
        format!("wq {WQ_VERSIOH}").magenta(),
        "(c)tttiw (l)MIT".blue(),
        "!highlight !help !exit".green()
    );
    println!(
        "{}",
        &format!(
            "{} {GIT_REV}\n{} {RUSTC_HOST}\n{} {RUSTC_VER} [llvm {RUSTC_LLVM_VERSION}]\n{} {BUILD_OPT_LEVEL}\n{} {cwd}",
            "rev:  ".dimmed(),
            "host: ".dimmed(),
            "rustc:".dimmed(),
            "O:    ".dimmed(),
            "cwd:  ".dimmed()
        ),
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
                        "!exit" | "!e" => {
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
                        cmd if cmd.starts_with("!help") || cmd.starts_with("!h") => {
                            // Show usage and arity for a builtin when provided: `!h <name>`
                            let mut parts = cmd.split_whitespace();
                            let _ = parts.next(); // skip !h/!help
                            if let Some(name) = parts.next() {
                                let b = Builtins::new();
                                if let Some(id) = b.get_id(name) {
                                    let id = id as u16;
                                    let usage = Builtins::usage_from_id(id).unwrap_or("?");
                                    let arity = Builtins::arity_from_id(id).unwrap_or("?");
                                    println!("{usage} ({arity})");
                                } else {
                                    system_msg_printer::stdout(
                                        format!("unknown builtin: {name}"),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                                continue;
                            }
                            println!("{}", include_str!("../d/refcard"));
                            continue;
                        }
                        "!highlight" | "!hl" => {
                            stdin_set_highlight(!stdin_highlight_enabled());
                            continue;
                        }
                        "!builtins" | "!bfn" => {
                            dump_builtins();
                            continue;
                        }
                        "!gb" | "!g" => {
                            match vm.get_environment() {
                                Some(env) => {
                                    // Compute widths
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
                            vm.reset_session();
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
                                    "boxed display is now {}",
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
                                    "xray is now {}",
                                    (if xray_mode { "on" } else { "off" }).underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd.starts_with("!d") => {
                            let rest = cmd.trim_start_matches("!d");
                            if let Some(level_str) = rest.strip_prefix('.') {
                                // one-time: !d.<level>
                                if let Ok(level) = level_str.parse::<u8>() {
                                    oneshot_debug = Some(level);
                                    system_msg_printer::stdout(
                                        format!("debug level will be {level} for next eval"),
                                        system_msg_printer::MsgType::Info,
                                    );
                                }
                            } else if rest.is_empty() {
                                match vm.get_debug_level() {
                                    0 => vm.set_debug_level(1),
                                    _ => vm.set_debug_level(0),
                                }
                                system_msg_printer::stdout(
                                    format!(
                                        "debug level is now {}",
                                        vm.get_debug_level().to_string().underline()
                                    ),
                                    system_msg_printer::MsgType::Info,
                                );
                            } else if let Ok(level) = rest.parse::<u8>() {
                                vm.set_debug_level(level);
                                system_msg_printer::stdout(
                                    format!("debug level is now {}", level.to_string().underline()),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                            continue;
                        }
                        "!time" | "!t" => {
                            time_mode = !time_mode;
                            system_msg_printer::stdout(
                                format!(
                                    "time mode is now {}",
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
                            vm.set_wqdb(!vm.is_wqdb_enabled());
                            system_msg_printer::stdout(
                                format!(
                                    "wqdb is now {}",
                                    (if vm.is_wqdb_enabled() { "on" } else { "off" }).underline()
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
                        "" => {
                            // Empty line; continue
                            continue;
                        }
                        _ => {}
                    }
                }
                // If this is a directive line, hand it to hotchoco
                // eprintln!("input={input}");
                if buffer.is_empty() {
                    let t = input.trim_start();
                    if t.starts_with("!") {
                        let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
                        match hotchoco::repl_eval_inline(
                            &mut vm,
                            input,
                            &cwd,
                            &repl_loading,
                            false,
                            rtflags.bt,
                        ) {
                            Ok(report) => print_load_report_ui(&report),
                            Err(err) => {
                                // Only treat EOF as a signal to continue buffering multi-line input
                                if let hotchoco::LoadErrorKind::Eval(_, we) = &err.kind
                                    && we.err_type == WqErrType::Eof
                                {
                                    buffer.push_str(input);
                                    buffer.push('\n');
                                    continue;
                                }
                                print_load_error_ui(&err, &mut vm, rtflags.bt);
                            }
                        }
                        // move on to next prompt
                        continue;
                    }
                }
                buffer.push_str(input);
                let src_eval = buffer.trim();
                // Prepare for one-time cmds
                let prev_dbg_level = vm.get_debug_level();
                if let Some(level) = oneshot_debug.take() {
                    vm.set_debug_level(level);
                }
                if oneshot_wqdb {
                    vm.set_wqdb(true);
                }
                let start_t = if time_mode || oneshot_time {
                    Some(Instant::now())
                } else {
                    None
                };
                // eval
                // Ensure interactive inputs map to a unique source label per iteration
                let source_label = format!("wq[{}]", line_number);
                vm.dbg_set_source(&source_label, src_eval);
                vm.dbg_set_offset(0);
                // Note whether wqdb was active for this evaluation (persistent or one-time)
                let wqdb_active_for_eval = vm.is_wqdb_enabled() || oneshot_wqdb;
                let attempt = vm.eval_string(src_eval);
                // reset one-time cmds and wqdb
                if oneshot_wqdb {
                    vm.set_wqdb(false);
                    oneshot_wqdb = false;
                }
                // reset one-time dbg level
                vm.set_debug_level(prev_dbg_level);
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
                        if error.err_type == WqErrType::Eof {
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
                                vm.dbg_print_bt();
                            }
                            // If wqdb was active for this eval, enter post-mortem shell
                            if wqdb_active_for_eval {
                                enter_wqdb_post_mortem(&mut vm);
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
    let mut vm = VmEvaluator::new();
    vm.set_debug_level(rtflags.debug_level);
    vm.set_stdin(Box::new(RustylineInput::new().unwrap()));
    // if wqdb_mode {
    // Enter wqdb persistently for script execution
    vm.set_wqdb(rtflags.wqdb);
    // vm.arm_wqdb_next();
    // }
    let loading = RefCell::new(HashSet::new());
    // Use the loader to execute the script with proper debug source tracking
    match hotchoco::repl_load_script(&mut vm, filename, &loading, true, rtflags.bt) {
        Ok(_) => {
            // script exec should not print load report
            // print_load_report_ui(&report)
        }
        Err(err) => {
            print_load_error_ui(&err, &mut vm, rtflags.bt);
            if vm.is_wqdb_enabled() {
                enter_wqdb_post_mortem(&mut vm);
            }
        }
    }
}

fn exec_cmd(content: &str, rtflags: RuntimeFlags) {
    let mut vm = VmEvaluator::new();
    vm.set_debug_level(rtflags.debug_level);
    vm.set_stdin(Box::new(RustylineInput::new().unwrap()));
    vm.set_wqdb(rtflags.wqdb);
    let loading = RefCell::new(HashSet::new());
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match hotchoco::repl_eval_inline(&mut vm, content, &cwd, &loading, true, rtflags.bt) {
        Ok(_report) => {
            // silent
        }
        Err(err) => {
            print_load_error_ui(&err, &mut vm, rtflags.bt);
            if vm.is_wqdb_enabled() {
                enter_wqdb_post_mortem(&mut vm);
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

fn dump_builtins() {
    let mut funcs = Builtins::new().list_functions();
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

    // Pad left cells so the right column always starts at the same x
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

fn xray_info(v: &Value) -> String {
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

impl ReplStdin for RustylineInput {
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

fn print_load_report_ui(report: &hotchoco::LoadReport) {
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

fn print_load_error_ui<R: ReplEngine>(err: &hotchoco::LoadError, evaluator: &mut R, bt: bool) {
    match &err.kind {
        hotchoco::LoadErrorKind::Cycle(path) => {
            system_msg_printer::stderr(
                format!("Cannot load {}: cycling", path.display()),
                system_msg_printer::MsgType::Error,
            );
        }
        hotchoco::LoadErrorKind::Io(path, e) => {
            system_msg_printer::stderr(
                format!("Cannot load {}: {}", path.display(), e),
                system_msg_printer::MsgType::Error,
            );
        }
        hotchoco::LoadErrorKind::Eval(label, e) => {
            system_msg_printer::stderr(
                format!("Error in {label}: {e}"),
                system_msg_printer::MsgType::Error,
            );
            if bt && e.err_type.is_runtime() {
                evaluator.dbg_print_bt();
            }
        }
    }
    if !err.stack.is_empty() {
        system_msg_printer::stderr(
            format!("import stack: {}", err.stack.join(" -> ")),
            system_msg_printer::MsgType::Info,
        );
    }
}

mod system_msg_printer {
    use wqpl::colored::Colorize;

    pub enum MsgType {
        Info,
        Error,
        Success,
    }

    fn format_msg(msg: String, msg_type: MsgType) -> String {
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

    pub fn stdout(msg: String, msg_type: MsgType) {
        println!("{}", format_msg(msg, msg_type));
    }

    pub fn stderr(msg: String, msg_type: MsgType) {
        eprintln!("{}", format_msg(msg, msg_type));
    }
}
