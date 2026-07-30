use std::sync::Arc;

use smallvec::SmallVec;

use super::{Sv4, index_load_err, named_arg_index_err};
use crate::interpret::InterpreterHook;
use crate::session::dbglog::DebugLogFlags;
use crate::value::{Value, WqResult};
use crate::vm::call::{CallSpec, ResolvedCallable};
use crate::vm::pure::PureCallback;
use crate::vm::{TailFrame, Vm, call_err};

// --- concrete dispatch functions passed by the interpret loop ---

fn invoke_user_push(vm: &mut Vm, _idx: usize, target: &Value, argc: usize) -> WqResult<bool> {
    if let Some(spec) = CallSpec::from_user_callable(target, argc, None) {
        vm.enter_spec(spec)?;
        Ok(true)
    } else {
        let result = vm.invoke_user(target, argc, None)?;
        vm.stack.push(result);
        Ok(false)
    }
}

fn tail_invoke_user(vm: &mut Vm, idx: usize, target: &Value, argc: usize) -> WqResult<bool> {
    let tail_frame = if vm.debug_artifacts_enabled() {
        let chunk = vm.expect_current_chunk();
        Some(TailFrame {
            chunk,
            pc: idx + 1,
            func_name: vm.func_name_arc_for_chunk(chunk),
            instructions: Arc::clone(&vm.instructions),
        })
    } else {
        None
    };
    vm.tail_invoke_user(target, argc)?;
    if let Some(frame) = tail_frame {
        vm.push_tail_call_frame(frame);
    }
    Ok(true)
}

fn invoke_spec_push(vm: &mut Vm, _idx: usize, spec: CallSpec) -> WqResult<bool> {
    vm.enter_spec(spec)?;
    Ok(true)
}

fn prepare_tail(vm: &mut Vm, idx: usize, spec: CallSpec) -> WqResult<bool> {
    let tail_frame = if vm.debug_artifacts_enabled() {
        let chunk = vm.expect_current_chunk();
        Some(TailFrame {
            chunk,
            pc: idx + 1,
            func_name: vm.func_name_arc_for_chunk(chunk),
            instructions: Arc::clone(&vm.instructions),
        })
    } else {
        None
    };
    vm.prepare_tail(spec)?;
    if let Some(frame) = tail_frame {
        vm.push_tail_call_frame(frame);
    }
    Ok(true)
}

fn invoke_user_named(
    vm: &mut Vm,
    _idx: usize,
    target: &Value,
    argc: usize,
    name: &str,
) -> WqResult<bool> {
    let name_hint = CallSpec::name_hint(Some(name));
    if let Some(spec) = CallSpec::from_user_callable(target, argc, name_hint.clone()) {
        vm.enter_spec(spec)?;
        Ok(true)
    } else {
        let result = vm.invoke_user(target, argc, name_hint)?;
        vm.stack.push(result);
        Ok(false)
    }
}

fn tail_invoke_user_named(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
    _name: &str,
) -> WqResult<bool> {
    tail_invoke_user(vm, idx, target, argc)
}

#[inline]
fn dispatch_spec<const TAIL: bool>(vm: &mut Vm, idx: usize, mut spec: CallSpec) -> WqResult<bool> {
    spec.cache_idx = Some(idx);
    if TAIL {
        prepare_tail(vm, idx, spec)
    } else {
        invoke_spec_push(vm, idx, spec)
    }
}

#[inline]
fn dispatch_user_value<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
) -> WqResult<bool> {
    if TAIL {
        tail_invoke_user(vm, idx, target, argc)
    } else {
        invoke_user_push(vm, idx, target, argc)
    }
}

#[inline]
fn dispatch_user_named_value<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
    name: &str,
) -> WqResult<bool> {
    if TAIL {
        tail_invoke_user_named(vm, idx, target, argc, name)
    } else {
        invoke_user_named(vm, idx, target, argc, name)
    }
}

