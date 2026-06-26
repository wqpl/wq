#![cfg(not(target_arch = "wasm32"))]

use std::ffi::OsString;
use std::fmt::Write as _;
use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Effects, Style, Styles};
use clap::{ArgAction, CommandFactory, Parser, Subcommand};
pub use wqpl::display::{BoxPrintConfig, apply_box_spec};
use wqpl::session::dbglog::DebugLogFlags;
use wqpl::style::{AnsiColor as WqAnsiColor, ColorMode, TextStyle, paint};

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
            experimental: Vec::new(),
            box_print: BoxPrintConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FmtOpts {
    pub nlcd: bool,
    pub olw: bool,
    pub max_width: Option<usize>,
    pub wrap_only: bool,
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
    Markdown {
        path: PathBuf,
        no_pager: bool,
    },
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

const HELP_HEADER_EFFECTS: Effects = Effects::new()
    .insert(Effects::BOLD)
    .insert(Effects::UNDERLINE);

const HELP_HEADER: Style = AnsiColor::White.on_default().effects(HELP_HEADER_EFFECTS);

const HELP_LITERAL: Style = AnsiColor::BrightMagenta.on_default().effects(Effects::BOLD);
const HELP_PLACEHOLDER: Style = AnsiColor::Blue.on_default();
const HELP_ERROR: Style = AnsiColor::BrightRed.on_default().effects(Effects::BOLD);
const HELP_WARNING: Style = AnsiColor::BrightYellow.on_default().effects(Effects::BOLD);

const HELP_STYLES: Styles = Styles::styled()
    .header(HELP_HEADER)
    .usage(HELP_HEADER)
    .literal(HELP_LITERAL)
    .placeholder(HELP_PLACEHOLDER)
    .error(HELP_ERROR)
    .valid(HELP_LITERAL)
    .invalid(HELP_WARNING);

/// The wq Programming Language, https://wq-pl.com
///
/// Run an interactive wq REPL, wq scripts, or render Markdown docs.
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

    /// Script or Markdown file to run (positional alternative to subcommands)
    script: Option<PathBuf>,
}

/// Global options applicable to most wq commands.
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

    /// Print rendered help and Markdown files directly instead of using $PAGER
    #[arg(long, global = true)]
    no_pager: bool,

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

    /// Configure result display (on, off, box, axis, xray, color; +/- modifies)
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
    /// Supports newline-after-closing-delimiter, one-line-wizard, and wrap-only
    /// modes.
    Fmt {
        /// Insert a newline after closing delimiters
        #[arg(long)]
        nlcd: bool,
        /// Enable one-line-wizard mode
        #[arg(long)]
        olw: bool,
        /// Target line width for formatter decisions
        #[arg(long, value_name = "COLS", value_parser = parse_fold_width)]
        width: Option<usize>,
        /// Only insert parser-safe wrapping newlines
        #[arg(long, conflicts_with_all = ["nlcd", "olw"])]
        wrap_only: bool,
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

fn print_after_help() {
    print!("{}", after_help_text());
}

fn print_after_help_exec() {
    print!("{}", after_help_exec_text());
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
    let no_pager = cli.runtime.no_pager;
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
            Commands::Fmt {
                nlcd,
                olw,
                width,
                wrap_only,
                script,
            } => {
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
                    opts: FmtOpts {
                        nlcd,
                        olw,
                        max_width: width,
                        wrap_only,
                    },
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
            CliCommand::Markdown { path, no_pager }
        } else {
            CliCommand::Script(path)
        }
    } else {
        CliCommand::Repl
    };

    if no_pager && !matches!(&cmd, CliCommand::Markdown { .. } | CliCommand::Help { .. }) {
        let err = CliArgs::command().error(
            clap::error::ErrorKind::InvalidValue,
            "--no-pager only applies to Markdown files and `wq help`",
        );
        if !silent {
            let _ = err.print();
        }
        return Err(2);
    }

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
    write_common_appendix(&mut out);
    write_top_examples(&mut out);
    out
}

fn after_help_exec_text() -> String {
    let mut out = String::new();
    write_common_appendix(&mut out);
    write_exec_examples(&mut out);
    out
}

fn write_common_appendix(out: &mut String) {
    write_debug_help(out);
    write_runtime_help(out);
}

