use std::vec::IntoIter;

use crate::completion::{Completer, Pair};
use crate::config::{CompletionType, Config, EditMode};
use crate::edit::init_state;
use crate::highlight::Highlighter;
use crate::hint::Hinter;
use crate::history::History as _;
use crate::keymap::{Bindings, Cmd, InputState, Refresher as _};
use crate::keys::{KeyCode as K, KeyEvent, KeyEvent as E, Modifiers as M};
use crate::tty::Sink;
use crate::validate::Validator;
use crate::{
    Context, DefaultEditor, Helper, ReadlineError, Result, apply_backspace_direct, readline_direct,
};

mod common;
mod emacs;
mod history;
mod vi_cmd;
mod vi_insert;

fn init_editor(mode: EditMode, keys: &[KeyEvent]) -> DefaultEditor {
    let config = Config::builder().edit_mode(mode).build();
    let mut editor = DefaultEditor::with_config(config).unwrap();
    editor.term.keys.extend(keys.iter().copied());
    editor
}

struct SimpleCompleter;
impl Completer for SimpleCompleter {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        _pos: usize,
        _ctx: &Context<'_>,
    ) -> Result<(usize, Vec<String>)> {
        Ok((
            0,
            if line == "rus" {
                vec![line.to_owned() + "t"]
            } else if line == "\\hbar" {
                vec!["ℏ".to_owned()]
            } else {
                vec![]
            },
        ))
    }
}
impl Hinter for SimpleCompleter {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        None
    }
}

impl Helper for SimpleCompleter {}
impl Highlighter for SimpleCompleter {}
impl Validator for SimpleCompleter {}

struct DescribedCompleter;
impl Completer for DescribedCompleter {
    type Candidate = Pair;

    fn complete(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Result<(usize, Vec<Pair>)> {
        Ok((
            0,
            vec![
                Pair::described(
                    "assignment-forms",
                    "assignment-forms",
                    "Bind, update, unpack, or checkpoint values with assignment forms and more tail.",
                ),
                Pair::described("assert", "assert", "Assert that an expression is true."),
            ],
        ))
    }
}
impl Hinter for DescribedCompleter {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        None
    }
}

impl Helper for DescribedCompleter {}
impl Highlighter for DescribedCompleter {}
impl Validator for DescribedCompleter {}

struct MenuCompleter;
impl Completer for MenuCompleter {
    type Candidate = Pair;

    fn complete(&self, _line: &str, _pos: usize, _ctx: &Context<'_>) -> Result<(usize, Vec<Pair>)> {
        Ok((
            0,
            vec![
                Pair::described("alpha", "alpha", "first item").with_kind("builtin"),
                Pair::described("beta", "beta", "second item").with_kind("global"),
                Pair::described("charlie", "charlie", "third item"),
                Pair::described("delta", "delta", "fourth item"),
                Pair::described("echo", "echo", "fifth item"),
                Pair::described("foxtrot", "foxtrot", "sixth item"),
                Pair::described("golf", "golf", "seventh item"),
                Pair::described("hotel", "hotel", "eighth item"),
                Pair::described("india", "india", "ninth item"),
                Pair::described("juliet", "juliet", "tenth item"),
            ],
        ))
    }
}
impl Hinter for MenuCompleter {
    type Hint = String;

    fn hint(&self, line: &str, _pos: usize, _ctx: &Context<'_>) -> Option<Self::Hint> {
        (line == "a").then(|| "bs".to_string())
    }
}

impl Helper for MenuCompleter {}
impl Highlighter for MenuCompleter {}
impl Validator for MenuCompleter {}

#[test]
fn complete_line() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(SimpleCompleter);
    let mut s = init_state(&mut out, "rus", 3, helper.as_ref(), &history);
    let config = Config::default();
    let bindings = Bindings::new();
    let mut input_state = InputState::new(&config, &bindings);
    let keys = vec![E::ENTER];
    let mut rdr: IntoIter<KeyEvent> = keys.into_iter();
    let cmd = super::complete_line(&mut rdr, &mut s, &mut input_state, &config).unwrap();
    assert_eq!(
        Some(Cmd::AcceptOrInsertLine {
            accept_in_the_middle: true
        }),
        cmd
    );
    assert_eq!("rust", s.line.as_str());
    assert_eq!(4, s.line.pos());
}