// --- dispatch helpers ---

pub(super) fn dispatch_postfix<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    match target {
        Value::BuiltinFunction { id, .. } => {
            if vm.try_start_resumable_builtin_value(*id, argc)? {
                return Ok(true);
            }
            let result = vm.invoke_builtin_value(*id, argc)?;
            vm.stack.push(result);
            Ok(false)
        }
        Value::Rng(rng) => {
            let result = vm.invoke_rng_value(rng, argc)?;
            vm.stack.push(result);
            Ok(false)
        }
        Value::LiftedCallable(data) => {
            let result = vm.invoke_function_composition_on_stack(data, argc)?;
            vm.stack.push(result);
            Ok(false)
        }
        Value::Cas(_) => {
            let result = vm.invoke_cas_callable_on_stack(target, argc)?;
            vm.stack.push(result);
            Ok(false)
        }
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            dispatch_user_value_cached::<TAIL>(vm, idx, target, argc, hooks, None)
        }
        _ => {
            if vm.pending_named_meta.take().is_some() {
                return Err(named_arg_index_err());
            }
            let base = vm.stack.len() - argc;
            let mut args: Sv4 = SmallVec::new();
            args.extend(vm.stack.drain(base..));
            let result = if args.len() == 1 {
                target.index(&args[0])
            } else {
                target.index_many(&args)
            };
            match result {
                Some(v) => vm.stack.push(v),
                None => {
                    let idx = if args.len() == 1 {
                        args.into_iter().next().unwrap()
                    } else {
                        Value::from_items(args.into_vec())
                    };
                    return Err(index_load_err(&idx, target));
                }
            }
            Ok(false)
        }
    }
}

