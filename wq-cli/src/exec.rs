use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wqpl::session::Session;

use crate::arg::RuntimeFlags;
use crate::display::{format_print_result, format_xray_info};
use crate::interrupt::{CliInterrupts, INTERRUPTED_EXIT_STATUS};
use crate::load::{eval_inline_with_load, load_script};
use crate::msg::{print_dry_run_status, print_load_error};
use crate::repl::input::RustylineInput;
use crate::wqdb::{enter_wqdb_after_err, wqdb_shell};
use crate::{apply_builtins_flag, apply_interpreter_flag, apply_seed_flag};

pub fn exec_script<P: AsRef<Path>>(filename: P, args: Vec<String>, rtflags: RuntimeFlags) -> i32 {
    let mut evaluator = Session::new();
    evaluator.set_argv(args);
    evaluator.set_debug_flags(rtflags.debug_flags);
    evaluator.set_backtrace_enabled(rtflags.bt);
    let editor = RustylineInput::new().expect("debugger editor should initialize");
    evaluator.set_input(Box::new(editor.clone()));
    let debugger_editor = editor.clone();
    evaluator.set_pause_handler(move |_event, debugger| wqdb_shell(debugger, &debugger_editor));
    evaluator.set_wqdb(rtflags.wqdb);
    if !rtflags.wqdb_cmds.is_empty() {
        evaluator.set_wqdb_batch_cmds(rtflags.wqdb_cmds.clone());
    }
    evaluator.set_dry_mode(rtflags.dry);
    apply_seed_flag(&mut evaluator, &rtflags);
    apply_builtins_flag(&mut evaluator, &rtflags);
    apply_interpreter_flag(&mut evaluator, &rtflags);
    let loading = RefCell::new(HashSet::new());
    let interrupts = CliInterrupts::install().expect("CLI Ctrl-C handler should initialize");
    let interrupt_guard = interrupts.arm(evaluator.interrupt_handle());
    let result = load_script(&mut evaluator, filename, &loading, true);
    drop(interrupt_guard);
    if evaluator.take_interrupt() {
        return INTERRUPTED_EXIT_STATUS;
    }
    match result {
        Ok(report) => {
            if let Some(status) = evaluator.take_halt_status() {
                return status;
            }
            if rtflags.print
                && !rtflags.dry
                && let Some(result) = report.result
            {
                println!("{}", format_print_result(&result, &rtflags.box_print));
                if rtflags.box_print.shows_xray() {
                    println!("{}", format_xray_info(&result, &rtflags.box_print));
                }
            }
            if rtflags.dry {
                print_dry_run_status();
            }
            0
        }
        Err(err) => {
            print_load_error(&err, &mut evaluator);
            if evaluator.is_wqdb_enabled()
                && let Some(failure) = err.evaluation_failure()
            {
                enter_wqdb_after_err(&mut evaluator, failure, &editor);
            }
            1
        }
    }
}

pub fn exec_cmd(content: &str, args: Vec<String>, rtflags: RuntimeFlags) -> i32 {
    let mut session = Session::new();
    session.set_argv(args);
    session.set_debug_flags(rtflags.debug_flags);
    session.set_backtrace_enabled(rtflags.bt);
    let editor = RustylineInput::new().expect("debugger editor should initialize");
    session.set_input(Box::new(editor.clone()));
    let debugger_editor = editor.clone();
    session.set_pause_handler(move |_event, debugger| wqdb_shell(debugger, &debugger_editor));
    session.set_wqdb(rtflags.wqdb);
    if !rtflags.wqdb_cmds.is_empty() {
        session.set_wqdb_batch_cmds(rtflags.wqdb_cmds.clone());
    }
    session.set_dry_mode(rtflags.dry);
    apply_seed_flag(&mut session, &rtflags);
    apply_builtins_flag(&mut session, &rtflags);
    apply_interpreter_flag(&mut session, &rtflags);
    let loading = RefCell::new(HashSet::new());
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let interrupts = CliInterrupts::install().expect("CLI Ctrl-C handler should initialize");
    let interrupt_guard = interrupts.arm(session.interrupt_handle());
    let result = eval_inline_with_load(&mut session, content, &cwd, &loading, true);
    drop(interrupt_guard);
    if session.take_interrupt() {
        return INTERRUPTED_EXIT_STATUS;
    }
    match result {
        Ok(report) => {
            if let Some(status) = session.take_halt_status() {
                return status;
            }
            if rtflags.print
                && !rtflags.dry
                && let Some(result) = report.result
            {
                println!("{}", format_print_result(&result, &rtflags.box_print));
                if rtflags.box_print.shows_xray() {
                    println!("{}", format_xray_info(&result, &rtflags.box_print));
                }
            }
            if rtflags.dry {
                print_dry_run_status();
            }
            0
        }
        Err(err) => {
            print_load_error(&err, &mut session);
            if session.is_wqdb_enabled()
                && let Some(failure) = err.evaluation_failure()
            {
                enter_wqdb_after_err(&mut session, failure, &editor);
            }
            1
        }
    }
}