fn write_debug_help(out: &mut String) {
    let token = help_color("token", WqAnsiColor::Red);
    let cst = help_color("cst", WqAnsiColor::Cyan);
    let ast = help_color("ast", WqAnsiColor::Yellow);
    let ast_v = help_color("ast-v", WqAnsiColor::BrightYellow);
    let inst = help_color("inst", WqAnsiColor::Green);
    let inst_v = help_color("inst-v", WqAnsiColor::BrightGreen);
    let wqdb_1 = help_color("wqdb", WqAnsiColor::Magenta);
    let wqdb_2 = help_color("wqdb-v", WqAnsiColor::BrightMagenta);
    let value = help_color("value", WqAnsiColor::Yellow);
    let cas = help_color("cas", WqAnsiColor::Yellow);
    let cas_v = help_color("cas-v", WqAnsiColor::Yellow);

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Debug flags"));
    let _ = writeln!(
        out,
        "  {token}, {cst}, {ast}, {ast_v}, {inst}, {inst_v}, {wqdb_1}, {wqdb_2}, {value}, {cas}, {cas_v}"
    );

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Debug aliases"));
    let _ = writeln!(
        out,
        "  0=off 1={inst} 2={inst},{ast} 3={inst},{ast},{value} 4={inst},{ast},{value},{inst_v},{ast_v}"
    );
}

fn write_runtime_help(out: &mut String) {
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Interpreters"));
    let _ = writeln!(out, "  vanilla, profiler, sample");

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Builtins"));
    let _ = writeln!(out, "  all, constrained, pure, minimal");

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Exit Codes"));
    let _ = writeln!(out, "  0  Success");
    let _ = writeln!(out, "  1  Execution Error");
    let _ = writeln!(out, "  2  Incorrect Usage");
}

fn write_top_examples(out: &mut String) {
    let wq = help_color("wq", WqAnsiColor::BrightMagenta);

    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Examples"));
    let _ = writeln!(out, "  1. Run a script:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("script.wq", WqAnsiColor::Blue)
    );
    let _ = writeln!(out, "  2. Evaluate inline code and print the result:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("exec '1+1' -p", WqAnsiColor::Blue)
    );
    let _ = writeln!(out, "  3. Inspect AST and instructions for inline code:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("exec '1+1' -d ast,inst -p", WqAnsiColor::Blue)
    );
    let _ = writeln!(out, "  4. Format a script:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("fmt script.wq", WqAnsiColor::Blue)
    );
    let _ = writeln!(out, "  5. Debug a script with wqdb:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("-w -o bt -o c script.wq", WqAnsiColor::Blue)
    );
    let _ = writeln!(out, "  6. Render a Markdown note:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("notes.wq.md", WqAnsiColor::Blue)
    );
    let _ = writeln!(out, "  7. Render a Markdown note without a pager:");
    let _ = writeln!(
        out,
        "     {wq} {}",
        help_color("--no-pager notes.wq.md", WqAnsiColor::Blue)
    );
}

fn write_exec_examples(out: &mut String) {
    let _ = writeln!(out);
    let _ = writeln!(out, "{}", help_header("Examples"));
    let _ = writeln!(out, "  1. Evaluate inline code:");
    let _ = writeln!(
        out,
        "     {}",
        help_color("wq exec '1+1' -p", WqAnsiColor::Cyan)
    );
    let _ = writeln!(out, "  2. Read code from stdin:");
    let _ = writeln!(
        out,
        "     {}",
        help_color("echo '1+1' | wq exec - -p", WqAnsiColor::Cyan)
    );
    let _ = writeln!(out, "  3. Dump AST and instructions:");
    let _ = writeln!(
        out,
        "     {}",
        help_color("wq exec '1+1' -d ast,inst -p", WqAnsiColor::Cyan)
    );
    let _ = writeln!(out, "  4. Run with the profiler interpreter:");
    let _ = writeln!(
        out,
        "     {}",
        help_color("wq exec '1+1' -i profiler -p", WqAnsiColor::Cyan)
    );
}

fn help_color(text: &str, color: WqAnsiColor) -> String {
    help_paint(text, TextStyle::new().fg(color))
}

fn help_header(text: &str) -> String {
    help_paint(text, TextStyle::new().bold().underline())
}

fn help_paint(text: &str, style: TextStyle) -> String {
    help_paint_with_color_mode(text, style, ColorMode::Auto)
}

