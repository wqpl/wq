use std::borrow::Cow;
use std::sync::Arc;

use ahash::AHashMap;
use smallvec::SmallVec;

use crate::builtins::{BuiltinContext, BuiltinEnum, BuiltinFnArgs, Builtins};
use crate::interpret::vanilla::Sv4;
use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::value::cell::ValueCell;
use crate::value::func::{CallableExpr, LiftedCallableData};
use crate::value::{Value, WqResult, eval_binary, eval_unary};
use crate::vm::inst::{Instruction, NamedArgMeta};
use crate::vm::slot::Slot;
use crate::vm::{Frame, InlineCache, Vm, arity_err_vm, ensure_stack_len, not_bound_err, vm_err};
use crate::wqdb::build::mark_stmt_heuristic;
use crate::wqdb::data::ChunkId;

const DEFAULT_OPERAND_STACK_CAPACITY: usize = 256;

struct TakenBuiltinArgs {
    args: BuiltinFnArgs,
    had_named_meta: bool,
}

fn nested_interpreter_type_name(kind: crate::interpret::InterpreterKind) -> &'static str {
    match kind {
        crate::interpret::InterpreterKind::Vanilla => {
            std::any::type_name::<crate::interpret::vanilla::VanillaInterpreter>()
        }
        crate::interpret::InterpreterKind::Sample => {
            std::any::type_name::<crate::interpret::sample::SampleInterpreter>()
        }
        crate::interpret::InterpreterKind::Profiler => {
            std::any::type_name::<crate::interpret::profiler::ProfilerInterpreter>()
        }
    }
}

