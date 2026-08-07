use std::error::Error;
use std::fmt::Display;
use std::io;
use std::process::Command;

pub type TestError = Box<dyn Error + Send + Sync>;
pub type TestResult<T = ()> = Result<T, TestError>;

pub trait ResultContext<T> {
    fn context(self, message: &str) -> TestResult<T>;

    fn with_context(self, message: impl FnOnce() -> String) -> TestResult<T>;
}

impl<T, E> ResultContext<T> for Result<T, E>
where
    E: Display,
{
    fn context(self, message: &str) -> TestResult<T> {
        self.map_err(|error| test_error(format!("{message}: {error}")))
    }

    fn with_context(self, message: impl FnOnce() -> String) -> TestResult<T> {
        self.map_err(|error| test_error(format!("{}: {error}", message())))
    }
}

impl<T> ResultContext<T> for Option<T> {
    fn context(self, message: &str) -> TestResult<T> {
        self.ok_or_else(|| test_error(message))
    }

    fn with_context(self, message: impl FnOnce() -> String) -> TestResult<T> {
        self.ok_or_else(|| test_error(message()))
    }
}

pub fn test_error(message: impl Into<String>) -> TestError {
    Box::new(io::Error::other(message.into()))
}

pub fn wq_command() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wq"));
    command.env_remove("CLICOLOR_FORCE").env("NO_COLOR", "1");
    command
}
