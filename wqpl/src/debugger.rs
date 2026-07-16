use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use crate::session::stdio::WqIoError;
use crate::style::ColorMode;
use crate::value::Value;
use crate::vm::Vm;
use crate::wqdb::data::{ChunkId, CodeLoc, DebugInfo, DebugLocalsFrame};
use crate::wqdb::model::{SourceBreakpoint, StepGranularity, StopHook};

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
pub enum DebugResume {
    #[default]
    Continue,
    StepIn,
    StepOver,
    StepOut,
}

/// Type-erased storage for a stateful, session-owned pause handler.
pub(crate) trait PauseHandler: 'static {
    fn on_pause(&mut self, event: PauseEvent, debugger: &mut Debugger<'_>) -> DebugResume;
}

impl<F> PauseHandler for F
where
    F: for<'vm> FnMut(PauseEvent, &mut Debugger<'vm>) -> DebugResume + 'static,
{
    fn on_pause(&mut self, event: PauseEvent, debugger: &mut Debugger<'_>) -> DebugResume {
        self(event, debugger)
    }
}

impl<'vm> Debugger<'vm> {
    pub(crate) fn new(vm: &'vm mut Vm) -> Self {
        Self { vm }
    }

    pub fn location(&self) -> CodeLoc {
        self.vm.loc()
    }

    pub fn debug_info(&self) -> &DebugInfo {
        self.vm.debug_info()
    }

    pub fn function_name(&self, chunk: ChunkId) -> String {
        self.vm.func_name_for_chunk(chunk)
    }

    pub fn write_stderr_line(&self, text: &str) -> Result<(), WqIoError> {
        self.vm.write_stderr_line(text)
    }

    pub fn color_mode(&self) -> ColorMode {
        self.vm.stderr_color_mode()
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
            .wqdb
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
            .wqdb
            .replace_source_breakpoints(&self.vm.debug_info, source_path, lines)
    }

    /// Take source breakpoints which became resolvable since the last call.
    pub fn take_resolved_source_breakpoints(&mut self) -> Vec<SourceBreakpoint> {
        self.vm.wqdb.take_resolved_source_breakpoints()
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

    pub fn backtrace(&self) -> Vec<(CodeLoc, Arc<str>)> {
        self.vm.bt_frames()
    }

    pub fn globals(&self) -> Vec<(String, Value)> {
        self.vm.dbg_globals()
    }

    pub fn locals(&self) -> Vec<(usize, Value)> {
        self.vm.dbg_locals()
    }

    pub fn local_frames(&self) -> Vec<DebugLocalsFrame> {
        self.vm.dbg_local_frames()
    }

    pub fn instruction_at(&self, pc: usize) -> Option<String> {
        self.vm.dbg_ins_at(pc)
    }

    pub fn instruction_at_with_color_mode(
        &self,
        pc: usize,
        color_mode: ColorMode,
    ) -> Option<String> {
        self.vm.dbg_ins_at_with_color_mode(pc, color_mode)
    }

    pub fn track_symbol(&mut self, name: &str) -> Result<Option<String>, String> {
        self.vm.dbg_track_symbol(name)
    }

    pub fn track_global_symbol(&mut self, name: &str) -> Option<String> {
        self.vm.dbg_track_global_symbol(name)
    }

    pub fn track_local_symbol(&mut self, name: &str) -> Result<Option<String>, String> {
        self.vm.dbg_track_local_symbol(name)
    }

    pub fn track_capture_slot(&mut self, slot: u16) -> Option<String> {
        self.vm.dbg_track_capture_slot(slot)
    }

    pub fn symbol_trackers(&self) -> Vec<(usize, bool, String)> {
        self.vm.dbg_symbol_trackers()
    }

    pub fn remove_symbol_tracker(&mut self, id: usize) -> bool {
        self.vm.dbg_remove_symbol_tracker(id)
    }

    pub fn clear_symbol_trackers(&mut self) {
        self.vm.dbg_clear_symbol_trackers();
    }

    pub fn take_batch_commands(&mut self) -> Vec<String> {
        self.vm.wqdb.take_batch_commands()
    }

    pub fn stop_hook_commands(&self) -> Vec<(usize, String)> {
        self.vm.wqdb.stop_hook_commands()
    }

    pub fn stop_hooks(&self) -> Vec<StopHook> {
        self.vm.wqdb.stop_hooks().to_vec()
    }

    pub fn add_stop_hook(&mut self, command: String) -> StopHook {
        self.vm.wqdb.add_stop_hook(command).clone()
    }

    pub fn remove_stop_hook(&mut self, id: usize) -> bool {
        self.vm.wqdb.remove_stop_hook(id)
    }

    pub fn clear_stop_hooks(&mut self) {
        self.vm.wqdb.clear_stop_hooks();
    }

    pub fn apply_resume(&mut self, action: DebugResume) {
        self.vm.apply_debug_resume(action);
    }
}

impl Vm {
    pub(crate) fn set_pause_handler<F>(&mut self, handler: F)
    where
        F: for<'vm> FnMut(PauseEvent, &mut Debugger<'vm>) -> DebugResume + 'static,
    {
        self.pause_handler = Some(Box::new(handler));
    }

    pub(crate) fn clear_pause_handler(&mut self) {
        self.pause_handler = None;
    }

    pub(crate) fn dispatch_pause(&mut self, event: PauseEvent) {
        let Some(mut handler) = self.pause_handler.take() else {
            self.apply_debug_resume(DebugResume::Continue);
            return;
        };

        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut debugger = Debugger::new(self);
            handler.on_pause(event, &mut debugger)
        }));
        self.pause_handler = Some(handler);

        match result {
            Ok(action) => self.apply_debug_resume(action),
            Err(payload) => resume_unwind(payload),
        }
    }

    pub(crate) fn apply_debug_resume(&mut self, action: DebugResume) {
        match action {
            DebugResume::Continue => self.dbg_continue(),
            DebugResume::StepIn => self.dbg_step_in(),
            DebugResume::StepOver => self.dbg_step_over(),
            DebugResume::StepOut => self.dbg_step_out(),
        }
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
            assert_eq!(debugger.location(), event.location);
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
        vm.wqdb.set_enabled(true);
        Debugger::new(&mut vm).set_temporary_breakpoint(location);

        assert_eq!(
            vm.wqdb
                .pause_reason_at(&DebugInfo::default(), location, 1, None),
            Some(PauseReason::TemporaryBreakpoint)
        );
    }
}
