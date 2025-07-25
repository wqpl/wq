use wq::evaluator::Evaluator;
use wq::repl::ReplEngine;
use wq::tools::formatter::{FormatOptions, Formatter};
use wq::vm::{self, VmEvaluator};

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::env;
use std::fs;
use std::io::{Write, stdout};
use std::thread;
use std::time::Duration;
use std::time::Instant;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wq::value::box_mode;
use wq::value::valuei::WqError;

use colored::Colorize;
use rand::Rng;

use crate::load_resolver::{load_script, parse_load_filename, resolve_load_path};

fn main() {
    let args = env::args().skip(1);

    let mut use_bcvm = true;
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
            "--tree-walker" | "-t" => use_bcvm = false,
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
        if use_bcvm {
            vm::vm_exec_script(&file, debug_mode);
        } else {
            execute_script(&file);
        }
        return;
    }

    println!(
        "{} {}",
        format!("wq {} (c) tttiw (l) mit", env!("CARGO_PKG_VERSION")).magenta(),
        "help | quit".green()
    );

    let mut evaluator: Box<dyn ReplEngine> = if use_bcvm {
        Box::new(VmEvaluator::new())
    } else {
        Box::new(Evaluator::new())
    };
    evaluator.set_debug(debug_mode);
    let mut rl = DefaultEditor::new().unwrap();
    let mut line_number = 1;
    let mut buffer = String::new();

    loop {
        // Prompt construction
        let prompt = if buffer.is_empty() {
            format!("{} {} ", line_number.to_string().blue(), "wq$".magenta())
        } else {
            let indent = " ".repeat(line_number.to_string().len());
            format!("{} {} ", indent, "...".magenta())
        };

        // Read input
        let readline = rl.readline(&prompt);
        match readline {
            Ok(line) => {
                let input = line.trim_end();
                if !input.is_empty() {
                    rl.add_history_entry(input).unwrap();
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
                        "help" | "\\h" => {
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
                                load_script(&mut *evaluator, Path::new(fname), &mut loading, false);
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
            Err(ReadlineError::Interrupted) => {
                break;
            }
            Err(ReadlineError::Eof) => {
                let mut rng = rand::rng();
                let p = 0.006666f64;
                // let p = 1f64;
                if rng.random_bool(p) {
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
                    system_msg_printer::stdout(
                        "\rprogram \"wq\" terminated       ".to_string(),
                        system_msg_printer::MsgType::Info,
                    );
                }
                break;
            }
            Err(error) => {
                system_msg_printer::stderr(
                    format!("Error reading input: {error}"),
                    system_msg_printer::MsgType::Error,
                );
                break;
            }
        }
    }
}

fn execute_script(filename: &str) {
    let mut evaluator = Evaluator::new();

    match fs::read_to_string(filename) {
        Ok(content) => {
            let mut loading = HashSet::new();
            let mut buffer = String::new();
            for (line_num, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                if buffer.is_empty() {
                    if let Some(fname) = parse_load_filename(line) {
                        let base = Path::new(filename)
                            .parent()
                            .unwrap_or_else(|| Path::new(""));
                        let path = resolve_load_path(base, fname);
                        load_script(&mut evaluator, &path, &mut loading, true);
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
                                format!("Error at script {filename} line {line_num}: \n {err}"),
                                system_msg_printer::MsgType::Error,
                            );
                            buffer.clear();
                        }
                    }
                }
            }

            if !buffer.trim().is_empty() {
                if let Err(err) = evaluator.eval_string(buffer.trim()) {
                    system_msg_printer::stderr(
                        format!("Error at script {filename}: \n {err}"),
                        system_msg_printer::MsgType::Error,
                    );
                }
            }
        }
        Err(error) => {
            system_msg_printer::stderr(
                format!("Cannot read {filename}: {error}"),
                system_msg_printer::MsgType::Error,
            );
            std::process::exit(1);
        }
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

fn create_boxed_text(text: &str, padding: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let max_len = lines.iter().map(|line| line.len()).max().unwrap_or(0);
    let total_width = max_len + padding * 2;

    let top_border = format!("+{}+", "-".repeat(total_width));
    let bottom_border = top_border.clone();

    let mut result = String::new();
    result.push_str(&top_border);
    result.push('\n');

    for line in lines {
        let spaces = max_len - line.len();
        result.push_str(&format!(
            "|{}{}{}|\n",
            " ".repeat(padding),
            line,
            " ".repeat(padding + spaces)
        ));
    }

    result.push_str(&bottom_border);
    result
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

    pub fn load_script(
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

                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with("//") {
                        continue;
                    }

                    if buffer.is_empty() {
                        if let Some(fname) = parse_load_filename(line) {
                            let sub_path = resolve_load_path(parent_dir, fname);
                            load_script(evaluator, &sub_path, loading, false);
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

                // Show newly introduced bindings
                // fixme: overridden bindings are not shown properly
                if !silent {
                    let vars_after = evaluator.env_vars();
                    let mut new_bindings = Vec::new();

                    for (name, value) in vars_after {
                        if !vars_before.contains_key(name) {
                            new_bindings.push((name, value));
                        }
                    }

                    if new_bindings.is_empty() {
                        system_msg_printer::stderr(
                            format!("no new bindings from {}", path.display()),
                            system_msg_printer::MsgType::Info,
                        );
                    } else {
                        new_bindings.sort_by_key(|(n, _)| (*n).clone());
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
