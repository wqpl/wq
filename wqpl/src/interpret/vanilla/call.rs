use std::sync::Arc;

use smallvec::SmallVec;

use super::{
    Sv4, cas_binding_call_arg_err, index_load_err, named_arg_index_err, not_bound_err, vm_err,
};
use crate::interpret::InterpreterHook;
use crate::value::{Value, WqResult};
use crate::vm::call::{CallSpec, ResolvedCallable};
use crate::vm::{Frame, Vm};

// --- concrete dispatch functions passed by the interpret loop ---

pub(super) fn invoke_user_push(
    vm: &mut Vm,
    _idx: usize,
    target: &Value,
    argc: usize,
) -> WqResult<bool> {
    let result = vm.invoke_user(target, argc, None)?;
    vm.stack.push(result);
    Ok(false)
}

pub(super) fn tail_invoke_user(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
) -> WqResult<bool> {
    vm.push_tail_call_frame(Frame {
        chunk: vm.current_chunk,
        pc: idx + 1,
        func_name: Arc::from(vm.func_name_for_chunk(vm.current_chunk)),
    });
    vm.tail_invoke_user(target, argc)?;
    Ok(true)
}

pub(super) fn invoke_spec_push(vm: &mut Vm, _idx: usize, spec: CallSpec) -> WqResult<bool> {
    let result = vm.invoke_spec(spec)?;
    vm.stack.push(result);
    Ok(false)
}

pub(super) fn prepare_tail(vm: &mut Vm, idx: usize, spec: CallSpec) -> WqResult<bool> {
    vm.push_tail_call_frame(Frame {
        chunk: vm.current_chunk,
        pc: idx + 1,
        func_name: Arc::from(vm.func_name_for_chunk(vm.current_chunk)),
    });
    vm.prepare_tail(spec)?;
    Ok(true)
}

pub(super) fn invoke_user_named(
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

pub(super) fn tail_invoke_user_named(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
    _name: &str,
) -> WqResult<bool> {
    tail_invoke_user(vm, idx, target, argc)
}

// --- dispatch helpers ---

pub(super) fn dispatch_postfix(
    vm: &mut Vm,
    idx: usize,
    target: &Value,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
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
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            dispatch_user_value_cached(vm, idx, target, argc, spec_dispatch, user_dispatch, hooks)
        }
        Value::Cas(_) if vm.pending_named_meta.is_some() => {
            let args = vm.take_call_args_from_stack(argc)?;
            if !args.is_empty() {
                return Err(cas_binding_call_arg_err());
            }
            let result = crate::cas::substitute_cas_bindings(target, args.named_items())?;
            vm.stack.push(result);
            Ok(false)
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

pub(super) fn dispatch_anon_call(
    vm: &mut Vm,
    idx: usize,
    func: &Value,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
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
            dispatch_user_value_cached(vm, idx, func, argc, spec_dispatch, user_dispatch, hooks)
        }
        _ => user_dispatch(vm, idx, func, argc),
    }
}

pub(super) fn dispatch_method_postfix(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    if let Value::Dict(map) = receiver {
        let dict_identity = Arc::as_ptr(map) as usize;
        if vm.inline_cache[idx].slot_b == Some(dict_identity)
            && let Some(ref target) = vm.inline_cache[idx].call_target
        {
            let spec = CallSpec::from_resolved(target, argc, None);
            spec_dispatch(vm, idx, spec)?;
            hooks.on_call_user_cache_hit();
            return Ok(true);
        }

        let Some(target) = map.get(method.as_ref()) else {
            return Err(index_load_err(&Value::Tag(Arc::clone(method)), receiver));
        };
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
            return spec_dispatch(vm, idx, spec);
        }
        return dispatch_postfix(vm, idx, target, argc, spec_dispatch, user_dispatch, hooks);
    }

    dispatch_method_postfix_fallback(
        vm,
        idx,
        receiver,
        method,
        argc,
        spec_dispatch,
        user_dispatch,
        hooks,
    )
}

pub(super) fn dispatch_method_call(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    if let Value::Dict(map) = receiver {
        let dict_identity = Arc::as_ptr(map) as usize;
        if vm.inline_cache[idx].slot_b == Some(dict_identity)
            && let Some(ref target) = vm.inline_cache[idx].call_target
        {
            let spec = CallSpec::from_resolved(target, argc, None);
            spec_dispatch(vm, idx, spec)?;
            hooks.on_call_user_cache_hit();
            return Ok(true);
        }

        let Some(target) = map.get(method.as_ref()) else {
            return Err(index_load_err(&Value::Tag(Arc::clone(method)), receiver));
        };
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
            return spec_dispatch(vm, idx, spec);
        }
        return dispatch_anon_call(vm, idx, target, argc, spec_dispatch, user_dispatch, hooks);
    }

    dispatch_method_call_fallback(
        vm,
        idx,
        receiver,
        method,
        argc,
        spec_dispatch,
        user_dispatch,
        hooks,
    )
}

