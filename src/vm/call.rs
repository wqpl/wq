use std::{borrow::Cow, sync::Arc};

use crate::{
    builtins::Builtins,
    interpreters::{Interpreter, default::DefaultInterpreter},
    value::{Excerpt, Value, ValueCell, WqResult},
    vm::{
        Frame, InlineCache, Vm, arity_err_vm, call_err, ensure_stack_len, instruction::Instruction,
        not_bound_err, slot::Slot, vm_err,
    },
    wqdb::{ChunkId, mark_stmt_heuristic},
};

/// Specification for a function call.
///
/// This struct packages all the necessary information to set up a new stack frame and execute a function.
/// It is used by the VM and Interpreters to handle function calls uniformly.
#[derive(Clone)]
pub struct CallSpec<'a> {
    pub instructions: Arc<[Instruction]>,
    pub params: Option<Arc<[String]>>,
    pub locals: u16,
    pub captured: Vec<ValueCell>,
    pub argc: usize,
    pub callee_name: Option<Cow<'a, str>>,
    pub dbg_chunk: Option<ChunkId>,
    pub callee: Value,
}

impl<'a> CallSpec<'a> {
    pub fn name_hint(s: Option<&'a str>) -> Option<Cow<'a, str>> {
        s.map(Cow::Borrowed)
    }
}

#[derive(Clone)]
pub(crate) enum CallTarget {
    Cfn(ResolvedCfn),
    Closure(ResolvedClosure),
}

#[derive(Clone)]
pub struct ResolvedCfn {
    pub(crate) value: Value,
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) locals: u16,
    pub(crate) code: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
}

#[derive(Clone)]
pub struct ResolvedClosure {
    pub(crate) value: Value,
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) locals: u16,
    pub(crate) captured: Vec<ValueCell>,
    pub(crate) code: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
}

pub(crate) enum LocalCallable {
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
pub(crate) struct PeekFunc {
    pub(crate) is_closure: bool,
    pub(crate) value: Value,
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) locals: u16,
    pub(crate) instructions: Arc<[Instruction]>,
    pub(crate) dbg_chunk: Option<ChunkId>,
    pub(crate) spans: Option<Arc<[(usize, usize)]>>,
    pub(crate) names: Option<Arc<[String]>>,
    pub(crate) captured: Vec<ValueCell>,
}

pub(crate) enum PeekLocal {
    Builtin(String),
    Func(PeekFunc),
}

