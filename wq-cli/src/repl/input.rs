use wq_rl::Editor;
use wq_rl::error::ReadlineError;
use wq_rl::history::FileHistory;
use wqpl::session::stdio::{WqGlobalHint, WqInputMode, WqStdin, WqStdinError};

use crate::repl::editor::WqReplHighlighter;

pub(crate) struct RustylineInput {
    rl: Editor<WqReplHighlighter, FileHistory>,
}

impl RustylineInput {
    pub(crate) fn new() -> wq_rl::Result<Self> {
        let config = wq_rl::Config::builder()
            .hint_accept_enabled(true)
            .completion_type(wq_rl::CompletionType::Menu)
            .build();
        let mut rl: Editor<WqReplHighlighter, _> = Editor::with_config(config)?;
        rl.set_helper(Some(WqReplHighlighter::new()));
        Ok(Self { rl })
    }
}

impl WqStdin for RustylineInput {
    fn readline(&mut self, prompt: &str) -> Result<String, WqStdinError> {
        match self.rl.readline(prompt) {
            Ok(line) => Ok(line),
            Err(ReadlineError::Eof) => Err(WqStdinError::Eof),
            Err(ReadlineError::Interrupted) => Err(WqStdinError::Interrupted),
            Err(e) => Err(WqStdinError::Other(e.to_string())),
        }
    }

    fn add_history(&mut self, line: &str) {
        let _ = self.rl.add_history_entry(line);
    }

    fn set_highlight(&mut self, on: bool) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_enabled(on);
        }
    }

    fn highlight_enabled(&self) -> bool {
        self.rl.helper().map(|h| h.enabled()).unwrap_or(true)
    }

    fn set_input_mode(&mut self, mode: WqInputMode) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_input_mode(mode);
        }
    }

    fn input_mode(&self) -> WqInputMode {
        self.rl
            .helper()
            .map(WqReplHighlighter::input_mode)
            .unwrap_or_default()
    }

    fn set_builtin_hints(&mut self, names: Vec<String>, usages: Vec<String>) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_builtin_hints(names, usages);
        }
    }

    fn set_global_hints(&mut self, hints: Vec<WqGlobalHint>) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_global_hints(hints);
        }
    }

    fn set_wqdb_function_hints(&mut self, names: Vec<String>) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_wqdb_function_hints(names);
        }
    }

    fn set_repl_hints(&mut self, names: Vec<String>, descs: Vec<String>) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_repl_hints(names, descs);
        }
    }

    fn set_hints_enabled(&mut self, on: bool) {
        if let Some(h) = self.rl.helper_mut() {
            h.set_hints_enabled(on);
        }
    }

    fn hints_enabled(&self) -> bool {
        self.rl.helper().map(|h| h.hints_enabled()).unwrap_or(true)
    }
}
