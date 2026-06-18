#![cfg(not(target_arch = "wasm32"))]

use std::borrow::Cow;

use wq_rl::completion::{Completer, Pair};
use wq_rl::highlight::{CmdKind, Highlighter as RLHighlighter, InputAreaStyle};
use wq_rl::hint::Hinter;
use wq_rl::validate::{ValidationContext, ValidationResult, Validator};
use wq_rl::{Context as RLContext, Helper};
use wqpl::builtins::BuiltinPreset;
use wqpl::highlight::{Highlighter, cursor_context_at};
use wqpl::interpret::InterpreterKind;
use wqpl::session::Session;
use wqpl::session::dbglog::DEBUG_LOG_FLAG_NAMES;

const RESET: &str = "\x1b[0m";
const REPL_INPUT_BG: &str = "\x1b[48;5;236m";
const REPL_INPUT_RESET: &str = "\x1b[0m";
const REPL_INPUT_TOKEN_RESET: &str = "\x1b[22;23;24;39m";

pub struct WqReplHighlighter {
    highlighter: Highlighter,
    enabled: bool,
    builtin_names: Vec<String>,
    builtin_usages: Vec<String>,
    global_names: Vec<String>,
    global_types: Vec<String>,
    global_excerpts: Vec<String>,
    repl_names: Vec<String>,
    repl_descs: Vec<String>,
    hints_enabled: bool,
}

impl Default for WqReplHighlighter {
    fn default() -> Self {
        WqReplHighlighter::new()
    }
}

