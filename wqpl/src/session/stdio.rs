use std::cell::RefCell;
use std::sync::{Arc, Mutex, TryLockError};

#[cfg(not(target_arch = "wasm32"))]
pub type WqInputHandle = Box<dyn WqInput + Send>;
#[cfg(target_arch = "wasm32")]
pub type WqInputHandle = Box<dyn WqInput>;

#[cfg(not(target_arch = "wasm32"))]
pub type WqOutputHandle = Box<dyn WqOutput + Send>;
#[cfg(target_arch = "wasm32")]
pub type WqOutputHandle = Box<dyn WqOutput>;

/// Host input used by wq programs.
///
/// REPL history, highlighting, completion, and debugger modes intentionally
/// live outside this trait. Hosts can implement those editor concerns on the
/// concrete input type they retain while giving the session a shared adapter.
pub trait WqInput {
    fn read_line(&mut self, prompt: &str) -> Result<String, WqIoError>;
}

pub enum WqInputPoll {
    Ready(Result<String, WqIoError>),
    Pending,
}

/// A destination for wq program output and evaluator diagnostics.
pub trait WqOutput {
    fn write(&mut self, text: &str) -> Result<(), WqIoError>;

    /// Whether this destination supports terminal control sequences.
    ///
    /// Redirected and callback-backed outputs default to false. Native output
    /// adapters override this with their actual terminal capability.
    fn is_terminal(&self) -> bool {
        false
    }

    /// Dimensions available to this output terminal, if any.
    ///
    /// Hosts with virtual terminals can provide their own dimensions. Other
    /// redirected and callback-backed outputs default to no terminal size.
    fn terminal_size(&self) -> Option<(usize, usize)> {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WqIoError {
    Interrupted,
    Eof,
    Unavailable(&'static str),
    Reentrant(&'static str),
    Other(String),
}

impl std::fmt::Display for WqIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interrupted => f.write_str("input interrupted"),
            Self::Eof => f.write_str("end of file"),
            Self::Unavailable(stream) => write!(f, "{stream} is not configured"),
            Self::Reentrant(stream) => write!(f, "reentrant access to {stream}"),
            Self::Other(error) => f.write_str(error),
        }
    }
}

impl std::error::Error for WqIoError {}

/// Per-session host streams.
///
/// Input uses `RefCell` and outputs use shared, non-blocking locks so
/// interpreter hooks and dynamically scoped debug helpers can write through
/// an immutable VM reference. Reentrant callbacks become normal host errors
/// instead of panicking or deadlocking.
pub(crate) struct RuntimeIo {
    input: RefCell<Option<WqInputHandle>>,
    stdout: RuntimeOutput,
    stderr: RuntimeOutput,
}

#[derive(Clone)]
pub(crate) struct RuntimeOutput {
    stream: &'static str,
    handle: Arc<Mutex<WqOutputHandle>>,
}

impl RuntimeOutput {
    fn new(stream: &'static str, output: WqOutputHandle) -> Self {
        Self {
            stream,
            handle: Arc::new(Mutex::new(output)),
        }
    }

    pub(crate) fn write(&self, text: &str) -> Result<(), WqIoError> {
        self.handle
            .try_lock()
            .map_err(|error| output_lock_error(self.stream, error))?
            .write(text)
    }

    pub(crate) fn write_line(&self, text: &str) -> Result<(), WqIoError> {
        let mut output = self
            .handle
            .try_lock()
            .map_err(|error| output_lock_error(self.stream, error))?;
        let mut line = String::with_capacity(text.len() + 1);
        line.push_str(text);
        line.push('\n');
        output.write(&line)
    }

    fn is_terminal(&self) -> bool {
        self.handle
            .try_lock()
            .is_ok_and(|output| output.is_terminal())
    }

