use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use num_bigint::BigInt;
use smallvec::SmallVec;

use crate::ast::{BinaryOperator, binary_op_display, unary_op_display};
use crate::interpret::{Interpreter, InterpreterHook, NO_OP_HOOK};
use crate::range::{make_range, make_range_from_next, range_alloc_len};
use crate::session::dbglog::DebugLogFlags;
use crate::value::cmp::eval_cmp_chain;
use crate::value::func::ClosureData;
use crate::value::unpack::extract_path as extract_unpack_path;
use crate::value::{Excerpt, Value, WqResult, eval_binary, eval_bool_op, eval_unary};
use crate::vm::debug::DebugBoundary;
use crate::vm::inst::{BinaryOpData, Capture, ClosurePayload, CmpBranchData, Instruction, Operand};
use crate::vm::trace::TraceRecord;
use crate::vm::{TryFrame, Vm, ensure_stack_len, last_clone_stack, pop1_stack, pop2_stack};
use crate::wqdb::build::{
    apply_stmt_debug_exact_offs, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
};
use crate::wqdb::data::ChunkId;
use crate::wqdb::{DebugPause, SymbolMutationKind};
use crate::wqerror::{Requirement, WqError, WqErrorType};

mod call;
mod mutate;
mod operand;
mod target;
pub(crate) mod trace;

use call::*;
use mutate::*;
use operand::*;
use target::*;
use trace::*;

pub(crate) struct VanillaInterpreter;
const NAME: &str = "vanilla";

pub(crate) type Sv4 = SmallVec<[Value; 4]>;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InterpretPoll {
    Ready(Value),
    Yielded { work_units: usize },
    AwaitingInput { request_id: u64, prompt: String },
    Paused(DebugPause),
}

struct InterpretControl {
    work_budget: Option<usize>,
    work_units: usize,
    stop_call_depth: Option<usize>,
}

impl InterpretControl {
    fn synchronous() -> Self {
        Self {
            work_budget: None,
            work_units: 0,
            stop_call_depth: None,
        }
    }

    fn sliced(work_budget: usize) -> Self {
        Self {
            work_budget: Some(work_budget.max(1)),
            work_units: 0,
            stop_call_depth: None,
        }
    }

    fn until_call_depth(stop_call_depth: usize) -> Self {
        Self {
            work_budget: None,
            work_units: 0,
            stop_call_depth: Some(stop_call_depth),
        }
    }

    fn budget_exhausted(&self) -> bool {
        self.work_budget
            .is_some_and(|budget| self.work_units >= budget)
    }
}

impl Interpreter for VanillaInterpreter {
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        let mut control = InterpretControl::synchronous();
        match self.interpret_with_try_stack(vm, limit, &mut control)? {
            InterpretPoll::Ready(value) => Ok(value),
            InterpretPoll::Yielded { .. } => unreachable!("synchronous interpreter yielded"),
            InterpretPoll::AwaitingInput { .. } => {
                unreachable!("synchronous interpreter requested cooperative input")
            }
            InterpretPoll::Paused(_) => {
                unreachable!("synchronous interpreter suspended for debugger")
            }
        }
    }
}

impl VanillaInterpreter {
    pub(crate) fn interpret_slice(
        &mut self,
        vm: &mut Vm,
        limit: usize,
        work_budget: usize,
    ) -> WqResult<InterpretPoll> {
        let mut control = InterpretControl::sliced(work_budget);
        let cooperative_execution = std::mem::replace(&mut vm.cooperative_execution, true);
        let result = self.interpret_with_try_stack(vm, limit, &mut control);
        vm.cooperative_execution = cooperative_execution;
        result
    }

    pub(crate) fn interpret_until_call_depth(
        &mut self,
        vm: &mut Vm,
        stop_call_depth: usize,
    ) -> WqResult<Value> {
        let limit = vm.instructions.len();
        let mut control = InterpretControl::until_call_depth(stop_call_depth);
        match self.interpret_with_try_stack(vm, limit, &mut control)? {
            InterpretPoll::Ready(value) => Ok(value),
            InterpretPoll::Yielded { .. } => {
                unreachable!("unlimited nested interpreter yielded")
            }
            InterpretPoll::AwaitingInput { .. } => {
                unreachable!("nested interpreter requested cooperative input")
            }
            InterpretPoll::Paused(_) => {
                unreachable!("nested interpreter suspended for debugger")
            }
        }
    }

    fn interpret_with_try_stack(
        &mut self,
        vm: &mut Vm,
        limit: usize,
        control: &mut InterpretControl,
    ) -> WqResult<InterpretPoll> {
        loop {
            match self.interpret_inner(vm, limit, control) {
                Ok(InterpretPoll::Ready(value)) => {
                    return Ok(InterpretPoll::Ready(value));
                }
                Ok(
                    poll @ (InterpretPoll::Yielded { .. }
                    | InterpretPoll::AwaitingInput { .. }
                    | InterpretPoll::Paused(_)),
                ) => {
                    return Ok(poll);
                }
                Err(err) => {
                    let mut err = vm.record_execution_failure(err);
                    vm.pending_trace_probe = None;
                    loop {
                        if Self::catch_current_try_error(vm, &err) {
                            break;
                        }
                        Self::discard_current_try_frames(vm);
                        if vm.is_builtin_callback_boundary() {
                            err = vm.decorate_builtin_callback_error(err);
                        }
                        if vm.capture_builtin_callback_error(&err) {
                            break;
                        }
                        if !vm.unwind_user_call() {
                            return Err(err);
                        }
                        vm.pending_trace_probe = None;
                        if control
                            .stop_call_depth
                            .is_some_and(|depth| vm.physical_call_depth() == depth)
                        {
                            return Err(err);
                        }
                    }
                }
            }
        }
    }

    fn interpret_inner(
        &mut self,
        vm: &mut Vm,
        limit: usize,
        control: &mut InterpretControl,
    ) -> WqResult<InterpretPoll> {
        let limit = if vm.physical_call_depth() == 0 {
            limit
        } else {
            vm.instructions.len()
        };
        if limit > vm.instructions.len() {
            return Err(vm_err(format!("limit out of bounds: {limit}")));
        }
        let hooks: &dyn InterpreterHook = match vm.hooks {
            Some(ptr) => unsafe { ptr.as_ref() },
            None => &NO_OP_HOOK,
        };
        let mut instructions = Arc::clone(&vm.instructions);
        let mut limit = limit;
        let debugger_checks_enabled = vm.debugger_checks_enabled();
        'exec: loop {
            if !Arc::ptr_eq(&instructions, &vm.instructions) {
                instructions = Arc::clone(&vm.instructions);
                limit = instructions.len();
            }
            while vm.pc < limit || vm.builtin_frame_at_current_depth() {
                if let Some(error) = vm.pending_host_error.take() {
                    return Err(error);
                }
                if let Some(request) = vm.pending_input_request() {
                    return Ok(InterpretPoll::AwaitingInput {
                        request_id: request.id,
                        prompt: request.prompt.clone(),
                    });
                }
                if control.budget_exhausted() {
                    return Ok(InterpretPoll::Yielded {
                        work_units: control.work_units,
                    });
                }
                vm.poll_interrupt();
                if vm.is_halted() {
                    break;
                }
                if vm.builtin_frame_is_runnable() {
                    control.work_units += 1;
                    vm.step_builtin_frame()?;
                    continue 'exec;
                }
                if Self::finish_try_boundary(vm) {
                    continue;
                }
                // Record a probe for the previously executed interesting
                // instruction.  We record *here* (one iteration late) so that
                // call instructions which `continue 'exec` after a cache hit
                // still get probed: marking is done before dispatch below.
                if vm.trace_depth > 0
                    && let Some(prev) = vm.pending_trace_probe.take()
                {
                    record_trace_probe(vm, prev);
                }
                let explicit_pause = matches!(instructions.get(vm.pc), Some(Instruction::Pause));
                if debugger_checks_enabled || explicit_pause {
                    match vm.debugger_pause_before_instruction(explicit_pause) {
                        DebugBoundary::Continue => {}
                        DebugBoundary::Suspended(pause) => {
                            return Ok(InterpretPoll::Paused(pause));
                        }
                    }
                    vm.poll_interrupt();
                    if vm.is_halted() {
                        break;
                    }
                }
                let idx = vm.pc;
                vm.pc += 1;
                control.work_units += 1;
                let op = &instructions[idx];
                // Mark the trace probe before dispatch. Some call arms continue after a
                // synchronous push, so the next loop iteration handles the flush.
                if vm.trace_depth > 0 && op.is_trace_interesting() {
                    vm.pending_trace_probe = Some(idx);
                }
                hooks.before_instruction(vm, idx, op);
                match op {
                    Instruction::LoadConst(v) => {
                        let val = (**v).clone();
                        vm.stack.push(if vm.debug_artifacts_enabled() {
                            vm.attach_debug_base_to_callable(val)
                        } else {
                            val
                        });
                    }
                    Instruction::LoadOwnedConst(slot) => {
                        let val = vm
                            .owned_consts
                            .get_mut(*slot)
                            .and_then(Option::take)
                            .ok_or_else(|| vm_err("owned constant has already been loaded"))?;
                        vm.stack.push(if vm.debug_artifacts_enabled() {
                            vm.attach_debug_base_to_callable(val)
                        } else {
                            val
                        });
                    }
                    Instruction::LoadVar(name) => {
                        let cache = &vm.inline_cache[idx];
                        if let Some(slot) = cache.slot
                            && let Some(val) = vm.global_slot_value(slot)
                        {
                            vm.stack.push(val.clone());
                            hooks.on_load_var_cache_hit(&|| true);
                            continue;
                        }

                        hooks.on_load_var_cache_miss();
                        if let Some(slot) = vm.lookup_global_slot(name) {
                            let val = vm
                                .global_slot_value(slot)
                                .ok_or_else(|| vm_err("invalid global slot"))?
                                .clone();
                            let cache = &mut vm.inline_cache[idx];
                            cache.slot = Some(slot);
                            vm.stack.push(val);
                            continue;
                        }

                        if let Some(value) = vm.builtins.get_value(name) {
                            vm.stack.push(value);
                            continue;
                        }

                        if vm.builtins.is_disabled_name(name) {
                            return Err(not_bound_err(format!(
                                "'{name}' has not been bound to a value"
                            ))
                            .attach_note(format!(
                                "builtin '{name}' exists but is disabled in the current preset"
                            )));
                        }

                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        )));
                    }
                    Instruction::LoadCallTarget(operand) => {
                        let target = resolve_operand(vm, idx, operand, 0, hooks)?;
                        vm.stack.push(target);
                    }
                    Instruction::LoadLocal(i) => {
                        let slot = usize::from(*i);
                        let val =
                            vm.current_locals()
                                .and_then(|f| f.get(slot))
                                .ok_or_else(|| {
                                    vm.attach_local_slot_note(slot, vm_err("invalid local slot"))
                                })?;
                        vm.stack.push(val.read());
                    }
                    Instruction::LoadCapture(i) => {
                        let idx = usize::from(*i);
                        let cap_num = *i;
                        let cell = vm
                            .current_captures()
                            .and_then(|c| c.get(idx))
                            .ok_or_else(|| vm_err(format!("invalid capture slot {cap_num}")))?;
                        let value = cell.lock().expect("poisoned capture").clone();
                        vm.stack.push(value);
                    }
                    Instruction::LoadClosure(payload) => {
                        let locals = payload.locals;
                        let captures = &payload.captures;
                        let mut captured_vals = Vec::with_capacity(captures.len());
                        for cap in captures {
                            match cap {
                                Capture::Local(slot) => {
                                    let slot_idx = usize::from(*slot);
                                    let val = if let Some(parent) = vm.current_locals() {
                                        parent
                                            .get(slot_idx)
                                            .map(|s| s.read())
                                            .unwrap_or_else(Value::empty_list)
                                    } else {
                                        Value::empty_list()
                                    };
                                    captured_vals.push(Arc::new(Mutex::new(val)));
                                }
                                Capture::LocalShared(slot) => {
                                    let slot_idx = usize::from(*slot);
                                    let cell = if let Some(parent) = vm.current_locals_mut() {
                                        parent
                                            .get_mut(slot_idx)
                                            .map(|s| s.ensure_cell())
                                            .unwrap_or_else(|| {
                                                Arc::new(Mutex::new(Value::empty_list()))
                                            })
                                    } else {
                                        Arc::new(Mutex::new(Value::empty_list()))
                                    };
                                    captured_vals.push(cell);
                                }
                                Capture::FromCapture(i) => {
                                    let cap_idx = usize::from(*i);
                                    let cell = vm
                                        .current_captures()
                                        .and_then(|c| c.get(cap_idx))
                                        .cloned()
                                        .unwrap_or_else(|| {
                                            Arc::new(Mutex::new(Value::empty_list()))
                                        });
                                    captured_vals.push(cell);
                                }
                                Capture::Global(name, span) => {
                                    let val = if let Some(v) = vm.lookup_global(name) {
                                        v
                                    } else {
                                        let mut err = not_bound_err(format!(
                                            "'{name}' has not been bound to a value"
                                        ))
                                        .attach_note("when loading a function");
                                        if let Some((start, end)) = span {
                                            let base_offs = vm.resolved_debug_base_offset();
                                            let abs_start = start + base_offs;
                                            let abs_end = end + base_offs;
                                            if let Some(sf) = vm.debug_info.file(
                                                vm.debug_info
                                                    .expect_chunk(vm.expect_current_chunk())
                                                    .file_id,
                                            ) {
                                                err = err
                                                    .span(Some((abs_start, abs_end)))
                                                    .source_ctx(
                                                        sf.text().to_string(),
                                                        sf.path().to_string(),
                                                    );
                                            }
                                        }
                                        return Err(err);
                                    };
                                    captured_vals.push(Arc::new(Mutex::new(val)));
                                }
                            }
                        }
                        let instructions = &payload.instructions;
                        let dbg_stmt_spans = &payload.dbg_stmt_spans;
                        let dbg_pc_spans = &payload.dbg_pc_spans;
                        let dbg_stmt_marks = &payload.dbg_stmt_marks;
                        let dbg_local_names = &payload.dbg_local_names;
                        let params = &payload.params;
                        let source_base_offset = vm.resolved_debug_base_offset();
                        let chunk_opt = load_closure_debug_chunk(vm, payload, source_base_offset);
                        hooks.on_closure_capture_alloc(&|| captured_vals.len());
                        vm.stack.push(Value::Closure(Arc::new(ClosureData {
                            params: params.clone(),
                            named_params: payload.named_params.clone(),
                            locals,
                            isolated_module: payload.isolated_module,
                            captured: Arc::from(captured_vals),
                            instructions: instructions.clone(),
                            dbg_chunk: chunk_opt,
                            dbg_stmt_spans: Some(dbg_stmt_spans.clone()),
                            dbg_source_base_offset: source_base_offset,
                            dbg_pc_spans: Some(dbg_pc_spans.clone()),
                            dbg_stmt_marks: Some(dbg_stmt_marks.clone()),
                            dbg_local_names: Some(dbg_local_names.clone()),
                            dbg_provenance: None,
                        })));
                    }

