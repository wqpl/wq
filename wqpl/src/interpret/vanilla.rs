use std::sync::{Arc, Mutex};

use indexmap::{IndexMap, IndexSet};
use smallvec::SmallVec;

use crate::interpret::{Interpreter, InterpreterHook, NO_OP_HOOK};
use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::session::stdio::wqstderr_println;
use crate::value::cmp::eval_cmp_chain;
use crate::value::func::{ClosureData, FunctionData};
use crate::value::{Value, WqResult, eval_binary, eval_unary};
use crate::vm::call::{
    CallSpec, LocalCallable, PeekLocalCallable, PeekLocalUser, peek_local_callable,
};
use crate::vm::inst::{Capture, Instruction};
use crate::vm::trace::TraceRecord;
use crate::vm::{Frame, Vm, ensure_stack_len, last_clone_stack, pop1_stack, pop2_stack};
use crate::wqdb::build::{
    apply_stmt_debug_exact_offs, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
};
use crate::wqdb::data::{ChunkId, CodeLoc, DebugChunkSpec};
use crate::wqerror::{WqError, WqErrorType};

mod call;
mod debug;
mod mutate;
mod operand;
mod range;
mod target;

use call::*;
use debug::*;
use mutate::*;
use operand::*;
use range::*;
use target::*;

pub(crate) struct VanillaInterpreter;
const NAME: &str = "vanilla";

pub(crate) type Sv4 = SmallVec<[Value; 4]>;