    fn terminal_size(&self) -> Option<(usize, usize)> {
        self.handle
            .try_lock()
            .ok()
            .and_then(|output| output.terminal_size())
    }
}

impl Default for RuntimeIo {
    fn default() -> Self {
        Self {
            input: RefCell::new(None),
            stdout: RuntimeOutput::new("stdout", default_stdout()),
            stderr: RuntimeOutput::new("stderr", default_stderr()),
        }
    }
}

impl RuntimeIo {
    pub(crate) fn set_input(&mut self, input: WqInputHandle) {
        *self.input.get_mut() = Some(input);
    }

    pub(crate) fn clear_input(&mut self) {
        *self.input.get_mut() = None;
    }

    pub(crate) fn set_stdout(&mut self, output: WqOutputHandle) {
        self.stdout = RuntimeOutput::new("stdout", output);
    }

    pub(crate) fn set_stderr(&mut self, output: WqOutputHandle) {
        self.stderr = RuntimeOutput::new("stderr", output);
    }

    pub(crate) fn read_line(&self, prompt: &str) -> Result<String, WqIoError> {
        let mut input = self
            .input
            .try_borrow_mut()
            .map_err(|_| WqIoError::Reentrant("stdin"))?;
        input
            .as_deref_mut()
            .ok_or(WqIoError::Unavailable("stdin"))?
            .read_line(prompt)
    }

    pub(crate) fn write_stdout(&self, text: &str) -> Result<(), WqIoError> {
        self.stdout.write(text)
    }

    pub(crate) fn write_stdout_line(&self, text: &str) -> Result<(), WqIoError> {
        self.stdout.write_line(text)
    }

    pub(crate) fn write_stderr(&self, text: &str) -> Result<(), WqIoError> {
        self.stderr.write(text)
    }

    pub(crate) fn write_stderr_line(&self, text: &str) -> Result<(), WqIoError> {
        self.stderr.write_line(text)
    }

    pub(crate) fn stdout_is_terminal(&self) -> bool {
        self.stdout.is_terminal()
    }

    pub(crate) fn stdout_terminal_size(&self) -> Option<(usize, usize)> {
        self.stdout.terminal_size()
    }

    pub(crate) fn stderr_is_terminal(&self) -> bool {
        self.stderr.is_terminal()
    }

