pub mod call;
pub(crate) mod debug;
pub mod inst;
mod owned_const;
pub(crate) mod pure;
mod slot;
pub(crate) use slot::Slot;
pub(crate) mod trace;

use std::collections::VecDeque;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ahash::AHashMap;

use crate::builtins::{BuiltinPreset, Builtins};
use crate::debugger::PauseHandler;
use crate::interpret::{Interpreter, InterpreterHook, InterpreterKind};
use crate::session::dbglog::{DebugLog, DebugLogFlags};
use crate::session::stdio::{RuntimeIo, WqIoError};
use crate::style::ColorMode;
use crate::value::cell::ValueCell;
use crate::value::rng::RngState;
use crate::value::{Value, WqResult};
use crate::vm::call::ResolvedCallable;
use crate::vm::inst::Instruction;
use crate::vm::owned_const::extract_owned_consts;
use crate::vm::trace::TraceRecord;
use crate::wqdb::Wqdb;
use crate::wqdb::data::{ChunkId, CrashSnapshot, DebugInfo};
use crate::wqerror::{WqError, WqErrorType};

pub(crate) type GlobalMap = crate::session::Bindings;
pub type GlobalSlotMap = AHashMap<String, usize>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HaltReason {
    Status(i32),
    Interrupted,
}

pub(crate) struct Vm {
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) owned_consts: Vec<Option<Value>>,
    pub(crate) pc: usize,
    pub(crate) stack: Vec<Value>,
    /// Global slots (stable indices) for fast access
    pub(crate) global_slots: Vec<Value>,
    pub(crate) global_slot_map: GlobalSlotMap,
    pub(crate) builtins: Builtins,
    pub(crate) builtins_preset: BuiltinPreset,
    pub(crate) default_rng: RngState,
    pub(crate) argv: Arc<[String]>,
    pub(crate) halt_reason: Option<HaltReason>,
    pub(crate) interrupt_requested: Arc<AtomicBool>,
    pub(crate) runtime_io: RuntimeIo,
    pub(crate) debug_log: DebugLog,
    pub(crate) color_mode: ColorMode,
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
    /// Suspended caller execution states for resumable user-function calls.
    pub(crate) execution_frames: Vec<ExecutionFrame>,
    // args_scratch: Vec<Value>,
    /// Tail-call journal for backtrace when TCE is active.
    pub(crate) tail_call_journal: TailCallJournal,
    /// Uncapped logical tail-call depth for debugger stepping.
    pub(crate) tail_call_depth: usize,

    /// Maximum physical call depth before raising Recursion error.
    pub(crate) max_call_depth: usize,

    // Debugging
    pub(crate) wqdb: Wqdb,
    pub(crate) debug_info: DebugInfo,
    pub(crate) pause_handler: Option<Box<dyn PauseHandler>>,
    pub(crate) current_chunk: Option<ChunkId>,
    pub(crate) call_stack: Vec<CallFrame>,
    /// Lightweight backtrace mode: build minimal debug info for frames on error
    pub(crate) bt_mode: bool,
    /// Per-run debug metadata required by runtime features like `@d`.
    pub(crate) runtime_debug_info: bool,
    /// Base byte offset into current source file for this execution (for loader
    /// slices)
    pub(crate) debug_src_offset: usize,
    pub(crate) last_crash: Option<Arc<CrashSnapshot>>,
    pub(crate) next_crash_id: u64,
    pub(crate) hooks: Option<NonNull<dyn InterpreterHook>>,
    pub(crate) try_stack: Vec<TryFrame>,
    pub(crate) returned: bool,
    /// The interpreter kind to use for nested calls.
    pub(crate) interpreter_kind: InterpreterKind,
    /// Named-argument metadata set by SetupNamedCall, consumed by next call.
    pub(crate) pending_named_meta: Option<Arc<crate::vm::inst::NamedArgMeta>>,
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
    pub(crate) pending_trace_probe: Option<usize>,
}

#[derive(Clone)]
pub(crate) struct TailFrame {
    pub chunk: ChunkId,
    pub pc: usize,
    pub func_name: std::sync::Arc<str>,
    pub instructions: Arc<[Instruction]>,
}

pub(crate) struct CallFrame {
    pub chunk: ChunkId,
    pub pc: usize,
    pub func_name: std::sync::Arc<str>,
    pub instructions: Arc<[Instruction]>,
    pub locals: Option<Arc<[(usize, Value)]>>,
    pub tail_frames: Vec<TailFrame>,
    pub tail_frames_overflowed: bool,
    pub tail_depth: usize,
}