impl Interpreter for VanillaInterpreter {
    fn interpret(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        if limit > vm.instructions.len() {
            return Err(vm_err(format!("limit out of bounds: {limit}")));
        }
        let hooks: &dyn InterpreterHook = match vm.hooks {
            Some(ptr) => unsafe { ptr.as_ref() },
            None => &NO_OP_HOOK,
        };
        let mut limit = limit;
        let mut inst_ptr = vm.instructions.as_ptr();
        let mut last_probe_pc: Option<usize> = None;
        'exec: loop {
            while vm.pc < limit {
                // Record a probe for the previously executed interesting
                // instruction.  We record *here* (one iteration late) so that
                // call instructions which `continue 'exec` after a cache hit
                // still get probed: marking is done before dispatch below.
                if vm.trace_depth > 0
                    && let Some(prev) = last_probe_pc.take()
                {
                    record_trace_probe(vm, prev);
                }
                if vm.wqdb.enabled {
                    let here = CodeLoc {
                        chunk: vm.current_chunk,
                        pc: vm.pc,
                    };
                    let depth = vm.call_depth();
                    // wqdb on_pause hook
                    if vm.wqdb.should_pause_at(&vm.debug_info, here, depth) {
                        let cb = vm.wqdb.on_pause;
                        vm.wqdb.note_pause(here);
                        if let Some(f) = cb {
                            f(vm);
                        }
                    }
                }
                let idx = vm.pc;
                vm.pc += 1;
                // Decouple lifetime to avoid borrowing vm during instruction execution
                let op_ptr = &vm.instructions[idx] as *const Instruction;
                let op = unsafe { &*op_ptr };
                // Mark for trace probe BEFORE dispatch.  Some call arms
                // `continue 'exec` after a synchronous push, skipping any
                // post-match check — the next iteration's top-of-loop flush
                // handles those uniformly.
                if op.is_trace_interesting() {
                    last_probe_pc = Some(idx);
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

                        if vm.builtins.has_function(name) {
                            vm.stack
                                .push(Value::BuiltinFunction(Arc::from(name.as_ref())));
                            continue;
                        }

                        if vm.builtins.is_disabled_name(name) {
                            return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        ))
                        .attach_note(format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        )));
                        }

                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        )));
                    }
                    Instruction::LoadLocal(i) => {
                        let slot = usize::from(*i);
                        let val = vm.locals.last().and_then(|f| f.get(slot)).ok_or_else(|| {
                            vm.attach_local_slot_note(slot, vm_err("invalid local slot"))
                        })?;
                        vm.stack.push(val.read());
                    }
                    Instruction::LoadCapture(i) => {
                        let idx = usize::from(*i);
                        let cap_num = *i;
                        let cell = vm
                            .captures
                            .last()
                            .and_then(|c| c.get(idx))
                            .ok_or_else(|| vm_err(format!("invalid capture slot {cap_num}")))?;
                        vm.stack
                            .push(cell.lock().expect("poisoned capture").clone());
                    }
                    Instruction::LoadClosure(payload) => {
                        let locals = payload.locals;
                        let captures = &payload.captures;
                        let mut captured_vals = Vec::with_capacity(captures.len());
                        for cap in captures {
                            match cap {
                                Capture::Local(slot) => {
                                    let slot_idx = usize::from(*slot);
                                    let val = if let Some(parent) = vm.locals.last() {
                                        parent
                                            .get(slot_idx)
                                            .map(|s| s.read())
                                            .unwrap_or_else(Value::unit)
                                    } else {
                                        Value::unit()
                                    };
                                    captured_vals.push(Arc::new(Mutex::new(val)));
                                }
                                Capture::LocalShared(slot) => {
                                    let slot_idx = usize::from(*slot);
                                    let cell = if let Some(parent) = vm.locals.last_mut() {
                                        parent
                                            .get_mut(slot_idx)
                                            .map(|s| s.ensure_cell())
                                            .unwrap_or_else(|| Arc::new(Mutex::new(Value::unit())))
                                    } else {
                                        Arc::new(Mutex::new(Value::unit()))
                                    };
                                    captured_vals.push(cell);
                                }
                                Capture::Outer(i) => {
                                    let cap_idx = usize::from(*i);
                                    let cell = vm
                                        .captures
                                        .last()
                                        .and_then(|c| c.get(cap_idx))
                                        .cloned()
                                        .unwrap_or_else(|| Arc::new(Mutex::new(Value::unit())));
                                    captured_vals.push(cell);
                                }
                                Capture::Global(name, span) => {
                                    let val = if let Some(v) = vm.lookup_global(name) {
                                        v
                                    } else {
                                        let mut err = not_bound_err(format!(
                                            "'{name}' is not bound as a global or a local"
                                        ))
                                        .attach_note("when loading closure");
                                        if let Some((start, end)) = span {
                                            let base_offs = vm.resolved_debug_base_offset();
                                            let abs_start = start + base_offs;
                                            let abs_end = end + base_offs;
                                            if let Some(sf) = vm
                                                .debug_info
                                                .file(vm.debug_info.chunk(vm.current_chunk).file_id)
                                            {
                                                err = err
                                                    .span(Some((abs_start, abs_end)))
                                                    .source_ctx(
                                                        sf.text.to_string(),
                                                        sf.path.to_string(),
                                                    );
                                            }
                                        }
                                        return Err(err);
                                    };
                                    captured_vals.push(Arc::new(Mutex::new(val)));
                                }
                            }
                        }
                        // Register a debug chunk for this closure's code (wqdb or bt mode)
                        let mut chunk_opt: Option<ChunkId> = None;
                        let instructions = &payload.instructions;
                        let dbg_stmt_spans = &payload.dbg_stmt_spans;
                        let dbg_pc_spans = &payload.dbg_pc_spans;
                        let dbg_stmt_marks = &payload.dbg_stmt_marks;
                        let dbg_local_names = &payload.dbg_local_names;
                        let params = &payload.params;
                        let source_base_offset = vm.resolved_debug_base_offset();
                        if vm.debug_artifacts_enabled() {
                            let file_id = vm.debug_info.chunk(vm.current_chunk).file_id;
                            let id = vm.debug_info.new_chunk("<fn>", file_id, instructions.len());
                            if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
                                eprintln!(
                                    "[wqdb]: LoadClosure new chunk={id:?} file_id={file_id} instructions={} base_offset={}",
                                    instructions.len(),
                                    source_base_offset,
                                );
                            }
                            {
                                let base_offs = source_base_offset;
                                let table = &mut vm.debug_info.chunk_mut(id).line_table;
                                if !dbg_pc_spans.is_empty() && !dbg_stmt_marks.is_empty() {
                                    apply_stmt_debug_exact_offs(
                                        table,
                                        file_id,
                                        dbg_pc_spans,
                                        dbg_stmt_marks,
                                        base_offs,
                                    );
                                } else if !dbg_stmt_spans.is_empty() {
                                    apply_stmt_spans_exact_offs(
                                        table,
                                        instructions.as_ref(),
                                        file_id,
                                        dbg_stmt_spans,
                                        base_offs,
                                    );
                                } else {
                                    mark_stmt_heuristic(table, instructions.as_ref());
                                }
                            }
                            if !dbg_local_names.is_empty() {
                                vm.debug_info.chunk_mut(id).local_names =
                                    Some(dbg_local_names.iter().cloned().collect());
                            } else if let Some(ps) = params.as_ref() {
                                vm.debug_info.chunk_mut(id).local_names =
                                    Some(ps.iter().cloned().collect());
                            }
                            chunk_opt = Some(id);
                        }
                        hooks.on_closure_capture_alloc(&|| captured_vals.len());
                        vm.stack.push(Value::Closure(Arc::new(ClosureData {
                            params: params.clone(),
                            named_params: payload.named_params.clone(),
                            locals,
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

                    Instruction::LoadNamedArgsProvided(bit) => {
                        let mask = vm
                            .stack
                            .pop()
                            .ok_or_else(|| vm_err("CheckNamedProvided: stack empty"))?;
                        let provided = match mask {
                            Value::Int(n) => (n & (1i64 << bit)) != 0,
                            _ => false,
                        };
                        vm.stack.push(Value::Bool(provided));
                    }
                    Instruction::LoadVarExists(name) => {
                        vm.stack
                            .push(Value::Bool(vm.lookup_global_slot(name).is_some()));
                    }
                    Instruction::LoadSelf => {
                        let me = vm
                            .current_closure_stack
                            .last()
                            .ok_or_else(|| vm_err("LoadSelf outside fn"))?;
                        vm.stack.push(me.clone());
                    }

                    Instruction::StoreVar(name) => store_var_impl(vm, idx, name, false)?,
                    Instruction::StoreVarKeep(name) => store_var_impl(vm, idx, name, true)?,
                    Instruction::StoreLocal(i) => store_local_impl(vm, *i, false)?,
                    Instruction::StoreLocalKeep(i) => store_local_impl(vm, *i, true)?,
                    Instruction::StoreCaptureKeep(i) => {
                        let slot = usize::from(*i);
                        let slot_num = *i;
                        let val = last_clone_stack(&vm.stack, || {
                            format!("store into capture slot {slot_num}")
                        })?;
                        let cell = vm
                            .captures
                            .last()
                            .and_then(|c| c.get(slot))
                            .ok_or_else(|| vm_err(format!("invalid capture slot {slot_num}")))?;
                        *cell.lock().expect("poisoned capture") = val;
                    }

                    Instruction::BinaryOp(data) => {
                        let op = data.op;
                        let right = resolve_operand(vm, idx, &data.right, 1, hooks)
                            .map_err(|e| e.src(format!("binary op {op:?} right operand")))?;
                        let left = resolve_operand(vm, idx, &data.left, 0, hooks)
                            .map_err(|e| e.src(format!("binary op {op:?} left operand")))?;
                        vm.stack.push(eval_binary(&op, &left, &right)?);
                    }
                    Instruction::Cat(n) => {
                        let count = *n;
                        ensure_stack_len(&vm.stack, count, || "cat operands".into())?;
                        let base = vm.stack.len() - count;
                        let mut items = Vec::with_capacity(count);
                        items.extend(vm.stack.drain(base..));
                        vm.stack.push(Value::cat_many(items));
                    }
                    Instruction::UnaryOp(data) => {
                        let op = data.op;
                        let val = resolve_operand(vm, idx, &data.operand, 0, hooks)
                            .map_err(|e| e.src(format!("unary op {op:?}")))?;
                        vm.stack.push(eval_unary(&op, &val)?);
                    }

                    Instruction::CallBuiltinId(id, argc) => {
                        let result = vm.invoke_bfn_id(*id, *argc)?;
                        vm.stack.push(result);
                    }
                    Instruction::CallUser(name, argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc, || format!("fn '{name}' args"))?;
                        if dispatch_user_call(
                            vm,
                            idx,
                            name,
                            argc,
                            invoke_spec_push,
                            invoke_user_named,
                            hooks,
                        )? {
                            continue 'exec;
                        }
                    }
                    Instruction::CallAnon(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "callable + args".into())?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_anon_call(vm, idx, &func, argc, invoke_user_push)? {
                            continue 'exec;
                        }
                    }
                    Instruction::CallLocal(slot, argc) => {
                        let argc = *argc;
                        let slot_num = *slot;
                        let slot_usize = usize::from(slot_num);
                        ensure_stack_len(&vm.stack, argc, || {
                            format!("local call slot {slot_num} args")
                        })
                        .map_err(|e| vm.attach_local_slot_note(slot_usize, e))?;
                        match self.resolve_local_callable(vm, idx, slot_num, slot_usize)? {
                            LocalCallable::Func {
                                value,
                                params_len,
                                locals,
                                instructions,
                                captured,
                                dbg_chunk,
                                name_hint,
                            } => {
                                let res = vm.invoke_spec(CallSpec {
                                    instructions,
                                    params_len,
                                    locals,
                                    captured,
                                    argc: argc as u32,
                                    callee_name: CallSpec::name_hint(name_hint.as_deref()),
                                    dbg_chunk,
                                    callee: value,
                                })?;
                                vm.stack.push(res);
                            }
                            LocalCallable::Builtin(name) => {
                                let result = vm.invoke_bfn_name(&name, argc)?;
                                vm.stack.push(result);
                            }
                        }
                    }

                    Instruction::TailCallUser(name, argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc, || format!("fn '{name}' args"))?;
                        if dispatch_user_call(
                            vm,
                            idx,
                            name,
                            argc,
                            prepare_tail,
                            tail_invoke_user_named,
                            hooks,
                        )? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailCallAnon(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "callable + args".into())?;
                        let func = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_anon_call(vm, idx, &func, argc, tail_invoke_user)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailCallLocal(slot, argc) => {
                        let argc = *argc;
                        let slot_num = *slot;
                        let slot_usize = usize::from(slot_num);
                        ensure_stack_len(&vm.stack, argc, || {
                            format!("local call slot {slot_num} args")
                        })
                        .map_err(|e| vm.attach_local_slot_note(slot_usize, e))?;
                        match self.resolve_local_callable(vm, idx, slot_num, slot_usize)? {
                            LocalCallable::Func {
                                value,
                                params_len,
                                locals,
                                instructions,
                                captured,
                                dbg_chunk,
                                name_hint,
                            } => {
                                vm.push_tail_call_frame(Frame {
                                    chunk: vm.current_chunk,
                                    pc: idx,
                                    func_name: Arc::from(vm.func_name_for_chunk(vm.current_chunk)),
                                });
                                vm.prepare_tail(CallSpec {
                                    instructions,
                                    params_len,
                                    locals,
                                    captured,
                                    argc: argc as u32,
                                    callee_name: CallSpec::name_hint(name_hint.as_deref()),
                                    dbg_chunk,
                                    callee: value,
                                })?;
                                continue;
                            }
                            LocalCallable::Builtin(name) => {
                                let result = vm.invoke_bfn_name(&name, argc)?;
                                vm.stack.push(result);
                            }
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
                            return Err(not_bound_err(format!("'{name}' has not been bound to a value"))
                                .attach_note(format!("a builtin named '{name}' exists but is disabled in the current preset")));
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
                    Instruction::IndexLoadCapture(slot) => {
                        let slot = usize::from(*slot);
                        let idx_val = pop1_stack(&mut vm.stack, || "index".into())?;
                        let target = read_capture_target(vm, slot)?;
                        match target.index(&idx_val) {
                            Some(v) => vm.stack.push(v),
                            None => return Err(index_load_err(&idx_val, &target)),
                        }
                    }

                    Instruction::IndexAssignVar(name) => {
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let assigned = vm
                            .with_global_slot_mut(name, |obj| {
                                obj.assign_by_index(&idx, val.clone())
                            })
                            .ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!(
                                        "when trying to assign to '{name}[{idx}]'"
                                    ))
                            })?;
                        if assigned.is_some() {
                            vm.stack.push(val);
                        } else {
                            return Err(index_err(format!("invalid index '{idx}'"))
                                .attach_note(format!("when trying to assign to {name}[{idx}]")));
                        }
                    }
                    Instruction::IndexAssignVarDrop(name) => {
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let assigned = vm
                            .with_global_slot_mut(name, |obj| obj.assign_by_index(&idx, val))
                            .ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!(
                                        "when trying to assign to '{name}[{idx}]'"
                                    ))
                            })?;
                        if assigned.is_none() {
                            return Err(index_err(format!("invalid index '{idx}'"))
                                .attach_note(format!("when trying to assign to {name}[{idx}]")));
                        }
                    }
                    Instruction::IndexAssignLocal(slot) => {
                        let slot = usize::from(*slot);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let slot_note = vm
                            .local_slot_name(slot)
                            .map(|name| format!("local slot {slot}: {name}"));
                        let assigned = {
                            let frame = vm
                                .locals
                                .last_mut()
                                .ok_or_else(|| vm_err("no local frame"))?;
                            let slot_ref = frame.get_mut(slot).ok_or_else(|| match &slot_note {
                                Some(note) => {
                                    vm_err(format!("invalid local slot {slot}")).attach_note(note)
                                }
                                None => vm_err(format!("invalid local slot {slot}")),
                            })?;
                            slot_ref.with_mut(|target| target.assign_by_index(&idx, val.clone()))
                        };
                        if assigned.is_some() {
                            vm.stack.push(val);
                        } else {
                            return Err(vm.attach_local_slot_note(
                                slot,
                                index_err(format!("invalid index '{idx}'")).attach_note(format!(
                                    "when trying to assign to local[{slot}][{idx}]"
                                )),
                            ));
                        }
                    }
                    Instruction::IndexAssignCapture(slot) => {
                        let slot = usize::from(*slot);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let assigned = {
                            let captures = vm
                                .captures
                                .last()
                                .ok_or_else(|| vm_err("no capture frame"))?;
                            let cell = captures
                                .get(slot)
                                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                            let mut target = cell.lock().expect("poisoned capture");
                            target.assign_by_index(&idx, val.clone())
                        };
                        if assigned.is_some() {
                            vm.stack.push(val);
                        } else {
                            return Err(index_err(format!("invalid index '{idx}'")).attach_note(
                                format!("when trying to assign to capture[{slot}][{idx}]"),
                            ));
                        }
                    }
                    Instruction::IndexAssignLocalDrop(slot) => {
                        let slot_n = usize::from(*slot);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let success = {
                            let slot_ref = vm.local_slot_mut(*slot)?;
                            slot_ref.with_mut(|target| target.assign_by_index(&idx, val))
                        };
                        if success.is_none() {
                            return Err(vm.attach_local_slot_note(
                                slot_n,
                                index_err(format!("invalid index '{idx}'")).attach_note(format!(
                                    "when trying to assign to local[{slot_n}][{idx}]"
                                )),
                            ));
                        }
                        // Drop result
                    }
                    Instruction::IndexAssignCaptureDrop(slot) => {
                        let slot = usize::from(*slot);
                        let val = pop1_stack(&mut vm.stack, || "index assignment value".into())?;
                        let idx = pop1_stack(&mut vm.stack, || "index for assignment".into())?;
                        let success = {
                            let captures = vm
                                .captures
                                .last()
                                .ok_or_else(|| vm_err("no capture frame"))?;
                            let cell = captures
                                .get(slot)
                                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                            let mut target = cell.lock().expect("poisoned capture");
                            target.assign_by_index(&idx, val)
                        };
                        if success.is_none() {
                            return Err(index_err(format!("invalid index '{idx}'")).attach_note(
                                format!("when trying to assign to capture[{slot}][{idx}]"),
                            ));
                        }
                    }

                    Instruction::Postfix(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "obj + args".into())?;
                        let target = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_postfix(vm, idx, &target, argc, invoke_user_push)? {
                            continue 'exec;
                        }
                    }
                    Instruction::PostfixVar(name, argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc, || "args".into())?;
                        let target = resolve_postfix_var(vm, idx, name)?;
                        if dispatch_postfix(vm, idx, &target, argc, invoke_user_push)? {
                            continue 'exec;
                        }
                    }
                    Instruction::PostfixLocal(slot, argc) => {
                        let argc = *argc;
                        let slot_usize = usize::from(*slot);
                        ensure_stack_len(&vm.stack, argc, || "args".into())?;
                        let target = read_local_target(vm, slot_usize)?;
                        if dispatch_postfix(vm, idx, &target, argc, invoke_user_push)? {
                            continue 'exec;
                        }
                    }
                    Instruction::PostfixCapture(slot, argc) => {
                        let argc = *argc;
                        let slot_usize = usize::from(*slot);
                        ensure_stack_len(&vm.stack, argc, || "args".into())?;
                        let target = read_capture_target(vm, slot_usize)?;
                        if dispatch_postfix(vm, idx, &target, argc, invoke_user_push)? {
                            continue 'exec;
                        }
                    }

                    Instruction::TailPostfix(argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc + 1, || "obj + args".into())?;
                        let target = vm.stack.remove(vm.stack.len() - argc - 1);
                        if dispatch_postfix(vm, idx, &target, argc, tail_invoke_user)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailPostfixVar(name, argc) => {
                        let argc = *argc;
                        ensure_stack_len(&vm.stack, argc, || "args".into())?;
                        let target = resolve_postfix_var(vm, idx, name)?;
                        if dispatch_postfix(vm, idx, &target, argc, tail_invoke_user)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailPostfixLocal(slot, argc) => {
                        let argc = *argc;
                        let slot_usize = usize::from(*slot);
                        ensure_stack_len(&vm.stack, argc, || "args".into())?;
                        let target = read_local_target(vm, slot_usize)?;
                        if dispatch_postfix(vm, idx, &target, argc, tail_invoke_user)? {
                            continue 'exec;
                        }
                    }
                    Instruction::TailPostfixCapture(slot, argc) => {
                        let argc = *argc;
                        let slot_usize = usize::from(*slot);
                        ensure_stack_len(&vm.stack, argc, || "args".into())?;
                        let target = read_capture_target(vm, slot_usize)?;
                        if dispatch_postfix(vm, idx, &target, argc, tail_invoke_user)? {
                            continue 'exec;
                        }
                    }

                    Instruction::IndexMutate { target, op } => index_mutate(vm, target, op)?,

                    Instruction::Jump(pos) => vm.pc = *pos,
                    Instruction::JumpIfFalse(pos) => {
                        let target = *pos;
                        let v = pop1_stack(&mut vm.stack, || "conditional jump".into())?;
                        let cond = v.try_to_rust_bool().ok_or_else(|| {
                            attach_pc_source_ctx(
                                vm,
                                idx,
                                domain_err_vm("invalid control flow condition, expected bool")
                                    .got1(&v)
                                    .attach_note(
                                        "this value is used as a branch or loop condition",
                                    ),
                            )
                        })?;
                        if !cond {
                            vm.pc = target;
                        }
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
                                domain_err_vm(
                                    "invalid comparison result in conditional jump, expected bool",
                                )
                                .got1(&lt)
                                .attach_note(
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
                        let slot_ref =
                            vm.locals
                                .last()
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
                                domain_err_vm(
                                    "invalid loop control condition, expected a numeric value",
                                )
                                .got1(other)
                                .attach_note("this value is used as a loop bound check"),
                            )),
                        })?;
                        if is_le_zero {
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
                                    return Err(
                                        vm_err("invalid dict key, expected symbol").got1(&other)
                                    );
                                }
                            }
                        }
                        drop(iter);
                        hooks.on_dict_alloc(&|| count);
                        vm.stack.push(Value::Dict(Arc::new(map)));
                    }
                    Instruction::MakeSet(n) => {
                        let count = *n;
                        ensure_stack_len(&vm.stack, count, || "set elements".into())?;
                        let base = vm.stack.len() - count;
                        let mut set = IndexSet::with_capacity(count);
                        for v in vm.stack.drain(base..) {
                            set.insert(v);
                        }
                        vm.stack.push(Value::Set(Arc::new(set)));
                    }
                    Instruction::MakeRange {
                        inclusive,
                        has_step,
                    } => {
                        let inclusive = *inclusive;
                        let has_step = *has_step;
                        let step_val = if has_step {
                            Some(pop1_stack(&mut vm.stack, || "range step".into())?)
                        } else {
                            None
                        };
                        let end_val = pop1_stack(&mut vm.stack, || "range end".into())?;
                        let start_val = pop1_stack(&mut vm.stack, || "range start".into())?;
                        let res = make_range(&start_val, &end_val, step_val.as_ref(), inclusive)
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
                        vm.tail_call_journal_overflow = false;
                        vm.returned = true;
                        break 'exec;
                    }

                    Instruction::Assert => {
                        let value = vm.stack.last().ok_or_else(|| vm_err("missing @d value"))?;
                        match value {
                            Value::Bool(true) => {}
                            Value::Bool(false) => {
                                wqstderr_println(render_debug_line(vm, idx, value));
                                return Err(vm_err("assertion failed: got false"));
                            }
                            _ => {
                                wqstderr_println(render_debug_line(vm, idx, value));
                                return Err(vm_err("assertion failed: not a bool"));
                            }
                        }
                    }
                    Instruction::Try(len) => {
                        let len = *len;
                        let start_pc = vm.pc;
                        let end_pc = start_pc + len;
                        let stack_start = vm.stack.len();
                        vm.try_depth += 1;
                        let initial_inst_ptr = vm.instructions.as_ptr();
                        let try_result = self.interpret(vm, end_pc);
                        vm.try_depth = vm.try_depth.saturating_sub(1);
                        match try_result {
                            Ok(val) => {
                                if vm.returned || vm.instructions.as_ptr() != initial_inst_ptr {
                                    return Ok(val);
                                }
                                if vm.pc == end_pc {
                                    vm.stack.truncate(stack_start);
                                    vm.stack.push(Value::Bool(true));
                                } else {
                                    vm.stack.truncate(stack_start);
                                }
                            }
                            Err(_) => {
                                vm.stack.truncate(stack_start);
                                vm.stack.push(Value::Bool(false));
                                vm.pc = end_pc;
                            }
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
                        wqstderr_println(render_trace_line(vm, idx, value, &records));
                    }
                    Instruction::Pause => {
                        let loc = CodeLoc {
                            chunk: vm.current_chunk_id(),
                            pc: idx,
                        };
                        if !vm.wqdb.pause_break_enabled(loc) {
                            continue;
                        }
                        vm.wqdb.note_pause(loc);
                        if let Some(f) = vm.wqdb.on_pause {
                            f(vm);
                        }
                    }
                }
            }

            if vm.instructions.as_ptr() != inst_ptr {
                inst_ptr = vm.instructions.as_ptr();
                limit = vm.instructions.len();
                continue;
            }

            break;
        }
        // Capture a probe for the last interesting instruction of this frame
        // before returning, so trace records inside a function body include
        // the result expression.
        if vm.trace_depth > 0
            && let Some(prev) = last_probe_pc.take()
        {
            record_trace_probe(vm, prev);
        }
        let value = vm.stack.pop().unwrap_or(Value::unit());
        Ok(value)
    }
}

