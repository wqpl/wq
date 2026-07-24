pub(crate) mod build;
pub(crate) mod data;
pub(crate) mod model;
pub(crate) mod state;

pub use data::{
    ChunkId, ChunkMeta, CodeLoc, CrashFrame, CrashId, CrashSnapshot, DebugInfo, DebugLocalsFrame,
    LineTable, ResolvedCodeLoc, SourceFile, SourceLocation, Span,
};
pub use model::{SourceBreakpoint, StepGranularity, SymbolTrackTarget, SymbolTracker};

use crate::value::Value;
use crate::vm::Vm;

/// Constrained access to the state needed by debugger frontends.
pub struct Debugger<'vm> {
    vm: &'vm mut Vm,
}

/// Why execution stopped before a debugger handler was called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PauseEvent {
    pub location: CodeLoc,
    pub reason: PauseReason,
}

/// The runtime condition that caused a debugger stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseReason {
    Entry,
    Step,
    Breakpoint { id: usize },
    TemporaryBreakpoint,
    ExplicitPause { id: usize },
}

/// How execution should proceed after a debugger stop.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ResumeAction {
    #[default]
    Continue,
    StepIn,
    StepOver,
    StepOut,
}

pub type DebugResume = ResumeAction;

/// Broad VM instruction category for frontend-specific presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionClass {
    Load,
    Store,
    Call,
    Jump,
    Stack,
    Operator,
    Indexing,
    Construct,
    Try,
}

/// Renderer-independent description of one VM instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugInstruction {
    pub pc: usize,
    pub opcode: String,
    /// Exact plain-text operand suffix, including delimiters.
    pub operands: String,
    pub annotations: Vec<String>,
    pub class: InstructionClass,
    pub is_special: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DebugPauseId(u64);

impl DebugPauseId {
    pub fn get(self) -> u64 {
        self.0
    }

    pub const fn from_u64(id: u64) -> Self {
        Self(id)
    }

