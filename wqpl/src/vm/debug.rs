use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::Arc;

use crate::session::dbglog::DebugLogFlags;
use crate::value::Value;
use crate::value::func::{
    CallableExpr, ClosureData, FunctionData, LiftedCallableData, UserFunctionShape,
};
use crate::vm::Vm;
use crate::vm::inst::{InstPrettyDumper, Instruction};
use crate::wqdb::build::{
    apply_stmt_debug_exact_offs, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
};
use crate::wqdb::data::{
    ChunkId, CodeLoc, CrashFrame, CrashId, CrashSnapshot, DebugChunkSpec, DebugInfo,
    DebugLocalsFrame, DebugProvenance,
};
use crate::wqdb::model::{BreakpointKind, StepGranularity, SymbolTrackTarget};
use crate::wqdb::{
    DebugError, DebugInstruction, DebugNotification, DebugPause, DebugPauseId, DebugResume,
    Debugger, PauseEvent, PauseReason, ResumeAction, SymbolMutation, SymbolMutationKind,
    TrackResult,
};
use crate::wqerror::{WqError, WqErrorType};

type CapturedCrash = (Vec<CrashFrame>, Vec<Option<Arc<[Instruction]>>>);

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DebugBoundary {
    Continue,
    Suspended(DebugPause),
}

/// Type-erased storage for a stateful, session-owned pause handler.
pub(crate) trait PauseHandler: 'static {
    fn on_pause(&mut self, event: PauseEvent, debugger: &mut Debugger<'_>) -> ResumeAction;
}

