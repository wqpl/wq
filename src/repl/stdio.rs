#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Mutex;

// Use sync Lazy for native; wasm uses thread-local cells instead
#[cfg(not(target_arch = "wasm32"))]
use once_cell::sync::Lazy;

#[derive(Debug)]
pub enum StdinError {
    Interrupted,
    Eof,
    Other(String),
}

impl std::fmt::Display for StdinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StdinError::Interrupted => write!(f, "Input interrupted"),
            StdinError::Eof => write!(f, "End of file"),
            StdinError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for StdinError {}

pub trait ReplStdin: Send {
    fn readline(&mut self, prompt: &str) -> Result<String, StdinError>;
    fn add_history(&mut self, _line: &str) {}
    fn set_highlight(&mut self, _on: bool) {}
    fn highlight_enabled(&self) -> bool;
}

#[cfg(not(target_arch = "wasm32"))]
pub static STDIN: Lazy<Mutex<Option<Box<dyn ReplStdin>>>> = Lazy::new(|| Mutex::new(None));
#[cfg(target_arch = "wasm32")]
thread_local! {
    pub static STDIN: RefCell<Option<Box<dyn ReplStdin>>> = RefCell::new(None);
}

pub fn set_stdin(reader: Box<dyn ReplStdin>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        *STDIN.lock().unwrap() = Some(reader);
    }
    #[cfg(target_arch = "wasm32")]
    STDIN.with(|cell| {
        *cell.borrow_mut() = Some(reader);
    });
}

pub fn stdin_readline(prompt: &str) -> Result<String, StdinError> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut guard = STDIN.lock().unwrap();
        if let Some(r) = guard.as_mut() {
            r.readline(prompt)
        } else {
            Err(StdinError::Other("Stdin not initialized".into()))
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDIN.with(|cell| {
        let mut guard = cell.borrow_mut();
        if let Some(r) = guard.as_mut() {
            r.readline(prompt)
        } else {
            Err(StdinError::Other("Stdin not initialized".into()))
        }
    })
}

pub fn stdin_add_history(line: &str) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = STDIN.lock().unwrap().as_mut() {
            r.add_history(line);
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_mut() {
            r.add_history(line);
        }
    });
}

pub fn stdin_set_highlight(on: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(r) = STDIN.lock().unwrap().as_deref_mut() {
            r.set_highlight(on); // no-op if impl doesn’t support it
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDIN.with(|cell| {
        if let Some(r) = cell.borrow_mut().as_deref_mut() {
            r.set_highlight(on); // no-op if impl doesn’t support it
        }
    });
}

pub fn stdin_highlight_enabled() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut r = STDIN.lock().unwrap();
        let g = r.as_mut().expect("STDIN not set");
        g.highlight_enabled()
    }
    #[cfg(target_arch = "wasm32")]
    STDIN.with(|cell| {
        let mut r = cell.borrow_mut();
        let g = r.as_mut().expect("STDIN not set");
        g.highlight_enabled()
    })
}

struct HighlightRestore(bool);
impl Drop for HighlightRestore {
    fn drop(&mut self) {
        stdin_set_highlight(self.0);
    }
}

// do something with highlight OFF, then restore ON
pub fn stdin_with_highlight_off<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let prev = stdin_highlight_enabled();
    stdin_set_highlight(false);
    let _restore = HighlightRestore(prev);
    f()
}

pub trait ReplStdout: Send {
    fn print(&mut self, s: &str);
    fn println(&mut self, s: &str);
}

#[cfg(not(target_arch = "wasm32"))]
pub static STDOUT: Lazy<Mutex<Option<Box<dyn ReplStdout>>>> = Lazy::new(|| Mutex::new(None));
#[cfg(target_arch = "wasm32")]
thread_local! {
    pub static STDOUT: RefCell<Option<Box<dyn ReplStdout>>> = RefCell::new(None);
}

pub fn set_stdout(writer: Option<Box<dyn ReplStdout>>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        *STDOUT.lock().unwrap() = writer;
    }
    #[cfg(target_arch = "wasm32")]
    STDOUT.with(|cell| {
        *cell.borrow_mut() = writer;
    });
}

pub fn stdout_print(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = STDOUT.lock().unwrap().as_mut() {
            w.print(s);
        } else {
            use std::io::{Write, stdout};
            print!("{s}");
            stdout().flush().ok();
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDOUT.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.print(s);
        } else {
            // In wasm, forward stdout to the browser/JS console
            web_sys::console::log_1(&s.into());
        }
    });
}

pub fn stdout_println(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = STDOUT.lock().unwrap().as_mut() {
            w.println(s);
        } else {
            println!("{s}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDOUT.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.println(s);
        } else {
            // In wasm, forward stdout to the browser/JS console
            web_sys::console::log_1(&s.into());
        }
    });
}

pub trait ReplStderr: Send {
    fn eprint(&mut self, s: &str);
    fn eprintln(&mut self, s: &str);
}

#[cfg(not(target_arch = "wasm32"))]
pub static STDERR: Lazy<Mutex<Option<Box<dyn ReplStderr>>>> = Lazy::new(|| Mutex::new(None));
#[cfg(target_arch = "wasm32")]
thread_local! {
    pub static STDERR: RefCell<Option<Box<dyn ReplStderr>>> = RefCell::new(None);
}

pub fn set_stderr(writer: Option<Box<dyn ReplStderr>>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        *STDERR.lock().unwrap() = writer;
    }
    #[cfg(target_arch = "wasm32")]
    STDERR.with(|cell| {
        *cell.borrow_mut() = writer;
    });
}

pub fn stderr_print(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = STDERR.lock().unwrap().as_mut() {
            w.eprint(s);
        } else {
            use std::io::{Write, stderr};
            print!("{s}");
            stderr().flush().ok();
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDERR.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.eprint(s);
        } else {
            web_sys::console::error_1(&s.into());
        }
    });
}

pub fn stderr_println(s: impl AsRef<str>) {
    let s = s.as_ref();
    #[cfg(not(target_arch = "wasm32"))]
    {
        if let Some(w) = STDERR.lock().unwrap().as_mut() {
            w.eprintln(s);
        } else {
            eprintln!("{s}");
        }
    }
    #[cfg(target_arch = "wasm32")]
    STDERR.with(|cell| {
        if let Some(w) = cell.borrow_mut().as_mut() {
            w.eprintln(s);
        } else {
            web_sys::console::error_1(&s.into());
        }
    });
}