pub(crate) struct ExecutionFrame {
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) pc: usize,
    pub(crate) stack: Vec<Value>,
    pub(crate) inline_cache: Vec<InlineCache>,
    pub(crate) tail_call_journal: TailCallJournal,
    pub(crate) tail_call_depth: usize,
    pub(crate) pushed_debug_frame: bool,
    pub(crate) pending_trace_probe: Option<usize>,
}

const TAIL_CALL_JOURNAL_CAP: usize = 128;

pub(crate) struct TailCallJournal {
    frames: VecDeque<TailFrame>,
    overflowed: bool,
}

impl Default for TailCallJournal {
    fn default() -> Self {
        Self {
            frames: VecDeque::with_capacity(TAIL_CALL_JOURNAL_CAP),
            overflowed: false,
        }
    }
}

impl TailCallJournal {
    fn push(&mut self, frame: TailFrame) {
        if self.frames.len() == TAIL_CALL_JOURNAL_CAP {
            self.frames.pop_front();
            self.overflowed = true;
        }
        self.frames.push_back(frame);
    }

    pub(crate) fn clear(&mut self) {
        self.frames.clear();
        self.overflowed = false;
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = &TailFrame> {
        self.frames.iter()
    }

    pub(crate) fn snapshot(&self) -> Vec<TailFrame> {
        self.frames.iter().cloned().collect()
    }

    pub(crate) fn overflowed(&self) -> bool {
        self.overflowed
    }
}

pub(crate) struct TryFrame {
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) locals_depth: usize,
    pub(crate) end_pc: usize,
    pub(crate) stack_start: usize,
    pub(crate) saved_pending_named_meta: Option<Arc<crate::vm::inst::NamedArgMeta>>,
    pub(crate) saved_trace_depth: u32,
    pub(crate) saved_trace_bases_len: usize,
    pub(crate) saved_trace_buf_len: usize,
}

#[derive(Clone, Default)]
pub(crate) struct InlineCache {
    pub(crate) version: u64,
    pub(crate) call_target: Option<ResolvedCallable>,
    pub(crate) named_layout: Option<Arc<call::NamedArgLayout>>,
    pub(crate) slot: Option<usize>,
    pub(crate) slot_b: Option<usize>,
}

pub(crate) struct PreparedInstructions {
    instructions: Vec<Instruction>,
    owned_consts: Vec<Option<Value>>,
}

impl PreparedInstructions {
    pub(crate) fn new(instructions: Vec<Instruction>) -> Self {
        Self {
            instructions,
            owned_consts: Vec::new(),
        }
    }

    pub(crate) fn with_owned_const_extraction(mut instructions: Vec<Instruction>) -> Self {
        let owned_consts = extract_owned_consts(&mut instructions);
        Self {
            instructions,
            owned_consts,
        }
    }

    pub(crate) fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    fn into_parts(self) -> (Vec<Instruction>, Vec<Option<Value>>) {
        (self.instructions, self.owned_consts)
    }
}

impl Vm {
    pub(crate) fn new(instructions: Vec<Instruction>) -> Self {
        Self::from_prepared_instructions(PreparedInstructions::new(instructions))
    }

    pub(crate) fn from_prepared_instructions(prepared: PreparedInstructions) -> Self {
        let (instructions, owned_consts) = prepared.into_parts();
        let len = instructions.len();
        let runtime_io = RuntimeIo::default();
        let debug_log = DebugLog::new(DebugLogFlags::empty(), runtime_io.stderr_output());
        Vm {
            instructions: Arc::<[Instruction]>::from(instructions),
            owned_consts,
            pc: 0,
            stack: Vec::with_capacity(256),
            global_slots: Vec::new(),
            global_slot_map: AHashMap::new(),
            builtins: Builtins::new(),
            builtins_preset: BuiltinPreset::DEFAULT,
            default_rng: RngState::from_entropy(),
            argv: Arc::from([]),
            halt_reason: None,
            interrupt_requested: Arc::new(AtomicBool::new(false)),
            runtime_io,
            debug_log,
            color_mode: ColorMode::Auto,
            locals: Vec::new(),
            captures: Vec::new(),
            inline_cache: vec![InlineCache::default(); len],
            cache_pool: AHashMap::new(),
            locals_pool: AHashMap::new(),
            stack_pool: Vec::new(),
            current_closure_stack: Vec::new(),
            execution_frames: Vec::new(),
            // args_scratch: Vec::new(),
            tail_call_journal: TailCallJournal::default(),
            tail_call_depth: 0,
            max_call_depth: if cfg!(debug_assertions) { 64 } else { 1024 },
            wqdb: Wqdb::default(),
            debug_info: DebugInfo::default(),
            pause_handler: None,
            current_chunk: None,
            call_stack: Vec::new(),
            bt_mode: false,
            runtime_debug_info: false,

            debug_src_offset: 0,
            last_crash: None,
            next_crash_id: 0,
            hooks: None,
            try_stack: Vec::new(),
            returned: false,
            interpreter_kind: InterpreterKind::Vanilla,
            pending_named_meta: None,
            trace_depth: 0,
            trace_buf: Vec::new(),
            trace_bases: Vec::new(),
            pending_trace_probe: None,
        }
    }

