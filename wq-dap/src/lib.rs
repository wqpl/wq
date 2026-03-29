//! # dap-rs, a Rust implementation of the Debug Adapter Protocol
//!
//! ## Introduction
//!
//! This crate is a Rust implementation of the [Debug Adapter Protocol](https://microsoft.github.io/debug-adapter-protocol/)
//! (or DAP for short).
//!
//! The best way to think of DAP is to compare it to [LSP](https://microsoft.github.io/language-server-protocol/)
//! (Language Server Protocol) but for debuggers. The core idea is the same: a
//! protocol that serves as *lingua franca* for editors and debuggers to talk to
//! each other. This means that an editor that implements DAP can use a debugger
//! that also implements DAP.
//!
//! In practice, the adapter might be separate from the actual debugger. For
//! example, one could implement an adapter that writes commands to the stdin of
//! a gdb subprocess, then parses the output it receives (this is why it's
//! called an "adapter" - it adapts the debugger to editors that know DAP).
pub mod basemsg;
pub mod error;
pub mod event;
pub mod prelude;
pub mod request;
pub mod response;
pub mod revreqt;
pub mod server;
pub mod r#type;
pub mod util;
pub use util::get_spec_version;
