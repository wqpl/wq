use std::borrow::Cow;
use std::cell::RefCell;

use rustyline::Context as RLContext;
use rustyline::Helper;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::{CmdKind, Highlighter as RLHighlighter};
use rustyline::hint::Hinter;
use rustyline::validate::Validator;

use tree_sitter::Language;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter as TSHighlighter};

const HIGHLIGHTS_SCM: &str = include_str!("../../vendor/tree-sitter-wq/queries/highlights.scm");
const INJECTIONS_SCM: &str = "";
const LOCALS_SCM: &str = "";

static HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.call",
    "function.builtin",
    "keyword",
    "keyword.return",
    "module",
    "number",
    "boolean",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "character",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
    "meta",
];

const RESET: &str = "\x1b[0m";

pub struct TSHelper {
    cfg: HighlightConfiguration,
    ts: RefCell<TSHighlighter>, // Highlighter needs &mut self; wrap for interior mutability
    enabled: bool,
}

impl Default for TSHelper {
    fn default() -> Self {
        TSHelper::new()
    }
}

impl TSHelper {
    pub fn new() -> Self {
        // LANGUAGE is a LanguageFn; convert into tree_sitter::Language.
        let lang: Language = tree_sitter_wq::LANGUAGE.into(); // or your_lang::language()

        let mut cfg =
            HighlightConfiguration::new(lang, "wq", HIGHLIGHTS_SCM, INJECTIONS_SCM, LOCALS_SCM)
                .expect("valid highlight queries");
        cfg.configure(HIGHLIGHT_NAMES); // register capture names

        Self {
            cfg,
            ts: RefCell::new(TSHighlighter::new()),
            enabled: true,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    #[inline]
    fn style_for_name(name: &str) -> (&'static str, &'static str) {
        match name {
            "attribute" => ("\x1b[3;38;5;201m", RESET),
            "comment" => ("\x1b[3;38;5;244m", RESET),
            "constant" => ("\x1b[38;5;202m", RESET),
            "constant.builtin" => ("\x1b[1;38;5;208m", RESET),
            "constructor" => ("\x1b[38;5;45m", RESET),
            "embedded" => ("\x1b[3;38;5;99m", RESET),
            "function" => ("\x1b[38;5;39m", RESET),
            "function.call" => ("\x1b[38;5;39m", RESET),
            "function.builtin" => ("\x1b[1;38;5;81m", RESET),
            "keyword" => ("\x1b[38;5;199m", RESET),
            "keyword.return" => ("\x1b[38;5;220m", RESET),
            "module" => ("\x1b[38;5;135m", RESET),
            "number" => ("\x1b[38;5;214m", RESET),
            "boolean" => ("\x1b[1;38;5;224m", RESET),
            "operator" => ("\x1b[1;38;5;180m", RESET),
            "property" => ("\x1b[38;5;207m", RESET),
            "property.builtin" => ("\x1b[1;38;5;213m", RESET),
            "punctuation" => ("\x1b[38;5;250m", RESET),
            "punctuation.bracket" => ("\x1b[38;5;123m", RESET),
            "punctuation.delimiter" => ("\x1b[38;5;160m", RESET),
            "punctuation.special" => ("\x1b[38;5;93m", RESET),
            "string" => ("\x1b[38;5;114m", RESET),
            "string.special" => ("\x1b[38;5;83m", RESET),
            "character" => ("\x1b[38;5;118m", RESET),
            "tag" => ("\x1b[38;5;196m", RESET),
            "type" => ("\x1b[38;5;99m", RESET),
            "type.builtin" => ("\x1b[1;38;5;93m", RESET),
            "variable" => ("\x1b[38;5;205m", RESET),
            "variable.builtin" => ("\x1b[4;38;5;205m", RESET),
            "variable.parameter" => ("\x1b[4;38;5;213m", RESET),
            "meta" => ("\x1b[1;4;38;5;213m", RESET),
            // Fallback: for an unknown dotted scope, try its base (e.g. "foo.bar" -> "foo")
            other => {
                if let Some((base, _)) = other.split_once('.') {
                    return Self::style_for_name(base);
                }
                (RESET, RESET)
            }
        }
    }

    fn style_for(idx: usize) -> (&'static str, &'static str) {
        Self::style_for_name(HIGHLIGHT_NAMES[idx])
    }

    fn colorize(&self, line: &str) -> String {
        let bytes = line.as_bytes();
        let mut out = String::with_capacity(line.len() + 16);
        let mut stack: Vec<usize> = Vec::new();

        // Keep the RefMut alive across iteration to avoid E0716.
        let mut ts = self.ts.borrow_mut();
        let iter = ts
            .highlight(&self.cfg, bytes, None, |_| None)
            .expect("highlight ok");

        for ev in iter {
            match ev.expect("event") {
                HighlightEvent::HighlightStart(h) => stack.push(h.0),
                HighlightEvent::HighlightEnd => {
                    stack.pop();
                }
                HighlightEvent::Source { start, end } => {
                    let s = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                    if let Some(&idx) = stack.last() {
                        let (on, off) = Self::style_for(idx);
                        out.push_str(on);
                        out.push_str(s);
                        out.push_str(off);
                    } else {
                        out.push_str(s);
                    }
                }
            }
        }
        out
    }
}

impl Helper for TSHelper {}

impl Completer for TSHelper {
    type Candidate = Pair;
    fn complete(
        &self,
        _line: &str,
        pos: usize,
        _ctx: &RLContext<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        Ok((pos, Vec::new()))
    }
}

impl Hinter for TSHelper {
    type Hint = String;
    fn hint(&self, _line: &str, _pos: usize, _ctx: &RLContext<'_>) -> Option<Self::Hint> {
        None
    }
}

impl Validator for TSHelper {}

impl RLHighlighter for TSHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if !self.enabled() {
            return std::borrow::Cow::Borrowed(line);
        }
        Cow::Owned(self.colorize(line))
    }
    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(
        &'s self,
        prompt: &'p str,
        _default: bool,
    ) -> Cow<'b, str> {
        Cow::Borrowed(prompt)
    }
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Borrowed(hint)
    }
    fn highlight_candidate<'c>(
        &self,
        cand: &'c str,
        _t: rustyline::config::CompletionType,
    ) -> Cow<'c, str> {
        Cow::Borrowed(cand)
    }
    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        true
    }
}
