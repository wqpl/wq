use std::borrow::Cow;
use std::sync::Arc;

use ahash::AHashMap;
use smallvec::SmallVec;

use crate::builtins::Builtins;
use crate::interpret::vanilla::Sv4;
use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::value::cell::ValueCell;
use crate::value::func::{ClosureData, FunctionData};
use crate::value::{Excerpt, Value, WqResult};
use crate::vm::inst::Instruction;
use crate::vm::slot::Slot;
use crate::vm::{
    Frame, InlineCache, Vm, arity_err_vm, call_err, ensure_stack_len, not_bound_err, vm_err,
};
use crate::wqdb::build::mark_stmt_heuristic;
use crate::wqdb::data::{ChunkId, DebugChunkSpec, DebugPcSpans, DebugStmtSpans};

impl Vm {
    // API for Builtins ============================

    pub(crate) fn call(
        &mut self,
        func: &Value,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        if let Value::BuiltinFunction(name) = func {
            return self.call_builtin_name(name, args);
        }
        let argc = args.len() as u32;
        self.stack.extend(args);
        match func {
            Value::CompiledFunction(f) => self.invoke_spec(CallSpec {
                instructions: f.instructions.clone(),
                params_len: f.params.as_ref().map(|p| p.len() as u32),
                locals: f.locals,
                captured: crate::value::cell::empty_cells(),
                argc,
                callee_name: None,
                dbg_chunk: f.dbg_chunk,
                callee: func.clone(),
            }),
            Value::Closure(c) => self.invoke_spec(CallSpec {
                instructions: c.instructions.clone(),
                params_len: c.params.as_ref().map(|p| p.len() as u32),
                locals: c.locals,
                captured: c.captured.clone(),
                argc,
                callee_name: None,
                dbg_chunk: c.dbg_chunk,
                callee: func.clone(),
            }),
            other => Err(not_bound_err(format!(
                "expected callable, got {}",
                other.type_name()
            ))),
        }
    }

    #[inline]
    pub(crate) fn call_builtin_name(
        &mut self,
        name: &str,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        let id = self
            .builtins
            .get_id(name)
            .ok_or_else(|| not_bound_err(format!("Unknown bfn: {name}")))?;
        self.call_builtin_id(
            id.try_into().map_err(|_| vm_err("builtin id overflow"))?,
            args,
        )
    }

    // API for Interpreter ============================