#[inline]
pub(crate) fn peek_local_callable(slot: u16, v: &Slot) -> WqResult<PeekLocal> {
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

impl Vm {
    pub fn call_function_with<I: Interpreter + ?Sized>(
        &mut self,
        spec: CallSpec,
        interpreter: &mut I,
    ) -> WqResult<Value> {
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
        #[cfg(not(target_arch = "wasm32"))]
        if crate::debug_flags::get_debug_flags().contains(crate::debug_flags::DebugFlags::WQDB_1) {
            eprintln!(
                "CALL enter chunk={:?} limit={} locals={} argc={} saved_pc={} interp_type={}",
                self.current_chunk,
                limit,
                local_count,
                argc,
                saved_pc,
                std::any::type_name::<I>()
            );
        }
        let res = interpreter
            .execute(self, limit)
            .inspect_err(|_| self.capture_bt_if_empty());
        #[cfg(not(target_arch = "wasm32"))]
        if crate::debug_flags::get_debug_flags().contains(crate::debug_flags::DebugFlags::WQDB_1) {
            eprintln!(
                "CALL after execute stack_len={} locals_depth={}",
                self.stack.len(),
                self.locals.len()
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        if crate::debug_flags::get_debug_flags().contains(crate::debug_flags::DebugFlags::WQDB_1) {
            eprintln!(
                "CALL leave chunk={:?} pc={} result_ok={}",
                self.current_chunk,
                self.pc,
                res.is_ok()
            );
        }
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

    pub fn call_value_with_args(&mut self, func: &Value, args: Vec<Value>) -> WqResult<Value> {
        let mut interpreter = DefaultInterpreter;
        self.call_value_with_args_interpreter(func, args, &mut interpreter)
    }

    pub fn call_value_with_args_interpreter<I: Interpreter + ?Sized>(
        &mut self,
        func: &Value,
        mut args: Vec<Value>,
        interpreter: &mut I,
    ) -> WqResult<Value> {
        #[cfg(not(target_arch = "wasm32"))]
        if crate::debug_flags::get_debug_flags().contains(crate::debug_flags::DebugFlags::WQDB_1) {
            eprintln!(
                "call_value_with_args_interpreter argc={} func_kind={}",
                args.len(),
                func.type_name()
            );
        }
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
            } => self.call_function_with(
                CallSpec {
                    instructions: instructions.clone(),
                    params: params.clone(),
                    locals: *locals,
                    captured: Vec::new(),
                    argc,
                    callee_name: None,
                    dbg_chunk: *dbg_chunk,
                    callee: func.clone(),
                },
                interpreter,
            ),
            Value::Closure {
                params,
                locals,
                captured,
                instructions,
                dbg_chunk,
                ..
            } => self.call_function_with(
                CallSpec {
                    instructions: instructions.clone(),
                    params: params.clone(),
                    locals: *locals,
                    captured: captured.clone(),
                    argc,
                    callee_name: None,
                    dbg_chunk: *dbg_chunk,
                    callee: func.clone(),
                },
                interpreter,
            ),
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
    pub(crate) fn local_slot_mut(&mut self, slot: u16) -> WqResult<&mut Slot> {
        self.last_frame_mut()?
            .get_mut(slot as usize)
            .ok_or_else(|| vm_err(format!("invalid local slot {slot}")))
    }

    #[inline]
    fn cache_compiled(&mut self, idx: usize, rf: ResolvedCfn, version: u64) {
        let entry = &mut self.inline_cache[idx];
        entry.version = version;
        entry.call_target = Some(CallTarget::Cfn(rf));
        entry.slot = None;
    }

    #[inline]
    fn cache_closure(&mut self, idx: usize, rc: ResolvedClosure, version: u64) {
        let entry = &mut self.inline_cache[idx];
        entry.version = version;
        entry.call_target = Some(CallTarget::Closure(rc));
        entry.slot = None;
    }

    #[inline]
    pub(crate) fn builtin_from_stack_by_id_with_interpreter<I: Interpreter + ?Sized>(
        &mut self,
        id: u16,
        argc: u16,
        interpreter: &mut I,
    ) -> WqResult<Value> {
        let argc = usize::from(argc);
        ensure_stack_len(&self.stack, argc, || Cow::Borrowed("builtin args"))?;
        let base = self.stack.len() - argc;
        let ptr = unsafe { self.stack.as_ptr().add(base) };
        let args = unsafe { std::slice::from_raw_parts(ptr, argc) };
        let out = if self.builtins.is_enabled_id(id) {
            self.call_builtin_id(id, args)?
        } else {
            let name = Builtins::name_from_id(id).ok_or_else(|| vm_err("invalid builtin id"))?;
            if let Some(val) = self.lookup_global(name) {
                let args_vec = args.to_vec();
                self.stack.truncate(base);
                return self.call_value_with_args_interpreter(&val, args_vec, interpreter);
            } else {
                return Err(
                    not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(
                        format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        ),
                    ),
                );
            }
        };
        self.stack.truncate(base);
        Ok(out)
    }

    #[inline]
    pub(crate) fn builtin_from_stack_by_name(
        &mut self,
        name: &str,
        argc: usize,
    ) -> WqResult<Value> {
        ensure_stack_len(&self.stack, argc, || Cow::Borrowed("builtin args"))?;
        let base = self.stack.len() - argc;
        let ptr = unsafe { self.stack.as_ptr().add(base) };
        let args = unsafe { std::slice::from_raw_parts(ptr, argc) };
        let out = self.call_builtin(name, args)?;
        self.stack.truncate(base);
        Ok(out)
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
                return Ok(match target {
                    CallTarget::Cfn(ResolvedCfn { value, .. })
                    | CallTarget::Closure(ResolvedClosure { value, .. }) => value.clone(),
                });
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
                if let Some(slot) = slot {
                    let name_version = self.global_slot_version(slot);
                    self.cache_compiled(
                        idx,
                        ResolvedCfn {
                            value: value.clone(),
                            params,
                            locals,
                            code: instructions,
                            dbg_chunk,
                        },
                        name_version,
                    );
                }
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
                if let Some(slot) = slot {
                    let name_version = self.global_slot_version(slot);
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
