use std::cell::{Cell, RefCell};
use std::fmt::Write as _;

#[cfg(target_arch = "wasm32")]
use js_sys::Function;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::console;
use wqpl::boxmode::format_boxed;
use wqpl::builtins::Builtins;
use wqpl::highlight::{HighlightEvent, HighlightName, Highlighter};
use wqpl::session::Session;
use wqpl::session::dbglog::DebugLogFlags;
#[cfg(target_arch = "wasm32")]
use wqpl::session::stdio::{
    WqStderr, WqStdin, WqStdinError, WqStdout, set_wqstderr, set_wqstdin, set_wqstdout,
};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn main_js() {
    // std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console::log_1(&"Hello from Rust!".into());
    colored::control::set_override(true);
}

// JS stream adapters
// ====================================================================================

#[cfg(target_arch = "wasm32")]
struct JsStdout {
    cb: Function,
}

#[cfg(target_arch = "wasm32")]
struct JsStderr {
    cb: Function,
}

#[cfg(target_arch = "wasm32")]
struct JsStdin {
    cb: Function,
    highlight: bool,
}

// Default console loggers used when no JS callback is provided
#[cfg(target_arch = "wasm32")]
struct ConsoleStdout;
#[cfg(target_arch = "wasm32")]
struct ConsoleStderr;