    pub(crate) fn invoke_spec(&mut self, spec: CallSpec) -> WqResult<Value> {
        let CallSpec {
            instructions,
            params_len,
            locals: local_count,
            captured,
            argc,
            callee_name,
            dbg_chunk,
            mut callee,
        } = spec;
        // Determine or create a debug chunk for the callee (only if debugging)
        let callee_chunk = if self.debug_artifacts_enabled() {
            let preferred_name = |chunk: Option<ChunkId>| {
                callee_name
                    .as_deref()
                    .map(str::to_string)
                    .unwrap_or_else(|| {
                        chunk
                            .map(|id| self.func_name_for_chunk(id))
                            .filter(|name| name != "<?>")
                            .unwrap_or_else(|| "<fn>".to_string())
                    })
            };
            match &callee {
                Value::CompiledFunction(f) => {
                    let title = preferred_name(dbg_chunk.or(f.dbg_chunk));
                    let chunk = self.ensure_dbg_chunk_with_spans(
                        &title,
                        DebugChunkSpec {
                            dbg_chunk: dbg_chunk.or(f.dbg_chunk),
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
                        callee = Value::CompiledFunction(Arc::new(new_f));
                    }
                    chunk.unwrap_or(self.current_chunk)
                }
                Value::Closure(c) => {
                    let title = preferred_name(dbg_chunk.or(c.dbg_chunk));
                    let chunk = self.ensure_dbg_chunk_with_spans(
                        &title,
                        DebugChunkSpec {
                            dbg_chunk: dbg_chunk.or(c.dbg_chunk),
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
                        callee = Value::Closure(Arc::new(new_c));
                    }
                    chunk.unwrap_or(self.current_chunk)
                }
                _ => {
                    let title = preferred_name(dbg_chunk);
                    if let Some(id) = dbg_chunk {
                        id
                    } else {
                        let file_id = self.debug_info.chunk(self.current_chunk).file_id;
                        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
                            eprintln!(
                                "[wqdb]: call_function_with fallback new name={title} file_id={file_id} instructions={}",
                                instructions.len(),
                            );
                        }
                        let id = self
                            .debug_info
                            .new_chunk(title, file_id, instructions.len());
                        let table = &mut self.debug_info.chunk_mut(id).line_table;
                        // Heuristic stepping if no spans are attached to the callee.
                        mark_stmt_heuristic(table, instructions.as_ref());
                        id
                    }
                }
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
        let cache_len = self.instructions.len();
        let new_cache = take_cache_from_pool(&mut self.cache_pool, cache_len);
        let saved_cache = std::mem::replace(&mut self.inline_cache, new_cache);
        let mut saved_tail_journal = std::mem::take(&mut self.tail_call_journal);
        let saved_tail_overflow = self.tail_call_journal_overflow;
        self.tail_call_journal_overflow = false;
        // Push debug frame
        let mut pushed_dbg = false;
        if self.debug_artifacts_enabled() {
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
        let mut frame = take_locals_from_pool(&mut self.locals_pool, local_count);
        // Arity check
        let named_meta = self.pending_named_meta.take();
        if let Some(ref meta) = named_meta {
            // Check positional arity
            if let Some(p) = params_len
                && meta.pos_count as u32 != p
            {
                return Err(arity_err_vm(format!(
                    "function expects {} positional arg(s), got {}",
                    p, meta.pos_count
                )));
            }
        } else if let Some(p) = params_len {
            if argc != p {
                return Err(arity_err_vm(format!(
                    "function expects {} arg(s), got {}",
                    p, argc
                )));
            }
        } else if argc > 3 {
            return Err(arity_err_vm(
                "implicit function expects up to 3 args".to_string(),
            ));
        }

        // Move args from caller’s stack to callee frame (arg0..argN-1)
        if let Some(meta) = named_meta {
            let total = argc as usize;
            // Read all arg values from stack (source order is preserved left-to-right
            // because they were pushed that way)
            let mut all_args: Vec<Value> = (0..total)
                .map(|_| {
                    saved_stack
                        .pop()
                        .ok_or_else(|| vm_err("stack underflow while moving named args"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            all_args.reverse(); // back to source order

            let named_arg_map: std::collections::HashMap<Arc<str>, Value> = meta
                .named
                .iter()
                .filter_map(|(pos, name)| {
                    all_args
                        .get(*pos as usize)
                        .map(|v| (name.clone(), v.clone()))
                })
                .collect();

            // Fill positional slots (skip named entries)
            let mut pos_slot: u32 = 0;
            for (i, v) in all_args.iter().enumerate() {
                let is_named = meta.named.iter().any(|(pos, _)| *pos as usize == i);
                if !is_named {
                    if (pos_slot as usize) < local_count as usize {
                        frame[pos_slot as usize] = Slot::Value(v.clone());
                    }
                    pos_slot += 1;
                }
            }

            // Get callee’s named params
            let callee_named: Option<Arc<[Arc<str>]>> = match &callee {
                Value::CompiledFunction(f) => f.named_params.clone(),
                Value::Closure(c) => c.named_params.clone(),
                _ => {
                    if !named_arg_map.is_empty() {
                        return Err(arity_err_vm(
                            "cannot pass named arguments to a non-function value",
                        ));
                    }
                    None
                }
            };

            // Check that all provided named args match named params
            if let Some(ref named_params) = callee_named {
                for (arg_name, _) in named_arg_map.iter() {
                    if !named_params.iter().any(|p| p.as_ref() == arg_name.as_ref()) {
                        return Err(arity_err_vm(format!(
                            "’{}’ is not a named parameter",
                            arg_name
                        )));
                    }
                }
            }

            // Fill named slots and build bitmask
            let mut mask: i64 = 0;
            if let Some(named_params) = callee_named {
                let pos_param_count = params_len.unwrap_or(0);
                for (named_idx, param_name) in named_params.iter().enumerate() {
                    if let Some(val) = named_arg_map.get(param_name) {
                        let slot = (pos_param_count + named_idx as u32) as usize;
                        if slot < local_count as usize {
                            frame[slot] = Slot::Value(val.clone());
                        }
                        mask |= 1i64 << named_idx;
                    }
                }
                // Set mask slot
                let mask_slot = (pos_param_count + named_params.len() as u32) as usize;
                if mask_slot < local_count as usize {
                    frame[mask_slot] = Slot::Value(Value::Int(mask));
                }
            }
        } else {
            // Original: simple positional arg move
            for i in (0..argc).rev() {
                let v = saved_stack
                    .pop()
                    .ok_or_else(|| vm_err("stack underflow while moving args"))?;
                frame[i as usize] = Slot::Value(v);
            }
        }
        self.locals.push(frame);
        self.captures.push(captured);
        self.current_closure_stack.push(callee);
        // Recursion limit check (tail calls are exempt because they reuse the frame)
        if self.locals.len() > self.max_call_depth {
            self.current_closure_stack.pop();
            if let Some(frame) = self.locals.pop() {
                return_locals_to_pool(&mut self.locals_pool, frame);
            }
            self.captures.pop();
            std::mem::swap(&mut self.stack, &mut saved_stack);
            self.instructions = saved_instructions;
            self.pc = saved_pc;
            let unused_cache = std::mem::replace(&mut self.inline_cache, saved_cache);
            return_cache_to_pool(&mut self.cache_pool, cache_len, unused_cache);
            std::mem::swap(&mut self.tail_call_journal, &mut saved_tail_journal);
            self.tail_call_journal_overflow = saved_tail_overflow;
            if pushed_dbg {
                self.call_stack.pop();
            }
            return Err(
                crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Recursion).msg(format!(
                    "exceeded maximum call depth {}",
                    self.max_call_depth
                )),
            );
        }
        let limit = self.instructions.len();
        let mut interpreter = self.interpreter_kind.create();
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "CALL enter chunk={:?} limit={} locals={} argc={} saved_pc={} interp_type={}",
                self.current_chunk,
                limit,
                local_count,
                argc,
                saved_pc,
                std::any::type_name_of_val(&*interpreter)
            );
        }
        let execute_res = interpreter.interpret(self, limit);
        self.returned = false;
        let res = match execute_res {
            Ok(value) => Ok(self.attach_provenance_to_returned_callable(value)),
            Err(e) => {
                self.capture_bt_if_empty();
                Err(e)
            }
        };
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "CALL after execute stack_len={} locals_depth={}",
                self.stack.len(),
                self.locals.len()
            );
        }
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "CALL leave chunk={:?} pc={} result_ok={}",
                self.current_chunk,
                self.pc,
                res.is_ok()
            );
        }
        self.current_closure_stack.pop();
        // Unwind
        if let Some(frame) = self.locals.pop() {
            return_locals_to_pool(&mut self.locals_pool, frame);
        }
        self.captures.pop();
        std::mem::swap(&mut self.stack, &mut saved_stack);
        let used_cache = std::mem::replace(&mut self.inline_cache, saved_cache);
        return_cache_to_pool(&mut self.cache_pool, cache_len, used_cache);
        self.instructions = saved_instructions;
        self.pc = saved_pc;
        std::mem::swap(&mut self.tail_call_journal, &mut saved_tail_journal);
        self.tail_call_journal_overflow = saved_tail_overflow;
        if pushed_dbg && let Some(fr) = self.call_stack.pop() {
            self.current_chunk = fr.chunk;
        }
        res
    }

    /// Call a user function with args already on the stack top (no intermediate
    /// allocation).
    pub(crate) fn invoke_user(
        &mut self,
        func: &Value,
        argc: u32,
        callee_name: Option<Cow<'_, str>>,
    ) -> WqResult<Value> {
        match func {
            Value::CompiledFunction(f) => self.invoke_spec(CallSpec {
                instructions: f.instructions.clone(),
                params_len: f.params.as_ref().map(|p| p.len() as u32),
                locals: f.locals,
                captured: crate::value::cell::empty_cells(),
                argc,
                callee_name,
                dbg_chunk: f.dbg_chunk,
                callee: func.clone(),
            }),
            Value::Closure(c) => self.invoke_spec(CallSpec {
                instructions: c.instructions.clone(),
                params_len: c.params.as_ref().map(|p| p.len() as u32),
                locals: c.locals,
                captured: c.captured.clone(),
                argc,
                callee_name,
                dbg_chunk: c.dbg_chunk,
                callee: func.clone(),
            }),
            other => Err(not_bound_err(format!(
                "expected callable, got {}",
                other.type_name()
            ))),
        }
    }

    /// Tail-call a user function with args already on the stack top.
    pub(crate) fn tail_invoke_user(&mut self, func: &Value, argc: u32) -> WqResult<()> {
        match func {
            Value::CompiledFunction(f) => self.prepare_tail(CallSpec {
                instructions: f.instructions.clone(),
                params_len: f.params.as_ref().map(|p| p.len() as u32),
                locals: f.locals,
                captured: crate::value::cell::empty_cells(),
                argc,
                callee_name: None,
                dbg_chunk: f.dbg_chunk,
                callee: func.clone(),
            }),
            Value::Closure(c) => self.prepare_tail(CallSpec {
                instructions: c.instructions.clone(),
                params_len: c.params.as_ref().map(|p| p.len() as u32),
                locals: c.locals,
                captured: c.captured.clone(),
                argc,
                callee_name: None,
                dbg_chunk: c.dbg_chunk,
                callee: func.clone(),
            }),
            other => Err(not_bound_err(format!(
                "expected fn, got {}",
                other.type_name()
            ))),
        }
    }

    pub(crate) fn prepare_tail(&mut self, spec: CallSpec<'_>) -> WqResult<()> {
        let CallSpec {
            instructions,
            params_len,
            locals: local_count,
            captured,
            argc,
            callee_name: _,
            dbg_chunk: _,
            callee,
        } = spec;

        let named_meta = self.pending_named_meta.take();

        // Arity check
        if let Some(ref meta) = named_meta
            && let Some(p) = params_len
            && meta.pos_count as u32 != p
        {
            return Err(arity_err_vm(format!(
                "function expects {} positional arg(s), got {}",
                p, meta.pos_count
            )));
        } else if named_meta.is_none()
            && let Some(p) = params_len
        {
            if argc != p {
                return Err(arity_err_vm(format!(
                    "function expects {} arg(s), got {}",
                    p, argc
                )));
            }
        } else if argc > 3 {
            return Err(arity_err_vm(
                "implicit function expects up to 3 args".to_string(),
            ));
        }

        ensure_stack_len(&self.stack, argc as usize, || "tail-call args".into())?;

        let mut frame = std::mem::take(
            self.locals
                .last_mut()
                .ok_or_else(|| vm_err("tail call without local frame"))?,
        );
        frame.clear();
        frame.resize(local_count as usize, Slot::default());

        if let Some(meta) = named_meta {
            let total = argc as usize;
            let mut all_args: Vec<Value> = (0..total)
                .map(|_| {
                    self.stack
                        .pop()
                        .ok_or_else(|| vm_err("stack underflow while moving named tail-call args"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            all_args.reverse();

            let named_arg_map: std::collections::HashMap<Arc<str>, Value> = meta
                .named
                .iter()
                .filter_map(|(pos, name)| {
                    all_args
                        .get(*pos as usize)
                        .map(|v| (name.clone(), v.clone()))
                })
                .collect();

            let mut pos_slot: u32 = 0;
            for (i, v) in all_args.iter().enumerate() {
                let is_named = meta.named.iter().any(|(pos, _)| *pos as usize == i);
                if !is_named {
                    if (pos_slot as usize) < local_count as usize {
                        frame[pos_slot as usize] = Slot::Value(v.clone());
                    }
                    pos_slot += 1;
                }
            }

            let callee_named: Option<Arc<[Arc<str>]>> = match &callee {
                Value::CompiledFunction(f) => f.named_params.clone(),
                Value::Closure(c) => c.named_params.clone(),
                _ => None,
            };

            let mut mask: i64 = 0;
            if let Some(named_params) = callee_named {
                let pos_param_count = params_len.unwrap_or(0);
                for (named_idx, param_name) in named_params.iter().enumerate() {
                    if let Some(val) = named_arg_map.get(param_name) {
                        let slot = (pos_param_count + named_idx as u32) as usize;
                        if slot < local_count as usize {
                            frame[slot] = Slot::Value(val.clone());
                        }
                        mask |= 1i64 << named_idx;
                    }
                }
                let mask_slot = (pos_param_count + named_params.len() as u32) as usize;
                if mask_slot < local_count as usize {
                    frame[mask_slot] = Slot::Value(Value::Int(mask));
                }
            }
        } else {
            for i in (0..argc).rev() {
                let v = self
                    .stack
                    .pop()
                    .ok_or_else(|| vm_err("stack underflow while moving tail-call args"))?;
                frame[i as usize] = Slot::Value(v);
            }
        }
        self.stack.clear();
        *self
            .locals
            .last_mut()
            .ok_or_else(|| vm_err("tail call without local frame"))? = frame;
        *self
            .captures
            .last_mut()
            .ok_or_else(|| vm_err("tail call without capture frame"))? = captured;
        *self
            .current_closure_stack
            .last_mut()
            .ok_or_else(|| vm_err("tail call without active callable"))? = callee;
        self.instructions = instructions;
        self.pc = 0;
        self.inline_cache.clear();
        self.inline_cache
            .resize(self.instructions.len(), InlineCache::default());
        Ok(())
    }

    #[inline]
    pub(crate) fn invoke_bfn_id(&mut self, id: u16, argc: u16) -> WqResult<Value> {
        let argc = usize::from(argc);
        let named_meta = self.pending_named_meta.take();
        ensure_stack_len(&self.stack, argc, || "builtin args".into())?;
        let base = self.stack.len() - argc;

        // Separate named args off the stack when metadata is present.
        if let Some(meta) = named_meta {
            let all_args: Vec<Value> = self.stack.drain(base..).collect();
            let mut pos_args: Sv4 = SmallVec::new();
            let mut named_args: Vec<(Arc<str>, Value)> = Vec::new();
            for (i, v) in all_args.into_iter().enumerate() {
                if let Some((_, name)) = meta.named.iter().find(|(p, _)| *p as usize == i) {
                    named_args.push((name.clone(), v));
                } else {
                    pos_args.push(v);
                }
            }
            let args = crate::builtins::BuiltinFnArgs::with_named(pos_args, named_args);
            let out = if self.builtins.is_enabled_id(id) {
                self.call_builtin_id(id, args)?
            } else {
                let name =
                    Builtins::name_from_id(id).ok_or_else(|| vm_err("invalid builtin id"))?;
                if let Some(val) = self.lookup_global(name) {
                    match &val {
                        Value::BuiltinFunction(bname) => {
                            return Err(arity_err_vm(format!(
                                "cannot pass named arguments to builtin override '{bname}'"
                            )));
                        }
                        _ => {
                            // User override: push positional args back, then call
                            let pos_len = args.len();
                            self.stack.extend(args);
                            return self.invoke_user(&val, pos_len as u32, None);
                        }
                    }
                } else {
                    return Err(not_bound_err(format!(
                        "'{name}' has not been bound to a value"
                    ))
                    .attach_note(format!(
                        "a builtin named '{name}' exists but is disabled in the current preset"
                    )));
                }
            };
            Ok(out)
        } else {
            // Fast path: drain all positional args from stack
            let pos_args: Sv4 = self.stack.drain(base..).collect();
            if self.builtins.is_enabled_id(id) {
                self.call_builtin_id(id, crate::builtins::BuiltinFnArgs::from(pos_args))
            } else {
                let name =
                    Builtins::name_from_id(id).ok_or_else(|| vm_err("invalid builtin id"))?;
                if let Some(val) = self.lookup_global(name) {
                    match &val {
                        Value::BuiltinFunction(bname) => self.call_builtin_name(
                            bname,
                            crate::builtins::BuiltinFnArgs::from(pos_args),
                        ),
                        _ => {
                            // User override: push args back on stack, invoke
                            self.stack.extend(pos_args);
                            self.invoke_user(&val, argc as u32, None)
                        }
                    }
                } else {
                    Err(not_bound_err(format!(
                        "'{name}' has not been bound to a value"
                    ))
                    .attach_note(format!(
                        "a builtin named '{name}' exists but is disabled in the current preset"
                    )))
                }
            }
        }
    }

    #[inline]
    pub(crate) fn invoke_bfn_name(&mut self, name: &str, argc: usize) -> WqResult<Value> {
        let named_meta = self.pending_named_meta.take();
        ensure_stack_len(&self.stack, argc, || "builtin args".into())?;
        let base = self.stack.len() - argc;

        if let Some(meta) = named_meta {
            let all_args: Vec<Value> = self.stack.drain(base..).collect();
            let mut pos_args: Sv4 = SmallVec::new();
            let mut named_args: Vec<(Arc<str>, Value)> = Vec::new();
            for (i, v) in all_args.into_iter().enumerate() {
                if let Some((_, param_name)) = meta.named.iter().find(|(p, _)| *p as usize == i) {
                    named_args.push((param_name.clone(), v));
                } else {
                    pos_args.push(v);
                }
            }
            self.call_builtin_name(
                name,
                crate::builtins::BuiltinFnArgs::with_named(pos_args, named_args),
            )
        } else {
            let pos_args: Sv4 = self.stack.drain(base..).collect();
            self.call_builtin_name(name, crate::builtins::BuiltinFnArgs::from(pos_args))
        }
    }

    #[inline]
    fn last_frame_mut(&mut self) -> WqResult<&mut Vec<Slot>> {
        self.locals
            .last_mut()
            .ok_or_else(|| vm_err("no local frame"))
    }

    #[inline]
    fn call_builtin_id(
        &mut self,
        id: u16,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        let argc = args.len();
        let func = *self
            .builtins
            .get_fn_by_id(usize::from(id))
            .ok_or_else(|| vm_err("invalid builtin id"))?;
        let result = (func)(self, args)?;
        if let Some(name) = Builtins::name_from_id(id)
            && let Some(hooks) = self.hooks
        {
            unsafe { hooks.as_ref() }.on_builtin_result(name, argc, &result);
        }
        Ok(result)
    }

    #[inline]
    pub(crate) fn local_slot_mut(&mut self, slot: u16) -> WqResult<&mut Slot> {
        let note = self
            .local_slot_name(slot as usize)
            .map(|name| format!("local slot {slot}: {name}"));
        self.last_frame_mut()?
            .get_mut(slot as usize)
            .ok_or_else(|| match &note {
                Some(note) => vm_err(format!("invalid local slot {slot}")).attach_note(note),
                None => vm_err(format!("invalid local slot {slot}")),
            })
    }

    #[inline]
    fn cache_callable(&mut self, idx: usize, rc: ResolvedCallable, version: u64) {
        let entry = &mut self.inline_cache[idx];
        entry.version = version;
        entry.call_target = Some(rc);
        entry.slot = None;
    }

    #[inline]
    pub(crate) fn resolve_user_callable(&mut self, idx: usize, name: &str) -> WqResult<Value> {
        // Fast path: cache
        let slot = self.lookup_global_slot(name);
        if let Some(slot) = slot {
            let name_version = self.global_slot_version(slot);
            if self.inline_cache[idx].version == name_version
                && let Some(ref target) = self.inline_cache[idx].call_target
            {
                return Ok(target.value.clone());
            }
        }

        // Slow path: resolve from globals
        let func_val = if let Some(slot) = slot {
            self.global_slot_value(slot)
                .ok_or_else(|| vm_err("invalid global slot"))?
                .clone()
        } else if self.builtins.is_disabled_name(name) {
            return Err(
                not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(
                    format!(
                        "a builtin named '{name}' exists but is disabled in the current preset"
                    ),
                ),
            );
        } else {
            self.lookup_global(name)
                .ok_or_else(|| not_bound_err(format!("fn '{name}' is not defined")))?
        };

        Ok(match func_val {
            Value::CompiledFunction(f) => {
                let (value, dbg_chunk) = if self.debug_artifacts_enabled() {
                    let dbg_chunk = self.ensure_dbg_chunk_with_spans(
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
                    let value = if f.dbg_chunk != dbg_chunk {
                        let mut new_f = FunctionData::clone(&f);
                        new_f.dbg_chunk = dbg_chunk;
                        Value::CompiledFunction(std::sync::Arc::new(new_f))
                    } else {
                        Value::CompiledFunction(std::sync::Arc::clone(&f))
                    };
                    (value, dbg_chunk)
                } else {
                    (
                        Value::CompiledFunction(std::sync::Arc::clone(&f)),
                        f.dbg_chunk,
                    )
                };
                if let Some(slot) = slot {
                    let name_version = self.global_slot_version(slot);
                    self.cache_callable(
                        idx,
                        ResolvedCallable {
                            value: value.clone(),
                            params_len: f.params.as_ref().map(|p| p.len() as u32),
                            locals: f.locals,
                            captured: crate::value::cell::empty_cells(),
                            code: f.instructions.clone(),
                            dbg_chunk,
                        },
                        name_version,
                    );
                }
                value
            }
            Value::Closure(c) => {
                let (value, dbg_chunk) = if self.debug_artifacts_enabled() {
                    let dbg_chunk = self.ensure_dbg_chunk_with_spans(
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
                    let value = if c.dbg_chunk != dbg_chunk {
                        let mut new_c = ClosureData::clone(&c);
                        new_c.dbg_chunk = dbg_chunk;
                        Value::Closure(std::sync::Arc::new(new_c))
                    } else {
                        Value::Closure(std::sync::Arc::clone(&c))
                    };
                    (value, dbg_chunk)
                } else {
                    (Value::Closure(std::sync::Arc::clone(&c)), c.dbg_chunk)
                };
                if let Some(slot) = slot {
                    let name_version = self.global_slot_version(slot);
                    self.cache_callable(
                        idx,
                        ResolvedCallable {
                            value: value.clone(),
                            params_len: c.params.as_ref().map(|p| p.len() as u32),
                            locals: c.locals,
                            captured: c.captured.clone(),
                            code: c.instructions.clone(),
                            dbg_chunk,
                        },
                        name_version,
                    );
                }
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

/// Specification for a function call.
///
/// This struct packages all the necessary information to set up a new stack
/// frame and execute a function. It is used by the VM and Interpreters to
/// handle function calls uniformly.
#[derive(Clone)]
pub(crate) struct CallSpec<'a> {
    pub(crate) callee: Value,
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) captured: Arc<[ValueCell]>,
    pub(crate) callee_name: Option<Cow<'a, str>>,
    pub(crate) argc: u32,
    pub(crate) params_len: Option<u32>,
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) locals: u16,
}

impl<'a> CallSpec<'a> {
    pub(crate) fn name_hint(s: Option<&'a str>) -> Option<Cow<'a, str>> {
        s.map(Cow::Borrowed)
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedCallable {
    pub(crate) value: Value,
    pub(crate) params_len: Option<u32>,
    pub(crate) locals: u16,
    pub(crate) captured: Arc<[ValueCell]>,
    pub(crate) code: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
}

pub(crate) enum LocalCallable {
    Func {
        value: Value,
        params_len: Option<u32>,
        locals: u16,
        instructions: Arc<[Instruction]>,
        captured: Arc<[ValueCell]>,
        dbg_chunk: Option<ChunkId>,
        name_hint: Option<String>,
    },
    Builtin(Arc<str>),
}

#[derive(Clone)]
pub(crate) struct PeekLocalUser {
    pub(crate) is_closure: bool,
    pub(crate) value: Value,
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) locals: u16,
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) spans: Option<DebugStmtSpans>,
    pub(crate) pc_spans: Option<DebugPcSpans>,
    pub(crate) stmt_marks: Option<Arc<[crate::vm::inst::DebugStmtMark]>>,
    pub(crate) names: Option<Arc<[String]>>,
    pub(crate) captured: Arc<[ValueCell]>,
}

pub(crate) enum PeekLocalCallable {
    Builtin(Arc<str>),
    User(PeekLocalUser),
}

#[inline]
pub(crate) fn peek_local_callable(slot: u16, v: &Slot) -> WqResult<PeekLocalCallable> {
    v.with_ref(|value| match value {
        Value::CompiledFunction(f) => Ok(PeekLocalCallable::User(PeekLocalUser {
            is_closure: false,
            value: value.clone(),
            params: f.params.clone(),
            locals: f.locals,
            instructions: f.instructions.clone(),
            dbg_chunk: f.dbg_chunk,
            spans: f.dbg_stmt_spans.clone(),
            pc_spans: f.dbg_pc_spans.clone(),
            stmt_marks: f.dbg_stmt_marks.clone(),
            names: f.dbg_local_names.clone(),
            captured: crate::value::cell::empty_cells(),
        })),
        Value::Closure(c) => Ok(PeekLocalCallable::User(PeekLocalUser {
            is_closure: true,
            value: value.clone(),
            params: c.params.clone(),
            locals: c.locals,
            instructions: c.instructions.clone(),
            dbg_chunk: c.dbg_chunk,
            spans: c.dbg_stmt_spans.clone(),
            pc_spans: c.dbg_pc_spans.clone(),
            stmt_marks: c.dbg_stmt_marks.clone(),
            names: c.dbg_local_names.clone(),
            captured: c.captured.clone(),
        })),
        Value::BuiltinFunction(name) => Ok(PeekLocalCallable::Builtin(name.clone())),
        other => Err(call_err(format!(
            "cannot call local {slot}: expected fn, found {} ({})",
            other.excerpt(),
            other.type_name(),
        ))),
    })
}

fn take_cache_from_pool(
    pool: &mut AHashMap<usize, Vec<Vec<InlineCache>>>,
    len: usize,
) -> Vec<InlineCache> {
    if let Some(bucket) = pool.get_mut(&len)
        && let Some(mut cache) = bucket.pop()
    {
        cache.resize(len, InlineCache::default());
        return cache;
    }
    vec![InlineCache::default(); len]
}

fn return_cache_to_pool(
    pool: &mut AHashMap<usize, Vec<Vec<InlineCache>>>,
    len: usize,
    mut cache: Vec<InlineCache>,
) {
    cache.clear();
    const MAX_POOL_PER_SIZE: usize = 4;
    let bucket = pool.entry(len).or_default();
    if bucket.len() < MAX_POOL_PER_SIZE {
        bucket.push(cache);
    }
}

fn take_locals_from_pool(pool: &mut AHashMap<u16, Vec<Vec<Slot>>>, local_count: u16) -> Vec<Slot> {
    if let Some(bucket) = pool.get_mut(&local_count)
        && let Some(mut frame) = bucket.pop()
    {
        frame.resize(local_count as usize, Slot::default());
        return frame;
    }
    vec![Slot::default(); local_count as usize]
}

fn return_locals_to_pool(pool: &mut AHashMap<u16, Vec<Vec<Slot>>>, mut frame: Vec<Slot>) {
    let local_count = frame.len() as u16;
    frame.clear();
    const MAX_POOL: usize = 4;
    let bucket = pool.entry(local_count).or_default();
    if bucket.len() < MAX_POOL {
        bucket.push(frame);
    }
}
