pub mod call;
pub(crate) mod debug;
pub mod inst;
mod slot;
pub(crate) use slot::Slot;
pub(crate) mod trace;

use std::ptr::NonNull;
use std::sync::Arc;

use ahash::AHashMap;

use crate::builtins::{BuiltinPreset, Builtins};
use crate::interpret::{Interpreter, InterpreterHook, InterpreterKind};
use crate::value::cell::ValueCell;
use crate::value::func::{ClosureData, FunctionData};
use crate::value::{Value, WqResult};
use crate::vm::call::ResolvedCallable;
use crate::vm::inst::Instruction;
use crate::vm::trace::TraceRecord;
use crate::wqdb::Wqdb;
use crate::wqdb::data::{Backtrace, ChunkId, DebugChunkSpec, DebugInfo, DebugLocalsFrame};
use crate::wqerror::{WqError, WqErrorType};

pub type GlobalMap = AHashMap<String, Value>;
pub type GlobalSlotMap = AHashMap<String, usize>;

pub struct Vm {
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) pc: usize,
    pub(crate) stack: Vec<Value>,
    /// Global slots (stable indices) for fast access
    pub(crate) global_slots: Vec<Value>,
    pub(crate) global_slot_versions: Vec<u64>,
    pub(crate) global_slot_map: GlobalSlotMap,
    pub(crate) builtins: Builtins,
    pub(crate) builtins_preset: BuiltinPreset,
    /// Stack of local slot frames
    pub(crate) locals: Vec<Vec<Slot>>,
    /// Stack of capture vectors (per frame), for closures
    pub(crate) captures: Vec<Arc<[ValueCell]>>,
    /// Inline caches for global lookups and call sites
    pub(crate) inline_cache: Vec<InlineCache>,
    /// Pool of cleared per-instruction caches, keyed by instruction count.
    pub(crate) cache_pool: AHashMap<usize, Vec<Vec<InlineCache>>>,
    /// Pool of cleared local-variable frames, keyed by slot count.
    pub(crate) locals_pool: AHashMap<u16, Vec<Vec<Slot>>>,
    /// Pool of cleared operand stacks to avoid per-call allocation.
    pub(crate) stack_pool: Vec<Vec<Value>>,
    /// Stack of currently executing functions/closures for LoadSelf
    pub(crate) current_closure_stack: Vec<Value>,
    // args_scratch: Vec<Value>,
    /// Tail-call journal for backtrace when TCE is active.
    pub(crate) tail_call_journal: Vec<Frame>,
    pub(crate) tail_call_journal_overflow: bool,

    /// Maximum physical call depth before raising Recursion error.
    pub(crate) max_call_depth: usize,

    // Debugging
    pub wqdb: Wqdb,
    pub debug_info: DebugInfo,
    pub(crate) current_chunk: ChunkId,
    pub(crate) call_stack: Vec<Frame>,
    /// Lightweight backtrace mode: build minimal debug info for frames on error
    pub(crate) bt_mode: bool,
    /// Per-run debug metadata required by runtime features like `@d`.
    pub(crate) runtime_debug_info: bool,
    /// Base byte offset into current source file for this execution (for loader
    /// slices)
    pub(crate) debug_src_offset: usize,
    pub(crate) last_backtrace: Option<Backtrace>,
    pub(crate) last_locals_snapshot: Option<Vec<DebugLocalsFrame>>,
    pub(crate) hooks: Option<NonNull<dyn InterpreterHook>>,
    pub(crate) try_depth: usize,
    pub(crate) returned: bool,
    /// The interpreter kind to use for nested calls.
    pub(crate) interpreter_kind: InterpreterKind,
    /// Named-argument metadata set by SetupNamedCall, consumed by next call.
    pub(crate) pending_named_meta: Option<Box<crate::vm::inst::NamedArgMeta>>,
    /// Value-provenance recording state for `@d` expressions.
    ///
    /// `trace_depth` is a nesting counter so a `@d` inside a callee of another
    /// `@d` does not interfere. Probes only fire when `trace_depth > 0`.
    ///
    /// `trace_bases` records the `trace_buf.len()` snapshot at each
    /// `TraceBegin`; the matching `Debug` renders records `[base..]` and
    /// truncates the buf back to that base so the outer trace is not polluted.
    pub(crate) trace_depth: u32,
    pub(crate) trace_buf: Vec<TraceRecord>,
    pub(crate) trace_bases: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Frame {
    pub chunk: ChunkId,
    pub pc: usize,
    pub func_name: std::sync::Arc<str>,
}