impl WqReplHighlighter {
    pub fn new() -> Self {
        Self {
            highlighter: Highlighter::new(),
            enabled: true,
            builtin_names: Vec::new(),
            builtin_usages: Vec::new(),
            global_names: Vec::new(),
            global_types: Vec::new(),
            global_excerpts: Vec::new(),
            repl_names: Vec::new(),
            repl_descs: Vec::new(),
            hints_enabled: true,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_hints_enabled(&mut self, on: bool) {
        self.hints_enabled = on;
    }

    pub fn hints_enabled(&self) -> bool {
        self.hints_enabled
    }

    pub fn set_builtin_hints(&mut self, names: Vec<String>, usages: Vec<String>) {
        self.builtin_names = names;
        self.builtin_usages = usages;
    }

    pub fn set_global_hints(
        &mut self,
        names: Vec<String>,
        types: Vec<String>,
        excerpts: Vec<String>,
    ) {
        self.global_names = names;
        self.global_types = types;
        self.global_excerpts = excerpts;
    }

    pub fn set_repl_hints(&mut self, names: Vec<String>, descs: Vec<String>) {
        self.repl_names = names;
        self.repl_descs = descs;
    }

    /// Find the start index of the "word" that ends at `pos`.
    /// For REPL commands the leading `!` is included.
    fn current_word_start(line: &str, pos: usize) -> usize {
        let bytes = line.as_bytes();
        let pos = pos.min(bytes.len());
        let mut start = pos;
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'?' || b == b'!' {
                start -= 1;
            } else {
                break;
            }
        }
        start
    }

    /// Return true if completion / hints should be suppressed at `pos`.
    ///
    /// Rules:
    /// - Inside comments, strings, f-string text, or tags.
    /// - Words immediately preceded by `@` (e.g. `@f`, `@r`).
    fn should_suppress(&self, line: &str, pos: usize) -> bool {
        let pos = pos.min(line.len());
        if cursor_context_at(line, pos).suppresses_completion() {
            return true;
        }
        let start = Self::current_word_start(line, pos);
        if start > 0 && line.as_bytes()[start - 1] == b'@' {
            return true;
        }
        false
    }

    fn colorize_input(&self, line: &str) -> String {
        let semantic_spans = self.semantic_highlight_spans(line);
        self.highlighter
            .highlight_ansi_with_semantic_spans_and_reset(
                line,
                &semantic_spans,
                REPL_INPUT_TOKEN_RESET,
            )
    }

    pub fn highlight_text(&self, text: &str) -> String {
        if self.enabled() {
            let semantic_spans = self.semantic_highlight_spans(text);
            self.highlighter
                .highlight_ansi_with_semantic_spans_and_reset(text, &semantic_spans, "")
        } else {
            text.to_string()
        }
    }

    fn semantic_highlight_spans(&self, text: &str) -> Vec<wqpl::highlight::SemanticHighlightSpan> {
        if !text.contains('{') && !text.contains('\'') {
            return Vec::new();
        }
        Session::new()
            .analyze_symbols(text)
            .map(|index| index.semantic_highlight_spans())
            .unwrap_or_default()
    }
}

impl Helper for WqReplHighlighter {}

impl Completer for WqReplHighlighter {
    type Candidate = Pair;
    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &RLContext<'_>,
    ) -> wq_rl::Result<(usize, Vec<Self::Candidate>)> {
        let pos = pos.min(line.len());
        if self.should_suppress(line, pos) {
            return Ok((pos, Vec::new()));
        }
        let start = Self::current_word_start(line, pos);
        let prefix = &line[start..pos];
        let mut candidates: Vec<Pair> = Vec::new();
        let trimmed = line.trim_start();
        if trimmed.starts_with('!') {
            // Check if we're completing an argument (cursor after first whitespace word)
            if let Some(first_space) = trimmed.find(|c: char| c.is_ascii_whitespace()) {
                let cmd_end = line.len() - trimmed.len() + first_space;
                if pos > cmd_end {
                    // Only complete the first argument; skip if more words already exist.
                    let words_before = line[..start].split_whitespace().count();
                    if words_before >= 2 {
                        return Ok((start, Vec::new()));
                    }
                    let cmd_word = &trimmed[..first_space];
                    let arg_prefix = prefix;
                    match cmd_word {
                        "!bfn" => {
                            for name in BuiltinPreset::names()
                                .iter()
                                .filter(|n| n.starts_with(arg_prefix))
                            {
                                candidates.push(Pair {
                                    display: name.to_string(),
                                    replacement: name.to_string(),
                                });
                            }
                        }
                        "!i" | "!interpreter" => {
                            for name in InterpreterKind::names()
                                .iter()
                                .filter(|n| n.starts_with(arg_prefix))
                            {
                                candidates.push(Pair {
                                    display: name.to_string(),
                                    replacement: name.to_string(),
                                });
                            }
                        }
                        "!help" | "!h" => {
                            let names = &self.builtin_names;
                            for name in names.iter().filter(|n| n.starts_with(arg_prefix)) {
                                candidates.push(Pair {
                                    display: name.clone(),
                                    replacement: name.clone(),
                                });
                            }
                        }
                        "!d" | "!debug" => {
                            let aliases = ["0", "1", "2", "3", "4"];
                            for name in aliases
                                .iter()
                                .copied()
                                .chain(
                                    DEBUG_LOG_FLAG_NAMES
                                        .iter()
                                        .flat_map(|(names, _)| names.iter())
                                        .copied(),
                                )
                                .filter(|n| n.starts_with(arg_prefix))
                            {
                                candidates.push(Pair {
                                    display: name.to_string(),
                                    replacement: name.to_string(),
                                });
                            }
                        }
                        "!fmt" => {
                            let modes = ["on", "off", "nlcd", "olw"];
                            for mode in modes.iter().filter(|n| n.starts_with(arg_prefix)) {
                                candidates.push(Pair {
                                    display: mode.to_string(),
                                    replacement: mode.to_string(),
                                });
                            }
                        }
                        _ => {}
                    }
                    candidates.sort_by(|a, b| a.display.cmp(&b.display));
                    candidates.dedup_by(|a, b| a.display == b.display);
                    return Ok((start, candidates));
                }
            }
            // Command name completion
            if prefix.is_empty() {
                return Ok((pos, Vec::new()));
            }
            let repl_names = &self.repl_names;
            let repl_descs = &self.repl_descs;
            for (name, desc) in repl_names
                .iter()
                .zip(repl_descs.iter())
                .filter(|(n, _)| n.starts_with(prefix))
            {
                candidates.push(Pair {
                    display: format!("{name} {desc}"),
                    replacement: name.clone(),
                });
            }
        } else {
            if prefix.is_empty() {
                return Ok((pos, Vec::new()));
            }
            let names = &self.builtin_names;
            for n in names.iter().filter(|n| n.starts_with(prefix)) {
                candidates.push(Pair {
                    display: n.clone(),
                    replacement: n.clone(),
                });
            }
            let globals = &self.global_names;
            for n in globals.iter().filter(|n| n.starts_with(prefix)) {
                candidates.push(Pair {
                    display: n.clone(),
                    replacement: n.clone(),
                });
            }
        }
        candidates.sort_by(|a, b| a.display.cmp(&b.display));
        candidates.dedup_by(|a, b| a.display == b.display);
        Ok((start, candidates))
    }
}