                    Instruction::LoadVarExists(name) => {
                        vm.stack
                            .push(Value::Bool(vm.lookup_global_slot(name).is_some()));
                    }
                    Instruction::LoadSelf => {
                        let me = vm.current_callable().ok_or_else(|| {
                            vm_err("current function is unavailable outside a function")
                        })?;
                        vm.stack.push(me.clone());
                    }

                    Instruction::StoreVar(name) => store_var_impl(vm, idx, name, false)?,
                    Instruction::StoreVarKeep(name) => store_var_impl(vm, idx, name, true)?,
                    Instruction::StoreLocal(i) => store_local_impl(vm, idx, *i, false)?,
                    Instruction::StoreLocalKeep(i) => store_local_impl(vm, idx, *i, true)?,
                    Instruction::StoreCaptureKeep(i) => {
                        let slot = usize::from(*i);
                        let slot_num = *i;
                        let val = last_clone_stack(&vm.stack, || {
                            format!("store into capture slot {slot_num}")
                        })?;
                        let track = vm.symbol_trackers_enabled();
                        let new = track.then(|| val.clone());
                        let old = {
                            let cell = vm.current_captures().and_then(|c| c.get(slot)).ok_or_else(
                                || vm_err(format!("invalid capture slot {slot_num}")),
                            )?;
                            let mut target = cell.lock().expect("poisoned capture");
                            let old = track.then(|| target.clone());
                            *target = val;
                            old
                        };
                        if let Some(new) = new {
                            vm.note_capture_symbol_write(
                                idx,
                                *i,
                                SymbolMutationKind::Store,
                                old,
                                new,
                            );
                        }
                    }

                    Instruction::Unpack(plan) => {
                        let source =
                            pop1_stack(&mut vm.stack, || "unpack assignment source".into())?;
                        let mut values = Vec::with_capacity(plan.paths.len() + 1);
                        values.push(source.clone());
                        for path in &plan.paths {
                            values.push(
                                extract_unpack_path(&source, path)
                                    .map_err(|err| index_load_err(&err.index, &err.target))?,
                            );
                        }
                        vm.unpack_frames.push(values.into_boxed_slice());
                    }
                    Instruction::LoadUnpack(slot) => {
                        let value = vm
                            .unpack_frames
                            .last()
                            .and_then(|frame| frame.get(*slot))
                            .cloned()
                            .ok_or_else(|| vm_err("invalid anonymous unpack slot"))?;
                        vm.stack.push(value);
                    }
                    Instruction::EndUnpack => {
                        vm.unpack_frames
                            .pop()
                            .ok_or_else(|| vm_err("missing anonymous unpack frame"))?;
                    }

                    Instruction::BinaryOp(data) => {
                        let op = data.op;
                        if let Some(result) = try_eval_int_binary(vm, data) {
                            hooks.on_binary_result(&op, &result);
                            vm.stack.push(result);
                            continue;
                        }
                        let right =
                            resolve_operand(vm, idx, &data.right, 1, hooks).map_err(|e| {
                                e.src(format!(
                                    "binary operator '{}' right operand",
                                    binary_op_display(&op)
                                ))
                            })?;
                        let left = resolve_operand(vm, idx, &data.left, 0, hooks).map_err(|e| {
                            e.src(format!(
                                "binary operator '{}' left operand",
                                binary_op_display(&op)
                            ))
                        })?;
                        let result = eval_binary(&op, &left, &right)?;
                        hooks.on_binary_result(&op, &result);
                        vm.stack.push(result);
                    }
                    Instruction::CatAssign(data) => {
                        let right = resolve_operand(vm, idx, &data.right, 1, hooks)
                            .map_err(|e| e.src("binary operator ',' right operand"))?;
                        let result = mutate_target(
                            vm,
                            idx,
                            &data.target,
                            SymbolMutationKind::Store,
                            |target| {
                                let left = std::mem::replace(target, Value::empty_list());
                                *target = left.cat(right);
                                Ok(target.clone())
                            },
                        )?;
                        vm.stack.push(result);
                    }
                    Instruction::Cat(n) => {
                        let count = *n;
                        ensure_stack_len(&vm.stack, count, || "cat operands".into())?;
                        let base = vm.stack.len() - count;
                        let mut items = Vec::with_capacity(count);
                        items.extend(vm.stack.drain(base..));
                        hooks.on_cat_alloc(&|| count);
                        vm.stack.push(Value::cat_many(items));
                    }
                    Instruction::UnaryOp(data) => {
                        let op = data.op;
                        let val =
                            resolve_operand(vm, idx, &data.operand, 0, hooks).map_err(|e| {
                                e.src(format!("unary operator '{}'", unary_op_display(&op)))
                            })?;
                        let result = eval_unary(&op, &val)?;
                        hooks.on_unary_result(&op, &result);
                        vm.stack.push(result);
                    }