    pub(crate) fn new(id: u64) -> Self {
        Self(id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugPause {
    id: DebugPauseId,
    event: PauseEvent,
}

impl DebugPause {
    pub fn id(&self) -> DebugPauseId {
        self.id
    }

    pub fn event(&self) -> PauseEvent {
        self.event
    }

    pub(crate) fn new(id: DebugPauseId, event: PauseEvent) -> Self {
        Self { id, event }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymbolMutationKind {
    Store,
    IndexAssign,
    Pop,
    Remove,
    Insert,
    InsertAt,
}

impl SymbolMutationKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Store => "store",
            Self::IndexAssign => "index-assign",
            Self::Pop => "pop",
            Self::Remove => "remove",
            Self::Insert => "insert",
            Self::InsertAt => "insert-at",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SymbolMutation {
    pub tracker_id: usize,
    pub target: SymbolTrackTarget,
    pub operation: SymbolMutationKind,
    pub location: CodeLoc,
    pub old_value: Option<Value>,
    pub new_value: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugNotification {
    SymbolChanged(SymbolMutation),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackResult {
    Added(SymbolTracker),
    Existing(SymbolTracker),
}

impl TrackResult {
    pub fn tracker(&self) -> &SymbolTracker {
        match self {
            Self::Added(tracker) | Self::Existing(tracker) => tracker,
        }
    }

    pub fn was_added(&self) -> bool {
        matches!(self, Self::Added(_))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugError {
    NoCurrentLocation,
    LocalNamesUnavailable,
    LocalNotFound { name: String },
    LocalSlotOutOfRange { slot: usize },
}

impl std::fmt::Display for DebugError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCurrentLocation => f.write_str("no current debug location"),
            Self::LocalNamesUnavailable => {
                f.write_str("current function has no local variable names")
            }
            Self::LocalNotFound { name } => {
                write!(
                    f,
                    "local variable '{name}' was not found in the current function"
                )
            }
            Self::LocalSlotOutOfRange { slot } => {
                write!(f, "local slot {slot} is out of range")
            }
        }
    }
}

impl std::error::Error for DebugError {}

impl<'vm> Debugger<'vm> {
    pub(crate) fn new(vm: &'vm mut Vm) -> Self {
        Self { vm }
    }

    pub fn location(&self) -> Option<CodeLoc> {
        self.vm.loc()
    }

    pub fn debug_info(&self) -> &DebugInfo {
        self.vm.debug_info()
    }

    pub fn resolve_location(&self, location: CodeLoc) -> Option<ResolvedCodeLoc> {
        self.vm.debug_info().resolve_location(location)
    }

    pub fn function_names(&self) -> Vec<String> {
        self.vm
            .debug_info()
            .function_names()
            .map(str::to_string)
            .collect()
    }

    pub fn local_names(&self, chunk: ChunkId) -> Option<&[String]> {
        self.vm
            .debug_info()
            .get_chunk(chunk)
            .and_then(|metadata| metadata.local_names.as_deref())
    }

    pub fn function_name(&self, chunk: ChunkId) -> String {
        self.vm.func_name_for_chunk(chunk)
    }

    pub fn step_granularity(&self) -> StepGranularity {
        self.vm.dbg_step_granularity()
    }

    pub fn set_step_granularity(&mut self, granularity: StepGranularity) {
        self.vm.dbg_set_step_granularity(granularity);
    }

    pub fn set_breakpoint(&mut self, location: CodeLoc) -> usize {
        self.vm.dbg_set_break(location)
    }

    /// Set a one-shot breakpoint that is cleared at the next debugger stop.
    pub fn set_temporary_breakpoint(&mut self, location: CodeLoc) {
        self.vm
            .debug_state
            .add_temp_break(location, Some(&self.vm.debug_log));
    }

    pub fn clear_breakpoint(&mut self, location: CodeLoc) {
        self.vm.dbg_clear_break(location);
    }

    /// Replace the host-managed breakpoints for one source.
    ///
    /// Requests for lines which have not been compiled remain pending and are
    /// resolved as later script regions register their debug information.
    pub fn set_source_breakpoints(
        &mut self,
        source_path: &str,
        lines: &[usize],
    ) -> Vec<SourceBreakpoint> {
        self.vm
            .debug_state
            .replace_source_breakpoints(&self.vm.debug_info, source_path, lines)
    }

    /// Take source breakpoints which became resolvable since the last call.
    pub fn take_resolved_source_breakpoints(&mut self) -> Vec<SourceBreakpoint> {
        self.vm.debug_state.take_resolved_source_breakpoints()
    }

    pub fn toggle_breakpoint_at(&mut self, location: CodeLoc) -> bool {
        self.vm.dbg_toggle_break_loc(location)
    }

    pub fn toggle_breakpoint_by_id(&mut self, id: usize) -> Option<bool> {
        self.vm.dbg_toggle_break_id(id)
    }

    pub fn toggle_all_breakpoints(&mut self) -> bool {
        self.vm.dbg_toggle_break_all()
    }

    pub fn breakpoints(&self) -> Vec<(usize, bool, CodeLoc)> {
        self.vm.dbg_breakpoints()
    }

    pub fn backtrace(&self) -> Vec<CrashFrame> {
        self.vm.crash_frames()
    }

    pub fn globals(&self) -> Vec<(String, Value)> {
        self.vm.dbg_globals()
    }

    pub fn locals(&self) -> Vec<(usize, Value)> {
        self.vm.dbg_locals()
    }

    pub fn frame_locals(&self, frame: usize) -> Option<DebugLocalsFrame> {
        self.vm.dbg_frame_locals(frame)
    }

    pub fn instruction_at(&self, pc: usize) -> Option<DebugInstruction> {
        self.vm.dbg_ins_at(pc)
    }

    pub fn track_symbol(&mut self, name: &str) -> Result<TrackResult, DebugError> {
        self.vm.dbg_track_symbol(name)
    }

    pub fn track_global_symbol(&mut self, name: &str) -> TrackResult {
        self.vm.dbg_track_global_symbol(name)
    }

    pub fn track_local_symbol(&mut self, name: &str) -> Result<TrackResult, DebugError> {
        self.vm.dbg_track_local_symbol(name)
    }

    pub fn track_capture_slot(&mut self, slot: u16) -> Result<TrackResult, DebugError> {
        self.vm.dbg_track_capture_slot(slot)
    }

    pub fn symbol_trackers(&self) -> Vec<SymbolTracker> {
        self.vm.dbg_symbol_trackers()
    }

    pub fn take_notifications(&mut self) -> Vec<DebugNotification> {
        self.vm.debug_state.take_notifications()
    }

    pub fn remove_symbol_tracker(&mut self, id: usize) -> bool {
        self.vm.dbg_remove_symbol_tracker(id)
    }

    pub fn clear_symbol_trackers(&mut self) {
        self.vm.dbg_clear_symbol_trackers();
    }

    pub fn apply_resume(&mut self, action: DebugResume) {
        self.vm.apply_debug_resume(action);
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn test_event() -> PauseEvent {
        PauseEvent {
            location: CodeLoc {
                chunk: ChunkId(0),
                pc: 0,
            },
            reason: PauseReason::Entry,
        }
    }

    #[test]
    fn capturing_pause_handler_receives_typed_event() {
        let calls = Arc::new(AtomicUsize::new(0));
        let captured_calls = Arc::clone(&calls);
        let mut vm = Vm::new(Vec::new());
        vm.set_pause_handler(move |event, debugger| {
            assert_eq!(event, test_event());
            assert_eq!(debugger.location(), Some(event.location));
            captured_calls.fetch_add(1, Ordering::SeqCst);
            DebugResume::Continue
        });

        vm.dispatch_pause(test_event());

        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn panicking_pause_handler_is_restored_before_unwinding() {
        let calls = Arc::new(AtomicUsize::new(0));
        let captured_calls = Arc::clone(&calls);
        let mut vm = Vm::new(Vec::new());
        vm.set_pause_handler(move |_, _| {
            let call = captured_calls.fetch_add(1, Ordering::SeqCst);
            if call == 0 {
                panic!("test pause handler panic");
            }
            DebugResume::Continue
        });

        let first = catch_unwind(AssertUnwindSafe(|| vm.dispatch_pause(test_event())));
        assert!(first.is_err());

        vm.dispatch_pause(test_event());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn temporary_breakpoint_reports_a_distinct_pause_reason() {
        let location = CodeLoc {
            chunk: ChunkId(3),
            pc: 7,
        };
        let mut vm = Vm::new(Vec::new());
        vm.debug_state.set_enabled(true);
        Debugger::new(&mut vm).set_temporary_breakpoint(location);

        assert_eq!(
            vm.debug_state
                .pause_reason_at(&DebugInfo::default(), location, 1, None),
            Some(PauseReason::TemporaryBreakpoint)
        );
    }
}
