#![cfg(not(target_arch = "wasm32"))]

macro_rules! wqdb_println {
    ($host:expr, $text:expr) => {
        $host.write_line($text)
    };
}

mod command;
pub(crate) mod editor;
mod execute;
mod host;
mod render;
mod shell;

pub(crate) use shell::WqdbShell;
