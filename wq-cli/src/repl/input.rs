use std::sync::{Arc, Mutex, MutexGuard};

use wq_rl::Editor;
use wq_rl::error::ReadlineError;
use wq_rl::history::FileHistory;
use wqpl::builtins::BuiltinPreset;
use wqpl::session::stdio::{WqInput, WqIoError};

use crate::repl::editor::WqReplHighlighter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WqGlobalHint {
    pub(crate) name: String,
    pub(crate) type_name: String,
    pub(crate) excerpt: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WqInputMode {
    #[default]
    Wq,
    Wqdb,
}

type ReplEditor = Editor<WqReplHighlighter, FileHistory>;

/// CLI-owned shared editor handle.
///
/// A clone is installed in each session as the minimal `WqInput` adapter. The
/// CLI retains another clone for editor-only behavior such as completion,
/// history, highlighting, and debugger input mode.
#[derive(Clone)]
pub(crate) struct RustylineInput {
    editor: Arc<Mutex<ReplEditor>>,
}

impl RustylineInput {
    pub(crate) fn new() -> wq_rl::Result<Self> {
        let config = wq_rl::Config::builder()
            .hint_accept_enabled(true)
            .completion_type(wq_rl::CompletionType::Menu)
            .build();
        let mut editor: ReplEditor = Editor::with_config(config)?;
        editor.set_helper(Some(WqReplHighlighter::new()));
        Ok(Self {
            editor: Arc::new(Mutex::new(editor)),
        })
    }

    fn lock_editor(&self) -> MutexGuard<'_, ReplEditor> {
        self.editor
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn read_line(&self, prompt: &str) -> Result<String, WqIoError> {
        match self.lock_editor().readline(prompt) {
            Ok(line) => Ok(line),
            Err(ReadlineError::Eof) => Err(WqIoError::Eof),
            Err(ReadlineError::Interrupted) => Err(WqIoError::Interrupted),
            Err(error) => Err(WqIoError::Other(error.to_string())),
        }
    }

    pub(crate) fn add_history(&self, line: &str) {
        let _ = self.lock_editor().add_history_entry(line);
    }

    pub(crate) fn set_highlight(&self, on: bool) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_enabled(on);
        }
    }

    pub(crate) fn highlight_enabled(&self) -> bool {
        self.lock_editor()
            .helper()
            .map(WqReplHighlighter::enabled)
            .unwrap_or(true)
    }

    pub(crate) fn set_input_mode(&self, mode: WqInputMode) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_input_mode(mode);
        }
    }

    pub(crate) fn input_mode(&self) -> WqInputMode {
        self.lock_editor()
            .helper()
            .map(WqReplHighlighter::input_mode)
            .unwrap_or_default()
    }

    pub(crate) fn with_input_mode<R>(&self, mode: WqInputMode, f: impl FnOnce() -> R) -> R {
        let previous = self.input_mode();
        self.set_input_mode(mode);
        let restore = InputModeRestore {
            input: self.clone(),
            previous,
        };
        let result = f();
        drop(restore);
        result
    }

    pub(crate) fn set_builtin_hints(&self, names: Vec<String>, usages: Vec<String>) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_builtin_hints(names, usages);
        }
    }

    pub(crate) fn set_builtins_preset(&self, preset: BuiltinPreset) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_builtins_preset(preset);
        }
    }

    pub(crate) fn set_global_hints(&self, hints: Vec<WqGlobalHint>) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_global_hints(hints);
        }
    }

    pub(crate) fn set_wqdb_function_hints(&self, names: Vec<String>) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_wqdb_function_hints(names);
        }
    }

    pub(crate) fn set_repl_hints(&self, names: Vec<String>, descs: Vec<String>) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_repl_hints(names, descs);
        }
    }

    pub(crate) fn set_hints_enabled(&self, on: bool) {
        if let Some(helper) = self.lock_editor().helper_mut() {
            helper.set_hints_enabled(on);
        }
    }

    pub(crate) fn hints_enabled(&self) -> bool {
        self.lock_editor()
            .helper()
            .map(WqReplHighlighter::hints_enabled)
            .unwrap_or(true)
    }
}

struct InputModeRestore {
    input: RustylineInput,
    previous: WqInputMode,
}

impl Drop for InputModeRestore {
    fn drop(&mut self) {
        self.input.set_input_mode(self.previous);
    }
}

impl WqInput for RustylineInput {
    fn read_line(&mut self, prompt: &str) -> Result<String, WqIoError> {
        RustylineInput::read_line(self, prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send<T: Send>() {}

    #[test]
    fn shared_input_adapter_is_send() {
        assert_send::<RustylineInput>();
    }

    #[test]
    fn cloned_handles_share_editor_state() {
        let input = RustylineInput::new().expect("editor should initialize");
        let adapter = input.clone();

        input.set_highlight(false);
        input.set_hints_enabled(false);

        assert!(!adapter.highlight_enabled());
        assert!(!adapter.hints_enabled());
    }

    #[test]
    fn input_mode_is_restored_after_callback() {
        let input = RustylineInput::new().expect("editor should initialize");

        input.with_input_mode(WqInputMode::Wqdb, || {
            assert_eq!(input.input_mode(), WqInputMode::Wqdb);
        });

        assert_eq!(input.input_mode(), WqInputMode::Wq);
    }
}
