use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

use crate::{
    astnode::BinaryOperator,
    value::{Excerpt, IntoWqValue, Value, ValueCell, WqResult, eval_binary, eval_unary},
    vm::{
        CallTarget, Frame, InlineCache, ResolvedCfn, ResolvedClosure, Slot, Vm,
        cmpchain::eval_cmp_chain,
        fastpath::fp_int_binary_op,
        instruction::{Capture, Instruction},
    },
    wqdb::{ChunkId, CodeLoc, DebugHost, apply_stmt_spans_exact_offs, mark_stmt_heuristic},
    wqerr::{WqErr, WqErrType},
};

use indexmap::IndexMap;

impl Vm {
    pub fn run(&mut self) -> WqResult<Value> {
        let limit = self.instructions.len();
        self.execute_until(limit)
    }

    fn execute_until(&mut self, limit: usize) -> WqResult<Value> {
        if limit > self.instructions.len() {
            return Err(vm_err(format!("limit out of bounds: {limit}")));
        }
        while self.pc < limit {
            if self.wqdb.enabled {
                let here = CodeLoc {
                    chunk: self.current_chunk,
                    pc: self.pc,
                };
                let depth = self.call_depth();
                // wqdb on_pause hook
                if self.wqdb.should_pause_at(&self.debug_info, here, depth) {
                    let cb = self.wqdb.on_pause;
                    self.wqdb.note_pause(here);
                    if let Some(f) = cb {
                        f(self);
                    }
                }
            }
            let idx = self.pc;
            self.pc += 1;
            let op = &self.instructions[idx];
            // let op = unsafe { self.instructions.get_unchecked(idx) };
            match op {
                Instruction::LoadConst(v) => self.stack.push(v.clone()),
                Instruction::LoadVar(name) => {
                    let use_cache = !self.is_internal_ephemeral(name);
                    if use_cache
                        && self.inline_cache[idx].version == self.global_version
                        && let Some(v) = self.inline_cache[idx].value.as_ref()
                    {
                        self.stack.push(v.clone());
                        continue;
                    }

                    let (val, ver) = if let Some(val) = self.lookup_global(name) {
                        (val, self.global_version)
                    } else if self.builtins.has_function(name) {
                        (Value::BuiltinFunction(name.clone()), u64::MAX)
                    } else {
                        return Err(not_bound_err(format!(
                            "'{name}' has not been bound to a value"
                        )));
                    };

                    if use_cache {
                        let cache = &mut self.inline_cache[idx];
                        cache.version = ver;
                        cache.value = Some(val.clone());
                    }
                    self.stack.push(val);
                }
                Instruction::StoreVar(name) => {
                    let name = name.clone();
                    let val = pop1_stack(&mut self.stack, || {
                        Cow::Owned(format!("store into variable '{name}'"))
                    })?;
                    self.assign_global(&name, val);
                }
                Instruction::StoreVarKeep(name) => {
                    let name = name.clone();
                    let val = last_clone_stack(&self.stack, || {
                        Cow::Owned(format!("store into variable '{name}'"))
                    })?;
                    self.assign_global(&name, val);
                }
                Instruction::LoadLocal(i) => {
                    let slot = *i as usize;
                    let slot_num = *i;
                    let val = self
                        .locals
                        .last()
                        .and_then(|f| f.get(slot))
                        .ok_or_else(|| vm_err(format!("invalid local slot {slot_num}")))?;
                    self.stack.push(val.read());
                }
                Instruction::StoreLocal(i) => {
                    let slot = *i as usize;
                    let slot_num = *i;
                    let val = pop1_stack(&mut self.stack, || {
                        Cow::Owned(format!("store into local slot {slot_num}"))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
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
                    let val = last_clone_stack(&self.stack, || {
                        Cow::Owned(format!("store into local slot {slot_num}"))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
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
                    self.stack.pop();
                }
                Instruction::Return => break,

                Instruction::BinaryOp(op) => {
                    let op = *op;
                    let (left, right) =
                        pop2_stack(&mut self.stack, || Cow::Owned(format!("binary op {op:?}")))?;
                    if let Some(res) = fp_int_binary_op(op, &left, &right) {
                        self.stack.push(res);
                        continue;
                    }
                    let result = eval_binary(&op, left, right)?;
                    self.stack.push(result);
                }
                Instruction::CmpChain(ops) => {
                    let ops = ops.as_slice();
                    let need = ops.len() + 1;
                    ensure_stack_len(&self.stack, need, || {
                        Cow::Owned(format!("comparison chain of length {}", ops.len()))
                    })?;
                    let mut values = Vec::with_capacity(need);
                    for _ in 0..need {
                        // ensure_stack_len guarantees pop succeeds
                        if let Some(v) = self.stack.pop() {
                            values.push(v);
                        }
                    }
                    values.reverse();
                    let result = eval_cmp_chain(ops, &values)?;
                    self.stack.push(result);
                }
                Instruction::UnaryOp(op) => {
                    let op = *op;
                    let val =
                        pop1_stack(&mut self.stack, || Cow::Owned(format!("unary op {op:?}")))?;
                    let result = eval_unary(&op, val)?;
                    self.stack.push(result);
                }

                Instruction::FloorDiv => {
                    let (left, right) =
                        pop2_stack(&mut self.stack, || Cow::Borrowed("floor-div operands"))?;
                    if let Some(v) = crate::vm::fastpath::fp_floor_div(&left, &right) {
                        self.stack.push(v);
                        continue;
                    }
                    // Fallback: compute divide then floor with full semantics (incl. bc)
                    let v = left
                        .divide(&right)
                        .and_then(|x| x.floor())
                        .map_err(|e| e.into_wqerror().src("floor-div"))?;
                    self.stack.push(v);
                }

                Instruction::MakeList(n) => {
                    let count = *n;
                    ensure_stack_len(&self.stack, count, || Cow::Borrowed("list elements"))?;
                    let base = self.stack.len() - count;
                    let mut items = Vec::with_capacity(count);
                    items.extend(self.stack.drain(base..));
                    self.stack.push(Value::from_items(items));
                }
                Instruction::MakeDict(n) => {
                    let count = *n;
                    let mut pairs = Vec::with_capacity(count);
                    for _ in 0..count {
                        let val = pop1_stack(&mut self.stack, || Cow::Borrowed("dict value"))?;
                        let key = match pop1_stack(&mut self.stack, || Cow::Borrowed("dict key"))? {
                            Value::Symbol(k) => k,
                            other => {
                                return Err(
                                    vm_err("invalid dict key, expected symbol").got1(&other)
                                );
                            }
                        };
                        pairs.push((key, val));
                    }
                    let mut map = IndexMap::with_capacity(count);
                    while let Some((k, v)) = pairs.pop() {
                        // reverse the pop order
                        map.insert(k, v);
                    }
                    self.stack.push(Value::Dict(map));
                }
                Instruction::MakeRange {
                    inclusive,
                    has_step,
                } => {
                    let inclusive = *inclusive;
                    let has_step = *has_step;
                    let step_val = if has_step {
                        Some(pop1_stack(&mut self.stack, || Cow::Borrowed("range step"))?)
                    } else {
                        None
                    };
                    let end_val = pop1_stack(&mut self.stack, || Cow::Borrowed("range end"))?;
                    let start_val = pop1_stack(&mut self.stack, || Cow::Borrowed("range start"))?;
                    let res = make_range(&start_val, &end_val, step_val.as_ref(), inclusive)
                        .map_err(|e| e.src("vm"))?;
                    self.stack.push(res);
                }

                Instruction::CallBuiltinId(id, argc) => {
                    let builtin_id = *id;
                    let argc = *argc;
                    let result = self.builtin_from_stack_by_id(builtin_id, argc)?;
                    self.stack.push(result);
                }
                Instruction::CallLocal(slot, argc) => {
                    let argc = *argc;
                    let slot = *slot;
                    ensure_stack_len(&self.stack, argc, || {
                        Cow::Owned(format!("local call slot {slot} args"))
                    })?;
                    let argc_val = argc;
                    let slot_usize = slot as usize;

                    let callable = {
                        let mut found: Option<LocalCallable> = None;
                        for fi in (0..self.locals.len()).rev() {
                            let peeked = if let Some(v) = self.locals[fi].get(slot_usize) {
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
                                    let dbg_new = self.ensure_dbg_chunk_with_spans(
                                        "<fn>",
                                        p.dbg_chunk,
                                        &p.instructions,
                                        &p.spans,
                                        &p.names,
                                        &p.params,
                                    );
                                    if dbg_new != p.dbg_chunk
                                        && let Some(slot_ref) = self
                                            .locals
                                            .get_mut(fi)
                                            .and_then(|f| f.get_mut(slot_usize))
                                    {
                                        slot_ref.with_mut(|value| {
                                            if let Value::CompiledFunction { dbg_chunk, .. }
                                            | Value::Closure { dbg_chunk, .. } = value
                                            {
                                                *dbg_chunk = dbg_new;
                                            }
                                        });
                                    }

                                    found = Some(LocalCallable::Func {
                                        value: p.value.clone(),
                                        params: p.params,
                                        locals: p.locals,
                                        instructions: p.instructions,
                                        captured: if p.is_closure {
                                            p.captured
                                        } else {
                                            Vec::new()
                                        },
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
                            let res = self.call_function(CallSpec {
                                instructions,
                                params,
                                locals,
                                captured,
                                argc: argc_val,
                                callee_name: CallSpec::name_hint(name_hint.as_deref()),
                                dbg_chunk,
                                callee: value,
                            })?;
                            self.stack.push(res);
                        }
                        LocalCallable::Builtin(name) => {
                            let result = self.builtin_from_stack_by_name(&name, argc_val)?;
                            self.stack.push(result);
                        }
                    }
                }
                Instruction::CallUser(name, argc) => {
                    let argc = *argc;
                    let name = name.clone();
                    ensure_stack_len(&self.stack, argc, || {
                        Cow::Owned(format!("fn '{name}' args"))
                    })?;
                    if self.inline_cache[idx].version == self.global_version
                        && let Some(ref target) = self.inline_cache[idx].call_target
                    {
                        match target {
                            CallTarget::Cfn(ResolvedCfn {
                                value,
                                params,
                                locals,
                                code,
                                dbg_chunk,
                            }) => {
                                let res = self.call_function(CallSpec {
                                    instructions: code.clone(),
                                    params: params.clone(),
                                    locals: *locals,
                                    captured: Vec::new(),
                                    argc,
                                    callee_name: CallSpec::name_hint(Some(name.as_str())),
                                    dbg_chunk: *dbg_chunk,
                                    callee: value.clone(),
                                })?;
                                self.stack.push(res);
                                continue;
                            }
                            CallTarget::Closure(ResolvedClosure {
                                value,
                                params,
                                locals,
                                captured,
                                code,
                                dbg_chunk,
                            }) => {
                                let res = self.call_function(CallSpec {
                                    instructions: code.clone(),
                                    params: params.clone(),
                                    locals: *locals,
                                    captured: captured.clone(),
                                    argc,
                                    callee_name: CallSpec::name_hint(Some(name.as_str())),
                                    dbg_chunk: *dbg_chunk,
                                    callee: value.clone(),
                                })?;
                                self.stack.push(res);
                                continue;
                            }
                        }
                    }
                    let func = self.resolve_user_callable(idx, &name)?;
                    if let Value::BuiltinFunction(bname) = &func {
                        let out = self.builtin_from_stack_by_name(bname, argc)?;
                        self.stack.push(out);
                    } else {
                        // Reuse the exact same path as CallOrIndex:
                        let base = self.stack.len() - argc;
                        let mut args = Vec::with_capacity(argc);
                        args.extend(self.stack.drain(base..));
                        let out = self.call_value_with_args(&func, args)?;
                        self.stack.push(out);
                    }
                }
                Instruction::CallAnon(argc) => {
                    let argc = *argc;
                    ensure_stack_len(&self.stack, argc + 1, || Cow::Borrowed("callable + args"))?;
                    let len = self.stack.len();
                    let base = len - argc;
                    let mut args = Vec::with_capacity(argc);
                    args.extend(self.stack.drain(base..));
                    let func = self
                        .stack
                        .pop()
                        .ok_or_else(|| vm_err("stack underflow while retrieving callable"))?;
                    let res = self.call_value_with_args(&func, args)?;
                    self.stack.push(res);
                }
                Instruction::CallOrIndex(argc) => {
                    let argc = *argc;
                    ensure_stack_len(&self.stack, argc + 1, || Cow::Borrowed("obj + args"))?;
                    let len = self.stack.len();
                    let base = len - argc;
                    let obj_index = base - 1;
                    let is_call = self.stack.get(obj_index).is_some_and(Value::is_callable);
                    if is_call {
                        let mut args = Vec::with_capacity(argc);
                        args.extend(self.stack.drain(base..));
                        let func = self
                            .stack
                            .pop()
                            .ok_or_else(|| vm_err("stack underflow while calling"))?;
                        let res = self.call_value_with_args(&func, args)?;
                        self.stack.push(res);
                    } else {
                        // index path
                        let mut args = Vec::with_capacity(argc);
                        args.extend(self.stack.drain(base..));
                        let obj = match self.stack.pop() {
                            Some(v) => v,
                            None => return Err(vm_err("stack underflow while indexing")),
                        };
                        let idx_val = if args.len() == 1 {
                            args.pop().unwrap()
                        } else {
                            Value::from_items(args)
                        };
                        match obj.index(&idx_val) {
                            Some(v) => self.stack.push(v),
                            None => {
                                return Err(index_err("invalid index")
                                    .attach_note(format!("index: '{}'", idx_val.excerpt()))
                                    .attach_note(format!("target: '{}'", obj.excerpt())));
                            }
                        }
                    }
                }
                Instruction::Index => {
                    let idx = pop1_stack(&mut self.stack, || Cow::Borrowed("index"))?;
                    let obj = pop1_stack(&mut self.stack, || Cow::Borrowed("object for indexing"))?;
                    match obj.index(&idx) {
                        Some(v) => self.stack.push(v),
                        None => {
                            return Err(index_err("invalid index")
                                .attach_note(format!("index: '{}'", idx.excerpt()))
                                .attach_note(format!("target: '{}'", obj.excerpt())));
                        }
                    }
                }
                Instruction::IndexAssign => {
                    let val =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index assignment value"))?;
                    let idx =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index for assignment"))?;
                    let obj_name = pop1_stack(&mut self.stack, || {
                        Cow::Borrowed("target object name for index assignment")
                    })?;
                    match obj_name {
                        Value::Symbol(name) => {
                            let obj = self.globals.get_mut(&name).ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!(
                                        "when trying to assign to '{name}[{idx}]'"
                                    ))
                            })?;
                            if obj.assign_by_index(&idx, val.clone()).is_some() {
                                self.stack.push(val);
                            } else {
                                return Err(index_err(format!("invalid index '{idx}'"))
                                    .attach_note(format!(
                                        "when trying to assign to {name}[{idx}]"
                                    )));
                            }
                        }
                        other => {
                            return Err(vm_err("invalid index assignment target, expected symbol")
                                .got1(&other));
                        }
                    }
                }
                Instruction::IndexAssignDrop => {
                    let val =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index assignment value"))?;
                    let idx =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index for assignment"))?;
                    let obj_name = pop1_stack(&mut self.stack, || {
                        Cow::Borrowed("target object name for index assignment")
                    })?;
                    match obj_name {
                        Value::Symbol(name) => {
                            let obj = self.globals.get_mut(&name).ok_or_else(|| {
                                not_bound_err(format!("'{name}' has not been bound to a value"))
                                    .attach_note(format!(
                                        "when trying to assign to '{name}[{idx}]'"
                                    ))
                            })?;
                            if obj.assign_by_index(&idx, val).is_none() {
                                return Err(index_err(format!("invalid index '{idx}'"))
                                    .attach_note(format!(
                                        "when trying to assign to {name}[{idx}]"
                                    )));
                            }
                            // Drop result
                        }
                        other => {
                            return Err(vm_err("invalid index assignment target, expected symbol")
                                .got1(&other));
                        }
                    }
                }
                Instruction::IndexAssignLocal(slot) => {
                    let slot = *slot;
                    let val =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index assignment value"))?;
                    let idx =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index for assignment"))?;
                    let assigned = {
                        let frame = self
                            .locals
                            .last_mut()
                            .ok_or_else(|| vm_err("no local frame"))?;
                        let slot_ref = frame
                            .get_mut(slot as usize)
                            .ok_or_else(|| vm_err(format!("invalid local slot {slot}")))?;
                        slot_ref.with_mut(|target| target.assign_by_index(&idx, val.clone()))
                    };
                    if assigned.is_some() {
                        self.stack.push(val);
                    } else {
                        return Err(index_err(format!("invalid index '{idx}'")).attach_note(
                            format!("when trying to assign to local[{slot}][{idx}]"),
                        ));
                    }
                }
                Instruction::IndexAssignLocalDrop(slot) => {
                    let slot = *slot;
                    let val =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index assignment value"))?;
                    let idx =
                        pop1_stack(&mut self.stack, || Cow::Borrowed("index for assignment"))?;
                    let success = {
                        let slot_ref = self.local_slot_mut(slot)?;
                        slot_ref.with_mut(|target| target.assign_by_index(&idx, val))
                    };
                    if success.is_none() {
                        return Err(index_err(format!("invalid index '{idx}'")).attach_note(
                            format!("when trying to assign to local[{slot}][{idx}]"),
                        ));
                    }
                    // Drop result
                }
                Instruction::Jump(pos) => self.pc = *pos,
                Instruction::JumpIfFalse(pos) => {
                    let target = *pos;
                    let v = pop1_stack(&mut self.stack, || Cow::Borrowed("conditional jump"))?;
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
                        self.pc = target;
                    }
                }
                Instruction::JumpIfGE(pos) => {
                    let target = *pos;
                    // Pop right then left, jump if left >= right
                    let (left, right) = pop2_stack(&mut self.stack, || {
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
                        self.pc = target;
                    }
                }
                Instruction::JumpIfLEZLocal(slot, pos) => {
                    let slot_num = *slot as usize;
                    let target = *pos;
                    // Jump if local[slot] <= 0
                    let slot_ref = self
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
                        self.pc = target;
                    }
                }

                Instruction::Inc1Local(slot) => {
                    let slot = *slot;
                    let one = Value::Int(1);
                    let new_val = {
                        let slot_ref = self.local_slot_mut(slot)?;
                        let old = slot_ref.read();
                        if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &old, &one) {
                            v
                        } else {
                            eval_binary(&BinaryOperator::Add, old, one)?
                        }
                    };
                    // Write back
                    let slot_ref = self.local_slot_mut(slot)?;
                    slot_ref.write(new_val);
                }

                Instruction::Inc1LocalKeep(slot) => {
                    let slot = *slot;
                    let one = Value::Int(1);
                    let new_val = {
                        let slot_ref = self.local_slot_mut(slot)?;
                        let old = slot_ref.read();
                        if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &old, &one) {
                            v
                        } else {
                            eval_binary(&BinaryOperator::Add, old, one)?
                        }
                    };
                    let slot_ref = self.local_slot_mut(slot)?;
                    slot_ref.write(new_val.clone());
                    self.stack.push(new_val);
                }

                Instruction::Inc1Var(name) => {
                    let name = name.clone();
                    let one = Value::Int(1);
                    let cur = self.lookup_global(&name).ok_or_else(|| {
                        not_bound_err(format!("'{name}' has not been bound to a value"))
                    })?;
                    let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &cur, &one)
                    {
                        v
                    } else {
                        eval_binary(&BinaryOperator::Add, cur, one)?
                    };
                    self.assign_global(&name, new_val);
                }

                Instruction::Inc1VarFromVar { src, dst } => {
                    let src = src.clone();
                    let dst = dst.clone();
                    let one = Value::Int(1);
                    let cur = self.lookup_global(&src).ok_or_else(|| {
                        not_bound_err(format!("'{src}' has not been bound to a value"))
                    })?;
                    let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &cur, &one)
                    {
                        v
                    } else {
                        eval_binary(&BinaryOperator::Add, cur, one)?
                    };
                    self.assign_global(&dst, new_val);
                }

                Instruction::Inc1VarKeep(name) => {
                    let name = name.clone();
                    let one = Value::Int(1);
                    let cur = self.lookup_global(&name).ok_or_else(|| {
                        not_bound_err(format!("'{name}' has not been bound to a value"))
                    })?;
                    let new_val = if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &cur, &one)
                    {
                        v
                    } else {
                        eval_binary(&BinaryOperator::Add, cur, one)?
                    };
                    self.assign_global(&name, new_val.clone());
                    self.stack.push(new_val);
                }

                Instruction::Inc1LocalFromLocal { src, dst } => {
                    let src = *src;
                    let dst = *dst;
                    let one = Value::Int(1);
                    let base = {
                        let src_ref = self
                            .locals
                            .last()
                            .and_then(|f| f.get(src as usize))
                            .ok_or_else(|| vm_err(format!("invalid local slot {src}")))?;
                        src_ref.read()
                    };
                    let new_val =
                        if let Some(v) = fp_int_binary_op(BinaryOperator::Add, &base, &one) {
                            v
                        } else {
                            eval_binary(&BinaryOperator::Add, base, one)?
                        };
                    let dst_ref = self.local_slot_mut(dst)?;
                    dst_ref.write(new_val);
                }

                Instruction::Try(len) => {
                    let len = *len;
                    let start_pc = self.pc;
                    let end_pc = start_pc + len;
                    let stack_start = self.stack.len();
                    match self.execute_until(end_pc) {
                        Ok(v) => {
                            self.stack.truncate(stack_start);
                            self.stack.push(Value::List(vec![v, Value::Int(0)]));
                        }
                        Err(e) => {
                            self.stack.truncate(stack_start);
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
                            self.stack.push(Value::List(vec![
                                Value::List(err_list),
                                e.err_type.to_code().into_wq_value(),
                            ]));
                        }
                    }
                    self.pc = end_pc;
                }
                Instruction::LoadCapture(i) => {
                    let idx = *i as usize;
                    let cap_num = *i;
                    let cell = self
                        .captures
                        .last()
                        .and_then(|c| c.get(idx))
                        .ok_or_else(|| vm_err(format!("invalid capture slot {cap_num}")))?;
                    self.stack
                        .push(cell.lock().expect("poisoned capture").clone());
                }
                Instruction::LoadSelf => {
                    let me = self
                        .current_closure_stack
                        .last()
                        .ok_or_else(|| vm_err("LoadSelf outside fn"))?;
                    self.stack.push(me.clone());
                }
                Instruction::LoadClosure {
                    params,
                    locals,
                    captures,
                    instructions,
                    dbg_stmt_spans,
                    dbg_local_names,
                } => {
                    let locals = *locals;
                    let mut captured_vals = Vec::with_capacity(captures.len());
                    for cap in captures {
                        match cap {
                            Capture::Local(slot) => {
                                let slot_idx = *slot as usize;
                                let val = if let Some(parent) = self.locals.last() {
                                    parent
                                        .get(slot_idx)
                                        .map(|s| s.read())
                                        .unwrap_or_else(Value::unit)
                                } else {
                                    Value::unit()
                                };
                                captured_vals.push(Arc::new(Mutex::new(val)));
                            }
                            // Capture::LocalShared(slot) => {
                            //     let slot_idx = *slot as usize;
                            //     let cell = if let Some(parent) = self.locals.last_mut() {
                            //         parent
                            //             .get_mut(slot_idx)
                            //             .map(|s| s.ensure_cell())
                            //             .unwrap_or_else(|| Arc::new(Mutex::new(Value::unit())))
                            //     } else {
                            //         Arc::new(Mutex::new(Value::unit()))
                            //     };
                            //     captured_vals.push(cell);
                            // }
                            Capture::FromCapture(i) => {
                                let cap_idx = *i as usize;
                                let cell = self
                                    .captures
                                    .last()
                                    .and_then(|c| c.get(cap_idx))
                                    .cloned()
                                    .unwrap_or_else(|| Arc::new(Mutex::new(Value::unit())));
                                captured_vals.push(cell);
                            }
                            Capture::Global(name) => {
                                let val = if let Some(v) = self.lookup_global(name) {
                                    v
                                }
                                // do not capture builtins
                                // else if self.builtins.get_id(name).is_some() {
                                //     Value::BuiltinFunction(name.clone())
                                // }
                                else {
                                    return Err(not_bound_err(format!("'{name}' is not defined")));
                                };
                                captured_vals.push(Arc::new(Mutex::new(val)));
                            }
                        }
                    }
                    // Register a debug chunk for this closure's code (wqdb or bt mode)
                    let mut chunk_opt: Option<ChunkId> = None;
                    if self.wqdb.enabled || self.bt_mode {
                        let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                        let id = self
                            .debug_info
                            .new_chunk("<fn>", file_id, instructions.len());
                        {
                            let base_offs = self.debug_src_offset();
                            let table = &mut self.debug_info.chunk_mut(id).line_table;
                            // Prefer exact span mapping when available.
                            // shifted by the current script's base byte offset,
                            // so backtraces point to the correct location in the full source
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
                            self.debug_info.chunk_mut(id).local_names =
                                Some(dbg_local_names.iter().cloned().collect());
                        } else if let Some(ps) = params.as_ref() {
                            self.debug_info.chunk_mut(id).local_names =
                                Some(ps.iter().cloned().collect());
                        }
                        chunk_opt = Some(id);
                    }
                    self.stack.push(Value::Closure {
                        params: params.clone(),
                        locals,
                        captured: captured_vals,
                        instructions: instructions.clone(),
                        dbg_chunk: chunk_opt,
                        dbg_stmt_spans: Some(dbg_stmt_spans.clone()),
                        dbg_local_names: Some(dbg_local_names.clone()),
                    });
                }
            }
        }
        Ok(self.stack.pop().unwrap_or(Value::unit()))
    }

    fn call_function(&mut self, spec: CallSpec) -> WqResult<Value> {
        let CallSpec {
            instructions,
            params,
            locals: local_count,
            mut captured,
            argc,
            callee_name,
            dbg_chunk,
            callee,
        } = spec;
        // Determine or create a debug chunk for the callee (only if debugging)
        let callee_chunk = if self.wqdb.enabled || self.bt_mode {
            if let Some(id) = dbg_chunk {
                id
            } else {
                let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                let title = callee_name.as_deref().unwrap_or("<fn>").to_string();
                let id = self
                    .debug_info
                    .new_chunk(title, file_id, instructions.len());
                let table = &mut self.debug_info.chunk_mut(id).line_table;
                // Heuristic stepping if no spans available from the call site
                mark_stmt_heuristic(table, instructions.as_ref());
                id
            }
        } else {
            self.current_chunk
        };
        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        let saved_pc = self.pc;
        // Preserve capacity, avoid reallocs across call
        let prev_cap = self.stack.capacity();
        let mut saved_stack = std::mem::replace(
            &mut self.stack,
            Vec::with_capacity(std::cmp::max(prev_cap, 256)),
        );
        let saved_cache = std::mem::replace(
            &mut self.inline_cache,
            vec![InlineCache::default(); self.instructions.len()],
        );
        // Push debug frame
        let mut pushed_dbg = false;
        if self.wqdb.enabled || self.bt_mode {
            let caller_chunk = self.current_chunk;
            self.call_stack.push(Frame {
                chunk: caller_chunk,
                pc: saved_pc,
                func_name: std::sync::Arc::from(self.func_name_for_chunk(caller_chunk)),
            });
            self.current_chunk = callee_chunk;
            pushed_dbg = true;
        }
        self.pc = 0;
        let mut frame = vec![Slot::default(); local_count as usize];
        // Arity check
        if let Some(p) = params.as_ref() {
            if argc != p.len() {
                return Err(arity_err_vm(format!(
                    "function expects {} arg(s), got {}",
                    p.len(),
                    argc
                )));
            }
        } else if argc > 3 {
            return Err(arity_err_vm(
                "implicit function expects up to 3 args".to_string(),
            ));
        }
        // Move args from caller’s stack to callee frame (arg0..argN-1)
        for i in (0..argc).rev() {
            let v = saved_stack
                .pop()
                .ok_or_else(|| vm_err("stack underflow while moving args"))?;
            frame[i] = Slot::Value(v);
        }
        self.locals.push(frame);
        self.captures.push(std::mem::take(&mut captured));
        self.current_closure_stack.push(callee);
        let limit = self.instructions.len();
        let res = self
            .execute_until(limit)
            .inspect_err(|_| self.capture_bt_if_empty());
        self.current_closure_stack.pop();
        // Unwind
        self.locals.pop();
        self.captures.pop();
        std::mem::swap(&mut self.stack, &mut saved_stack);
        self.instructions = saved_instructions;
        self.pc = saved_pc;
        self.inline_cache = saved_cache;
        if pushed_dbg && let Some(fr) = self.call_stack.pop() {
            self.current_chunk = fr.chunk;
        }
        res
    }

    #[inline]
    fn call_builtin(&mut self, name: &str, args: &[Value]) -> WqResult<Value> {
        let id = self
            .builtins
            .get_id(name)
            .ok_or_else(|| not_bound_err(format!("Unknown bfn: {name}")))?;
        self.call_builtin_id(
            id.try_into().map_err(|_| vm_err("builtin id overflow"))?,
            args,
        )
    }

    #[inline]
    fn call_builtin_id(&mut self, id: u16, args: &[Value]) -> WqResult<Value> {
        let func = *self
            .builtins
            .get_fn_by_id(usize::from(id))
            .ok_or_else(|| vm_err("invalid builtin id"))?;
        (func)(self, args)
    }

    pub fn call_value(&mut self, func: &Value, args: &[Value]) -> WqResult<Value> {
        if let Value::BuiltinFunction(name) = func {
            return self.call_builtin(name, args);
        }
        self.call_value_with_args(func, args.to_vec())
    }

    pub fn call_value_with_args(&mut self, func: &Value, mut args: Vec<Value>) -> WqResult<Value> {
        // Builtin fast path: no stack shuffling
        if let Value::BuiltinFunction(name) = func {
            return self.call_builtin(name, &args);
        }
        let argc = args.len();
        self.stack.append(&mut args); // moves, no clones
        match func {
            Value::CompiledFunction {
                params,
                locals,
                instructions,
                dbg_chunk,
                ..
            } => self.call_function(CallSpec {
                instructions: instructions.clone(),
                params: params.clone(),
                locals: *locals,
                captured: Vec::new(),
                argc,
                callee_name: None,
                dbg_chunk: *dbg_chunk,
                callee: func.clone(),
            }),
            Value::Closure {
                params,
                locals,
                captured,
                instructions,
                dbg_chunk,
                ..
            } => self.call_function(CallSpec {
                instructions: instructions.clone(),
                params: params.clone(),
                locals: *locals,
                captured: captured.clone(),
                argc,
                callee_name: None,
                dbg_chunk: *dbg_chunk,
                callee: func.clone(),
            }),
            other => Err(not_bound_err(format!(
                "expected fn, got {}",
                other.type_name()
            ))),
        }
    }

    #[inline]
    fn last_frame_mut(&mut self) -> WqResult<&mut Vec<Slot>> {
        self.locals
            .last_mut()
            .ok_or_else(|| vm_err("no local frame"))
    }

    #[inline]
    fn local_slot_mut(&mut self, slot: u16) -> WqResult<&mut Slot> {
        self.last_frame_mut()?
            .get_mut(slot as usize)
            .ok_or_else(|| vm_err(format!("invalid local slot {slot}")))
    }

    #[inline]
    fn cache_compiled(&mut self, idx: usize, rf: ResolvedCfn) {
        let entry = &mut self.inline_cache[idx];
        entry.version = self.global_version;
        entry.call_target = Some(CallTarget::Cfn(rf));
    }

    #[inline]
    fn cache_closure(&mut self, idx: usize, rc: ResolvedClosure) {
        let entry = &mut self.inline_cache[idx];
        entry.version = self.global_version;
        entry.call_target = Some(CallTarget::Closure(rc));
    }

    #[inline]
    fn builtin_from_stack_by_id(&mut self, id: u16, argc: u16) -> WqResult<Value> {
        let argc = usize::from(argc);
        ensure_stack_len(&self.stack, argc, || Cow::Borrowed("builtin args"))?;
        let base = self.stack.len() - argc;
        let ptr = unsafe { self.stack.as_ptr().add(base) };
        let args = unsafe { std::slice::from_raw_parts(ptr, argc) };
        let out = self.call_builtin_id(id, args)?;
        self.stack.truncate(base);
        Ok(out)
    }

    #[inline]
    fn builtin_from_stack_by_name(&mut self, name: &str, argc: usize) -> WqResult<Value> {
        ensure_stack_len(&self.stack, argc, || Cow::Borrowed("builtin args"))?;
        let base = self.stack.len() - argc;
        let ptr = unsafe { self.stack.as_ptr().add(base) };
        let args = unsafe { std::slice::from_raw_parts(ptr, argc) };
        let out = self.call_builtin(name, args)?;
        self.stack.truncate(base);
        Ok(out)
    }

    // #[inline]
    // fn with_args<F>(&mut self, argc: usize, f: F) -> WqResult<Value>
    // where
    //     F: FnOnce(&mut Self, &[Value]) -> WqResult<Value>,
    // {
    //     ensure_stack_len(&self.stack, argc, || Cow::Borrowed("builtin args"))?;
    //     let base = self.stack.len() - argc;

    //     // Moves the tail out; self.stack is truncated here.
    //     let args: Vec<Value> = self.stack.split_off(base);

    //     // Safe, stable slice into a separate Vec. No aliasing with &mut self.
    //     let out = f(self, &args)?;
    //     Ok(out)
    // }

    // #[inline]
    // fn builtin_from_stack_by_id(&mut self, id: u16, argc: u16) -> WqResult<Value> {
    //     self.with_args(argc as usize, |s, args| s.call_builtin_id(id, args))
    // }

    // #[inline]
    // fn builtin_from_stack_by_name(&mut self, name: &str, argc: usize) -> WqResult<Value> {
    //     self.with_args(argc, |s, args| s.call_builtin(name, args))
    // }

    // #[inline]
    // fn with_args<F>(&mut self, argc: usize, f: F) -> WqResult<Value>
    // where
    //     F: FnOnce(&mut Self, &[Value]) -> WqResult<Value>,
    // {
    //     ensure_stack_len(&self.stack, argc, || Cow::Borrowed("builtin args"))?;
    //     let base = self.stack.len() - argc;
    //     let mut buf = std::mem::take(&mut self.args_scratch);
    //     buf.clear(); // keep capacity, no alloc
    //     buf.extend(self.stack.drain(base..)); // moves tail into buf

    //     let out = f(self, &buf)?;

    //     // Put the buffer back for reuse next time.
    //     self.args_scratch = buf;
    //     Ok(out)
    // }

    // #[inline]
    // fn builtin_from_stack_by_id(&mut self, id: u16, argc: u16) -> WqResult<Value> {
    //     self.with_args(argc as usize, |s, args| s.call_builtin_id(id, args))
    // }

    // #[inline]
    // fn builtin_from_stack_by_name(&mut self, name: &str, argc: usize) -> WqResult<Value> {
    //     self.with_args(argc, |s, args| s.call_builtin(name, args))
    // }

    #[inline]
    fn resolve_user_callable(&mut self, idx: usize, name: &str) -> WqResult<Value> {
        // Fast path: cache
        if self.inline_cache[idx].version == self.global_version
            && let Some(ref target) = self.inline_cache[idx].call_target
        {
            return Ok(match target {
                CallTarget::Cfn(ResolvedCfn { value, .. })
                | CallTarget::Closure(ResolvedClosure { value, .. }) => value.clone(),
            });
        }

        // Slow path: resolve from globals
        let func_val = self
            .lookup_global(name)
            .ok_or_else(|| not_bound_err(format!("fn '{name}' is not defined")))?;

        Ok(match func_val {
            Value::CompiledFunction {
                params,
                locals,
                instructions,
                dbg_chunk,
                dbg_stmt_spans,
                dbg_local_names,
            } => {
                let dbg_chunk = self.ensure_dbg_chunk_with_spans(
                    name,
                    dbg_chunk,
                    instructions.as_ref(),
                    &dbg_stmt_spans,
                    &dbg_local_names,
                    &params,
                );
                let value = Value::CompiledFunction {
                    params: params.clone(),
                    locals,
                    instructions: instructions.clone(),
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                };
                self.cache_compiled(
                    idx,
                    ResolvedCfn {
                        value: value.clone(),
                        params,
                        locals,
                        code: instructions,
                        dbg_chunk,
                    },
                );
                value
            }
            Value::Closure {
                params,
                locals,
                captured,
                instructions,
                dbg_chunk,
                dbg_stmt_spans,
                dbg_local_names,
            } => {
                let dbg_chunk = self.ensure_dbg_chunk_with_spans(
                    name,
                    dbg_chunk,
                    instructions.as_ref(),
                    &dbg_stmt_spans,
                    &dbg_local_names,
                    &params,
                );
                let value = Value::Closure {
                    params: params.clone(),
                    locals,
                    captured: captured.clone(),
                    instructions: instructions.clone(),
                    dbg_chunk,
                    dbg_stmt_spans,
                    dbg_local_names,
                };
                self.cache_closure(
                    idx,
                    ResolvedClosure {
                        value: value.clone(),
                        params,
                        locals,
                        captured,
                        code: instructions,
                        dbg_chunk,
                    },
                );
                value
            }
            b @ Value::BuiltinFunction(_) => b, // name resolves to builtin each time
            other => {
                return Err(not_bound_err(format!(
                    "cannot call '{name}': expected fn, got {}",
                    other.type_name()
                )));
            }
        })
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
            return Err(WqErr::new(WqErrType::Domain)
                .msg("expected int for range start")
                .got1(start));
        }
    };
    let end_int = match end {
        Value::Int(n) => *n,
        _ => {
            return Err(WqErr::new(WqErrType::Domain)
                .msg("expected int for range end")
                .got1(end));
        }
    };
    let step_int = match step {
        Some(Value::Int(0)) => {
            return Err(WqErr::new(WqErrType::Domain).msg("range step cannot be 0"));
        }
        Some(Value::Int(n)) => *n,
        Some(other) => {
            return Err(WqErr::new(WqErrType::Domain)
                .msg("expected int for range step")
                .got1(other));
        }
        None => 1,
    };

    // if step_int > 0 && start_int > end_int {
    //     step_int = -step_int;
    // }

    let mut cur = start_int;
    let mut items: Vec<i64> = Vec::new();
    if step_int > 0 {
        while if inclusive {
            cur <= end_int
        } else {
            cur < end_int
        } {
            items.push(cur);
            cur = cur
                .checked_add(step_int)
                .ok_or_else(|| WqErr::new(WqErrType::NumericOverflow).msg("range overflow"))?;
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
                .ok_or_else(|| WqErr::new(WqErrType::NumericOverflow).msg("range overflow"))?;
        }
    }
    Ok(Value::IntList(items))
}