#[derive(Clone, Default)]
pub(crate) struct InlineCache {
    pub(crate) version: u64,
    pub(crate) call_target: Option<ResolvedCallable>,
    pub(crate) slot: Option<usize>,
    pub(crate) slot_b: Option<usize>,
    /// Cached frame depth (0 = innermost) for CallLocal / TailCallLocal.
    pub(crate) local_frame_depth: Option<u16>,
}

impl Vm {
    pub(crate) fn new(instructions: Vec<Instruction>) -> Self {
        let len = instructions.len();
        Vm {
            instructions: Arc::<[Instruction]>::from(instructions),
            pc: 0,
            stack: Vec::with_capacity(256),
            global_slots: Vec::new(),
            global_slot_versions: Vec::new(),
            global_slot_map: AHashMap::new(),
            builtins: Builtins::new(),
            builtins_preset: BuiltinPreset::DEFAULT,
            locals: Vec::new(),
            captures: Vec::new(),
            inline_cache: vec![InlineCache::default(); len],
            cache_pool: AHashMap::new(),
            locals_pool: AHashMap::new(),
            stack_pool: Vec::new(),
            current_closure_stack: Vec::new(),
            // args_scratch: Vec::new(),
            tail_call_journal: Vec::new(),
            tail_call_journal_overflow: false,
            max_call_depth: if cfg!(debug_assertions) { 64 } else { 1024 },
            wqdb: Wqdb::default(),
            debug_info: DebugInfo::default(),
            current_chunk: ChunkId(0),
            call_stack: Vec::new(),
            bt_mode: false,
            runtime_debug_info: false,

            debug_src_offset: 0,
            last_backtrace: None,
            last_locals_snapshot: None,
            hooks: None,
            try_depth: 0,
            returned: false,
            interpreter_kind: InterpreterKind::Vanilla,
            pending_named_meta: None,
            trace_depth: 0,
            trace_buf: Vec::new(),
            trace_bases: Vec::new(),
        }
    }

    /// Replace instructions and reset execution state.
    pub(crate) fn reset_inst_and_state(&mut self, instructions: Vec<Instruction>) {
        self.instructions = Arc::<[Instruction]>::from(instructions);
        self.pc = 0;
        self.stack.clear();
        self.locals.clear();
        self.inline_cache = vec![InlineCache::default(); self.instructions.len()];
        self.current_closure_stack.clear();
        self.hooks = None;
        self.try_depth = 0;
        self.returned = false;
        // Ensure no stale frames leak
        self.call_stack.clear();
        self.tail_call_journal.clear();
        self.tail_call_journal_overflow = false;
        self.trace_depth = 0;
        self.trace_buf.clear();
        self.trace_bases.clear();
        // Keep debug_src_offset as set by session for current run
    }

    /// Reset all global variable state (for session reset).
    pub(crate) fn reset_globals(&mut self) {
        self.global_slots.clear();
        self.global_slot_versions.clear();
        self.global_slot_map.clear();
    }

    /// Build a snapshot of globals from slots.
    pub fn global_env(&self) -> GlobalMap {
        let mut map = GlobalMap::default();
        for (name, slot) in self.global_slot_map.iter() {
            if let Some(val) = self.global_slots.get(*slot) {
                map.insert(name.clone(), val.clone());
            }
        }
        map
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
        let limit = self.instructions.len();
        let result = interpreter.interpret(self, limit);
        if result.is_err() {
            self.capture_bt_if_empty();
        }
        result
    }
}

impl Vm {
    /// Normally returns `true` because `Session` enables backtrace mode by
    /// default. This is unrelated to `debug_log_flags`, which only controls
    /// logging and does not affect backtraces or debug artifacts.
    #[inline]
    pub(crate) fn debug_artifacts_enabled(&self) -> bool {
        self.runtime_debug_info || self.wqdb.enabled || self.bt_mode || self.wqdb.on_pause.is_some()
    }