#[cfg(target_arch = "wasm32")]
impl WqStdout for JsStdout {
    fn print(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn println(&mut self, s: &str) {
        let mut out = s.to_owned();
        out.push('\n');
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(&out));
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStderr for JsStderr {
    fn eprint(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn eprintln(&mut self, s: &str) {
        let mut out = s.to_owned();
        out.push('\n');
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(&out));
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStdout for ConsoleStdout {
    fn print(&mut self, s: &str) {
        console::log_1(&s.into());
    }
    fn println(&mut self, s: &str) {
        console::log_1(&format!("{s}\n").into());
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStderr for ConsoleStderr {
    fn eprint(&mut self, s: &str) {
        console::error_1(&s.into());
    }
    fn eprintln(&mut self, s: &str) {
        console::error_1(&format!("{s}\n").into());
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStdin for JsStdin {
    fn readline(&mut self, prompt: &str) -> Result<String, WqStdinError> {
        match self.cb.call1(&JsValue::NULL, &JsValue::from_str(prompt)) {
            Ok(val) => {
                if val.is_undefined() || val.is_null() {
                    Err(WqStdinError::Eof)
                } else {
                    Ok(val.as_string().unwrap_or_default())
                }
            }
            Err(e) => Err(WqStdinError::Other(format!("{e:?}"))),
        }
    }

    fn add_history(&mut self, _line: &str) {}

    fn set_highlight(&mut self, on: bool) {
        self.highlight = on;
    }

    fn highlight_enabled(&self) -> bool {
        self.highlight
    }
}

// Global std stream setters
// ===============================================================

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_stdout_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_wqstdout(Some(Box::new(JsStdout { cb: f }))),
        None => set_wqstdout(Some(Box::new(ConsoleStdout))),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_stderr_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_wqstderr(Some(Box::new(JsStderr { cb: f }))),
        None => set_wqstderr(Some(Box::new(ConsoleStderr))),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_stdin_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_wqstdin(Box::new(JsStdin {
            cb: f,
            highlight: true,
        })),
        None => {
            // Reset to no custom stdin; subsequent reads will error
            // There's no setter to clear stdin, so set a reader that always EOFs.
            set_wqstdin(Box::new(JsStdin {
                cb: Function::new_no_args("return null;"),
                highlight: true,
            }));
        }
    }
}

// wq Session API
// ====================================================================================

#[wasm_bindgen]
pub struct EvalResult {
    value: String,
    is_cas: bool,
}

#[wasm_bindgen]
impl EvalResult {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> String {
        self.value.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn is_cas(&self) -> bool {
        self.is_cas
    }
}

#[wasm_bindgen]
pub struct WasmWqSession {
    box_mode: Cell<bool>,
    session: RefCell<Session>,
}

#[wasm_bindgen]
impl WasmWqSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmWqSession {
        WasmWqSession {
            box_mode: Cell::new(true),
            session: RefCell::new(Session::new()),
        }
    }

    /// Evaluate a source string and return the value's string form.
    #[wasm_bindgen]
    pub fn eval_wq(&self, src: &str) -> Result<String, JsValue> {
        let mut vm = self.session.borrow_mut();
        vm.set_bt_mode(true);
        vm.dbg_set_source("<wasm>", src);
        vm.dbg_set_offset(0);
        match vm.eval_string(src) {
            Ok(v) => {
                let s = if self.box_mode.get() {
                    format_boxed(&v)
                } else {
                    format!("{v}")
                };
                Ok(s)
            }
            Err(e) => {
                if e.err_type.is_runtime() {
                    vm.dbg_print_bt();
                }
                Err(JsValue::from_str(&format!("{e}")))
            }
        }
    }

    /// Evaluate a source string and return both the string form and whether it
    /// is a CAS expression.
    #[wasm_bindgen]
    pub fn eval_wq_result(&self, src: &str) -> Result<EvalResult, JsValue> {
        let mut vm = self.session.borrow_mut();
        vm.set_bt_mode(true);
        vm.dbg_set_source("<wasm>", src);
        vm.dbg_set_offset(0);
        match vm.eval_string(src) {
            Ok(v) => {
                let is_cas = v.is_cas();
                let s = if is_cas {
                    format!("{v}")
                } else if self.box_mode.get() {
                    format_boxed(&v)
                } else {
                    format!("{v}")
                };
                Ok(EvalResult { value: s, is_cas })
            }
            Err(e) => {
                if e.err_type.is_runtime() {
                    vm.dbg_print_bt();
                }
                Err(JsValue::from_str(&format!("{e}")))
            }
        }
    }

    pub fn set_debug_flags(&self, spec: &str) -> Result<(), JsValue> {
        match DebugLogFlags::parse(spec) {
            Ok(flags) => {
                wqpl::session::dbglog::set_debug_log_flags(flags);
                Ok(())
            }
            Err(e) => Err(JsValue::from_str(&e)),
        }
    }

    pub fn get_debug_flags(&self) -> String {
        let flags = wqpl::session::dbglog::get_debug_log_flags();
        let names = flags.display_names();
        if names.is_empty() {
            "off".to_string()
        } else {
            names.join(",")
        }
    }

    pub fn get_bt_mode(&self) {
        self.session.borrow().get_bt_mode();
    }

    pub fn set_bt_mode(&self, on: bool) {
        self.session.borrow_mut().set_wqdb(on);
    }

    pub fn reset_session(&self) {
        self.session.borrow_mut().reset_session();
    }

    // ///Return a formatted view of user-defined global bindings.
    // pub fn get_env(&self) -> String {
    //     use std::fmt::Write as _;
    //     let vm = self.vm.borrow();
    //     if let Some(env) = vm.get_environment() {
    //         let mut name_w = "name".len();
    //         let mut value_w = "value".len();
    //         let mut type_w = "type".len();
    //         for (name, v) in env {
    //             name_w = name_w.max(name.len());
    //             value_w = value_w.max(v.to_string().len());
    //             type_w = type_w.max(v.type_name().len());
    //         }
    //         let mut out = String::new();
    //         let _ = writeln!(
    //             out,
    //             "{:<name_w$}  {:<value_w$}  {:<type_w$}",
    //             "name",
    //             "value",
    //             "type",
    //             name_w = name_w,
    //             value_w = value_w,
    //             type_w = type_w
    //         );
    //         let _ = writeln!(
    //             out,
    //             "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
    //             "",
    //             "",
    //             "",
    //             name_w = name_w,
    //             value_w = value_w,
    //             type_w = type_w
    //         );
    //         for (name, v) in env {
    //             let _ = writeln!(
    //                 out,
    //                 "{:<name_w$}  {:<value_w$}  {:<type_w$}",
    //                 name,
    //                 v.to_string(),
    //                 v.type_name(),
    //                 name_w = name_w,
    //                 value_w = value_w,
    //                 type_w = type_w
    //             );
    //         }
    //         if out.ends_with('\n') {
    //             out.pop();
    //         }
    //         out
    //     } else {
    //         "no global bindings".to_string()
    //     }
    // }

    /// Clear user-defined bindings while preserving debug state.
    pub fn clear_env(&self) {
        self.session.borrow_mut().clear_environment();
    }

    /// Toggle boxed display of evaluation results. Returns the new state.
    pub fn toggle_box_mode(&self) -> bool {
        let new = !self.box_mode.get();
        self.box_mode.set(new);
        new
    }

    pub fn get_box_mode(&self) -> bool {
        self.box_mode.get()
    }
}

impl Default for WasmWqSession {
    fn default() -> Self {
        Self::new()
    }
}

// ===================== Convenience one-offs =====================

/// Evaluate a string in a fresh VM and return the result as a string.
#[wasm_bindgen]
pub fn eval_wq(code: &str) -> Result<String, JsValue> {
    let session = WasmWqSession::new();
    session.eval_wq(code)
}

/// Version string for splash and title
#[wasm_bindgen]
pub fn get_wq_ver() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// #[wasm_bindgen]
// pub fn get_help_doc() -> String {
//     include_str!("../d/refcard").to_string()
// }

/// Builtin function names (columns)
#[wasm_bindgen]
pub fn get_builtins() -> String {
    let mut funcs = Builtins::new().list_functions();
    funcs.sort();
    col_wrap(&funcs, 6, 2)
}

// /// Error codes and names for quick reference
// #[wasm_bindgen]
// pub fn get_err_codes() -> String {
//     let mut out = String::new();
//     let all: &[(u16, &str)] = &[
//         (WqErrorType::Vm.to_code(), WqErrorType::Vm.name()),
//         (WqErrorType::Eof.to_code(), WqErrorType::Eof.name()),
//         (WqErrorType::Syntax.to_code(), WqErrorType::Syntax.name()),
//         (
//             WqErrorType::NotBound.to_code(),
//             WqErrorType::NotBound.name(),
//         ),
//         (WqErrorType::Index.to_code(), WqErrorType::Index.name()),
//         (WqErrorType::Call.to_code(), WqErrorType::Call.name()),
//         (WqErrorType::Arity.to_code(), WqErrorType::Arity.name()),
//         (WqErrorType::Domain.to_code(), WqErrorType::Domain.name()),
//         (WqErrorType::Length.to_code(), WqErrorType::Length.name()),
//         (
//             WqErrorType::NumericOverflow.to_code(),
//             WqErrorType::NumericOverflow.name(),
//         ),
//         (WqErrorType::ZeroDiv.to_code(), WqErrorType::ZeroDiv.name()),
//         (WqErrorType::Io.to_code(), WqErrorType::Io.name()),
//         (WqErrorType::Encode.to_code(), WqErrorType::Encode.name()),
//         (WqErrorType::Exec.to_code(), WqErrorType::Exec.name()),
//         (WqErrorType::Raise.to_code(), WqErrorType::Raise.name()),
//     ];
//     // width calc
//     let w_code = all
//         .iter()
//         .map(|(c, _)| c.to_string().len())
//         .max()
//         .unwrap_or(1);
//     let w_name = all.iter().map(|(_, n)| n.len()).max().unwrap_or(1);
//     out.push_str(&format!("{:<w_code$}  {:<w_name$}\n", "code", "name"));
//     out.push_str(&format!("{:-<w_code$}  {:-<w_name$}\n", "", ""));
//     for (code, name) in all {
//         out.push_str(&format!("{code:<w_code$}  {name:<w_name$}\n"));
//     }
//     if out.ends_with('\n') {
//         out.pop();
//     }
//     out
// }

fn col_wrap(items: &[String], columns: usize, gutter: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let max_len = items.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (i, name) in items.iter().enumerate() {
        let _ = write!(&mut out, "{:<w$}", name, w = max_len + gutter);
        if (i + 1) % columns == 0 {
            out.push('\n');
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

#[wasm_bindgen]
pub fn set_ansi_styles_enabled(on: bool) {
    colored::control::set_override(on);
}

/// Highlight wq source code and return HTML with CSS class names.
#[wasm_bindgen]
pub fn highlight_wq(src: &str) -> String {
    let highlighter = Highlighter::new();
    let semantic_spans = if src.contains('{') || src.contains('\'') {
        Session::new()
            .analyze_symbols(src)
            .map(|index| index.semantic_highlight_spans())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let events = highlighter.highlight_with_semantic_spans(src, &semantic_spans);
    let mut out = String::with_capacity(src.len() * 2);
    let bytes = src.as_bytes();
    let mut stack: Vec<HighlightName> = Vec::new();

    for ev in events {
        match ev {
            HighlightEvent::HighlightStart(h) => stack.push(h),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let s = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                if let Some(&name) = stack.last() {
                    out.push_str("<span class=\"");
                    out.push_str(class_for_name(name));
                    out.push_str("\">");
                    out.push_str(&escape_html(s));
                    out.push_str("</span>");
                } else {
                    out.push_str(&escape_html(s));
                }
            }
        }
    }
    out
}

fn class_for_name(name: HighlightName) -> &'static str {
    match name {
        HighlightName::Comment => "hl-comment",
        HighlightName::Constant => "hl-constant",
        HighlightName::ConstantBuiltin => "hl-constant-builtin",
        HighlightName::Function => "hl-function",
        HighlightName::FunctionCall => "hl-function-call",
        HighlightName::FunctionBuiltin => "hl-function-builtin",
        HighlightName::Keyword => "hl-keyword",
        HighlightName::KeywordReturn => "hl-keyword-return",
        HighlightName::KeywordDebug => "hl-keyword-debug",
        HighlightName::Module => "hl-module",
        HighlightName::Number => "hl-number",
        HighlightName::Boolean => "hl-boolean",
        HighlightName::Operator => "hl-operator",
        HighlightName::OperatorPipe => "hl-operator-pipe",
        HighlightName::Property => "hl-property",
        HighlightName::PropertyBuiltin => "hl-property-builtin",
        HighlightName::Punctuation => "hl-punctuation",
        HighlightName::PunctuationBracket => "hl-punctuation-bracket",
        HighlightName::PunctuationBracket1 => "hl-punctuation-bracket-1",
        HighlightName::PunctuationBracket2 => "hl-punctuation-bracket-2",
        HighlightName::PunctuationBracket3 => "hl-punctuation-bracket-3",
        HighlightName::PunctuationBracket4 => "hl-punctuation-bracket-4",
        HighlightName::PunctuationBracket5 => "hl-punctuation-bracket-5",
        HighlightName::PunctuationBracket6 => "hl-punctuation-bracket-6",
        HighlightName::PunctuationDelimiter => "hl-punctuation-delimiter",
        HighlightName::PunctuationSpecial => "hl-punctuation-special",
        HighlightName::String => "hl-string",
        HighlightName::StringSpecial => "hl-string-special",
        HighlightName::Tag => "hl-tag",
        HighlightName::Type => "hl-type",
        HighlightName::TypeBuiltin => "hl-type-builtin",
        HighlightName::Variable => "hl-variable",
        HighlightName::VariableOuter => "hl-variable-outer",
        HighlightName::VariableRefCapture => "hl-variable-ref-capture",
        HighlightName::VariableBuiltin => "hl-variable-builtin",
        HighlightName::VariableParameter => "hl-variable-parameter",
        HighlightName::Meta => "hl-meta",
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}
