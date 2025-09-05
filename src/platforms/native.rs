#![cfg(not(target_arch = "wasm32"))]

use crate::apps::formatter::{FormatConfig, Formatter};
use crate::builtins::Builtins;
use crate::desserts::daydream::{
    Command, ExecSource, FmtOpts, ParseOutcome, RuntimeFlags, parse_args,
};
use crate::desserts::hotchoco;
use crate::desserts::icedtea::create_boxed_text;
use crate::desserts::tshelper::TSHelper;
use crate::repl::stdio::{
    ReplStdin, StdinError, stdin_add_history, stdin_highlight_enabled, stdin_readline,
    stdin_set_highlight,
};
use crate::repl::{VmEvaluator, repl_engine::ReplEngine};
use crate::value::box_mode;
use crate::wqerror::WqError;
use colored::Colorize;
use rand::Rng;
use rustyline::Editor;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use std::cell::RefCell;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{Read, Write, stdout};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use std::time::Instant;

pub fn start() {
    match parse_args(env::args_os().skip(1)) {
        ParseOutcome::ShowHelp => {
            print_cli_usage();
        }
        ParseOutcome::ShowVersion => {
            print_version();
        }
        ParseOutcome::Error { msg, code } => {
            eprintln!("{msg}");
            std::process::exit(code);
        }
        ParseOutcome::Succeed(parsed) => {
            let rt = parsed.runtime;
            match parsed.command {
                Command::Fmt { script, opts } => {
                    format_script_print_with_opts(&script, opts);
                }
                Command::Exec(ExecSource::Inline(src)) => {
                    exec_command(&src, rt);
                }
                Command::Exec(ExecSource::Stdin) => {
                    let mut input = String::new();
                    let _ = std::io::stdin().read_to_string(&mut input);
                    exec_command(&input, rt);
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

    vm.set_debug_level(rtflags.debug_level);
    vm.set_bt_mode(rtflags.bt);
    vm.set_wqdb(rtflags.wqdb);
    vm.set_stdin(Box::new(RustylineInput::new().unwrap()));

    let mut line_number = 1;
    let mut buffer = String::new();
    // One-shot controls for next input
    let mut oneshot_time = false;
    let mut oneshot_debug: Option<u8> = None;
    let mut oneshot_wqdb = false;

    // Unified loader state for directive lines handled by hotchoco
    let repl_loading = RefCell::new(HashSet::new());

    println!(
        "{} {} {}",
        format!("wq {}", env!("CARGO_PKG_VERSION")).magenta(),
        "(c)tttiw (l)MIT".blue(),
        "!highlight !help !exit".green()
    );

    loop {
        // Prompt construction
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
                        "!help" | "!h" => {
                            println!("{}", create_boxed_text(include_str!("../../d/refcard"), 2));
                            continue;
                        }
                        "!highlight" | "!hl" => {
                            stdin_set_highlight(!stdin_highlight_enabled());
                            continue;
                        }
                        "!builtins" | "!bfn" => {
                            print_builtins();
                            continue;
                        }
                        "!error" | "!err" => {
                            println!("{}", WqError::dump_error_codes());
                            continue;
                        }
                        "!g" | "!vars" => {
                            match vm.get_environment() {
                                Some(env) => {
                                    eprintln!("{}", "~ global bindings ~".red().underline());
                                    for (key, value) in env {
                                        eprintln!(
                                            "{}: {} {}",
                                            key.red().bold(),
                                            value.to_string().yellow(),
                                            value.type_name().green().underline()
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
                            box_mode::set_boxed(!box_mode::is_boxed());
                            system_msg_printer::stdout(
                                format!(
                                    "boxed display is now {}",
                                    (if box_mode::is_boxed() { "on" } else { "off" }).underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "!x" | "!xray" => {
                            xray_mode = !xray_mode;
                            system_msg_printer::stdout(
                                format!(
                                    "xray mode is now {}",
                                    (if xray_mode { "on" } else { "off" }).underline()
                                ),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd.starts_with("!d") => {
                            let rest = cmd.trim_start_matches("!d");
                            if let Some(level_str) = rest.strip_prefix('.') {
                                // One-shot: !d.<level>
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
                            vm.set_wqdb(!vm.is_wqdb_on());
                            system_msg_printer::stdout(
                                format!(
                                    "wqdb is now {}",
                                    (if vm.is_wqdb_on() { "on" } else { "off" }).underline()
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
                                if let hotchoco::LoadErrorKind::Eval(_, ref we) = err.kind {
                                    if matches!(we, WqError::EofError(_)) {
                                        buffer.push_str(input);
                                        buffer.push('\n');
                                        continue;
                                    }
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

                // Prepare for one-shot cmds
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
                // Ensure interactive inputs always map to a fresh <repl> source
                vm.dbg_set_source("<repl>", src_eval);
                vm.dbg_set_offset(0);
                let attempt = vm.eval_string(src_eval);

                // Revert one-shot cmds
                // revert one-shot wqdb
                if oneshot_wqdb {
                    vm.set_wqdb(false);
                    oneshot_wqdb = false;
                }
                // revert one-shot dbg level
                vm.set_debug_level(prev_dbg_level);

                // handle eval result
                match attempt {
                    Ok(result) => {
                        system_msg_printer::stdout(
                            format!("{result}"),
                            system_msg_printer::MsgType::Success,
                        );
                        // X-Ray list info
                        if xray_mode && result.is_list() {
                            let info = xray_info(&result);
                            system_msg_printer::stdout(info, system_msg_printer::MsgType::Info);
                        }
                        if let Some(st) = start_t {
                            system_msg_printer::stdout(
                                format!("time elapsed: {:?}", st.elapsed()),
                                system_msg_printer::MsgType::Info,
                            );
                            // reset one-shot time mode
                            oneshot_time = false;
                        }
                        buffer.clear();
                        line_number += 1;
                    }
                    Err(error) => {
                        if matches!(&error, WqError::EofError(_)) {
                            buffer.push('\n');
                            // one-shot time consumed
                            oneshot_time = false;
                            continue;
                        } else {
                            system_msg_printer::stderr(
                                format!("{error}"),
                                system_msg_printer::MsgType::Error,
                            );
                            // Only show backtrace for runtime errors; skip for parse/EOF errors
                            if rtflags.bt
                                && !matches!(&error, WqError::SyntaxError(_) | WqError::EofError(_))
                            {
                                vm.dbg_print_bt();
                            }
                            if let Some(st) = start_t {
                                let d = st.elapsed();
                                system_msg_printer::stdout(
                                    format!("time elapsed: {d:?}"),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                            // one-shot time consumed
                            oneshot_time = false;
                            buffer.clear();
                            line_number += 1;
                        }
                    }
                }
            }
            Err(StdinError::Eof) => {
                let mut rng = rand::rng();
                if rng.random_bool(0.006666f64) {
                    print!("{}", "\u{258D} ".cyan());
                    stdout().flush().unwrap();
                    thread::sleep(Duration::from_millis(2000));
                    print!("{}", "\r\u{258D} ".red());
                    stdout().flush().unwrap();
                    let message = "you should’ve said goodbye.".red();
                    for ch in message.chars() {
                        thread::sleep(Duration::from_millis(150));
                        print!("{}", ch.to_string().red());
                        stdout().flush().unwrap();
                    }
                    println!("\rprogram \"wq\" terminated       ",);
                }
                break;
            }
            Err(StdinError::Interrupted) => {
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
            // silence in script exec
            // print_load_report_ui(&report)
        }
        Err(err) => print_load_error_ui(&err, &mut vm, rtflags.bt),
    }
}

fn exec_command(content: &str, rtflags: RuntimeFlags) {
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
        Err(err) => print_load_error_ui(&err, &mut vm, rtflags.bt),
    }
}

fn format_script_print_with_opts<P: AsRef<Path>>(filename: P, opts: FmtOpts) {
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

fn print_cli_usage() {
    println!("{}", create_boxed_text(include_str!("../../d/usage"), 2));
}

fn print_version() {
    println!("wq {}", env!("CARGO_PKG_VERSION"));
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

fn print_builtins() {
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
}

fn xray_info(v: &crate::value::Value) -> String {
    use crate::builtins::list as blist;
    use crate::value::Value;

    let count = v.len();
    let depth = match blist::depth(&[v.clone()]) {
        Ok(s) => &s.to_string(),
        _ => "?",
    };
    let shape = match blist::shape(&[v.clone()]) {
        Ok(s) => &s.to_string(),
        _ => "?",
    };
    let rank = match blist::shape(&[v.clone()]) {
        Ok(s) => &s.len().to_string(),
        _ => "?",
    };
    let uniform = match blist::is_uniform(&[v.clone()]) {
        Ok(Value::Bool(b)) => b,
        _ => false,
    };
    format!("xray: count={count}, shape={shape}, rank={rank}, depth={depth}, uniform?={uniform}")
}

pub struct RustylineInput {
    // rl: DefaultEditor,
    rl: Editor<TSHelper, FileHistory>,
}

impl RustylineInput {
    // pub fn new() -> WqResult<Self> {
    //     Ok(Self {
    //         rl: DefaultEditor::new().map_err(|e| WqError::IoError(e.to_string()))?,
    //     })
    // }
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
            format!("no new bindings from `{}`", report.label),
            system_msg_printer::MsgType::Info,
        );
        return;
    }

    if !report.new_bindings.is_empty() {
        system_msg_printer::stderr(
            format!(
                "new bindings from `{}`: {}",
                report.label,
                report.new_bindings.join(", ")
            ),
            system_msg_printer::MsgType::Info,
        );
    }
    if !report.overridden.is_empty() {
        system_msg_printer::stderr(
            format!(
                "overridden bindings from `{}`: {}",
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
            if bt && !matches!(e, WqError::SyntaxError(_) | WqError::EofError(_)) {
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
    use colored::Colorize;

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