#[test]
fn complete_symbol() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(SimpleCompleter);
    let mut s = init_state(&mut out, "\\hbar", 5, helper.as_ref(), &history);
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();
    let bindings = Bindings::new();
    let mut input_state = InputState::new(&config, &bindings);
    let keys = vec![E::ENTER];
    let mut rdr: IntoIter<KeyEvent> = keys.into_iter();
    let cmd = super::complete_line(&mut rdr, &mut s, &mut input_state, &config).unwrap();
    assert_eq!(None, cmd);
    assert_eq!("ℏ", s.line.as_str());
    assert_eq!(3, s.line.pos());
}

#[test]
fn list_completion_aligns_and_truncates_descriptions() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(DescribedCompleter);
    let mut s = init_state(&mut out, "a", 1, helper.as_ref(), &history);
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .completion_show_all_if_ambiguous(true)
        .build();
    let bindings = Bindings::new();
    let mut input_state = InputState::new(&config, &bindings);
    let keys = Vec::new();
    let mut rdr: IntoIter<KeyEvent> = keys.into_iter();

    let cmd = super::complete_line(&mut rdr, &mut s, &mut input_state, &config).unwrap();

    assert_eq!(None, cmd);
    assert!(out.output.contains("\nassignment-forms  Bind, update"));
    assert!(out.output.contains("\nassert            Assert that"));
    assert!(out.output.contains("..."));
    assert!(!out.output.contains("and more tail."));
}

#[test]
fn menu_completion_is_bounded_without_prompt_or_pager() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(MenuCompleter);
    let mut s = init_state(&mut out, "", 0, helper.as_ref(), &history);
    let config = Config::builder()
        .completion_type(CompletionType::Menu)
        .build();
    let bindings = Bindings::new();
    let mut input_state = InputState::new(&config, &bindings);
    let keys = vec![E::ESC];
    let mut rdr: IntoIter<KeyEvent> = keys.into_iter();

    let cmd = super::complete_line(&mut rdr, &mut s, &mut input_state, &config).expect("complete");

    assert_eq!(None, cmd);
    assert!(out.output.contains("\n> ● alpha    first item"));
    assert!(out.output.contains("\n  ● hotel    eighth item"));
    assert!(
        out.output
            .contains("\n  1-8 of 10  selected 1/10  builtin  alpha")
    );
    assert!(!out.output.contains("india"));
    assert!(!out.output.contains("Display all"));
    assert!(!out.output.contains("--More--"));
}

#[test]
fn menu_completion_accepts_selected_candidate() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(MenuCompleter);
    let mut s = init_state(&mut out, "", 0, helper.as_ref(), &history);
    let config = Config::builder()
        .completion_type(CompletionType::Menu)
        .build();
    let bindings = Bindings::new();
    let mut input_state = InputState::new(&config, &bindings);
    let keys = vec![E(K::Tab, M::NONE), E::ENTER];
    let mut rdr: IntoIter<KeyEvent> = keys.into_iter();

    let cmd = super::complete_line(&mut rdr, &mut s, &mut input_state, &config).expect("complete");

    assert_eq!(None, cmd);
    assert_eq!("beta", s.line.as_str());
    assert!(
        out.output
            .contains("\n  1-8 of 10  selected 2/10  global  beta")
    );
}

#[test]
fn menu_completion_reserves_right_edge_for_long_text() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(MenuCompleter);
    let s = init_state(&mut out, "", 0, helper.as_ref(), &history);
    let cols = 24;
    let candidates = vec![
        Pair::described(
            "alpha",
            "alpha",
            "long text excerpt that should be truncated before the terminal edge",
        )
        .with_kind("builtin"),
        Pair::described("beta", "beta", "second item").with_kind("global"),
    ];

    let menu = super::render_completion_menu(&candidates, &s.layout, cols, 0);

    assert!(menu.contains("..."));
    for line in menu.lines().filter(|line| !line.is_empty()) {
        assert!(s.layout.width(line) < cols, "{line}");
    }
}

#[test]
fn menu_completion_interrupt_clears_hint() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(MenuCompleter);
    let mut s = init_state(&mut out, "a", 1, helper.as_ref(), &history);
    let config = Config::builder()
        .completion_type(CompletionType::Menu)
        .build();
    let bindings = Bindings::new();
    let mut input_state = InputState::new(&config, &bindings);
    let keys = vec![E(K::Char('C'), M::CTRL)];
    let mut rdr: IntoIter<KeyEvent> = keys.into_iter();

    let err =
        super::complete_line(&mut rdr, &mut s, &mut input_state, &config).expect_err("interrupt");

    assert!(matches!(err, ReadlineError::Interrupted));
    assert_eq!("a", s.line.as_str());
    assert!(s.hint.is_none());
    assert!(!out.output.ends_with("bs"));
}