impl<F> PauseHandler for F
where
    F: for<'vm> FnMut(PauseEvent, &mut Debugger<'vm>) -> ResumeAction + 'static,
{
    fn on_pause(&mut self, event: PauseEvent, debugger: &mut Debugger<'_>) -> ResumeAction {
        self(event, debugger)
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
        self.debug_state.note_pause(event.location);
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

    pub(crate) fn debugger_pause_before_instruction(
        &mut self,
        explicit_pause: bool,
    ) -> DebugBoundary {
        if let Some(pause) = self.pending_debug_pause.clone() {
            return DebugBoundary::Suspended(pause);
        }

        let Some(chunk) = self.current_chunk else {
            return DebugBoundary::Continue;
        };
        let location = CodeLoc { chunk, pc: self.pc };
        let reason = self.debug_state.pause_reason_at(
            &self.debug_info,
            location,
            self.call_depth(),
            Some(&self.debug_log),
        );
        let reason = reason.or_else(|| {
            explicit_pause
                .then(|| self.debug_state.explicit_pause_id(location))
                .flatten()
                .map(|id| PauseReason::ExplicitPause { id })
        });
        let Some(reason) = reason else {
            return DebugBoundary::Continue;
        };
        let event = PauseEvent { location, reason };

        if self.cooperative_execution {
            self.debug_state.note_pause(location);
            let id = DebugPauseId::new(self.next_debug_pause_id);
            self.next_debug_pause_id = self.next_debug_pause_id.saturating_add(1);
            let pause = DebugPause::new(id, event);
            self.pending_debug_pause = Some(pause.clone());
            DebugBoundary::Suspended(pause)
        } else {
            self.dispatch_pause(event);
            DebugBoundary::Continue
        }
    }

    pub(crate) fn resume_debug_pause(
        &mut self,
        pause_id: DebugPauseId,
        action: ResumeAction,
    ) -> Result<(), WqError> {
        let Some(pause) = self.pending_debug_pause.as_ref() else {
            return Err(WqError::new(WqErrorType::Vm).msg("debugger pause is not pending"));
        };
        if pause.id() != pause_id {
            return Err(WqError::new(WqErrorType::Vm).msg(format!(
                "debugger pause {} does not match pending pause {}",
                pause_id.get(),
                pause.id().get()
            )));
        }
        self.pending_debug_pause = None;
        self.apply_debug_resume(action);
        Ok(())
    }

    pub(crate) fn set_backtrace_enabled(&mut self, flag: bool) {
        self.bt_mode = flag;
    }

    pub(crate) fn resolved_debug_base_offset(&self) -> usize {
        match self.current_closure_stack.last() {
            Some(Value::CompiledFunction(f)) => f.dbg_source_base_offset,
            Some(Value::Closure(c)) => c.dbg_source_base_offset,
            _ => self.debug_src_offset,
        }
    }

    pub(crate) fn attach_debug_base_to_callable(&self, value: Value) -> Value {
        match value {
            Value::CompiledFunction(f) => {
                let base_offset = self.resolved_debug_base_offset();
                if f.dbg_source_base_offset == base_offset {
                    Value::CompiledFunction(f)
                } else {
                    let mut new_f = FunctionData::clone(&f);
                    new_f.dbg_source_base_offset = base_offset;
                    Value::CompiledFunction(std::sync::Arc::new(new_f))
                }
            }
            Value::Closure(c) => {
                let base_offset = self.resolved_debug_base_offset();
                if c.dbg_source_base_offset == base_offset {
                    Value::Closure(c)
                } else {
                    let mut new_c = ClosureData::clone(&c);
                    new_c.dbg_source_base_offset = base_offset;
                    Value::Closure(std::sync::Arc::new(new_c))
                }
            }
            Value::LiftedCallable(data) => {
                let mut new_data = LiftedCallableData::clone(&data);
                new_data.expr = self.attach_debug_base_to_callable_expr(new_data.expr);
                Value::LiftedCallable(std::sync::Arc::new(new_data))
            }
            other => other,
        }
    }

    fn attach_debug_base_to_callable_expr(&self, expr: CallableExpr) -> CallableExpr {
        match expr {
            CallableExpr::Const(value) => CallableExpr::Const(value),
            CallableExpr::Call(value) => {
                CallableExpr::Call(self.attach_debug_base_to_callable(value))
            }
            CallableExpr::Unary { op, operand } => CallableExpr::Unary {
                op,
                operand: std::sync::Arc::new(
                    self.attach_debug_base_to_callable_expr((*operand).clone()),
                ),
            },
            CallableExpr::Binary { op, left, right } => CallableExpr::Binary {
                op,
                left: std::sync::Arc::new(self.attach_debug_base_to_callable_expr((*left).clone())),
                right: std::sync::Arc::new(
                    self.attach_debug_base_to_callable_expr((*right).clone()),
                ),
            },
        }
    }

    fn stamped_debug_chunk_is_reusable(
        &self,
        chunk: ChunkId,
        name: &str,
        shape: &UserFunctionShape<'_>,
    ) -> bool {
        let needs_local_names = shape
            .dbg_local_names
            .as_ref()
            .is_some_and(|names| !names.is_empty())
            || shape
                .params
                .as_ref()
                .is_some_and(|params| !params.is_empty());
        let needs_exact_spans = shape
            .dbg_pc_spans
            .as_ref()
            .is_some_and(|spans| !spans.is_empty())
            && shape.dbg_stmt_marks.is_some();
        let needs_real_spans = shape
            .dbg_stmt_spans
            .as_ref()
            .is_some_and(|spans| !spans.is_empty())
            || shape
                .dbg_stmt_marks
                .as_ref()
                .is_some_and(|marks| !marks.is_empty());
        self.debug_info.get_chunk(chunk).is_some_and(|meta| {
            meta.name.as_ref() == name
                && (!needs_exact_spans || meta.has_exact_spans)
                && (!needs_real_spans || meta.has_real_spans)
                && (!needs_local_names || meta.local_names.is_some())
        })
    }

    pub(crate) fn stamp_user_function_debug_chunk(
        &mut self,
        value: &mut Value,
        name: &str,
        dbg_chunk: Option<ChunkId>,
    ) -> Option<ChunkId> {
        let (chunk, needs_update) = {
            let Some(shape) = value.as_user_function() else {
                return dbg_chunk;
            };
            if let Some(chunk) = dbg_chunk
                && shape.dbg_chunk == Some(chunk)
                && self.stamped_debug_chunk_is_reusable(chunk, name, &shape)
            {
                return Some(chunk);
            }
            let mut spec = shape.debug_spec();
            if dbg_chunk.is_some() {
                spec.dbg_chunk = dbg_chunk;
            }
            let chunk = self.ensure_dbg_chunk_with_spans(name, spec);
            (chunk, shape.dbg_chunk != chunk)
        };
        if needs_update {
            *value = value
                .with_user_function_dbg_chunk(chunk)
                .expect("checked user function");
        }
        chunk
    }

    /// Prepare debug info for a top-level script run in the REPL.
    /// Creates a new source file and a script chunk and selects it as current.
    pub(crate) fn script_prepare_debug(&mut self, virtual_path: &str, source: &str) {
        if !self.debug_artifacts_enabled() {
            return;
        }
        let file_id = self.debug_info.new_file(virtual_path, source);
        let len = self.instructions.len();
        let chunk = self.debug_info.new_chunk("<script>", file_id, len);
        if self.debug_log.enabled(DebugLogFlags::WQDB) {
            self.debug_log.emit_line(format!(
                "[wqdb]: script_prepare_debug path={virtual_path} file_id={file_id} chunk={chunk:?} instructions={len}",
            ));
        }
        self.current_chunk = Some(chunk);
    }

    pub(crate) fn set_debug_src_offset(&mut self, offs: usize) {
        self.debug_src_offset = offs;
    }

    #[inline]
    pub(crate) fn begin_evaluation(&mut self) {
        self.current_chunk = None;
        self.last_crash = None;
        self.runtime_debug_info = false;
        self.debug_src_offset = 0;
    }

    pub(crate) fn end_evaluation(&mut self) {
        self.current_chunk = None;
    }

    pub(crate) fn publish_crash(&mut self, crash: Option<Arc<CrashSnapshot>>) {
        self.last_crash = crash;
    }

    pub(crate) fn record_execution_failure(&mut self, error: WqError) -> WqError {
        if error.crash.is_some() {
            return error;
        }
        let mut error = crate::interpret::vanilla::trace::attach_pc_source_ctx(
            self,
            self.pc.saturating_sub(1),
            error,
        );
        if let Some(crash) = self.capture_crash() {
            error = error.with_crash(crash);
        }
        error
    }

    fn capture_crash(&mut self) -> Option<Arc<CrashSnapshot>> {
        if !self.debug_artifacts_enabled() || self.current_chunk.is_none() {
            return None;
        }
        let id = CrashId::new(self.next_crash_id);
        self.next_crash_id = self.next_crash_id.saturating_add(1);
        let (frames, instructions) = self.live_crash_frames();
        Some(std::sync::Arc::new(CrashSnapshot::new(
            id,
            frames,
            instructions,
        )))
    }

    fn read_frame_locals(frame: &[crate::vm::slot::Slot]) -> Vec<(usize, Value)> {
        frame
            .iter()
            .enumerate()
            .map(|(idx, slot)| (idx, slot.read()))
            .collect()
    }

    pub(crate) fn func_name_arc_for_chunk(&self, id: ChunkId) -> std::sync::Arc<str> {
        self.debug_info
            .get_chunk(id)
            .map(|m| std::sync::Arc::clone(&m.name))
            .unwrap_or_else(|| std::sync::Arc::from("<?>"))
    }

    pub(crate) fn func_name_for_chunk(&self, id: ChunkId) -> String {
        self.func_name_arc_for_chunk(id).to_string()
    }

    fn callable_provenance(value: &Value) -> Option<DebugProvenance> {
        match value {
            Value::CompiledFunction(f) => f.dbg_provenance.clone(),
            Value::Closure(c) => c.dbg_provenance.clone(),
            Value::LiftedCallable(c) => c.dbg_provenance.clone(),
            _ => None,
        }
    }

    fn set_callable_provenance(value: Value, provenance: DebugProvenance) -> Value {
        match value {
            Value::CompiledFunction(f) => {
                let mut new_f = FunctionData::clone(&f);
                new_f.dbg_provenance = Some(provenance);
                Value::CompiledFunction(std::sync::Arc::new(new_f))
            }
            Value::Closure(c) => {
                let mut new_c = ClosureData::clone(&c);
                new_c.dbg_provenance = Some(provenance);
                Value::Closure(std::sync::Arc::new(new_c))
            }
            Value::LiftedCallable(c) => {
                let mut new_c = LiftedCallableData::clone(&c);
                new_c.expr = Self::set_callable_expr_provenance(new_c.expr, &provenance);
                new_c.dbg_provenance = Some(provenance);
                Value::LiftedCallable(std::sync::Arc::new(new_c))
            }
            other => other,
        }
    }

    fn set_callable_expr_provenance(
        expr: CallableExpr,
        provenance: &DebugProvenance,
    ) -> CallableExpr {
        match expr {
            CallableExpr::Const(value) => CallableExpr::Const(value),
            CallableExpr::Call(value) => CallableExpr::Call(Self::set_callable_provenance(
                value,
                std::sync::Arc::clone(provenance),
            )),
            CallableExpr::Unary { op, operand } => CallableExpr::Unary {
                op,
                operand: std::sync::Arc::new(Self::set_callable_expr_provenance(
                    (*operand).clone(),
                    provenance,
                )),
            },
            CallableExpr::Binary { op, left, right } => CallableExpr::Binary {
                op,
                left: std::sync::Arc::new(Self::set_callable_expr_provenance(
                    (*left).clone(),
                    provenance,
                )),
                right: std::sync::Arc::new(Self::set_callable_expr_provenance(
                    (*right).clone(),
                    provenance,
                )),
            },
        }
    }

    fn append_unique_frame(
        frames: &mut Vec<(CodeLoc, std::sync::Arc<str>)>,
        frame: (CodeLoc, std::sync::Arc<str>),
    ) {
        if frames.last().is_none_or(|last| *last != frame) {
            frames.push(frame);
        }
    }

    fn located_crash_frame(
        &self,
        function: Arc<str>,
        location: CodeLoc,
        locals: Option<Arc<[(usize, Value)]>>,
    ) -> CrashFrame {
        let source = self
            .debug_info
            .resolve_location(location)
            .and_then(|resolved| resolved.source);
        CrashFrame::Located {
            function,
            location,
            source,
            locals,
        }
    }

    fn append_captured_frame(
        frames: &mut Vec<CrashFrame>,
        instructions: &mut Vec<Option<Arc<[crate::vm::inst::Instruction]>>>,
        frame: CrashFrame,
        frame_instructions: Option<Arc<[crate::vm::inst::Instruction]>>,
    ) {
        frames.push(frame);
        instructions.push(frame_instructions);
    }

    fn last_frame_matches(frames: &[CrashFrame], function: &Arc<str>, location: CodeLoc) -> bool {
        frames.last().is_some_and(|frame| {
            matches!(
                frame,
                CrashFrame::Located {
                    function: existing_function,
                    location: existing_location,
                    ..
                } if existing_function == function && *existing_location == location
            )
        })
    }

    pub(crate) fn attach_provenance_to_returned_callable(&self, value: Value) -> Value {
        if !self.callable_provenance_enabled() {
            return value;
        }
        if !matches!(
            value,
            Value::CompiledFunction(_) | Value::Closure(_) | Value::LiftedCallable(_)
        ) {
            return value;
        }
        let Some(current_chunk) = self.current_chunk else {
            return value;
        };

        let mut frames = Vec::new();
        Self::append_unique_frame(
            &mut frames,
            (
                CodeLoc {
                    chunk: current_chunk,
                    pc: self.pc.saturating_sub(1),
                },
                self.func_name_arc_for_chunk(current_chunk),
            ),
        );
        if let Some(active) = self.current_closure_stack.last()
            && let Some(existing) = Self::callable_provenance(active)
        {
            for frame in existing.iter().cloned() {
                Self::append_unique_frame(&mut frames, frame);
            }
        }
        if let Some(existing) = Self::callable_provenance(&value) {
            for frame in existing.iter().cloned() {
                Self::append_unique_frame(&mut frames, frame);
            }
        }
        Self::set_callable_provenance(value, std::sync::Arc::from(frames))
    }

    /// Ensures a dbg chunk (with spans if present).
    /// No-op when debugging is off or chunk exists.
    #[inline]
    pub(crate) fn ensure_dbg_chunk_with_spans(
        &mut self,
        name: &str,
        spec: DebugChunkSpec<'_>,
    ) -> Option<ChunkId> {
        let DebugChunkSpec {
            dbg_chunk,
            instructions,
            dbg_stmt_spans,
            source_base_offset,
            dbg_pc_spans,
            dbg_stmt_marks,
            dbg_local_names,
            params,
        } = spec;
        if !self.debug_artifacts_enabled() {
            return dbg_chunk;
        }

        if let Some(id) = dbg_chunk {
            if self.debug_log.enabled(DebugLogFlags::WQDB) {
                self.debug_log.emit_line(format!(
                    "[wqdb]: ensure_dbg_chunk reuse chunk={id:?} name={name}"
                ));
            }
            let (file_id, needs_rename, has_exact_spans, has_real_spans, has_local_names) = {
                let meta = self.debug_info.expect_chunk(id);
                (
                    meta.file_id,
                    meta.name.as_ref() != name,
                    meta.has_exact_spans,
                    meta.has_real_spans,
                    meta.local_names.is_some(),
                )
            };

            if needs_rename {
                self.debug_info.rename_function_chunk(id, name);
            }

            if let (Some(pc_spans), Some(stmt_marks)) =
                (dbg_pc_spans.as_ref(), dbg_stmt_marks.as_ref())
                && !has_exact_spans
            {
                let base_offs = source_base_offset;
                let (has_exact, has_real) = {
                    let table = &mut self.debug_info.expect_chunk_mut(id).line_table;
                    apply_stmt_debug_exact_offs(
                        table,
                        file_id,
                        pc_spans.as_ref(),
                        stmt_marks.as_ref(),
                        base_offs,
                        Some(&self.debug_log),
                    )
                };
                self.debug_info
                    .expect_chunk_mut(id)
                    .note_debug_spans(has_exact, has_real);
            } else if let Some(spans) = dbg_stmt_spans.as_ref()
                && !has_real_spans
            {
                let base_offs = source_base_offset;
                let has_real = {
                    let table = &mut self.debug_info.expect_chunk_mut(id).line_table;
                    apply_stmt_spans_exact_offs(
                        table,
                        instructions,
                        file_id,
                        spans.as_ref(),
                        base_offs,
                        Some(&self.debug_log),
                    )
                };
                self.debug_info
                    .expect_chunk_mut(id)
                    .note_debug_spans(false, has_real);
            }

            if !has_local_names {
                if let Some(names) = dbg_local_names.as_ref() {
                    self.debug_info.expect_chunk_mut(id).local_names =
                        Some(names.iter().cloned().collect());
                } else if let Some(ps) = params.as_ref() {
                    self.debug_info.expect_chunk_mut(id).local_names =
                        Some(ps.iter().cloned().collect());
                }
            }

            return Some(id);
        }
        let file_id = self.debug_info.expect_chunk(self.current_chunk?).file_id;
        let id = self.debug_info.new_function_chunk(
            Some(std::sync::Arc::from(name)),
            file_id,
            instructions.len(),
        );
        if self.debug_log.enabled(DebugLogFlags::WQDB) {
            self.debug_log.emit_line(format!(
                "[wqdb]: ensure_dbg_chunk new name={name} file_id={file_id} instructions={} base_offset={}",
                instructions.len(),
                source_base_offset,
            ));
        }

        let base_offs = source_base_offset;
        if let (Some(pc_spans), Some(stmt_marks)) = (dbg_pc_spans.as_ref(), dbg_stmt_marks.as_ref())
        {
            let (has_exact, has_real) = {
                let table = &mut self.debug_info.expect_chunk_mut(id).line_table;
                apply_stmt_debug_exact_offs(
                    table,
                    file_id,
                    pc_spans.as_ref(),
                    stmt_marks.as_ref(),
                    base_offs,
                    Some(&self.debug_log),
                )
            };
            self.debug_info
                .expect_chunk_mut(id)
                .note_debug_spans(has_exact, has_real);
        } else if let Some(spans) = dbg_stmt_spans.as_ref() {
            let has_real = {
                let table = &mut self.debug_info.expect_chunk_mut(id).line_table;
                apply_stmt_spans_exact_offs(
                    table,
                    instructions,
                    file_id,
                    spans.as_ref(),
                    base_offs,
                    Some(&self.debug_log),
                )
            };
            self.debug_info
                .expect_chunk_mut(id)
                .note_debug_spans(false, has_real);
        } else {
            let table = &mut self.debug_info.expect_chunk_mut(id).line_table;
            mark_stmt_heuristic(table, instructions, Some(&self.debug_log));
        }

        if let Some(names) = dbg_local_names.as_ref() {
            self.debug_info.expect_chunk_mut(id).local_names =
                Some(names.iter().cloned().collect());
        } else if let Some(ps) = params.as_ref() {
            self.debug_info.expect_chunk_mut(id).local_names = Some(ps.iter().cloned().collect());
        }

        Some(id)
    }

    pub(crate) fn loc(&self) -> Option<CodeLoc> {
        if let Some(crash) = self.last_crash.as_ref()
            && let Some(location) = crash.frames().first().and_then(CrashFrame::location)
        {
            return Some(location);
        }
        if let Some(loc) = self.debug_state.pause_loc() {
            return Some(loc);
        }
        Some(CodeLoc {
            chunk: self.current_chunk?,
            pc: self.pc,
        })
    }

    /// The VM's call depth is debugger-only and includes journaled tail-call
    /// frames so step-over and step-out follow logical calls.
    pub(crate) fn call_depth(&self) -> usize {
        self.call_stack
            .iter()
            .fold(self.tail_call_depth, |depth, frame| {
                depth.saturating_add(frame.tail_depth)
            })
            .saturating_add(self.call_stack.len())
    }

    pub(crate) fn debug_info(&self) -> &DebugInfo {
        &self.debug_info
    }

    pub(crate) fn dbg_track_symbol(&mut self, name: &str) -> Result<TrackResult, DebugError> {
        let Some(current_chunk) = self.loc().map(|location| location.chunk) else {
            return Ok(self.dbg_track_global_symbol(name));
        };
        if let Some(meta) = self.debug_info.get_chunk(current_chunk)
            && let Some(names) = &meta.local_names
            && let Some(slot) = names.iter().position(|candidate| candidate == name)
        {
            let target = SymbolTrackTarget::Local {
                chunk: current_chunk,
                slot: u16::try_from(slot).map_err(|_| DebugError::LocalSlotOutOfRange { slot })?,
                name: name.to_string(),
            };
            let (tracker, added) = self.debug_state.ensure_symbol_tracker(target);
            return Ok(if added {
                TrackResult::Added(tracker.clone())
            } else {
                TrackResult::Existing(tracker.clone())
            });
        }

        Ok(self.dbg_track_global_symbol(name))
    }

    pub(crate) fn dbg_track_global_symbol(&mut self, name: &str) -> TrackResult {
        let target = SymbolTrackTarget::Global {
            name: name.to_string(),
        };
        let (tracker, added) = self.debug_state.ensure_symbol_tracker(target);
        if added {
            TrackResult::Added(tracker.clone())
        } else {
            TrackResult::Existing(tracker.clone())
        }
    }

    pub(crate) fn dbg_track_local_symbol(&mut self, name: &str) -> Result<TrackResult, DebugError> {
        let current_chunk = self
            .loc()
            .map(|location| location.chunk)
            .ok_or(DebugError::NoCurrentLocation)?;
        let meta = self.debug_info.expect_chunk(current_chunk);
        let names = meta
            .local_names
            .as_ref()
            .ok_or(DebugError::LocalNamesUnavailable)?;
        let slot = names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| DebugError::LocalNotFound {
                name: name.to_string(),
            })?;
        let target = SymbolTrackTarget::Local {
            chunk: current_chunk,
            slot: u16::try_from(slot).map_err(|_| DebugError::LocalSlotOutOfRange { slot })?,
            name: name.to_string(),
        };
        let (tracker, added) = self.debug_state.ensure_symbol_tracker(target);
        Ok(if added {
            TrackResult::Added(tracker.clone())
        } else {
            TrackResult::Existing(tracker.clone())
        })
    }

    pub(crate) fn dbg_track_capture_slot(&mut self, slot: u16) -> Result<TrackResult, DebugError> {
        let current_chunk = self
            .loc()
            .map(|location| location.chunk)
            .ok_or(DebugError::NoCurrentLocation)?;
        let target = SymbolTrackTarget::Capture {
            chunk: current_chunk,
            slot,
            name: self
                .debug_info
                .get_chunk(current_chunk)
                .and_then(|meta| meta.local_names.as_ref())
                .and_then(|names| names.get(usize::from(slot)))
                .cloned(),
        };
        let (tracker, added) = self.debug_state.ensure_symbol_tracker(target);
        Ok(if added {
            TrackResult::Added(tracker.clone())
        } else {
            TrackResult::Existing(tracker.clone())
        })
    }

    pub(crate) fn dbg_symbol_trackers(&self) -> Vec<crate::wqdb::model::SymbolTracker> {
        self.debug_state.symbol_trackers().to_vec()
    }

    pub(crate) fn dbg_remove_symbol_tracker(&mut self, id: usize) -> bool {
        self.debug_state.remove_symbol_tracker(id)
    }

    pub(crate) fn dbg_clear_symbol_trackers(&mut self) {
        self.debug_state.clear_symbol_trackers();
    }

    #[inline]
    pub(crate) fn symbol_trackers_enabled(&self) -> bool {
        self.debug_state.is_enabled() && self.debug_state.has_symbol_trackers()
    }

    pub(crate) fn note_global_symbol_write(
        &mut self,
        pc: usize,
        name: &str,
        operation: SymbolMutationKind,
        old: Option<Value>,
        new: Value,
    ) {
        self.note_symbol_write(
            pc,
            SymbolTrackTarget::Global {
                name: name.to_string(),
            },
            operation,
            old,
            new,
        );
    }

    pub(crate) fn note_local_symbol_write(
        &mut self,
        pc: usize,
        slot: u16,
        operation: SymbolMutationKind,
        old: Option<Value>,
        new: Value,
    ) {
        let Some(chunk) = self.current_chunk else {
            return;
        };
        let target = SymbolTrackTarget::Local {
            chunk,
            slot,
            name: self
                .local_slot_name(usize::from(slot))
                .map(str::to_string)
                .unwrap_or_else(|| format!("loc[{slot}]")),
        };
        self.note_symbol_write(pc, target, operation, old, new);
    }

    pub(crate) fn note_capture_symbol_write(
        &mut self,
        pc: usize,
        slot: u16,
        operation: SymbolMutationKind,
        old: Option<Value>,
        new: Value,
    ) {
        let Some(chunk) = self.current_chunk else {
            return;
        };
        let target = SymbolTrackTarget::Capture {
            chunk,
            slot,
            name: self
                .debug_info
                .get_chunk(chunk)
                .and_then(|meta| meta.local_names.as_ref())
                .and_then(|names| names.get(usize::from(slot)))
                .cloned(),
        };
        self.note_symbol_write(pc, target, operation, old, new);
    }

    fn note_symbol_write(
        &mut self,
        pc: usize,
        target: SymbolTrackTarget,
        operation: SymbolMutationKind,
        old: Option<Value>,
        new: Value,
    ) {
        if !self.symbol_trackers_enabled() {
            return;
        }

        let trackers: Vec<_> = self
            .debug_state
            .symbol_trackers()
            .iter()
            .filter(|tracker| tracker.enabled && tracker.target.matches_event(&target))
            .cloned()
            .collect();
        if trackers.is_empty() {
            return;
        }

        let Some(chunk) = self.current_chunk else {
            return;
        };
        let location = CodeLoc { chunk, pc };
        for tracker in trackers {
            self.debug_state
                .push_notification(DebugNotification::SymbolChanged(SymbolMutation {
                    tracker_id: tracker.id,
                    target: tracker.target,
                    operation,
                    location,
                    old_value: old.clone(),
                    new_value: new.clone(),
                }));
        }
    }

    pub(crate) fn dbg_continue(&mut self) {
        self.debug_state.clear_mode();
    }

    pub(crate) fn dbg_step_granularity(&self) -> StepGranularity {
        self.debug_state.granularity()
    }

    pub(crate) fn dbg_set_step_granularity(&mut self, granularity: StepGranularity) {
        self.debug_state.set_granularity(granularity);
    }

    pub(crate) fn dbg_step_in(&mut self) {
        if self.debug_log.enabled(DebugLogFlags::WQDB) {
            self.debug_log
                .emit_line(format!("[wqdb]: dbg_step_in called at PC {}", self.pc));
        }
        self.debug_state.req_in(self.call_depth());
        if self.debug_log.enabled(DebugLogFlags::WQDB) {
            self.debug_log
                .emit_line("[wqdb]: step-in mode on, will pause at next statement");
        }
    }

    pub(crate) fn dbg_step_over(&mut self) {
        self.debug_state.req_over(self.call_depth());
    }

    pub(crate) fn dbg_step_out(&mut self) {
        self.debug_state.req_out(self.call_depth());
    }

    pub(crate) fn dbg_set_break(&mut self, loc: CodeLoc) -> usize {
        self.debug_state
            .ensure_breakpoint(loc, BreakpointKind::Persistent)
            .id
    }

    pub(crate) fn dbg_clear_break(&mut self, loc: CodeLoc) {
        self.debug_state.clear_breakpoint(loc);
    }

    pub(crate) fn dbg_toggle_break_loc(&mut self, loc: CodeLoc) -> bool {
        self.debug_state.toggle_breakpoint_at(loc)
    }

    pub(crate) fn dbg_toggle_break_id(&mut self, id: usize) -> Option<bool> {
        self.debug_state.toggle_breakpoint_by_id(id)
    }

    pub(crate) fn dbg_toggle_break_all(&mut self) -> bool {
        self.debug_state.toggle_all_breakpoints()
    }

    pub(crate) fn dbg_breakpoints(&self) -> Vec<(usize, bool, CodeLoc)> {
        self.debug_state.breakpoints()
    }

    pub(crate) fn crash_frames(&self) -> Vec<CrashFrame> {
        if let Some(crash) = self.last_crash.as_ref() {
            return crash.frames().to_vec();
        }

        self.live_crash_frames().0
    }

    fn live_crash_frames(&self) -> CapturedCrash {
        let Some(current_chunk) = self.current_chunk else {
            return (Vec::new(), Vec::new());
        };
        let mut frames = Vec::new();
        let mut frame_instructions = Vec::new();
        Self::append_captured_frame(
            &mut frames,
            &mut frame_instructions,
            self.located_crash_frame(
                self.func_name_arc_for_chunk(current_chunk),
                CodeLoc {
                    chunk: current_chunk,
                    pc: self.pc.saturating_sub(1),
                },
                self.locals
                    .last()
                    .map(|locals| std::sync::Arc::from(Self::read_frame_locals(locals))),
            ),
            Some(Arc::clone(&self.instructions)),
        );
        for fr in self.tail_call_journal.iter().rev() {
            Self::append_captured_frame(
                &mut frames,
                &mut frame_instructions,
                self.located_crash_frame(
                    fr.func_name.clone(),
                    CodeLoc {
                        chunk: fr.chunk,
                        pc: fr.pc.saturating_sub(1),
                    },
                    None,
                ),
                Some(Arc::clone(&fr.instructions)),
            );
        }
        if self.tail_call_journal.overflowed() {
            Self::append_captured_frame(
                &mut frames,
                &mut frame_instructions,
                CrashFrame::TailCallsOmitted,
                None,
            );
        }
        if let Some(active) = self.current_closure_stack.last()
            && let Some(provenance) = Self::callable_provenance(active)
        {
            for (index, (location, function)) in provenance.iter().enumerate() {
                if index == 0 && Self::last_frame_matches(&frames, function, *location) {
                    continue;
                }
                Self::append_captured_frame(
                    &mut frames,
                    &mut frame_instructions,
                    self.located_crash_frame(Arc::clone(function), *location, None),
                    None,
                );
            }
        }
        for fr in self.call_stack.iter().rev() {
            Self::append_captured_frame(
                &mut frames,
                &mut frame_instructions,
                self.located_crash_frame(
                    fr.func_name.clone(),
                    CodeLoc {
                        chunk: fr.chunk,
                        pc: fr.pc.saturating_sub(1),
                    },
                    fr.locals.clone(),
                ),
                Some(Arc::clone(&fr.instructions)),
            );
            for tail in fr.tail_frames.iter().rev() {
                Self::append_captured_frame(
                    &mut frames,
                    &mut frame_instructions,
                    self.located_crash_frame(
                        Arc::clone(&tail.func_name),
                        CodeLoc {
                            chunk: tail.chunk,
                            pc: tail.pc.saturating_sub(1),
                        },
                        None,
                    ),
                    Some(Arc::clone(&tail.instructions)),
                );
            }
            if fr.tail_frames_overflowed {
                Self::append_captured_frame(
                    &mut frames,
                    &mut frame_instructions,
                    CrashFrame::TailCallsOmitted,
                    None,
                );
            }
        }
        (frames, frame_instructions)
    }

    pub(crate) fn dbg_globals(&self) -> Vec<(String, Value)> {
        self.global_slot_map
            .iter()
            .filter_map(|(name, slot)| {
                self.global_slots
                    .get(*slot)
                    .map(|val| (name.clone(), val.clone()))
            })
            .collect()
    }

    pub(crate) fn dbg_locals(&self) -> Vec<(usize, Value)> {
        if let Some(snapshot) = self.last_crash.as_ref() {
            return snapshot
                .frames()
                .first()
                .and_then(CrashFrame::locals)
                .map(<[(usize, Value)]>::to_vec)
                .unwrap_or_default();
        }
        if let Some(frame) = self.locals.last() {
            frame
                .iter()
                .enumerate()
                .map(|(idx, slot)| (idx, slot.read()))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn dbg_frame_locals(&self, frame_index: usize) -> Option<DebugLocalsFrame> {
        let frames = self.crash_frames();
        let CrashFrame::Located {
            function,
            location,
            locals: Some(locals),
            ..
        } = frames.get(frame_index)?
        else {
            return None;
        };
        Some(DebugLocalsFrame {
            loc: *location,
            name: Arc::clone(function),
            locals: locals.as_ref().to_vec(),
        })
    }

    pub(crate) fn dbg_ins_at(&self, pc: usize) -> Option<DebugInstruction> {
        let location = self.loc();
        let local_names = location
            .and_then(|location| self.debug_info.get_chunk(location.chunk))
            .and_then(|meta| meta.local_names.as_deref());
        let instructions = self
            .last_crash
            .as_ref()
            .and_then(|crash| crash.instructions(0))
            .unwrap_or(&self.instructions);
        InstPrettyDumper::describe_at(instructions, pc, local_names)
    }
}
