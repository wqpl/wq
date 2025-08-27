#![cfg(target_arch = "wasm32")]

use crate::builtins_help;
use crate::helpers::string_helpers::create_boxed_text;
use crate::repl::{ReplEngine, ReplStdin, ReplStdout, StdinError, VmEvaluator, set_stdout};
use crate::value::box_mode;
use js_sys::{Array, Reflect};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn run_wasm(code: String, opts: JsValue) -> Result<JsValue, JsValue> {
    // choose stdin from opts.stdin
    let stdin_val = Reflect::get(&opts, &JsValue::from_str("stdin")).unwrap_or(JsValue::UNDEFINED);
    // optional stdout sink from opts.stdout
    let stdout_val =
        Reflect::get(&opts, &JsValue::from_str("stdout")).unwrap_or(JsValue::UNDEFINED);

    // Install stdout hook if provided
    if !stdout_val.is_undefined() && !stdout_val.is_null() {
        if js_sys::Function::instanceof(&stdout_val) {
            let f: js_sys::Function = stdout_val.unchecked_into();
            set_stdout(Some(Box::new(FunctionStdout { cb: f })));
        } else if Array::is_array(&stdout_val) {
            let arr: Array = stdout_val.unchecked_into();
            set_stdout(Some(Box::new(ArrayStdout { arr })));
        } else {
            set_stdout(None);
        }
    } else {
        set_stdout(None);
    }

    // case 1: stdin is an array (preloaded)
    if Array::is_array(&stdin_val) {
        let arr: Array = stdin_val.unchecked_into();
        let lines: Vec<String> = arr
            .iter()
            .map(|v| v.as_string().unwrap_or_default())
            .collect();

        let mut eval: Box<dyn ReplEngine> = Box::new(VmEvaluator::new());
        eval.set_stdin(Box::new(ArrayStdin::new(lines))); // sync path OK
        let result = eval
            .eval_string(&code)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        return Ok(JsValue::from_str(&result.to_string()));
    }

    // case 2: stdin is a function returning a Promise
    // if stdin_val.is_function() {
    //     use crate::repl_wasm::wasm_async_stdin::{AsyncReplInput, JsFnStdin};
    //     let cb: Function = stdin_val.unchecked_into();
    //     let mut eval = VmEvaluator::new();

    //     // You need an async REPL/VM evaluation that awaits stdin.
    //     // If your eval is currently sync, create an async wrapper that awaits input calls on wasm.
    //     set_async_stdin(Box::new(JsFnStdin::new(cb))); // your async setter
    //     let result = eval
    //         .eval_string_async(&code)
    //         .await
    //         .map_err(|e| JsValue::from_str(&e.to_string()))?;
    //     return Ok(JsValue::from_str(&result.to_string()));
    // }

    // Default: no stdin supplied -> treat as EOF-only
    let mut eval: Box<dyn ReplEngine> = Box::new(VmEvaluator::new());
    eval.set_stdin(Box::new(ArrayStdin::new(vec![])));
    let result = eval
        .eval_string(&code)
        .map_err(|e| JsValue::from_str(&e.to_string()))?;
    Ok(JsValue::from_str(&result.to_string()))
}

pub struct ArrayStdin {
    lines: Vec<String>, // pop from front
}

impl ArrayStdin {
    pub fn new(lines: Vec<String>) -> Self {
        Self { lines }
    }
}

impl ReplStdin for ArrayStdin {
    fn readline(&mut self, _prompt: &str) -> Result<String, StdinError> {
        if self.lines.is_empty() {
            Err(StdinError::Eof)
        } else {
            Ok(self.lines.remove(0))
        }
    }
}

// WASM stdout hooks
struct ArrayStdout {
    arr: Array,
}

impl ReplStdout for ArrayStdout {
    fn print(&mut self, s: &str) {
        let _ = self.arr.push(&JsValue::from_str(s));
    }
    fn println(&mut self, s: &str) {
        let _ = self.arr.push(&JsValue::from_str(&(s.to_string() + "\n")));
    }
}

struct FunctionStdout {
    cb: js_sys::Function,
}

impl ReplStdout for FunctionStdout {
    fn print(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn println(&mut self, s: &str) {
        let _ = self
            .cb
            .call1(&JsValue::NULL, &JsValue::from_str(&(s.to_string() + "\n")));
    }
}

// SAFETY: WebAssembly in the browser runs single-threaded (no preemptive threads),
// and these wrappers are only used behind a global during execution of `run_wasm`.
// Marking them Send satisfies the Mutex-bound in the host without actual cross-thread use.
unsafe impl Send for ArrayStdout {}
unsafe impl Send for FunctionStdout {}

// Reusable REPL session that preserves environment and accumulates stdin.
#[wasm_bindgen]
pub struct WqSession {
    eval: VmEvaluator,
    stdin_buf: Arc<Mutex<VecDeque<String>>>,
    stdout_pref: Option<StdoutPref>,
}

enum StdoutPref {
    Array(Array),
    Function(js_sys::Function),
}

#[wasm_bindgen]
impl WqSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WqSession {
        WqSession {
            eval: VmEvaluator::new(),
            stdin_buf: Arc::new(Mutex::new(VecDeque::new())),
            stdout_pref: None,
        }
    }

