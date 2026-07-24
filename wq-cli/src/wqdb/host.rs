use std::cell::RefCell;
use std::io::{IsTerminal as _, Write as _};
use std::ops::{Deref, DerefMut};

use wqpl::session::stdio::WqIoError;
use wqpl::style::ColorMode;
use wqpl::wqdb::Debugger;

use crate::repl::input::RustylineInput;

#[derive(Default)]
pub(super) struct ShellState {
    batch_commands: Vec<String>,
    stop_hooks: Vec<StopHook>,
    next_stop_hook_id: usize,
}

impl ShellState {
    pub(super) fn new(batch_commands: Vec<String>) -> Self {
        Self {
            batch_commands,
            next_stop_hook_id: 1,
            ..Self::default()
        }
    }
}

#[derive(Clone)]
pub(super) struct StopHook {
    pub(super) id: usize,
    pub(super) enabled: bool,
    pub(super) command: String,
}

pub(super) struct Host<'debugger, 'vm> {
    debugger: &'debugger mut Debugger<'vm>,
    editor: &'debugger RustylineInput,
    state: &'debugger RefCell<ShellState>,
    output_error: RefCell<Option<WqIoError>>,
}

impl<'debugger, 'vm> Host<'debugger, 'vm> {
    pub(super) fn new(
        debugger: &'debugger mut Debugger<'vm>,
        editor: &'debugger RustylineInput,
        state: &'debugger RefCell<ShellState>,
    ) -> Self {
        Self {
            debugger,
            editor,
            state,
            output_error: RefCell::new(None),
        }
    }

    pub(super) fn editor(&self) -> &RustylineInput {
        self.editor
    }

    pub(super) fn write_line(&self, text: impl AsRef<str>) {
        if self.output_error.borrow().is_some() {
            return;
        }
        let mut stderr = std::io::stderr().lock();
        if let Err(error) = writeln!(stderr, "{}", text.as_ref()) {
            self.output_error
                .replace(Some(WqIoError::Other(error.to_string())));
        }
    }

    pub(super) fn color_mode(&self) -> ColorMode {
        ColorMode::Auto.resolve(std::io::stderr().is_terminal())
    }

    pub(super) fn output_failed(&self) -> bool {
        self.output_error.borrow().is_some()
    }

    pub(super) fn take_batch_commands(&self) -> Vec<String> {
        std::mem::take(&mut self.state.borrow_mut().batch_commands)
    }

    pub(super) fn stop_hook_commands(&self) -> Vec<String> {
        self.state
            .borrow()
            .stop_hooks
            .iter()
            .filter(|hook| hook.enabled)
            .map(|hook| hook.command.clone())
            .collect()
    }

    pub(super) fn stop_hooks(&self) -> Vec<StopHook> {
        self.state.borrow().stop_hooks.clone()
    }

    pub(super) fn add_stop_hook(&self, command: String) -> StopHook {
        let mut state = self.state.borrow_mut();
        let hook = StopHook {
            id: state.next_stop_hook_id,
            enabled: true,
            command,
        };
        state.next_stop_hook_id += 1;
        state.stop_hooks.push(hook.clone());
        hook
    }

    pub(super) fn remove_stop_hook(&self, id: usize) -> bool {
        let mut state = self.state.borrow_mut();
        let old_len = state.stop_hooks.len();
        state.stop_hooks.retain(|hook| hook.id != id);
        state.stop_hooks.len() != old_len
    }

    pub(super) fn clear_stop_hooks(&self) {
        self.state.borrow_mut().stop_hooks.clear();
    }
}

impl<'debugger, 'vm> Deref for Host<'debugger, 'vm> {
    type Target = Debugger<'vm>;

    fn deref(&self) -> &Self::Target {
        self.debugger
    }
}

impl DerefMut for Host<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.debugger
    }
}
