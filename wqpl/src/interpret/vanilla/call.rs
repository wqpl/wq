use std::sync::Arc;

use smallvec::SmallVec;

use super::{Sv4, index_load_err, named_arg_index_err, not_bound_err, vm_err};
use crate::interpret::InterpreterHook;
use crate::value::{Value, WqResult};
use crate::vm::call::{CallSpec, ResolvedCallable};
use crate::vm::{Frame, Vm};

// --- concrete dispatch functions passed by the interpret loop ---

fn invoke_user_push(vm: &mut Vm, _idx: usize, target: &Value, argc: usize) -> WqResult<bool> {
    let result = vm.invoke_user(target, argc, None)?;
    vm.stack.push(result);
    Ok(false)
}

fn tail_invoke_user(vm: &mut Vm, idx: usize, target: &Value, argc: usize) -> WqResult<bool> {
    if vm.debug_artifacts_enabled() {
        vm.push_tail_call_frame(Frame {
            chunk: vm.current_chunk,
            pc: idx + 1,
            func_name: vm.func_name_arc_for_chunk(vm.current_chunk),
        });
    }
    vm.tail_invoke_user(target, argc)?;
    Ok(true)
}

fn invoke_spec_push(vm: &mut Vm, _idx: usize, spec: CallSpec) -> WqResult<bool> {
    let result = vm.invoke_spec(spec)?;
    vm.stack.push(result);
    Ok(false)
}

fn prepare_tail(vm: &mut Vm, idx: usize, spec: CallSpec) -> WqResult<bool> {
    if vm.debug_artifacts_enabled() {
        vm.push_tail_call_frame(Frame {
            chunk: vm.current_chunk,
            pc: idx + 1,
            func_name: vm.func_name_arc_for_chunk(vm.current_chunk),
        });
    }
    vm.prepare_tail(spec)?;
    Ok(true)
}

fn invoke_user_named(
    vm: &mut Vm,
    _idx: usize,
    target: &Value,
    argc: usize,
    name: &str,
) -> WqResult<bool> {
    let result = vm.invoke_user(target, argc, CallSpec::name_hint(Some(name)))?;
    vm.stack.push(result);
    Ok(false)
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
fn dispatch_spec<const TAIL: bool>(vm: &mut Vm, idx: usize, spec: CallSpec) -> WqResult<bool> {
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
            let result = vm.invoke_bfn_value(*id, argc)?;
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
            let out = vm.invoke_bfn_value(*id, argc)?;
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

pub(super) fn dispatch_method_postfix<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    if let Value::Dict(map) = receiver {
        let dict_identity = Arc::as_ptr(map).addr();
        let Some(target) = map.get(method.as_ref()) else {
            clear_call_target_cache(vm, idx);
            return Err(index_load_err(&Value::Tag(Arc::clone(method)), receiver));
        };
        if vm.inline_cache[idx].slot_b == Some(dict_identity)
            && let Some(identity) = user_callable_identity(target)
            && vm.inline_cache[idx].version == identity
            && let Some(ref cached) = vm.inline_cache[idx].call_target
        {
            let spec = CallSpec::from_resolved(cached, argc, None);
            dispatch_spec::<TAIL>(vm, idx, spec)?;
            hooks.on_call_user_cache_hit();
            return Ok(true);
        }
        if let Some(identity) = user_callable_identity(target)
            && let Some(resolved) =
                ResolvedCallable::from_user_callable(target.clone(), user_dbg_chunk(target))
        {
            let spec = CallSpec::from_resolved(&resolved, argc, None);
            vm.inline_cache[idx].version = identity;
            vm.inline_cache[idx].call_target = Some(resolved);
            vm.inline_cache[idx].slot = None;
            vm.inline_cache[idx].slot_b = Some(dict_identity);
            hooks.on_call_user_cache_miss();
            return dispatch_spec::<TAIL>(vm, idx, spec);
        }
        clear_call_target_cache(vm, idx);
        return dispatch_postfix::<TAIL>(vm, idx, target, argc, hooks);
    }

    dispatch_method_postfix_fallback::<TAIL>(vm, idx, receiver, method, argc, hooks)
}

pub(super) fn dispatch_method_call<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    if let Value::Dict(map) = receiver {
        let dict_identity = Arc::as_ptr(map).addr();
        let Some(target) = map.get(method.as_ref()) else {
            clear_call_target_cache(vm, idx);
            return Err(index_load_err(&Value::Tag(Arc::clone(method)), receiver));
        };
        if vm.inline_cache[idx].slot_b == Some(dict_identity)
            && let Some(identity) = user_callable_identity(target)
            && vm.inline_cache[idx].version == identity
            && let Some(ref cached) = vm.inline_cache[idx].call_target
        {
            let spec = CallSpec::from_resolved(cached, argc, None);
            dispatch_spec::<TAIL>(vm, idx, spec)?;
            hooks.on_call_user_cache_hit();
            return Ok(true);
        }
        if let Some(identity) = user_callable_identity(target)
            && let Some(resolved) =
                ResolvedCallable::from_user_callable(target.clone(), user_dbg_chunk(target))
        {
            let spec = CallSpec::from_resolved(&resolved, argc, None);
            vm.inline_cache[idx].version = identity;
            vm.inline_cache[idx].call_target = Some(resolved);
            vm.inline_cache[idx].slot = None;
            vm.inline_cache[idx].slot_b = Some(dict_identity);
            hooks.on_call_user_cache_miss();
            return dispatch_spec::<TAIL>(vm, idx, spec);
        }
        clear_call_target_cache(vm, idx);
        return dispatch_anon_call::<TAIL>(vm, idx, target, argc, hooks);
    }

    dispatch_method_call_fallback::<TAIL>(vm, idx, receiver, method, argc, hooks)
}

