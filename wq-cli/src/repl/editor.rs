#![cfg(not(target_arch = "wasm32"))]

use std::borrow::Cow;

use wq_rl::completion::{Completer, FilenameCompleter, Pair};
use wq_rl::highlight::{CmdKind, Highlighter as RLHighlighter, InputAreaStyle};
use wq_rl::hint::{Hint as RLHint, Hinter};
use wq_rl::validate::{ValidationContext, ValidationResult, Validator};
use wq_rl::{Context as RLContext, Helper};
use wqpl::builtins::BuiltinPreset;
use wqpl::doc::{self, DocKind};
use wqpl::highlight::{Highlighter, cursor_context_at};
use wqpl::interpret::InterpreterKind;
use wqpl::session::Session;
use wqpl::session::dbglog::DEBUG_LOG_FLAG_NAMES;

use super::command::{self, ReplArgKind};
use crate::load::embed::embedded_aliases;

const RESET: &str = "\x1b[0m";
const REPL_INPUT_BG: &str = "\x1b[48;5;236m";
const REPL_INPUT_RESET: &str = "\x1b[0m";
const REPL_INPUT_TOKEN_RESET: &str = "\x1b[22;23;24;39m";
const HINT_DIM: &str = "\x1b[38;5;244m";
const HINT_RESET: &str = "\x1b[22;39m";
const MENU_MARKER: &str = "*";
const MENU_MARKER_DIM: &str = "\x1b[38;5;67m";
const MENU_MARKER_SELECTED: &str = "\x1b[38;5;150m";
const MENU_FOOTER: &str = "\x1b[38;5;248m";
const MENU_SELECTED: &str = "\x1b[1;38;5;252m";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WqHint {
    display: String,
    completion: Option<String>,
}

impl WqHint {
    fn completion(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            display: text.clone(),
            completion: Some(text),
        }
    }

    fn info(text: impl Into<String>) -> Self {
        Self {
            display: text.into(),
            completion: None,
        }
    }
}

impl RLHint for WqHint {
    fn display(&self) -> &str {
        &self.display
    }

    fn completion(&self) -> Option<&str> {
        self.completion.as_deref()
    }
}

pub struct WqReplHighlighter {
    highlighter: Highlighter,
    path_completer: FilenameCompleter,
    enabled: bool,
    builtin_names: Vec<String>,
    builtin_usages: Vec<String>,
    help_topics: Vec<(String, String)>,
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
        let (repl_names, repl_descs) = command::repl_hint_vectors();
        Self {
            highlighter: Highlighter::new(),
            path_completer: FilenameCompleter::new(),
            enabled: true,
            builtin_names: Vec::new(),
            builtin_usages: Vec::new(),
            help_topics: Self::collect_help_topics(),
            global_names: Vec::new(),
            global_types: Vec::new(),
            global_excerpts: Vec::new(),
            repl_names,
            repl_descs,
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

    fn collect_help_topics() -> Vec<(String, String)> {
        let mut topics = Vec::new();
        for topic in doc::all_topics() {
            let mut names = Vec::new();
            if topic.kind == DocKind::Builtin {
                names.extend(topic.aliases.clone());
            } else {
                names.push(topic.id.clone());
                names.extend(topic.aliases.clone());
            }
            for name in names {
                topics.push((name, topic.summary.clone()));
            }
        }
        topics.sort_by(|a, b| a.0.cmp(&b.0));
        topics.dedup_by(|a, b| a.0 == b.0);
        topics
    }

    fn first_arg_prefix(line: &str, pos: usize, cmd_end: usize) -> Option<(usize, &str)> {
        let after_cmd = line.get(cmd_end..pos)?;
        let leading_ws = after_cmd
            .char_indices()
            .find(|(_, ch)| !ch.is_ascii_whitespace())
            .map(|(idx, _)| idx)
            .unwrap_or(after_cmd.len());
        let arg_start = cmd_end + leading_ws;
        let arg_prefix = line.get(arg_start..pos)?;
        if !arg_prefix.is_empty() && arg_prefix.chars().last().is_some_and(char::is_whitespace) {
            return None;
        }
        if arg_prefix.split_whitespace().nth(1).is_some() {
            return None;
        }
        Some((arg_start, arg_prefix))
    }

    fn push_name_candidates<I, S>(candidates: &mut Vec<Pair>, names: I, prefix: &str, kind: &str)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for name in names {
            let name = name.as_ref();
            if name.starts_with(prefix) {
                candidates.push(Pair::new(name, name).with_kind(kind));
            }
        }
    }

