pub mod instruction;

pub(crate) mod call;
pub(crate) mod debug;

mod slot;

use std::{borrow::Cow, sync::Arc};

use crate::{
    builtins::Builtins,
    interpreters::Interpreter,
    value::{Value, ValueCell, WqResult},
    vm::{call::CallTarget, debug::Backtrace, instruction::Instruction, slot::Slot},
    wqdb::{ChunkId, DebugInfo, Wqdb},
    wqerror::{WqError, WqErrorType},
};

use ahash::AHashMap;

pub type GlobalMap = AHashMap<String, Value>;
pub type GlobalSlotMap = AHashMap<String, usize>;

pub struct Vm {
    pub instructions: Arc<[Instruction]>,
    pub(crate) pc: usize,
    pub(crate) stack: Vec<Value>,
    /// Global variables
    pub(crate) globals: GlobalMap,
    /// Global slots (stable indices) for fast access
    pub(crate) global_slots: Vec<Value>,
    pub(crate) global_slot_versions: Vec<u64>,
    pub(crate) global_slot_map: GlobalSlotMap,
    pub(crate) global_slots_dirty: bool,
    pub(crate) globals_dirty: bool,
    pub(crate) builtins: Builtins,
    /// Stack of local slot frames
    pub(crate) locals: Vec<Vec<Slot>>,
    /// Stack of capture vectors (per frame), for closures
    pub(crate) captures: Vec<Vec<ValueCell>>,
    /// Inline caches for global lookups and call sites
    pub(crate) inline_cache: Vec<InlineCache>,
    /// Stack of currently executing functions/closures for LoadSelf
    pub(crate) current_closure_stack: Vec<Value>,
    // args_scratch: Vec<Value>,

    // Debugging
    pub wqdb: Wqdb,
    pub debug_info: DebugInfo,
    pub(crate) current_chunk: ChunkId,
    pub(crate) call_stack: Vec<Frame>,
    /// Lightweight backtrace mode: build minimal debug info for frames on error
    pub(crate) bt_mode: bool,
    /// Base byte offset into current source file for this execution (for loader slices)
    pub(crate) debug_src_offset: usize,
    pub(crate) last_backtrace: Option<Backtrace>,
}

#[derive(Clone)]
pub(crate) struct Frame {
    pub chunk: ChunkId,
    pub pc: usize,
    pub func_name: std::sync::Arc<str>,
}

#[derive(Clone, Default)]
pub(crate) struct InlineCache {
    pub(crate) version: u64,
    pub(crate) value: Option<Value>,
    pub(crate) call_target: Option<CallTarget>,
    pub(crate) slot: Option<usize>,
}

impl Vm {
    pub fn new(instructions: Vec<Instruction>) -> Self {
        let len = instructions.len();
        Vm {
            instructions: Arc::<[Instruction]>::from(instructions),
            pc: 0,
            stack: Vec::with_capacity(256),
            globals: AHashMap::new(),
            global_slots: Vec::new(),
            global_slot_versions: Vec::new(),
            global_slot_map: AHashMap::new(),
            global_slots_dirty: false,
            globals_dirty: false,
            builtins: Builtins::new(),
            locals: Vec::new(),
            captures: Vec::new(),
            inline_cache: vec![InlineCache::default(); len],
            current_closure_stack: Vec::new(),
            // args_scratch: Vec::new(),
            wqdb: Wqdb::default(),
            debug_info: DebugInfo::default(),
            current_chunk: ChunkId(0),
            call_stack: Vec::new(),
            bt_mode: false,
            debug_src_offset: 0,
            last_backtrace: None,
        }
    }

    /// Replace instructions and reset execution state.
    pub fn reset(&mut self, instructions: Vec<Instruction>) {
        self.instructions = Arc::<[Instruction]>::from(instructions);
        self.pc = 0;
        self.stack.clear();
        self.locals.clear();
        self.inline_cache = vec![InlineCache::default(); self.instructions.len()];
        self.current_closure_stack.clear();
        // Ensure no stale frames leak across runs (affects backtraces)
        self.call_stack.clear();
        // Keep debug_src_offset as set by evaluator for current run
    }

    /// Access the global environment.
    pub fn global_env(&self) -> &GlobalMap {
        &self.globals
    }

    /// Mutable access to the global environment.
    pub fn global_env_mut(&mut self) -> &mut GlobalMap {
        self.sync_globals_from_slots();
        self.global_slots_dirty = true;
        &mut self.globals
    }
}

impl Vm {
    // pub fn run(&mut self) -> WqResult<Value> {
    //     let mut interpreter = DefaultInterpreter;
    //     self.run_with_interpreter(&mut interpreter)
    // }

