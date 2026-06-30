use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::session::stdio::wqstderr_println;
use crate::value::func::{
    CallableExpr, ClosureData, FunctionData, LiftedCallableData, UserFunctionShape,
};
use crate::value::{Excerpt, Value};
use crate::vm::Vm;
use crate::wqdb::build::{
    apply_stmt_debug_exact_offs, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
};
use crate::wqdb::data::{
    Backtrace, ChunkId, CodeLoc, DebugChunkSpec, DebugInfo, DebugLocalsFrame, DebugProvenance,
    DebugStepHints,
};
use crate::wqdb::model::{BreakpointKind, StepMode, SymbolTrackTarget};

impl Vm {
    pub fn set_bt_mode(&mut self, flag: bool) {
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
        self.debug_info.chunk_opt(chunk).is_some_and(|meta| {
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
    pub fn script_prepare_debug(&mut self, virtual_path: &str, source: &str) {
        if !self.debug_artifacts_enabled() {
            return;
        }
        let file_id = self.debug_info.new_file(virtual_path, source);
        let len = self.instructions.len();
        let chunk = self.debug_info.new_chunk("<script>", file_id, len);
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "[wqdb]: script_prepare_debug path={virtual_path} file_id={file_id} chunk={chunk:?} instructions={len}",
            );
        }
        self.current_chunk = chunk;
    }

    pub fn set_debug_src_offset(&mut self, offs: usize) {
        self.debug_src_offset = offs;
    }

    pub fn debug_src_offset(&self) -> usize {
        self.debug_src_offset
    }

    #[inline]
    pub fn clear_last_bt(&mut self) {
        self.last_backtrace = None;
        self.last_locals_snapshot = None;
    }

    #[inline]
    pub fn take_last_bt(&mut self) -> Option<Backtrace> {
        self.last_backtrace.take()
    }

    #[inline]
    pub fn capture_bt_if_empty(&mut self) {
        if !self.debug_artifacts_enabled() {
            return;
        }
        if self.last_backtrace.is_none() {
            self.last_backtrace = Some(self.bt_frames());
        }
        if self.last_locals_snapshot.is_none() {
            self.last_locals_snapshot = Some(self.live_local_frames());
        }
    }

    fn read_frame_locals(frame: &[crate::vm::slot::Slot]) -> Vec<(usize, Value)> {
        frame
            .iter()
            .enumerate()
            .map(|(idx, slot)| (idx, slot.read()))
            .collect()
    }

    fn live_local_frames(&self) -> Vec<DebugLocalsFrame> {
        let mut frames = Vec::new();
        if let Some(current) = self.locals.last() {
            // Tail-call journal frames share the current physical locals
            for fr in self.tail_call_journal.iter().rev() {
                frames.push(DebugLocalsFrame {
                    loc: CodeLoc {
                        chunk: fr.chunk,
                        pc: fr.pc.saturating_sub(1),
                    },
                    name: fr.func_name.clone(),
                    locals: Self::read_frame_locals(current),
                });
            }
            frames.push(DebugLocalsFrame {
                loc: CodeLoc {
                    chunk: self.current_chunk,
                    pc: self.pc.saturating_sub(1),
                },
                name: self.func_name_arc_for_chunk(self.current_chunk),
                locals: Self::read_frame_locals(current),
            });
        }
        for (fr, locals) in self
            .call_stack
            .iter()
            .rev()
            .zip(self.locals.iter().rev().skip(1))
        {
            frames.push(DebugLocalsFrame {
                loc: CodeLoc {
                    chunk: fr.chunk,
                    pc: fr.pc.saturating_sub(1),
                },
                name: fr.func_name.clone(),
                locals: Self::read_frame_locals(locals),
            });
        }
        frames
    }

    fn next_stmt_in_chunk(&self, chunk: ChunkId, after_pc: usize) -> Option<CodeLoc> {
        let meta = self.debug_info.chunk(chunk);
        for pc in after_pc..meta.len {
            if meta.line_table.is_stmt(pc) {
                return Some(CodeLoc { chunk, pc });
            }
        }
        None
    }

    fn current_stmt_hint(&self) -> Option<CodeLoc> {
        let meta = self.debug_info.chunk(self.current_chunk);
        meta.line_table
            .stmt_start_pc(self.pc)
            .or_else(|| meta.line_table.stmt_start_pc(self.pc.saturating_sub(1)))
            .map(|pc| CodeLoc {
                chunk: self.current_chunk,
                pc,
            })
            .or_else(|| self.next_stmt_in_chunk(self.current_chunk, self.pc))
            .or_else(|| self.next_stmt_in_chunk(self.current_chunk, self.pc.saturating_sub(1)))
    }

    fn previous_stmt_hint(&self, current: CodeLoc) -> Option<CodeLoc> {
        let meta = self.debug_info.chunk(current.chunk);
        for pc in (0..current.pc).rev() {
            if meta.line_table.is_stmt(pc) {
                return Some(CodeLoc {
                    chunk: current.chunk,
                    pc,
                });
            }
        }
        None
    }

    fn caller_resume_hint(&self) -> Option<CodeLoc> {
        let caller = self.call_stack.last()?;
        self.next_stmt_in_chunk(caller.chunk, caller.pc.saturating_add(1))
    }

    fn step_hints(&self) -> DebugStepHints {
        let current = self.current_stmt_hint().unwrap_or(CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc.saturating_sub(1),
        });
        let previous = self.previous_stmt_hint(current);
        let next_same_frame = self.next_stmt_in_chunk(current.chunk, current.pc.saturating_add(1));
        match self.wqdb.mode() {
            StepMode::In => DebugStepHints {
                previous,
                step: next_same_frame,
                next: next_same_frame,
                finish: self.caller_resume_hint(),
            },
            StepMode::Over => DebugStepHints {
                previous,
                step: None,
                next: if self.is_at_return() {
                    self.caller_resume_hint()
                } else {
                    next_same_frame
                },
                finish: self.caller_resume_hint(),
            },
            StepMode::Out => DebugStepHints {
                previous,
                step: None,
                next: next_same_frame,
                finish: self.caller_resume_hint(),
            },
            StepMode::None => DebugStepHints {
                previous,
                step: next_same_frame,
                next: next_same_frame,
                finish: self.caller_resume_hint(),
            },
        }
    }

