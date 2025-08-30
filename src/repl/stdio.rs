use std::sync::Mutex;

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
            StdinError::Eof => write!(f, "End of File"),
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

pub static STDIN: Lazy<Mutex<Option<Box<dyn ReplStdin>>>> = Lazy::new(|| Mutex::new(None));

pub fn set_stdin(reader: Box<dyn ReplStdin>) {
    *STDIN.lock().unwrap() = Some(reader);
}

pub fn stdin_readline(prompt: &str) -> Result<String, StdinError> {
    let mut guard = STDIN.lock().unwrap();
    if let Some(r) = guard.as_mut() {
        r.readline(prompt)
    } else {
        Err(StdinError::Other("Stdin not initialized".into()))
    }
}

pub fn stdin_add_history(line: &str) {
    if let Some(r) = STDIN.lock().unwrap().as_mut() {
        r.add_history(line);
    }
}

pub fn stdin_set_highlight(on: bool) {
    if let Some(r) = STDIN.lock().unwrap().as_deref_mut() {
        r.set_highlight(on); // no-op if impl doesn’t support it
    }
}

pub fn stdin_highlight_enabled() -> bool {
    let mut r = STDIN.lock().unwrap();
    let g = r.as_mut().expect("STDIN not set");
    g.highlight_enabled()
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

pub static STDOUT: Lazy<Mutex<Option<Box<dyn ReplStdout>>>> = Lazy::new(|| Mutex::new(None));

pub fn set_stdout(writer: Option<Box<dyn ReplStdout>>) {
    *STDOUT.lock().unwrap() = writer;
}

pub fn stdout_print(s: &str) {
    if let Some(w) = STDOUT.lock().unwrap().as_mut() {
        w.print(s);
    } else {
        use std::io::{Write, stdout};
        print!("{s}");
        let _ = stdout().flush();
    }
}

pub fn stdout_println(s: &str) {
    if let Some(w) = STDOUT.lock().unwrap().as_mut() {
        w.println(s);
    } else {
        println!("{s}");
    }
}

pub trait ReplStderr: Send {
    fn eprint(&mut self, s: &str);
    fn eprintln(&mut self, s: &str);
}

pub static STDERR: Lazy<Mutex<Option<Box<dyn ReplStderr>>>> = Lazy::new(|| Mutex::new(None));

pub fn set_stderr(writer: Option<Box<dyn ReplStderr>>) {
    *STDERR.lock().unwrap() = writer;
}

pub fn stderr_print(s: &str) {
    if let Some(w) = STDERR.lock().unwrap().as_mut() {
        w.eprint(s);
    } else {
        use std::io::{Write, stderr};
        print!("{s}");
        let _ = stderr().flush();
    }
}

pub fn stderr_println(s: &str) {
    if let Some(w) = STDERR.lock().unwrap().as_mut() {
        w.eprintln(s);
    } else {
        eprintln!("{s}");
    }
}
