use wq::evaluator::Evaluator;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::time::Instant;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wq::value::WqError;
use wq::value::box_mode;

use colored::Colorize;

fn main() {
    let args: Vec<String> = env::args().collect();

    // Handle command line script execution
    if args.len() > 1 {
        execute_script(&args[1]);
        return;
    }

    println!("{}", "wq 0.2.0 (c) tttiw (l) mit".magenta());
    println!("{}", "help | quit".green());

    let mut evaluator = Evaluator::new();
    let mut line_number = 1;
    let mut buffer = String::new();

    loop {
        // Print prompt
        if buffer.is_empty() {
            print!("{} {} ", line_number.to_string().blue(), "wq$".magenta());
        } else {
            let indent = " ".repeat(line_number.to_string().len());
            print!("{} {} ", indent, "...".magenta());
        }
        io::stdout().flush().unwrap();

        // Read input
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let input = input.trim_end();

                // Handle repl commands only if buffer is empty
                if buffer.is_empty() {
                    match input {
                        "quit" | "exit" | "\\q" => {
                            system_msg_printer::stdout(
                                "bye".to_string(),
                                system_msg_printer::MsgType::Info,
                            );
                            break;
                        }
                        "help" | "\\h" => {
                            show_help();
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
                                            format!("{}: {}", key, value),
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
                            evaluator.environment_mut().clear();
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
                                load_script(&mut evaluator, Path::new(fname), &mut loading, false);
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
                        if is_eof_error(&error) {
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

fn show_help() {
    println!(
        "{}",
        r#"
        +    -    *    /    %    :    ,    #
                       assignment^   cat count
        =    ~    <    <=   >    >=
        eq  neq
        $[cond;tb;fb]       $.[cond;tb1;tb2;...]
        W[cond;b1]          N[n;b1]
        abs neg signum sqrt exp ln floor ceiling
        count first last reverse sum max min avg
        rand sin cos tan sinh cosh tanh
        til range take drop where distinct sort
        cat flatten and or not xor
        type string symbol echo showt exec
        ----------------------------------------
        int float char symbol bool list dict function
        lst:(1;2.5);lst[0] dct:(`a:1;`b:2.5);dct[`a]
        func f:{[x;n]t:x;N[n-1;t:t*x];t};f[2;3;]
                                      required^
        repl: \h   \v   \c    \l   \t   \b  \d    \q
              help vars clear load time box debug quit"#
    );
}

fn parse_load_filename(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("load ") {
        Some(rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix("\\l ") {
        Some(rest.trim())
    } else {
        None
    }
}

fn is_eof_error(err: &WqError) -> bool {
    matches!(err, WqError::EofError(_))
}

fn resolve_load_path(base: &Path, fname: &str) -> PathBuf {
    let path = Path::new(fname);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn load_script(
    evaluator: &mut Evaluator,
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
    let vars_before = evaluator.environment().variables().clone();

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
                        if is_eof_error(&err) {
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
                let vars_after = evaluator.environment().variables();
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
                        if is_eof_error(&err) {
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