#[inline]
fn vm_err(msg: impl Into<String>) -> WqErr {
    WqErr::new(WqErrType::Vm).src("vm").msg(msg.into())
}

#[inline]
fn index_err(msg: impl Into<String>) -> WqErr {
    WqErr::new(WqErrType::Index).src("vm").msg(msg.into())
}

#[inline]
fn call_err(msg: impl Into<String>) -> WqErr {
    WqErr::new(WqErrType::Call).src("vm").msg(msg.into())
}

#[inline]
fn not_bound_err(msg: impl Into<String>) -> WqErr {
    WqErr::new(WqErrType::NotBound).src("vm").msg(msg.into())
}

#[inline]
fn arity_err_vm(msg: impl Into<String>) -> WqErr {
    WqErr::new(WqErrType::Arity).src("vm").msg(msg.into())
}

#[inline]
fn domain_err_vm(msg: impl Into<String>) -> WqErr {
    WqErr::new(WqErrType::Domain).src("vm").msg(msg.into())
}

#[derive(Clone)]
struct CallSpec<'a> {
    instructions: Arc<[Instruction]>,
    params: Option<Arc<[String]>>,
    locals: u16,
    captured: Vec<ValueCell>,
    argc: usize,
    callee_name: Option<Cow<'a, str>>,
    dbg_chunk: Option<ChunkId>,
    callee: Value,
}