    pub fn set_runtime_debug_info(&mut self, flag: bool) {
        self.runtime_debug_info = flag;
    }

    #[inline]
    pub(crate) fn push_tail_call_frame(&mut self, frame: Frame) {
        const TAIL_CALL_JOURNAL_CAP: usize = 128;
        if self.tail_call_journal.len() >= TAIL_CALL_JOURNAL_CAP {
            self.tail_call_journal_overflow = true;
            // Shift out oldest to keep most recent
            self.tail_call_journal.remove(0);
        }
        self.tail_call_journal.push(frame);
    }

    pub fn current_chunk_id(&self) -> ChunkId {
        self.current_chunk
    }

    pub(crate) fn local_slot_name(&self, slot: usize) -> Option<&str> {
        self.debug_info
            .chunk_opt(self.current_chunk)?
            .local_names
            .as_ref()?
            .get(slot)
            .map(String::as_str)
    }

    pub(crate) fn attach_local_slot_note(&self, slot: usize, err: WqError) -> WqError {
        if let Some(name) = self.local_slot_name(slot) {
            err.attach_note(format!("local slot {slot}: {name}"))
        } else {
            err
        }
    }

    pub(crate) fn lookup_global(&self, name: &str) -> Option<Value> {
        if let Some(slot) = self.lookup_global_slot(name) {
            return self.global_slots.get(slot).cloned();
        }
        None
    }

