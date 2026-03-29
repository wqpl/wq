use std::sync::Arc;

use super::{not_bound_err, vm_err};
use crate::interpret::InterpreterHook;
use crate::value::{Value, WqResult};
use crate::vm::inst::Operand;
use crate::vm::{Vm, pop1_stack};

pub(super) fn resolve_operand(
    vm: &mut Vm,
    idx: usize,
    operand: &Operand,
    op_idx: u8,
    hooks: &dyn InterpreterHook,
) -> WqResult<Value> {
    match operand {
        Operand::Stack => pop1_stack(&mut vm.stack, || "operand stack pop".into()),
        Operand::Const(v) => Ok((**v).clone()),
        Operand::Local(i) => {
            let slot = usize::from(*i);
            let slot_num = *i;
            let val = vm.locals.last().and_then(|f| f.get(slot)).ok_or_else(|| {
                vm.attach_local_slot_note(slot, vm_err(format!("invalid local slot {slot_num}")))
            })?;
            Ok(val.read())
        }
        Operand::Capture(i) => {
            let slot = usize::from(*i);
            let slot_num = *i;
            let cell = vm
                .captures
                .last()
                .and_then(|c| c.get(slot))
                .ok_or_else(|| vm_err(format!("invalid capture slot {slot_num}")))?;
            Ok(cell.lock().expect("poisoned capture").clone())
        }
        Operand::Var(name) => {
            let cache_slot = if op_idx == 0 {
                vm.inline_cache[idx].slot
            } else {
                vm.inline_cache[idx].slot_b
            };
            if let Some(slot) = cache_slot
                && let Some(val) = vm.global_slot_value(slot)
            {
                hooks.on_load_var_cache_hit(&|| true);
                return Ok(val.clone());
            }
            hooks.on_load_var_cache_miss();
            if let Some(slot) = vm.lookup_global_slot(name) {
                let val = vm
                    .global_slot_value(slot)
                    .ok_or_else(|| vm_err("invalid global slot"))?
                    .clone();
                if op_idx == 0 {
                    vm.inline_cache[idx].slot = Some(slot);
                } else {
                    vm.inline_cache[idx].slot_b = Some(slot);
                }
                Ok(val)
            } else if vm.builtins.has_function(name) {
                Ok(Value::BuiltinFunction(Arc::from(name.as_ref())))
            } else if vm.builtins.is_disabled_name(name) {
                Err(
                    not_bound_err(format!("'{name}' has not been bound to a value")).attach_note(
                        format!(
                            "a builtin named '{name}' exists but is disabled in the current preset"
                        ),
                    ),
                )
            } else {
                Err(not_bound_err(format!(
                    "'{name}' has not been bound to a value"
                )))
            }
        }
        Operand::Self_ => vm
            .current_closure_stack
            .last()
            .cloned()
            .ok_or_else(|| vm_err("LoadSelf outside fn")),
    }
}