                    Instruction::CallBuiltinId(id, argc) => {
                        if vm.try_start_resumable_builtin(*id, *argc, false)? {
                            continue 'exec;
                        }
                        let result = vm.invoke_builtin_id(*id, *argc)?;
                        vm.stack.push(result);
                    }
                    Instruction::CallBuiltinDiscardId(id, argc) => {
                        if vm.try_start_resumable_builtin(*id, *argc, true)? {
                            continue 'exec;
                        }
                        let result = vm.invoke_builtin_discard_id(*id, *argc)?;
                        vm.stack.push(result);
                    }
                    Instruction::CallUser(name, argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || {
                            format!("function '{name}' target and arguments")
                        })?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_loaded_user_call::<false>(vm, idx, name, &func, argc, hooks)? {
                            continue 'exec;
                        }
                    }
                    Instruction::CallAnon(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "callable and arguments".into())?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_anon_call::<false>(vm, idx, &func, argc, hooks)? {
                            continue 'exec;
                        }
                    }
                    Instruction::CallLocal(slot, argc) => {
                        let argc = *argc;
                        let slot_num = *slot;
                        let slot_usize = usize::from(slot_num);
                        ensure_stack_len(&vm.stack, argc + 1, || {
                            format!("local call slot {slot_num} target and arguments")
                        })
                        .map_err(|e| vm.attach_local_slot_note(slot_usize, e))?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_loaded_local_call::<false>(
                            vm, idx, slot_num, &func, argc, hooks,
                        )? {
                            continue 'exec;
                        }
                    }

                    Instruction::TailCallUser(name, argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || {
                            format!("function '{name}' target and arguments")
                        })?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_loaded_user_call::<true>(vm, idx, name, &func, argc, hooks)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailCallAnon(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "callable and arguments".into())?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_anon_call::<true>(vm, idx, &func, argc, hooks)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailCallLocal(slot, argc) => {
                        let argc = *argc;
                        let slot_num = *slot;
                        let slot_usize = usize::from(slot_num);
                        ensure_stack_len(&vm.stack, argc + 1, || {
                            format!("local call slot {slot_num} target and arguments")
                        })
                        .map_err(|e| vm.attach_local_slot_note(slot_usize, e))?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_loaded_local_call::<true>(
                            vm, idx, slot_num, &func, argc, hooks,
                        )? {
                            continue 'exec;
                        }
                    }

                    Instruction::Index => {
                        let idx_val = pop1_stack(&mut vm.stack, || "index".into())?;
                        let obj = pop1_stack(&mut vm.stack, || "object for indexing".into())?;
                        match obj.index(&idx_val) {
                            Some(v) => vm.stack.push(v),
                            None => return Err(index_load_err(&idx_val, &obj)),
                        }
                    }
                    Instruction::IndexMany(argc) => {
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let obj = pop1_stack(&mut vm.stack, || "object for indexing".into())?;
                        index_with_args(&mut vm.stack, &obj, args)?;
                    }
                    Instruction::CheckAtomPathIndex => {
                        let Some(idx_val) = vm.stack.last() else {
                            return Err(vm_err("path index"));
                        };
                        if idx_val.bulk_index_key().is_some() {
                            return Err(index_err(
                                "bulk index cannot appear before the final path segment",
                            )
                            .attach_note(format!(
                                "index: {} ({})",
                                idx_val.excerpt(),
                                idx_val.category()
                            )));
                        }
                    }

                    Instruction::IndexLoadVar(name) => {
                        let idx_val = pop1_stack(&mut vm.stack, || "index".into())?;

                        let mut slot_opt = vm.inline_cache[idx].slot;
                        if slot_opt.is_none()
                            && let Some(slot) = vm.lookup_global_slot(name)
                        {
                            vm.inline_cache[idx].slot = Some(slot);
                            slot_opt = Some(slot);
                        }

                        let global_val_opt = if let Some(slot) = slot_opt {
                            vm.global_slot_value(slot)
                        } else {
                            vm.lookup_global_ref(name)
                        };

                        if let Some(global_val) = global_val_opt {
                            match global_val.index(&idx_val) {
                                Some(v) => vm.stack.push(v),
                                None => return Err(index_load_err(&idx_val, global_val)),
                            }
                        } else if vm.builtins.is_disabled_name(name) {
                            return Err(not_bound_err(format!(
                                "'{name}' has not been bound to a value"
                            ))
                            .attach_note(format!(
                                "builtin '{name}' exists but is disabled in the current preset"
                            )));
                        } else {
                            return Err(not_bound_err(format!(
                                "'{name}' has not been bound to a value"
                            )));
                        }
                    }
                    Instruction::IndexManyLoadVar(name, argc) => {
                        let args = take_index_args(&mut vm.stack, *argc)?;

                        let mut slot_opt = vm.inline_cache[idx].slot;
                        if slot_opt.is_none()
                            && let Some(slot) = vm.lookup_global_slot(name)
                        {
                            vm.inline_cache[idx].slot = Some(slot);
                            slot_opt = Some(slot);
                        }

                        let global_val_opt = if let Some(slot) = slot_opt {
                            vm.global_slot_value(slot)
                        } else {
                            vm.lookup_global_ref(name)
                        };

                        if let Some(global_val) = global_val_opt {
                            let value = index_value_with_args(global_val, args)?;
                            vm.stack.push(value);
                        } else if vm.builtins.is_disabled_name(name) {
                            return Err(not_bound_err(format!(
                                "'{name}' has not been bound to a value"
                            ))
                            .attach_note(format!(
                                "builtin '{name}' exists but is disabled in the current preset"
                            )));
                        } else {
                            return Err(not_bound_err(format!(
                                "'{name}' has not been bound to a value"
                            )));
                        }
                    }
                    Instruction::IndexLoadLocal(slot) => {
                        let slot = usize::from(*slot);
                        let idx_val = pop1_stack(&mut vm.stack, || "index".into())?;
                        let target = read_local_target(vm, slot)?;
                        match target.index(&idx_val) {
                            Some(v) => vm.stack.push(v),
                            None => return Err(index_load_err(&idx_val, &target)),
                        }
                    }
                    Instruction::IndexManyLoadLocal(slot, argc) => {
                        let slot = usize::from(*slot);
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let target = read_local_target(vm, slot)?;
                        index_with_args(&mut vm.stack, &target, args)?;
                    }
                    Instruction::IndexLoadCapture(slot) => {
                        let slot = usize::from(*slot);
                        let idx_val = pop1_stack(&mut vm.stack, || "index".into())?;
                        let target = read_capture_target(vm, slot)?;
                        match target.index(&idx_val) {
                            Some(v) => vm.stack.push(v),
                            None => return Err(index_load_err(&idx_val, &target)),
                        }
                    }
                    Instruction::IndexManyLoadCapture(slot, argc) => {
                        let slot = usize::from(*slot);
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let target = read_capture_target(vm, slot)?;
                        index_with_args(&mut vm.stack, &target, args)?;
                    }

                    Instruction::IndexAssignVar(name) => {
                        let pc = idx;
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = vm
                            .with_global_slot_mut(name, |obj| {
                                let old = track.then(|| obj.clone());
                                let assigned = obj.assign_by_index(&idx, val.clone());
                                if assigned.is_some()
                                    && let Some(old) = old
                                {
                                    change = Some((old, obj.clone()));
                                }
                                assigned
                            })
                            .ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!("when assigning to '{name}'"))
                            })?;
                        if assigned.is_some() {
                            vm.stack.push(val);
                            if let Some((old, new)) = change {
                                vm.note_global_symbol_write(
                                    pc,
                                    name,
                                    SymbolMutationKind::IndexAssign,
                                    Some(old),
                                    new,
                                );
                            }
                        } else {
                            return Err(invalid_index_assign_err(&idx, format!("'{name}'")));
                        }
                    }
                    Instruction::IndexManyAssignVar(name, argc) => {
                        let pc = idx;
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = vm
                            .with_global_slot_mut(name, |obj| {
                                let old = track.then(|| obj.clone());
                                let assigned = obj.assign_by_indices(&args, val.clone());
                                if assigned.is_some()
                                    && let Some(old) = old
                                {
                                    change = Some((old, obj.clone()));
                                }
                                assigned
                            })
                            .ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!("when assigning to '{name}'"))
                            })?;
                        if assigned.is_some() {
                            vm.stack.push(val);
                            if let Some((old, new)) = change {
                                vm.note_global_symbol_write(
                                    pc,
                                    name,
                                    SymbolMutationKind::IndexAssign,
                                    Some(old),
                                    new,
                                );
                            }
                        } else {
                            return Err(invalid_index_assign_err_for_args(
                                &args,
                                format!("'{name}'"),
                            ));
                        }
                    }
                    Instruction::IndexAssignVarDrop(name) => {
                        let pc = idx;
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = vm
                            .with_global_slot_mut(name, |obj| {
                                let old = track.then(|| obj.clone());
                                let assigned = obj.assign_by_index(&idx, val);
                                if assigned.is_some()
                                    && let Some(old) = old
                                {
                                    change = Some((old, obj.clone()));
                                }
                                assigned
                            })
                            .ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!("when assigning to '{name}'"))
                            })?;
                        if assigned.is_none() {
                            return Err(invalid_index_assign_err(&idx, format!("'{name}'")));
                        }
                        if let Some((old, new)) = change {
                            vm.note_global_symbol_write(
                                pc,
                                name,
                                SymbolMutationKind::IndexAssign,
                                Some(old),
                                new,
                            );
                        }
                    }
                    Instruction::IndexManyAssignVarDrop(name, argc) => {
                        let pc = idx;
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = vm
                            .with_global_slot_mut(name, |obj| {
                                let old = track.then(|| obj.clone());
                                let assigned = obj.assign_by_indices(&args, val);
                                if assigned.is_some()
                                    && let Some(old) = old
                                {
                                    change = Some((old, obj.clone()));
                                }
                                assigned
                            })
                            .ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!("when assigning to '{name}'"))
                            })?;
                        if assigned.is_none() {
                            return Err(invalid_index_assign_err_for_args(
                                &args,
                                format!("'{name}'"),
                            ));
                        }
                        if let Some((old, new)) = change {
                            vm.note_global_symbol_write(
                                pc,
                                name,
                                SymbolMutationKind::IndexAssign,
                                Some(old),
                                new,
                            );
                        }
                    }
                    Instruction::IndexAssignLocal(slot) => {
                        let pc = idx;
                        let slot_num = *slot;
                        let slot = usize::from(slot_num);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let slot_note = vm
                            .local_slot_name(slot)
                            .map(|name| format!("local slot {slot}: {name}"));
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = {
                            let frame = vm
                                .current_locals_mut()
                                .ok_or_else(|| vm_err("no local frame"))?;
                            let slot_ref = frame.get_mut(slot).ok_or_else(|| match &slot_note {
                                Some(note) => {
                                    vm_err(format!("invalid local slot {slot}")).attach_note(note)
                                }
                                None => vm_err(format!("invalid local slot {slot}")),
                            })?;
                            let old = track.then(|| slot_ref.read());
                            let assigned = slot_ref
                                .with_mut(|target| target.assign_by_index(&idx, val.clone()));
                            if assigned.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, slot_ref.read()));
                            }
                            assigned
                        };
                        if assigned.is_some() {
                            vm.stack.push(val);
                            if let Some((old, new)) = change {
                                vm.note_local_symbol_write(
                                    pc,
                                    slot_num,
                                    SymbolMutationKind::IndexAssign,
                                    Some(old),
                                    new,
                                );
                            }
                        } else {
                            return Err(vm.attach_local_slot_note(
                                slot,
                                invalid_index_assign_err(&idx, "a local value"),
                            ));
                        }
                    }
                    Instruction::IndexManyAssignLocal(slot, argc) => {
                        let pc = idx;
                        let slot_num = *slot;
                        let slot = usize::from(slot_num);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let slot_note = vm
                            .local_slot_name(slot)
                            .map(|name| format!("local slot {slot}: {name}"));
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = {
                            let frame = vm
                                .current_locals_mut()
                                .ok_or_else(|| vm_err("no local frame"))?;
                            let slot_ref = frame.get_mut(slot).ok_or_else(|| match &slot_note {
                                Some(note) => {
                                    vm_err(format!("invalid local slot {slot}")).attach_note(note)
                                }
                                None => vm_err(format!("invalid local slot {slot}")),
                            })?;
                            let old = track.then(|| slot_ref.read());
                            let assigned = slot_ref
                                .with_mut(|target| target.assign_by_indices(&args, val.clone()));
                            if assigned.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, slot_ref.read()));
                            }
                            assigned
                        };
                        if assigned.is_some() {
                            vm.stack.push(val);
                            if let Some((old, new)) = change {
                                vm.note_local_symbol_write(
                                    pc,
                                    slot_num,
                                    SymbolMutationKind::IndexAssign,
                                    Some(old),
                                    new,
                                );
                            }
                        } else {
                            return Err(vm.attach_local_slot_note(
                                slot,
                                invalid_index_assign_err_for_args(&args, "a local value"),
                            ));
                        }
                    }
                    Instruction::IndexAssignCapture(slot) => {
                        let pc = idx;
                        let slot_num = *slot;
                        let slot = usize::from(slot_num);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = {
                            let captures = vm
                                .current_captures()
                                .ok_or_else(|| vm_err("no capture frame"))?;
                            let cell = captures
                                .get(slot)
                                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                            let mut target = cell.lock().expect("poisoned capture");
                            let old = track.then(|| target.clone());
                            let assigned = target.assign_by_index(&idx, val.clone());
                            if assigned.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, target.clone()));
                            }
                            assigned
                        };
                        if assigned.is_some() {
                            vm.stack.push(val);
                            if let Some((old, new)) = change {
                                vm.note_capture_symbol_write(
                                    pc,
                                    slot_num,
                                    SymbolMutationKind::IndexAssign,
                                    Some(old),
                                    new,
                                );
                            }
                        } else {
                            return Err(invalid_index_assign_err(&idx, "a captured value"));
                        }
                    }
                    Instruction::IndexManyAssignCapture(slot, argc) => {
                        let pc = idx;
                        let slot_num = *slot;
                        let slot = usize::from(slot_num);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let assigned = {
                            let captures = vm
                                .current_captures()
                                .ok_or_else(|| vm_err("no capture frame"))?;
                            let cell = captures
                                .get(slot)
                                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                            let mut target = cell.lock().expect("poisoned capture");
                            let old = track.then(|| target.clone());
                            let assigned = target.assign_by_indices(&args, val.clone());
                            if assigned.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, target.clone()));
                            }
                            assigned
                        };
                        if assigned.is_some() {
                            vm.stack.push(val);
                            if let Some((old, new)) = change {
                                vm.note_capture_symbol_write(
                                    pc,
                                    slot_num,
                                    SymbolMutationKind::IndexAssign,
                                    Some(old),
                                    new,
                                );
                            }
                        } else {
                            return Err(invalid_index_assign_err_for_args(
                                &args,
                                "a captured value",
                            ));
                        }
                    }
                    Instruction::IndexAssignLocalDrop(slot) => {
                        let pc = idx;
                        let slot_n = usize::from(*slot);
                        let slot_num = *slot;
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let success = {
                            let slot_ref = vm.local_slot_mut(*slot)?;
                            let old = track.then(|| slot_ref.read());
                            let success =
                                slot_ref.with_mut(|target| target.assign_by_index(&idx, val));
                            if success.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, slot_ref.read()));
                            }
                            success
                        };
                        if success.is_none() {
                            return Err(vm.attach_local_slot_note(
                                slot_n,
                                invalid_index_assign_err(&idx, "a local value"),
                            ));
                        }
                        if let Some((old, new)) = change {
                            vm.note_local_symbol_write(
                                pc,
                                slot_num,
                                SymbolMutationKind::IndexAssign,
                                Some(old),
                                new,
                            );
                        }
                        // Drop result
                    }
                    Instruction::IndexManyAssignLocalDrop(slot, argc) => {
                        let pc = idx;
                        let slot_n = usize::from(*slot);
                        let slot_num = *slot;
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let success = {
                            let slot_ref = vm.local_slot_mut(*slot)?;
                            let old = track.then(|| slot_ref.read());
                            let success =
                                slot_ref.with_mut(|target| target.assign_by_indices(&args, val));
                            if success.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, slot_ref.read()));
                            }
                            success
                        };
                        if success.is_none() {
                            return Err(vm.attach_local_slot_note(
                                slot_n,
                                invalid_index_assign_err_for_args(&args, "a local value"),
                            ));
                        }
                        if let Some((old, new)) = change {
                            vm.note_local_symbol_write(
                                pc,
                                slot_num,
                                SymbolMutationKind::IndexAssign,
                                Some(old),
                                new,
                            );
                        }
                    }
                    Instruction::IndexAssignCaptureDrop(slot) => {
                        let pc = idx;
                        let slot_num = *slot;
                        let slot = usize::from(slot_num);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let success = {
                            let captures = vm
                                .current_captures()
                                .ok_or_else(|| vm_err("no capture frame"))?;
                            let cell = captures
                                .get(slot)
                                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                            let mut target = cell.lock().expect("poisoned capture");
                            let old = track.then(|| target.clone());
                            let success = target.assign_by_index(&idx, val);
                            if success.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, target.clone()));
                            }
                            success
                        };
                        if success.is_none() {
                            return Err(invalid_index_assign_err(&idx, "a captured value"));
                        }
                        if let Some((old, new)) = change {
                            vm.note_capture_symbol_write(
                                pc,
                                slot_num,
                                SymbolMutationKind::IndexAssign,
                                Some(old),
                                new,
                            );
                        }
                    }
                    Instruction::IndexManyAssignCaptureDrop(slot, argc) => {
                        let pc = idx;
                        let slot_num = *slot;
                        let slot = usize::from(slot_num);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let args = take_index_args(&mut vm.stack, *argc)?;
                        let track = vm.symbol_trackers_enabled();
                        let mut change = None;
                        let success = {
                            let captures = vm
                                .current_captures()
                                .ok_or_else(|| vm_err("no capture frame"))?;
                            let cell = captures
                                .get(slot)
                                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                            let mut target = cell.lock().expect("poisoned capture");
                            let old = track.then(|| target.clone());
                            let success = target.assign_by_indices(&args, val);
                            if success.is_some()
                                && let Some(old) = old
                            {
                                change = Some((old, target.clone()));
                            }
                            success
                        };
                        if success.is_none() {
                            return Err(invalid_index_assign_err_for_args(
                                &args,
                                "a captured value",
                            ));
                        }
                        if let Some((old, new)) = change {
                            vm.note_capture_symbol_write(
                                pc,
                                slot_num,
                                SymbolMutationKind::IndexAssign,
                                Some(old),
                                new,
                            );
                        }
                    }

                    Instruction::Postfix(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "target and arguments".into())?;
                        let target = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_postfix::<false>(vm, idx, &target, argc, hooks)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailPostfix(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "target and arguments".into())?;
                        let target = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_postfix::<true>(vm, idx, &target, argc, hooks)? {
                            continue 'exec;
                        }
                    }
                    Instruction::IndexMutate { target, op } => index_mutate(vm, idx, target, op)?,

                    Instruction::Jump(pos) => vm.pc = *pos,
                    Instruction::JumpIfFalse(pos) => {
                        let target = *pos;
                        let v = pop1_stack(&mut vm.stack, || "conditional jump".into())?;
                        let cond = v.try_to_rust_bool().ok_or_else(|| {
                            attach_pc_source_ctx(
                                vm,
                                idx,
                                domain_requirement(Requirement::BOOL).got1(&v).attach_note(
                                    "this value is used as a branch or loop condition",
                                ),
                            )
                        })?;
                        if !cond {
                            vm.pc = target;
                        }
                    }
                    Instruction::JumpIfCmpFalse(data) => {
                        let target = data.target;
                        let cond = eval_cmp_branch_condition(vm, idx, data, hooks)?;
                        if !cond {
                            vm.pc = target;
                        }
                    }
                    Instruction::NLoopEnter(data) => {
                        let index = read_n_loop_local(vm, data.index)?;
                        let count = read_n_loop_local(vm, data.count)?;
                        if eval_n_loop_condition(vm, idx, &index, &count)? {
                            store_local_value(vm, idx, data.snapshot, index)?;
                        } else {
                            vm.pc = data.target;
                        }
                    }
                    Instruction::NLoopNext(data) => {
                        let snapshot = read_n_loop_local(vm, data.snapshot)?;
                        let next = match snapshot {
                            Value::Int(value) if let Some(next) = value.checked_add(1) => {
                                Value::Int(next)
                            }
                            snapshot => {
                                eval_binary(&BinaryOperator::Add, &snapshot, &Value::Int(1))?
                            }
                        };
                        hooks.on_binary_result(&BinaryOperator::Add, &next);
                        store_local_value(vm, idx, data.index, next)?;
                        vm.pc = data.target;
                    }
                    Instruction::JumpIfGE(pos) => {
                        let target = *pos;
                        // Pop right then left, jump if left >= right
                        let (left, right) =
                            pop2_stack(&mut vm.stack, || "compare-jump (left >= right)".into())?;
                        // Fast path: direct Int/Int comparison
                        if let (Value::Int(a), Value::Int(b)) = (&left, &right) {
                            if *a >= *b {
                                vm.pc = target;
                            }
                            continue;
                        }
                        let lt = left.lt(&right).map_err(|e| e.src(NAME))?;
                        let cond = lt.try_to_rust_bool().ok_or_else(|| {
                            attach_pc_source_ctx(
                                vm,
                                idx,
                                domain_requirement(Requirement::BOOL).got1(&lt).attach_note(
                                    "this comparison is used as a loop or branch condition",
                                ),
                            )
                        })?;
                        if !cond {
                            vm.pc = target;
                        }
                    }
                    Instruction::JumpIfLEZLocal(slot, pos) => {
                        let slot_num = usize::from(*slot);
                        let target = *pos;
                        // Jump if local[slot] <= 0
                        let slot_ref = vm
                            .current_locals()
                            .and_then(|f| f.get(slot_num))
                            .ok_or_else(|| {
                                vm.attach_local_slot_note(
                                    slot_num,
                                    vm_err(format!("invalid local slot {slot_num}")),
                                )
                            })?;
                        let is_le_zero = slot_ref.with_ref(|val| match val {
                            Value::Int(n) => Ok(*n <= 0),
                            Value::Float(f) => Ok(**f <= 0.0),
                            other => Err(attach_pc_source_ctx(
                                vm,
                                idx,
                                domain_requirement(Requirement::one_of([
                                    Requirement::INT,
                                    Requirement::FLOAT,
                                ]))
                                .got1(other)
                                .attach_note("this value is used as a loop bound check"),
                            )),
                        })?;
                        if is_le_zero {
                            vm.pc = target;
                        }
                    }
                    Instruction::JumpIfNamedProvided(slot, bit, pos) => {
                        let slot_num = usize::from(*slot);
                        let target = *pos;
                        let slot_ref = vm
                            .current_locals()
                            .and_then(|frame| frame.get(slot_num))
                            .ok_or_else(|| {
                                vm.attach_local_slot_note(
                                    slot_num,
                                    vm_err(format!("invalid local slot {slot_num}")),
                                )
                            })?;
                        let provided = slot_ref.with_ref(
                            |value| matches!(value, Value::Int(mask) if mask & (1i64 << bit) != 0),
                        );
                        if provided {
                            vm.pc = target;
                        }
                    }

                    Instruction::BoolAndLazy(target) => {
                        // Peek: only pop+push on short-circuit (false/0).
                        if vm
                            .stack
                            .last()
                            .is_some_and(|v| v.try_to_rust_bool() == Some(false))
                        {
                            vm.stack.pop();
                            vm.stack.push(Value::Bool(false));
                            vm.pc = *target;
                        }
                    }
                    Instruction::BoolOrLazy(target) => {
                        // Peek: only pop+push on short-circuit (true/1).
                        if vm
                            .stack
                            .last()
                            .is_some_and(|v| v.try_to_rust_bool() == Some(true))
                        {
                            vm.stack.pop();
                            vm.stack.push(Value::Bool(true));
                            vm.pc = *target;
                        }
                    }
                    Instruction::BoolCombine(operator) => {
                        let (left, right) =
                            pop2_stack(&mut vm.stack, || "lazy bool operands".into())?;
                        let result = eval_bool_op(*operator, &left, &right)?;
                        hooks.on_lazy_bool_result(*operator, &result);
                        vm.stack.push(result);
                    }
                    Instruction::MakeList(n) => {
                        let count = *n;
                        ensure_stack_len(&vm.stack, count, || "list elements".into())?;
                        let base = vm.stack.len() - count;
                        let mut items = Vec::with_capacity(count);
                        items.extend(vm.stack.drain(base..));
                        hooks.on_list_alloc(&|| count);
                        vm.stack.push(Value::from_items(items));
                    }
                    Instruction::MakeDict(n) => {
                        let count = *n;
                        ensure_stack_len(&vm.stack, count * 2, || "dict key-value pairs".into())?;
                        let base = vm.stack.len() - count * 2;
                        let mut map = IndexMap::with_capacity(count);
                        let mut iter = vm.stack.drain(base..);

                        for _ in 0..count {
                            let key = iter.next().unwrap();
                            let val = iter.next().unwrap();
                            match key {
                                Value::Tag(k) => {
                                    map.insert(k, val);
                                }
                                other => {
                                    return Err(domain_requirement(Requirement::TAG)
                                        .attach_note("for a dict key")
                                        .got1(&other));
                                }
                            }
                        }
                        drop(iter);
                        hooks.on_dict_alloc(&|| count);
                        vm.stack.push(Value::Dict(Arc::new(map)));
                    }

                    Instruction::MakeRange {
                        inclusive,
                        has_next,
                    } => {
                        let inclusive = *inclusive;
                        let has_next = *has_next;
                        let end_val = pop1_stack(&mut vm.stack, || "range end".into())?;
                        let next_val = if has_next {
                            Some(pop1_stack(&mut vm.stack, || "range next".into())?)
                        } else {
                            None
                        };
                        let start_val = pop1_stack(&mut vm.stack, || "range start".into())?;
                        let res = if let Some(next_val) = next_val.as_ref() {
                            make_range_from_next(&start_val, next_val, &end_val, inclusive)
                        } else {
                            make_range(&start_val, &end_val, None, inclusive)
                        }
                        .map_err(|e| e.src(NAME))?;
                        hooks.on_range_alloc(&|| range_alloc_len(&res));
                        vm.stack.push(res);
                    }

                    Instruction::PrepareNamedArgs(meta) => {
                        vm.pending_named_meta = Some(meta.clone());
                    }

                    Instruction::CmpChain(ops) => {
                        let ops = ops.as_ref();
                        let need = ops.len() + 1;
                        ensure_stack_len(&vm.stack, need, || {
                            format!("comparison chain of length {}", ops.len())
                        })?;
                        let base = vm.stack.len() - need;

                        let result = eval_cmp_chain(ops, &vm.stack[base..])?;
                        vm.stack.truncate(base);
                        vm.stack.push(result);
                    }

                    Instruction::Pop => {
                        vm.stack.pop();
                    }
                    Instruction::Return => {
                        hooks.on_return(vm);
                        vm.tail_call_journal.clear();
                        vm.tail_call_depth = 0;
                        vm.returned = true;
                        if vm.trace_depth > 0
                            && let Some(prev) = vm.pending_trace_probe.take()
                        {
                            record_trace_probe(vm, prev);
                        }
                        let value = vm.stack.pop().unwrap_or_else(Value::empty_list);
                        Self::discard_current_try_frames(vm);
                        if let Some(stop_call_depth) = control.stop_call_depth
                            && vm.physical_call_depth() == stop_call_depth + 1
                        {
                            let value = vm
                                .finish_user_call(value, false)
                                .expect("stopped call should have a caller frame");
                            return Ok(InterpretPoll::Ready(value));
                        }
                        if vm.physical_call_depth() == 0 {
                            return Ok(InterpretPoll::Ready(value));
                        }
                        vm.finish_user_call(value, true);
                        continue 'exec;
                    }

                    Instruction::Try(len) => {
                        let len = *len;
                        let start_pc = vm.pc;
                        let end_pc = start_pc
                            .checked_add(len)
                            .filter(|end| *end <= limit)
                            .ok_or_else(|| vm_err("invalid try region"))?;
                        vm.try_stack.push(TryFrame {
                            instructions: Arc::clone(&vm.instructions),
                            call_depth: vm.physical_call_depth(),
                            end_pc,
                            stack_start: vm.stack.len(),
                            saved_pending_named_meta: vm.pending_named_meta.take(),
                            saved_trace_depth: vm.trace_depth,
                            saved_trace_bases_len: vm.trace_bases.len(),
                            saved_trace_buf_len: vm.trace_buf.len(),
                            unpack_depth: vm.unpack_frames.len(),
                        });
                    }
                    Instruction::Import(import) => {
                        if let Some(value) = vm.import_module(import)? {
                            vm.stack.push(value);
                        } else {
                            continue 'exec;
                        }
                    }
                    Instruction::TraceBegin => {
                        vm.trace_depth = vm.trace_depth.saturating_add(1);
                        vm.trace_bases.push(vm.trace_buf.len());
                    }
                    Instruction::Debug => {
                        let base = vm.trace_bases.pop().unwrap_or(vm.trace_buf.len());
                        if vm.trace_depth > 0 {
                            vm.trace_depth -= 1;
                        }
                        let records: Vec<TraceRecord> = vm.trace_buf.drain(base..).collect();
                        let value = vm.stack.last().ok_or_else(|| vm_err("missing @d value"))?;
                        vm.debug_log
                            .emit_line(render_trace_line(vm, idx, value, &records));
                    }
                    Instruction::Pause => {}
                }
            }

            if Self::finish_try_boundary(vm) {
                continue 'exec;
            }

            if !Arc::ptr_eq(&instructions, &vm.instructions) {
                instructions = Arc::clone(&vm.instructions);
                limit = instructions.len();
                continue;
            }

            if vm.trace_depth > 0
                && let Some(prev) = vm.pending_trace_probe.take()
            {
                record_trace_probe(vm, prev);
            }
            let value = vm.stack.pop().unwrap_or_else(Value::empty_list);
            Self::discard_current_try_frames(vm);
            if let Some(stop_call_depth) = control.stop_call_depth
                && vm.physical_call_depth() == stop_call_depth + 1
            {
                let value = vm
                    .finish_user_call(value, false)
                    .expect("stopped call should have a caller frame");
                return Ok(InterpretPoll::Ready(value));
            }
            if vm.physical_call_depth() == 0 {
                return Ok(InterpretPoll::Ready(value));
            }
            vm.finish_user_call(value, true);
        }
    }

    fn finish_try_boundary(vm: &mut Vm) -> bool {
        let Some(frame) = vm.try_stack.last() else {
            return false;
        };
        if frame.call_depth != vm.physical_call_depth()
            || !Arc::ptr_eq(&frame.instructions, &vm.instructions)
            || vm.pc < frame.end_pc
        {
            return false;
        }

        let frame = vm.try_stack.pop().expect("checked try frame");
        let completed = vm.pc == frame.end_pc;
        let stack_start = frame.stack_start;
        let value = completed.then(|| {
            if vm.stack.len() > stack_start {
                vm.stack.pop().expect("stack length checked")
            } else {
                Value::empty_list()
            }
        });
        Self::restore_try_state(vm, frame, false);
        vm.stack.truncate(stack_start);
        if let Some(value) = value {
            vm.stack.push(try_result("ok", value));
        }
        true
    }

    fn catch_current_try_error(vm: &mut Vm, err: &WqError) -> bool {
        if err.err_type == WqErrorType::Vm {
            return false;
        }
        let Some(frame) = vm.try_stack.last() else {
            return false;
        };
        if frame.call_depth != vm.physical_call_depth()
            || !Arc::ptr_eq(&frame.instructions, &vm.instructions)
        {
            return false;
        }

        let frame = vm.try_stack.pop().expect("checked try frame");
        let end_pc = frame.end_pc;
        let instructions = Arc::clone(&frame.instructions);
        let stack_start = frame.stack_start;
        let frames = err.crash.as_deref().map_or(&[][..], |crash| crash.frames());
        let error_value = err.to_wq_value(&vm.debug_info, frames);
        Self::restore_try_state(vm, frame, true);
        vm.instructions = instructions;
        vm.pc = end_pc;
        vm.stack.truncate(stack_start);
        vm.stack.push(try_result("error", error_value));
        true
    }

    fn discard_current_try_frames(vm: &mut Vm) {
        while vm
            .try_stack
            .last()
            .is_some_and(|frame| frame.call_depth == vm.physical_call_depth())
        {
            let frame = vm.try_stack.pop().expect("checked try frame");
            Self::restore_try_state(vm, frame, false);
        }
    }

    fn restore_try_state(vm: &mut Vm, frame: TryFrame, rollback_trace: bool) {
        vm.unpack_frames.truncate(frame.unpack_depth);
        vm.pending_named_meta = frame.saved_pending_named_meta;
        if rollback_trace {
            vm.trace_depth = frame.saved_trace_depth;
            vm.trace_bases.truncate(frame.saved_trace_bases_len);
            vm.trace_buf.truncate(frame.saved_trace_buf_len);
        }
    }
}