    pub(crate) fn func_name_arc_for_chunk(&self, id: ChunkId) -> std::sync::Arc<str> {
        self.debug_info
            .chunk_opt(id)
            .map(|m| std::sync::Arc::clone(&m.name))
            .unwrap_or_else(|| std::sync::Arc::from("<?>"))
    }

    pub fn func_name_for_chunk(&self, id: ChunkId) -> String {
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

        let mut frames = Vec::new();
        Self::append_unique_frame(
            &mut frames,
            (
                CodeLoc {
                    chunk: self.current_chunk,
                    pc: self.pc.saturating_sub(1),
                },
                self.func_name_arc_for_chunk(self.current_chunk),
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
            if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
                eprintln!("[wqdb]: ensure_dbg_chunk reuse chunk={id:?} name={name}");
            }
            let (file_id, needs_rename, has_exact_spans, has_real_spans, has_local_names) = {
                let meta = self.debug_info.chunk(id);
                (
                    meta.file_id,
                    meta.name.as_ref() != name,
                    meta.has_exact_spans,
                    meta.has_real_spans,
                    meta.local_names.is_some(),
                )
            };

            if needs_rename {
                self.debug_info.rename_chunk(id, name);
            }

            if let (Some(pc_spans), Some(stmt_marks)) =
                (dbg_pc_spans.as_ref(), dbg_stmt_marks.as_ref())
                && !has_exact_spans
            {
                let base_offs = source_base_offset;
                let (has_exact, has_real) = {
                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                    apply_stmt_debug_exact_offs(
                        table,
                        file_id,
                        pc_spans.as_ref(),
                        stmt_marks.as_ref(),
                        base_offs,
                    )
                };
                self.debug_info
                    .chunk_mut(id)
                    .note_debug_spans(has_exact, has_real);
            } else if let Some(spans) = dbg_stmt_spans.as_ref()
                && !has_real_spans
            {
                let base_offs = source_base_offset;
                let has_real = {
                    let table = &mut self.debug_info.chunk_mut(id).line_table;
                    apply_stmt_spans_exact_offs(
                        table,
                        instructions,
                        file_id,
                        spans.as_ref(),
                        base_offs,
                    )
                };
                self.debug_info
                    .chunk_mut(id)
                    .note_debug_spans(false, has_real);
            }

            if !has_local_names {
                if let Some(names) = dbg_local_names.as_ref() {
                    self.debug_info.chunk_mut(id).local_names =
                        Some(names.iter().cloned().collect());
                } else if let Some(ps) = params.as_ref() {
                    self.debug_info.chunk_mut(id).local_names = Some(ps.iter().cloned().collect());
                }
            }

            return Some(id);
        }
        let file_id = self.debug_info.chunk(self.current_chunk).file_id;
        let id = self.debug_info.new_chunk(name, file_id, instructions.len());
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "[wqdb]: ensure_dbg_chunk new name={name} file_id={file_id} instructions={} base_offset={}",
                instructions.len(),
                source_base_offset,
            );
        }

        let base_offs = source_base_offset;
        if let (Some(pc_spans), Some(stmt_marks)) = (dbg_pc_spans.as_ref(), dbg_stmt_marks.as_ref())
        {
            let (has_exact, has_real) = {
                let table = &mut self.debug_info.chunk_mut(id).line_table;
                apply_stmt_debug_exact_offs(
                    table,
                    file_id,
                    pc_spans.as_ref(),
                    stmt_marks.as_ref(),
                    base_offs,
                )
            };
            self.debug_info
                .chunk_mut(id)
                .note_debug_spans(has_exact, has_real);
        } else if let Some(spans) = dbg_stmt_spans.as_ref() {
            let has_real = {
                let table = &mut self.debug_info.chunk_mut(id).line_table;
                apply_stmt_spans_exact_offs(table, instructions, file_id, spans.as_ref(), base_offs)
            };
            self.debug_info
                .chunk_mut(id)
                .note_debug_spans(false, has_real);
        } else {
            let table = &mut self.debug_info.chunk_mut(id).line_table;
            mark_stmt_heuristic(table, instructions);
        }

        if let Some(names) = dbg_local_names.as_ref() {
            self.debug_info.chunk_mut(id).local_names = Some(names.iter().cloned().collect());
        } else if let Some(ps) = params.as_ref() {
            self.debug_info.chunk_mut(id).local_names = Some(ps.iter().cloned().collect());
        }

        Some(id)
    }

    pub fn loc(&self) -> CodeLoc {
        if let Some(bt) = self.last_backtrace.as_ref()
            && let Some((loc, _)) = bt.first()
        {
            return *loc;
        }
        if let Some(loc) = self.wqdb.pause_loc() {
            return loc;
        }
        CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc,
        }
    }

    /// The VM’s call_depth() is debugger-only and stays at 0 unless
    /// debug/backtrace mode is active
    pub(crate) fn call_depth(&self) -> usize {
        self.call_stack.len()
    }

    pub fn debug_info(&self) -> &DebugInfo {
        &self.debug_info
    }

    pub fn dbg_track_symbol(&mut self, name: &str) -> Result<Option<String>, String> {
        let current_chunk = self.loc().chunk;
        if let Some(meta) = self.debug_info.chunk_opt(current_chunk)
            && let Some(names) = &meta.local_names
            && let Some(slot) = names.iter().position(|candidate| candidate == name)
        {
            let target = SymbolTrackTarget::Local {
                chunk: current_chunk,
                slot: u16::try_from(slot).map_err(|_| "local slot out of range".to_string())?,
                name: name.to_string(),
            };
            let label = self.format_symbol_track_target(&target);
            let (tracker, added) = self.wqdb.ensure_symbol_tracker(target);
            return Ok(added.then(|| format!("tracking #{} {label}", tracker.id)));
        }

        Ok(self.dbg_track_global_symbol(name))
    }

    pub fn dbg_track_global_symbol(&mut self, name: &str) -> Option<String> {
        let target = SymbolTrackTarget::Global {
            name: name.to_string(),
        };
        let label = self.format_symbol_track_target(&target);
        let (tracker, added) = self.wqdb.ensure_symbol_tracker(target);
        added.then(|| format!("tracking #{} {label}", tracker.id))
    }

    pub fn dbg_track_local_symbol(&mut self, name: &str) -> Result<Option<String>, String> {
        let current_chunk = self.loc().chunk;
        let meta = self.debug_info.chunk(current_chunk);
        let names = meta
            .local_names
            .as_ref()
            .ok_or_else(|| "current chunk has no local symbol names".to_string())?;
        let slot = names
            .iter()
            .position(|candidate| candidate == name)
            .ok_or_else(|| format!("local symbol '{name}' not found in current chunk"))?;
        let target = SymbolTrackTarget::Local {
            chunk: current_chunk,
            slot: u16::try_from(slot).map_err(|_| "local slot out of range".to_string())?,
            name: name.to_string(),
        };
        let label = self.format_symbol_track_target(&target);
        let (tracker, added) = self.wqdb.ensure_symbol_tracker(target);
        Ok(added.then(|| format!("tracking #{} {label}", tracker.id)))
    }

    pub fn dbg_track_capture_slot(&mut self, slot: u16) -> Option<String> {
        let current_chunk = self.loc().chunk;
        let target = SymbolTrackTarget::Capture {
            chunk: current_chunk,
            slot,
            name: self
                .debug_info
                .chunk_opt(current_chunk)
                .and_then(|meta| meta.local_names.as_ref())
                .and_then(|names| names.get(usize::from(slot)))
                .cloned(),
        };
        let label = self.format_symbol_track_target(&target);
        let (tracker, added) = self.wqdb.ensure_symbol_tracker(target);
        added.then(|| format!("tracking #{} {label}", tracker.id))
    }

    pub fn dbg_symbol_trackers(&self) -> Vec<(usize, bool, String)> {
        self.wqdb
            .symbol_trackers()
            .iter()
            .map(|tracker| {
                (
                    tracker.id,
                    tracker.enabled,
                    self.format_symbol_track_target(&tracker.target),
                )
            })
            .collect()
    }

    pub fn dbg_remove_symbol_tracker(&mut self, id: usize) -> bool {
        self.wqdb.remove_symbol_tracker(id)
    }

    pub fn dbg_clear_symbol_trackers(&mut self) {
        self.wqdb.clear_symbol_trackers();
    }

    #[inline]
    pub(crate) fn symbol_trackers_enabled(&self) -> bool {
        self.wqdb.enabled && self.wqdb.has_symbol_trackers()
    }

    pub(crate) fn note_global_symbol_write(
        &mut self,
        pc: usize,
        name: &str,
        op: &'static str,
        old: Option<Value>,
        new: Value,
    ) {
        self.note_symbol_write(
            pc,
            SymbolTrackTarget::Global {
                name: name.to_string(),
            },
            op,
            old,
            new,
        );
    }

    pub(crate) fn note_local_symbol_write(
        &mut self,
        pc: usize,
        slot: u16,
        op: &'static str,
        old: Option<Value>,
        new: Value,
    ) {
        let target = SymbolTrackTarget::Local {
            chunk: self.current_chunk,
            slot,
            name: self
                .local_slot_name(usize::from(slot))
                .map(str::to_string)
                .unwrap_or_else(|| format!("loc[{slot}]")),
        };
        self.note_symbol_write(pc, target, op, old, new);
    }

    pub(crate) fn note_capture_symbol_write(
        &mut self,
        pc: usize,
        slot: u16,
        op: &'static str,
        old: Option<Value>,
        new: Value,
    ) {
        let target = SymbolTrackTarget::Capture {
            chunk: self.current_chunk,
            slot,
            name: self
                .debug_info
                .chunk_opt(self.current_chunk)
                .and_then(|meta| meta.local_names.as_ref())
                .and_then(|names| names.get(usize::from(slot)))
                .cloned(),
        };
        self.note_symbol_write(pc, target, op, old, new);
    }

    fn note_symbol_write(
        &mut self,
        pc: usize,
        target: SymbolTrackTarget,
        op: &'static str,
        old: Option<Value>,
        new: Value,
    ) {
        if !self.symbol_trackers_enabled() {
            return;
        }

        let trackers: Vec<_> = self
            .wqdb
            .symbol_trackers()
            .iter()
            .filter(|tracker| tracker.enabled && tracker.target.matches_event(&target))
            .cloned()
            .collect();
        if trackers.is_empty() {
            return;
        }

        let loc = CodeLoc {
            chunk: self.current_chunk,
            pc,
        };
        let location = self.format_symbol_track_loc(loc);
        let old_text = Self::format_symbol_track_value(old.as_ref());
        let new_text = Self::format_symbol_track_value(Some(&new));
        for tracker in trackers {
            wqstderr_println(format!(
                "[wqdb:track #{}] {} {op} at {location}: {old_text} -> {new_text}",
                tracker.id,
                self.format_symbol_track_target(&tracker.target),
            ));
        }
    }

    fn format_symbol_track_value(value: Option<&Value>) -> String {
        match value {
            Some(value) => format!("{} ({})", value.excerpt(), value.type_name()),
            None => "<unbound>".to_string(),
        }
    }

    fn format_symbol_track_loc(&self, loc: CodeLoc) -> String {
        let meta = self.debug_info.chunk(loc.chunk);
        let span = meta.line_table.context_span_at(loc.pc);
        if span.file_id != u32::MAX
            && let Some(sf) = self.debug_info.file(span.file_id)
        {
            let (line, col) = sf.line_col(span.start);
            return format!("{}:{line}:{col} in {}", sf.path, meta.name);
        }
        format!("pc {} in {}", loc.pc, meta.name)
    }

    fn format_symbol_track_target(&self, target: &SymbolTrackTarget) -> String {
        match target {
            SymbolTrackTarget::Global { name } => format!("global {name}"),
            SymbolTrackTarget::Local { chunk, slot, name } => {
                let chunk_name = self
                    .debug_info
                    .chunk_opt(*chunk)
                    .map(|meta| meta.name.as_ref())
                    .unwrap_or("?");
                format!("local {name} ({chunk_name} slot {slot})")
            }
            SymbolTrackTarget::Capture { chunk, slot, name } => {
                let chunk_name = self
                    .debug_info
                    .chunk_opt(*chunk)
                    .map(|meta| meta.name.as_ref())
                    .unwrap_or("?");
                match name {
                    Some(name) => format!("capture {name} ({chunk_name} slot {slot})"),
                    None => format!("capture slot {slot} ({chunk_name})"),
                }
            }
        }
    }

    pub fn dbg_continue(&mut self) {
        self.wqdb.clear_mode();
    }

    pub fn dbg_step_in(&mut self) {
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!("[wqdb]: dbg_step_in called at PC {}", self.pc);
        }
        self.wqdb.req_in(self.call_depth());
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!("[wqdb]: step-in mode on, will pause at next statement");
        }
    }

    pub fn dbg_step_over(&mut self) {
        // Step over: pause at the next statement encountered in the
        // current or outer frames (do not step into deeper frames).
        // Heuristic: also place a temporary breakpoint at the first
        // statement inside a forward-branch loop body (e.g. W[...]) so
        // 'next' on a loop header does not jump past the entire loop.
        self.wqdb.req_over(self.call_depth());
        let here = CodeLoc {
            chunk: self.current_chunk,
            pc: self.pc,
        };
        let meta = self.debug_info.chunk(here.chunk);
        // At a Return instruction, set up temp breaks in the caller
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc + 1..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            return;
        }

        // Add a forward-only temp break at the next stmt in this chunk
        // To guarantee progress at the last stmt of a function
        for pc in here.pc + 1..meta.len {
            if meta.line_table.is_stmt(pc) {
                self.wqdb.add_temp_break(CodeLoc {
                    chunk: here.chunk,
                    pc,
                });
                break;
            }
        }
        // If on a loop header (cond -> exit)
        // Pause at the first stmt inside the body
        let code = &self.instructions;
        // Find a nearby conditional jump with a forward target (typical loop header)
        let mut cond_pc_and_exit: Option<(usize, usize)> = None;
        for k in (here.pc.saturating_sub(16))..((here.pc + 32).min(code.len().saturating_sub(1))) {
            use crate::vm::inst::Instruction::*;
            let hit = match code.get(k) {
                Some(JumpIfFalse(t)) if *t > k + 1 => Some((k, *t)),
                Some(JumpIfGE(t)) if *t > k + 1 => Some((k, *t)),
                Some(JumpIfLEZLocal(_, t)) if *t > k + 1 => Some((k, *t)),
                _ => None,
            };
            if let Some(pair) = hit {
                cond_pc_and_exit = Some(pair);
                break;
            }
        }
        // If at a Return instruction, set up temp breaks in the caller
        // And clear step mode to continue properly
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            self.wqdb.clear_mode();
            return;
        }
        if let Some((cond_pc, exit_pc)) = cond_pc_and_exit {
            // First stmt in [cond_pc+1, exit_pc)
            for pc in (cond_pc + 1)..exit_pc {
                if meta.line_table.is_stmt(pc) {
                    self.wqdb.add_temp_break(CodeLoc {
                        chunk: here.chunk,
                        pc,
                    });
                    break;
                }
            }
        }
    }

    pub fn dbg_step_out(&mut self) {
        if self.is_at_return() {
            if !self.call_stack.is_empty() {
                let caller_frame = &self.call_stack[self.call_stack.len() - 1];
                let caller_meta = self.debug_info.chunk(caller_frame.chunk);
                // Look for the next statement after the call site
                for pc in caller_frame.pc..caller_meta.len {
                    if caller_meta.line_table.is_stmt(pc) {
                        self.wqdb.add_temp_break(CodeLoc {
                            chunk: caller_frame.chunk,
                            pc,
                        });
                        break;
                    }
                }
            }
            self.wqdb.clear_mode();
            return;
        }
        self.wqdb.req_out(self.call_depth());
        if !self.call_stack.is_empty() {
            let caller_frame = &self.call_stack[self.call_stack.len() - 1];
            let caller_meta = self.debug_info.chunk(caller_frame.chunk);
            // Look for the next statement after the call site
            for pc in caller_frame.pc..caller_meta.len {
                if caller_meta.line_table.is_stmt(pc) {
                    self.wqdb.add_temp_break(CodeLoc {
                        chunk: caller_frame.chunk,
                        pc,
                    });
                    break;
                }
            }
        }
    }

    pub fn dbg_set_break(&mut self, loc: CodeLoc) {
        self.wqdb.ensure_breakpoint(loc, BreakpointKind::Persistent);
    }

    pub fn dbg_clear_break(&mut self, loc: CodeLoc) {
        self.wqdb.breaks.remove(&loc);
    }

    pub fn dbg_toggle_break_loc(&mut self, loc: CodeLoc) -> bool {
        if let Some(bp) = self.wqdb.breaks.get_mut(&loc) {
            bp.enabled = !bp.enabled;
            bp.enabled
        } else {
            self.wqdb.ensure_breakpoint(loc, BreakpointKind::Persistent);
            true
        }
    }

    pub fn dbg_toggle_break_id(&mut self, id: usize) -> Option<bool> {
        for bp in self.wqdb.breaks.values_mut() {
            if bp.id == id {
                bp.enabled = !bp.enabled;
                return Some(bp.enabled);
            }
        }
        None
    }

    pub fn dbg_toggle_break_all(&mut self) -> bool {
        let mut any_disabled = false;
        for bp in self.wqdb.breaks.values() {
            if !bp.enabled {
                any_disabled = true;
                break;
            }
        }
        // if any is disabled, enable all. else disable all.
        let new_state = any_disabled;
        for bp in self.wqdb.breaks.values_mut() {
            bp.enabled = new_state;
        }
        new_state
    }

    pub fn dbg_breakpoints(&self) -> Vec<(usize, bool, CodeLoc)> {
        let mut bps: Vec<_> = self
            .wqdb
            .breaks
            .iter()
            .map(|(loc, bp)| (bp.id, bp.enabled, *loc))
            .collect();
        bps.sort_by_key(|(id, _, _)| *id);
        bps
    }

    // fn dbg_reset_breaks(&mut self) {
    //     self.wqdb.breaks.clear();
    // }

    pub fn bt_frames(&self) -> Vec<(CodeLoc, std::sync::Arc<str>)> {
        // Prefer a captured backtrace snapshot if present (e.g., after a crash)
        if let Some(bt) = self.last_backtrace.as_ref() {
            return bt.clone();
        }
        let mut v = Vec::new();
        v.push((
            CodeLoc {
                chunk: self.current_chunk,
                pc: self.pc.saturating_sub(1),
            },
            self.func_name_arc_for_chunk(self.current_chunk),
        ));
        // Tail-call journal frames (most recent first)
        for fr in self.tail_call_journal.iter().rev() {
            v.push((
                CodeLoc {
                    chunk: fr.chunk,
                    pc: fr.pc.saturating_sub(1),
                },
                fr.func_name.clone(),
            ));
        }
        if self.tail_call_journal_overflow {
            Self::append_unique_frame(
                &mut v,
                (
                    CodeLoc {
                        chunk: ChunkId(0),
                        pc: 0,
                    },
                    std::sync::Arc::from("(... tail calls omitted ...)"),
                ),
            );
        }
        if let Some(active) = self.current_closure_stack.last()
            && let Some(provenance) = Self::callable_provenance(active)
        {
            for frame in provenance.iter().cloned() {
                Self::append_unique_frame(&mut v, frame);
            }
        }
        for fr in self.call_stack.iter().rev() {
            Self::append_unique_frame(
                &mut v,
                (
                    CodeLoc {
                        chunk: fr.chunk,
                        pc: fr.pc.saturating_sub(1),
                    },
                    fr.func_name.clone(),
                ),
            );
        }
        v
    }

    pub fn dbg_globals(&self) -> Vec<(String, Value)> {
        self.global_slot_map
            .iter()
            .filter_map(|(name, slot)| {
                self.global_slots
                    .get(*slot)
                    .map(|val| (name.clone(), val.clone()))
            })
            .collect()
    }

    pub fn dbg_locals(&self) -> Vec<(usize, Value)> {
        if let Some(snapshot) = self.last_locals_snapshot.as_ref() {
            return snapshot
                .first()
                .map(|frame| frame.locals.clone())
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

    pub fn dbg_local_frames(&self) -> Vec<DebugLocalsFrame> {
        self.last_locals_snapshot
            .clone()
            .unwrap_or_else(|| self.live_local_frames())
    }

    pub fn dbg_step_hints(&self) -> DebugStepHints {
        self.step_hints()
    }

    fn is_at_return(&self) -> bool {
        if self.pc < self.instructions.len() {
            matches!(
                self.instructions[self.pc],
                crate::vm::inst::Instruction::Return
            )
        } else {
            false
        }
    }

    pub fn dbg_ins_at(&self, pc: usize) -> Option<String> {
        self.instructions.get(pc).map(|ins| format!("{ins:?}"))
    }
}
