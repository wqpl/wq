#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

#[cfg(not(target_arch = "wasm32"))]
pub type WqStdinHandle = Box<dyn WqStdin + Send>;
#[cfg(target_arch = "wasm32")]
pub type WqStdinHandle = Box<dyn WqStdin>;

#[cfg(not(target_arch = "wasm32"))]
pub type WqStdoutHandle = Box<dyn WqStdout + Send>;
#[cfg(target_arch = "wasm32")]
pub type WqStdoutHandle = Box<dyn WqStdout>;

#[cfg(not(target_arch = "wasm32"))]
pub type WqStderrHandle = Box<dyn WqStderr + Send>;
#[cfg(target_arch = "wasm32")]
pub type WqStderrHandle = Box<dyn WqStderr>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) static WQ_STDIN: Mutex<Option<WqStdinHandle>> = Mutex::new(None);

#[cfg(target_arch = "wasm32")]
thread_local! {
    pub(crate) static WQ_STDIN: RefCell<Option<WqStdinHandle>> = RefCell::new(None);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) static WQ_STDOUT: Mutex<Option<WqStdoutHandle>> = Mutex::new(None);

#[cfg(target_arch = "wasm32")]
thread_local! {
    pub(crate) static WQ_STDOUT: RefCell<Option<WqStdoutHandle>> = RefCell::new(None);
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) static WQ_STDERR: Mutex<Option<WqStderrHandle>> = Mutex::new(None);

#[cfg(target_arch = "wasm32")]
thread_local! {
    pub(crate) static WQ_STDERR: RefCell<Option<WqStderrHandle>> = RefCell::new(None);
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WqInputMode {
    #[default]
    Wq,
    Wqdb,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WqGlobalHint {
    pub name: String,
    pub type_name: String,
    pub excerpt: String,
}

pub trait WqStdin {
    fn readline(&mut self, prompt: &str) -> Result<String, WqStdinError>;
    fn add_history(&mut self, _line: &str) {}
    fn set_highlight(&mut self, _on: bool) {}
    fn highlight_enabled(&self) -> bool;
    fn set_input_mode(&mut self, _mode: WqInputMode) {}
    fn input_mode(&self) -> WqInputMode {
        WqInputMode::Wq
    }
    /// Update the list of builtin names and usages used for completion/hints.
    fn set_builtin_hints(&mut self, _names: Vec<String>, _usages: Vec<String>) {}
    /// Update the global variables used for completion and hints.
    fn set_global_hints(&mut self, _hints: Vec<WqGlobalHint>) {}
    /// Update the debugger function names used for completion.
    fn set_wqdb_function_hints(&mut self, _names: Vec<String>) {}
    /// Update the list of repl command names and descriptions used for
    /// completion/hints.
    fn set_repl_hints(&mut self, _names: Vec<String>, _descs: Vec<String>) {}
    /// Toggle whether builtin hints are shown.
    fn set_hints_enabled(&mut self, _on: bool) {}
    fn hints_enabled(&self) -> bool {
        true
    }
}

pub trait WqStdout {
    fn print(&mut self, s: &str);
    fn println(&mut self, s: &str);
}

pub trait WqStderr {
    fn eprint(&mut self, s: &str);
    fn eprintln(&mut self, s: &str);
}

#[derive(Debug)]
pub enum WqStdinError {
    Interrupted,
    Eof,
    Other(String),
}

impl std::fmt::Display for WqStdinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WqStdinError::Interrupted => write!(f, "Input interrupted"),
            WqStdinError::Eof => write!(f, "End of file"),
            WqStdinError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for WqStdinError {}

pub fn set_wqstdin(reader: WqStdinHandle) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        *WQ_STDIN.lock().unwrap() = Some(reader);
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        *cell.borrow_mut() = Some(reader);
    });
}

pub fn set_wqstdout(writer: Option<WqStdoutHandle>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        *WQ_STDOUT.lock().unwrap() = writer;
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDOUT.with(|cell| {
        *cell.borrow_mut() = writer;
    });
}

pub fn set_wqstderr(writer: Option<WqStderrHandle>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        *WQ_STDERR.lock().unwrap() = writer;
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDERR.with(|cell| {
        *cell.borrow_mut() = writer;
    });
}

pub fn wqstdout_print(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = WQ_STDOUT.lock().unwrap().as_mut() {
            w.print(s);
        } else {
            use std::io::{Write, stdout};
            print!("{s}");
            stdout().flush().ok();
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDOUT.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.print(s);
        }
    });
}

pub fn wqstdout_println(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = WQ_STDOUT.lock().unwrap().as_mut() {
            w.println(s);
        } else {
            println!("{s}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDOUT.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.println(s);
        }
    });
}

pub fn wqstderr_print(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = WQ_STDERR.lock().unwrap().as_mut() {
            w.eprint(s);
        } else {
            use std::io::{Write, stderr};
            print!("{s}");
            stderr().flush().ok();
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDERR.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.eprint(s);
        }
    });
}