    /// Replace instructions and reset execution state.
    pub(crate) fn reset_with_prepared_instructions(&mut self, prepared: PreparedInstructions) {
        let (instructions, owned_consts) = prepared.into_parts();
        self.owned_consts = owned_consts;
        self.instructions = Arc::<[Instruction]>::from(instructions);
        self.pc = 0;
        self.stack.clear();
        self.locals.clear();
        self.captures.clear();
        self.inline_cache = vec![InlineCache::default(); self.instructions.len()];
        self.current_closure_stack.clear();
        self.execution_frames.clear();
        self.hooks = None;
        self.try_stack.clear();
        self.returned = false;
        self.pending_named_meta = None;
        // Ensure no stale frames leak
        self.call_stack.clear();
        self.tail_call_journal.clear();
        self.tail_call_depth = 0;
        self.trace_depth = 0;
        self.trace_buf.clear();
        self.trace_bases.clear();
        self.pending_trace_probe = None;
        // Keep debug_src_offset as set by session for current run
    }

    /// Reset all global variable state (for session reset).
    pub(crate) fn reset_globals(&mut self) {
        self.global_slots.clear();
        self.global_slot_map.clear();
        self.debug_info.clear_function_names();
    }

    /// Build a snapshot of globals from slots.
    pub(crate) fn global_env(&self) -> GlobalMap {
        let mut map = GlobalMap::default();
        for (name, slot) in self.global_slot_map.iter() {
            if let Some(val) = self.global_slots.get(*slot) {
                map.insert(name.clone(), val.clone());
            }
        }
        map
    }

    pub(crate) fn write_stdout(&self, text: &str) -> Result<(), WqIoError> {
        self.runtime_io.write_stdout(text)
    }

    pub(crate) fn write_stdout_line(&self, text: &str) -> Result<(), WqIoError> {
        self.runtime_io.write_stdout_line(text)
    }

    pub(crate) fn write_stderr(&self, text: &str) -> Result<(), WqIoError> {
        self.runtime_io.write_stderr(text)
    }

    pub(crate) fn write_stderr_line(&self, text: &str) -> Result<(), WqIoError> {
        self.runtime_io.write_stderr_line(text)
    }

    pub(crate) fn stdout_color_mode(&self) -> ColorMode {
        self.color_mode
            .resolve(self.runtime_io.stdout_is_terminal())
    }

    pub(crate) fn stdout_terminal_size(&self) -> Option<(usize, usize)> {
        self.runtime_io.stdout_terminal_size()
    }

    pub(crate) fn stderr_color_mode(&self) -> ColorMode {
        self.color_mode
            .resolve(self.runtime_io.stderr_is_terminal())
    }

    pub(crate) fn stderr_is_terminal(&self) -> bool {
        self.runtime_io.stderr_is_terminal()
    }

    pub(crate) fn read_line(&self, prompt: &str) -> Result<String, WqIoError> {
        self.runtime_io.read_line(prompt)
    }
}

impl Vm {
    // pub fn run(&mut self) -> WqResult<Value> {
    //     let mut interpreter = DefaultInterpreter;
    //     self.run_with_interpreter(&mut interpreter)
    // }

    pub(crate) fn run_with_interpreter<I: Interpreter + ?Sized>(
        &mut self,
        interpreter: &mut I,
    ) -> WqResult<Value> {
        let limit = self.instructions.len();
        interpreter.interpret(self, limit)
    }
}

impl Vm {
    #[inline]
    pub(crate) fn debug_mapping_enabled(&self) -> bool {
        self.runtime_debug_info || self.wqdb.is_enabled() || self.bt_mode
    }

    #[inline]
    pub(crate) fn callable_provenance_enabled(&self) -> bool {
        self.debug_mapping_enabled()
    }

    /// Normally returns `true` because `Session` enables backtrace mode by
    /// default. This is unrelated to `debug_log_flags`, which only controls
    /// logging and does not affect backtraces or debug artifacts.
    #[inline]
    pub(crate) fn debug_artifacts_enabled(&self) -> bool {
        self.debug_mapping_enabled()
    }

