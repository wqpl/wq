mod arg;
mod dap;
mod display;
mod exec;
mod help;
mod load;
mod msg;
mod note;
mod repl;
mod symbol;
mod wqdb;

use std::io::Read as _;
use std::path::Path;

use wqpl::builtins::BuiltinPreset;
use wqpl::format::{FormatConfig, Formatter};
use wqpl::interpret::InterpreterKind;
use wqpl::session::Session;
use wqpl::vm::Vm;

use crate::arg::{CliCommand, ExecSource, FmtOpts, RuntimeFlags, parse_args};

fn spawn_wq_thread<F, T>(stack_size_mb: usize, workload: F) -> T
where
    F: FnOnce() -> T,
    F: Send + 'static,
    T: Send + 'static,
{
    let handle = std::thread::Builder::new()
        .name("wq".into())
        .stack_size(stack_size_mb * 1024 * 1024)
        .spawn(workload)
        .expect("failed to spawn wq runtime thread");

    handle.join().unwrap_or_else(|payload| {
        std::panic::resume_unwind(payload);
    })
}

fn main() {
    let (rtflags, cmd) = match parse_args(std::env::args_os().skip(1), false) {
        Ok(v) => v,
        Err(code) => std::process::exit(code),
    };
    match cmd {
        CliCommand::Fmt { script, opts } => {
            format_script(&script, opts);
        }
        CliCommand::Exec(ExecSource::Inline(src)) => {
            spawn_wq_thread(rtflags.stack_size_mebibyte, move || {
                exec::exec_cmd(&src, rtflags)
            });
        }
        CliCommand::Exec(ExecSource::Stdin) => {
            let mut input = String::new();
            let _ = std::io::stdin().read_to_string(&mut input);
            spawn_wq_thread(rtflags.stack_size_mebibyte, move || {
                exec::exec_cmd(&input, rtflags)
            });
        }
        CliCommand::Script(path) => {
            spawn_wq_thread(rtflags.stack_size_mebibyte, move || {
                exec::exec_script(&path, rtflags)
            });
        }
        CliCommand::Notebook(path, interactive) => {
            spawn_wq_thread(rtflags.stack_size_mebibyte, move || {
                note::run_notebook(&path, rtflags, interactive)
            });
        }
        CliCommand::Symbols { script, name } => {
            symbol::run_symbols(&script, &name);
        }
        CliCommand::Repl => {
            spawn_wq_thread(rtflags.stack_size_mebibyte, move || {
                repl::enter_repl(rtflags)
            });
        }
        CliCommand::Dap { script } => {
            dap::run_dap(script);
        }
        CliCommand::Help {
            no_pager,
            topic,
            prefer_doc_topic,
            fold_width,
        } => {
            help::run(topic, no_pager, prefer_doc_topic, fold_width);
        }
    }
}

fn format_script<P: AsRef<Path>>(filename: P, opts: FmtOpts) {
    let path = filename.as_ref();
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let mut config = FormatConfig {
                indent_size: 2,
                nlcd: opts.nlcd,
                one_line_wizard: opts.olw,
                ..FormatConfig::default()
            };
            if let Some(width) = opts.max_width {
                config.max_width = width;
            }
            config.wrap_only = opts.wrap_only;
            let fmt = Formatter::new(config);
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

pub(crate) fn apply_interpreter_flag(evaluator: &mut Session, rtflags: &RuntimeFlags) {
    if let Some(name) = rtflags.interpreter.as_deref()
        && let Err(err) = evaluator.set_interpreter_by_name(name)
    {
        let list = InterpreterKind::names().join(", ");
        eprintln!("{err}; available: {list}");
        std::process::exit(2);
    }
}

pub(crate) fn apply_builtins_flag(evaluator: &mut Session, rtflags: &RuntimeFlags) {
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

/// Callback for wqdb pause hook - called by the VM when debugger pauses
fn wqdb_pause_handler(host: &mut Vm) {
    wqdb::wqdb_shell(host);
}
