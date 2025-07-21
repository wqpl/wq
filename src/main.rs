use wq::evaluator::Evaluator;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::time::Instant;

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

    loop {
        // Print prompt
        print!("{} {} ", line_number.to_string().blue(), "wq$".magenta());
        io::stdout().flush().unwrap();

        // Read input
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let input = input.trim();

                // Handle repl commands
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
                                println!("{result}");
                            }
                            Err(error) => {
                                eprintln!("Error: {error}");
                            }
                        }

                        let duration = start.elapsed();
                        println!("{}", format!("time elapsed: {duration:?}").cyan());
                        continue;
                    }
                    cmd if cmd.starts_with("load ") || cmd.starts_with("\\l ") => {
                        let filename = if cmd.starts_with("load ") {
                            &cmd[5..]
                        } else {
                            &cmd[3..]
                        };
                        load_script(&mut evaluator, filename.trim());
                        continue;
                    }
                    "" => {
                        // Empty line; continue
                        continue;
                    }
                    _ => {}
                }

                match evaluator.eval_string(input) {
                    Ok(result) => {
                        println!("{result}");
                    }
                    Err(error) => {
                        eprintln!("Error: {error}");
                    }
                }

                line_number += 1;
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
        +    -    *    /    %    :
        $[cond;tb;fb] W[cond;b1] N[n;b1]
        abs neg signum sqrt exp ln floor ceiling
        count first last reverse sum max min avg
        rand sin cos tan sinh cosh tanh
        til range take drop where distinct sort
        cat flatten and or not xor
        type string echo exec
        int float char(string) symbol bool
        list l:(1;2.5);l[0]
        func f:{[x;n]t:x;N[n-1;t:t*x];t};f[2;3;]
                                     required ^
        repl: \h   \v   \c    \l   \t   \b  \d    \q
              help vars clear load time box debug quit "#
    );
}

fn load_script(evaluator: &mut Evaluator, filename: &str) {
    // record variables before loading
    let vars_before = evaluator.environment().variables().clone();

    match fs::read_to_string(filename) {
        Ok(content) => {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("//") {
                    continue;
                }

                match evaluator.eval_string(line) {
                    Ok(_) => {} // Silent execution
                    Err(error) => {
                        eprintln!("Error in {filename}: {error}");
                        return;
                    }
                }
            }

            // Show newly introduced bindings
            // fixme: overridden bindings are not shown properly
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
                println!("{}:", "new bindings:".blue());
                for (name, value) in new_bindings {
                    println!("  {name} = {value}");
                }
            }
        }
        Err(error) => {
            eprintln!("Cannot load {filename}: {error}");
        }
    }
}

fn execute_script(filename: &str) {
    let mut evaluator = Evaluator::new();

    match fs::read_to_string(filename) {
        Ok(content) => {
            println!("Executing script: {filename}");

            for (line_num, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                print!("{}", format!("({}) ", line_num + 1).blue());
                io::stdout().flush().unwrap();

                match evaluator.eval_string(line) {
                    Ok(result) => {
                        println!("{result}");
                    }
                    Err(error) => {
                        println!("{error}");
                    }
                }
            }
        }
        Err(error) => {
            eprintln!("Cannot read {filename}: {error}");
            std::process::exit(1);
        }
    }
}