    pub(crate) fn set_runtime_debug_info(&mut self, flag: bool) {
        self.runtime_debug_info = flag;
    }

    #[inline]
    pub(crate) fn poll_interrupt(&mut self) {
        if self.interrupt_requested.swap(false, Ordering::AcqRel) {
            self.halt_reason.get_or_insert(HaltReason::Interrupted);
        }
    }

    pub(crate) fn is_halted(&self) -> bool {
        self.halt_reason.is_some()
    }

    pub(crate) fn halt_status(&self) -> Option<i32> {
        match self.halt_reason {
            Some(HaltReason::Status(status)) => Some(status),
            Some(HaltReason::Interrupted) | None => None,
        }
    }

    pub(crate) fn take_halt_status(&mut self) -> Option<i32> {
        let status = self.halt_status()?;
        self.halt_reason = None;
        Some(status)
    }

    pub(crate) fn take_interrupt(&mut self) -> bool {
        let pending = self.interrupt_requested.swap(false, Ordering::AcqRel);
        match self.halt_reason {
            Some(HaltReason::Interrupted) => {
                self.halt_reason = None;
                true
            }
            Some(HaltReason::Status(_)) => false,
            None => pending,
        }
    }

    pub(crate) fn request_halt(&mut self, status: i32) {
        self.halt_reason = Some(HaltReason::Status(status));
    }

    #[inline]
    pub(crate) fn push_tail_call_frame(&mut self, frame: TailFrame) {
        self.tail_call_depth = self.tail_call_depth.saturating_add(1);
        self.tail_call_journal.push(frame);
    }

    pub(crate) fn current_chunk_id(&self) -> Option<ChunkId> {
        self.current_chunk
    }

    pub(crate) fn expect_current_chunk(&self) -> ChunkId {
        self.current_chunk
            .expect("executing debug-mapped code must have a current chunk")
    }

    pub(crate) fn local_slot_name(&self, slot: usize) -> Option<&str> {
        self.debug_info
            .get_chunk(self.current_chunk?)?
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
        Some(result)
    }

    pub(crate) fn assign_global_and_slot(&mut self, name: &str, mut value: Value) -> usize {
        if value.as_user_function().is_none() {
            self.debug_info.remove_function_name(name);
        } else if self.debug_artifacts_enabled() {
            let _ = self.stamp_user_function_debug_chunk(&mut value, name, None);
        }
        let slot = match self.global_slot_map.get(name).copied() {
            Some(slot) => slot,
            None => {
                let slot = self.global_slots.len();
                self.global_slot_map.insert(name.to_string(), slot);
                self.global_slots.push(Value::empty_list());
                slot
            }
        };
        if let Some(dest) = self.global_slots.get_mut(slot) {
            *dest = value;
        }
        slot
    }

    pub(crate) fn assign_global_at_slot(&mut self, name: &str, slot: usize, mut value: Value) {
        if value.as_user_function().is_none() {
            self.debug_info.remove_function_name(name);
        } else if self.debug_artifacts_enabled() {
            let _ = self.stamp_user_function_debug_chunk(&mut value, name, None);
        }
        if let Some(dest) = self.global_slots.get_mut(slot) {
            *dest = value;
        }
        if self.global_slot_map.get(name).copied().is_none() {
            self.global_slot_map.insert(name.to_string(), slot);
        }
    }

    // pub(crate) fn remove_global(&mut self, name: &str) -> bool {
    //     self.sync_global_slots_if_dirty();
    //     let Some(slot) = self.global_slot_map.remove(name) else {
    //         return false;
    //     };