#[test]
fn menu_completion_interrupts_readline() {
    let config = Config::builder()
        .completion_type(CompletionType::Menu)
        .build();
    let mut editor =
        crate::Editor::<MenuCompleter, crate::history::DefaultHistory>::with_config(config)
            .expect("editor");
    editor.set_helper(Some(MenuCompleter));
    editor
        .term
        .keys
        .extend([E(K::Tab, M::NONE), E(K::Char('C'), M::CTRL)]);

    let err = editor
        .readline_with_initial(">>", ("a", ""))
        .expect_err("interrupt");

    assert!(matches!(err, ReadlineError::Interrupted));
}

#[test]
fn interrupt_clears_visible_hint() {
    let mut out = Sink::default();
    let history = crate::history::DefaultHistory::new();
    let helper = Some(MenuCompleter);
    let mut s = init_state(&mut out, "a", 1, helper.as_ref(), &history);
    let config = Config::builder().build();
    let bindings = Bindings::new();
    let input_state = InputState::new(&config, &bindings);
    let mut kill_ring = crate::kill_ring::KillRing::new(60);

    s.refresh_line().expect("paint hint");
    assert!(s.hint.is_some());

    let result = crate::command::execute(
        Cmd::Interrupt,
        &mut s,
        &input_state,
        &mut kill_ring,
        &config,
    );

    assert!(matches!(result, Err(ReadlineError::Interrupted)));
    assert!(s.hint.is_none());
    assert!(matches!(out.hints.last(), Some(None)));
}

// `keys`: keys to press
// `expected_line`: line after enter key
fn assert_line(mode: EditMode, keys: &[KeyEvent], expected_line: &str) {
    let mut editor = init_editor(mode, keys);
    let actual_line = editor.readline(">>").unwrap();
    assert_eq!(expected_line, actual_line);
}

// `initial`: line status before `keys` pressed: strings before and after cursor
// `keys`: keys to press
// `expected_line`: line after enter key
fn assert_line_with_initial(
    mode: EditMode,
    initial: (&str, &str),
    keys: &[KeyEvent],
    expected_line: &str,
) {
    let mut editor = init_editor(mode, keys);
    let actual_line = editor.readline_with_initial(">>", initial).unwrap();
    assert_eq!(expected_line, actual_line);
}

// `initial`: line status before `keys` pressed: strings before and after cursor
// `keys`: keys to press
// `expected`: line status before enter key: strings before and after cursor
fn assert_cursor(mode: EditMode, initial: (&str, &str), keys: &[KeyEvent], expected: (&str, &str)) {
    let mut editor = init_editor(mode, keys);
    let actual_line = editor.readline_with_initial("", initial).unwrap();
    assert_eq!(expected.0.to_owned() + expected.1, actual_line);
    assert_eq!(expected.0.len(), editor.term.cursor);
}

// `entries`: history entries before `keys` pressed
// `keys`: keys to press
// `expected`: line status before enter key: strings before and after cursor
fn assert_history(
    mode: EditMode,
    entries: &[&str],
    keys: &[KeyEvent],
    prompt: &str,
    expected: (&str, &str),
) {
    let mut editor = init_editor(mode, keys);
    for entry in entries {
        editor.history.add(entry).unwrap();
    }
    let actual_line = editor.readline(prompt).unwrap();
    assert_eq!(expected.0.to_owned() + expected.1, actual_line);
    if prompt.is_empty() {
        assert_eq!(expected.0.len(), editor.term.cursor);
    }
}

#[test]
fn unknown_esc_key() {
    for mode in &[EditMode::Emacs, EditMode::Vi] {
        assert_line(*mode, &[E(K::UnknownEscSeq, M::NONE), E::ENTER], "");
    }
}

#[test]
fn test_send() {
    fn assert_send<T: Send>() {}
    assert_send::<DefaultEditor>();
}

#[test]
fn test_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<DefaultEditor>();
}

#[test]
fn test_apply_backspace_direct() {
    assert_eq!(
        &apply_backspace_direct("Hel\u{0008}\u{0008}el\u{0008}llo ☹\u{0008}☺"),
        "Hello ☺"
    );
}

#[test]
fn test_readline_direct() {
    use std::io::Cursor;

    let mut write_buf = vec![];
    let output = readline_direct(
        Cursor::new("([)\n\u{0008}\n\n\r\n])".as_bytes()),
        Cursor::new(&mut write_buf),
        Some(crate::validate::MatchingBracketValidator::new()).as_ref(),
    );

    assert_eq!(
        &write_buf,
        b"Mismatched brackets: '[' is not properly closed"
    );
    assert_eq!(&output.unwrap(), "([\n\n\r\n])");
}
