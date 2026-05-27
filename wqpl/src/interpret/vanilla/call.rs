use std::sync::Arc;

use smallvec::SmallVec;

use super::{Sv4, index_load_err, named_arg_index_err, not_bound_err, vm_err};
use crate::interpret::InterpreterHook;
use crate::value::{Value, WqResult};
use crate::vm::call::CallSpec;
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
        pc: idx,
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
        pc: idx,
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
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
) -> WqResult<bool> {
    match target {
        Value::BuiltinFunction(name) => {
            let result = vm.invoke_bfn_name(name, argc)?;
            vm.stack.push(result);
            Ok(false)
        }
        Value::FunctionComposition(data) => {
            let result = vm.invoke_function_composition_on_stack(data, argc)?;
            vm.stack.push(result);
            Ok(false)
        }
        Value::CompiledFunction { .. } | Value::Closure { .. } => {
            user_dispatch(vm, idx, target, argc)
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
    user_dispatch: fn(&mut Vm, usize, &Value, usize) -> WqResult<bool>,
) -> WqResult<bool> {
    match func {
        Value::BuiltinFunction(name) => {
            let out = vm.invoke_bfn_name(name, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        Value::FunctionComposition(data) => {
            let out = vm.invoke_function_composition_on_stack(data, argc)?;
            vm.stack.push(out);
            Ok(false)
        }
        _ => user_dispatch(vm, idx, func, argc),
    }
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
    if let Value::BuiltinFunction(bname) = &func {
        let out = vm.invoke_bfn_name(bname, argc)?;
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
    if vm.builtins.has_function(name) {
        return Ok(Value::BuiltinFunction(Arc::from(name)));
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
