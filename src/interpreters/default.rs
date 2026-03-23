mod cmpchain;
mod fastpath;

use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use crate::{
    astnode::BinaryOperator,
    interpreters::{
        Interpreter,
        default::{
            cmpchain::eval_cmp_chain,
            fastpath::{fp_floor_div, fp_int_binary_op},
        },
    },
    value::{Excerpt, IntoWqValue, Value, WqResult, eval_binary, eval_unary},
    vm::{
        Vm,
        call::{
            CallSpec, CallTarget, LocalCallable, PeekLocal, ResolvedCfn, ResolvedClosure,
            peek_local_callable,
        },
        ensure_stack_len,
        instruction::{Capture, Instruction},
        last_clone_stack, pop1_stack, pop2_stack,
    },
    wqdb::{ChunkId, CodeLoc, DebugHost, apply_stmt_spans_exact_offs, mark_stmt_heuristic},
    wqerror::{WqError, WqErrorType},
};

use indexmap::IndexMap;

pub struct DefaultInterpreter;

pub(crate) trait ProfilerHooks {
    fn before_instruction(&mut self, _vm: &Vm, _idx: usize, _op: &Instruction) {}

    fn on_load_var_cache_hit(&mut self, _slot_cached: bool) {}

    fn on_load_var_cache_miss(&mut self) {}

    fn on_call_user_cache_hit(&mut self) {}

    fn on_call_user_cache_miss(&mut self) {}

    fn on_list_alloc(&mut self, _len: usize) {}

    fn on_dict_alloc(&mut self, _len: usize) {}

    fn on_range_alloc(&mut self, _len: usize) {}

    fn on_closure_capture_alloc(&mut self, _len: usize) {}

    fn on_return(&mut self, _vm: &Vm) {}
}

#[derive(Default)]
pub(crate) struct NoopProfilerHooks;

impl ProfilerHooks for NoopProfilerHooks {}

impl Interpreter for DefaultInterpreter {
    fn execute(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        if limit > vm.instructions.len() {
            return Err(vm_err(format!("limit out of bounds: {limit}")));
        }
        let mut hooks = NoopProfilerHooks;
        while vm.pc < limit {
            if !self.execute_one_with_hooks(vm, limit, &mut hooks)? {
                break;
            }
        }
        Ok(vm.stack.pop().unwrap_or(Value::unit()))
    }
}

impl DefaultInterpreter {
    // pub(crate) fn execute_one(&mut self, vm: &mut Vm, limit: usize) -> WqResult<bool> {
    //     let mut hooks = NoopProfilerHooks;
    //     self.execute_one_with_hooks(vm, limit, &mut hooks)
    // }