    pub(crate) fn lookup_global_ref(&self, name: &str) -> Option<&Value> {
        if let Some(slot) = self.lookup_global_slot(name) {
            return self.global_slots.get(slot);
        }
        None
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

    pub(crate) fn with_global_slot_mut<R>(
        &mut self,
        name: &str,
        f: impl FnOnce(&mut Value) -> R,
    ) -> Option<R> {
        let slot = self.lookup_global_slot(name)?;
        let result = {
            let slot_val = self.global_slots.get_mut(slot)?;
            f(slot_val)
        };
        self.bump_global_slot_version(slot);
        Some(result)
    }

    pub(crate) fn assign_global_and_slot(&mut self, name: &str, mut value: Value) -> usize {
        if self.debug_artifacts_enabled() {
            if let Value::CompiledFunction(f) = &value {
                let chunk = self.ensure_dbg_chunk_with_spans(
                    name,
                    DebugChunkSpec {
                        dbg_chunk: f.dbg_chunk,
                        instructions: f.instructions.as_ref(),
                        dbg_stmt_spans: &f.dbg_stmt_spans,
                        source_base_offset: f.dbg_source_base_offset,
                        dbg_pc_spans: &f.dbg_pc_spans,
                        dbg_stmt_marks: &f.dbg_stmt_marks,
                        dbg_local_names: &f.dbg_local_names,
                        params: &f.params,
                    },
                );
                if f.dbg_chunk != chunk {
                    let mut new_f = FunctionData::clone(f);
                    new_f.dbg_chunk = chunk;
                    value = Value::CompiledFunction(std::sync::Arc::new(new_f));
                }
            } else if let Value::Closure(c) = &value {
                let chunk = self.ensure_dbg_chunk_with_spans(
                    name,
                    DebugChunkSpec {
                        dbg_chunk: c.dbg_chunk,
                        instructions: c.instructions.as_ref(),
                        dbg_stmt_spans: &c.dbg_stmt_spans,
                        source_base_offset: c.dbg_source_base_offset,
                        dbg_pc_spans: &c.dbg_pc_spans,
                        dbg_stmt_marks: &c.dbg_stmt_marks,
                        dbg_local_names: &c.dbg_local_names,
                        params: &c.params,
                    },
                );
                if c.dbg_chunk != chunk {
                    let mut new_c = ClosureData::clone(c);
                    new_c.dbg_chunk = chunk;
                    value = Value::Closure(std::sync::Arc::new(new_c));
                }
            }
        }
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
        if self.debug_artifacts_enabled() {
            if let Value::CompiledFunction(f) = &value {
                let chunk = self.ensure_dbg_chunk_with_spans(
                    name,
                    DebugChunkSpec {
                        dbg_chunk: f.dbg_chunk,
                        instructions: f.instructions.as_ref(),
                        dbg_stmt_spans: &f.dbg_stmt_spans,
                        source_base_offset: f.dbg_source_base_offset,
                        dbg_pc_spans: &f.dbg_pc_spans,
                        dbg_stmt_marks: &f.dbg_stmt_marks,
                        dbg_local_names: &f.dbg_local_names,
                        params: &f.params,
                    },
                );
                if f.dbg_chunk != chunk {
                    let mut new_f = FunctionData::clone(f);
                    new_f.dbg_chunk = chunk;
                    value = Value::CompiledFunction(std::sync::Arc::new(new_f));
                }
            } else if let Value::Closure(c) = &value {
                let chunk = self.ensure_dbg_chunk_with_spans(
                    name,
                    DebugChunkSpec {
                        dbg_chunk: c.dbg_chunk,
                        instructions: c.instructions.as_ref(),
                        dbg_stmt_spans: &c.dbg_stmt_spans,
                        source_base_offset: c.dbg_source_base_offset,
                        dbg_pc_spans: &c.dbg_pc_spans,
                        dbg_stmt_marks: &c.dbg_stmt_marks,
                        dbg_local_names: &c.dbg_local_names,
                        params: &c.params,
                    },
                );
                if c.dbg_chunk != chunk {
                    let mut new_c = ClosureData::clone(c);
                    new_c.dbg_chunk = chunk;
                    value = Value::Closure(std::sync::Arc::new(new_c));
                }
            }
        }
        if let Some(dest) = self.global_slots.get_mut(slot) {
            *dest = value;
        }
        if self.global_slot_map.get(name).copied().is_none() {
            self.global_slot_map.insert(name.to_string(), slot);
        }
        if self.global_slot_versions.len() <= slot {
            self.global_slot_versions.resize(slot + 1, 0);
        }
        self.bump_global_slot_version(slot);
    }

    // pub(crate) fn remove_global(&mut self, name: &str) -> bool {
    //     self.sync_global_slots_if_dirty();
    //     let Some(slot) = self.global_slot_map.remove(name) else {
    //         return false;
    //     };

    //     let last_slot = self.global_slots.len().saturating_sub(1);
    //     self.global_slots.swap_remove(slot);
    //     self.global_slot_versions.swap_remove(slot);

    //     if slot != last_slot
    //         && let Some((_, moved_slot)) = self
    //             .global_slot_map
    //             .iter_mut()
    //             .find(|(_, mapped_slot)| **mapped_slot == last_slot)
    //     {
    //         *moved_slot = slot;
    //     }

    //     self.globals_dirty = true;
    //     true
    // }

    #[inline]
    pub(crate) fn set_hooks(&mut self, hooks: Option<&dyn InterpreterHook>) {
        self.hooks = hooks.map(NonNull::from);
    }
}

#[inline]
pub(crate) fn ensure_stack_len<F>(stack: &[Value], need: usize, ctx: F) -> WqResult<()>
where
    F: FnOnce() -> String,
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
    F: FnOnce() -> String,
{
    stack
        .pop()
        .ok_or_else(|| vm_err(format!("stack underflow: {}", ctx())))
}

#[inline]
pub(crate) fn pop2_stack<F>(stack: &mut Vec<Value>, ctx: F) -> WqResult<(Value, Value)>
where
    F: FnOnce() -> String,
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
    F: FnOnce() -> String,
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

    #[test]
    fn debug_artifacts_enabled_when_on_pause_set() {
        let mut vm = Vm::new(Vec::new());
        vm.set_bt_mode(false);
        vm.wqdb.enabled = false;
        vm.runtime_debug_info = false;
        assert!(!vm.debug_artifacts_enabled());

        fn dummy(_: &mut Vm) {}
        vm.wqdb.on_pause = Some(dummy);
        assert!(
            vm.debug_artifacts_enabled(),
            "debug artifacts should be enabled when on_pause is registered"
        );
    }
}
