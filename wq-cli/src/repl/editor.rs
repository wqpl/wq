#![cfg(not(target_arch = "wasm32"))]

use std::borrow::Cow;
use std::collections::HashMap;

use wq_rl::completion::{Completer, FilenameCompleter, Pair};
use wq_rl::highlight::{CmdKind, Highlighter as RLHighlighter, InputAreaStyle};
use wq_rl::hint::{Hint as RLHint, Hinter};
use wq_rl::validate::{ValidationContext, ValidationResult, Validator};
use wq_rl::{Context as RLContext, Helper};
use wqpl::builtins::{BuiltinNamedArg, BuiltinPreset};
use wqpl::completion::{self as wq_completion, CompletionCandidate};
use wqpl::doc::{self, DocKind};
use wqpl::frontend::Frontend;
use wqpl::highlight::{Highlighter, cursor_context_at};
use wqpl::interpret::InterpreterKind;
use wqpl::session::dbglog::DEBUG_LOG_FLAG_NAMES;

use super::command::{self, ReplArgKind};
use super::input::{WqGlobalHint, WqInputMode};
use crate::load::embed::embedded_aliases;
use crate::wqdb::editor as wqdb_editor;

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
const WQDB_COMMAND_COLOR: &str = "\x1b[32m";
const WQDB_UNKNOWN_COMMAND_COLOR: &str = "\x1b[31m";
const WQDB_SUBCOMMAND_COLOR: &str = "\x1b[96m";
const WQDB_FLAG_COLOR: &str = "\x1b[95m";
const WQDB_NUMBER_COLOR: &str = "\x1b[93m";
const WQDB_ARGUMENT_COLOR: &str = "\x1b[93m";

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
    frontend: Frontend,
    highlighter: Highlighter,
    path_completer: FilenameCompleter,
    enabled: bool,
    builtin_names: Vec<String>,
    builtin_usages: Vec<String>,
    builtin_named_args: HashMap<String, Vec<BuiltinNamedArg>>,
    help_topics: Vec<(String, String)>,
    global_hints: Vec<WqGlobalHint>,
    wqdb_function_names: Vec<String>,
    repl_names: Vec<String>,
    repl_descs: Vec<String>,
    hints_enabled: bool,
    input_mode: WqInputMode,
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
            frontend: Frontend::default(),
            highlighter: Highlighter::new(),
            path_completer: FilenameCompleter::new(),
            enabled: true,
            builtin_names: Vec::new(),
            builtin_usages: Vec::new(),
            builtin_named_args: HashMap::new(),
            help_topics: Self::collect_help_topics(),
            global_hints: Vec::new(),
            wqdb_function_names: Vec::new(),
            repl_names,
            repl_descs,
            hints_enabled: true,
            input_mode: WqInputMode::Wq,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_input_mode(&mut self, mode: WqInputMode) {
        self.input_mode = mode;
    }

    pub fn input_mode(&self) -> WqInputMode {
        self.input_mode
    }

    pub fn set_hints_enabled(&mut self, on: bool) {
        self.hints_enabled = on;
    }

    pub fn hints_enabled(&self) -> bool {
        self.hints_enabled
    }

    #[cfg(test)]
    pub fn set_builtin_hints(&mut self, names: Vec<String>, usages: Vec<String>) {
        self.builtin_names = names;
        self.builtin_usages = usages;
        self.builtin_named_args.clear();
    }

    pub fn set_builtin_completion_candidates(&mut self, candidates: Vec<CompletionCandidate>) {
        self.builtin_names.clear();
        self.builtin_usages.clear();
        self.builtin_named_args.clear();

        self.builtin_names.reserve(candidates.len());
        self.builtin_usages.reserve(candidates.len());
        for candidate in candidates {
            if !candidate.named_args.is_empty() {
                self.builtin_named_args
                    .insert(candidate.label.clone(), candidate.named_args);
            }
            self.builtin_names.push(candidate.label);
            self.builtin_usages
                .push(candidate.detail.unwrap_or_default());
        }
    }

    pub fn set_builtins_preset(&mut self, preset: BuiltinPreset) {
        self.frontend = Frontend::with_preset(preset);
        self.highlighter = Highlighter::with_preset(preset);
    }

    pub fn set_global_hints(&mut self, hints: Vec<WqGlobalHint>) {
        self.global_hints = hints;
    }

    pub fn set_wqdb_function_hints(&mut self, names: Vec<String>) {
        self.wqdb_function_names = names;
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

    fn colorize_wq_input(&self, line: &str) -> String {
        let semantic_spans = self.semantic_highlight_spans(line);
        self.highlighter
            .highlight_ansi_with_semantic_spans_and_reset(
                line,
                &semantic_spans,
                REPL_INPUT_TOKEN_RESET,
            )
    }

    fn colorize_wqdb_input(&self, line: &str) -> String {
        let mut out = String::with_capacity(line.len() * 2);
        let mut copied = 0;
        for span in wqdb_editor::token_spans(line) {
            out.push_str(&line[copied..span.start]);
            let color = match span.kind {
                wqdb_editor::TokenKind::Command => WQDB_COMMAND_COLOR,
                wqdb_editor::TokenKind::UnknownCommand => WQDB_UNKNOWN_COMMAND_COLOR,
                wqdb_editor::TokenKind::Subcommand => WQDB_SUBCOMMAND_COLOR,
                wqdb_editor::TokenKind::Flag => WQDB_FLAG_COLOR,
                wqdb_editor::TokenKind::Number => WQDB_NUMBER_COLOR,
                wqdb_editor::TokenKind::Argument => WQDB_ARGUMENT_COLOR,
            };
            out.push_str(color);
            out.push_str(&line[span.start..span.end]);
            out.push_str(REPL_INPUT_TOKEN_RESET);
            copied = span.end;
        }
        out.push_str(&line[copied..]);
        out
    }

    fn colorize_input(&self, line: &str) -> String {
        match self.input_mode {
            WqInputMode::Wq => self.colorize_wq_input(line),
            WqInputMode::Wqdb => self.colorize_wqdb_input(line),
        }
    }

    fn complete_wqdb(&self, line: &str, pos: usize) -> (usize, Vec<Pair>) {
        let target = wqdb_editor::cursor_target(line, pos);
        let start = target.start();
        let (command, previous_args, prefix) = match &target {
            wqdb_editor::CursorTarget::Empty { .. } => return (start, Vec::new()),
            wqdb_editor::CursorTarget::Command { prefix, .. } => {
                let candidates = wqdb_editor::command_entries(prefix)
                    .into_iter()
                    .map(|entry| {
                        Pair::described(entry.name, entry.name, entry.summary).with_kind("command")
                    })
                    .collect();
                return (start, candidates);
            }
            wqdb_editor::CursorTarget::Argument {
                command,
                prefix,
                previous_args,
                ..
            } => (*command, previous_args.as_slice(), *prefix),
        };
        let mut candidates = wqdb_editor::argument_candidates(command, previous_args, prefix)
            .into_iter()
            .map(|candidate| {
                Pair::described(candidate.value, candidate.value, candidate.description)
                    .with_kind(candidate.kind)
            })
            .collect::<Vec<_>>();
        match wqdb_editor::dynamic_argument_kind(command, previous_args) {
            Some(wqdb_editor::DynamicArgumentKind::Function) => {
                for name in self
                    .wqdb_function_names
                    .iter()
                    .filter(|name| name.starts_with(prefix))
                {
                    candidates
                        .push(Pair::described(name, name, "debug function").with_kind("function"));
                }
            }
            Some(wqdb_editor::DynamicArgumentKind::Symbol) => {
                for hint in self
                    .global_hints
                    .iter()
                    .filter(|hint| hint.name.starts_with(prefix))
                {
                    candidates.push(
                        Pair::described(&hint.name, &hint.name, "track global symbol")
                            .with_kind("global"),
                    );
                }
            }
            Some(wqdb_editor::DynamicArgumentKind::Command) => {
                candidates.extend(
                    wqdb_editor::command_entries(prefix)
                        .into_iter()
                        .map(|entry| {
                            Pair::described(entry.name, entry.name, entry.summary)
                                .with_kind("command")
                        }),
                );
            }
            None => {}
        }
        candidates.sort_by(|left, right| left.display.cmp(&right.display));
        candidates.dedup_by(|left, right| left.replacement == right.replacement);
        (start, candidates)
    }

    fn complete_builtin_named_arg(&self, line: &str, pos: usize) -> Option<(usize, Vec<Pair>)> {
        let context =
            wq_completion::builtin_named_arg_completion_context(&self.frontend, line, pos)?;
        let named_args = self.builtin_named_args.get(&context.builtin_name)?;
        let mut candidates = named_args
            .iter()
            .filter(|arg| arg.name.starts_with(&context.prefix))
            .filter(|arg| !context.used_names.iter().any(|name| name == arg.name))
            .map(|arg| {
                let replacement = format!("`{}:", arg.name);
                Pair::described(
                    replacement.clone(),
                    replacement,
                    format!("{} · {}", arg.value_label, arg.summary),
                )
                .with_kind("named argument")
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| left.display.cmp(&right.display));
        Some((context.replace_start, candidates))
    }

    fn hint_wqdb(&self, line: &str, pos: usize) -> Option<WqHint> {
        let target = wqdb_editor::cursor_target(line, pos);
        let (command, prefix) = match &target {
            wqdb_editor::CursorTarget::Empty { .. } => return None,
            wqdb_editor::CursorTarget::Command { prefix, .. } => {
                if let Some(entry) = wqdb_editor::command_entry(prefix) {
                    return Some(WqHint::info(format!(
                        "  {}  {}",
                        entry.usage, entry.summary
                    )));
                }
                let entry = wqdb_editor::command_entries(prefix).into_iter().next()?;
                return Some(WqHint::completion(&entry.name[prefix.len()..]));
            }
            wqdb_editor::CursorTarget::Argument {
                command, prefix, ..
            } => (*command, *prefix),
        };
        let (_, candidates) = self.complete_wqdb(line, pos);
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.replacement == prefix)
        {
            return candidate
                .description
                .as_ref()
                .map(|description| WqHint::info(format!("  {description}")));
        }
        if let Some(candidate) = candidates.first() {
            return Some(WqHint::completion(&candidate.replacement[prefix.len()..]));
        }
        wqdb_editor::command_entry(command)
            .map(|entry| WqHint::info(format!("  {}  {}", entry.usage, entry.summary)))
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
        self.frontend
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
        if self.input_mode == WqInputMode::Wqdb {
            return Ok(self.complete_wqdb(line, pos));
        }
        if let Some(completion) = self.complete_builtin_named_arg(line, pos) {
            return Ok(completion);
        }
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
            for hint in self
                .global_hints
                .iter()
                .filter(|hint| hint.name.starts_with(prefix))
            {
                let description = match (hint.category.is_empty(), hint.excerpt.is_empty()) {
                    (true, true) => None,
                    (true, false) => Some(hint.excerpt.clone()),
                    (false, true) => Some(format!(":{}", hint.category)),
                    (false, false) => Some(format!(":{} {}", hint.category, hint.excerpt)),
                };
                let candidate = description.map_or_else(
                    || Pair::new(hint.name.clone(), hint.name.clone()),
                    |desc| Pair::described(hint.name.clone(), hint.name.clone(), desc),
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
        if !self.hints_enabled {
            return None;
        }
        if self.input_mode == WqInputMode::Wqdb {
            return self.hint_wqdb(line, pos);
        }
        if self.should_suppress(line, pos) {
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
        let mut merged: Vec<(String, Option<String>)> = Vec::new();
        for (name, usage) in names.iter().zip(usages.iter()) {
            merged.push((name.clone(), Some(format!("  {}", usage.clone()))));
        }
        for hint in &self.global_hints {
            merged.push((
                hint.name.clone(),
                Some(format!("  :{} {}", hint.category, hint.excerpt)),
            ));
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
        if self.input_mode == WqInputMode::Wqdb {
            return Ok(ValidationResult::Valid(None));
        }
        if input.trim().is_empty() {
            return Ok(ValidationResult::Valid(None));
        }
        if input.trim_start().starts_with('\\') {
            return Ok(ValidationResult::Valid(None));
        }
        if self.frontend.is_complete_input(input) {
            Ok(ValidationResult::Valid(None))
        } else {
            Ok(ValidationResult::Incomplete(Some("... ".to_string())))
        }
    }
}

impl RLHighlighter for WqReplHighlighter {
    fn input_area_style(&self) -> Option<InputAreaStyle> {
        Some(InputAreaStyle {
            background: REPL_INPUT_BG,
            reset: REPL_INPUT_RESET,
            horizontal_padding: 1,
            vertical_padding: 1,
        })
    }

    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.enabled() {
            return std::borrow::Cow::Borrowed(line);
        }
        Cow::Owned(self.colorize_input(line))
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Owned(prompt.replace(RESET, REPL_INPUT_TOKEN_RESET))
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        if Self::is_completion_menu_hint(hint) {
            Cow::Owned(Self::colorize_completion_menu_hint(hint))
        } else {
            Cow::Owned(format!("{HINT_DIM}{hint}{HINT_RESET}"))
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
    fn input_area_style_is_platform_independent() {
        let h = WqReplHighlighter::new();

        assert_eq!(
            Some(InputAreaStyle {
                background: REPL_INPUT_BG,
                reset: REPL_INPUT_RESET,
                horizontal_padding: 1,
                vertical_padding: 1,
            }),
            h.input_area_style()
        );
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

        assert!(out.contains(MENU_MARKER_SELECTED));
        assert!(out.contains(MENU_FOOTER));
        assert_eq!(strip_ansi(&out), src);

        let selected_line = "> * alpha    first item";
        let selected_out = h.highlight_hint(selected_line);

        assert!(selected_out.contains(MENU_MARKER_SELECTED));
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
    fn builtin_preset_updates_parsing_and_highlighting() {
        let mut h = WqReplHighlighter::new();

        h.set_builtins_preset(BuiltinPreset::Minimal);

        assert!(!h.frontend.builtins().is_enabled_name("print"));
        assert!(h.highlight_text("print[]").contains("\x1b[38;5;117mprint"));
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
        h.set_global_hints(vec![WqGlobalHint {
            name: "score".to_string(),
            category: "int".to_string(),
            excerpt: "score:42".to_string(),
        }]);
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
        assert_eq!(score.description.as_deref(), Some(":int score:42"));
    }

    #[test]
    fn builtin_named_argument_completion_inserts_name_and_colon() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_completion_candidates(wqpl::completion::builtin_completion_candidates(
            &wqpl::builtins::Builtins::new(),
            false,
        ));
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);
        let src = "split[\"a,b\";\",\";`ma";

        let (start, candidates) = h.complete(src, src.len(), &ctx).expect("completion");

        assert_eq!(&src[start..], "`ma");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].replacement, "`max:");
        assert_eq!(candidates[0].kind.as_deref(), Some("named argument"));
        assert_eq!(
            candidates[0].description.as_deref(),
            Some("n · maximum number of splits")
        );
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

    #[test]
    fn wqdb_mode_completes_debugger_commands_instead_of_wq_names() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum[xs*]".to_string()]);
        h.set_input_mode(WqInputMode::Wqdb);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (start, candidates) = h.complete("s", 1, &ctx).expect("completion");

        assert_eq!(start, 0);
        assert!(candidates.iter().any(|c| c.replacement == "step"));
        assert!(candidates.iter().any(|c| c.replacement == "stop-hook"));
        assert!(!candidates.iter().any(|c| c.replacement == "sum"));
    }

    #[test]
    fn wqdb_mode_completes_command_arguments() {
        let mut h = WqReplHighlighter::new();
        h.set_input_mode(WqInputMode::Wqdb);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (start, candidates) = h.complete("g i", 3, &ctx).expect("completion");

        assert_eq!(start, 2);
        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.replacement.as_str())
                .collect::<Vec<_>>(),
            vec!["inst"]
        );
        assert_eq!(candidates[0].kind.as_deref(), Some("granularity"));
    }

    #[test]
    fn wqdb_mode_completes_dynamic_debugger_arguments() {
        let mut h = WqReplHighlighter::new();
        h.set_global_hints(vec![WqGlobalHint {
            name: "count".to_string(),
            category: "function".to_string(),
            excerpt: "3".to_string(),
        }]);
        h.set_wqdb_function_hints(vec!["worker".to_string()]);
        h.set_input_mode(WqInputMode::Wqdb);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (_, function_candidates) = h.complete("bf w", 4, &ctx).expect("function completion");
        let (_, hook_candidates) = h
            .complete("stop-hook add gr", "stop-hook add gr".len(), &ctx)
            .expect("hook completion");
        let (_, nested_candidates) = h
            .complete("stop-hook add track a", "stop-hook add track a".len(), &ctx)
            .expect("nested command completion");

        assert_eq!(function_candidates.len(), 1);
        assert_eq!(function_candidates[0].replacement, "worker");
        assert_eq!(function_candidates[0].kind.as_deref(), Some("function"));
        assert!(
            hook_candidates
                .iter()
                .any(|candidate| candidate.replacement == "granularity")
        );
        assert!(
            nested_candidates
                .iter()
                .any(|candidate| candidate.replacement == "add")
        );
    }

    #[test]
    fn wqdb_mode_exact_command_hint_shows_usage() {
        let mut h = WqReplHighlighter::new();
        h.set_input_mode(WqInputMode::Wqdb);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let hint = h.hint("g", 1, &ctx).expect("hint");

        assert!(hint.display().contains("g [line|expr|inst]"));
        assert!(!hint.display().contains(" | "));
        assert!(hint.display().contains("show or set stepping granularity"));
        assert_eq!(hint.completion(), None);
    }

    #[test]
    fn wqdb_mode_highlights_nested_command_and_number() {
        let mut h = WqReplHighlighter::new();
        h.set_input_mode(WqInputMode::Wqdb);
        let src = "stop-hook add b 12";

        let out = h.colorize_input(src);

        assert!(out.contains(&format!("{WQDB_COMMAND_COLOR}stop-hook")));
        assert!(out.contains(&format!("{WQDB_SUBCOMMAND_COLOR}add")));
        assert!(out.contains(&format!("{WQDB_COMMAND_COLOR}b")));
        assert!(out.contains(&format!("{WQDB_NUMBER_COLOR}12")));
        assert_eq!(strip_ansi(&out), src);
    }

    #[test]
    fn leaving_wqdb_mode_restores_wq_completion() {
        let mut h = WqReplHighlighter::new();
        h.set_builtin_hints(vec!["sum".to_string()], vec!["sum[xs*]".to_string()]);
        h.set_input_mode(WqInputMode::Wqdb);
        h.set_input_mode(WqInputMode::Wq);
        let history = DefaultHistory::new();
        let ctx = RLContext::new(&history);

        let (_, candidates) = h.complete("su", 2, &ctx).expect("completion");

        assert!(candidates.iter().any(|c| c.replacement == "sum"));
        assert!(!candidates.iter().any(|c| c.replacement == "step"));
    }
}