fn dispatch_method_call_fallback<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    let base = vm.stack.len() - argc;
    let args: Sv4 = vm.stack.drain(base..).collect();
    vm.stack.push(Value::Tag(Arc::clone(method)));
    let _ = dispatch_postfix::<false>(vm, idx, receiver, 1, hooks)?;
    let target = vm
        .stack
        .pop()
        .ok_or_else(|| vm_err("method lookup produced no value"))?;
    vm.stack.extend(args);
    dispatch_anon_call::<TAIL>(vm, idx, &target, argc, hooks)
}

fn dispatch_method_postfix_fallback<const TAIL: bool>(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    let base = vm.stack.len() - argc;
    let args: Sv4 = vm.stack.drain(base..).collect();
    vm.stack.push(Value::Tag(Arc::clone(method)));
    let _ = dispatch_postfix::<false>(vm, idx, receiver, 1, hooks)?;
    let target = vm
        .stack
        .pop()
        .ok_or_else(|| vm_err("method lookup produced no value"))?;
    vm.stack.extend(args);
    dispatch_postfix::<TAIL>(vm, idx, &target, argc, hooks)
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

    if vm.inline_cache[idx].version == identity
        && let Some(ref target) = vm.inline_cache[idx].call_target
    {
        let spec = CallSpec::from_resolved(target, argc, CallSpec::name_hint(name));
        dispatch_spec::<TAIL>(vm, idx, spec)?;
        hooks.on_call_user_cache_hit();
        return Ok(true);
    }

    hooks.on_call_user_cache_miss();
    if let Some(target) = ResolvedCallable::from_user_callable(func.clone(), user_dbg_chunk(func)) {
        let spec = CallSpec::from_resolved(&target, argc, CallSpec::name_hint(name));
        vm.inline_cache[idx].version = identity;
        vm.inline_cache[idx].call_target = Some(target);
        vm.inline_cache[idx].slot = None;
        vm.inline_cache[idx].slot_b = None;
        dispatch_spec::<TAIL>(vm, idx, spec)
    } else {
        if let Some(name) = name {
            dispatch_user_named_value::<TAIL>(vm, idx, func, argc, name)
        } else {
            dispatch_user_value::<TAIL>(vm, idx, func, argc)
        }
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
            let out = vm.invoke_bfn_value(*id, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            dispatch_user_value_cached::<TAIL>(vm, idx, func, argc, hooks, Some(name))
        }
        Value::LiftedCallable(_) | Value::Cas(_) => {
            dispatch_user_named_value::<TAIL>(vm, idx, func, argc, name)
        }
        other => {
            vm.pending_named_meta.take();
            Err(not_bound_err(format!(
                "cannot call '{name}': expected callable, got {}",
                other.type_name()
            )))
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

fn clear_call_target_cache(vm: &mut Vm, idx: usize) {
    let entry = &mut vm.inline_cache[idx];
    entry.version = 0;
    entry.call_target = None;
    entry.slot_b = None;
}

pub(super) fn resolve_postfix_var(vm: &mut Vm, idx: usize, name: &str) -> WqResult<Value> {
    if let Some(slot) = vm.inline_cache[idx].slot
        && let Some(val) = vm.global_slot_value(slot)
    {
        return Ok(val.clone());
    }
    if let Some(slot) = vm.lookup_global_slot(name) {
        let val = vm
            .global_slot_value(slot)
            .ok_or_else(|| vm_err("invalid global slot"))?
            .clone();
        vm.inline_cache[idx].slot = Some(slot);
        return Ok(val);
    }
    if let Some(value) = vm.builtins.get_value(name) {
        return Ok(value);
    }
    if vm.builtins.is_disabled_name(name) {
        return Err(
            not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(format!(
                "a builtin named '{name}' exists but is disabled in the current preset"
            )),
        );
    }
    Err(not_bound_err(format!(
        "'{name}' has not been bound to a value"
    )))
}
