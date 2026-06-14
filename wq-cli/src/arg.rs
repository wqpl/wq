#![cfg(not(target_arch = "wasm32"))]

use std::ffi::OsString;
use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Style, Styles};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
use colored::Colorize;
pub use wqpl::display::{BoxPrintConfig, apply_box_spec};
use wqpl::session::dbglog::DebugLogFlags;

pub const DEFAULT_STACK_SIZE_MEBIBYTE: usize = 12;

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeFlags {
    pub wqdb: bool,
    pub wqdb_cmds: Vec<String>,
    pub dry: bool,
    pub bt: bool, // default: true
    pub debug_flags: DebugLogFlags,
    pub interpreter: Option<String>,
    pub builtins: Option<String>,
    pub print: bool,                // default: false
    pub stack_size_mebibyte: usize, // default: 12
    pub run_notebook: bool,         // default: false
    pub experimental: Vec<String>,
    pub box_print: BoxPrintConfig,
}

impl Default for RuntimeFlags {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeFlags {
    pub fn new() -> Self {
        Self {
            wqdb: false,
            wqdb_cmds: Vec::new(),
            dry: false,
            bt: true,
            debug_flags: DebugLogFlags::empty(),
            interpreter: None,
            builtins: None,
            print: false,
            stack_size_mebibyte: DEFAULT_STACK_SIZE_MEBIBYTE,
            run_notebook: false,
            experimental: Vec::new(),
            box_print: BoxPrintConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FmtOpts {
    pub nlcd: bool,
    pub olw: bool,
}

#[derive(Debug, Clone)]
pub enum ExecSource {
    Stdin,
    Inline(String),
}

#[derive(Debug, Clone)]
pub enum CliCommand {
    Repl,
    Script(PathBuf),
    Notebook(PathBuf, bool), // path, interactive
    Exec(ExecSource),
    Fmt {
        script: PathBuf,
        opts: FmtOpts,
    },
    Symbols {
        script: PathBuf,
        name: String,
    },
    Dap {
        script: Option<PathBuf>,
    },
    Help {
        no_pager: bool,
        topic: Option<String>,
        prefer_doc_topic: bool,
        fold_width: Option<usize>,
    },
}

const HELP_HEADER: Style = AnsiColor::Green.on_default().effects(Effects::BOLD);
const HELP_USAGE: Style = AnsiColor::Yellow.on_default().effects(Effects::BOLD);
const HELP_LITERAL: Style = AnsiColor::Cyan.on_default().effects(Effects::BOLD);
const HELP_PLACEHOLDER: Style = AnsiColor::Blue.on_default();

const HELP_STYLES: Styles = Styles::styled()
    .header(HELP_HEADER)
    .usage(HELP_USAGE)
    .literal(HELP_LITERAL)
    .placeholder(HELP_PLACEHOLDER)
    .error(AnsiColor::Red.on_default().effects(Effects::BOLD))
    .valid(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .invalid(AnsiColor::Yellow.on_default().effects(Effects::BOLD));

/// The wq Programming Language, https://wq-pl.com
///
/// Run an interactive wq REPL, wq scripts, or wq notebooks.
#[derive(Parser, Debug, Clone)]
#[command(
    name = "wq",
    version,
    author = "tttiw",
    disable_help_subcommand = true,
    styles = HELP_STYLES,
    help_template = "{name} {version}\n{author-with-newline}{about-with-newline}\n{usage-heading} {usage}\n\n{all-args}{after-help}"
)]
struct CliArgs {
    #[command(flatten)]
    runtime: RuntimeOpts,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Script or notebook to run (positional alternative to subcommands)
    script: Option<PathBuf>,
}

/// Global runtime options applicable to most wq commands.
#[derive(Parser, Debug, Clone)]
struct RuntimeOpts {
    /// Print the final evaluation result
    #[arg(short = 'p', long, global = true)]
    print: bool,

    /// Enable wqdb
    #[arg(short = 'w', long = "wqdb", global = true)]
    wqdb: bool,

    /// Pass a wqdb command (can be used repeatedly)
    #[arg(short = 'o', long = "wqdb-cmd", value_name = "CMD", global = true)]
    wqdb_cmds: Vec<String>,

    /// Read wqdb commands from a file
    #[arg(short = 's', long = "wqdb-script", value_name = "FILE", global = true)]
    wqdb_script: Option<PathBuf>,

    /// Dry run: parse but do not execute
    #[arg(long, global = true)]
    dry: bool,

    /// Specify builtins preset (all, constrained, pure, minimal)
    #[arg(short = 'b', long, value_name = "PRESET", global = true)]
    builtins: Option<String>,

    /// Disable backtrace. Necessary for certain experimental features.
    #[arg(long, global = true)]
    no_bt: bool,

    /// Enable experimental feature (can be used repeatedly)
    #[arg(long = "exp", value_name = "NAME", global = true)]
    exp: Vec<String>,

    /// Run a notebook non-interactively
    #[arg(long)]
    run_notebook: bool,

    /// Thread stack size in MiB (2-64)
    #[arg(long, value_name = "MiB", value_parser = parse_stack_size, global = true)]
    stack_size: Option<usize>,

    /// Select interpreter (vanilla, profiler, sample)
    #[arg(short = 'i', long, value_name = "NAME", global = true)]
    interpreter: Option<String>,

    /// Configure debug output flags [default: off; -d defaults to inst; +/-
    /// modifies]
    #[arg(
        short = 'd',
        long = "debug",
        value_name = "SPEC",
        default_missing_value = "1",
        num_args = 0..=1,
        allow_hyphen_values = true,
        action = ArgAction::Append,
        global = true
    )]
    debug: Vec<String>,