fn try_result(kind: &'static str, value: Value) -> Value {
    Value::List(Arc::new(vec![Value::Tag(Arc::from(kind)), value]))
}

fn load_closure_debug_chunk(
    vm: &mut Vm,
    payload: &ClosurePayload,
    source_base_offset: usize,
) -> Option<ChunkId> {
    if !vm.debug_artifacts_enabled() {
        return None;
    }
    let file_id = vm
        .debug_info
        .expect_chunk(vm.expect_current_chunk())
        .file_id;
    if let Some(id) = payload.dbg_chunk
        && vm
            .debug_info
            .get_chunk(id)
            .is_some_and(|meta| meta.file_id == file_id && meta.len == payload.instructions.len())
    {
        if vm.debug_log.enabled(DebugLogFlags::WQDB) {
            vm.debug_log
                .emit_line(format!("[wqdb]: LoadClosure reuse chunk={id:?}"));
        }
        return Some(id);
    }

    let instructions = &payload.instructions;
    let id = vm
        .debug_info
        .new_function_chunk(None, file_id, instructions.len());
    if vm.debug_log.enabled(DebugLogFlags::WQDB) {
        vm.debug_log.emit_line(format!(
            "[wqdb]: LoadClosure new chunk={id:?} file_id={file_id} instructions={} base_offset={}",
            instructions.len(),
            source_base_offset,
        ));
    }
    if !payload.dbg_pc_spans.is_empty() && !payload.dbg_stmt_marks.is_empty() {
        let (has_exact, has_real) = {
            let table = &mut vm.debug_info.expect_chunk_mut(id).line_table;
            apply_stmt_debug_exact_offs(
                table,
                file_id,
                payload.dbg_pc_spans.as_ref(),
                payload.dbg_stmt_marks.as_ref(),
                source_base_offset,
                Some(&vm.debug_log),
            )
        };
        vm.debug_info
            .expect_chunk_mut(id)
            .note_debug_spans(has_exact, has_real);
    } else if !payload.dbg_stmt_spans.is_empty() {
        let has_real = {
            let table = &mut vm.debug_info.expect_chunk_mut(id).line_table;
            apply_stmt_spans_exact_offs(
                table,
                instructions.as_ref(),
                file_id,
                payload.dbg_stmt_spans.as_ref(),
                source_base_offset,
                Some(&vm.debug_log),
            )
        };
        vm.debug_info
            .expect_chunk_mut(id)
            .note_debug_spans(false, has_real);
    } else {
        let table = &mut vm.debug_info.expect_chunk_mut(id).line_table;
        mark_stmt_heuristic(table, instructions.as_ref(), Some(&vm.debug_log));
    }
    if !payload.dbg_local_names.is_empty() {
        vm.debug_info.expect_chunk_mut(id).local_names =
            Some(payload.dbg_local_names.iter().cloned().collect());
    } else if let Some(ps) = payload.params.as_ref() {
        vm.debug_info.expect_chunk_mut(id).local_names = Some(ps.iter().cloned().collect());
    }
    Some(id)
}