fn dispatch_method_call_fallback(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    let base = vm.stack.len() - argc;
    let args: Sv4 = vm.stack.drain(base..).collect();
    vm.stack.push(Value::Tag(Arc::clone(method)));
    let _ = dispatch_postfix(
        vm,
        idx,
        receiver,
        1,
        invoke_spec_push,
        invoke_user_push,
        hooks,
    )?;
    let target = vm
        .stack
        .pop()
        .ok_or_else(|| vm_err("method lookup produced no value"))?;
    vm.stack.extend(args);
    dispatch_anon_call(vm, idx, &target, argc, spec_dispatch, user_dispatch, hooks)
}

fn dispatch_method_postfix_fallback(
    vm: &mut Vm,
    idx: usize,
    receiver: &Value,
    method: &Arc<str>,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    let base = vm.stack.len() - argc;
    let args: Sv4 = vm.stack.drain(base..).collect();
    vm.stack.push(Value::Tag(Arc::clone(method)));
    let _ = dispatch_postfix(
        vm,
        idx,
        receiver,
        1,
        invoke_spec_push,
        invoke_user_push,
        hooks,
    )?;
    let target = vm
        .stack
        .pop()
        .ok_or_else(|| vm_err("method lookup produced no value"))?;
    vm.stack.extend(args);
    dispatch_postfix(vm, idx, &target, argc, spec_dispatch, user_dispatch, hooks)
}

fn dispatch_user_value_cached(
    vm: &mut Vm,
    idx: usize,
    func: &Value,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    let Some(identity) = user_callable_identity(func) else {
        return user_dispatch(vm, idx, func, argc);
    };

    if vm.inline_cache[idx].version == identity
        && let Some(ref target) = vm.inline_cache[idx].call_target
    {
        let spec = CallSpec::from_resolved(target, argc, None);
        spec_dispatch(vm, idx, spec)?;
        hooks.on_call_user_cache_hit();
        return Ok(true);
    }

    hooks.on_call_user_cache_miss();
    if let Some(target) = ResolvedCallable::from_user_callable(func.clone(), user_dbg_chunk(func)) {
        let spec = CallSpec::from_resolved(&target, argc, None);
        vm.inline_cache[idx].version = identity;
        vm.inline_cache[idx].call_target = Some(target);
        vm.inline_cache[idx].slot = None;
        vm.inline_cache[idx].slot_b = None;
        spec_dispatch(vm, idx, spec)
    } else {
        user_dispatch(vm, idx, func, argc)
    }
}

fn user_callable_identity(func: &Value) -> Option<u64> {
    match func {
        Value::CompiledFunction(data) => Some(Arc::as_ptr(data) as usize as u64),
        Value::Closure(data) => Some(Arc::as_ptr(data) as usize as u64),
        _ => None,
    }
}

fn user_dbg_chunk(func: &Value) -> Option<crate::wqdb::data::ChunkId> {
    func.as_user_function().and_then(|shape| shape.dbg_chunk)
}

pub(super) fn dispatch_user_call(
    vm: &mut Vm,
    idx: usize,
    name: &str,
    argc: usize,
    spec_dispatch: fn(&mut Vm, usize, CallSpec) -> WqResult<bool>,
    val_dispatch: fn(&mut Vm, usize, &Value, usize, &str) -> WqResult<bool>,
    hooks: &dyn InterpreterHook,
) -> WqResult<bool> {
    // Try inline cache
    if let Some(slot) = vm.lookup_global_slot(name) {
        let name_version = vm.global_slot_version(slot);
        if vm.inline_cache[idx].version == name_version
            && let Some(ref target) = vm.inline_cache[idx].call_target
        {
            let spec = CallSpec::from_resolved(target, argc, CallSpec::name_hint(Some(name)));
            spec_dispatch(vm, idx, spec)?;
            hooks.on_call_user_cache_hit();
            return Ok(true); // continue 'exec
        }
    }

    hooks.on_call_user_cache_miss();
    let func = vm.resolve_user_callable(idx, name)?;
    if let Value::BuiltinFunction { id, .. } = &func {
        let out = vm.invoke_bfn_value(*id, argc)?;
        vm.stack.push(out);
        return Ok(false);
    }

    val_dispatch(vm, idx, &func, argc, name)
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