impl Hinter for WqReplHighlighter {
    type Hint = String;
    fn hint(&self, line: &str, pos: usize, _ctx: &RLContext<'_>) -> Option<Self::Hint> {
        let pos = pos.min(line.len());
        if !self.hints_enabled || self.should_suppress(line, pos) {
            return None;
        }
        let start = Self::current_word_start(line, pos);
        let prefix = &line[start..pos];
        if prefix.is_empty() {
            return None;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with('!') {
            // REPL command argument hinting
            if let Some(first_space) = trimmed.find(|c: char| c.is_ascii_whitespace()) {
                let cmd_end = line.len() - trimmed.len() + first_space;
                if pos > cmd_end {
                    // Only hint the first argument; skip if more words already exist.
                    let words_before = line[..start].split_whitespace().count();
                    if words_before >= 2 {
                        return None;
                    }
                    let cmd_word = &trimmed[..first_space];
                    let arg_prefix = prefix;
                    let mut candidates: Vec<String> = Vec::new();
                    match cmd_word {
                        "!bfn" => {
                            candidates.extend(
                                BuiltinPreset::names()
                                    .iter()
                                    .filter(|n| n.starts_with(arg_prefix))
                                    .map(|n| n.to_string()),
                            );
                        }
                        "!i" | "!interpreter" => {
                            candidates.extend(
                                InterpreterKind::names()
                                    .iter()
                                    .filter(|n| n.starts_with(arg_prefix))
                                    .map(|n| n.to_string()),
                            );
                        }
                        "!help" | "!h" => {
                            let names = &self.builtin_names;
                            candidates.extend(
                                names.iter().filter(|n| n.starts_with(arg_prefix)).cloned(),
                            );
                        }
                        "!d" | "!debug" => {
                            let aliases = ["0", "1", "2", "3", "4"];
                            candidates.extend(
                                aliases
                                    .iter()
                                    .copied()
                                    .chain(
                                        DEBUG_LOG_FLAG_NAMES
                                            .iter()
                                            .flat_map(|(names, _)| names.iter())
                                            .copied(),
                                    )
                                    .filter(|n| n.starts_with(arg_prefix))
                                    .map(|n| n.to_string()),
                            );
                        }
                        "!fmt" => {
                            let modes = ["on", "off", "nlcd", "olw"];
                            candidates.extend(
                                modes
                                    .iter()
                                    .filter(|n| n.starts_with(arg_prefix))
                                    .map(|n| n.to_string()),
                            );
                        }
                        _ => {}
                    }
                    candidates.sort();
                    candidates.dedup();
                    let name = candidates.first()?;
                    if name == arg_prefix {
                        return None;
                    }
                    let suffix = &name[arg_prefix.len()..];
                    return Some(suffix.to_string());
                }
            }
            // REPL command name hinting
            let repl_names = &self.repl_names;
            let repl_descs = &self.repl_descs;
            let mut merged: Vec<(String, Option<String>)> = Vec::new();
            for (name, desc) in repl_names.iter().zip(repl_descs.iter()) {
                merged.push((name.clone(), Some(format!("  {desc}"))));
            }
            merged.sort_by(|a, b| a.0.cmp(&b.0));
            merged.dedup_by(|a, b| a.0 == b.0);

            let matches: Vec<usize> = merged
                .iter()
                .enumerate()
                .filter(|(_, (n, _))| n.starts_with(prefix))
                .map(|(i, _)| i)
                .collect();
            if matches.is_empty() {
                return None;
            }
            let idx = matches[0];
            let (name, hint) = &merged[idx];
            if name == prefix {
                return hint.as_ref().map(|h| h.to_string());
            }
            let suffix = &name[prefix.len()..];
            return Some(suffix.to_string());
        }

        // Builtin + global hinting
        let names = &self.builtin_names;
        let usages = &self.builtin_usages;
        let globals = &self.global_names;
        let global_types = &self.global_types;
        let global_excerpts = &self.global_excerpts;

        let mut merged: Vec<(String, Option<String>)> = Vec::new();
        for (name, usage) in names.iter().zip(usages.iter()) {
            merged.push((name.clone(), Some(format!("  {}", usage.clone()))));
        }
        for (i, name) in globals.iter().enumerate() {
            let ty = global_types.get(i).cloned().unwrap_or_default();
            let excerpt = global_excerpts.get(i).cloned().unwrap_or_default();
            merged.push((name.clone(), Some(format!("  :{ty} {excerpt}"))));
        }
        merged.sort_by(|a, b| a.0.cmp(&b.0));
        merged.dedup_by(|a, b| a.0 == b.0);

        let matches: Vec<usize> = merged
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| n.starts_with(prefix))
            .map(|(i, _)| i)
            .collect();
        if matches.is_empty() {
            return None;
        }
        let idx = matches[0];
        let (name, hint) = &merged[idx];
        if name == prefix {
            return hint.as_ref().map(|h| h.to_string());
        }
        let suffix = &name[prefix.len()..];
        Some(suffix.to_string())
    }
}