    pub fn run_with_interpreter<I: Interpreter + ?Sized>(
        &mut self,
        interpreter: &mut I,
    ) -> WqResult<Value> {
        self.sync_global_slots_if_dirty();
        let limit = self.instructions.len();
        let result = interpreter.execute(self, limit);
        self.sync_globals_from_slots();
        result
    }
}

impl Vm {
    #[inline]
    pub(crate) fn is_internal_ephemeral(&self, name: &str) -> bool {
        name == "_n" || name.starts_with("--vm-n-loop-old-") || name.starts_with("--vm-n-loop-res-")
    }

    pub fn current_chunk_id(&self) -> ChunkId {
        self.current_chunk
    }

    pub(crate) fn lookup_global(&self, name: &str) -> Option<Value> {
        if self.global_slots_dirty {
            return self.globals.get(name).cloned();
        }
        if let Some(slot) = self.lookup_global_slot(name) {
            return self.global_slots.get(slot).cloned();
        }
        self.globals.get(name).cloned()
    }

    #[inline]
    pub(crate) fn lookup_global_slot(&self, name: &str) -> Option<usize> {
        self.global_slot_map.get(name).copied()
    }

    #[inline]
    pub(crate) fn global_slot_value(&self, slot: usize) -> Option<&Value> {
        self.global_slots.get(slot)
    }

    #[inline]
    pub(crate) fn global_slot_version(&self, slot: usize) -> u64 {
        self.global_slot_versions.get(slot).copied().unwrap_or(0)
    }

    #[inline]
    pub(crate) fn bump_global_slot_version(&mut self, slot: usize) -> u64 {
        if let Some(entry) = self.global_slot_versions.get_mut(slot) {
            *entry = entry.wrapping_add(1);
            *entry
        } else {
            0
        }
    }

    fn sync_global_slots_if_dirty(&mut self) {
        if !self.global_slots_dirty {
            return;
        }
        self.global_slot_map.clear();
        self.global_slots.clear();
        self.global_slot_versions.clear();
        for (name, value) in self.globals.iter() {
            let slot = self.global_slots.len();
            self.global_slot_map.insert(name.clone(), slot);
            self.global_slots.push(value.clone());
            self.global_slot_versions.push(0);
        }
        self.globals_dirty = false;
        self.global_slots_dirty = false;
    }

    pub(crate) fn sync_globals_from_slots(&mut self) {
        if !self.globals_dirty {
            return;
        }
        self.globals.clear();
        for (name, slot) in self.global_slot_map.iter() {
            if let Some(val) = self.global_slots.get(*slot) {
                self.globals.insert(name.clone(), val.clone());
            }
        }
        self.globals_dirty = false;
    }

    pub(crate) fn with_global_slot_mut<R>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Value) -> R,
    ) -> Option<R> {
        self.sync_global_slots_if_dirty();
        let slot = self.lookup_global_slot(name)?;
        let result = {
            let slot_val = self.global_slots.get_mut(slot)?;
            f(slot_val)
        };
        self.globals_dirty = true;
        self.bump_global_slot_version(slot);
        Some(result)
    }

    pub(crate) fn assign_global(&mut self, name: &str, value: Value) {
        self.assign_global_and_slot(name, value);
    }

    pub(crate) fn assign_global_and_slot(&mut self, name: &str, mut value: Value) -> usize {
        self.sync_global_slots_if_dirty();
        if self.wqdb.enabled || self.bt_mode {
            match &mut value {
                Value::CompiledFunction {
                    params,
                    instructions,
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                    ..
                }
                | Value::Closure {
                    params,
                    instructions,
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                    ..
                } => {
                    let chunk = self.ensure_dbg_chunk_with_spans(
                        name,
                        *dbg_chunk,
                        instructions.as_ref(),
                        dbg_stmt_spans,
                        dbg_local_names,
                        params,
                    );
                    *dbg_chunk = chunk;
                }
                _ => {}
            }
        }
        self.globals_dirty = true;
        let slot = match self.global_slot_map.get(name).copied() {
            Some(slot) => slot,
            None => {
                let slot = self.global_slots.len();
                self.global_slot_map.insert(name.to_string(), slot);
                self.global_slots.push(Value::unit());
                self.global_slot_versions.push(0);
                slot
            }
        };
        if let Some(dest) = self.global_slots.get_mut(slot) {
            *dest = value;
        }
        self.bump_global_slot_version(slot);
        slot
    }

    pub(crate) fn assign_global_at_slot(&mut self, name: &str, slot: usize, mut value: Value) {
        self.sync_global_slots_if_dirty();
        if self.wqdb.enabled || self.bt_mode {
            match &mut value {
                Value::CompiledFunction {
                    params,
                    instructions,
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                    ..
                }
                | Value::Closure {
                    params,
                    instructions,
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                    ..
                } => {
                    let chunk = self.ensure_dbg_chunk_with_spans(
                        name,
                        *dbg_chunk,
                        instructions.as_ref(),
                        dbg_stmt_spans,
                        dbg_local_names,
                        params,
                    );
                    *dbg_chunk = chunk;
                }
                _ => {}
            }
        }
        if let Some(dest) = self.global_slots.get_mut(slot) {
            *dest = value;
        }
        self.globals_dirty = true;
        if self.global_slot_map.get(name).copied().is_none() {
            self.global_slot_map.insert(name.to_string(), slot);
        }
        if self.global_slot_versions.len() <= slot {
            self.global_slot_versions.resize(slot + 1, 0);
        }
        self.bump_global_slot_version(slot);
    }
}