    fn push_described_candidates(
        candidates: &mut Vec<Pair>,
        entries: impl IntoIterator<Item = (String, String)>,
        prefix: &str,
        kind: &str,
    ) {
        for (name, desc) in entries {
            if name.starts_with(prefix) {
                candidates.push(Pair::described(name.clone(), name, desc).with_kind(kind));
            }
        }
    }

    fn help_topic_entries(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.help_topics.iter().cloned()
    }

    fn help_topic_summary(&self, name: &str) -> Option<&str> {
        self.help_topics
            .iter()
            .find(|(topic_name, _)| topic_name == name)
            .map(|(_, summary)| summary.as_str())
    }

    fn embedded_load_entries(prefix: &str) -> Vec<Pair> {
        embedded_aliases()
            .map(|alias| format!("<{alias}>"))
            .filter(|name| name.starts_with(prefix))
            .map(|name| Pair::new(name.clone(), name).with_kind("embedded"))
            .collect()
    }

    fn command_arg_kind(cmd_word: &str) -> Option<ReplArgKind> {
        command::find_by_alias(cmd_word).and_then(|spec| spec.arg_kind())
    }

    fn repl_arg_candidate_names(&self, kind: ReplArgKind, prefix: &str) -> Vec<String> {
        match kind {
            ReplArgKind::BuiltinPreset => BuiltinPreset::names()
                .iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| (*name).to_string())
                .collect(),
            ReplArgKind::Interpreter => InterpreterKind::names()
                .iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| (*name).to_string())
                .collect(),
            ReplArgKind::HelpTopic => self
                .help_topics
                .iter()
                .filter(|(name, _)| name.starts_with(prefix))
                .map(|(name, _)| name.clone())
                .collect(),
            ReplArgKind::DebugFlags => {
                let aliases = ["0", "1", "2", "3", "4"];
                aliases
                    .iter()
                    .copied()
                    .chain(
                        DEBUG_LOG_FLAG_NAMES
                            .iter()
                            .flat_map(|(names, _)| names.iter())
                            .copied(),
                    )
                    .filter(|name| name.starts_with(prefix))
                    .map(str::to_string)
                    .collect()
            }
            ReplArgKind::FmtMode => ["on", "off", "nlcd", "oneline"]
                .iter()
                .filter(|name| name.starts_with(prefix))
                .map(|name| (*name).to_string())
                .collect(),
            ReplArgKind::LoadTarget if prefix.starts_with('<') => embedded_aliases()
                .map(|alias| format!("<{alias}>"))
                .filter(|name| name.starts_with(prefix))
                .collect(),
            ReplArgKind::LoadTarget | ReplArgKind::BoxSpec => Vec::new(),
        }
    }

    fn push_repl_arg_candidates(
        &self,
        candidates: &mut Vec<Pair>,
        kind: ReplArgKind,
        prefix: &str,
    ) {
        match kind {
            ReplArgKind::HelpTopic => {
                Self::push_described_candidates(
                    candidates,
                    self.help_topic_entries(),
                    prefix,
                    "help",
                );
            }
            ReplArgKind::LoadTarget if prefix.starts_with('<') => {
                candidates.extend(Self::embedded_load_entries(prefix));
            }
            ReplArgKind::BoxSpec => {}
            _ => {
                let label = match kind {
                    ReplArgKind::BuiltinPreset => "preset",
                    ReplArgKind::Interpreter => "interpreter",
                    ReplArgKind::DebugFlags => "debug",
                    ReplArgKind::FmtMode => "mode",
                    ReplArgKind::LoadTarget => "path",
                    ReplArgKind::HelpTopic | ReplArgKind::BoxSpec => unreachable!(),
                };
                Self::push_name_candidates(
                    candidates,
                    self.repl_arg_candidate_names(kind, prefix),
                    prefix,
                    label,
                );
            }
        }
    }

    fn repl_arg_hint(&self, kind: ReplArgKind, prefix: &str) -> Option<WqHint> {
        if kind == ReplArgKind::HelpTopic
            && let Some(summary) = self.help_topic_summary(prefix)
        {
            return Some(WqHint::info(format!("  {summary}")));
        }

        let mut candidates = self.repl_arg_candidate_names(kind, prefix);
        candidates.sort();
        candidates.dedup();
        let name = candidates.first()?;
        if name == prefix {
            return None;
        }
        let suffix = &name[prefix.len()..];
        Some(WqHint::completion(suffix))
    }

    /// Find the start index of the "word" that ends at `pos`.
    /// For REPL commands the leading `\` is included.
    fn current_word_start(line: &str, pos: usize) -> usize {
        let bytes = line.as_bytes();
        let pos = pos.min(bytes.len());
        let mut start = pos;
        while start > 0 {
            let b = bytes[start - 1];
            if b.is_ascii_alphanumeric() || b == b'_' || b == b'?' || b == b'\\' {
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

    fn is_completion_menu_hint(hint: &str) -> bool {
        hint.lines().any(Self::is_completion_menu_row)
    }

    fn is_completion_menu_row(line: &str) -> bool {
        line.starts_with("> * ") || line.starts_with("  * ")
    }

    fn colorize_completion_menu_hint(hint: &str) -> String {
        let mut out = String::from(HINT_DIM);
        let is_menu = Self::is_completion_menu_hint(hint);
        for (idx, line) in hint.split('\n').enumerate() {
            if idx > 0 {
                out.push('\n');
            }
            if let Some(rest) = line.strip_prefix("> * ") {
                out.push_str(MENU_SELECTED);
                out.push_str("> ");
                out.push_str(MENU_MARKER_SELECTED);
                out.push_str(MENU_MARKER);
                out.push(' ');
                out.push_str(MENU_SELECTED);
                out.push_str(rest);
                out.push_str(HINT_DIM);
            } else if let Some(rest) = line.strip_prefix("  * ") {
                out.push_str(HINT_DIM);
                out.push_str("  ");
                out.push_str(MENU_MARKER_DIM);
                out.push_str(MENU_MARKER);
                out.push(' ');
                out.push_str(HINT_DIM);
                out.push_str(rest);
            } else if is_menu && !line.is_empty() {
                out.push_str(MENU_FOOTER);
                out.push_str(line);
                out.push_str(HINT_DIM);
            } else {
                out.push_str(line);
            }
        }
        out.push_str(HINT_RESET);
        out
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
        if trimmed.starts_with('\\') {
            // Check if we're completing an argument (cursor after first whitespace word)
            if let Some(first_space) = trimmed.find(|c: char| c.is_ascii_whitespace()) {
                let cmd_end = line.len() - trimmed.len() + first_space;
                if let Some((arg_start, arg_prefix)) = Self::first_arg_prefix(line, pos, cmd_end) {
                    let cmd_word = &trimmed[..first_space];
                    if let Some(arg_kind) = Self::command_arg_kind(cmd_word) {
                        if arg_kind == ReplArgKind::LoadTarget && !arg_prefix.starts_with('<') {
                            let (start, mut paths) =
                                self.path_completer.complete_path(line, pos)?;
                            for path in &mut paths {
                                path.kind = Some("path".to_string());
                            }
                            return Ok((start, paths));
                        }
                        self.push_repl_arg_candidates(&mut candidates, arg_kind, arg_prefix);
                    }
                    candidates.sort_by(|a, b| a.display.cmp(&b.display));
                    candidates.dedup_by(|a, b| a.display == b.display);
                    return Ok((arg_start, candidates));
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
                candidates.push(Pair::described(name, name, desc).with_kind("command"));
            }
        } else {
            if prefix.is_empty() {
                return Ok((pos, Vec::new()));
            }
            let names = &self.builtin_names;
            for (idx, name) in names
                .iter()
                .enumerate()
                .filter(|(_, name)| name.starts_with(prefix))
            {
                let candidate = self
                    .builtin_usages
                    .get(idx)
                    .filter(|usage| !usage.is_empty())
                    .map_or_else(
                        || Pair::new(name.clone(), name.clone()),
                        |usage| Pair::described(name.clone(), name.clone(), usage.clone()),
                    )
                    .with_kind("builtin");
                candidates.push(candidate);
            }
            let globals = &self.global_names;
            for (idx, name) in globals
                .iter()
                .enumerate()
                .filter(|(_, name)| name.starts_with(prefix))
            {
                let ty = self.global_types.get(idx).cloned().unwrap_or_default();
                let excerpt = self.global_excerpts.get(idx).cloned().unwrap_or_default();
                let description = match (ty.is_empty(), excerpt.is_empty()) {
                    (true, true) => None,
                    (true, false) => Some(excerpt),
                    (false, true) => Some(format!(":{ty}")),
                    (false, false) => Some(format!(":{ty} {excerpt}")),
                };
                let candidate = description.map_or_else(
                    || Pair::new(name.clone(), name.clone()),
                    |desc| Pair::described(name.clone(), name.clone(), desc),
                );
                candidates.push(candidate.with_kind("global"));
            }
        }
        candidates.sort_by(|a, b| a.display.cmp(&b.display));
        candidates.dedup_by(|a, b| a.display == b.display);
        Ok((start, candidates))
    }
}

impl Hinter for WqReplHighlighter {
    type Hint = WqHint;
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
        if trimmed.starts_with('\\') {
            // REPL command argument hinting
            if let Some(first_space) = trimmed.find(|c: char| c.is_ascii_whitespace()) {
                let cmd_end = line.len() - trimmed.len() + first_space;
                if let Some((_, arg_prefix)) = Self::first_arg_prefix(line, pos, cmd_end) {
                    let cmd_word = &trimmed[..first_space];
                    if let Some(arg_kind) = Self::command_arg_kind(cmd_word)
                        && let Some(hint) = self.repl_arg_hint(arg_kind, arg_prefix)
                    {
                        return Some(hint);
                    }
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
                return hint.as_ref().map(|h| WqHint::info(h.to_string()));
            }
            let suffix = &name[prefix.len()..];
            return Some(WqHint::completion(suffix));
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
            return hint.as_ref().map(|h| WqHint::info(h.to_string()));
        }
        let suffix = &name[prefix.len()..];
        Some(WqHint::completion(suffix))
    }
}

impl Validator for WqReplHighlighter {
    fn validate(&self, ctx: &mut ValidationContext) -> wq_rl::Result<ValidationResult> {
        let input = ctx.input();
        if input.trim().is_empty() {
            return Ok(ValidationResult::Valid(None));
        }
        if input.trim_start().starts_with('\\') {
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
            if Self::is_completion_menu_hint(hint) {
                Cow::Owned(Self::colorize_completion_menu_hint(hint))
            } else {
                Cow::Owned(format!("{HINT_DIM}{hint}{HINT_RESET}"))
            }
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
    use wq_rl::highlight::Highlighter as _;
    use wq_rl::hint::Hint as _;
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
    fn completion_menu_hint_coloring_preserves_visible_text() {
        let h = WqReplHighlighter::new();
        let src = "\n> * alpha    first item\n  * beta     second item\n  1-2 of 2  selected 1/2  builtin  alpha";
        let out = h.highlight_hint(src);

        if cfg!(unix) {
            assert!(out.contains(MENU_MARKER_SELECTED));
            assert!(out.contains(MENU_FOOTER));
        }
        assert_eq!(strip_ansi(&out), src);

        let selected_line = "> * alpha    first item";
        let selected_out = h.highlight_hint(selected_line);

        if cfg!(unix) {
            assert!(selected_out.contains(MENU_MARKER_SELECTED));
        }
        assert_eq!(strip_ansi(&selected_out), selected_line);
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

    #[test]
    fn expression_completion_carries_menu_metadata() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum[xs*]".to_string()]);
        h.set_global_hints(
            vec!["score".to_string()],
            vec!["num".to_string()],
            vec!["score:42".to_string()],
        );
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (_, candidates) = h.complete("s", 1, &ctx).expect("completion");
        let sum = candidates
            .iter()
            .find(|candidate| candidate.replacement == "sum")
            .expect("sum candidate");
        let score = candidates
            .iter()
            .find(|candidate| candidate.replacement == "score")
            .expect("score candidate");

        assert_eq!(sum.kind.as_deref(), Some("builtin"));
        assert_eq!(sum.description.as_deref(), Some("sum[xs*]"));
        assert_eq!(score.kind.as_deref(), Some("global"));
        assert_eq!(score.description.as_deref(), Some(":num score:42"));
    }

    #[test]
    fn hint_prefix_completion_is_insertable() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum[xs*]".to_string()]);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let hint = h.hint("su", 2, &ctx).expect("hint");

        assert_eq!(hint.display(), "m");
        assert_eq!(hint.completion(), Some("m"));
    }

    #[test]
    fn exact_builtin_usage_hint_is_display_only() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum[xs*]".to_string()]);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let hint = h.hint("sum", 3, &ctx).expect("hint");

        assert_eq!(hint.display(), "  sum[xs*]");
        assert_eq!(hint.completion(), None);
    }

    #[test]
    fn command_completion_includes_load_directives() {
        let mut h = WqReplHighlighter::new();
        h.set_repl_hints(
            vec![
                r"\load".to_string(),
                r"\l".to_string(),
                r"\p".to_string(),
                r"\help".to_string(),
            ],
            vec![
                "load embedded script or file".to_string(),
                "load embedded script or file".to_string(),
                "load prelude".to_string(),
                "show help".to_string(),
            ],
        );
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (_, load_candidates) = h.complete(r"\lo", 3, &ctx).expect("completion");
        let (_, p_candidates) = h.complete(r"\p", 2, &ctx).expect("completion");

        assert!(load_candidates.iter().any(|c| c.replacement == r"\load"));
        assert!(p_candidates.iter().any(|c| c.replacement == r"\p"));
    }

    #[test]
    fn help_completion_includes_static_topics() {
        let h = WqReplHighlighter::new();
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (start, candidates) = h
            .complete(r"\help assign", r"\help assign".len(), &ctx)
            .expect("completion");

        assert_eq!(start, r"\help ".len());
        assert!(
            candidates
                .iter()
                .any(|c| c.replacement == "assignment-forms")
        );
    }

    #[test]
    fn load_completion_includes_embedded_alias() {
        let h = WqReplHighlighter::new();
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (start, candidates) = h
            .complete(r"\load <pre", r"\load <pre".len(), &ctx)
            .expect("completion");

        assert_eq!(start, r"\load ".len());
        assert!(candidates.iter().any(|c| c.replacement == "<prelude>"));
    }

    #[test]
    fn exact_command_hint_is_display_only() {
        let mut h = WqReplHighlighter::new();
        h.set_repl_hints(vec![r"\help".to_string()], vec!["show help".to_string()]);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let hint = h.hint(r"\help", 5, &ctx).expect("hint");

        assert_eq!(hint.display(), "  show help");
        assert_eq!(hint.completion(), None);
    }

    #[test]
    fn exact_help_topic_hint_shows_summary_display_only() {
        let h = WqReplHighlighter::new();
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let hint = h
            .hint(
                r"\help assignment-forms",
                r"\help assignment-forms".len(),
                &ctx,
            )
            .expect("hint");

        assert_eq!(
            hint.display(),
            "  Bind, update, unpack, or checkpoint values with assignment forms."
        );
        assert_eq!(hint.completion(), None);
    }
}