    /// Configure result display (box, xray, color; +/- modifies)
    #[arg(
        long = "box",
        value_name = "SPEC",
        allow_hyphen_values = true,
        global = true
    )]
    box_display: Option<String>,
}

#[derive(Subcommand, Debug, Clone)]
enum Commands {
    /// Format a wq script.
    ///
    /// Reads a wq source file and emits a formatted version.
    /// Supports newline-after-closing-delimiter mode and one-line-wizard mode.
    Fmt {
        /// Insert a newline after closing delimiters
        #[arg(long)]
        nlcd: bool,
        /// Enable one-line-wizard mode
        #[arg(long)]
        olw: bool,
        /// Path to the wq script to format
        script: PathBuf,
    },
    /// Execute inline wq code or read from stdin.
    ///
    /// Runs wq code directly without a file. If CODE is omitted or '-',
    /// reads from standard input.
    Exec {
        /// wq code to execute, or '-' to read from stdin
        code: Option<String>,
    },
    /// Show symbols exported by a wq script.
    ///
    /// Inspects the named script and lists symbols matching the given name.
    Symbols {
        /// Path to the wq script to inspect
        script: PathBuf,
        /// Symbol name or pattern to search for
        name: String,
    },
    /// Start a wq DAP server.
    ///
    /// Reads DAP messages from stdin and writes responses to stdout.
    Dap {
        /// Optional script path to launch (can also be provided via launch
        /// config)
        script: Option<PathBuf>,
    },
    /// Show wq command help or language reference docs.
    Help {
        /// Print directly instead of using $PAGER for reference docs
        #[arg(long)]
        no_pager: bool,
        /// Fold reference docs at this many columns (default: terminal width)
        #[arg(long, value_name = "COLS", value_parser = parse_fold_width)]
        fold_width: Option<usize>,
        /// Reference topic to resolve directly, even when it matches a command
        #[arg(long = "topic", value_name = "TOPIC", conflicts_with = "topic")]
        doc_topic: Option<String>,
        /// Command, builtin, keyword, or syntax topic
        topic: Option<String>,
    },
}

fn parse_stack_size(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n) if (2..=64).contains(&n) => Ok(n),
        Ok(n) => Err(format!("value {n} out of range (2-64 MiB)")),
        Err(_) => Err(format!("invalid value: {s}")),
    }
}

fn parse_fold_width(s: &str) -> Result<usize, String> {
    match s.parse::<usize>() {
        Ok(n) if n > 0 => Ok(n),
        Ok(_) => Err("value must be at least 1".to_string()),
        Err(_) => Err(format!("invalid value: {s}")),
    }
}