// int fast path =================================

fn try_eval_int_binary(vm: &mut Vm, data: &BinaryOpData) -> Option<Value> {
    try_eval_int_binary_operands(vm, data.op, &data.left, &data.right)
}

fn try_eval_int_binary_operands(
    vm: &mut Vm,
    op: BinaryOperator,
    left_operand: &Operand,
    right_operand: &Operand,
) -> Option<Value> {
    let right_is_stack = matches!(right_operand, Operand::Stack);
    let left_is_stack = matches!(left_operand, Operand::Stack);
    let stack_len = vm.stack.len();
    let stack_count = usize::from(right_is_stack) + usize::from(left_is_stack);
    if stack_count > stack_len {
        return None;
    }

    let right_stack_idx = if right_is_stack {
        Some(stack_len - 1)
    } else {
        None
    };
    let left_stack_idx = if left_is_stack {
        Some(stack_len - 1 - usize::from(right_is_stack))
    } else {
        None
    };
    let right = int_operand(vm, right_operand, right_stack_idx)?;
    let left = int_operand(vm, left_operand, left_stack_idx)?;
    let result = eval_int_binary(op, left, right)?;

    if stack_count > 0 {
        vm.stack.truncate(stack_len - stack_count);
    }
    Some(result)
}

fn try_eval_int_cmp_branch(
    vm: &mut Vm,
    op: BinaryOperator,
    left_operand: &Operand,
    right_operand: &Operand,
) -> Option<bool> {
    let right_is_stack = matches!(right_operand, Operand::Stack);
    let left_is_stack = matches!(left_operand, Operand::Stack);
    let stack_len = vm.stack.len();
    let stack_count = usize::from(right_is_stack) + usize::from(left_is_stack);
    if stack_count > stack_len {
        return None;
    }

    let right_stack_idx = if right_is_stack {
        Some(stack_len - 1)
    } else {
        None
    };
    let left_stack_idx = if left_is_stack {
        Some(stack_len - 1 - usize::from(right_is_stack))
    } else {
        None
    };
    let right = int_operand(vm, right_operand, right_stack_idx)?;
    let left = int_operand(vm, left_operand, left_stack_idx)?;
    let result = eval_int_comparison(op, left, right)?;

    if stack_count > 0 {
        vm.stack.truncate(stack_len - stack_count);
    }
    Some(result)
}

fn eval_cmp_branch_condition(
    vm: &mut Vm,
    idx: usize,
    data: &CmpBranchData,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    if let Some(result) = try_eval_int_cmp_branch(vm, data.op, &data.left, &data.right) {
        return Ok(result);
    }

    let result =
        if let Some(result) = try_eval_int_binary_operands(vm, data.op, &data.left, &data.right) {
            result
        } else {
            let right = resolve_operand(vm, idx, &data.right, 1, hooks).map_err(|e| {
                e.src(format!(
                    "comparison operator '{}' right operand",
                    binary_op_display(&data.op)
                ))
            })?;
            let left = resolve_operand(vm, idx, &data.left, 0, hooks).map_err(|e| {
                e.src(format!(
                    "comparison operator '{}' left operand",
                    binary_op_display(&data.op)
                ))
            })?;
            eval_binary(&data.op, &left, &right)?
        };

    result.try_to_rust_bool().ok_or_else(|| {
        attach_pc_source_ctx(
            vm,
            idx,
            domain_requirement(Requirement::BOOL)
                .got1(&result)
                .attach_note("this value is used as a branch or loop condition"),
        )
    })
}

fn read_n_loop_local(vm: &Vm, slot: u16) -> WqResult<Value> {
    vm.current_locals()
        .and_then(|locals| locals.get(usize::from(slot)))
        .map(crate::vm::Slot::read)
        .ok_or_else(|| {
            vm.attach_local_slot_note(usize::from(slot), vm_err("invalid local slot for N-loop"))
        })
}

fn eval_n_loop_condition(vm: &Vm, idx: usize, index: &Value, count: &Value) -> WqResult<bool> {
    if let (Value::Int(index), Value::Int(count)) = (index, count) {
        return Ok(index < count);
    }
    let result = eval_binary(&BinaryOperator::Lt, index, count)?;
    result.try_to_rust_bool().ok_or_else(|| {
        attach_pc_source_ctx(
            vm,
            idx,
            domain_requirement(Requirement::BOOL)
                .got1(&result)
                .attach_note("this value is used as a branch or loop condition"),
        )
    })
}

fn int_operand(vm: &Vm, operand: &Operand, stack_idx: Option<usize>) -> Option<i64> {
    match operand {
        Operand::Stack => match vm.stack.get(stack_idx?)? {
            Value::Int(n) => Some(*n),
            _ => None,
        },
        Operand::Const(value) => match &**value {
            Value::Int(n) => Some(*n),
            _ => None,
        },
        Operand::Local(slot) => {
            let frame = vm.current_locals()?;
            let slot = frame.get(usize::from(*slot))?;
            slot.with_ref(|value| match value {
                Value::Int(n) => Some(*n),
                _ => None,
            })
        }
        Operand::Capture(_) | Operand::Var(_) | Operand::Self_ => None,
    }
}

fn eval_int_binary(op: BinaryOperator, left: i64, right: i64) -> Option<Value> {
    use BinaryOperator::*;

    let result = match op {
        Add => left.checked_add(right).map(Value::Int),
        Subtract => left.checked_sub(right).map(Value::Int),
        Multiply => left.checked_mul(right).map(Value::Int),
        Divide => (right != 0).then(|| Value::float(left as f64 / right as f64)),
        Modulo => {
            if right == 0 || left == i64::MIN && right == -1 {
                None
            } else {
                Some(Value::Int(left % right))
            }
        }
        FloorDiv => {
            if right == 0 || left == i64::MIN && right == -1 {
                None
            } else {
                let q0 = left / right;
                let r = left % right;
                Some(Value::Int(if r == 0 || (left ^ right) >= 0 {
                    q0
                } else {
                    q0 - 1
                }))
            }
        }
        Equal | EqualDot => Some(Value::Bool(left == right)),
        NotEqual | NotEqualDot => Some(Value::Bool(left != right)),
        Lt => Some(Value::Bool(left < right)),
        Lte => Some(Value::Bool(left <= right)),
        Gt => Some(Value::Bool(left > right)),
        Gte => Some(Value::Bool(left >= right)),
        BitAnd => Some(Value::Int(left & right)),
        BitOr => Some(Value::Int(left | right)),
        BitXor => Some(Value::Int(left ^ right)),
        Shl => u32::try_from(right)
            .ok()
            .map(|shift| Value::from_bigint(BigInt::from(left) << shift)),
        Shr => u32::try_from(right)
            .ok()
            .map(|shift| Value::from_bigint(BigInt::from(left) >> shift)),
        Power | PowerDot | DivideDot | Matmul | Cat => None,
    }?;

    Some(result)
}

fn eval_int_comparison(op: BinaryOperator, left: i64, right: i64) -> Option<bool> {
    use BinaryOperator::*;

    match op {
        Equal | EqualDot => Some(left == right),
        NotEqual | NotEqualDot => Some(left != right),
        Lt => Some(left < right),
        Lte => Some(left <= right),
        Gt => Some(left > right),
        Gte => Some(left >= right),
        Add | Subtract | Multiply | Power | PowerDot | Divide | DivideDot | Modulo | Matmul
        | Cat | BitAnd | BitOr | Shl | Shr | BitXor | FloorDiv => None,
    }
}

fn take_index_args(stack: &mut Vec<Value>, argc: usize) -> WqResult<Sv4> {
    ensure_stack_len(stack, argc, || "index arguments".into())?;
    let base = stack.len() - argc;
    let mut args = Sv4::new();
    args.extend(stack.drain(base..));
    Ok(args)
}

fn index_args_value(args: &[Value]) -> Value {
    match args {
        [arg] => arg.clone(),
        _ => Value::from_items(args.to_vec()),
    }
}

fn invalid_index_assign_err(idx: &Value, target: impl std::fmt::Display) -> WqError {
    index_err("invalid index")
        .got1(idx)
        .attach_note(format!("when assigning to {target}"))
}

fn invalid_index_assign_err_for_args(args: &[Value], target: impl std::fmt::Display) -> WqError {
    invalid_index_assign_err(&index_args_value(args), target)
}

fn index_with_args(stack: &mut Vec<Value>, target: &Value, args: Sv4) -> WqResult<()> {
    let value = index_value_with_args(target, args)?;
    stack.push(value);
    Ok(())
}

fn index_value_with_args(target: &Value, args: Sv4) -> WqResult<Value> {
    let result = if args.len() == 1 {
        target.index(&args[0])
    } else {
        target.index_many(&args)
    };
    match result {
        Some(value) => Ok(value),
        None => {
            let idx = if args.len() == 1 {
                args.into_iter()
                    .next()
                    .expect("single index arg should exist")
            } else {
                Value::from_items(args.into_vec())
            };
            Err(index_load_err(&idx, target))
        }
    }
}

// error helpers ===================================

#[inline]
fn vm_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Vm).src(NAME).msg(msg.into())
}

#[inline]
fn not_bound_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::NotBound)
        .src(NAME)
        .msg(msg.into())
}

#[inline]
fn domain_requirement(requirement: Requirement) -> WqError {
    WqError::new(WqErrorType::Domain)
        .src(NAME)
        .expected(requirement)
}

#[inline]
fn index_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Index).src(NAME).msg(msg.into())
}