impl VanillaInterpreter {
    fn build_local_callable(
        &self,
        vm: &mut Vm,
        fi: usize,
        slot_usize: usize,
        p: PeekLocalUser,
    ) -> WqResult<LocalCallable> {
        let dbg_new = vm.ensure_dbg_chunk_with_spans(
            "<fn>",
            DebugChunkSpec {
                dbg_chunk: p.dbg_chunk,
                instructions: p.instructions.as_ref(),
                dbg_stmt_spans: &p.spans,
                source_base_offset: match &p.value {
                    Value::CompiledFunction(f) => f.dbg_source_base_offset,
                    Value::Closure(c) => c.dbg_source_base_offset,
                    _ => vm.resolved_debug_base_offset(),
                },
                dbg_pc_spans: &p.pc_spans,
                dbg_stmt_marks: &p.stmt_marks,
                dbg_local_names: &p.names,
                params: &p.params,
            },
        );
        if dbg_new != p.dbg_chunk
            && let Some(slot_ref) = vm.locals.get_mut(fi).and_then(|f| f.get_mut(slot_usize))
        {
            slot_ref.with_mut(|value| {
                if let Value::CompiledFunction(f) = value {
                    if f.dbg_chunk != dbg_new {
                        let mut new_f = FunctionData::clone(f);
                        new_f.dbg_chunk = dbg_new;
                        *value = Value::CompiledFunction(Arc::new(new_f));
                    }
                } else if let Value::Closure(c) = value
                    && c.dbg_chunk != dbg_new
                {
                    let mut new_c = ClosureData::clone(c);
                    new_c.dbg_chunk = dbg_new;
                    *value = Value::Closure(Arc::new(new_c));
                }
            });
        }
        Ok(LocalCallable::Func {
            value: p.value.clone(),
            params_len: p.params.as_ref().map(|x| x.len() as u32),
            locals: p.locals,
            instructions: p.instructions,
            captured: if p.is_closure {
                p.captured
            } else {
                crate::value::cell::empty_cells()
            },
            dbg_chunk: dbg_new,
            name_hint: None,
        })
    }

