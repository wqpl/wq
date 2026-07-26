use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use wqpl::session::Session;

use crate::arg::RuntimeFlags;
use crate::display::{format_print_result, format_xray_info};
use crate::interrupt::{CliInterrupts, INTERRUPTED_EXIT_STATUS};
use crate::load::{eval_inline_with_load, install_module_resolver, load_script};
use crate::msg::{print_dry_run_status, print_load_error};
use crate::repl::input::RustylineInput;
use crate::wqdb::WqdbShell;
use crate::{apply_builtins_flag, apply_interpreter_flag, apply_seed_flag};

pub fn exec_script<P: AsRef<Path>>(filename: P, args: Vec<String>, rtflags: RuntimeFlags) -> i32 {
    let mut evaluator = Session::new();
    install_module_resolver(&mut evaluator);
    evaluator.set_argv(args);
    evaluator.set_debug_flags(rtflags.debug_flags);
    evaluator.set_backtrace_enabled(rtflags.bt);
    let editor = RustylineInput::new().expect("debugger editor should initialize");
    evaluator.set_input(Box::new(editor.clone()));
    let debugger_shell = WqdbShell::new(editor.clone(), rtflags.wqdb_cmds.clone());
    let pause_shell = debugger_shell.clone();
    evaluator.set_pause_handler(move |_event, debugger| pause_shell.on_pause(debugger));
    evaluator.set_wqdb(rtflags.wqdb);
    evaluator.set_dry_mode(rtflags.dry);
    apply_seed_flag(&mut evaluator, &rtflags);
    apply_builtins_flag(&mut evaluator, &rtflags);
    apply_interpreter_flag(&mut evaluator, &rtflags);
    let loading = RefCell::new(HashSet::new());
    let interrupts = CliInterrupts::install().expect("CLI Ctrl-C handler should initialize");
    let interrupt_guard = interrupts.arm(evaluator.interrupt_handle());
    let result = load_script(&mut evaluator, filename, &loading, true);
    drop(interrupt_guard);
    debugger_shell.flush_notifications(&mut evaluator);
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
                debugger_shell.enter_after_error(&mut evaluator, failure);
            }
            1
        }
    }
}

pub fn exec_cmd(content: &str, args: Vec<String>, rtflags: RuntimeFlags) -> i32 {
    let mut session = Session::new();
    install_module_resolver(&mut session);
    session.set_argv(args);
    session.set_debug_flags(rtflags.debug_flags);
    session.set_backtrace_enabled(rtflags.bt);
    let editor = RustylineInput::new().expect("debugger editor should initialize");
    session.set_input(Box::new(editor.clone()));
    let debugger_shell = WqdbShell::new(editor.clone(), rtflags.wqdb_cmds.clone());
    let pause_shell = debugger_shell.clone();
    session.set_pause_handler(move |_event, debugger| pause_shell.on_pause(debugger));
    session.set_wqdb(rtflags.wqdb);
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
    debugger_shell.flush_notifications(&mut session);
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
                debugger_shell.enter_after_error(&mut session, failure);
            }
            1
        }
    }
}