    // Append one or more lines to the shared stdin buffer.
    // Accepts a string or an array of strings.
    pub fn push_stdin(&mut self, lines: JsValue) {
        let mut guard = self.stdin_buf.lock().unwrap();
        if let Some(s) = lines.as_string() {
            guard.push_back(s);
            return;
        }
        if Array::is_array(&lines) {
            let arr: Array = lines.unchecked_into();
            for v in arr.iter() {
                if let Some(s) = v.as_string() {
                    guard.push_back(s);
                }
            }
        }
    }

    // Configure persistent stdout sink for this session.
    pub fn set_stdout(&mut self, stdout: JsValue) {
        if js_sys::Function::instanceof(&stdout) {
            self.stdout_pref = Some(StdoutPref::Function(stdout.unchecked_into()));
        } else if Array::is_array(&stdout) {
            self.stdout_pref = Some(StdoutPref::Array(stdout.unchecked_into()));
        } else {
            self.stdout_pref = None;
        }
    }

    // Evaluate code using the preserved environment.
    // opts may contain { stdin, stdout }.
    pub async fn eval(&mut self, code: String, opts: JsValue) -> Result<JsValue, JsValue> {
        // Merge stdin: if opts.stdin present, append to buffer.
        let stdin_val =
            Reflect::get(&opts, &JsValue::from_str("stdin")).unwrap_or(JsValue::UNDEFINED);
        if Array::is_array(&stdin_val) {
            self.push_stdin(stdin_val.clone());
        } else if let Some(s) = stdin_val.as_string() {
            self.push_stdin(JsValue::from_str(&s));
        }

        // Install session-backed stdin
        self.eval.set_stdin(Box::new(SessionStdin {
            buf: self.stdin_buf.clone(),
        }));

        // Choose stdout: opts.stdout overrides session preference for this call.
        let stdout_val =
            Reflect::get(&opts, &JsValue::from_str("stdout")).unwrap_or(JsValue::UNDEFINED);
        if !stdout_val.is_undefined() && !stdout_val.is_null() {
            if js_sys::Function::instanceof(&stdout_val) {
                let f: js_sys::Function = stdout_val.unchecked_into();
                set_stdout(Some(Box::new(FunctionStdout { cb: f })));
            } else if Array::is_array(&stdout_val) {
                let arr: Array = stdout_val.unchecked_into();
                set_stdout(Some(Box::new(ArrayStdout { arr })));
            } else {
                set_stdout(None);
            }
        } else if let Some(pref) = &self.stdout_pref {
            match pref {
                StdoutPref::Array(arr) => {
                    set_stdout(Some(Box::new(ArrayStdout { arr: arr.clone() })))
                }
                StdoutPref::Function(f) => {
                    set_stdout(Some(Box::new(FunctionStdout { cb: f.clone() })))
                }
            }
        } else {
            set_stdout(None);
        }

        let result = self
            .eval
            .eval_string(&code)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        // Clear stdout hook after call to avoid lingering globals.
        set_stdout(None);

        Ok(JsValue::from_str(&result.to_string()))
    }

    pub fn set_debug(&mut self) -> bool {
        if self.eval.is_debug() {
            self.eval.set_debug(false);
            false
        } else {
            self.eval.set_debug(true);
            true
        }
    }

    pub fn get_env(&self) -> String {
        match self.eval.get_environment() {
            Some(env) => {
                let mut result = String::from("user-defined bindings:\n");
                for (key, value) in env {
                    result.push_str(&format!("{key}: {value}\n"));
                }
                result
            }
            None => "no user-defined bindings".to_string(),
        }
    }

    pub fn clear_env(&mut self) {
        self.eval.clear_environment();
    }

    pub fn set_box_mode(&mut self) -> bool {
        if box_mode::is_boxed() {
            box_mode::set_boxed(false);
            false
        } else {
            box_mode::set_boxed(true);
            true
        }
    }
}

impl Default for WqSession {
    fn default() -> Self {
        Self::new()
    }
}

// ReplInput backed by a session-shared buffer.
struct SessionStdin {
    buf: Arc<Mutex<VecDeque<String>>>,
}

impl ReplStdin for SessionStdin {
    fn readline(&mut self, _prompt: &str) -> Result<String, StdinError> {
        let mut guard = self
            .buf
            .lock()
            .map_err(|e| StdinError::Other(e.to_string()))?;
        match guard.pop_front() {
            Some(s) => Ok(s),
            None => Err(StdinError::Eof),
        }
    }
}

#[wasm_bindgen]
pub fn get_help_doc(name: &str) -> String {
    if name.is_empty() {
        create_boxed_text(
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/doc/refcard.txt")),
            2,
        )
    } else if let Some(text) = builtins_help::get_builtin_help(name) {
        create_boxed_text(text, 2)
    } else {
        format!("no help available for '{name}'")
    }
}

#[wasm_bindgen]
pub fn get_wq_ver() -> String {
    env!("CARGO_PKG_VERSION").into()
}