fn interpret_nested_with_kind(
    kind: crate::interpret::InterpreterKind,
    vm: &mut Vm,
    limit: usize,
) -> WqResult<Value> {
    match kind {
        crate::interpret::InterpreterKind::Vanilla => {
            let mut interpreter = crate::interpret::vanilla::VanillaInterpreter;
            crate::interpret::Interpreter::interpret(&mut interpreter, vm, limit)
        }
        crate::interpret::InterpreterKind::Sample => {
            let mut interpreter = crate::interpret::sample::SampleInterpreter::default();
            crate::interpret::Interpreter::interpret(&mut interpreter, vm, limit)
        }
        crate::interpret::InterpreterKind::Profiler => {
            let mut interpreter = crate::interpret::profiler::ProfilerInterpreter::default();
            crate::interpret::Interpreter::interpret(&mut interpreter, vm, limit)
        }
    }
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
        match func {
            Value::LiftedCallable(data) => self.call_function_composition(data, args),
            Value::Cas(_) => self.call_cas_callable(func, args),
            Value::CompiledFunction(_) | Value::Closure(_) => {
                let (positional, named) = args.into_parts();
                let argc = positional
                    .len()
                    .checked_add(named.len())
                    .ok_or_else(|| arity_err_vm("call has too many arguments".to_string()))?;
                let pos_count = u16::try_from(positional.len()).map_err(|_| {
                    arity_err_vm("call has too many positional arguments".to_string())
                })?;
                let named_positions: Vec<(u16, Arc<str>)> = named
                    .iter()
                    .enumerate()
                    .map(|(index, (name, _))| {
                        let position = u16::try_from(positional.len() + index)
                            .map_err(|_| arity_err_vm("call has too many arguments".to_string()))?;
                        Ok((position, Arc::clone(name)))
                    })
                    .collect::<WqResult<_>>()?;
                let base = self.stack.len();
                self.stack.extend(positional);
                self.stack.extend(named.into_iter().map(|(_, value)| value));
                let saved_named_meta = self.pending_named_meta.take();
                if !named_positions.is_empty() {
                    self.pending_named_meta = Some(Arc::new(NamedArgMeta {
                        pos_count,
                        named: named_positions.into_boxed_slice(),
                    }));
                }
                let spec =
                    CallSpec::from_user_callable(func, argc, None).expect("matched user function");
                let res = self.invoke_spec(spec);
                self.pending_named_meta = saved_named_meta;
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

    fn resolve_named_arg_layout(
        &mut self,
        cache_idx: Option<usize>,
        argc: usize,
        params_len: Option<usize>,
        local_count: u16,
        named_meta: Option<&Arc<NamedArgMeta>>,
        callee_named: Option<&Arc<[Arc<str>]>>,
    ) -> WqResult<Option<Arc<NamedArgLayout>>> {
        let Some(meta) = named_meta else {
            validate_positional_call_shape(argc, params_len, local_count)?;
            return Ok(None);
        };
        let named_params = callee_named.ok_or_else(|| {
            arity_err_vm("cannot pass named arguments to a non-function value".to_string())
        })?;

        if let Some(idx) = cache_idx
            && let Some(cached) = self
                .inline_cache
                .get(idx)
                .and_then(|entry| entry.named_layout.as_ref())
            && cached.matches(argc, params_len, local_count, meta, named_params)
        {
            return Ok(Some(Arc::clone(cached)));
        }

        let layout = Arc::new(build_named_arg_layout(
            argc,
            params_len,
            local_count,
            Arc::clone(meta),
            Arc::clone(named_params),
        )?);
        if let Some(idx) = cache_idx
            && let Some(entry) = self.inline_cache.get_mut(idx)
        {
            entry.named_layout = Some(Arc::clone(&layout));
        }
        Ok(Some(layout))
    }

    pub(crate) fn invoke_cas_callable_on_stack(
        &mut self,
        expr: &Value,
        argc: usize,
    ) -> WqResult<Value> {
        let args = self.take_call_args_from_stack(argc)?;
        self.call_cas_callable(expr, args)
    }

    pub(crate) fn call_cas_callable(
        &mut self,
        expr: &Value,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        if !expr.is_cas_expr() {
            return Err(not_bound_err(format!(
                "expected callable, got {}",
                expr.type_name()
            )));
        }

        let result = if args.has_named() {
            crate::cas::substitute_cas_bindings(expr, args.named_items())?
        } else {
            expr.clone()
        };

        match args.len() {
            0 => Ok(result),
            1 => {
                let var = crate::cas::infer_single_cas_var(&result)
                    .map_err(|err| err.src("CAS callable"))?;
                crate::cas::substitute_cas(&result, &Value::from_cas_var(var), &args[0])
            }
            _ => Err(arity_err_vm(
                "CAS callable expects at most one positional argument",
            )),
        }
    }

    pub(crate) fn invoke_function_composition_on_stack(
        &mut self,
        data: &LiftedCallableData,
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
        data: &LiftedCallableData,
        args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        if args.has_named() {
            return Err(arity_err_vm(
                "cannot pass named arguments to a composed function",
            ));
        }
        let args: Sv4 = args.into_iter().collect();
        self.eval_callable_expr(&data.expr, &args)
    }

    fn eval_callable_expr(&mut self, expr: &CallableExpr, args: &[Value]) -> WqResult<Value> {
        match expr {
            CallableExpr::Const(value) => Ok(value.clone()),
            CallableExpr::Call(value) => self.call(value, BuiltinFnArgs::from_cloned_slice(args)),
            CallableExpr::Unary { op, operand } => {
                let value = self.eval_callable_expr(operand, args)?;
                eval_unary(op, &value)
            }
            CallableExpr::Binary { op, left, right } => {
                let left = self.eval_callable_expr(left, args)?;
                let right = self.eval_callable_expr(right, args)?;
                eval_binary(op, &left, &right)
            }
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
            cache_idx,
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
        let named_layout = self.resolve_named_arg_layout(
            cache_idx,
            argc,
            params_len,
            local_count,
            named_meta.as_ref(),
            callee_named.as_ref(),
        )?;

        // Stack and metadata checks must happen before state swap
        ensure_stack_len(&self.stack, argc, || "call args".into())?;

        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        let saved_pc = self.pc;
        let callee_stack =
            take_stack_from_pool(&mut self.stack_pool, DEFAULT_OPERAND_STACK_CAPACITY);
        let mut saved_stack = std::mem::replace(&mut self.stack, callee_stack);
        let cache_len = self.instructions.len();
        let new_cache = take_cache_from_pool(&mut self.cache_pool, cache_len);
        let saved_cache = std::mem::replace(&mut self.inline_cache, new_cache);
        let mut saved_tail_journal = std::mem::take(&mut self.tail_call_journal);
        let saved_tail_depth = std::mem::take(&mut self.tail_call_depth);
        // Push debug frame
        let mut pushed_dbg = false;
        if self.debug_artifacts_enabled() {
            let caller_chunk = self.current_chunk;
            self.call_stack.push(Frame {
                chunk: caller_chunk,
                pc: saved_pc,
                func_name: self.func_name_arc_for_chunk(caller_chunk),
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
            named_layout.as_deref(),
            "stack underflow while moving args",
        )?;
        self.locals.push(frame);
        self.captures.push(captured);
        self.current_closure_stack.push(callee);
        // Recursion limit check (tail calls are exempt because they reuse the frame)
        let max_call_depth = self.max_call_depth;
        if self.locals.len() > max_call_depth {
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
            self.tail_call_depth = saved_tail_depth;
            if pushed_dbg && let Some(fr) = self.call_stack.pop() {
                self.current_chunk = fr.chunk;
            }
            return Err(
                crate::wqerror::WqError::new(crate::wqerror::WqErrorType::Recursion)
                    .msg(format!("exceeded maximum call depth {}", max_call_depth)),
            );
        }
        let limit = self.instructions.len();
        let interpreter_kind = self.interpreter_kind;
        if get_debug_log_flags().contains(DebugLogFlags::WQDB) {
            eprintln!(
                "CALL enter chunk={:?} limit={} locals={} argc={} saved_pc={} interp_type={}",
                self.current_chunk,
                limit,
                local_count,
                argc,
                saved_pc,
                nested_interpreter_type_name(interpreter_kind)
            );
        }
        let execute_res = interpret_nested_with_kind(interpreter_kind, self, limit);
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
        self.tail_call_depth = saved_tail_depth;
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
            Value::Cas(_) => self.invoke_cas_callable_on_stack(func, argc),
            Value::LiftedCallable(data) => self.invoke_function_composition_on_stack(data, argc),
            Value::CompiledFunction(_) | Value::Closure(_) => self.invoke_spec(
                CallSpec::from_user_callable(func, argc, callee_name)
                    .expect("matched user function"),
            ),
            other => Err(not_bound_err(format!(
                "expected callable, got {}",
                other.type_name()
            ))),
        }
    }

    /// Tail-call a user function with args already on the stack top.
    pub(crate) fn tail_invoke_user(&mut self, func: &Value, argc: usize) -> WqResult<()> {
        match func {
            Value::CompiledFunction(_) | Value::Closure(_) => self.prepare_tail(
                CallSpec::from_user_callable(func, argc, None).expect("matched user function"),
            ),
            other => Err(not_bound_err(format!(
                "expected func, got {}",
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
            callee_name,
            dbg_chunk,
            mut callee,
            cache_idx,
        } = spec;

        let callee_chunk = if self.debug_artifacts_enabled() {
            let requested_dbg_chunk =
                dbg_chunk.or_else(|| callee.as_user_function().and_then(|shape| shape.dbg_chunk));
            let title = callee_name
                .as_deref()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    requested_dbg_chunk
                        .map(|id| self.func_name_for_chunk(id))
                        .filter(|name| name != "<?>")
                        .unwrap_or_else(|| "<fn>".to_string())
                });
            self.stamp_user_function_debug_chunk(&mut callee, &title, requested_dbg_chunk)
                .unwrap_or(self.current_chunk)
        } else {
            self.current_chunk
        };

        let named_meta = self.pending_named_meta.take();

        let callee_named = callee
            .as_user_function()
            .and_then(|shape| shape.named_params.clone());
        let named_layout = self.resolve_named_arg_layout(
            cache_idx,
            argc,
            params_len,
            local_count,
            named_meta.as_ref(),
            callee_named.as_ref(),
        )?;

        ensure_stack_len(&self.stack, argc, || "tail-call args".into())?;

        let mut frame = std::mem::take(
            self.locals
                .last_mut()
                .ok_or_else(|| vm_err("tail call without local frame"))?,
        );
        frame.clear();
        frame.resize(usize::from(local_count), Slot::default());

        fill_call_frame_from_stack(
            &mut self.stack,
            &mut frame,
            argc,
            named_layout.as_deref(),
            "stack underflow while moving tail-call args",
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
        if self.debug_artifacts_enabled() {
            self.current_chunk = callee_chunk;
        }
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
                Err(
                    not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(
                        format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        ),
                    ),
                )
            }
        }
    }

    #[inline]
    pub(crate) fn invoke_bfn_discard_id(&mut self, id: u16, argc: u16) -> WqResult<Value> {
        let argc = usize::from(argc);
        let taken = self.take_builtin_args_from_stack(argc)?;
        if self.builtins.is_enabled_id(id) {
            self.call_builtin_discard_id(id, taken.args)
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
                            self.call_builtin_discard_id(*builtin_id, taken.args)
                        }
                    }
                    _ => {
                        // User override: push positional args back, then call.
                        let pos_len = taken.args.len();
                        self.stack.extend(taken.args);
                        self.invoke_user(&val, pos_len, None).map(|_| Value::unit())
                    }
                }
            } else {
                Err(
                    not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(
                        format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        ),
                    ),
                )
            }
        }
    }

    #[inline]
    pub(crate) fn invoke_bfn_value(&mut self, id: u16, argc: usize) -> WqResult<Value> {
        let taken = self.take_builtin_args_from_stack(argc)?;
        self.call_builtin_id(id, taken.args)
    }

    pub(crate) fn take_call_args_from_stack(&mut self, argc: usize) -> WqResult<BuiltinFnArgs> {
        Ok(self.take_builtin_args_from_stack(argc)?.args)
    }

    fn take_builtin_args_from_stack(&mut self, argc: usize) -> WqResult<TakenBuiltinArgs> {
        let named_meta = self.pending_named_meta.take();
        ensure_stack_len(&self.stack, argc, || "builtin args".into())?;
        let base = self.stack.len() - argc;
        let had_named_meta = named_meta.is_some();
        let args = if let Some(meta) = named_meta {
            let all_args: Sv4 = self.stack.drain(base..).collect();
            let mut pos_args: Sv4 = SmallVec::with_capacity(usize::from(meta.pos_count));
            let mut named_args: Vec<(Arc<str>, Value)> = Vec::with_capacity(meta.named.len());
            let mut named_iter = meta.named.iter().peekable();
            for (i, v) in all_args.into_iter().enumerate() {
                if named_iter.peek().is_some_and(|(p, _)| usize::from(*p) == i) {
                    let (_, name) = named_iter.next().expect("peeked named argument");
                    named_args.push((Arc::clone(name), v));
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
        mut args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        let argc = args.len();
        let func = *self
            .builtins
            .get_fn_by_id(usize::from(id))
            .ok_or_else(|| vm_err("invalid builtin id"))?;
        if self.builtins.validate_runtime_call_args(id, &args)? {
            args.mark_runtime_validated();
        }
        let result = func.invoke(self, args)?;
        self.record_builtin_result(id, argc, &result);
        Ok(result)
    }

    #[inline]
    fn call_builtin_discard_id(
        &mut self,
        id: u16,
        mut args: crate::builtins::BuiltinFnArgs,
    ) -> WqResult<Value> {
        let argc = args.len();
        let func = *self
            .builtins
            .get_fn_by_id(usize::from(id))
            .ok_or_else(|| vm_err("invalid builtin id"))?;
        if self.builtins.validate_runtime_call_args(id, &args)? {
            args.mark_runtime_validated();
        }
        let result =
            if let Some(discard_fn) = BuiltinEnum::from_id(id).and_then(BuiltinEnum::discard_fn) {
                discard_fn(self, args)?
            } else {
                func.invoke(self, args).map(|_| Value::unit())?
            };
        self.record_builtin_result(id, argc, &result);
        Ok(result)
    }

    #[inline]
    fn record_builtin_result(&self, id: u16, argc: usize, result: &Value) {
        if let Some(hooks) = self.hooks
            && let Some(name) = Builtins::name_from_id(id)
        {
            unsafe { hooks.as_ref() }.on_builtin_result(name, argc, result);
        }
    }

    #[inline]
    pub(crate) fn local_slot_mut(&mut self, slot: u16) -> WqResult<&mut Slot> {
        let note = self
            .local_slot_name(usize::from(slot))
            .map(|name| format!("local slot {slot}: {name}"));
        self.last_frame_mut()?
            .get_mut(usize::from(slot))
            .ok_or_else(|| match &note {
                Some(note) => vm_err(format!("invalid local slot {slot}")).attach_note(note),
                None => vm_err(format!("invalid local slot {slot}")),
            })
    }
}

impl BuiltinContext for Vm {
    fn call(&mut self, func: &Value, args: BuiltinFnArgs) -> WqResult<Value> {
        Vm::call(self, func, args)
    }

    fn list_enabled_builtins(&self) -> Vec<String> {
        self.builtins.list_functions()
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
    pub(crate) cache_idx: Option<usize>,
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
            cache_idx: None,
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
            cache_idx: None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct NamedArgLayout {
    argc: usize,
    params_len: Option<usize>,
    local_count: u16,
    meta: Arc<NamedArgMeta>,
    named_params: Arc<[Arc<str>]>,
    destinations: Box<[u16]>,
    mask_slot: u16,
    mask: i64,
}

impl NamedArgLayout {
    fn matches(
        &self,
        argc: usize,
        params_len: Option<usize>,
        local_count: u16,
        meta: &Arc<NamedArgMeta>,
        named_params: &Arc<[Arc<str>]>,
    ) -> bool {
        self.argc == argc
            && self.params_len == params_len
            && self.local_count == local_count
            && Arc::ptr_eq(&self.meta, meta)
            && Arc::ptr_eq(&self.named_params, named_params)
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

fn validate_positional_call_shape(
    argc: usize,
    params_len: Option<usize>,
    local_count: u16,
) -> WqResult<()> {
    if let Some(expected) = params_len {
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

fn build_named_arg_layout(
    argc: usize,
    params_len: Option<usize>,
    local_count: u16,
    meta: Arc<NamedArgMeta>,
    named_params: Arc<[Arc<str>]>,
) -> WqResult<NamedArgLayout> {
    if named_params.len() > usize::try_from(i64::BITS).expect("i64 bit count fits in usize") {
        return Err(vm_err("function has more than 64 named parameters"));
    }
    let pos_count = usize::from(meta.pos_count);
    if let Some(expected) = params_len
        && pos_count != expected
    {
        return Err(arity_err_vm(format!(
            "function expects {expected} positional arg(s), got {pos_count}"
        )));
    }
    let described_argc = pos_count
        .checked_add(meta.named.len())
        .ok_or_else(|| arity_err_vm("call has too many arguments".to_string()))?;
    if described_argc != argc {
        return Err(vm_err(format!(
            "named call metadata describes {described_argc} args, but call has {argc}"
        )));
    }
    if usize::from(local_count) < argc {
        return Err(vm_err(format!(
            "function local count {local_count} is smaller than arg count {argc}"
        )));
    }

    let mut named_indices = AHashMap::with_capacity(named_params.len());
    for (index, name) in named_params.iter().enumerate() {
        if named_indices.insert(name.as_ref(), index).is_some() {
            return Err(vm_err(format!(
                "function has duplicate named parameter '{name}'"
            )));
        }
    }

    let mut destinations = vec![u16::MAX; argc];
    let mut mask = 0i64;
    for (argument_index, (position, name)) in meta.named.iter().enumerate() {
        if meta.named[..argument_index]
            .iter()
            .any(|(_, previous)| previous == name)
        {
            return Err(arity_err_vm(format!("duplicate named argument '{name}'")));
        }
        let position = usize::from(*position);
        let destination = destinations.get_mut(position).ok_or_else(|| {
            vm_err(format!(
                "named argument position {position} is out of range"
            ))
        })?;
        if *destination != u16::MAX {
            return Err(vm_err(format!(
                "named argument position {position} is repeated"
            )));
        }
        let named_index = named_indices
            .get(name.as_ref())
            .copied()
            .ok_or_else(|| arity_err_vm(format!("'{name}' is not a named parameter")))?;
        let slot = params_len
            .unwrap_or(0)
            .checked_add(named_index)
            .ok_or_else(|| vm_err("named argument slot overflow"))?;
        *destination = u16::try_from(slot)
            .map_err(|_| vm_err(format!("named argument slot {slot} is out of range")))?;
        let bit = 1i64
            .checked_shl(
                u32::try_from(named_index)
                    .map_err(|_| vm_err("named argument mask index overflow"))?,
            )
            .ok_or_else(|| vm_err("function has more than 64 named parameters"))?;
        mask |= bit;
    }

    let mut next_positional_slot = 0usize;
    for destination in &mut destinations {
        if *destination == u16::MAX {
            *destination = u16::try_from(next_positional_slot).map_err(|_| {
                vm_err(format!(
                    "positional argument slot {next_positional_slot} is out of range"
                ))
            })?;
            next_positional_slot += 1;
        }
    }
    if next_positional_slot != pos_count {
        return Err(vm_err(format!(
            "named call metadata describes {pos_count} positional args, found {next_positional_slot}"
        )));
    }

    let mask_slot = params_len
        .unwrap_or(0)
        .checked_add(named_params.len())
        .ok_or_else(|| vm_err("named argument mask slot overflow"))?;
    let mask_slot = u16::try_from(mask_slot).map_err(|_| {
        vm_err(format!(
            "named argument mask slot {mask_slot} is out of range"
        ))
    })?;
    for destination in destinations.iter().copied().chain([mask_slot]) {
        if destination >= local_count {
            return Err(vm_err(format!(
                "argument destination slot {destination} is outside {local_count} locals"
            )));
        }
    }

    Ok(NamedArgLayout {
        argc,
        params_len,
        local_count,
        meta,
        named_params,
        destinations: destinations.into_boxed_slice(),
        mask_slot,
        mask,
    })
}

fn fill_call_frame_from_stack(
    stack: &mut Vec<Value>,
    frame: &mut [Slot],
    argc: usize,
    named_layout: Option<&NamedArgLayout>,
    underflow: &'static str,
) -> WqResult<()> {
    ensure_stack_len(stack, argc, || underflow.to_string())?;
    if let Some(layout) = named_layout {
        debug_assert_eq!(argc, layout.destinations.len());
        let base = stack.len() - argc;
        for (value, destination) in stack.drain(base..).zip(layout.destinations.iter().copied()) {
            frame[usize::from(destination)] = Slot::Value(value);
        }
        frame[usize::from(layout.mask_slot)] = Slot::Value(Value::Int(layout.mask));
    } else {
        for i in (0..argc).rev() {
            let v = stack.pop().expect("stack length checked");
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
        frame.resize(usize::from(local_count), Slot::default());
        return frame;
    }
    vec![Slot::default(); usize::from(local_count)]
}

fn return_locals_to_pool(pool: &mut AHashMap<u16, Vec<Vec<Slot>>>, mut frame: Vec<Slot>) {
    let Ok(local_count) = u16::try_from(frame.len()) else {
        return;
    };
    frame.clear();
    const MAX_POOL: usize = 4;
    let bucket = pool.entry(local_count).or_default();
    if bucket.len() < MAX_POOL {
        bucket.push(frame);
    }
}

fn take_stack_from_pool(pool: &mut Vec<Vec<Value>>, min_cap: usize) -> Vec<Value> {
    if let Some(pos) = pool
        .iter()
        .enumerate()
        .filter(|(_, stack)| stack.capacity() >= min_cap)
        .min_by_key(|(_, stack)| stack.capacity())
        .map(|(index, _)| index)
    {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use smallvec::smallvec;

    use super::*;
    use crate::ast::{BinaryOperator, UnaryOperator};
    use crate::value::func::FunctionData;
    use crate::vm::inst::{Instruction, Operand};

    fn make_fn(params: &[&str], instructions: Vec<Instruction>) -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: Some(Arc::<[String]>::from(
                params
                    .iter()
                    .map(|name| name.to_string())
                    .collect::<Vec<_>>(),
            )),
            named_params: None,
            locals: u16::try_from(params.len()).expect("test function params fit in u16"),
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

    fn add_const(n: i64) -> Value {
        make_fn(
            &["x"],
            vec![
                Instruction::binary_op(
                    BinaryOperator::Add,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(n))),
                ),
                Instruction::Return,
            ],
        )
    }

    fn multiply_const(n: i64) -> Value {
        make_fn(
            &["x"],
            vec![
                Instruction::binary_op(
                    BinaryOperator::Multiply,
                    Operand::Local(0),
                    Operand::Const(Box::new(Value::Int(n))),
                ),
                Instruction::Return,
            ],
        )
    }

    fn identity() -> Value {
        make_fn(&["x"], vec![Instruction::LoadLocal(0), Instruction::Return])
    }

    #[test]
    fn callable_expr_evaluates_nested_plans_without_named_args() {
        let mut vm = Vm::new(vec![]);
        let f = add_const(1);
        let g = multiply_const(2);
        let nested = Value::function_composition(
            BinaryOperator::Multiply,
            Value::function_composition(BinaryOperator::Add, f, g),
            Value::Int(2),
        );

        let out = vm
            .call(&nested, BuiltinFnArgs::from(Value::Int(3)))
            .expect("callable expression should evaluate");
        assert_eq!(out, Value::Int(20));

        let named = BuiltinFnArgs::with_named(
            smallvec![Value::Int(3)],
            vec![(Arc::<str>::from("name"), Value::Int(4))],
        );
        let err = vm
            .call(&nested, named)
            .expect_err("named args should be rejected");
        assert!(
            err.to_string()
                .contains("cannot pass named arguments to a composed function")
        );
    }

    #[test]
    fn callable_expr_reuses_constants_and_reports_leaf_arity_errors() {
        let mut vm = Vm::new(vec![]);
        let composed =
            Value::function_composition(BinaryOperator::Subtract, Value::Int(10), add_const(1));

        let out = vm
            .call(&composed, BuiltinFnArgs::from(Value::Int(3)))
            .expect("callable expression should evaluate");
        assert_eq!(out, Value::Int(6));

        let err = vm
            .call(
                &composed,
                BuiltinFnArgs::from(vec![Value::Int(1), Value::Int(2)]),
            )
            .expect_err("leaf callable arity should still be checked");
        assert!(err.to_string().contains("arity"));
    }

    #[test]
    fn unary_callable_expr_evaluates_pointwise() {
        let mut vm = Vm::new(vec![]);
        let neg = Value::unary_function_composition(UnaryOperator::Negate, add_const(1));
        let out = vm
            .call(&neg, BuiltinFnArgs::from(Value::Int(3)))
            .expect("negated callable should evaluate");
        assert_eq!(out, Value::Int(-4));

        let bit_not = Value::unary_function_composition(UnaryOperator::Not, identity());
        let out = vm
            .call(&bit_not, BuiltinFnArgs::from(Value::Int(5)))
            .expect("bit-not callable should evaluate");
        assert_eq!(out, Value::Int(-6));
    }

    #[test]
    fn call_preserves_named_arguments_for_user_functions() {
        let named_identity = Value::CompiledFunction(Arc::new(FunctionData {
            params: Some(Arc::from([])),
            named_params: Some(Arc::from([Arc::<str>::from("x")])),
            locals: 2,
            instructions: Arc::from([Instruction::LoadLocal(0), Instruction::Return]),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }));
        let args =
            BuiltinFnArgs::with_named(smallvec![], vec![(Arc::<str>::from("x"), Value::Int(7))]);
        let mut vm = Vm::new(vec![]);

        let result = vm
            .call(&named_identity, args)
            .expect("named user call should succeed");

        assert_eq!(result, Value::Int(7));
    }

    #[test]
    fn user_call_shape_rejects_duplicate_named_arguments() {
        let meta = Arc::new(NamedArgMeta {
            pos_count: 0,
            named: Box::new([(0, Arc::<str>::from("x")), (1, Arc::<str>::from("x"))]),
        });
        let named_params: Arc<[Arc<str>]> = Arc::from([Arc::<str>::from("x")]);

        let err = build_named_arg_layout(2, Some(0), 2, meta, named_params)
            .expect_err("duplicate named args should fail user call validation");

        assert_eq!(err.err_type, crate::wqerror::WqErrorType::Arity);
        assert_eq!(err.msg.as_deref(), Some("duplicate named argument 'x'"));
    }

    #[test]
    fn named_arg_layout_is_cached_and_moves_values_without_cloning() {
        let meta = Arc::new(NamedArgMeta {
            pos_count: 1,
            named: Box::new([(1, Arc::<str>::from("b"))]),
        });
        let named_params: Arc<[Arc<str>]> =
            Arc::from([Arc::<str>::from("a"), Arc::<str>::from("b")]);
        let mut vm = Vm::new(vec![Instruction::Return]);

        let first = vm
            .resolve_named_arg_layout(Some(0), 2, Some(1), 4, Some(&meta), Some(&named_params))
            .expect("layout should resolve")
            .expect("named call should have a layout");
        let second = vm
            .resolve_named_arg_layout(Some(0), 2, Some(1), 4, Some(&meta), Some(&named_params))
            .expect("cached layout should resolve")
            .expect("named call should have a layout");

        assert!(Arc::ptr_eq(&first, &second));
        let mut stack = vec![Value::Int(10), Value::Int(99)];
        let mut frame = vec![Slot::default(); 4];
        fill_call_frame_from_stack(&mut stack, &mut frame, 2, Some(&first), "test underflow")
            .expect("frame fill should succeed");

        assert!(stack.is_empty());
        assert_eq!(frame[0].read(), Value::Int(10));
        assert_eq!(frame[2].read(), Value::Int(99));
        assert_eq!(frame[3].read(), Value::Int(2));
    }

    #[test]
    fn nested_call_stack_capacity_does_not_follow_caller_history() {
        let callee = make_fn(
            &[],
            vec![Instruction::load_const(Value::Int(1)), Instruction::Return],
        );
        let mut vm = Vm::new(vec![]);
        vm.stack = Vec::with_capacity(8 * 1024);

        let result = vm
            .call(&callee, BuiltinFnArgs::from(vec![]))
            .expect("call should succeed");

        assert_eq!(result, Value::Int(1));
        assert!(vm.stack.capacity() >= 8 * 1024);
        assert_eq!(vm.stack_pool.len(), 1);
        assert_eq!(vm.stack_pool[0].capacity(), DEFAULT_OPERAND_STACK_CAPACITY);
    }

    #[test]
    fn stack_pool_selects_smallest_suitable_buffer() {
        let mut pool = vec![Vec::with_capacity(8 * 1024), Vec::with_capacity(512)];

        let stack: Vec<Value> = take_stack_from_pool(&mut pool, 256);

        assert_eq!(stack.capacity(), 512);
    }
}
