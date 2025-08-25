use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::env;
use std::fs;
use std::io::{Write, stdout};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use wq::apps::formatter::{FormatOptions, Formatter};
use wq::builtins_help;
use wq::helpers::string_helpers::create_boxed_text;
use wq::repl::ReplInput;
use wq::repl::{ReplEngine, StdinError, VmEvaluator, stdin_add_history, stdin_readline};
use wq::value::WqResult;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wq::value::WqError;
use wq::value::box_mode;

use colored::Colorize;
use rand::Rng;

use crate::load_resolver::{parse_load_filename, repl_load_script};

fn main() {
    let args = env::args().skip(1);

    let mut debug_mode = false;

    let mut file: Option<String> = None;
    let mut format_file: Option<String> = None;
    let mut iter = args.peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                println!(
                    "{}",
                    create_boxed_text(
                        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/doc/usage.txt")),
                        2,
                    )
                );
                return;
            }
            "--version" | "-v" => {
                println!("wq {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "--format" | "-f" => {
                if let Some(p) = iter.next() {
                    format_file = Some(p);
                } else {
                    eprintln!("usage: wq -f <script>");
                    std::process::exit(1);
                }
            }
            "--debug" | "-d" => debug_mode = true,
            _ => {
                if file.is_none() {
                    file = Some(arg);
                } else {
                    eprintln!("Unexpected argument: {arg}");
                    std::process::exit(1);
                }
            }
        }
    }

    // Handle formatting request
    if let Some(file) = format_file {
        format_script_print(&file);
        return;
    }
    // Handle command line script execution
    if let Some(file) = file {
        exec_script(&file, debug_mode);
        return;
    }

    println!(
        "{} {}",
        format!("wq {} (c) tttiw (l) mit", env!("CARGO_PKG_VERSION")).magenta(),
        "help | quit".green()
    );

    let mut evaluator: Box<dyn ReplEngine> = Box::new(VmEvaluator::new());

    evaluator.set_debug(debug_mode);
    evaluator.set_stdin(Box::new(RustylineInput::new().unwrap()));
    let mut line_number = 1;
    let mut buffer = String::new();

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
                        "quit" | "exit" | "\\q" => {
                            system_msg_printer::stdout(
                                "bye..".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            break;
                        }
                        "bye" => {
                            system_msg_printer::stdout(
                                "bye".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            break;
                        }
                        cmd if cmd.starts_with("help") || cmd.starts_with("\\h") => {
                            let arg = if let Some(rest) = cmd.strip_prefix("help") {
                                rest.trim()
                            } else if let Some(rest) = cmd.strip_prefix("\\h") {
                                rest.trim()
                            } else {
                                ""
                            };

                            if arg.is_empty() {
                                println!(
                                    "{}",
                                    create_boxed_text(
                                        include_str!(concat!(
                                            env!("CARGO_MANIFEST_DIR"),
                                            "/doc/refcard.txt"
                                        )),
                                        2,
                                    )
                                );
                            } else if let Some(text) = builtins_help::get_builtin_help(arg) {
                                println!("{}", create_boxed_text(text, 2));
                            } else {
                                system_msg_printer::stderr(
                                    format!("no help available for '{arg}'"),
                                    system_msg_printer::MsgType::Error,
                                );
                            }
                            continue;
                        }
                        "vars" | "\\v" => {
                            match evaluator.get_environment() {
                                Some(env) => {
                                    system_msg_printer::stdout(
                                        "user-defined bindings:".to_string(),
                                        system_msg_printer::MsgType::Info,
                                    );
                                    for (key, value) in env {
                                        system_msg_printer::stdout(
                                            format!("{key}: {value}"),
                                            system_msg_printer::MsgType::Info,
                                        );
                                    }
                                }
                                None => system_msg_printer::stdout(
                                    "no user-defined bindings".to_string(),
                                    system_msg_printer::MsgType::Info,
                                ),
                            }
                            continue;
                        }
                        "clear" | "\\c" => {
                            evaluator.clear_environment();
                            system_msg_printer::stdout(
                                "user-defined bindings cleared".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        "box" | "\\b" => {
                            if box_mode::is_boxed() {
                                system_msg_printer::stdout(
                                    "boxed display is now off".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                                box_mode::set_boxed(false)
                            } else {
                                system_msg_printer::stdout(
                                    "boxed display is now on".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                                box_mode::set_boxed(true)
                            }
                            continue;
                        }
                        "box?" => {
                            if box_mode::is_boxed() {
                                system_msg_printer::stdout(
                                    "boxed display is on".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                            } else {
                                system_msg_printer::stdout(
                                    "boxed display is off".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                            continue;
                        }
                        "debug" | "\\d" => {
                            if evaluator.is_debug() {
                                system_msg_printer::stdout(
                                    "debug mode is off".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                                evaluator.set_debug(false)
                            } else {
                                system_msg_printer::stdout(
                                    "debug mode is on".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                                evaluator.set_debug(true)
                            }
                            continue;
                        }
                        "debug?" => {
                            if evaluator.is_debug() {
                                system_msg_printer::stdout(
                                    "debug mode is on".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                            } else {
                                system_msg_printer::stdout(
                                    "debug mode is off".to_string(),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                            continue;
                        }
                        "bye!" | "goodbye" => {
                            let mut rng = rand::rng();
                            let mut stdout = stdout();
                            let frames = if rng.random_bool(0.5) {
                                [";D", ";D", ";D", ";D", ";)"]
                            } else {
                                [":D", ":D", ":D", ":D", ":)"]
                            };
                            print!("{}", "\u{258D} bye! ".cyan());
                            stdout.flush().unwrap();
                            thread::sleep(Duration::from_millis(250));
                            for &face in &frames {
                                print!("\r{}", format!("\u{258D} bye! {face}").cyan());
                                stdout.flush().unwrap();
                                thread::sleep(Duration::from_millis(300));
                            }
                            print!("\r{}", "\u{258D} bye!    ".cyan());
                            println!();
                            break;
                        }
                        cmd if cmd.starts_with("\\t") || cmd.starts_with("time ") => {
                            let src = if let Some(rest) = cmd.strip_prefix("\\t") {
                                rest.trim()
                            } else if let Some(rest) = cmd.strip_prefix("time ") {
                                rest.trim()
                            } else {
                                ""
                            };

                            if src.is_empty() {
                                system_msg_printer::stderr(
                                    "usage: time <expression>".to_string(),
                                    system_msg_printer::MsgType::Error,
                                );
                                continue;
                            }

                            let start = Instant::now();

                            match evaluator.eval_string(src) {
                                Ok(result) => {
                                    system_msg_printer::stdout(
                                        format!("{result}"),
                                        system_msg_printer::MsgType::Success,
                                    );
                                }
                                Err(error) => {
                                    system_msg_printer::stderr(
                                        format!("{error}"),
                                        system_msg_printer::MsgType::Error,
                                    );
                                }
                            }

                            let duration = start.elapsed();
                            system_msg_printer::stdout(
                                format!("time elapsed: {duration:?}"),
                                system_msg_printer::MsgType::Info,
                            );
                            continue;
                        }
                        cmd if cmd.starts_with("load ") || cmd.starts_with("\\l ") => {
                            if let Some(fname) = parse_load_filename(cmd) {
                                let mut loading = HashSet::new();
                                repl_load_script(
                                    &mut *evaluator,
                                    Path::new(fname),
                                    &mut loading,
                                    false,
                                );
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

                buffer.push_str(input);
                let attempt = evaluator.eval_string(buffer.trim());
                match attempt {
                    Ok(result) => {
                        system_msg_printer::stdout(
                            format!("{result}"),
                            system_msg_printer::MsgType::Success,
                        );
                        buffer.clear();
                        line_number += 1;
                    }
                    Err(error) => {
                        if matches!(&error, WqError::EofError(_)) {
                            buffer.push('\n');
                            continue;
                        } else {
                            system_msg_printer::stderr(
                                format!("{error}"),
                                system_msg_printer::MsgType::Error,
                            );
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

fn exec_script(path: &String, debug: bool) {
    let path = Path::new(path);
    let mut loading = HashSet::new();
    let mut visited = HashSet::new();
    let src = match load_resolver::expand_script(path, &mut loading, &mut visited) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Cannot load {}: {e}", path.display());
            return;
        }
    };
    let mut vm = VmEvaluator::new();
    vm.set_debug(debug);
    vm.set_stdin(Box::new(RustylineInput::new().unwrap()));
    match vm.eval_string(&src) {
        Ok(_) => {
            //if val != Value::Null {
            // println!("{val}");
            //}
        }
        Err(e) => eprintln!("Error executing script: {e}"),
    }
}

fn format_script_print(filename: &str) {
    match fs::read_to_string(filename) {
        Ok(content) => {
            let fmt = Formatter::new(FormatOptions::default());
            match fmt.format_script(&content) {
                Ok(out) => println!("{out}"),
                Err(err) => {
                    eprintln!("{err}");
                    std::process::exit(1);
                }
            }
        }
        Err(err) => {
            eprintln!("Cannot read {filename}: {err}");
            std::process::exit(1);
        }
    }
}

pub struct RustylineInput {
    rl: DefaultEditor,
}

impl RustylineInput {
    pub fn new() -> WqResult<Self> {
        Ok(Self {
            rl: DefaultEditor::new().map_err(|e| WqError::IoError(e.to_string()))?,
        })
    }
}

impl ReplInput for RustylineInput {
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
}

mod load_resolver {

    use super::*;

    pub fn parse_load_filename(line: &str) -> Option<&str> {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("load ") {
            Some(rest.trim())
        } else if let Some(rest) = trimmed.strip_prefix("\\l ") {
            Some(rest.trim())
        } else {
            None
        }
    }

    pub fn resolve_load_path(base: &Path, fname: &str) -> PathBuf {
        let path = Path::new(fname);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        }
    }

    pub fn expand_script(
        path: &Path,
        loading: &mut HashSet<PathBuf>,
        visited: &mut HashSet<PathBuf>,
    ) -> std::io::Result<String> {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !loading.insert(canonical.clone()) {
            // already in loading stack -> cycle
            println!("Cannot load {}: cycling", canonical.display());
            return Ok(String::new());
        }
        if visited.contains(&canonical) {
            loading.remove(&canonical);
            return Ok(String::new());
        }
        visited.insert(canonical.clone());
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                loading.remove(&canonical);
                return Err(e);
            }
        };
        let mut result = String::new();
        let parent = path.parent().unwrap_or_else(|| Path::new(""));
        for (i, line) in content.lines().enumerate() {
            if i == 0 && line.starts_with("#!") {
                continue;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            if let Some(fname) = parse_load_filename(trimmed) {
                let sub = resolve_load_path(parent, fname);
                result.push_str(&expand_script(&sub, loading, visited)?);
            } else {
                result.push_str(trimmed);
                result.push('\n');
            }
        }
        loading.remove(&canonical);
        Ok(result)
    }

    pub fn repl_load_script(
        evaluator: &mut dyn ReplEngine,
        path: &Path,
        loading: &mut HashSet<PathBuf>,
        silent: bool,
    ) {
        let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if loading.contains(&canonical) {
            system_msg_printer::stderr(
                format!("Cannot load {}: cycling", canonical.display()),
                system_msg_printer::MsgType::Error,
            );
            return;
        }
        loading.insert(canonical.clone());

        // record variables before loading
        let vars_before = evaluator.env_vars().clone();

        match fs::read_to_string(path) {
            Ok(content) => {
                let parent_dir = path.parent().unwrap_or_else(|| Path::new(""));
                let mut buffer = String::new();

                for (i, line) in content.lines().enumerate() {
                    if i == 0 && line.starts_with("#!") {
                        continue;
                    }
                    let line = line.trim();
                    if line.is_empty() || line.starts_with("//") {
                        continue;
                    }

                    if buffer.is_empty() {
                        if let Some(fname) = parse_load_filename(line) {
                            let sub_path = resolve_load_path(parent_dir, fname);
                            repl_load_script(evaluator, &sub_path, loading, false);
                            continue;
                        }
                    }

                    buffer.push_str(line);
                    match evaluator.eval_string(buffer.trim()) {
                        Ok(_) => {
                            buffer.clear();
                        }
                        Err(err) => {
                            if matches!(&err, WqError::EofError(_)) {
                                buffer.push('\n');
                                continue;
                            } else {
                                system_msg_printer::stderr(
                                    format!("Error in {}: {err}", path.display()),
                                    system_msg_printer::MsgType::Error,
                                );
                                return;
                            }
                        }
                    }
                }

                if !buffer.trim().is_empty() {
                    if let Err(err) = evaluator.eval_string(buffer.trim()) {
                        system_msg_printer::stderr(
                            format!("Error in {}: {err}", path.display()),
                            system_msg_printer::MsgType::Error,
                        );
                        return;
                    }
                }

                // Show newly introduced or overridden bindings
                if !silent {
                    let vars_after = evaluator.env_vars().clone();
                    let mut new_bindings = Vec::new();
                    let mut overridden = Vec::new();

                    for (name, value) in &vars_after {
                        match vars_before.get(name) {
                            None => new_bindings.push((name.clone(), value.clone())),
                            Some(before_val) if before_val != value => {
                                overridden.push((name.clone(), value.clone()));
                            }
                            _ => {}
                        }
                    }

                    if new_bindings.is_empty() && overridden.is_empty() {
                        system_msg_printer::stderr(
                            format!("no new bindings from {}", path.display()),
                            system_msg_printer::MsgType::Info,
                        );
                    } else {
                        if !new_bindings.is_empty() {
                            new_bindings.sort_by_key(|(n, _)| n.clone());
                            system_msg_printer::stderr(
                                format!("new bindings from {}:", path.display()),
                                system_msg_printer::MsgType::Info,
                            );
                            for (name, value) in new_bindings {
                                system_msg_printer::stderr(
                                    format!("  {name} = {value}"),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                        }

                        if !overridden.is_empty() {
                            overridden.sort_by_key(|(n, _)| n.clone());
                            system_msg_printer::stderr(
                                format!("overridden bindings from {}:", path.display()),
                                system_msg_printer::MsgType::Info,
                            );
                            for (name, value) in overridden {
                                system_msg_printer::stderr(
                                    format!("  {name} = {value}"),
                                    system_msg_printer::MsgType::Info,
                                );
                            }
                        }
                    }
                }
            }
            Err(error) => {
                system_msg_printer::stderr(
                    format!("Cannot load {}: {error}", path.display()),
                    system_msg_printer::MsgType::Error,
                );
            }
        }
        loading.remove(&canonical);
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