#[inline]
pub(crate) fn ensure_stack_len<F>(stack: &[Value], need: usize, ctx: F) -> WqResult<()>
where
    F: FnOnce() -> Cow<'static, str>,
{
    if stack.len() < need {
        let msg = ctx();
        return Err(vm_err(format!(
            "stack underflow: need {need} for {msg}, have {}",
            stack.len()
        )));
    }
    Ok(())
}

#[inline]
pub(crate) fn pop1_stack<F>(stack: &mut Vec<Value>, ctx: F) -> WqResult<Value>
where
    F: FnOnce() -> Cow<'static, str>,
{
    stack
        .pop()
        .ok_or_else(|| vm_err(format!("stack underflow: {}", ctx())))
}

#[inline]
pub(crate) fn pop2_stack<F>(stack: &mut Vec<Value>, ctx: F) -> WqResult<(Value, Value)>
where
    F: FnOnce() -> Cow<'static, str>,
{
    if stack.len() < 2 {
        let msg = ctx();
        return Err(vm_err(format!(
            "stack underflow: need 2 for {msg}, have {}",
            stack.len()
        )));
    }
    let b = stack.pop().unwrap();
    let a = stack.pop().unwrap();
    Ok((a, b))
}

#[inline]
pub(crate) fn last_clone_stack<F>(stack: &[Value], ctx: F) -> WqResult<Value>
where
    F: FnOnce() -> Cow<'static, str>,
{
    stack
        .last()
        .cloned()
        .ok_or_else(|| vm_err(format!("stack underflow: {}", ctx())))
}

#[inline]
fn vm_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Vm).src("vm").msg(msg.into())
}

#[inline]
fn call_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Call).src("vm").msg(msg.into())
}

#[inline]
fn not_bound_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::NotBound)
        .src("vm")
        .msg(msg.into())
}

#[inline]
fn arity_err_vm(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Arity).src("vm").msg(msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        compiler::Compiler,
        interpreters::default::DefaultInterpreter,
        lexer::Lexer,
        parser::Parser,
        post_parser::{folder, resolver::Resolver},
    };

    fn eval_expr(src: &str) -> Value {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        let ast = folder::fold(ast);
        let mut compiler = Compiler::new();
        compiler
            .compile(&ast)
            .expect("compile comparison chain expression");
        compiler.fuse();
        compiler.instructions.push(Instruction::Return);
        let instructions = std::mem::take(&mut compiler.instructions);
        let mut vm = Vm::new(instructions);
        vm.run_with_interpreter(&mut DefaultInterpreter)
            .expect("execute")
    }

    #[test]
    fn chained_equality_all_true() {
        let result = eval_expr("1=1=1");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn chained_equality_false_middle() {
        let result = eval_expr("1=2=2");
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn chained_mixed_inequality_true() {
        let result = eval_expr("3<4<=4");
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn chained_mixed_inequality_false() {
        let result = eval_expr("3<2<=5");
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn floor_div_int_basic() {
        assert_eq!(eval_expr("floor[7/3]"), Value::Int(2));
        assert_eq!(eval_expr("floor[-7/3]"), Value::Int(-3));
        assert_eq!(eval_expr("floor[7/-3]"), Value::Int(-3));
        assert_eq!(eval_expr("floor[-7/-3]"), Value::Int(2));
        assert_eq!(eval_expr("floor[4/2]"), Value::Int(2));
    }

    #[test]
    fn floor_div_float_and_mixed() {
        assert_eq!(eval_expr("floor[7.5/2]"), Value::Int(3));
        assert_eq!(eval_expr("floor[7/2.0]"), Value::Int(3));
        assert_eq!(eval_expr("floor[(-7.0)/3]"), Value::Int(-3));
    }

    #[test]
    fn floor_div_broadcasting_list_scalar() {
        let result = eval_expr("floor[(1;2;3)/2]");
        assert_eq!(
            result,
            Value::from_items(vec![Value::Int(0), Value::Int(1), Value::Int(1)])
        );
    }
}