impl Validator for WqReplHighlighter {
    fn validate(&self, ctx: &mut ValidationContext) -> wq_rl::Result<ValidationResult> {
        let input = ctx.input();
        if input.trim().is_empty() {
            return Ok(ValidationResult::Valid(None));
        }
        if input.trim_start().starts_with('!') {
            return Ok(ValidationResult::Valid(None));
        }
        if Session::is_complete_input(input) {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Incomplete(Some("... ".to_string())))
        }
    }
}

impl RLHighlighter for WqReplHighlighter {
    fn input_area_style(&self) -> Option<InputAreaStyle> {
        if cfg!(unix) {
            Some(InputAreaStyle {
                background: REPL_INPUT_BG,
                reset: REPL_INPUT_RESET,
                horizontal_padding: 1,
                vertical_padding: 1,
            })
        } else {
            None
        }
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.enabled() {
            return std::borrow::Cow::Borrowed(line);
        }
        if cfg!(unix) {
            Cow::Owned(self.colorize_input(line))
        } else {
            Cow::Owned(self.highlight_text(line))
        }
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        if cfg!(unix) {
            Cow::Owned(prompt.replace(RESET, REPL_INPUT_TOKEN_RESET))
        } else {
            Cow::Borrowed(prompt)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        if cfg!(unix) {
            Cow::Owned(format!("\x1b[38;5;244m{hint}\x1b[39m"))
        } else {
            Cow::Borrowed(hint)
        }
    }

    fn highlight_candidate<'c>(
        &self,
        cand: &'c str,
        _t: wq_rl::config::CompletionType,
    ) -> Cow<'c, str> {
        Cow::Borrowed(cand)
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use wq_rl::history::DefaultHistory;

    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut chars = s.chars();
        while let Some(ch) = chars.next() {
            if ch == '\x1b' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    #[test]
    fn highlight_text_marks_ref_capture_deeper_blue() {
        let h = WqReplHighlighter::new();
        let src = "a:1; f:'{[] a}; f[]";
        let out = h.highlight_text(src);

        assert!(out.contains("\x1b[38;5;39ma"));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn colorize_input_preserves_repl_token_reset() {
        let h = WqReplHighlighter::new();
        let src = "a:1; f:'{[] a}; f[]";
        let out = h.colorize_input(src);

        assert!(out.contains("\x1b[38;5;39ma\x1b[22;23;24;39m"));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn highlight_text_marks_parameters() {
        let h = WqReplHighlighter::new();
        let src = "f:{[x] x+1}";
        let out = h.highlight_text(src);

        assert!(out.contains("\x1b[38;5;215mx"));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn completion_is_suppressed_inside_unterminated_multiline_string() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum x".to_string()]);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);
        let src = "\"hello\nsu";

        let (_, candidates) = h.complete(src, src.len(), &ctx).expect("completion");

        assert!(h.should_suppress(src, src.len()));
        assert!(candidates.is_empty());
    }

    #[test]
    fn completion_resumes_after_closed_multiline_string() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum x".to_string()]);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);
        let src = "\"hello\nworld\"\nsu";

        let (start, candidates) = h.complete(src, src.len(), &ctx).expect("completion");

        assert!(!h.should_suppress(src, src.len()));
        assert_eq!(start, src.len() - 2);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].replacement, "sum");
    }

    #[test]
    fn fstring_text_suppresses_but_expr_allows_completion() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum x".to_string()]);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);
        let text_src = "@f \"hello su\"";
        let text_pos = text_src.find("su").expect("text") + 2;
        let expr_src = "@f \"hello {su}\"";
        let expr_pos = expr_src.find("su").expect("expr") + 2;

        let (_, text_candidates) = h.complete(text_src, text_pos, &ctx).expect("completion");
        let (expr_start, expr_candidates) =
            h.complete(expr_src, expr_pos, &ctx).expect("completion");

        assert!(h.should_suppress(text_src, text_pos));
        assert!(text_candidates.is_empty());
        assert!(!h.should_suppress(expr_src, expr_pos));
        assert_eq!(expr_start, expr_pos - 2);
        assert_eq!(expr_candidates.len(), 1);
        assert_eq!(expr_candidates[0].replacement, "sum");
    }
}
