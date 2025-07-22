use wq::evaluator::Evaluator;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::time::Instant;

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

    println!("{}", "wq (c) tttiw (l) mit".magenta());
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
                            println!("bye");
                            break;
                        }
                        "help" | "\\h" => {
                            show_help();
                            continue;
                        }
                        "vars" | "\\v" => {
                            evaluator.show_environment();
                            continue;
                        }
                        "clear" | "\\c" => {
                            evaluator.environment_mut().clear();
                            println!("Variables cleared");
                            continue;
                        }
                        "box" | "\\b" => {
                            if box_mode::is_boxed() {
                                println!("{}", format!("display mode is now default").cyan());
                                box_mode::set_boxed(false)
                            } else {
                                println!("{}", format!("display mode is now boxed").cyan());
                                box_mode::set_boxed(true)
                            }
                            continue;
                        }
                        "box?" => {
                            if box_mode::is_boxed() {
                                println!("{}", format!("display mode is now boxed").cyan());
                            } else {
                                println!("{}", format!("display mode is now default").cyan());
                            }
                            continue;
                        }
                        "debug" | "\\d" => {
                            if evaluator.is_debug() {
                                println!("{}", format!("debug mode is now off").cyan());
                                evaluator.set_debug(false)
                            } else {
                                println!("{}", format!("debug mode is now on").cyan());
                                evaluator.set_debug(true)
                            }
                            continue;
                        }
                        "debug?" => {
                            if evaluator.is_debug() {
                                println!("{}", format!("debug mode is on").cyan());
                            } else {
                                println!("{}", format!("debug mode is off").cyan());
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
                                println!("{}", "expected wq code".red());
                                continue;
                            }

                            let start = Instant::now();

                            match evaluator.eval_string(src) {
                                Ok(result) => {
                                    println!("\u{258D} {result}");
                                }
                                Err(error) => {
                                    eprintln!("{error}");
                                }
                            }

                            let duration = start.elapsed();
                            println!("{}", format!("\u{258D} time elapsed: {duration:?}").cyan());
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
                        println!("\u{258D} {result}");
                        buffer.clear();
                        line_number += 1;
                    }
                    Err(error) => {
                        if is_eof_error(&error) {
                            buffer.push('\n');
                            continue;
                        } else {
                            eprintln!("{error}");
                            buffer.clear();
                            line_number += 1;
                        }
                    }
                }
            }
            Err(error) => {
                eprintln!("Error reading input: {error}");
                break;
            }
        }
    }
}

fn show_help() {
    println!(
        "{}",
        r#"
        +    -    *    /    %    :    ,
                       assignment^ cat^
        =    ~    <    <=   >    >=
        ^eq  ^neq
        $[cond;tb;fb] W[cond;b1] N[n;b1]
        abs neg signum sqrt exp ln floor ceiling
        count first last reverse sum max min avg
        rand sin cos tan sinh cosh tanh
        til range take drop where distinct sort
        cat flatten and or not xor
        type string symbol echo exec
        ----------------------------------------
        int float char symbol bool list dict function
        lst:(1;2.5);lst[0] dct:(`a:1;`b:2.5);dct[`a]
        func f:{[x;n]t:x;N[n-1;t:t*x];t};f[2;3;]
                                      required^
        repl: \h   \v   \c    \l   \t   \b  \d    \q
              help vars clear load time box debug quit"#
    );
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
        eprintln!("Cannot load {}: cycling", canonical.display());
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
                            eprintln!("Error in {}: {err}", path.display());
                            return;
                        }
                    }
                }
            }

            if !buffer.trim().is_empty() {
                if let Err(err) = evaluator.eval_string(buffer.trim()) {
                    eprintln!("Error in {}: {err}", path.display());
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
                    println!("{}", "no new bindings".blue());
                } else {
                    new_bindings.sort_by_key(|(n, _)| (*n).clone());
                    println!("{}", "new bindings:".blue());
                    for (name, value) in new_bindings {
                        println!("  {name} = {value}");
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("Cannot load {}: {error}", path.display());
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
                            println!("Error at script {filename} line {line_num}: \n {err}");
                            buffer.clear();
                        }
                    }
                }
            }

            if !buffer.trim().is_empty() {
                if let Err(err) = evaluator.eval_string(buffer.trim()) {
                    println!("Error at script {filename}: \n {err}");
                }
            }
        }
        Err(error) => {
            eprintln!("Cannot read {filename}: {error}");
            std::process::exit(1);
        }
    }
}