pub fn wqstderr_println(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = WQ_STDERR.lock().unwrap().as_mut() {
            w.eprintln(s);
        } else {
            eprintln!("{s}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDERR.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.eprintln(s);
        }
    });
}

pub fn wqstdin_readline(prompt: &str) -> Result<String, WqStdinError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut guard = WQ_STDIN.lock().unwrap();
        if let Some(r) = guard.as_mut() {
            r.readline(prompt)
        } else {
            Err(WqStdinError::Other("Stdin not initialized".into()))
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        let mut guard = cell.borrow_mut();
        if let Some(r) = guard.as_mut() {
            r.readline(prompt)
        } else {
            Err(WqStdinError::Other("Stdin not initialized".into()))
        }
    })
}

pub fn wqstdin_add_history(line: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_mut() {
            r.add_history(line);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_mut() {
            r.add_history(line);
        }
    });
}

pub fn wqstdin_set_highlight(on: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_highlight(on); // no-op if impl doesn’t support it
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_highlight(on); // no-op if impl doesn’t support it
        }
    });
}

pub fn wqstdin_highlight_enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut r = WQ_STDIN.lock().unwrap();
        let g = r.as_mut().expect("STDIN not set");
        g.highlight_enabled()
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        let mut r = cell.borrow_mut();
        let g = r.as_mut().expect("STDIN not set");
        g.highlight_enabled()
    })
}

pub fn wqstdin_set_input_mode(mode: WqInputMode) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_input_mode(mode);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_input_mode(mode);
        }
    });
}

pub fn wqstdin_input_mode() -> WqInputMode {
    #[cfg(not(target_arch = "wasm32"))]
    {
        WQ_STDIN
            .lock()
            .unwrap()
            .as_deref()
            .map_or(WqInputMode::Wq, WqStdin::input_mode)
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        cell.borrow()
            .as_deref()
            .map_or(WqInputMode::Wq, WqStdin::input_mode)
    })
}

pub fn wqstdin_set_builtin_hints(names: Vec<String>, usages: Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_builtin_hints(names, usages);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_builtin_hints(names, usages);
        }
    });
}

pub fn wqstdin_set_global_hints(hints: Vec<WqGlobalHint>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_global_hints(hints);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_global_hints(hints);
        }
    });
}

pub fn wqstdin_set_wqdb_function_hints(names: Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_wqdb_function_hints(names);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_wqdb_function_hints(names);
        }
    });
}

pub fn wqstdin_set_repl_hints(names: Vec<String>, descs: Vec<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_repl_hints(names, descs);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_repl_hints(names, descs);
        }
    });
}

pub fn wqstdin_set_hints_enabled(on: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = WQ_STDIN.lock().unwrap().as_deref_mut() {
            r.set_hints_enabled(on);
        }
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_hints_enabled(on);
        }
    });
}

pub fn wqstdin_hints_enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut r = WQ_STDIN.lock().unwrap();
        let g = r.as_mut().expect("STDIN not set");
        g.hints_enabled()
    }
    #[cfg(target_arch = "wasm32")]
    WQ_STDIN.with(|cell| {
        let mut r = cell.borrow_mut();
        let g = r.as_mut().expect("STDIN not set");
        g.hints_enabled()
    })
}

struct HighlightRestore(bool);
impl Drop for HighlightRestore {
    fn drop(&mut self) {
        wqstdin_set_highlight(self.0);
    }
}

// Do something with highlight off, then restore on
pub fn wqstdin_with_highlight_off<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let prev = wqstdin_highlight_enabled();
    wqstdin_set_highlight(false);
    let _restore = HighlightRestore(prev);
    f()
}

struct InputModeRestore(WqInputMode);
impl Drop for InputModeRestore {
    fn drop(&mut self) {
        wqstdin_set_input_mode(self.0);
    }
}

pub fn wqstdin_with_input_mode<F, R>(mode: WqInputMode, f: F) -> R
where
    F: FnOnce() -> R,
{
    let previous = wqstdin_input_mode();
    wqstdin_set_input_mode(mode);
    let _restore = InputModeRestore(previous);
    f()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;

    struct LocalStdout {
        wrote: Rc<Cell<bool>>,
    }

    impl WqStdout for LocalStdout {
        fn print(&mut self, _s: &str) {
            self.wrote.set(true);
        }

        fn println(&mut self, s: &str) {
            self.print(s);
        }
    }

    #[test]
    fn stdio_traits_accept_local_implementors() {
        let wrote = Rc::new(Cell::new(false));
        let mut stdout = LocalStdout {
            wrote: Rc::clone(&wrote),
        };

        stdout.print("local");

        assert!(wrote.get());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn native_stdio_handles_are_send() {
        fn assert_send<T: Send>() {}

        assert_send::<WqStdinHandle>();
        assert_send::<WqStdoutHandle>();
        assert_send::<WqStderrHandle>();
    }
}