    fn resolve_local_callable(
        &self,
        vm: &mut Vm,
        idx: usize,
        slot: u16,
        slot_usize: usize,
    ) -> WqResult<LocalCallable> {
        let try_peek_at = |vm: &Vm, fi: usize| -> Option<WqResult<PeekLocalCallable>> {
            let v = vm.locals[fi].get(slot_usize)?;
            Some(peek_local_callable(slot, v).map_err(|e| vm.attach_local_slot_note(slot_usize, e)))
        };

        let mut found_fi: Option<usize> = None;
        let callable = {
            let mut found: Option<LocalCallable> = None;

            // Try cached frame depth first
            if let Some(depth) = vm.inline_cache[idx].local_frame_depth {
                let depth = usize::from(depth);
                if depth < vm.locals.len() {
                    let fi = vm.locals.len() - 1 - depth;
                    if let Some(result) = try_peek_at(vm, fi) {
                        let peeked = result?;
                        match peeked {
                            PeekLocalCallable::Builtin(name) => {
                                found = Some(LocalCallable::Builtin(name));
                                found_fi = Some(fi);
                            }
                            PeekLocalCallable::User(p) => {
                                found = Some(self.build_local_callable(vm, fi, slot_usize, p)?);
                                found_fi = Some(fi);
                            }
                        }
                    }
                }
            }

            // Fall back to full iteration
            if found.is_none() {
                for fi in (0..vm.locals.len()).rev() {
                    if found_fi == Some(fi) {
                        continue;
                    }
                    let peeked = if let Some(result) = try_peek_at(vm, fi) {
                        result?
                    } else {
                        continue;
                    };

                    match peeked {
                        PeekLocalCallable::Builtin(name) => {
                            found = Some(LocalCallable::Builtin(name));
                            found_fi = Some(fi);
                            break;
                        }
                        PeekLocalCallable::User(p) => {
                            found = Some(self.build_local_callable(vm, fi, slot_usize, p)?);
                            found_fi = Some(fi);
                            break;
                        }
                    }
                }
            }

            // Update cache
            if let Some(fi) = found_fi {
                vm.inline_cache[idx].local_frame_depth = Some((vm.locals.len() - 1 - fi) as u16);
            }

            found
        }
        .ok_or_else(|| {
            vm.attach_local_slot_note(slot_usize, vm_err(format!("invalid local slot {slot}")))
        })?;
        Ok(callable)
    }
}

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
fn domain_err_vm(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Domain).src(NAME).msg(msg.into())
}