    pub(crate) fn execute_one_with_hooks<H: ProfilerHooks>(
        &mut self,
        vm: &mut Vm,
        limit: usize,
        hooks: &mut H,
    ) -> WqResult<bool> {
        if vm.pc >= limit {
            return Ok(false);
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
        hooks.before_instruction(vm, idx, op);
        match op {
            Instruction::LoadConst(v) => vm.stack.push(v.clone()),
            Instruction::LoadVar(name) => {
                let cache = &vm.inline_cache[idx];
                if let Some(slot) = cache.slot
                    && let Some(val) = vm.global_slot_value(slot)
                {
                    vm.stack.push(val.clone());
                    hooks.on_load_var_cache_hit(true);
                    return Ok(true);
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
                    return Ok(true);
                }

                if vm.builtins.has_function(name) {
                    vm.stack.push(Value::BuiltinFunction(name.clone()));
                    return Ok(true);
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
            Instruction::StoreVar(name) => {
                let val = pop1_stack(&mut vm.stack, || {
                    Cow::Owned(format!("store into variable '{name}'"))
                })?;
                if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.assign_global_at_slot(name, slot, val);
                } else {
                    let slot = vm.assign_global_and_slot(name, val);
                    vm.inline_cache[idx].slot = Some(slot);
                }
            }
            Instruction::StoreVarKeep(name) => {
                let val = last_clone_stack(&vm.stack, || {
                    Cow::Owned(format!("store into variable '{name}'"))
                })?;
                if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.assign_global_at_slot(name, slot, val);
                } else {
                    let slot = vm.assign_global_and_slot(name, val);
                    vm.inline_cache[idx].slot = Some(slot);
                }
            }
            Instruction::LoadLocal(i) => {
                let slot = *i as usize;
                let slot_num = *i;
                let val = vm
                    .locals
                    .last()
                    .and_then(|f| f.get(slot))
                    .ok_or_else(|| vm_err(format!("invalid local slot {slot_num}")))?;
                vm.stack.push(val.read());
            }
            Instruction::StoreLocal(i) => {
                let slot = *i as usize;
                let slot_num = *i;
                let val = pop1_stack(&mut vm.stack, || {
                    Cow::Owned(format!("store into local slot {slot_num}"))
                })?;
                if let Some(frame) = vm.locals.last_mut() {
                    if let Some(dest) = frame.get_mut(slot) {
                        dest.write(val);
                    } else {
                        return Err(vm_err(format!("invalid local slot {slot_num}")));
                    }
                } else {
                    return Err(vm_err("no local frame"));
                }
            }
            Instruction::StoreLocalKeep(i) => {
                let slot = *i as usize;
                let slot_num = *i;
                let val = last_clone_stack(&vm.stack, || {
                    Cow::Owned(format!("store into local slot {slot_num}"))
                })?;
                if let Some(frame) = vm.locals.last_mut() {
                    if let Some(dest) = frame.get_mut(slot) {
                        dest.write(val);
                    } else {
                        return Err(vm_err(format!("invalid local slot {slot_num}")));
                    }
                } else {
                    return Err(vm_err("no local frame"));
                }
            }
            Instruction::Pop => {
                vm.stack.pop();
            }
            Instruction::Return => {
                hooks.on_return(vm);
                return Ok(false);
            }

            Instruction::BinaryOp(op) => {
                let op = *op;
                let (left, right) =
                    pop2_stack(&mut vm.stack, || Cow::Owned(format!("binary op {op:?}")))?;
                if let Some(res) = fp_int_binary_op(op, &left, &right) {
                    vm.stack.push(res);
                    return Ok(true);
                }
                let result = eval_binary(&op, left, right)?;
                vm.stack.push(result);
            }
            Instruction::CmpChain(ops) => {
                let ops = ops.as_ref();
                let need = ops.len() + 1;
                ensure_stack_len(&vm.stack, need, || {
                    Cow::Owned(format!("comparison chain of length {}", ops.len()))
                })?;
                let base = vm.stack.len() - need;

                let result = eval_cmp_chain(ops, &vm.stack[base..])?;
                vm.stack.truncate(base);
                vm.stack.push(result);
            }
            Instruction::UnaryOp(op) => {
                let op = *op;
                let val = pop1_stack(&mut vm.stack, || Cow::Owned(format!("unary op {op:?}")))?;
                let result = eval_unary(&op, val)?;
                vm.stack.push(result);
            }

            Instruction::FloorDiv => {
                let (left, right) =
                    pop2_stack(&mut vm.stack, || Cow::Borrowed("floor-div operands"))?;
                if let Some(v) = fp_floor_div(&left, &right) {
                    vm.stack.push(v);
                    return Ok(true);
                }
                // Fallback: compute divide then floor with full semantics (incl. bc)
                let v = left
                    .divide(&right)
                    .and_then(|x| x.floor())
                    .map_err(|e| e.into_wqerror().src("floor-div"))?;
                vm.stack.push(v);
            }

            Instruction::MakeList(n) => {
                let count = *n;
                ensure_stack_len(&vm.stack, count, || Cow::Borrowed("list elements"))?;
                let base = vm.stack.len() - count;
                let mut items = Vec::with_capacity(count);
                items.extend(vm.stack.drain(base..));
                hooks.on_list_alloc(count);
                vm.stack.push(Value::from_items(items));
            }
            Instruction::MakeDict(n) => {
                let count = *n;
                ensure_stack_len(&vm.stack, count * 2, || {
                    Cow::Borrowed("dict key-value pairs")
                })?;
                let base = vm.stack.len() - count * 2;
                let mut map = IndexMap::with_capacity(count);
                let mut iter = vm.stack.drain(base..);

                for _ in 0..count {
                    let key = iter.next().unwrap();
                    let val = iter.next().unwrap();
                    match key {
                        Value::Symbol(k) => {
                            map.insert(k, val);
                        }
                        other => {
                            return Err(vm_err("invalid dict key, expected symbol").got1(&other));
                        }
                    }
                }
                drop(iter); // explicitly finish drain
                hooks.on_dict_alloc(count);
                vm.stack.push(Value::Dict(Box::new(map)));
            }
            Instruction::MakeRange {
                inclusive,
                has_step,
            } => {
                let inclusive = *inclusive;
                let has_step = *has_step;
                let step_val = if has_step {
                    Some(pop1_stack(&mut vm.stack, || Cow::Borrowed("range step"))?)
                } else {
                    None
                };
                let end_val = pop1_stack(&mut vm.stack, || Cow::Borrowed("range end"))?;
                let start_val = pop1_stack(&mut vm.stack, || Cow::Borrowed("range start"))?;
                let res = make_range(&start_val, &end_val, step_val.as_ref(), inclusive)
                    .map_err(|e| e.src("vm"))?;
                hooks.on_range_alloc(range_alloc_len(&res));
                vm.stack.push(res);
            }

            Instruction::CallBuiltinId(id, argc) => {
                let builtin_id = *id;
                let argc = *argc;
                let result =
                    vm.builtin_from_stack_by_id_with_interpreter(builtin_id, argc, self)?;
                vm.stack.push(result);
            }
            Instruction::CallLocal(slot, argc) => {
                let argc = *argc;
                let slot = *slot;
                ensure_stack_len(&vm.stack, argc, || {
                    Cow::Owned(format!("local call slot {slot} args"))
                })?;
                let argc_val = argc;
                let slot_usize = slot as usize;

                let callable = {
                    let mut found: Option<LocalCallable> = None;
                    for fi in (0..vm.locals.len()).rev() {
                        let peeked = if let Some(v) = vm.locals[fi].get(slot_usize) {
                            peek_local_callable(slot, v)?
                        } else {
                            continue;
                        };

                        match peeked {
                            PeekLocal::Builtin(name) => {
                                found = Some(LocalCallable::Builtin(name));
                                break;
                            }
                            PeekLocal::Func(p) => {
                                let dbg_new = vm.ensure_dbg_chunk_with_spans(
                                    "<fn>",
                                    p.dbg_chunk,
                                    &p.instructions,
                                    &p.spans,
                                    &p.names,
                                    &p.params,
                                );
                                if dbg_new != p.dbg_chunk
                                    && let Some(slot_ref) =
                                        vm.locals.get_mut(fi).and_then(|f| f.get_mut(slot_usize))
                                {
                                    slot_ref.with_mut(|value| {
                                        if let Value::CompiledFunction(f) = value {
                                            if f.dbg_chunk != dbg_new {
                                                let mut new_f =
                                                    crate::value::FunctionData::clone(f);
                                                new_f.dbg_chunk = dbg_new;
                                                *value = Value::CompiledFunction(Arc::new(new_f));
                                            }
                                        } else if let Value::Closure(c) = value
                                            && c.dbg_chunk != dbg_new
                                        {
                                            let mut new_c = crate::value::ClosureData::clone(c);
                                            new_c.dbg_chunk = dbg_new;
                                            *value = Value::Closure(Arc::new(new_c));
                                        }
                                    });
                                }

                                found = Some(LocalCallable::Func {
                                    value: p.value.clone(),
                                    params: p.params,
                                    locals: p.locals,
                                    instructions: p.instructions,
                                    captured: if p.is_closure { p.captured } else { Vec::new() },
                                    dbg_chunk: dbg_new,
                                    name_hint: None,
                                });
                                break;
                            }
                        }
                    }
                    found
                }
                .ok_or_else(|| vm_err(format!("invalid local slot {slot}")))?;
                match callable {
                    LocalCallable::Func {
                        value,
                        params,
                        locals,
                        instructions,
                        captured,
                        dbg_chunk,
                        name_hint,
                    } => {
                        let res = vm.call_function_with(
                            CallSpec {
                                instructions,
                                params,
                                locals,
                                captured,
                                argc: argc_val,
                                callee_name: CallSpec::name_hint(name_hint.as_deref()),
                                dbg_chunk,
                                callee: value,
                            },
                            self,
                        )?;
                        vm.stack.push(res);
                    }
                    LocalCallable::Builtin(name) => {
                        let result = vm.builtin_from_stack_by_name(&name, argc_val)?;
                        vm.stack.push(result);
                    }
                }
            }
            Instruction::CallUser(name, argc) => {
                let argc = *argc;
                ensure_stack_len(&vm.stack, argc, || Cow::Owned(format!("fn '{name}' args")))?;
                if let Some(slot) = vm.lookup_global_slot(name) {
                    let name_version = vm.global_slot_version(slot);
                    if vm.inline_cache[idx].version == name_version
                        && let Some(ref target) = vm.inline_cache[idx].call_target
                    {
                        match target {
                            CallTarget::Cfn(ResolvedCfn {
                                value,
                                params,
                                locals,
                                code,
                                dbg_chunk,
                            }) => {
                                let res = vm.call_function_with(
                                    CallSpec {
                                        instructions: code.clone(),
                                        params: params.clone(),
                                        locals: *locals,
                                        captured: Vec::new(),
                                        argc,
                                        callee_name: CallSpec::name_hint(Some(name.as_str())),
                                        dbg_chunk: *dbg_chunk,
                                        callee: value.clone(),
                                    },
                                    self,
                                )?;
                                vm.stack.push(res);
                                hooks.on_call_user_cache_hit();
                                return Ok(true);
                            }
                            CallTarget::Closure(ResolvedClosure {
                                value,
                                params,
                                locals,
                                captured,
                                code,
                                dbg_chunk,
                            }) => {
                                let res = vm.call_function_with(
                                    CallSpec {
                                        instructions: code.clone(),
                                        params: params.clone(),
                                        locals: *locals,
                                        captured: captured.clone(),
                                        argc,
                                        callee_name: CallSpec::name_hint(Some(name.as_str())),
                                        dbg_chunk: *dbg_chunk,
                                        callee: value.clone(),
                                    },
                                    self,
                                )?;
                                vm.stack.push(res);
                                return Ok(true);
                            }
                        }
                    }
                }
                hooks.on_call_user_cache_miss();
                let func = vm.resolve_user_callable(idx, name)?;
                if let Value::BuiltinFunction(bname) = &func {
                    let out = vm.builtin_from_stack_by_name(bname, argc)?;
                    vm.stack.push(out);
                } else {
                    // Reuse the exact same path as CallOrIndex:
                    let base = vm.stack.len() - argc;
                    let mut args = Vec::with_capacity(argc);
                    args.extend(vm.stack.drain(base..));
                    let out = vm.call_value_with_args(&func, args)?;
                    vm.stack.push(out);
                }
            }
            Instruction::CallAnon(argc) => {
                let argc = *argc;
                ensure_stack_len(&vm.stack, argc + 1, || Cow::Borrowed("callable + args"))?;
                let len = vm.stack.len();
                let base = len - argc;
                let mut args = Vec::with_capacity(argc);
                args.extend(vm.stack.drain(base..));
                let func = vm
                    .stack
                    .pop()
                    .ok_or_else(|| vm_err("stack underflow while retrieving callable"))?;
                let res = vm.call_value_with_args(&func, args)?;
                vm.stack.push(res);
            }
            Instruction::CallOrIndexLocal(slot, argc) => {
                let argc = *argc;
                let slot_usize = *slot as usize;
                ensure_stack_len(&vm.stack, argc, || Cow::Borrowed("args"))?;
                let len = vm.stack.len();
                let base = len - argc;

                let is_call = {
                    let frame = vm.locals.last().ok_or_else(|| vm_err("no local frame"))?;
                    let slot_ref = frame
                        .get(slot_usize)
                        .ok_or_else(|| vm_err(format!("invalid local slot {slot_usize}")))?;
                    slot_ref.with_ref(Value::is_callable)
                };

                if is_call {
                    let mut args = Vec::with_capacity(argc);
                    args.extend(vm.stack.drain(base..));
                    // Function calls need to take ownership or a clone for execution.
                    let func = {
                        let frame = vm.locals.last().unwrap();
                        frame.get(slot_usize).unwrap().read()
                    };
                    let res = vm.call_value_with_args(&func, args)?;
                    vm.stack.push(res);
                } else {
                    let mut args = Vec::with_capacity(argc);
                    args.extend(vm.stack.drain(base..));
                    let idx_val = if args.len() == 1 {
                        args.pop().unwrap()
                    } else {
                        Value::from_items(args)
                    };

                    let res = {
                        let frame = vm.locals.last().unwrap();
                        let slot_ref = frame.get(slot_usize).unwrap();
                        slot_ref.with_ref(|obj| obj.index(&idx_val))
                    };

                    match res {
                        Some(v) => vm.stack.push(v),
                        None => {
                            let obj_excerpt = {
                                let frame = vm.locals.last().unwrap();
                                let slot_ref = frame.get(slot_usize).unwrap();
                                slot_ref.with_ref(|obj| obj.excerpt())
                            };
                            return Err(index_err("invalid index")
                                .attach_note(format!("index: '{}'", idx_val.excerpt()))
                                .attach_note(format!("target: '{}'", obj_excerpt)));
                        }
                    }
                }
            }
            Instruction::CallOrIndexVar(name, argc) => {
                let argc = *argc;
                ensure_stack_len(&vm.stack, argc, || Cow::Borrowed("args"))?;

                let mut slot_opt = vm.inline_cache[idx].slot;
                if slot_opt.is_none()
                    && let Some(slot) = vm.lookup_global_slot(name)
                {
                    vm.inline_cache[idx].slot = Some(slot);
                    slot_opt = Some(slot);
                }

                let global_val_opt = if let Some(slot) = slot_opt {
                    vm.global_slot_value(slot).cloned()
                } else {
                    vm.lookup_global(name)
                };

                if let Some(global_val) = global_val_opt {
                    let is_call = Value::is_callable(&global_val);

                    let len = vm.stack.len();
                    let base = len - argc;
                    let mut args = Vec::with_capacity(argc);
                    args.extend(vm.stack.drain(base..));

                    if is_call {
                        let res = vm.call_value_with_args(&global_val, args)?;
                        vm.stack.push(res);
                    } else {
                        let idx_val = if args.len() == 1 {
                            args.pop().unwrap()
                        } else {
                            Value::from_items(args)
                        };
                        match global_val.index(&idx_val) {
                            Some(v) => vm.stack.push(v),
                            None => {
                                let target_excerpt = global_val.excerpt();
                                return Err(index_err("invalid index")
                                    .attach_note(format!("index: '{}'", idx_val.excerpt()))
                                    .attach_note(format!("target: '{}'", target_excerpt)));
                            }
                        }
                    }
                } else {
                    if vm.builtins.has_function(name) {
                        let res = vm.builtin_from_stack_by_name(name, argc)?;
                        vm.stack.push(res);
                    } else if vm.builtins.is_disabled_name(name) {
                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        ))
                        .attach_note(format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        )));
                    } else {
                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        )));
                    }
                }
            }
            Instruction::IndexLoadVar(name) => {
                let idx_val = pop1_stack(&mut vm.stack, || Cow::Borrowed("index"))?;

                let mut slot_opt = vm.inline_cache[idx].slot;
                if slot_opt.is_none()
                    && let Some(slot) = vm.lookup_global_slot(name)
                {
                    vm.inline_cache[idx].slot = Some(slot);
                    slot_opt = Some(slot);
                }

                let global_val_opt = if let Some(slot) = slot_opt {
                    vm.global_slot_value(slot).cloned()
                } else {
                    vm.lookup_global(name)
                };

                if let Some(global_val) = global_val_opt {
                    match global_val.index(&idx_val) {
                        Some(v) => vm.stack.push(v),
                        None => {
                            let target_excerpt = global_val.excerpt();
                            return Err(index_err("invalid index")
                                .attach_note(format!("index: '{}'", idx_val.excerpt()))
                                .attach_note(format!("target: '{}'", target_excerpt)));
                        }
                    }
                } else {
                    if vm.builtins.is_disabled_name(name) {
                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        ))
                        .attach_note(format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        )));
                    } else {
                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        )));
                    }
                }
            }
            Instruction::CallOrIndex(argc) => {
                let argc = *argc;
                ensure_stack_len(&vm.stack, argc + 1, || Cow::Borrowed("obj + args"))?;
                let len = vm.stack.len();
                let base = len - argc;
                let obj_index = base - 1;
                let is_call = vm.stack.get(obj_index).is_some_and(Value::is_callable);
                if is_call {
                    let mut args = Vec::with_capacity(argc);
                    args.extend(vm.stack.drain(base..));
                    let func = vm
                        .stack
                        .pop()
                        .ok_or_else(|| vm_err("stack underflow while calling"))?;
                    let res = vm.call_value_with_args(&func, args)?;
                    vm.stack.push(res);
                } else {
                    // index path
                    let mut args = Vec::with_capacity(argc);
                    args.extend(vm.stack.drain(base..));
                    let obj = match vm.stack.pop() {
                        Some(v) => v,
                        None => return Err(vm_err("stack underflow while indexing")),
                    };
                    let idx_val = if args.len() == 1 {
                        args.pop().unwrap()
                    } else {
                        Value::from_items(args)
                    };
                    match obj.index(&idx_val) {
                        Some(v) => vm.stack.push(v),
                        None => {
                            return Err(index_err("invalid index")
                                .attach_note(format!("index: '{}'", idx_val.excerpt()))
                                .attach_note(format!("target: '{}'", obj.excerpt())));
                        }
                    }
                }
            }
            Instruction::Index => {
                let idx = pop1_stack(&mut vm.stack, || Cow::Borrowed("index"))?;
                let obj = pop1_stack(&mut vm.stack, || Cow::Borrowed("object for indexing"))?;
                match obj.index(&idx) {
                    Some(v) => vm.stack.push(v),
                    None => {
                        return Err(index_err("invalid index")
                            .attach_note(format!("index: '{}'", idx.excerpt()))
                            .attach_note(format!("target: '{}'", obj.excerpt())));
                    }
                }
            }
            Instruction::IndexLoadLocal(slot) => {
                let slot = *slot as usize;
                let idx = pop1_stack(&mut vm.stack, || Cow::Borrowed("index"))?;

                let res = {
                    let frame = vm.locals.last().ok_or_else(|| vm_err("no local frame"))?;
                    let slot_ref = frame
                        .get(slot)
                        .ok_or_else(|| vm_err(format!("invalid local slot {slot}")))?;
                    slot_ref.with_ref(|obj| obj.index(&idx))
                };

                match res {
                    Some(v) => vm.stack.push(v),
                    None => {
                        let obj_excerpt = {
                            let frame = vm.locals.last().unwrap();
                            let slot_ref = frame.get(slot).unwrap();
                            slot_ref.with_ref(|obj| obj.excerpt())
                        };
                        return Err(index_err("invalid index")
                            .attach_note(format!("index: '{}'", idx.excerpt()))
                            .attach_note(format!("target: '{}'", obj_excerpt)));
                    }
                }
            }
            Instruction::IndexAssignVar(name) => {
                let val = pop1_stack(&mut vm.stack, || Cow::Borrowed("index assignment value"))?;
                let idx = pop1_stack(&mut vm.stack, || Cow::Borrowed("index for assignment"))?;
                let assigned = vm
                    .with_global_slot_mut(name, |obj| obj.assign_by_index(&idx, val.clone()))
                    .ok_or_else(|| {
                        not_bound_err(format!("'{name}' has not been bound to a value"))
                            .attach_note(format!("when trying to assign to '{name}[{idx}]'"))
                    })?;
                if assigned.is_some() {
                    vm.stack.push(val);
                } else {
                    return Err(index_err(format!("invalid index '{idx}'"))
                        .attach_note(format!("when trying to assign to {name}[{idx}]")));
                }
            }
            Instruction::IndexAssignVarDrop(name) => {
                let val = pop1_stack(&mut vm.stack, || Cow::Borrowed("index assignment value"))?;
                let idx = pop1_stack(&mut vm.stack, || Cow::Borrowed("index for assignment"))?;
                let assigned = vm
                    .with_global_slot_mut(name, |obj| obj.assign_by_index(&idx, val))
                    .ok_or_else(|| {
                        not_bound_err(format!("'{name}' has not been bound to a value"))
                            .attach_note(format!("when trying to assign to '{name}[{idx}]'"))
                    })?;
                if assigned.is_none() {
                    return Err(index_err(format!("invalid index '{idx}'"))
                        .attach_note(format!("when trying to assign to {name}[{idx}]")));
                }
            }
            Instruction::IndexAssignLocal(slot) => {
                let slot = *slot;
                let val = pop1_stack(&mut vm.stack, || Cow::Borrowed("index assignment value"))?;
                let idx = pop1_stack(&mut vm.stack, || Cow::Borrowed("index for assignment"))?;
                let assigned = {
                    let frame = vm
                        .locals
                        .last_mut()
                        .ok_or_else(|| vm_err("no local frame"))?;
                    let slot_ref = frame
                        .get_mut(slot as usize)
                        .ok_or_else(|| vm_err(format!("invalid local slot {slot}")))?;
                    slot_ref.with_mut(|target| target.assign_by_index(&idx, val.clone()))
                };
                if assigned.is_some() {
                    vm.stack.push(val);
                } else {
                    return Err(index_err(format!("invalid index '{idx}'"))
                        .attach_note(format!("when trying to assign to local[{slot}][{idx}]")));
                }
            }
            Instruction::IndexAssignLocalDrop(slot) => {
                let slot = *slot;
                let val = pop1_stack(&mut vm.stack, || Cow::Borrowed("index assignment value"))?;
                let idx = pop1_stack(&mut vm.stack, || Cow::Borrowed("index for assignment"))?;
                let success = {
                    let slot_ref = vm.local_slot_mut(slot)?;
                    slot_ref.with_mut(|target| target.assign_by_index(&idx, val))
                };
                if success.is_none() {
                    return Err(index_err(format!("invalid index '{idx}'"))
                        .attach_note(format!("when trying to assign to local[{slot}][{idx}]")));
                }
                // Drop result
            }
            Instruction::Jump(pos) => vm.pc = *pos,
            Instruction::JumpIfFalse(pos) => {
                let target = *pos;
                let v = pop1_stack(&mut vm.stack, || Cow::Borrowed("conditional jump"))?;
                let is_false = match v {
                    Value::Bool(b) => !b,
                    _ => {
                        return Err(domain_err_vm(
                            "invalid control flow　condition, expected bool",
                        )
                        .got1(&v));
                    }
                };
                if is_false {
                    vm.pc = target;
                }
            }
            Instruction::JumpIfGE(pos) => {
                let target = *pos;
                // Pop right then left, jump if left >= right
                let (left, right) = pop2_stack(&mut vm.stack, || {
                    Cow::Borrowed("compare-jump (left >= right)")
                })?;
                let lt = left.lt(&right).map_err(|e| e.into_wqerror().src("vm"))?;
                let cond = match lt {
                    Value::Bool(b) => !b,
                    v => {
                        return Err(domain_err_vm(
                            "invalid control flow　condition, expected bool",
                        )
                        .got1(&v));
                    }
                };
                if cond {
                    vm.pc = target;
                }
            }
            Instruction::JumpIfLEZLocal(slot, pos) => {
                let slot_num = *slot as usize;
                let target = *pos;
                // Jump if local[slot] <= 0
                let slot_ref = vm
                    .locals
                    .last()
                    .and_then(|f| f.get(slot_num))
                    .ok_or_else(|| vm_err(format!("invalid local slot {slot_num}")))?;
                let is_le_zero = slot_ref.with_ref(|val| match val {
                    Value::Int(n) => Ok(*n <= 0),
                    Value::Float(f) => Ok(*f <= 0.0),
                    _ => Err(domain_err_vm("invalid control flow condition")),
                })?;
                if is_le_zero {
                    vm.pc = target;
                }
            }

            Instruction::Inc1Local(slot) => {
                let slot = *slot;
                let one = Value::Int(1);
                let new_val = {
                    let slot_ref = vm.local_slot_mut(slot)?;
                    let old = slot_ref.read();
                    if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &old, &one) {
                        v
                    } else {
                        eval_binary(&BinaryOperator::Add, old, one)?
                    }
                };
                // Write back
                let slot_ref = vm.local_slot_mut(slot)?;
                slot_ref.write(new_val);
            }

            Instruction::Inc1LocalKeep(slot) => {
                let slot = *slot;
                let one = Value::Int(1);
                let new_val = {
                    let slot_ref = vm.local_slot_mut(slot)?;
                    let old = slot_ref.read();
                    if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &old, &one) {
                        v
                    } else {
                        eval_binary(&BinaryOperator::Add, old, one)?
                    }
                };
                let slot_ref = vm.local_slot_mut(slot)?;
                slot_ref.write(new_val.clone());
                vm.stack.push(new_val);
            }

            Instruction::Inc1Var(name) => {
                let one = Value::Int(1);
                let cur = if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.global_slot_value(slot)
                        .ok_or_else(|| vm_err("invalid global slot"))?
                        .clone()
                } else if let Some(slot) = vm.lookup_global_slot(name) {
                    vm.inline_cache[idx].slot = Some(slot);
                    vm.global_slot_value(slot)
                        .ok_or_else(|| vm_err("invalid global slot"))?
                        .clone()
                } else {
                    return Err(not_bound_err(format!(
                        "'{name}' has not been bound to a value"
                    )));
                };
                let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &cur, &one) {
                    v
                } else {
                    eval_binary(&BinaryOperator::Add, cur, one)?
                };
                if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.assign_global_at_slot(name, slot, new_val);
                } else {
                    vm.assign_global(name, new_val);
                }
            }

            Instruction::Inc1VarFromVar { src, dst } => {
                let one = Value::Int(1);
                let cur = if let Some(slot) = vm.lookup_global_slot(src) {
                    vm.global_slot_value(slot)
                        .ok_or_else(|| vm_err("invalid global slot"))?
                        .clone()
                } else {
                    return Err(not_bound_err(format!(
                        "'{src}' has not been bound to a value"
                    )));
                };
                let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &cur, &one) {
                    v
                } else {
                    eval_binary(&BinaryOperator::Add, cur, one)?
                };
                if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.assign_global_at_slot(dst, slot, new_val);
                } else if let Some(slot) = vm.lookup_global_slot(dst) {
                    vm.inline_cache[idx].slot = Some(slot);
                    vm.assign_global_at_slot(dst, slot, new_val);
                } else {
                    vm.assign_global(dst, new_val);
                }
            }

            Instruction::Inc1VarKeep(name) => {
                let one = Value::Int(1);
                let cur = if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.global_slot_value(slot)
                        .ok_or_else(|| vm_err("invalid global slot"))?
                        .clone()
                } else if let Some(slot) = vm.lookup_global_slot(name) {
                    vm.inline_cache[idx].slot = Some(slot);
                    vm.global_slot_value(slot)
                        .ok_or_else(|| vm_err("invalid global slot"))?
                        .clone()
                } else {
                    return Err(not_bound_err(format!(
                        "'{name}' has not been bound to a value"
                    )));
                };
                let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &cur, &one) {
                    v
                } else {
                    eval_binary(&BinaryOperator::Add, cur, one)?
                };
                if let Some(slot) = vm.inline_cache[idx].slot {
                    vm.assign_global_at_slot(name, slot, new_val.clone());
                } else {
                    vm.assign_global(name, new_val.clone());
                }
                vm.stack.push(new_val);
            }

            Instruction::Inc1LocalFromLocal { src, dst } => {
                let src = *src;
                let dst = *dst;
                let one = Value::Int(1);
                let base = {
                    let src_ref = vm
                        .locals
                        .last()
                        .and_then(|f| f.get(src as usize))
                        .ok_or_else(|| vm_err(format!("invalid local slot {src}")))?;
                    src_ref.read()
                };
                let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &base, &one) {
                    v
                } else {
                    eval_binary(&BinaryOperator::Add, base, one)?
                };
                let dst_ref = vm.local_slot_mut(dst)?;
                dst_ref.write(new_val);
            }

            Instruction::Try(len) => {
                let len = *len;
                let start_pc = vm.pc;
                let end_pc = start_pc + len;
                let stack_start = vm.stack.len();
                match self.execute(vm, end_pc) {
                    Ok(v) => {
                        vm.stack.truncate(stack_start);
                        vm.stack.push(Value::List(vec![v, Value::Int(0)]));
                    }
                    Err(e) => {
                        vm.stack.truncate(stack_start);
                        let mut err_list = vec![e.err_type.to_string().into_wq_value()];
                        if let Some(m) = e.msg {
                            err_list.push(m.into_wq_value());
                        }
                        if let Some(s) = e.src {
                            err_list.push(s.into_wq_value());
                        }
                        for note in e.notes {
                            err_list.push(note.into_wq_value());
                        }
                        vm.stack.push(Value::List(vec![
                            Value::List(err_list),
                            e.err_type.to_code().into_wq_value(),
                        ]));
                    }
                }
                vm.pc = end_pc;
            }
            Instruction::LoadCapture(i) => {
                let idx = *i as usize;
                let cap_num = *i;
                let cell = vm
                    .captures
                    .last()
                    .and_then(|c| c.get(idx))
                    .ok_or_else(|| vm_err(format!("invalid capture slot {cap_num}")))?;
                vm.stack
                    .push(cell.lock().expect("poisoned capture").clone());
            }
            Instruction::LoadSelf => {
                let me = vm
                    .current_closure_stack
                    .last()
                    .ok_or_else(|| vm_err("LoadSelf outside fn"))?;
                vm.stack.push(me.clone());
            }
            Instruction::LoadClosure(payload) => {
                let locals = payload.locals;
                let captures = &payload.captures;
                let mut captured_vals = Vec::with_capacity(captures.len());
                for cap in captures {
                    match cap {
                        Capture::Local(slot) => {
                            let slot_idx = *slot as usize;
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
                        Capture::FromCapture(i) => {
                            let cap_idx = *i as usize;
                            let cell = vm
                                .captures
                                .last()
                                .and_then(|c| c.get(cap_idx))
                                .cloned()
                                .unwrap_or_else(|| Arc::new(Mutex::new(Value::unit())));
                            captured_vals.push(cell);
                        }
                        Capture::Global(name) => {
                            let val = if let Some(v) = vm.lookup_global(name) {
                                v
                            } else {
                                return Err(not_bound_err(format!("'{name}' is not defined")));
                            };
                            captured_vals.push(Arc::new(Mutex::new(val)));
                        }
                    }
                }
                // Register a debug chunk for this closure's code (wqdb or bt mode)
                let mut chunk_opt: Option<ChunkId> = None;
                let instructions = &payload.instructions;
                let dbg_stmt_spans = &payload.dbg_stmt_spans;
                let dbg_local_names = &payload.dbg_local_names;
                let params = &payload.params;
                if vm.wqdb.enabled || vm.bt_mode {
                    let file_id = vm.debug_info.chunk(vm.current_chunk).file_id;
                    let id = vm.debug_info.new_chunk("<fn>", file_id, instructions.len());
                    {
                        let base_offs = vm.debug_src_offset;
                        let table = &mut vm.debug_info.chunk_mut(id).line_table;
                        if !dbg_stmt_spans.is_empty() {
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
                hooks.on_closure_capture_alloc(captured_vals.len());
                vm.stack
                    .push(Value::Closure(Arc::new(crate::value::ClosureData {
                        params: params.clone(),
                        locals,
                        captured: captured_vals,
                        instructions: instructions.clone(),
                        dbg_chunk: chunk_opt,
                        dbg_stmt_spans: Some(dbg_stmt_spans.clone()),
                        dbg_local_names: Some(dbg_local_names.clone()),
                    })));
            }
        }
        Ok(true)
    }
}

fn make_range(
    start: &Value,
    end: &Value,
    step: Option<&Value>,
    inclusive: bool,
) -> WqResult<Value> {
    let start_int = match start {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("expected int for range start")
                .got1(start));
        }
    };
    let end_int = match end {
        Value::Int(n) => *n,
        _ => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("expected int for range end")
                .got1(end));
        }
    };
    let step_int = match step {
        Some(Value::Int(0)) => {
            return Err(WqError::new(WqErrorType::Domain).msg("range step cannot be 0"));
        }
        Some(Value::Int(n)) => *n,
        Some(other) => {
            return Err(WqError::new(WqErrorType::Domain)
                .msg("expected int for range step")
                .got1(other));
        }
        None => 1,
    };

    // if step_int > 0 && start_int > end_int {
    //     step_int = -step_int;
    // }

    let mut cur = start_int;

    let capacity = if step_int > 0 && end_int >= start_int {
        let diff = end_int.abs_diff(start_int);
        let steps = diff / step_int as u64;
        let mut cap = usize::try_from(steps).unwrap_or(0);
        if inclusive || diff % (step_int as u64) != 0 {
            cap += 1;
        }
        cap
    } else if step_int < 0 && start_int >= end_int {
        let diff = start_int.abs_diff(end_int);
        let steps = diff / step_int.unsigned_abs();
        let mut cap = usize::try_from(steps).unwrap_or(0);
        if inclusive || diff % step_int.unsigned_abs() != 0 {
            cap += 1;
        }
        cap
    } else {
        0
    };

    let mut items: Vec<i64> = Vec::with_capacity(capacity);

    if step_int > 0 {
        while if inclusive {
            cur <= end_int
        } else {
            cur < end_int
        } {
            items.push(cur);
            cur = cur
                .checked_add(step_int)
                .ok_or_else(|| WqError::new(WqErrorType::NumericOverflow).msg("range overflow"))?;
        }
    } else {
        while if inclusive {
            cur >= end_int
        } else {
            cur > end_int
        } {
            items.push(cur);
            cur = cur
                .checked_add(step_int)
                .ok_or_else(|| WqError::new(WqErrorType::NumericOverflow).msg("range overflow"))?;
        }
    }
    Ok(Value::IntList(items))
}

#[inline]
pub(crate) fn range_alloc_len(value: &Value) -> usize {
    match value {
        Value::IntList(items) => items.len(),
        Value::List(items) => items.len(),
        _ => 0,
    }
}

#[inline]
fn vm_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Vm)
        .src("default interp")
        .msg(msg.into())
}

#[inline]
fn not_bound_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::NotBound)
        .src("default interp")
        .msg(msg.into())
}

#[inline]
fn domain_err_vm(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Domain).src("vm").msg(msg.into())
}

#[inline]
fn index_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Index).src("vm").msg(msg.into())
}
