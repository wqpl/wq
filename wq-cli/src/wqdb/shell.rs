use std::cell::RefCell;
use std::rc::Rc;

use wqpl::session::stdio::WqIoError;
use wqpl::session::{EvaluationFailure, Session};
use wqpl::style::AnsiColor;
use wqpl::wqdb::{DebugNotification, DebugResume, Debugger};

use super::execute::{
    exec_single_wqdb_cmd, exec_stop_hooks, exec_wqdb_cmds, print_crash_locals, print_stop_card,
    print_stop_controls, print_symbol_mutation,
};
use super::host::{Host, ShellState};
use super::render::{color, prompt, title};
use crate::repl::InteractiveOutputSpacing;
use crate::repl::input::{RustylineInput, WqInputMode};

#[derive(Clone)]
pub(crate) struct WqdbShell {
    editor: RustylineInput,
    state: Rc<RefCell<ShellState>>,
}

impl WqdbShell {
    pub(crate) fn new(editor: RustylineInput, batch_commands: Vec<String>) -> Self {
        Self {
            editor,
            state: Rc::new(RefCell::new(ShellState::new(batch_commands))),
        }
    }

    pub(crate) fn on_pause(&self, debugger: &mut Debugger<'_>) -> DebugResume {
        let mut host = Host::new(debugger, &self.editor, &self.state);
        wqdb_shell_inner(&mut host)
    }

    pub(crate) fn flush_notifications(&self, session: &mut Session) {
        let mut debugger = session.debugger();
        let mut host = Host::new(&mut debugger, &self.editor, &self.state);
        for notification in host.take_notifications() {
            match notification {
                DebugNotification::SymbolChanged(mutation) => {
                    print_symbol_mutation(&host, &mutation);
                }
            }
        }
    }

    /// Enter wqdb after a crash and reuse this shell's command state.
    pub(crate) fn enter_after_error(&self, session: &mut Session, failure: &EvaluationFailure) {
        let Some(mut debugger) = session.postmortem_debugger(failure) else {
            return;
        };
        let mut host = Host::new(&mut debugger, &self.editor, &self.state);
        host.write_line(format!(
            "{}: {}",
            title("wqdb", host.color_mode()),
            color("error occurred", AnsiColor::Red, host.color_mode()),
        ));
        print_crash_locals(&mut host);
        let action = wqdb_shell_inner(&mut host);
        drop(host);
        debugger.apply_resume(action);
    }
}

fn wqdb_shell_inner(host: &mut Host<'_, '_>) -> DebugResume {
    for notification in host.take_notifications() {
        match notification {
            DebugNotification::SymbolChanged(mutation) => {
                print_symbol_mutation(host, &mutation);
            }
        }
    }
    let commands = host.take_batch_commands();
    if !commands.is_empty() {
        let action = exec_wqdb_cmds(host, &commands);
        if host.output_failed() {
            return DebugResume::Continue;
        }
        return action.unwrap_or(DebugResume::Continue);
    }

    if let Some(action) = exec_stop_hooks(host) {
        return action;
    }
    if host.output_failed() {
        return DebugResume::Continue;
    }

    let mut debugger_line = 1usize;
    let mut output_spacing = InteractiveOutputSpacing::default();
    print_stop_card(host);
    print_stop_controls(host, host.step_granularity());
    if host.output_failed() {
        return DebugResume::Continue;
    }
    output_spacing.after_output();
    loop {
        let mut function_names = host
            .debug_info()
            .function_names()
            .map(str::to_string)
            .collect::<Vec<_>>();
        function_names.sort();
        host.editor().set_wqdb_function_hints(function_names);
        if output_spacing.before_prompt() {
            wqdb_println!(host, "");
            if host.output_failed() {
                return DebugResume::Continue;
            }
        }
        #[cfg(not(target_os = "windows"))]
        let prompt = prompt(host.step_granularity(), debugger_line, host.color_mode());
        #[cfg(target_os = "windows")]
        let prompt = prompt(
            host.step_granularity(),
            debugger_line,
            wqpl::style::ColorMode::Never,
        );

        let result = host
            .editor()
            .with_input_mode(WqInputMode::Wqdb, || host.editor().read_line(&prompt));
        match result {
            Ok(line) => {
                debugger_line += 1;
                let command = line.trim();
                if output_spacing.after_input(command) {
                    wqdb_println!(host, "");
                }
                if command.is_empty() {
                    continue;
                }
                if let Some(action) = exec_single_wqdb_cmd(host, command) {
                    return action;
                }
                if host.output_failed() {
                    return DebugResume::Continue;
                }
            }
            Err(WqIoError::Interrupted) => continue,
            Err(_) => return DebugResume::Continue,
        }
    }
}