#[inline]
fn index_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Index).src(NAME).msg(msg.into())
}

#[inline]
fn named_arg_index_err() -> WqError {
    WqError::new(WqErrorType::Arity)
        .src(NAME)
        .msg("cannot pass named arguments when indexing a non-function value")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::astnode::BinaryOperator;
    use crate::interpret::Interpreter;
    use crate::interpret::vanilla::VanillaInterpreter;
    use crate::value::Value;
    use crate::vm::Vm;
    use crate::vm::inst::{Instruction, Operand};

    #[test]
    fn invalid_local_slot_errors_include_slot_name_note() {
        let mut vm = Vm::new(vec![Instruction::LoadLocal(1)]);
        let file_id = vm.debug_info.new_file("<test>", "");
        let chunk = vm.debug_info.new_chunk("<test>", file_id, 1);
        vm.debug_info.chunk_mut(chunk).local_names = Some(vec!["x".into(), "y".into()]);
        vm.current_chunk = chunk;

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

    fn run_vm(insts: Vec<Instruction>) -> Value {
        let len = insts.len();
        let mut vm = Vm::new(insts);
        let mut interpreter = VanillaInterpreter;
        interpreter.interpret(&mut vm, len).expect("execute")
    }

    #[test]
    fn booland_lazy_short_circuits_on_false() {
        // left=false → push false, jump over right operand + BinaryOp to Return
        let result = run_vm(vec![
            Instruction::load_const(Value::Bool(false)),
            Instruction::BoolAndLazy(4), // short-circuit → jump to Return
            Instruction::load_const(Value::Bool(true)),
            Instruction::binary_op(BinaryOperator::BoolAnd, Operand::Stack, Operand::Stack),
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
            Instruction::binary_op(BinaryOperator::BoolAnd, Operand::Stack, Operand::Stack),
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
            Instruction::binary_op(BinaryOperator::BoolOr, Operand::Stack, Operand::Stack),
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
            Instruction::binary_op(BinaryOperator::BoolOr, Operand::Stack, Operand::Stack),
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
        vm.runtime_debug_info = true;
        let file_id = vm.debug_info.new_file("<trace-test>", "test source text");
        let chunk = vm.debug_info.new_chunk("<trace-test>", file_id, len);
        // Give every PC a dummy span so trace probes aren't silently dropped.
        let meta = vm.debug_info.chunk_mut(chunk);
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
        vm.current_chunk = chunk;
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
        // No Debug to drain — check buf after Return
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
        assert!(vm.trace_buf[0].type_name.contains("int"));
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
}