fn print_debug_help() {
    let token = "token".red();
    let cst = "cst".cyan();
    let ast = "ast".yellow();
    let ast_v = "ast-v".bright_yellow();
    let inst = "inst".green();
    let inst_v = "inst-v".bright_green();
    let wqdb_1 = "wqdb".magenta();
    let wqdb_2 = "wqdb-v".bright_magenta();
    let value = "value".yellow();
    let cas = "cas".yellow();
    let cas_v = "cas-v".yellow();

    println!();
    println!("{}", "Debug flags".bold().underline());
    println!(
        "  {token}, {cst}, {ast}, {ast_v}, {inst}, {inst_v}, {wqdb_1}, {wqdb_2}, {value}, {cas}, {cas_v}"
    );

    println!();
    println!("{}", "Debug aliases".bold().underline());
    println!(
        "  0=off 1={inst} 2={inst},{ast} 3={inst},{ast},{value} 4={inst},{ast},{value},{inst_v},{ast_v}"
    );
}

fn print_runtime_help() {
    println!();
    println!("{}", "Interpreters".bold().underline());
    println!("  vanilla, profiler, sample");

    println!();
    println!("{}", "Builtins".bold().underline());
    println!("  all, constrained, pure, minimal");

    println!();
    println!("{}", "Exit Codes".bold().underline());
    println!("  0  Success");
    println!("  2  Incorrect Usage");
}

fn print_examples_top() {
    println!();
    println!("{}", "Examples".bold().underline());
    println!("  1. Run a script:");
    println!("     {}", "wq /path/to/script.wq".cyan());
    println!("  2. Run a notebook interactively:");
    println!("     {}", "wq /path/to/notebook.wq.md".cyan());
    println!("  3. Run a notebook non-interactively:");
    println!(
        "     {}",
        "wq --run-notebook /path/to/notebook.wq.md".cyan()
    );
    println!("  4. Run code & dump instructions & print final evaluation:");
    println!("     {}", "wq exec \"echo(1+1)\" -d1 -p".cyan());
    println!("  5. Ditto + dump AST + profiler:");
    println!("     {}", "wq exec \"echo(1+1)\" -i p -d2 -p".cyan());
    println!("  6. Format a script:");
    println!("     {}", "wq fmt script.wq".cyan());
    println!("  7. Debug a script with wqdb:");
    println!("     {}", "wq -w -o bt -o c script.wq".cyan());
}

fn print_after_help() {
    print_debug_help();
    print_runtime_help();
    print_examples_top();
}

fn print_after_help_exec() {
    print_debug_help();
    print_runtime_help();
    println!();
    println!("{}", "Examples".bold().underline());
    println!("  1. Execute inline code:");
    println!("     {}", "wq exec \"echo(1+1)\"".cyan());
    println!("  2. Execute from stdin:");
    println!("     {}", "echo '1+1' | wq exec -".cyan());
    println!("  3. Execute with debug output and print result:");
    println!("     {}", "wq exec \"echo(1+1)\" -d1 -p".cyan());
    println!("  4. Execute with AST dump and profiler:");
    println!("     {}", "wq exec \"echo(1+1)\" -i p -d2 -p".cyan());
}