    pub(crate) fn stderr_output(&self) -> RuntimeOutput {
        self.stderr.clone()
    }
}

fn output_lock_error(
    stream: &'static str,
    error: TryLockError<std::sync::MutexGuard<'_, WqOutputHandle>>,
) -> WqIoError {
    match error {
        TryLockError::WouldBlock => WqIoError::Reentrant(stream),
        TryLockError::Poisoned(_) => WqIoError::Other(format!("{stream} output lock is poisoned")),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn default_stdout() -> WqOutputHandle {
    Box::new(NativeStdout)
}

#[cfg(target_arch = "wasm32")]
fn default_stdout() -> WqOutputHandle {
    Box::new(NullOutput)
}

#[cfg(not(target_arch = "wasm32"))]
fn default_stderr() -> WqOutputHandle {
    Box::new(NativeStderr)
}

#[cfg(target_arch = "wasm32")]
fn default_stderr() -> WqOutputHandle {
    Box::new(NullOutput)
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeStdout;

#[cfg(not(target_arch = "wasm32"))]
impl WqOutput for NativeStdout {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        use std::io::Write as _;

        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(text.as_bytes())
            .and_then(|()| stdout.flush())
            .map_err(|error| WqIoError::Other(error.to_string()))
    }

    fn is_terminal(&self) -> bool {
        use std::io::IsTerminal as _;

        std::io::stdout().is_terminal()
    }

    fn terminal_size(&self) -> Option<(usize, usize)> {
        if !self.is_terminal() {
            return None;
        }
        terminal_size::terminal_size()
            .map(|(width, height)| (usize::from(width.0), usize::from(height.0)))
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct NativeStderr;

#[cfg(not(target_arch = "wasm32"))]
impl WqOutput for NativeStderr {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        use std::io::Write as _;

        let mut stderr = std::io::stderr().lock();
        stderr
            .write_all(text.as_bytes())
            .and_then(|()| stderr.flush())
            .map_err(|error| WqIoError::Other(error.to_string()))
    }

    fn is_terminal(&self) -> bool {
        use std::io::IsTerminal as _;

        std::io::stderr().is_terminal()
    }
}

#[cfg(target_arch = "wasm32")]
struct NullOutput;

#[cfg(target_arch = "wasm32")]
impl WqOutput for NullOutput {
    fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    struct Capture(Arc<Mutex<String>>);

    impl WqOutput for Capture {
        fn write(&mut self, text: &str) -> Result<(), WqIoError> {
            self.0
                .lock()
                .expect("capture lock should not be poisoned")
                .push_str(text);
            Ok(())
        }
    }

    #[test]
    fn stdout_and_stderr_are_independent() {
        let stdout = Arc::new(Mutex::new(String::new()));
        let stderr = Arc::new(Mutex::new(String::new()));
        let mut io = RuntimeIo::default();
        io.set_stdout(Box::new(Capture(Arc::clone(&stdout))));
        io.set_stderr(Box::new(Capture(Arc::clone(&stderr))));

        io.write_stdout("out").expect("write stdout");
        io.write_stderr("err").expect("write stderr");

        assert_eq!(&*stdout.lock().expect("stdout lock"), "out");
        assert_eq!(&*stderr.lock().expect("stderr lock"), "err");
    }

    struct ChunkCapture(Arc<Mutex<Vec<String>>>);

    impl WqOutput for ChunkCapture {
        fn write(&mut self, text: &str) -> Result<(), WqIoError> {
            self.0
                .lock()
                .expect("chunk capture lock should not be poisoned")
                .push(text.to_string());
            Ok(())
        }
    }

    #[test]
    fn line_output_is_delivered_as_one_callback_chunk() {
        let chunks = Arc::new(Mutex::new(Vec::new()));
        let mut io = RuntimeIo::default();
        io.set_stdout(Box::new(ChunkCapture(Arc::clone(&chunks))));

        io.write_stdout_line("one line").expect("write line");

        assert_eq!(
            chunks
                .lock()
                .expect("chunk capture lock should not be poisoned")
                .as_slice(),
            ["one line\n"]
        );
    }

    struct TerminalOutput;

    impl WqOutput for TerminalOutput {
        fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
            Ok(())
        }

        fn is_terminal(&self) -> bool {
            true
        }

        fn terminal_size(&self) -> Option<(usize, usize)> {
            Some((120, 40))
        }
    }

    #[test]
    fn output_capabilities_are_owned_by_each_handle() {
        let mut io = RuntimeIo::default();
        io.set_stdout(Box::new(Capture(Arc::new(Mutex::new(String::new())))));
        io.set_stderr(Box::new(TerminalOutput));

        assert!(!io.stdout_is_terminal());
        assert!(io.stderr_is_terminal());
        assert_eq!(io.stdout_terminal_size(), None);

        io.set_stdout(Box::new(TerminalOutput));
        assert_eq!(io.stdout_terminal_size(), Some((120, 40)));
    }

    struct ReentrantOutput {
        output: Arc<Mutex<Option<RuntimeOutput>>>,
    }

    impl WqOutput for ReentrantOutput {
        fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
            let output = self
                .output
                .lock()
                .expect("reentrant output state should not be poisoned")
                .clone()
                .expect("runtime output should be installed");
            output.write("nested")
        }
    }

    #[test]
    fn reentrant_output_returns_an_error_instead_of_deadlocking() {
        let output = Arc::new(Mutex::new(None));
        let mut io = RuntimeIo::default();
        io.set_stdout(Box::new(ReentrantOutput {
            output: Arc::clone(&output),
        }));
        *output
            .lock()
            .expect("reentrant output state should not be poisoned") = Some(io.stdout.clone());

        assert_eq!(
            io.write_stdout("outer"),
            Err(WqIoError::Reentrant("stdout"))
        );
    }
}
