use super::{index_err, not_bound_err, vm_err};
use crate::value::access::{insert_in_place, parse_pop_count, pop_in_place, remove_in_place};
use crate::value::{Excerpt, Value, WqResult};
use crate::vm::inst::{MutationOp, StoreTarget};
use crate::vm::{Vm, pop1_stack};
use crate::wqerror::WqError;

pub(super) fn index_load_err(idx_val: &Value, target: &Value) -> WqError {
    index_err("invalid index")
        .attach_note(format!("index: '{}'", idx_val.excerpt()))
        .attach_note(format!("target: '{}'", target.excerpt()))
}

pub(super) fn pop_count_from_stack(vm: &mut Vm, has_count: bool) -> WqResult<usize> {
    if !has_count {
        return Ok(1);
    }

    let count = pop1_stack(&mut vm.stack, || "pop count".into())?;
    parse_pop_count(&count).map_err(|e: WqError| e.src("pop"))
}

pub(super) fn index_mutate(vm: &mut Vm, target: &StoreTarget, op: &MutationOp) -> WqResult<()> {
    match op {
        MutationOp::Pop => {
            let count = pop_count_from_stack(vm, true)?;
            let popped = mutate_target(vm, target, |t| pop_in_place(t, count))?;
            vm.stack.push(popped);
        }
        MutationOp::Remove => {
            let idx = pop1_stack(&mut vm.stack, || "remove index".into())?;
            let removed = mutate_target(vm, target, |t| remove_in_place(t, &idx))?;
            vm.stack.push(removed);
        }
        MutationOp::Insert | MutationOp::InsertAt => {
            let dsts = if matches!(op, MutationOp::InsertAt) {
                Some(pop1_stack(&mut vm.stack, || "insert destination".into())?)
            } else {
                None
            };
            let xs = pop1_stack(&mut vm.stack, || "insert value".into())?;
            let result = mutate_target(vm, target, |t| insert_in_place(t, &xs, dsts.as_ref()))?;
            vm.stack.push(result);
        }
    }
    Ok(())
}

pub(super) fn mutate_target(
    vm: &mut Vm,
    target: &StoreTarget,
    f: impl FnOnce(&mut Value) -> WqResult<Value>,
) -> WqResult<Value> {
    match target {
        StoreTarget::Var(name) => vm
            .with_global_slot_mut(name, f)
            .ok_or_else(|| not_bound_err(format!("'{name}' has not been bound to a value")))?,
        StoreTarget::Local(slot) => {
            let slot_ref = vm.local_slot_mut(*slot)?;
            slot_ref.with_mut(f)
        }
        StoreTarget::Capture(slot) => {
            let captures = vm
                .captures
                .last()
                .ok_or_else(|| vm_err("no capture frame"))?;
            let cell = captures
                .get(usize::from(*slot))
                .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
            let mut target = cell.lock().expect("poisoned capture");
            f(&mut target)
        }
    }
}

pub(super) fn store_var_impl(vm: &mut Vm, idx: usize, name: &str, keep: bool) -> WqResult<()> {
    let val = if keep {
        crate::vm::last_clone_stack(&vm.stack, || format!("store into variable '{name}'"))?
    } else {
        pop1_stack(&mut vm.stack, || format!("store into variable '{name}'"))?
    };
    if let Some(slot) = vm.inline_cache[idx].slot {
        vm.assign_global_at_slot(name, slot, val);
    } else {
        let slot = vm.assign_global_and_slot(name, val);
        vm.inline_cache[idx].slot = Some(slot);
    }
    Ok(())
}

pub(super) fn store_local_impl(vm: &mut Vm, i: u16, keep: bool) -> WqResult<()> {
    let slot = usize::from(i);
    let val = if keep {
        crate::vm::last_clone_stack(&vm.stack, || "store into local slot".into())
            .map_err(|e| vm.attach_local_slot_note(slot, e))?
    } else {
        pop1_stack(&mut vm.stack, || "store into local slot".into())
            .map_err(|e| vm.attach_local_slot_note(slot, e))?
    };
    if let Some(frame) = vm.locals.last_mut() {
        if let Some(dest) = frame.get_mut(slot) {
            dest.write(val);
        } else {
            return Err(vm.attach_local_slot_note(slot, vm_err("invalid local slot")));
        }
    } else {
        return Err(vm_err("no local frame"));
    }
    Ok(())
}
