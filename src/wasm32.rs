#![cfg(target_arch = "wasm32")]

use std::{
    cell::{Cell, RefCell},
    fmt::Write as _,
};

use crate::{
    builtins::Builtins,
    // create_boxed_text,
    repl::box_mode::format_boxed,
    repl::{
        VmEvaluator,
        stdio::{ReplStderr, ReplStdin, ReplStdout, StdinError, set_stderr, set_stdin, set_stdout},
    },
    wqerr::WqErrType,
};

use js_sys::Function;
use wasm_bindgen::prelude::*;

// ===================== JS stream adapters =====================

struct JsStdout {
    cb: Function,
}

struct JsStderr {
    cb: Function,
}

struct JsStdin {
    cb: Function,
    highlight: bool,
}

// Safe on wasm (single-threaded); these callbacks are only invoked on the main thread.
unsafe impl Send for JsStdout {}
unsafe impl Send for JsStderr {}
unsafe impl Send for JsStdin {}

impl ReplStdout for JsStdout {
    fn print(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn println(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
}

impl ReplStderr for JsStderr {
    fn eprint(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn eprintln(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
}

impl ReplStdin for JsStdin {
    fn readline(&mut self, prompt: &str) -> Result<String, StdinError> {
        match self.cb.call1(&JsValue::NULL, &JsValue::from_str(prompt)) {
            Ok(val) => {
                if val.is_undefined() || val.is_null() {
                    Err(StdinError::Eof)
                } else {
                    Ok(val.as_string().unwrap_or_default())
                }
            }
            Err(e) => Err(StdinError::Other(format!("{e:?}"))),
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

// ===================== Global std stream setters =====================

#[wasm_bindgen]
pub fn set_stdout_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_stdout(Some(Box::new(JsStdout { cb: f }))),
        None => set_stdout(None),
    }
}

#[wasm_bindgen]
pub fn set_stderr_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_stderr(Some(Box::new(JsStderr { cb: f }))),
        None => set_stderr(None),
    }
}

#[wasm_bindgen]
pub fn set_stdin_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_stdin(Box::new(JsStdin {
            cb: f,
            highlight: true,
        })),
        None => {
            // Reset to no custom stdin; subsequent reads will error
            // There's no setter to clear stdin, so set a reader that always EOFs.
            set_stdin(Box::new(JsStdin {
                cb: Function::new_no_args("return null;"),
                highlight: true,
            }));
        }
    }
}

// ===================== Session API =====================

thread_local! {
    static BOX_MODE: Cell<bool> = const { Cell::new(false) };
}

#[wasm_bindgen]
pub struct WqSession {
    vm: RefCell<VmEvaluator>,
}

#[wasm_bindgen]
impl WqSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WqSession {
        WqSession {
            vm: RefCell::new(VmEvaluator::new()),
        }
    }

