use std::borrow::Cow;
use std::sync::Arc;

use ahash::AHashMap;
use smallvec::SmallVec;

use crate::builtins::{BuiltinFnArgs, Builtins};
use crate::interpret::vanilla::Sv4;
use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::value::cell::ValueCell;
use crate::value::func::FunctionCompositionData;
use crate::value::{Excerpt, Value, WqResult, eval_binary};
use crate::vm::inst::{Instruction, NamedArgMeta};
use crate::vm::slot::Slot;
use crate::vm::{
    Frame, InlineCache, Vm, arity_err_vm, call_err, ensure_stack_len, not_bound_err, vm_err,
};
use crate::wqdb::build::mark_stmt_heuristic;
use crate::wqdb::data::ChunkId;

struct TakenBuiltinArgs {
    args: BuiltinFnArgs,
    had_named_meta: bool,
}

impl Vm {
    // API for Builtins ============================

    pub(crate) fn call(
        &mut self,
        func: &Value,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        if let Value::BuiltinFunction { id, .. } = func {
            return self.call_builtin_id(*id, args);
        }
        let argc = args.len();
        match func {
            Value::FunctionComposition(data) => self.call_function_composition(data, args),
            Value::CompiledFunction(_) | Value::Closure(_) => {
                let base = self.stack.len();
                self.stack.extend(args);
                let spec =
                    CallSpec::from_user_callable(func, argc, None).expect("matched user function");
                let res = self.invoke_spec(spec);
                if res.is_err() {
                    self.stack.truncate(base);
                }
                res
            }
            other => Err(not_bound_err(format!(
                "expected callable, got {}",
                other.type_name()
            ))),
        }
    }

    pub(crate) fn invoke_function_composition_on_stack(
        &mut self,
        data: &FunctionCompositionData,
        argc: usize,
    ) -> WqResult<Value> {
        if self.pending_named_meta.take().is_some() {
            return Err(arity_err_vm(
                "cannot pass named arguments to a composed function",
            ));
        }
        ensure_stack_len(&self.stack, argc, || "composed function args".into())?;
        let base = self.stack.len() - argc;
        let args: Sv4 = self.stack.drain(base..).collect();
        self.call_function_composition(data, crate::builtins::BuiltinFnArgs::from(args))
    }

    pub(crate) fn call_function_composition(
        &mut self,
        data: &FunctionCompositionData,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        if args.has_named() {
            return Err(arity_err_vm(
                "cannot pass named arguments to a composed function",
            ));
        }
        let args: Vec<Value> = args.into_iter().collect();
        let left = self.eval_function_composition_operand(&data.left, &args)?;
        let right = self.eval_function_composition_operand(&data.right, &args)?;
        eval_binary(&data.op, &left, &right)
    }