fn help_paint_with_color_mode(text: &str, style: TextStyle, color_mode: ColorMode) -> String {
    paint(text, style, color_mode)
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

    #[test]
    fn help_appendix_uses_explicit_style_renderer() {
        assert_eq!(
            help_paint_with_color_mode(
                "Examples",
                TextStyle::new().bold().underline(),
                ColorMode::Always,
            ),
            "\x1b[1;4mExamples\x1b[0m"
        );
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
    fn help_no_pager_uses_global_flag() {
        let (_, cmd) = ok(parse_args(v(&["help", "--no-pager", "map"])));
        match cmd {
            CliCommand::Help {
                no_pager,
                topic,
                prefer_doc_topic,
                fold_width,
            } => {
                assert!(no_pager);
                assert_eq!(topic.as_deref(), Some("map"));
                assert!(!prefer_doc_topic);
                assert_eq!(fold_width, None);
            }
            _ => panic!("expected Help"),
        }
    }

    #[test]
    fn rendered_top_level_help_includes_note_rendered_appendix() {
        let text = render_cli_help(None).expect("top-level help");

        assert!(text.contains("Usage: wq"));
        assert!(text.contains("Debug flags"));
        assert!(text.contains("Run a script"));
        assert!(text.contains("wq script.wq"));
        assert!(text.contains("wq exec '1+1' -d ast,inst -p"));
    }

    #[test]
    fn rendered_exec_help_uses_exec_appendix() {
        let text = render_cli_help(Some("exec")).expect("exec help");

        assert!(text.contains("Usage: wq exec"));
        assert!(text.contains("Evaluate inline code"));
        assert!(text.contains("wq exec '1+1' -p"));
        assert!(!text.contains("Run a script"));
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
                assert_eq!(opts.max_width, None);
                assert!(!opts.wrap_only);
            }
            _ => panic!("expected Fmt"),
        }
        let (_, cmd) = ok(parse_args(v(&[
            "fmt",
            "--width",
            "40",
            "--wrap-only",
            "f.wq",
        ])));
        match cmd {
            CliCommand::Fmt { opts, .. } => {
                assert_eq!(opts.max_width, Some(40));
                assert!(opts.wrap_only);
                assert!(!opts.nlcd);
                assert!(!opts.olw);
            }
            _ => panic!("expected Fmt"),
        }
        let (_, cmd) = ok(parse_args(v(&["fmt", "--width", "72", "f.wq"])));
        match cmd {
            CliCommand::Fmt { opts, .. } => {
                assert_eq!(opts.max_width, Some(72));
                assert!(!opts.wrap_only);
            }
            _ => panic!("expected Fmt"),
        }
        assert_eq!(is_err(parse_args(v(&["fmt"]))), 2);
        assert_eq!(is_err(parse_args(v(&["fmt", "f.wq", "extra"]))), 2);
        assert_eq!(is_err(parse_args(v(&["fmt", "-d3", "f.wq"]))), 2);
        assert_eq!(
            is_err(parse_args(v(&["fmt", "--wrap-only", "--olw", "f.wq"]))),
            2
        );
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

        let (rt, _) = ok(parse_args(v(&["--box", "off", "a.wq"])));
        assert!(!rt.box_print.boxed);
        assert!(!rt.box_print.xray);
        assert!(!rt.box_print.axis);
        assert!(!rt.box_print.color);
        assert_eq!(rt.box_print.summary(), "[]");

        let (rt, _) = ok(parse_args(v(&["--box", "on,-color", "a.wq"])));
        assert!(rt.box_print.boxed);
        assert!(!rt.box_print.xray);
        assert!(rt.box_print.axis);
        assert!(!rt.box_print.color);
        assert_eq!(rt.box_print.summary(), "[box,axis]");

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
    fn markdown_route_by_extension() {
        let (_, cmd) = ok(parse_args(v(&["notes.md"])));
        match cmd {
            CliCommand::Markdown { path, no_pager } => {
                assert_eq!(path, PathBuf::from("notes.md"));
                assert!(!no_pager);
            }
            _ => panic!("expected Markdown"),
        }
    }

    #[test]
    fn markdown_no_pager_flag() {
        let (_, cmd) = ok(parse_args(v(&["--no-pager", "notes.md"])));
        match cmd {
            CliCommand::Markdown { path, no_pager } => {
                assert_eq!(path, PathBuf::from("notes.md"));
                assert!(no_pager);
            }
            _ => panic!("expected Markdown"),
        }
    }

    #[test]
    fn removed_run_notebook_flag_errors() {
        assert_eq!(is_err(parse_args(v(&["--run-notebook", "notes.md"]))), 2);
    }

    #[test]
    fn no_pager_only_applies_to_rendered_markdown_or_help() {
        assert_eq!(is_err(parse_args(v(&["--no-pager", "script.wq"]))), 2);
        assert_eq!(is_err(parse_args(v(&["--no-pager"]))), 2);
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