/// Parse args (excluding argv[0]).
///
/// Non-succeed cases (help, version, parse errors) are handled internally:
/// messages are printed directly and an `Err(exit_code)` is returned.
pub fn parse_args<I>(args: I, silent: bool) -> Result<(RuntimeFlags, CliCommand), i32>
where
    I: IntoIterator<Item = OsString>,
{
    let mut argv = vec![OsString::from("wq")];
    argv.extend(args);
    let cli = match CliArgs::try_parse_from(&argv) {
        Ok(c) => c,
        Err(e) => {
            let kind = e.kind();
            if !silent {
                let _ = e.print();
            }
            match kind {
                clap::error::ErrorKind::DisplayHelp => {
                    let is_long_help = argv.iter().any(|a| a.to_str() == Some("--help"))
                        || argv.iter().any(|a| a.to_str() == Some("help"));
                    if is_long_help && !silent {
                        let is_subcommand_help = argv.iter().any(|a| {
                            matches!(a.to_str(), Some("fmt" | "exec" | "symbols" | "help"))
                        });
                        if !is_subcommand_help {
                            print_after_help();
                        } else if argv.iter().any(|a| a.to_str() == Some("exec")) {
                            print_after_help_exec();
                        }
                    }
                    return Err(0);
                }
                clap::error::ErrorKind::DisplayVersion => return Err(0),
                _ => return Err(2),
            }
        }
    };

    let mut rt = RuntimeFlags::new();
    rt.print = cli.runtime.print;
    rt.wqdb = cli.runtime.wqdb;
    rt.wqdb_cmds = cli.runtime.wqdb_cmds;
    if let Some(path) = cli.runtime.wqdb_script {
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                for line in content.lines() {
                    let line = line.trim();
                    if !line.is_empty() && !line.starts_with("//") {
                        rt.wqdb_cmds.push(line.to_string());
                    }
                }
            }
            Err(e) => {
                let err = CliArgs::command().error(
                    clap::error::ErrorKind::InvalidValue,
                    format!("cannot read wqdb script '{}': {e}", path.display()),
                );
                if !silent {
                    let _ = err.print();
                }
                return Err(2);
            }
        }
    }
    rt.dry = cli.runtime.dry;
    rt.builtins = cli.runtime.builtins;
    if cli.runtime.no_bt {
        rt.bt = false;
    }
    for item in cli.runtime.exp {
        for part in item.split(',') {
            let part = part.trim();
            if !part.is_empty() {
                rt.experimental.push(part.to_string());
            }
        }
    }
    rt.stack_size_mebibyte = cli
        .runtime
        .stack_size
        .unwrap_or(DEFAULT_STACK_SIZE_MEBIBYTE);
    rt.interpreter = cli.runtime.interpreter;
    for spec in cli.runtime.debug {
        let spec = if spec == "--" { "1" } else { &spec };
        if let Err(message) = rt.debug_flags.apply_spec(spec) {
            let err = CliArgs::command().error(clap::error::ErrorKind::InvalidValue, message);
            if !silent {
                let _ = err.print();
            }
            return Err(2);
        }
    }
    rt.run_notebook = cli.runtime.run_notebook;
    if let Some(spec) = cli.runtime.box_display
        && let Err(message) = apply_box_spec(&mut rt.box_print, &spec)
    {
        let err = CliArgs::command().error(clap::error::ErrorKind::InvalidValue, message);
        if !silent {
            let _ = err.print();
        }
        return Err(2);
    }

    let cmd = if let Some(sub) = cli.command {
        match sub {
            Commands::Fmt { nlcd, olw, script } => {
                if rt != RuntimeFlags::new() {
                    let err = CliArgs::command().error(
                        clap::error::ErrorKind::InvalidValue,
                        "fmt: runtime flags are not supported",
                    );
                    if !silent {
                        let _ = err.print();
                    }
                    return Err(2);
                }
                CliCommand::Fmt {
                    script,
                    opts: FmtOpts { nlcd, olw },
                }
            }
            Commands::Exec { code } => match code.as_deref() {
                None | Some("-") => CliCommand::Exec(ExecSource::Stdin),
                Some(s) => CliCommand::Exec(ExecSource::Inline(s.to_string())),
            },
            Commands::Symbols { script, name } => CliCommand::Symbols { script, name },
            Commands::Dap { script } => {
                if rt != RuntimeFlags::new() {
                    let err = CliArgs::command().error(
                        clap::error::ErrorKind::InvalidValue,
                        "dap: runtime flags are not supported",
                    );
                    if !silent {
                        let _ = err.print();
                    }
                    return Err(2);
                }
                CliCommand::Dap { script }
            }
            Commands::Help {
                no_pager,
                fold_width,
                topic,
                doc_topic,
            } => CliCommand::Help {
                no_pager,
                prefer_doc_topic: doc_topic.is_some(),
                topic: doc_topic.or(topic),
                fold_width,
            },
        }
    } else if let Some(path) = cli.script {
        if path.extension().is_some_and(|e| e == "md") {
            CliCommand::Notebook(path, !rt.run_notebook)
        } else {
            CliCommand::Script(path)
        }
    } else {
        CliCommand::Repl
    };

    Ok((rt, cmd))
}

pub fn render_cli_help(topic: Option<&str>) -> Option<String> {
    match topic {
        None => {
            let mut command = CliArgs::command();
            let mut text = render_command_help(&mut command);
            text.push_str(&after_help_text());
            Some(text)
        }
        Some(topic) => {
            let mut command = CliArgs::command();
            command.build();
            let mut sub = command
                .find_subcommand(topic)?
                .clone()
                .bin_name(format!("wq {topic}"));
            let mut text = render_command_help(&mut sub);
            if topic == "exec" {
                text.push_str(&after_help_exec_text());
            }
            Some(text)
        }
    }
}