    //     let last_slot = self.global_slots.len().saturating_sub(1);
    //     self.global_slots.swap_remove(slot);
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
pub(crate) fn call_err(msg: impl Into<String>) -> WqError {
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
    use std::sync::Arc;

    use super::*;
    use crate::debugger::DebugResume;

    #[test]
    fn new_keeps_constants_inline_by_default() {
        let vm = Vm::new(vec![Instruction::load_const(Value::IntList(Arc::new(
            vec![1, 2, 3],
        )))]);

        assert!(vm.owned_consts.is_empty());
        assert!(matches!(vm.instructions[0], Instruction::LoadConst(_)));
    }

    #[test]
    fn prepared_instruction_reset_clears_transient_call_state() {
        let mut vm = Vm::new(Vec::new());
        vm.captures.push(Arc::<[ValueCell]>::from([]));
        vm.pending_named_meta = Some(Arc::new(crate::vm::inst::NamedArgMeta {
            pos_count: 0,
            named: Box::new([]),
        }));

        vm.reset_with_prepared_instructions(PreparedInstructions::new(Vec::new()));

        assert!(vm.captures.is_empty());
        assert!(vm.pending_named_meta.is_none());
    }

    #[test]
    fn opt_in_preparation_extracts_owned_consts() {
        let vm = Vm::from_prepared_instructions(PreparedInstructions::with_owned_const_extraction(
            vec![Instruction::load_const(Value::IntList(Arc::new(vec![
                1, 2, 3,
            ])))],
        ));

        assert_eq!(vm.owned_consts.len(), 1);
        assert!(matches!(vm.instructions[0], Instruction::LoadOwnedConst(0)));
    }

    #[test]
    fn idle_pause_callback_does_not_enable_debug_artifacts() {
        let mut vm = Vm::new(Vec::new());
        vm.set_backtrace_enabled(false);
        vm.wqdb.set_enabled(false);
        vm.runtime_debug_info = false;
        assert!(!vm.debug_artifacts_enabled());

        vm.set_pause_handler(|_, _| DebugResume::Continue);
        assert!(
            !vm.debug_artifacts_enabled(),
            "an installed pause callback is only a hook, not an active debug-artifact request"
        );
    }

    #[test]
    fn callable_provenance_stays_enabled_for_bt_mapping() {
        let mut vm = Vm::new(Vec::new());
        vm.set_backtrace_enabled(false);
        vm.wqdb.set_enabled(false);
        vm.runtime_debug_info = false;
        assert!(!vm.debug_mapping_enabled());
        assert!(!vm.callable_provenance_enabled());

        vm.set_backtrace_enabled(true);
        assert!(vm.debug_mapping_enabled());
        assert!(vm.callable_provenance_enabled());
    }

    #[test]
    fn symbol_trackers_are_inert_when_wqdb_disabled() {
        let mut vm = Vm::new(Vec::new());
        assert!(!vm.symbol_trackers_enabled());

        vm.dbg_track_global_symbol("x");
        assert!(!vm.symbol_trackers_enabled());

        vm.wqdb.set_enabled(true);
        assert!(vm.symbol_trackers_enabled());

        vm.wqdb.set_enabled(false);
        assert!(!vm.symbol_trackers_enabled());
    }

    #[test]
    fn capture_backtrace_is_inert_when_debug_artifacts_are_disabled() {
        let mut vm = Vm::new(Vec::new());
        vm.set_backtrace_enabled(false);
        vm.wqdb.set_enabled(false);
        vm.runtime_debug_info = false;

        let error = vm.record_execution_failure(WqError::new(WqErrorType::Raise));

        assert!(error.crash.is_none());
        assert!(vm.last_crash.is_none());
    }

    #[test]
    fn tail_call_journal_keeps_recent_frames_in_ring_order() {
        let mut journal = TailCallJournal::default();
        for pc in 0..TAIL_CALL_JOURNAL_CAP + 3 {
            journal.push(TailFrame {
                chunk: ChunkId(0),
                pc,
                func_name: Arc::from("f"),
                instructions: Arc::from([]),
            });
        }

        assert_eq!(journal.frames.len(), TAIL_CALL_JOURNAL_CAP);
        assert_eq!(journal.frames.front().map(|frame| frame.pc), Some(3));
        assert_eq!(
            journal.frames.back().map(|frame| frame.pc),
            Some(TAIL_CALL_JOURNAL_CAP + 2)
        );
        assert!(journal.overflowed());
    }

    #[test]
    fn crash_snapshot_marks_omitted_tail_calls_without_a_fake_location() {
        let mut vm = Vm::new(Vec::new());
        vm.set_backtrace_enabled(true);
        let file = vm.debug_info.new_file("tail.wq", "1/0");
        let chunk = vm
            .debug_info
            .new_chunk("current", file, TAIL_CALL_JOURNAL_CAP + 2);
        vm.current_chunk = Some(chunk);
        for pc in 0..=TAIL_CALL_JOURNAL_CAP {
            vm.push_tail_call_frame(TailFrame {
                chunk,
                pc: pc + 1,
                func_name: Arc::from(format!("tail-{pc}")),
                instructions: Arc::from([]),
            });
        }

        let error = vm.record_execution_failure(WqError::new(WqErrorType::Raise));
        let omitted = error
            .crash
            .as_ref()
            .expect("crash snapshot")
            .frames()
            .iter()
            .find(|frame| matches!(frame, crate::wqdb::data::CrashFrame::TailCallsOmitted))
            .expect("tail overflow marker");

        assert!(omitted.location().is_none());
    }
}