    /// Evaluate a source string and return the value's string form.
    #[wasm_bindgen]
    pub fn eval_wq(&self, src: &str) -> Result<String, JsValue> {
        let mut vm = self.vm.borrow_mut();
        vm.set_bt_mode(true);
        vm.dbg_set_source("<wasm>", src);
        vm.dbg_set_offset(0);
        match vm.eval_string(src) {
            Ok(v) => {
                let s = BOX_MODE.with(|b| {
                    if b.get() {
                        format_boxed(&v)
                    } else {
                        format!("{v}")
                    }
                });
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

    pub fn set_debug_level(&self, level: u8) {
        self.vm.borrow_mut().set_debug_level(level);
    }

    pub fn get_debug_level(&self) -> u8 {
        self.vm.borrow().get_debug_level()
    }

    pub fn get_bt_mode(&self) {
        self.vm.borrow().get_bt_mode();
    }

    pub fn set_bt_mode(&self, on: bool) {
        self.vm.borrow_mut().set_wqdb(on);
    }

    pub fn reset_session(&self) {
        self.vm.borrow_mut().reset_session();
    }

    /// Return a formatted view of user-defined global bindings.
    pub fn get_env(&self) -> String {
        use std::fmt::Write as _;
        let vm = self.vm.borrow();
        if let Some(env) = vm.get_environment() {
            let mut name_w = "name".len();
            let mut value_w = "value".len();
            let mut type_w = "type".len();
            for (name, v) in env {
                name_w = name_w.max(name.len());
                value_w = value_w.max(v.to_string().len());
                type_w = type_w.max(v.type_name().len());
            }
            let mut out = String::new();
            let _ = writeln!(
                out,
                "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                "name",
                "value",
                "type",
                name_w = name_w,
                value_w = value_w,
                type_w = type_w
            );
            let _ = writeln!(
                out,
                "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
                "",
                "",
                "",
                name_w = name_w,
                value_w = value_w,
                type_w = type_w
            );
            for (name, v) in env {
                let _ = writeln!(
                    out,
                    "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                    name,
                    v.to_string(),
                    v.type_name(),
                    name_w = name_w,
                    value_w = value_w,
                    type_w = type_w
                );
            }
            if out.ends_with('\n') {
                out.pop();
            }
            out
        } else {
            "no global bindings".to_string()
        }
    }

    /// Clear user-defined bindings while preserving debug state.
    pub fn clear_env(&self) {
        self.vm.borrow_mut().environment_mut().clear();
    }
}

impl Default for WqSession {
    fn default() -> Self {
        Self::new()
    }
}

// ===================== Convenience one-offs =====================

/// Evaluate a string in a fresh VM and return the result as a string.
#[wasm_bindgen]
pub fn eval_wq(code: &str) -> Result<String, JsValue> {
    let session = WqSession::new();
    session.eval_wq(code)
}

/// Toggle boxed display of evaluation results. Returns the new state.
#[wasm_bindgen]
pub fn set_box_mode() -> bool {
    BOX_MODE.with(|b| {
        let new = !b.get();
        b.set(new);
        new
    })
}

/// Version string for splash and title
#[wasm_bindgen]
pub fn get_wq_ver() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Help/refcard as a boxed string (web expects a string to append)
#[wasm_bindgen]
pub fn get_help_doc() -> String {
    // create_boxed_text(include_str!("../d/refcard"), 2)
    include_str!("../d/refcard").to_string()
}

/// Builtin function names (columns)
#[wasm_bindgen]
pub fn get_builtins() -> String {
    let mut funcs = Builtins::new().list_functions();
    funcs.sort();
    col_wrap(&funcs, 6, 2)
}

/// Error codes and names for quick reference
#[wasm_bindgen]
pub fn get_err_codes() -> String {
    let mut out = String::new();
    let all: &[(u16, &str)] = &[
        (WqErrType::Vm.to_code(), WqErrType::Vm.name()),
        (WqErrType::Eof.to_code(), WqErrType::Eof.name()),
        (WqErrType::Syntax.to_code(), WqErrType::Syntax.name()),
        (WqErrType::NotBound.to_code(), WqErrType::NotBound.name()),
        (WqErrType::Index.to_code(), WqErrType::Index.name()),
        (WqErrType::Call.to_code(), WqErrType::Call.name()),
        (WqErrType::Arity.to_code(), WqErrType::Arity.name()),
        (WqErrType::Domain.to_code(), WqErrType::Domain.name()),
        (WqErrType::Length.to_code(), WqErrType::Length.name()),
        (
            WqErrType::NumericOverflow.to_code(),
            WqErrType::NumericOverflow.name(),
        ),
        (WqErrType::ZeroDiv.to_code(), WqErrType::ZeroDiv.name()),
        (WqErrType::Io.to_code(), WqErrType::Io.name()),
        (WqErrType::Encode.to_code(), WqErrType::Encode.name()),
        (WqErrType::Exec.to_code(), WqErrType::Exec.name()),
        (WqErrType::Raise.to_code(), WqErrType::Raise.name()),
    ];
    // width calc
    let w_code = all
        .iter()
        .map(|(c, _)| c.to_string().len())
        .max()
        .unwrap_or(1);
    let w_name = all.iter().map(|(_, n)| n.len()).max().unwrap_or(1);
    out.push_str(&format!("{:<w_code$}  {:<w_name$}\n", "code", "name"));
    out.push_str(&format!("{:-<w_code$}  {:-<w_name$}\n", "", ""));
    for (code, name) in all {
        out.push_str(&format!("{code:<w_code$}  {name:<w_name$}\n"));
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

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

// (no explicit env formatter type-bound to internal GlobalMap to avoid visibility issues)