fn render_command_help(command: &mut clap::Command) -> String {
    let mut buf = Vec::new();
    command
        .write_long_help(&mut buf)
        .expect("rendering clap help should not fail");
    String::from_utf8(buf).expect("clap help should be utf-8")
}

fn after_help_text() -> String {
    let mut out = String::new();
    out.push_str("\nDebug flags\n");
    out.push_str("  token, cst, ast, ast-v, inst, inst-v, wqdb, wqdb-v, value, cas, cas-v\n");
    out.push_str("\nDebug aliases\n");
    out.push_str("  0=off 1=inst 2=inst,ast 3=inst,ast,value 4=inst,ast,value,inst-v,ast-v\n");
    out.push_str(&runtime_help_text());
    out.push_str("\nExamples\n");
    out.push_str("  1. Run a script:\n");
    out.push_str("     wq /path/to/script.wq\n");
    out.push_str("  2. Run a notebook interactively:\n");
    out.push_str("     wq /path/to/notebook.wq.md\n");
    out.push_str("  3. Run a notebook non-interactively:\n");
    out.push_str("     wq --run-notebook /path/to/notebook.wq.md\n");
    out.push_str("  4. Run code & dump instructions & print final evaluation:\n");
    out.push_str("     wq exec \"echo(1+1)\" -d1 -p\n");
    out.push_str("  5. Ditto + dump AST + profiler:\n");
    out.push_str("     wq exec \"echo(1+1)\" -i p -d2 -p\n");
    out.push_str("  6. Format a script:\n");
    out.push_str("     wq fmt script.wq\n");
    out.push_str("  7. Debug a script with wqdb:\n");
    out.push_str("     wq -w -o bt -o c script.wq\n");
    out
}

fn after_help_exec_text() -> String {
    let mut out = String::new();
    out.push_str("\nDebug flags\n");
    out.push_str("  token, cst, ast, ast-v, inst, inst-v, wqdb, wqdb-v, value, cas, cas-v\n");
    out.push_str("\nDebug aliases\n");
    out.push_str("  0=off 1=inst 2=inst,ast 3=inst,ast,value 4=inst,ast,value,inst-v,ast-v\n");
    out.push_str(&runtime_help_text());
    out.push_str("\nExamples\n");
    out.push_str("  1. Execute inline code:\n");
    out.push_str("     wq exec \"echo(1+1)\"\n");
    out.push_str("  2. Execute from stdin:\n");
    out.push_str("     echo '1+1' | wq exec -\n");
    out.push_str("  3. Execute with debug output and print result:\n");
    out.push_str("     wq exec \"echo(1+1)\" -d1 -p\n");
    out.push_str("  4. Execute with AST dump and profiler:\n");
    out.push_str("     wq exec \"echo(1+1)\" -i p -d2 -p\n");
    out
}