pub(super) fn dispatch_anon_call<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    func: &Value,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    match func {
        Value::BuiltinFunction { id, .. } => {
            if vm.try_start_resumable_builtin_value(*id, argc)? {
                return Ok(true);
            }
            let out = vm.invoke_builtin_value(*id, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::Rng(rng) => {
            let out = vm.invoke_rng_value(rng, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::LiftedCallable(data) => {
            let out = vm.invoke_function_composition_on_stack(data, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            dispatch_user_value_cached::<TAIL>(vm, idx, func, argc, hooks, None)
        }
        _ => dispatch_user_value::<TAIL>(vm, idx, func, argc),
    }
}

fn dispatch_user_value_cached<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    func: &Value,
    argc: usize,
    hooks: &dyn InterpreterHook,
    name: Option<&str>,
) -> WqResult<bool> {
    let Some(identity) = user_callable_identity(func) else {
        return if let Some(name) = name {
            dispatch_user_named_value::<TAIL>(vm, idx, func, argc, name)
        } else {
            dispatch_user_value::<TAIL>(vm, idx, func, argc)
        };
    };

    if vm.inline_cache[idx].version == identity && vm.inline_cache[idx].call_target.is_some() {
        hooks.on_call_user_cache_hit();
        if try_pure_user_call(vm, idx, argc) {
            return Ok(false);
        }
        let spec = CallSpec::from_resolved(
            vm.inline_cache[idx]
                .call_target
                .as_ref()
                .expect("checked cached call target"),
            argc,
            CallSpec::name_hint(name),
        );
        dispatch_spec::<TAIL>(vm, idx, spec)?;
        return Ok(true);
    }

    hooks.on_call_user_cache_miss();
    let mut cache_value = func.clone();
    let dbg_chunk = if vm.debug_artifacts_enabled() {
        let existing_chunk = user_dbg_chunk(func);
        let debug_name = name
            .map(str::to_string)
            .or_else(|| {
                existing_chunk
                    .map(|chunk| vm.func_name_for_chunk(chunk))
                    .filter(|name| name != "<?>")
            })
            .unwrap_or_else(|| "<fn>".to_string());
        vm.stamp_user_function_debug_chunk(&mut cache_value, &debug_name, existing_chunk)
    } else {
        user_dbg_chunk(func)
    };
    let pure_call = PureCallback::compile(&cache_value, argc);
    if let Some(target) = ResolvedCallable::from_user_callable(cache_value, dbg_chunk) {
        let spec = CallSpec::from_resolved(&target, argc, CallSpec::name_hint(name));
        vm.inline_cache[idx].version = identity;
        vm.inline_cache[idx].call_target = Some(target);
        vm.inline_cache[idx].pure_call = pure_call;
        vm.inline_cache[idx].slot = None;
        vm.inline_cache[idx].slot_b = None;
        if try_pure_user_call(vm, idx, argc) {
            return Ok(false);
        }
        dispatch_spec::<TAIL>(vm, idx, spec)
    } else {
        if let Some(name) = name {
            dispatch_user_named_value::<TAIL>(vm, idx, func, argc, name)
        } else {
            dispatch_user_value::<TAIL>(vm, idx, func, argc)
        }
    }
}

fn try_pure_user_call(vm: &mut Vm, idx: usize, argc: usize) -> bool {
    if vm.debug_state.is_enabled()
        || vm.debug_log.enabled(DebugLogFlags::WQDB)
        || vm.hooks.is_some()
        || vm.trace_depth > 0
        || vm.cooperative_execution
        || vm.pending_named_meta.is_some()
    {
        return false;
    }
    let Some(callback) = vm.inline_cache[idx].pure_call.clone() else {
        return false;
    };
    let Some(base) = vm.stack.len().checked_sub(argc) else {
        return false;
    };
    let result = {
        let args = vm.stack[base..].iter().collect::<SmallVec<[&Value; 4]>>();
        callback.eval(&args)
    };
    let Ok(Some(value)) = result else {
        return false;
    };
    if value.is_callable() {
        return false;
    }
    vm.stack.truncate(base);
    vm.stack.push(value);
    true
}

pub(super) fn dispatch_loaded_local_call<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    _slot: u16,
    func: &Value,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    match func {
        Value::BuiltinFunction { id, .. } => {
            if vm.try_start_resumable_builtin_value(*id, argc)? {
                return Ok(true);
            }
            let out = vm.invoke_builtin_value(*id, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::Rng(rng) => {
            let out = vm.invoke_rng_value(rng, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            dispatch_user_value_cached::<TAIL>(vm, idx, func, argc, hooks, None)
        }
        other => Err(call_err("cannot call local value: expected callable").got1(other)),
    }
}

pub(super) fn dispatch_loaded_user_call<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    name: &str,
    func: &Value,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    match func {
        Value::BuiltinFunction { id, .. } => {
            if vm.try_start_resumable_builtin_value(*id, argc)? {
                return Ok(true);
            }
            let out = vm.invoke_builtin_value(*id, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            dispatch_user_value_cached::<TAIL>(vm, idx, func, argc, hooks, Some(name))
        }
        Value::LiftedCallable(_) | Value::Cas(_) | Value::Rng(_) => {
            dispatch_user_named_value::<TAIL>(vm, idx, func, argc, name)
        }
        other => {
            vm.pending_named_meta.take();
            Err(call_err(format!("cannot call '{name}': expected callable")).got1(other))
        }
    }
}

pub(super) fn user_callable_identity(func: &Value) -> Option<u64> {
    match func {
        Value::CompiledFunction(data) => Some(pointer_addr_to_u64(Arc::as_ptr(data).addr())),
        Value::Closure(data) => Some(pointer_addr_to_u64(Arc::as_ptr(data).addr())),
        _ => None,
    }
}

fn pointer_addr_to_u64(addr: usize) -> u64 {
    u64::try_from(addr).unwrap_or(u64::MAX)
}

fn user_dbg_chunk(func: &Value) -> Option<crate::wqdb::data::ChunkId> {
    func.as_user_function().and_then(|shape| shape.dbg_chunk)
}
