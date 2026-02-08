pub mod default;
pub mod sample;

use crate::{
    value::{Value, WqResult},
    vm::Vm,
};

/// The trait for an instruction interpreter.
///
/// An interpreter is responsible for executing instructions from the VM's current instruction set,
/// updating the VM's state (stack, pc, locals, etc.) accordingly.
pub trait Interpreter {
    /// Execute instructions until the PC reaches the limit.
    ///
    /// The interpreter is free to execute fewer instructions if a return or error occurs.
    fn execute(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpreterKind {
    Default,
    Sample,
}

pub const INTERPRETER_NAMES: [&str; 2] = ["default", "sample"];

impl InterpreterKind {
    pub fn name(&self) -> &'static str {
        match self {
            InterpreterKind::Default => "default",
            InterpreterKind::Sample => "sample",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "default" | "def" | "definterp" => Some(InterpreterKind::Default),
            "sample" => Some(InterpreterKind::Sample),

            _ => None,
        }
    }
}
