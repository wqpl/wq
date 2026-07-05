pub mod profiler;
pub mod sample;
pub mod vanilla;

use crate::ast::{BinaryOperator, UnaryOperator};
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::vm::inst::Instruction;

/// The trait for an instruction interpreter.
///
/// An interpreter is responsible for executing instructions from the VM's
/// current instruction set, updating the VM's state (stack, pc, locals, etc.)
/// accordingly.
pub trait Interpreter {
    /// Execute instructions until the PC reaches the limit.
    ///
    /// The interpreter is free to execute fewer instructions if a return or
    /// error occurs.
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterpreterKind {
    Vanilla,
    Sample,
    Profiler,
}

impl InterpreterKind {
    pub fn names() -> &'static [&'static str] {
        &["vanilla", "sample", "profiler"]
    }

    pub fn name(&self) -> &'static str {
        match self {
            InterpreterKind::Vanilla => "vanilla",
            InterpreterKind::Sample => "sample",
            InterpreterKind::Profiler => "profiler",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "vanilla" | "v" => Some(InterpreterKind::Vanilla),
            "sample" | "s" => Some(InterpreterKind::Sample),
            "profiler" | "p" => Some(InterpreterKind::Profiler),
            _ => None,
        }
    }
}

pub(crate) trait InterpreterHook: 'static {
    fn before_instruction(&self, _vm: &Vm, _idx: usize, _op: &Instruction) {}
    fn on_load_var_cache_hit(&self, _slot_cached: &dyn Fn() -> bool) {}
    fn on_load_var_cache_miss(&self) {}
    fn on_call_user_cache_hit(&self) {}
    fn on_call_user_cache_miss(&self) {}
    fn on_binary_result(&self, _op: &BinaryOperator, _result: &Value) {}
    fn on_unary_result(&self, _op: &UnaryOperator, _result: &Value) {}
    fn on_builtin_result(&self, _name: &str, _argc: usize, _result: &Value) {}
    fn on_cat_alloc(&self, _len: &dyn Fn() -> usize) {}
    fn on_list_alloc(&self, _len: &dyn Fn() -> usize) {}
    fn on_dict_alloc(&self, _len: &dyn Fn() -> usize) {}
    fn on_range_alloc(&self, _len: &dyn Fn() -> usize) {}
    fn on_closure_capture_alloc(&self, _len: &dyn Fn() -> usize) {}
    fn on_return(&self, _vm: &Vm) {}
}

#[derive(Default)]
pub(crate) struct NoOpInterpreterHook;

impl InterpreterHook for NoOpInterpreterHook {}

pub(crate) static NO_OP_HOOK: NoOpInterpreterHook = NoOpInterpreterHook;