#[inline]
fn named_arg_index_err() -> WqError {
    WqError::new(WqErrorType::Arity)
        .src(NAME)
        .msg("cannot pass named arguments when indexing a non-callable value")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use num_bigint::BigInt;

    use super::{domain_requirement, named_arg_index_err};
    use crate::ast::{BinaryOperator, BoolOperator};
    use crate::builtins::BuiltinFnArgs;
    use crate::interpret::Interpreter;
    use crate::interpret::vanilla::VanillaInterpreter;
    use crate::session::stdio::{WqIoError, WqOutput};
    use crate::value::func::FunctionData;
    use crate::value::{Value, WqResult, eval_binary};
    use crate::vm::inst::{BinaryOpData, ClosurePayload, Instruction, Operand};
    use crate::vm::{FunctionActivation, PreparedInstructions, Slot, Vm};
    use crate::wqdb::data::ChunkId;
    use crate::wqerror::Requirement;

    struct SinkOutput;

    impl WqOutput for SinkOutput {
        fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
            Ok(())
        }
    }

    fn set_active_function(vm: &mut Vm, locals: Vec<Slot>, callable: Value) {
        vm.active_function = Some(FunctionActivation {
            locals,
            captures: crate::value::cell::empty_cells(),
            callable,
        });
    }

    #[test]
    fn runtime_requirement_helpers_use_canonical_terms() {
        let condition = domain_requirement(Requirement::BOOL).got1(&Value::Int(1));
        assert_eq!(condition.msg.as_deref(), Some("expected bool"));
        assert_eq!(condition.notes.as_slice(), ["got 1 (int)"]);

        let indexing = named_arg_index_err();
        assert_eq!(
            indexing.msg.as_deref(),
            Some("cannot pass named arguments when indexing a non-callable value")
        );
    }

    #[test]
    fn invalid_local_slot_errors_include_slot_name_note() {
        let mut vm = Vm::new(vec![Instruction::LoadLocal(1)]);
        let file_id = vm.debug_info.new_file("<test>", "");
        let chunk = vm.debug_info.new_chunk("<test>", file_id, 1);
        vm.debug_info.expect_chunk_mut(chunk).local_names = Some(vec!["x".into(), "y".into()]);
        vm.current_chunk = Some(chunk);

        let mut interpreter = VanillaInterpreter;
        let err = interpreter.interpret(&mut vm, 1).unwrap_err();

        assert!(
            err.notes
                .iter()
                .any(|note| note.contains("local slot 1: y")),
            "notes were {:?}",
            err.notes
        );
    }

    #[test]
    fn index_load_var_reads_from_slots() {
        let mut vm = Vm::new(vec![Instruction::IndexLoadVar("xs".into())]);
        vm.assign_global_and_slot("xs", Value::IntList(Arc::new(vec![10, 20, 30])));
        vm.stack.push(Value::Int(1));

        let mut interpreter = VanillaInterpreter;
        let out = interpreter.interpret(&mut vm, 1).expect("execute");

        assert_eq!(out, Value::Int(20));
    }

    #[test]
    fn first_global_literal_mutation_keeps_backing_storage() {
        let backing = Arc::new(vec![1, 2, 3]);
        let backing_ptr = Arc::as_ptr(&backing);
        let mut vm = Vm::from_prepared_instructions(
            PreparedInstructions::with_owned_const_extraction(vec![
                Instruction::load_const(Value::IntList(backing)),
                Instruction::StoreVar("a".into()),
                Instruction::load_const(Value::Int(0)),
                Instruction::load_const(Value::Int(9)),
                Instruction::IndexAssignVarDrop("a".into()),
                Instruction::LoadVar("a".into()),
                Instruction::Return,
            ]),
        );
        let mut interpreter = VanillaInterpreter;

        let out = interpreter.interpret(&mut vm, 7).expect("execute");

        assert_eq!(out, Value::IntList(Arc::new(vec![9, 2, 3])));
        let Some(Value::IntList(items)) = vm.lookup_global_ref("a") else {
            panic!("expected global a to be an int-list");
        };
        assert_eq!(Arc::as_ptr(items), backing_ptr);
    }

    fn run_vm(insts: Vec<Instruction>) -> Value {
        run_vm_result(insts).expect("execute")
    }

    fn run_vm_result(insts: Vec<Instruction>) -> WqResult<Value> {
        let len = insts.len();
        let mut vm = Vm::new(insts);
        let mut interpreter = VanillaInterpreter;
        interpreter.interpret(&mut vm, len)
    }

    fn run_vm_result_with_locals(insts: Vec<Instruction>, locals: Vec<Value>) -> WqResult<Value> {
        let len = insts.len();
        let mut vm = Vm::new(insts);
        set_active_function(
            &mut vm,
            locals.into_iter().map(Slot::Value).collect(),
            Value::empty_list(),
        );
        let mut interpreter = VanillaInterpreter;
        interpreter.interpret(&mut vm, len)
    }

    fn make_fn(params: Option<&[&str]>, locals: u16, instructions: Vec<Instruction>) -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: params.map(|names| {
                Arc::<[String]>::from(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            }),
            named_params: None,
            locals,
            isolated_module: false,
            instructions: instructions.into(),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    #[test]
    fn load_closure_reuses_registered_debug_chunk() {
        let payload = ClosurePayload {
            params: None,
            named_params: None,
            locals: 0,
            isolated_module: false,
            captures: Vec::new(),
            instructions: vec![Instruction::Return].into(),
            dbg_chunk: Some(ChunkId(1)),
            dbg_stmt_spans: Vec::<(usize, usize)>::new().into(),
            dbg_pc_spans: Vec::<Option<(usize, usize)>>::new().into(),
            dbg_stmt_marks: Vec::new().into(),
            dbg_local_names: Vec::<String>::new().into(),
        };
        let mut vm = Vm::new(vec![Instruction::load_closure(payload)]);
        vm.runtime_debug_info = true;
        let file_id = vm.debug_info.new_file("<test>", "");
        let script_chunk = vm.debug_info.new_chunk("<script>", file_id, 1);
        let closure_chunk = vm.debug_info.new_chunk("<fn>", file_id, 1);
        vm.current_chunk = Some(script_chunk);

        let mut interpreter = VanillaInterpreter;
        let result = interpreter.interpret(&mut vm, 1).expect("execute");

        assert_eq!(closure_chunk, ChunkId(1));
        let Value::Closure(closure) = result else {
            panic!("expected closure");
        };
        assert_eq!(closure.dbg_chunk, Some(closure_chunk));
        assert!(
            vm.debug_info.get_chunk(ChunkId(2)).is_none(),
            "loading a pre-registered closure must not allocate another debug chunk"
        );
    }

    #[test]
    fn int_binary_fast_path_consumes_stack_operands_on_success() {
        let mut vm = Vm::new(Vec::new());
        vm.stack.push(Value::Int(9));
        vm.stack.push(Value::Int(3));
        let data = BinaryOpData {
            op: BinaryOperator::Subtract,
            left: Operand::Stack,
            right: Operand::Stack,
        };

        let result = super::try_eval_int_binary(&mut vm, &data);

        assert_eq!(result, Some(Value::Int(6)));
        assert!(vm.stack.is_empty());
    }

    #[test]
    fn int_binary_fast_path_leaves_stack_on_overflow_bailout() {
        let mut vm = Vm::new(Vec::new());
        vm.stack.push(Value::Int(i64::MAX));
        vm.stack.push(Value::Int(1));
        let data = BinaryOpData {
            op: BinaryOperator::Add,
            left: Operand::Stack,
            right: Operand::Stack,
        };

        let result = super::try_eval_int_binary(&mut vm, &data);

        assert_eq!(result, None);
        assert_eq!(vm.stack, vec![Value::Int(i64::MAX), Value::Int(1)]);

        let generic = run_vm(vec![
            Instruction::load_const(Value::Int(i64::MAX)),
            Instruction::load_const(Value::Int(1)),
            Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
            Instruction::Return,
        ]);
        assert_eq!(
            generic,
            Value::from_bigint(BigInt::from(i64::MAX) + BigInt::from(1))
        );
    }

    #[test]
    fn int_binary_fast_path_reads_local_and_const_operands() {
        let mut vm = Vm::new(Vec::new());
        set_active_function(
            &mut vm,
            vec![Slot::Value(Value::Int(40))],
            Value::empty_list(),
        );
        let data = BinaryOpData {
            op: BinaryOperator::Add,
            left: Operand::Local(0),
            right: Operand::const_val(Value::Int(2)),
        };

        let result = super::try_eval_int_binary(&mut vm, &data);

        assert_eq!(result, Some(Value::Int(42)));
        assert!(vm.stack.is_empty());
    }

    #[test]
    fn int_binary_fast_path_matches_eval_binary_for_supported_cases() {
        use BinaryOperator::*;

        let cases = [
            (Add, 2, 3),
            (Add, -4, 7),
            (Subtract, 2, 7),
            (Multiply, -6, 7),
            (Divide, 7, 2),
            (Divide, -7, 2),
            (Modulo, 7, 3),
            (Modulo, -7, 3),
            (FloorDiv, 7, 3),
            (FloorDiv, -7, 3),
            (FloorDiv, 7, -3),
            (FloorDiv, -7, -3),
            (Equal, 5, 5),
            (EqualDot, 5, 6),
            (NotEqual, 5, 6),
            (NotEqualDot, 5, 5),
            (Lt, -1, 2),
            (Lte, 2, 2),
            (Gt, 3, 2),
            (Gte, 2, 3),
            (BitAnd, 0b1100, 0b1010),
            (BitOr, 0b1100, 0b1010),
            (BitXor, 0b1100, 0b1010),
            (Shl, 3, 2),
            (Shl, 1, 65),
            (Shr, -8, 1),
            (Shr, 8, 65),
        ];

        for (op, left, right) in cases {
            let expected = eval_binary(&op, &Value::Int(left), &Value::Int(right))
                .expect("generic int math should succeed");
            let mut vm = Vm::new(Vec::new());
            vm.stack.push(Value::Int(left));
            vm.stack.push(Value::Int(right));
            let data = BinaryOpData {
                op,
                left: Operand::Stack,
                right: Operand::Stack,
            };

            let fast = super::try_eval_int_binary(&mut vm, &data);

            assert_eq!(fast, Some(expected), "{op:?} {left} {right}");
            assert!(vm.stack.is_empty(), "{op:?} left stack behind");
        }
    }

    #[test]
    fn int_binary_vm_matches_eval_binary_for_edge_cases() {
        use BinaryOperator::*;

        let cases = [
            (Add, i64::MAX, 1),
            (Subtract, i64::MIN, 1),
            (Multiply, i64::MAX, 2),
            (Divide, 1, 0),
            (DivideDot, 7, 2),
            (Modulo, 1, 0),
            (FloorDiv, 1, 0),
            (FloorDiv, i64::MIN, -1),
            (Power, 2, 10),
            (PowerDot, 2, -3),
            (Shl, 1, -1),
            (Shr, 1, -1),
        ];

        for (op, left, right) in cases {
            let expected = eval_binary(&op, &Value::Int(left), &Value::Int(right));
            let actual = run_vm_result(vec![
                Instruction::load_const(Value::Int(left)),
                Instruction::load_const(Value::Int(right)),
                Instruction::binary_op(op, Operand::Stack, Operand::Stack),
                Instruction::Return,
            ]);

            assert_eq!(actual, expected, "{op:?} {left} {right}");
        }
    }

    #[test]
    fn cmp_branch_fast_path_reads_local_operands() {
        let out = run_vm_result_with_locals(
            vec![
                Instruction::jump_if_cmp_false(
                    BinaryOperator::Lt,
                    Operand::Local(0),
                    Operand::Local(1),
                    3,
                ),
                Instruction::load_const(Value::Int(42)),
                Instruction::Return,
                Instruction::load_const(Value::Int(99)),
                Instruction::Return,
            ],
            vec![Value::Int(1), Value::Int(2)],
        )
        .expect("execute");

        assert_eq!(out, Value::Int(42));

        let out = run_vm_result_with_locals(
            vec![
                Instruction::jump_if_cmp_false(
                    BinaryOperator::Lt,
                    Operand::Local(0),
                    Operand::Local(1),
                    3,
                ),
                Instruction::load_const(Value::Int(42)),
                Instruction::Return,
                Instruction::load_const(Value::Int(99)),
                Instruction::Return,
            ],
            vec![Value::Int(3), Value::Int(2)],
        )
        .expect("execute");

        assert_eq!(out, Value::Int(99));
    }

    #[test]
    fn cmp_branch_fallback_matches_value_comparison() {
        let out = run_vm_result(vec![
            Instruction::jump_if_cmp_false(
                BinaryOperator::Lt,
                Operand::const_val(Value::float(1.5)),
                Operand::const_val(Value::float(2.0)),
                3,
            ),
            Instruction::load_const(Value::Int(42)),
            Instruction::Return,
            Instruction::load_const(Value::Int(99)),
            Instruction::Return,
        ])
        .expect("execute");

        assert_eq!(out, Value::Int(42));

        let out = run_vm_result(vec![
            Instruction::jump_if_cmp_false(
                BinaryOperator::Lt,
                Operand::const_val(Value::float(3.0)),
                Operand::const_val(Value::float(2.0)),
                3,
            ),
            Instruction::load_const(Value::Int(42)),
            Instruction::Return,
            Instruction::load_const(Value::Int(99)),
            Instruction::Return,
        ])
        .expect("execute");

        assert_eq!(out, Value::Int(99));
    }

    #[test]
    fn cmp_branch_matches_binary_jump_for_scalar_cases() {
        use BinaryOperator::*;

        let cases = [
            (Equal, Value::Int(1), Value::Int(1)),
            (Equal, Value::Int(1), Value::Int(2)),
            (EqualDot, Value::Int(2), Value::Int(2)),
            (NotEqual, Value::Int(1), Value::Int(2)),
            (NotEqualDot, Value::Int(2), Value::Int(2)),
            (Lt, Value::Int(1), Value::Int(2)),
            (Lt, Value::Int(2), Value::Int(1)),
            (Lte, Value::Int(2), Value::Int(2)),
            (Gt, Value::Int(3), Value::Int(2)),
            (Gte, Value::Int(2), Value::Int(3)),
            (Lt, Value::float(1.5), Value::float(2.0)),
            (Gt, Value::float(1.5), Value::float(2.0)),
        ];

        for (op, left, right) in cases {
            let expected = run_vm(vec![
                Instruction::load_const(left.clone()),
                Instruction::load_const(right.clone()),
                Instruction::binary_op(op, Operand::Stack, Operand::Stack),
                Instruction::JumpIfFalse(6),
                Instruction::load_const(Value::Int(42)),
                Instruction::Return,
                Instruction::load_const(Value::Int(99)),
                Instruction::Return,
            ]);
            let fused_const = run_vm(vec![
                Instruction::jump_if_cmp_false(
                    op,
                    Operand::const_val(left.clone()),
                    Operand::const_val(right.clone()),
                    3,
                ),
                Instruction::load_const(Value::Int(42)),
                Instruction::Return,
                Instruction::load_const(Value::Int(99)),
                Instruction::Return,
            ]);
            let fused_stack = run_vm(vec![
                Instruction::load_const(left.clone()),
                Instruction::load_const(right.clone()),
                Instruction::jump_if_cmp_false(op, Operand::Stack, Operand::Stack, 5),
                Instruction::load_const(Value::Int(42)),
                Instruction::Return,
                Instruction::load_const(Value::Int(99)),
                Instruction::Return,
            ]);

            assert_eq!(fused_const, expected, "{op:?} const");
            assert_eq!(fused_stack, expected, "{op:?} stack");
        }
    }

    #[test]
    fn tail_call_local_refreshes_instruction_snapshot() {
        let callee = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(1)),
                Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
                Instruction::Return,
            ],
        );
        let caller = make_fn(
            Some(&["f"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(41)),
                Instruction::TailCallLocal(0, 1),
                Instruction::Return,
            ],
        );
        let mut vm = Vm::new(Vec::new());

        let result = vm
            .call(&caller, BuiltinFnArgs::from(callee))
            .expect("tail call through local function");

        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn booland_lazy_short_circuits_on_false() {
        // left=false → push false, jump over right operand + BinaryOp to Return
        let result = run_vm(vec![
            Instruction::load_const(Value::Bool(false)),
            Instruction::BoolAndLazy(4), // short-circuit → jump to Return
            Instruction::load_const(Value::Bool(true)),
            Instruction::BoolCombine(BoolOperator::And),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Bool(false));
    }

    #[test]
    fn booland_lazy_no_short_circuit_on_true() {
        // left=true → continue to evaluate right operand
        let result = run_vm(vec![
            Instruction::load_const(Value::Bool(true)),
            Instruction::BoolAndLazy(4),
            Instruction::load_const(Value::Bool(true)),
            Instruction::BoolCombine(BoolOperator::And),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn boolor_lazy_short_circuits_on_true() {
        // left=true → push true, jump over right operand + BinaryOp to Return
        let result = run_vm(vec![
            Instruction::load_const(Value::Bool(true)),
            Instruction::BoolOrLazy(4),
            Instruction::load_const(Value::Bool(false)),
            Instruction::BoolCombine(BoolOperator::Or),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn boolor_lazy_no_short_circuit_on_false() {
        // left=false → continue to evaluate right operand
        let result = run_vm(vec![
            Instruction::load_const(Value::Bool(false)),
            Instruction::BoolOrLazy(4),
            Instruction::load_const(Value::Bool(true)),
            Instruction::BoolCombine(BoolOperator::Or),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Bool(true));
    }

    // --- JumpIfGE tests ---

    #[test]
    fn jumpifge_int_left_greater() {
        // JumpIfGE: Jump if left >= right. 5 >= 3 → jump.
        let result = run_vm(vec![
            Instruction::load_const(Value::Int(5)),
            Instruction::load_const(Value::Int(3)),
            Instruction::JumpIfGE(5), // 5 >= 3 → jump to pc=5 (past LoadConst(99) and Return)
            Instruction::load_const(Value::Int(99)),
            Instruction::Return,
            Instruction::load_const(Value::Int(1)),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn jumpifge_int_left_equal() {
        // JumpIfGE: Jump if left >= right. 3 >= 3 → jump (equal counts).
        let result = run_vm(vec![
            Instruction::load_const(Value::Int(3)),
            Instruction::load_const(Value::Int(3)),
            Instruction::JumpIfGE(5),
            Instruction::load_const(Value::Int(99)),
            Instruction::Return,
            Instruction::load_const(Value::Int(1)),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Int(1));
    }

    #[test]
    fn jumpifge_int_no_jump() {
        // JumpIfGE: Jump if left >= right. 2 >= 5 → no jump, fall through.
        let result = run_vm(vec![
            Instruction::load_const(Value::Int(2)),
            Instruction::load_const(Value::Int(5)),
            Instruction::JumpIfGE(5),
            Instruction::load_const(Value::Int(42)),
            Instruction::Return,
            Instruction::load_const(Value::Int(99)),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn jumpifge_int_negative() {
        // JumpIfGE with negative ints. -1 >= -5 → jump.
        let result = run_vm(vec![
            Instruction::load_const(Value::Int(-1)),
            Instruction::load_const(Value::Int(-5)),
            Instruction::JumpIfGE(5),
            Instruction::load_const(Value::Int(99)),
            Instruction::Return,
            Instruction::load_const(Value::Int(1)),
            Instruction::Return,
        ]);
        assert_eq!(result, Value::Int(1));
    }

    // --- Trace (value provenance) tests ---

    fn setup_trace_vm(insts: Vec<Instruction>) -> Vm {
        let len = insts.len();
        let mut vm = Vm::new(insts);
        vm.runtime_io.set_stderr(Box::new(SinkOutput));
        vm.debug_log.set_output(vm.runtime_io.stderr_output());
        vm.color_mode = crate::style::ColorMode::Never;
        vm.runtime_debug_info = true;
        let file_id = vm.debug_info.new_file("<trace-test>", "test source text");
        let chunk = vm.debug_info.new_chunk("<trace-test>", file_id, len);
        // Give every PC a dummy span so trace probes aren't silently dropped.
        let meta = vm.debug_info.expect_chunk_mut(chunk);
        for pc in 0..len {
            meta.line_table.set_exact_span(
                pc,
                crate::wqdb::data::Span {
                    file_id,
                    start: 0,
                    end: 4,
                },
            );
        }
        vm.current_chunk = Some(chunk);
        vm
    }

    #[test]
    fn trace_begin_increments_depth() {
        let insts = vec![
            Instruction::TraceBegin,
            Instruction::load_const(Value::Int(42)),
            Instruction::Return,
        ];
        let len = insts.len();
        let mut vm = setup_trace_vm(insts);
        let mut interpreter = VanillaInterpreter;
        let _ = interpreter.interpret(&mut vm, len).expect("execute");
        assert_eq!(vm.trace_depth, 1);
        assert_eq!(vm.trace_bases.len(), 1);
        assert!(vm.trace_buf.is_empty()); // LoadConst is not interesting
    }

    #[test]
    fn trace_records_binary_op() {
        // No Debug to drain
        let insts = vec![
            Instruction::TraceBegin,
            Instruction::load_const(Value::Int(1)),
            Instruction::load_const(Value::Int(2)),
            Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
            Instruction::Return,
        ];
        let len = insts.len();
        let mut vm = setup_trace_vm(insts);
        let mut interpreter = VanillaInterpreter;
        let result = interpreter.interpret(&mut vm, len).expect("execute");
        assert_eq!(result, Value::Int(3));
        assert_eq!(vm.trace_depth, 1); // Return doesn't pop trace_depth
        assert_eq!(vm.trace_buf.len(), 1);
        assert_eq!(vm.trace_buf[0].value_excerpt, "3");
        assert!(vm.trace_buf[0].debug_kind.contains("int"));
    }

    #[test]
    fn trace_debug_drains_buf_and_pops_base() {
        let insts = vec![
            Instruction::TraceBegin,
            Instruction::load_const(Value::Int(1)),
            Instruction::load_const(Value::Int(2)),
            Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
            Instruction::Debug,
            Instruction::Return,
        ];
        let len = insts.len();
        let mut vm = setup_trace_vm(insts);
        let mut interpreter = VanillaInterpreter;
        let result = interpreter.interpret(&mut vm, len).expect("execute");
        assert_eq!(result, Value::Int(3));
        assert_eq!(vm.trace_depth, 0); // Debug decrements
        assert!(vm.trace_buf.is_empty()); // Debug drains
        assert!(vm.trace_bases.is_empty()); // Debug pops base
    }

    #[test]
    fn trace_index_load_probe() {
        let insts = vec![
            Instruction::TraceBegin,
            Instruction::load_const(Value::Int(1)),
            Instruction::IndexLoadVar("xs".into()),
            Instruction::Return,
        ];
        let len = insts.len();
        let mut vm = setup_trace_vm(insts);
        vm.assign_global_and_slot("xs", Value::IntList(Arc::new(vec![10, 20, 30])));
        let mut interpreter = VanillaInterpreter;
        let result = interpreter.interpret(&mut vm, len).expect("execute");
        assert_eq!(result, Value::Int(20));
        assert_eq!(vm.trace_buf.len(), 1);
        assert_eq!(vm.trace_buf[0].value_excerpt, "20");
    }

    #[test]
    fn trace_begin_does_not_flush_pretrace_probe() {
        let insts = vec![
            Instruction::load_const(Value::Int(1)),
            Instruction::load_const(Value::Int(2)),
            Instruction::binary_op(BinaryOperator::Add, Operand::Stack, Operand::Stack),
            Instruction::TraceBegin,
            Instruction::load_const(Value::Int(4)),
            Instruction::Return,
        ];
        let len = insts.len();
        let mut vm = setup_trace_vm(insts);
        let mut interpreter = VanillaInterpreter;
        let result = interpreter.interpret(&mut vm, len).expect("execute");
        assert_eq!(result, Value::Int(4));
        assert_eq!(
            vm.trace_buf.len(),
            0,
            "trace should not record interesting instructions that ran before TraceBegin"
        );
    }
}

#[cfg(test)]
mod call_safety {
    use std::sync::Arc;

    use indexmap::IndexMap;

    use crate::builtins::{BuiltinFnArgs, Builtins};
    use crate::interpret::Interpreter;
    use crate::interpret::vanilla::{InterpretPoll, VanillaInterpreter};
    use crate::value::func::FunctionData;
    use crate::value::{Value, cell};
    use crate::vm::call::CallSpec;
    use crate::vm::inst::Instruction;
    use crate::vm::{FunctionActivation, InlineCache, Slot, Vm};
    use crate::wqerror::WqErrorType;

    fn make_fn(params: Option<&[&str]>, locals: u16, instructions: Vec<Instruction>) -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: params.map(|names| {
                Arc::<[String]>::from(names.iter().map(|s| s.to_string()).collect::<Vec<_>>())
            }),
            named_params: None,
            locals,
            isolated_module: false,
            instructions: instructions.into(),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    fn set_active_function(vm: &mut Vm, locals: Vec<Slot>, callable: Value) {
        vm.active_function = Some(FunctionActivation {
            locals,
            captures: cell::empty_cells(),
            callable,
        });
    }

    fn dict_with_method(method: Value) -> Value {
        let mut map = IndexMap::new();
        map.insert(Arc::<str>::from("f"), method);
        Value::Dict(Arc::new(map))
    }

    fn run_vm_again(vm: &mut Vm, limit: usize) -> Value {
        vm.pc = 0;
        vm.returned = false;
        let mut interpreter = VanillaInterpreter;
        interpreter.interpret(vm, limit).expect("execute")
    }

    #[test]
    fn pure_user_call_avoids_a_physical_frame() {
        let add_one = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
                    crate::vm::inst::Operand::Local(0),
                    crate::vm::inst::Operand::Const(Box::new(Value::Int(1))),
                ),
                Instruction::Return,
            ],
        );
        let instructions = vec![
            Instruction::load_const(add_one),
            Instruction::load_const(Value::Int(41)),
            Instruction::CallAnon(1),
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);
        vm.max_call_depth = 0;

        let result = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect("pure user call should not enter a physical frame");

        assert_eq!(result, Value::Int(42));
        assert!(vm.inline_cache[2].pure_call.is_some());
    }

    #[test]
    fn pure_user_call_error_deoptimizes_to_a_physical_frame() {
        let invalid_index = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(0)),
                Instruction::Index,
                Instruction::Return,
            ],
        );
        let instructions = vec![
            Instruction::load_const(invalid_index),
            Instruction::load_const(Value::Int(41)),
            Instruction::CallAnon(1),
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);
        vm.max_call_depth = 0;

        let error = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect_err("pure-plan errors should use the authoritative framed call");

        assert_eq!(error.err_type, WqErrorType::Recursion);
    }

    #[test]
    fn sliced_interpretation_resumes_inside_nested_calls() {
        let add_one = make_fn(
            Some(&["x"]),
            1,
            vec![
                Instruction::LoadLocal(0),
                Instruction::load_const(Value::Int(1)),
                Instruction::binary_op(
                    crate::ast::BinaryOperator::Add,
                    crate::vm::inst::Operand::Stack,
                    crate::vm::inst::Operand::Stack,
                ),
                Instruction::Return,
            ],
        );
        let instructions = vec![
            Instruction::LoadVar("f".into()),
            Instruction::load_const(Value::Int(41)),
            Instruction::CallUser("f".into(), 1),
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);
        vm.assign_global_and_slot("f", add_one);
        let mut interpreter = VanillaInterpreter;
        let mut yields = 0;

        let result = loop {
            match interpreter
                .interpret_slice(&mut vm, limit, 1)
                .expect("slice should execute")
            {
                InterpretPoll::Yielded { work_units } => {
                    assert_eq!(work_units, 1);
                    yields += 1;
                }
                InterpretPoll::AwaitingInput { .. } => {
                    panic!("test program should not request input")
                }
                InterpretPoll::Paused(_) => panic!("debugger should be disabled"),
                InterpretPoll::Ready(value) => break value,
            }
        };

        assert_eq!(result, Value::Int(42));
        assert!(
            yields >= 6,
            "expected yields in caller and callee, got {yields}"
        );
        assert!(vm.caller_frames.is_empty());
        assert!(vm.active_function.is_none());
    }

    #[test]
    fn sliced_interpretation_preserves_try_across_call_frames() {
        let bad = make_fn(
            None,
            0,
            vec![Instruction::LoadVar("missing".into()), Instruction::Return],
        );
        let instructions = vec![
            Instruction::Try(2),
            Instruction::LoadVar("bad".into()),
            Instruction::CallUser("bad".into(), 0),
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);
        vm.assign_global_and_slot("bad", bad);
        let mut interpreter = VanillaInterpreter;

        let result = loop {
            match interpreter
                .interpret_slice(&mut vm, limit, 1)
                .expect("try should catch the sliced call error")
            {
                InterpretPoll::Yielded { .. } => {}
                InterpretPoll::AwaitingInput { .. } => {
                    panic!("test program should not request input")
                }
                InterpretPoll::Paused(_) => panic!("debugger should be disabled"),
                InterpretPoll::Ready(value) => break value,
            }
        };

        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        assert_eq!(result.first(), Some(&Value::Tag(Arc::from("error"))));
        assert!(vm.caller_frames.is_empty());
    }

    #[test]
    fn call_non_callable_leaves_stack_unchanged() {
        let mut vm = Vm::new(vec![Instruction::Return]);
        vm.stack.push(Value::Int(42));
        let base = vm.stack.len();

        let result = vm.call(
            &Value::Int(1), // not callable
            BuiltinFnArgs::from(vec![Value::Int(10), Value::Int(20)]),
        );

        assert!(result.is_err());
        assert_eq!(
            vm.stack.len(),
            base,
            "stack must be unchanged after calling a non-callable"
        );
        assert_eq!(vm.stack.last(), Some(&Value::Int(42)));
    }

    #[test]
    fn call_arity_error_cleans_up_stack() {
        let func = make_fn(
            Some(&["x"]),
            1,
            vec![Instruction::LoadLocal(0), Instruction::Return],
        );
        let mut vm = Vm::new(vec![Instruction::Return]);
        vm.stack.push(Value::Int(99));
        let base = vm.stack.len();

        // Call with 2 args but function expects 1
        let result = vm.call(
            &func,
            BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
        );

        assert!(result.is_err());
        assert_eq!(
            vm.stack.len(),
            base,
            "stack must be cleaned up on arity error"
        );
        assert_eq!(vm.stack.last(), Some(&Value::Int(99)));
    }

    #[test]
    fn recursion_limit_restores_current_chunk() {
        let self_ref = make_fn(
            None,
            0,
            vec![
                Instruction::LoadSelf,
                Instruction::CallAnon(0),
                Instruction::Return,
            ],
        );

        let mut vm = Vm::new(vec![Instruction::Return]);
        vm.max_call_depth = 1;
        vm.runtime_debug_info = true;
        let file_id = vm.debug_info.new_file("<test>", "");
        let chunk = vm.debug_info.new_chunk("<test>", file_id, 1);
        vm.current_chunk = Some(chunk);
        let saved_chunk = vm.current_chunk;

        let result = vm.call(&self_ref, BuiltinFnArgs::from(vec![]));

        assert!(result.is_err());
        assert_eq!(
            vm.current_chunk, saved_chunk,
            "current_chunk must be restored after recursion-limit error"
        );
    }

    #[test]
    fn tail_call_same_code_preserves_inline_cache() {
        let insts: Arc<[Instruction]> =
            Arc::from([Instruction::load_const(Value::Int(1)), Instruction::Return]);

        let mut vm = Vm::new(vec![Instruction::Return; 2]);
        vm.instructions = Arc::clone(&insts);
        vm.inline_cache = vec![InlineCache::default(); 2];
        vm.inline_cache[0].slot = Some(42);
        set_active_function(&mut vm, vec![Slot::default()], Value::empty_list());

        vm.prepare_tail(CallSpec {
            instructions: Arc::clone(&insts),
            params_len: None,
            locals: 0,
            captured: cell::empty_cells(),
            argc: 0,
            callee_name: None,
            dbg_chunk: None,
            callee: Value::empty_list(),
            cache_idx: None,
        })
        .expect("prepare_tail");

        assert_eq!(vm.inline_cache.len(), 2);
        assert_eq!(
            vm.inline_cache[0].slot,
            Some(42),
            "inline cache entry must survive same-code tail call"
        );
    }

    #[test]
    fn tail_call_local_count_error_preserves_vm_state() {
        let insts: Arc<[Instruction]> = Arc::from([Instruction::Return]);
        let mut vm = Vm::new(vec![Instruction::Return]);
        vm.stack.push(Value::Int(1));
        vm.stack.push(Value::Int(2));
        set_active_function(&mut vm, vec![Slot::default()], Value::empty_list());

        let result = vm.prepare_tail(CallSpec {
            instructions: insts,
            params_len: None,
            locals: 1,
            captured: cell::empty_cells(),
            argc: 2,
            callee_name: None,
            dbg_chunk: None,
            callee: Value::empty_list(),
            cache_idx: None,
        });

        assert!(result.is_err());
        assert_eq!(vm.stack, vec![Value::Int(1), Value::Int(2)]);
        let activation = vm
            .active_function
            .as_ref()
            .expect("function activation should remain");
        assert_eq!(activation.locals.len(), 1);
        assert!(activation.captures.is_empty());
    }

    #[test]
    fn builtin_value_call_uses_cached_id() {
        let mut vm = Vm::new(vec![Instruction::Return]);
        let abs = Value::builtin_function("abs", Builtins::ABS);

        let result = vm
            .call(&abs, BuiltinFnArgs::from(Value::Int(-3)))
            .expect("builtin value call");

        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn indexed_call_cache_revalidates_dict_entry_identity() {
        let first = make_fn(
            None,
            0,
            vec![Instruction::load_const(Value::Int(1)), Instruction::Return],
        );
        let second = make_fn(
            None,
            0,
            vec![Instruction::load_const(Value::Int(2)), Instruction::Return],
        );
        let mut vm = Vm::new(vec![
            Instruction::LoadVar("d".into()),
            Instruction::load_const(Value::Tag(Arc::<str>::from("f"))),
            Instruction::Index,
            Instruction::CallAnon(0),
            Instruction::Return,
        ]);
        vm.assign_global_and_slot("d", dict_with_method(first));
        let limit = vm.instructions.len();

        assert_eq!(run_vm_again(&mut vm, limit), Value::Int(1));
        vm.with_global_slot_mut("d", |target| {
            assert_eq!(
                target.assign_by_index(&Value::Tag(Arc::<str>::from("f")), second),
                Some(())
            );
        })
        .expect("global d should exist");

        assert_eq!(run_vm_again(&mut vm, limit), Value::Int(2));
    }

    #[test]
    fn local_call_cache_revalidates_slot_callable_identity() {
        let first = make_fn(
            None,
            0,
            vec![Instruction::load_const(Value::Int(1)), Instruction::Return],
        );
        let second = make_fn(
            None,
            0,
            vec![Instruction::load_const(Value::Int(2)), Instruction::Return],
        );
        let mut vm = Vm::new(vec![
            Instruction::LoadLocal(0),
            Instruction::CallLocal(0, 0),
            Instruction::Return,
        ]);
        set_active_function(&mut vm, vec![Slot::Value(first)], Value::empty_list());
        let limit = vm.instructions.len();

        assert_eq!(run_vm_again(&mut vm, limit), Value::Int(1));
        vm.current_locals_mut()
            .expect("active locals should remain")[0]
            .write(second);

        assert_eq!(run_vm_again(&mut vm, limit), Value::Int(2));
    }

    #[test]
    fn local_call_cache_stores_stamped_callable_with_debug_artifacts() {
        let func = make_fn(
            None,
            0,
            vec![Instruction::load_const(Value::Int(1)), Instruction::Return],
        );
        let mut vm = Vm::new(vec![
            Instruction::LoadLocal(0),
            Instruction::CallLocal(0, 0),
            Instruction::Return,
        ]);
        set_active_function(&mut vm, vec![Slot::Value(func)], Value::empty_list());
        vm.set_backtrace_enabled(true);
        let file = vm.debug_info.new_file("<test>", "f[]");
        vm.current_chunk = Some(vm.debug_info.new_chunk("<test>", file, 3));

        assert_eq!(run_vm_again(&mut vm, 3), Value::Int(1));

        let cached = vm.inline_cache[1]
            .call_target
            .as_ref()
            .expect("local call should cache its resolved target");
        assert!(cached.dbg_chunk.is_some());
        assert_eq!(
            cached
                .value
                .as_user_function()
                .and_then(|shape| shape.dbg_chunk),
            cached.dbg_chunk
        );
        assert_eq!(run_vm_again(&mut vm, 3), Value::Int(1));
    }

    #[test]
    fn named_arg_error_through_call_leaves_stack_unchanged() {
        use std::sync::Arc;

        use crate::vm::inst::NamedArgMeta;

        // Function with one positional param "x", one named param "y", plus mask slot
        let func = Value::CompiledFunction(Arc::new(FunctionData {
            params: Some(Arc::from(["x".to_string()])),
            named_params: Some(Arc::from([Arc::<str>::from("y")])),
            locals: 3, // x, y, mask
            isolated_module: false,
            instructions: Arc::from([Instruction::Return]),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }));

        let mut vm = Vm::new(vec![Instruction::Return]);
        vm.stack.push(Value::Int(99));
        let base = vm.stack.len();

        // Set up named meta with a bad named arg "z" (not a named param)
        vm.pending_named_meta = Some(Arc::new(NamedArgMeta {
            pos_count: 1,
            named: Box::new([(1u16, Arc::<str>::from("z"))]),
        }));

        let result = vm.call(
            &func,
            BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
        );

        assert!(result.is_err());
        assert_eq!(
            vm.stack.len(),
            base,
            "stack must be cleaned up on named arg error"
        );
        assert_eq!(vm.stack.last(), Some(&Value::Int(99)));
    }

    #[test]
    fn invoke_user_stack_underflow_preserves_vm_state() {
        use std::borrow::Cow;

        let func = make_fn(
            Some(&["x"]),
            1,
            vec![Instruction::LoadLocal(0), Instruction::Return],
        );

        let mut vm = Vm::new(vec![
            Instruction::load_const(Value::Int(1)),
            Instruction::Return,
        ]);
        let saved_instructions = Arc::clone(&vm.instructions);
        let saved_pc = vm.pc;
        vm.stack.push(Value::Int(42));
        let saved_stack_len = vm.stack.len();

        // invoke_user with argc=2 but only 1 value on stack
        let result = vm.invoke_user(&func, 2, Some(Cow::Borrowed("<test>")));

        assert!(result.is_err());
        assert!(Arc::ptr_eq(&vm.instructions, &saved_instructions));
        assert_eq!(vm.pc, saved_pc);
        assert_eq!(vm.stack.len(), saved_stack_len);
        assert_eq!(vm.stack.last(), Some(&Value::Int(42)));
    }

    #[test]
    fn caught_call_error_does_not_leave_backtrace_snapshot() {
        let bad = make_fn(
            None,
            0,
            vec![Instruction::LoadVar("missing".into()), Instruction::Return],
        );
        let instructions = vec![
            Instruction::Try(2),
            Instruction::LoadVar("bad".into()),
            Instruction::CallUser("bad".into(), 0),
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);
        vm.set_backtrace_enabled(true);
        let file_id = vm.debug_info.new_file("<test>", "@t bad[]");
        vm.current_chunk = Some(vm.debug_info.new_chunk("<test>", file_id, limit));
        vm.assign_global_and_slot("bad", bad);

        let result = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect("try should catch missing global error");

        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        assert_eq!(result.first(), Some(&Value::Tag(Arc::from("error"))));
        assert!(vm.last_crash.is_none());
    }

    #[test]
    fn caught_trace_error_restores_trace_scope() {
        let instructions = vec![
            Instruction::Try(3),
            Instruction::TraceBegin,
            Instruction::binary_op(
                crate::ast::BinaryOperator::Divide,
                crate::vm::inst::Operand::Const(Box::new(Value::Int(1))),
                crate::vm::inst::Operand::Const(Box::new(Value::Int(0))),
            ),
            Instruction::Debug,
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);

        let result = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect("try should catch divide error");

        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        assert_eq!(result.first(), Some(&Value::Tag(Arc::from("error"))));
        assert_eq!(vm.trace_depth, 0);
        assert!(vm.trace_bases.is_empty());
        assert!(vm.trace_buf.is_empty());
    }

    #[test]
    fn invalid_try_region_returns_vm_error() {
        let instructions = vec![Instruction::Try(usize::MAX), Instruction::Return];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);

        let err = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect_err("invalid try region should fail safely");

        assert_eq!(err.err_type, WqErrorType::Vm);
        assert_eq!(err.msg.as_deref(), Some("invalid try region"));
    }

    #[test]
    fn try_does_not_catch_vm_error() {
        let instructions = vec![
            Instruction::Try(1),
            Instruction::Try(usize::MAX),
            Instruction::Return,
        ];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);

        let err = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect_err("try must not catch VM errors");

        assert_eq!(err.err_type, WqErrorType::Vm);
        assert_eq!(err.msg.as_deref(), Some("invalid try region"));
    }

    #[test]
    fn empty_try_region_does_not_consume_existing_stack_value() {
        let instructions = vec![Instruction::Try(0), Instruction::Return];
        let limit = instructions.len();
        let mut vm = Vm::new(instructions);
        vm.stack.push(Value::Int(42));

        let result = VanillaInterpreter
            .interpret(&mut vm, limit)
            .expect("empty try should succeed with an empty list");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::Tag(Arc::from("ok")),
                Value::empty_list(),
            ]))
        );
        assert_eq!(vm.stack, vec![Value::Int(42)]);
    }
}