impl<'a> CallSpec<'a> {
    fn name_hint(s: Option<&'a str>) -> Option<Cow<'a, str>> {
        s.map(Cow::Borrowed)
    }
}

enum LocalCallable {
    Func {
        value: Value,
        params: Option<Arc<[String]>>,
        locals: u16,
        instructions: Arc<[Instruction]>,
        captured: Vec<ValueCell>,
        dbg_chunk: Option<ChunkId>,
        name_hint: Option<String>,
    },
    Builtin(String),
}

#[derive(Clone)]
struct PeekFunc {
    is_closure: bool,
    value: Value,
    params: Option<Arc<[String]>>,
    locals: u16,
    instructions: Arc<[Instruction]>,
    dbg_chunk: Option<ChunkId>,
    spans: Option<Arc<[(usize, usize)]>>,
    names: Option<Arc<[String]>>,
    captured: Vec<ValueCell>,
}

enum PeekLocal {
    Builtin(String),
    Func(PeekFunc),
}

#[inline]
fn ensure_stack_len<F>(stack: &[Value], need: usize, ctx: F) -> WqResult<()>
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
fn pop1_stack<F>(stack: &mut Vec<Value>, ctx: F) -> WqResult<Value>
where
    F: FnOnce() -> Cow<'static, str>,
{
    stack
        .pop()
        .ok_or_else(|| vm_err(format!("stack underflow: {}", ctx())))
}

#[inline]
fn pop2_stack<F>(stack: &mut Vec<Value>, ctx: F) -> WqResult<(Value, Value)>
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
fn last_clone_stack<F>(stack: &[Value], ctx: F) -> WqResult<Value>
where
    F: FnOnce() -> Cow<'static, str>,
{
    stack
        .last()
        .cloned()
        .ok_or_else(|| vm_err(format!("stack underflow: {}", ctx())))
}

#[inline]
fn peek_local_callable(slot: u16, v: &Slot) -> WqResult<PeekLocal> {
    v.with_ref(|value| match value {
        Value::CompiledFunction {
            params,
            locals,
            instructions,
            dbg_chunk,
            dbg_stmt_spans,
            dbg_local_names,
        } => Ok(PeekLocal::Func(PeekFunc {
            is_closure: false,
            value: value.clone(),
            params: params.clone(),
            locals: *locals,
            instructions: instructions.clone(),
            dbg_chunk: *dbg_chunk,
            spans: dbg_stmt_spans.clone(),
            names: dbg_local_names.clone(),
            captured: Vec::new(),
        })),
        Value::Closure {
            params,
            locals,
            captured,
            instructions,
            dbg_chunk,
            dbg_stmt_spans,
            dbg_local_names,
        } => Ok(PeekLocal::Func(PeekFunc {
            is_closure: true,
            value: value.clone(),
            params: params.clone(),
            locals: *locals,
            instructions: instructions.clone(),
            dbg_chunk: *dbg_chunk,
            spans: dbg_stmt_spans.clone(),
            names: dbg_local_names.clone(),
            captured: captured.clone(),
        })),
        Value::BuiltinFunction(name) => Ok(PeekLocal::Builtin(name.clone())),
        other => Err(call_err(format!(
            "cannot call local {slot}: expected fn, found {} ({})",
            other.excerpt(),
            other.type_name(),
        ))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        lexer::Lexer,
        parser::Parser,
        post_parser::{folder, resolver::Resolver},
        vm::compiler::Compiler,
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
        vm.run().expect("execute")
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