fn runtime_help_text() -> String {
    let mut out = String::new();
    out.push_str("\nInterpreters\n");
    out.push_str("  vanilla, profiler, sample\n");
    out.push_str("\nBuiltins\n");
    out.push_str("  all, constrained, pure, minimal\n");
    out.push_str("\nExit Codes\n");
    out.push_str("  0  Success\n");
    out.push_str("  2  Incorrect Usage\n");
    out
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn parse_args<I>(args: I) -> Result<(RuntimeFlags, CliCommand), i32>
    where
        I: IntoIterator<Item = OsString>,
    {
        super::parse_args(args, true)
    }

    fn v(xs: &[&str]) -> Vec<OsString> {
        xs.iter().map(OsString::from).collect()
    }

    fn ok(p: Result<(RuntimeFlags, CliCommand), i32>) -> (RuntimeFlags, CliCommand) {
        p.unwrap()
    }

    fn is_err(p: Result<(RuntimeFlags, CliCommand), i32>) -> i32 {
        p.unwrap_err()
    }

    #[test]
    fn help_and_version_short_circuit() {
        assert_eq!(is_err(parse_args(v(&["--help"]))), 0);
        assert_eq!(is_err(parse_args(v(&["--version"]))), 0);
        assert_eq!(is_err(parse_args(v(&["foo.wq", "-h"]))), 0);
    }

    #[test]
    fn help_topic_flag_prefers_doc_topic() {
        let (_, cmd) = ok(parse_args(v(&["help", "--topic", "exec"])));
        match cmd {
            CliCommand::Help {
                no_pager,
                topic,
                prefer_doc_topic,
                fold_width,
            } => {
                assert!(!no_pager);
                assert_eq!(topic.as_deref(), Some("exec"));
                assert!(prefer_doc_topic);
                assert_eq!(fold_width, None);
            }
            _ => panic!("expected Help"),
        }
        assert_eq!(
            is_err(parse_args(v(&["help", "exec", "--topic", "fmt"]))),
            2
        );
    }

    #[test]
    fn help_fold_width_parses() {
        let (_, cmd) = ok(parse_args(v(&[
            "help",
            "--topic",
            "map",
            "--fold-width",
            "72",
        ])));
        match cmd {
            CliCommand::Help {
                no_pager,
                topic,
                prefer_doc_topic,
                fold_width,
            } => {
                assert!(!no_pager);
                assert_eq!(topic.as_deref(), Some("map"));
                assert!(prefer_doc_topic);
                assert_eq!(fold_width, Some(72));
            }
            _ => panic!("expected Help"),
        }
        assert_eq!(
            is_err(parse_args(v(&["help", "map", "--fold-width", "0"]))),
            2
        );
    }

    #[test]
    fn script_and_extras_error() {
        let (_, cmd) = ok(parse_args(v(&["main.wq"])));
        assert!(matches!(cmd, CliCommand::Script(_)));
        assert_eq!(is_err(parse_args(v(&["main.wq", "extra", "stuff"]))), 2);
    }

    #[test]
    fn fmt_happy_path_and_mode_errors() {
        let (_, cmd) = ok(parse_args(v(&["fmt", "--nlcd", "f.wq"])));
        match cmd {
            CliCommand::Fmt { script, opts } => {
                assert_eq!(script, PathBuf::from("f.wq"));
                assert!(opts.nlcd);
                assert!(!opts.olw);
            }
            _ => panic!("expected Fmt"),
        }
        assert_eq!(is_err(parse_args(v(&["fmt"]))), 2);
        assert_eq!(is_err(parse_args(v(&["fmt", "f.wq", "extra"]))), 2);
        assert_eq!(is_err(parse_args(v(&["fmt", "-d3", "f.wq"]))), 2);
    }

    #[test]
    fn exec_inline_stdin_and_extras() {
        let (_, cmd) = ok(parse_args(v(&["exec", "1+1"])));
        assert!(matches!(cmd, CliCommand::Exec(ExecSource::Inline(_))));
        let (_, cmd) = ok(parse_args(v(&["exec", "-"])));
        assert!(matches!(cmd, CliCommand::Exec(ExecSource::Stdin)));
        assert_eq!(is_err(parse_args(v(&["exec", "1+1", "oops"]))), 2);
    }

    #[test]
    fn interpreter_flag_parses() {
        let (rt, _) = ok(parse_args(v(&["-i", "sample", "a.wq"])));
        assert_eq!(rt.interpreter.as_deref(), Some("sample"));
    }

    #[test]
    fn dry_flag_parses() {
        let (rt, _) = ok(parse_args(v(&["--dry", "a.wq"])));
        assert!(rt.dry);
    }

    #[test]
    fn box_flag_parses_display_spec() {
        let (rt, _) = ok(parse_args(v(&["a.wq"])));
        assert!(rt.box_print.boxed);
        assert!(!rt.box_print.xray);
        assert!(rt.box_print.axis);
        assert!(rt.box_print.color);
        assert_eq!(rt.box_print.summary(), "[box,axis,color]");

        let (rt, _) = ok(parse_args(v(&["--box", "xray", "a.wq"])));
        assert!(!rt.box_print.boxed);
        assert!(rt.box_print.xray);
        assert!(!rt.box_print.axis);
        assert!(!rt.box_print.color);
        assert_eq!(rt.box_print.summary(), "[xray]");

        let (rt, _) = ok(parse_args(v(&["--box", "+xray,-color", "a.wq"])));
        assert!(rt.box_print.boxed);
        assert!(rt.box_print.xray);
        assert!(rt.box_print.axis);
        assert!(!rt.box_print.color);

        let (rt, _) = ok(parse_args(v(&["--box", "box,color", "a.wq"])));
        assert!(rt.box_print.boxed);
        assert!(!rt.box_print.xray);
        assert!(!rt.box_print.axis);
        assert!(rt.box_print.color);

        let (rt, _) = ok(parse_args(v(&["--box", "-box", "a.wq"])));
        assert!(!rt.box_print.boxed);
        assert!(!rt.box_print.xray);
        assert!(rt.box_print.axis);
        assert!(rt.box_print.color);
        assert_eq!(rt.box_print.summary(), "[axis,color]");
        assert_eq!(is_err(parse_args(v(&["--box", "sparkle", "a.wq"]))), 2);

        let mut config = BoxPrintConfig::default();
        config.toggle_box();
        assert_eq!(config.summary(), "[]");
        config.toggle_box();
        assert_eq!(config.summary(), "[box,axis,color]");
        apply_box_spec(&mut config, "box").unwrap();
        assert_eq!(config.summary(), "[box]");
        config.toggle_box();
        assert_eq!(config.summary(), "[]");
        config.toggle_box();
        assert_eq!(config.summary(), "[box,axis,color]");
    }

    #[test]
    fn debug_forms_and_last_wins() {
        let (rt, _) = ok(parse_args(v(&["-d3", "a.wq"])));
        assert_eq!(rt.debug_flags, DebugLogFlags::from_alias(3).unwrap());
        assert_eq!(is_err(parse_args(v(&["-d", "7", "a.wq"]))), 2);
        // clap consumes the value after -d, so use -d1 or separate with --
        let (rt, _) = ok(parse_args(v(&["-d1", "a.wq"])));
        assert_eq!(rt.debug_flags, DebugLogFlags::from_alias(1).unwrap());
        let (rt, _) = ok(parse_args(v(&["-d", "--", "a.wq"])));
        assert_eq!(rt.debug_flags, DebugLogFlags::from_alias(1).unwrap());
        assert_eq!(is_err(parse_args(v(&["-d1", "-d9", "a.wq"]))), 2);
    }

    #[test]
    fn debug_names_parse() {
        let (rt, _) = ok(parse_args(v(&["--debug", "token,cst,inst,wqdb", "a.wq"])));
        let expected = DebugLogFlags::from_names(["token", "cst", "inst", "wqdb"]);
        assert_eq!(rt.debug_flags, expected);
    }

    #[test]
    fn debug_modifier_specs_apply_in_order() {
        let (rt, _) = ok(parse_args(v(&[
            "-d1",
            "--debug",
            "+ast,+value",
            "--debug",
            "-inst",
            "a.wq",
        ])));
        let expected = DebugLogFlags::from_names(["ast", "value"]);
        assert_eq!(rt.debug_flags, expected);

        let (rt, _) = ok(parse_args(v(&["-d4", "--debug", "-ast", "a.wq"])));
        let expected = DebugLogFlags::from_names(["inst", "inst-v", "value"]);
        assert_eq!(rt.debug_flags, expected);

        let (rt, _) = ok(parse_args(v(&["-d4", "--debug", "cas", "a.wq"])));
        let expected = DebugLogFlags::from_names(["cas"]);
        assert_eq!(rt.debug_flags, expected);

        let (rt, _) = ok(parse_args(v(&["-d1", "-d", "-inst", "a.wq"])));
        assert_eq!(rt.debug_flags, DebugLogFlags::empty());
    }

    #[test]
    fn dashdash_stops_flag_parsing() {
        assert_eq!(
            is_err(parse_args(v(&["-d", "2", "--", "-file", "rest"]))),
            2
        );
    }

    #[test]
    fn repl_default_when_empty() {
        let (_, cmd) = ok(parse_args(v(&[])));
        assert!(matches!(cmd, CliCommand::Repl));
    }

    #[test]
    fn unknown_flags_are_reported() {
        assert_eq!(is_err(parse_args(v(&["--bogus", "a.wq"]))), 2);
    }

    #[test]
    fn stack_size_parses() {
        let (rt, _) = ok(parse_args(v(&["--stack-size", "16", "a.wq"])));
        assert_eq!(rt.stack_size_mebibyte, 16);
        let (rt, _) = ok(parse_args(v(&["--stack-size", "2", "a.wq"])));
        assert_eq!(rt.stack_size_mebibyte, 2);
        let (rt, _) = ok(parse_args(v(&["--stack-size", "48", "a.wq"])));
        assert_eq!(rt.stack_size_mebibyte, 48);
    }

    #[test]
    fn stack_size_missing_errors() {
        assert_eq!(is_err(parse_args(v(&["--stack-size"]))), 2);
    }

    #[test]
    fn notebook_route_by_extension() {
        let (_, cmd) = ok(parse_args(v(&["notes.md"])));
        match cmd {
            CliCommand::Notebook(path, interactive) => {
                assert_eq!(path, PathBuf::from("notes.md"));
                assert!(interactive);
            }
            _ => panic!("expected Notebook"),
        }
    }

    #[test]
    fn notebook_non_interactive_flag() {
        let (_, cmd) = ok(parse_args(v(&["--run-notebook", "notes.md"])));
        match cmd {
            CliCommand::Notebook(path, interactive) => {
                assert_eq!(path, PathBuf::from("notes.md"));
                assert!(!interactive);
            }
            _ => panic!("expected Notebook"),
        }
    }

    #[test]
    fn notebook_flag_ignored_for_wq_files() {
        let (_, cmd) = ok(parse_args(v(&["--run-notebook", "script.wq"])));
        match cmd {
            CliCommand::Script(path) => {
                assert_eq!(path, PathBuf::from("script.wq"));
            }
            _ => panic!("expected Script"),
        }
    }

    #[test]
    fn wqdb_flag_enables_debugger() {
        let (rt, _) = ok(parse_args(v(&["-w", "a.wq"])));
        assert!(rt.wqdb);
        assert!(rt.wqdb_cmds.is_empty());
    }

    #[test]
    fn wqdb_cmd_flag_parses() {
        let (rt, _) = ok(parse_args(v(&["-w", "-o", "bt", "a.wq"])));
        assert!(rt.wqdb);
        assert_eq!(rt.wqdb_cmds, vec!["bt"]);
    }

    #[test]
    fn wqdb_cmd_flag_multiple() {
        let (rt, _) = ok(parse_args(v(&["-w", "-o", "bt", "-o", "c", "a.wq"])));
        assert!(rt.wqdb);
        assert_eq!(rt.wqdb_cmds, vec!["bt", "c"]);
    }

    #[test]
    fn wqdb_cmd_flag_long_form() {
        let (rt, _) = ok(parse_args(v(&[
            "--wqdb",
            "--wqdb-cmd",
            "bt",
            "--wqdb-cmd",
            "c",
            "a.wq",
        ])));
        assert!(rt.wqdb);
        assert_eq!(rt.wqdb_cmds, vec!["bt", "c"]);
    }

    #[test]
    fn wqdb_does_not_interfere_with_subcommands() {
        let (rt, cmd) = ok(parse_args(v(&["-w", "exec", "1+1"])));
        assert!(rt.wqdb);
        assert!(rt.wqdb_cmds.is_empty());
        assert!(matches!(cmd, CliCommand::Exec(ExecSource::Inline(_))));

        assert_eq!(is_err(parse_args(v(&["-w", "fmt", "f.wq"]))), 2);
        assert_eq!(is_err(parse_args(v(&["-w", "dap"]))), 2);
        assert_eq!(is_err(parse_args(v(&["--dry", "dap"]))), 2);
    }

    #[test]
    fn wqdb_script_flag_reads_file() {
        let tmp = std::env::temp_dir().join("wqdb_test_cmds.txt");
        std::fs::write(&tmp, "bt\nc\n// comment\n\ninfo\n").unwrap();
        let (rt, _) = ok(parse_args(v(&["-w", "-s", tmp.to_str().unwrap(), "a.wq"])));
        assert!(rt.wqdb);
        assert_eq!(rt.wqdb_cmds, vec!["bt", "c", "info"]);
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn wqdb_script_flag_missing_file_errors() {
        assert_eq!(
            is_err(parse_args(v(&[
                "-w",
                "-s",
                "/nonexistent/wqdb.txt",
                "a.wq"
            ]))),
            2
        );
    }
}