    fn eval_function_composition_operand(
        &mut self,
        value: &Value,
        args: &[Value],
    ) -> WqResult<Value> {
        if value.is_callable() {
            self.call(value, crate::builtins::BuiltinFnArgs::from(args.to_vec()))
        } else {
            Ok(value.clone())
        }
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
            if callee.as_user_function().is_some() {
                let requested_dbg_chunk = dbg_chunk.or_else(|| {
                    callee
                        .as_user_function()
                        .expect("checked user function")
                        .dbg_chunk
                });
                let title = preferred_name(requested_dbg_chunk);
                let chunk =
                    self.stamp_user_function_debug_chunk(&mut callee, &title, requested_dbg_chunk);
                chunk.unwrap_or(self.current_chunk)
            } else {
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
        } else {
            self.current_chunk
        };

        // --- Pre-validate arity and named args before swapping execution state ---
        let named_meta = self.pending_named_meta.take();
        let callee_named = callee
            .as_user_function()
            .and_then(|shape| shape.named_params.clone());
        validate_user_call_shape(
            argc,
            params_len,
            local_count,
            named_meta.as_deref(),
            callee_named.as_deref(),
        )?;

        // Stack and metadata sanity checks — must happen before state swap
        ensure_stack_len(&self.stack, argc, || "call args".into())?;

        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        let saved_pc = self.pc;
        let prev_cap = self.stack.capacity();
        let callee_stack = take_stack_from_pool(&mut self.stack_pool, std::cmp::max(prev_cap, 256));
        let mut saved_stack = std::mem::replace(&mut self.stack, callee_stack);
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
        fill_call_frame_from_stack(
            &mut saved_stack,
            &mut frame,
            argc,
            params_len,
            named_meta.as_deref(),
            callee_named.as_deref(),
            "stack underflow while moving args",
            "stack underflow while moving named args",
        )?;
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
            let callee_stack = std::mem::replace(&mut self.stack, saved_stack);
            return_stack_to_pool(&mut self.stack_pool, callee_stack);
            self.instructions = saved_instructions;
            self.pc = saved_pc;
            let unused_cache = std::mem::replace(&mut self.inline_cache, saved_cache);
            return_cache_to_pool(&mut self.cache_pool, cache_len, unused_cache);
            std::mem::swap(&mut self.tail_call_journal, &mut saved_tail_journal);
            self.tail_call_journal_overflow = saved_tail_overflow;
            if pushed_dbg && let Some(fr) = self.call_stack.pop() {
                self.current_chunk = fr.chunk;
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
        let callee_stack = std::mem::replace(&mut self.stack, saved_stack);
        return_stack_to_pool(&mut self.stack_pool, callee_stack);
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
        argc: usize,
        callee_name: Option<Cow<'_, str>>,
    ) -> WqResult<Value> {
        match func {
            Value::FunctionComposition(data) => {
                self.invoke_function_composition_on_stack(data, argc)
            }
            Value::CompiledFunction(_) | Value::Closure(_) => {
                self.invoke_spec(
                    CallSpec::from_user_callable(func, argc, callee_name)
                        .expect("matched user function"),
                )
            }
            other => Err(not_bound_err(format!(
                "expected callable, got {}",
                other.type_name()
            ))),
        }
    }

    /// Tail-call a user function with args already on the stack top.
    pub(crate) fn tail_invoke_user(&mut self, func: &Value, argc: usize) -> WqResult<()> {
        match func {
            Value::CompiledFunction(_) | Value::Closure(_) => {
                self.prepare_tail(
                    CallSpec::from_user_callable(func, argc, None).expect("matched user function"),
                )
            }
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

        let callee_named = callee
            .as_user_function()
            .and_then(|shape| shape.named_params.clone());
        validate_user_call_shape(
            argc,
            params_len,
            local_count,
            named_meta.as_deref(),
            callee_named.as_deref(),
        )?;

        ensure_stack_len(&self.stack, argc, || "tail-call args".into())?;

        let mut frame = std::mem::take(
            self.locals
                .last_mut()
                .ok_or_else(|| vm_err("tail call without local frame"))?,
        );
        frame.clear();
        frame.resize(local_count as usize, Slot::default());

        fill_call_frame_from_stack(
            &mut self.stack,
            &mut frame,
            argc,
            params_len,
            named_meta.as_deref(),
            callee_named.as_deref(),
            "stack underflow while moving tail-call args",
            "stack underflow while moving named tail-call args",
        )?;
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
        let same_code = Arc::ptr_eq(&self.instructions, &instructions);
        self.instructions = instructions;
        self.pc = 0;
        if !same_code {
            self.inline_cache.clear();
        }
        self.inline_cache
            .resize(self.instructions.len(), InlineCache::default());
        Ok(())
    }

    #[inline]
    pub(crate) fn invoke_bfn_id(&mut self, id: u16, argc: u16) -> WqResult<Value> {
        let argc = usize::from(argc);
        let taken = self.take_builtin_args_from_stack(argc)?;
        if self.builtins.is_enabled_id(id) {
            self.call_builtin_id(id, taken.args)
        } else {
            let name = Builtins::name_from_id(id).ok_or_else(|| vm_err("invalid builtin id"))?;
            if let Some(val) = self.lookup_global(name) {
                match &val {
                    Value::BuiltinFunction {
                        name: bname,
                        id: builtin_id,
                    } => {
                        if taken.had_named_meta {
                            Err(arity_err_vm(format!(
                                "cannot pass named arguments to builtin override '{bname}'"
                            )))
                        } else {
                            self.call_builtin_id(*builtin_id, taken.args)
                        }
                    }
                    _ => {
                        // User override: push positional args back, then call.
                        let pos_len = taken.args.len();
                        self.stack.extend(taken.args);
                        self.invoke_user(&val, pos_len, None)
                    }
                }
            } else {
                Err(not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(
                    format!("a builtin named '{name}' exists but is disabled in the current preset"),
                ))
            }
        }
    }

    #[inline]
    pub(crate) fn invoke_bfn_value(&mut self, id: u16, argc: usize) -> WqResult<Value> {
        let taken = self.take_builtin_args_from_stack(argc)?;
        self.call_builtin_id(id, taken.args)
    }

    fn take_builtin_args_from_stack(&mut self, argc: usize) -> WqResult<TakenBuiltinArgs> {
        let named_meta = self.pending_named_meta.take();
        ensure_stack_len(&self.stack, argc, || "builtin args".into())?;
        let base = self.stack.len() - argc;
        let had_named_meta = named_meta.is_some();
        let args = if let Some(meta) = named_meta {
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
            BuiltinFnArgs::with_named(pos_args, named_args)
        } else {
            let pos_args: Sv4 = self.stack.drain(base..).collect();
            BuiltinFnArgs::from(pos_args)
        };
        Ok(TakenBuiltinArgs {
            args,
            had_named_meta,
        })
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

        if func_val.as_user_function().is_some() {
            let mut value = func_val;
            let dbg_chunk = if self.debug_artifacts_enabled() {
                self.stamp_user_function_debug_chunk(&mut value, name, None)
            } else {
                value
                    .as_user_function()
                    .expect("checked user function")
                    .dbg_chunk
            };
            if let Some(slot) = slot {
                let name_version = self.global_slot_version(slot);
                self.cache_callable(
                    idx,
                    ResolvedCallable::from_user_callable(value.clone(), dbg_chunk)
                        .expect("checked user function"),
                    name_version,
                );
            }
            Ok(value)
        } else {
            match func_val {
                b @ Value::BuiltinFunction { .. } => Ok(b),
                other => Err(not_bound_err(format!(
                    "cannot call '{name}': expected fn, got {}",
                    other.type_name()
                ))),
            }
        }
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
    pub(crate) argc: usize,
    pub(crate) params_len: Option<usize>,
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) locals: u16,
}

impl<'a> CallSpec<'a> {
    pub(crate) fn name_hint(s: Option<&'a str>) -> Option<Cow<'a, str>> {
        s.map(Cow::Borrowed)
    }

    pub(crate) fn from_user_callable(
        callee: &Value,
        argc: usize,
        callee_name: Option<Cow<'a, str>>,
    ) -> Option<Self> {
        let shape = callee.as_user_function()?;
        Some(Self {
            callee: callee.clone(),
            instructions: Arc::clone(shape.instructions),
            captured: shape.captured(),
            callee_name,
            argc,
            params_len: shape.params_len(),
            dbg_chunk: shape.dbg_chunk,
            locals: shape.locals,
        })
    }

    pub(crate) fn from_resolved(
        target: &ResolvedCallable,
        argc: usize,
        callee_name: Option<Cow<'a, str>>,
    ) -> Self {
        Self {
            callee: target.value.clone(),
            instructions: target.code.clone(),
            captured: target.captured.clone(),
            callee_name,
            argc,
            params_len: target.params_len,
            dbg_chunk: target.dbg_chunk,
            locals: target.locals,
        }
    }
}

#[derive(Clone)]
pub(crate) struct ResolvedCallable {
    pub(crate) value: Value,
    pub(crate) params_len: Option<usize>,
    pub(crate) locals: u16,
    pub(crate) captured: Arc<[ValueCell]>,
    pub(crate) code: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
}

impl ResolvedCallable {
    pub(crate) fn from_user_callable(value: Value, dbg_chunk: Option<ChunkId>) -> Option<Self> {
        let (params_len, locals, captured, code) = {
            let shape = value.as_user_function()?;
            (
                shape.params_len(),
                shape.locals,
                shape.captured(),
                Arc::clone(shape.instructions),
            )
        };
        Some(Self {
            value,
            params_len,
            locals,
            captured,
            code,
            dbg_chunk,
        })
    }
}

pub(crate) enum LocalCallable {
    Func {
        value: Value,
        params_len: Option<usize>,
        locals: u16,
        instructions: Arc<[Instruction]>,
        captured: Arc<[ValueCell]>,
        dbg_chunk: Option<ChunkId>,
        name_hint: Option<String>,
    },
    Builtin(u16),
}

#[derive(Clone)]
pub(crate) struct PeekLocalUser {
    pub(crate) value: Value,
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) locals: u16,
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) captured: Arc<[ValueCell]>,
}

pub(crate) enum PeekLocalCallable {
    Builtin(u16),
    User(PeekLocalUser),
}

#[inline]
pub(crate) fn peek_local_callable(slot: u16, v: &Slot) -> WqResult<PeekLocalCallable> {
    v.with_ref(|value| {
        if let Some(shape) = value.as_user_function() {
            return Ok(PeekLocalCallable::User(PeekLocalUser {
                value: value.clone(),
                params: shape.params.clone(),
                locals: shape.locals,
                instructions: Arc::clone(shape.instructions),
                dbg_chunk: shape.dbg_chunk,
                captured: shape.captured(),
            }));
        }
        match value {
            Value::BuiltinFunction { id, .. } => Ok(PeekLocalCallable::Builtin(*id)),
            other => Err(call_err(format!(
                "cannot call local {slot}: expected fn, found {} ({})",
                other.excerpt(),
                other.type_name(),
            ))),
        }
    })
}

fn validate_user_call_shape(
    argc: usize,
    params_len: Option<usize>,
    local_count: u16,
    named_meta: Option<&NamedArgMeta>,
    callee_named: Option<&[Arc<str>]>,
) -> WqResult<()> {
    if let Some(meta) = named_meta {
        if let Some(expected) = params_len
            && usize::from(meta.pos_count) != expected
        {
            return Err(arity_err_vm(format!(
                "function expects {} positional arg(s), got {}",
                expected, meta.pos_count
            )));
        }
        if !meta.named.is_empty() {
            if let Some(named_params) = callee_named {
                for (_, arg_name) in &meta.named {
                    if !named_params.iter().any(|p| p.as_ref() == arg_name.as_ref()) {
                        return Err(arity_err_vm(format!(
                            "'{}' is not a named parameter",
                            arg_name
                        )));
                    }
                }
            } else {
                return Err(arity_err_vm(
                    "cannot pass named arguments to a non-function value",
                ));
            }
        }
    } else if let Some(expected) = params_len {
        if argc != expected {
            return Err(arity_err_vm(format!(
                "function expects {} arg(s), got {}",
                expected, argc
            )));
        }
    } else if argc > 3 {
        return Err(arity_err_vm(
            "implicit function expects up to 3 args".to_string(),
        ));
    }

    if usize::from(local_count) < argc {
        return Err(vm_err(format!(
            "function local count {local_count} is smaller than arg count {argc}"
        )));
    }
    Ok(())
}

fn fill_call_frame_from_stack(
    stack: &mut Vec<Value>,
    frame: &mut [Slot],
    argc: usize,
    params_len: Option<usize>,
    named_meta: Option<&NamedArgMeta>,
    callee_named: Option<&[Arc<str>]>,
    positional_underflow: &'static str,
    named_underflow: &'static str,
) -> WqResult<()> {
    if let Some(meta) = named_meta {
        let mut all_args: Vec<Value> = (0..argc)
            .map(|_| stack.pop().ok_or_else(|| vm_err(named_underflow)))
            .collect::<Result<Vec<_>, _>>()?;
        all_args.reverse();

        let mut pos_slot: u32 = 0;
        for (i, v) in all_args.iter().enumerate() {
            let is_named = meta.named.iter().any(|(pos, _)| *pos as usize == i);
            if !is_named {
                if let Some(slot) = frame.get_mut(pos_slot as usize) {
                    *slot = Slot::Value(v.clone());
                }
                pos_slot += 1;
            }
        }

        let mut mask: i64 = 0;
        if let Some(named_params) = callee_named {
            let pos_param_count = params_len.unwrap_or(0);
            for (named_idx, param_name) in named_params.iter().enumerate() {
                let mut named_value = None;
                for (pos, arg_name) in &meta.named {
                    if arg_name.as_ref() == param_name.as_ref() {
                        named_value = all_args.get(*pos as usize);
                    }
                }
                if let Some(val) = named_value {
                    let slot = pos_param_count + named_idx;
                    if let Some(frame_slot) = frame.get_mut(slot) {
                        *frame_slot = Slot::Value(val.clone());
                    }
                    mask |= 1i64 << named_idx;
                }
            }
            let mask_slot = pos_param_count + named_params.len();
            if let Some(frame_slot) = frame.get_mut(mask_slot) {
                *frame_slot = Slot::Value(Value::Int(mask));
            }
        }
    } else {
        for i in (0..argc).rev() {
            let v = stack
                .pop()
                .ok_or_else(|| vm_err(positional_underflow))?;
            frame[i] = Slot::Value(v);
        }
    }
    Ok(())
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

fn take_stack_from_pool(pool: &mut Vec<Vec<Value>>, min_cap: usize) -> Vec<Value> {
    if let Some(pos) = pool.iter().position(|v| v.capacity() >= min_cap) {
        let mut v = pool.swap_remove(pos);
        v.clear();
        return v;
    }
    Vec::with_capacity(min_cap)
}

fn return_stack_to_pool(pool: &mut Vec<Vec<Value>>, mut stack: Vec<Value>) {
    stack.clear();
    const MAX_POOL: usize = 4;
    const MAX_STACK_POOL_CAP: usize = 16 * 1024;
    if pool.len() < MAX_POOL && stack.capacity() <= MAX_STACK_POOL_CAP {
        pool.push(stack);
    }
}
