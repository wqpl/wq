use super::{index_err, not_bound_err, vm_err};
use crate::value::access::{insert_in_place, parse_pop_count, pop_in_place, remove_in_place};
use crate::value::{Excerpt, Value, WqResult};
use crate::vm::inst::{MutationOp, StoreTarget};
use crate::vm::{Vm, pop1_stack};
use crate::wqdb::SymbolMutationKind;
use crate::wqerror::WqError;

pub(super) fn index_load_err(idx_val: &Value, target: &Value) -> WqError {
    index_err("invalid index")
        .attach_note(format!(
            "index: {} ({})",
            idx_val.excerpt(),
            idx_val.category()
        ))
        .attach_note(format!(
            "target: {} ({})",
            target.excerpt(),
            target.category()
        ))
}

pub(super) fn pop_count_from_stack(vm: &mut Vm, has_count: bool) -> WqResult<usize> {
    if !has_count {
        return Ok(1);
    }

    let count = pop1_stack(&mut vm.stack, || "pop count".into())?;
    parse_pop_count(&count).map_err(|e: WqError| e.src("pop"))
}

pub(super) fn index_mutate(
    vm: &mut Vm,
    pc: usize,
    target: &StoreTarget,
    op: &MutationOp,
) -> WqResult<()> {
    let op_label = mutation_op_label(op);
    match op {
        MutationOp::Pop => {
            let count = pop_count_from_stack(vm, true)?;
            let popped = mutate_target(vm, pc, target, op_label, |t| pop_in_place(t, count))?;
            vm.stack.push(popped);
        }
        MutationOp::Remove => {
            let idx = pop1_stack(&mut vm.stack, || "remove index".into())?;
            let removed = mutate_target(vm, pc, target, op_label, |t| remove_in_place(t, &idx))?;
            vm.stack.push(removed);
        }
        MutationOp::Insert | MutationOp::InsertAt => {
            let dsts = if matches!(op, MutationOp::InsertAt) {
                Some(pop1_stack(&mut vm.stack, || "insert destination".into())?)
            } else {
                None
            };
            let xs = pop1_stack(&mut vm.stack, || "insert value".into())?;
            let result = mutate_target(vm, pc, target, op_label, |t| {
                insert_in_place(t, &xs, dsts.as_ref())
            })?;
            vm.stack.push(result);
        }
    }
    Ok(())
}

fn mutation_op_label(op: &MutationOp) -> SymbolMutationKind {
    match op {
        MutationOp::Pop => SymbolMutationKind::Pop,
        MutationOp::Remove => SymbolMutationKind::Remove,
        MutationOp::Insert => SymbolMutationKind::Insert,
        MutationOp::InsertAt => SymbolMutationKind::InsertAt,
    }
}

pub(super) fn mutate_target(
    vm: &mut Vm,
    pc: usize,
    target: &StoreTarget,
    operation: SymbolMutationKind,
    f: impl FnOnce(&mut Value) -> WqResult<Value>,
) -> WqResult<Value> {
    let track = vm.symbol_trackers_enabled();
    match target {
        StoreTarget::Var(name) => {
            let mut change = None;
            let result = vm
                .with_global_slot_mut(name, |target| {
                    let old = track.then(|| target.clone());
                    let result = f(target)?;
                    if let Some(old) = old {
                        change = Some((old, target.clone()));
                    }
                    Ok(result)
                })
                .ok_or_else(|| {
                    not_bound_err(format!("'{name}' has not been bound to a value"))
                })??;
            if let Some((old, new)) = change {
                vm.note_global_symbol_write(pc, name, operation, Some(old), new);
            }
            Ok(result)
        }
        StoreTarget::Local(slot) => {
            let change;
            let result = {
                let slot_ref = vm.local_slot_mut(*slot)?;
                let old = track.then(|| slot_ref.read());
                let result = slot_ref.with_mut(f)?;
                change = old.map(|old| (old, slot_ref.read()));
                result
            };
            if let Some((old, new)) = change {
                vm.note_local_symbol_write(pc, *slot, operation, Some(old), new);
            }
            Ok(result)
        }
        StoreTarget::Capture(slot) => {
            let change;
            let result = {
                let captures = vm
                    .captures
                    .last()
                    .ok_or_else(|| vm_err("no capture frame"))?;
                let cell = captures
                    .get(usize::from(*slot))
                    .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
                let mut target = cell.lock().expect("poisoned capture");
                let old = track.then(|| target.clone());
                let result = f(&mut target)?;
                change = old.map(|old| (old, target.clone()));
                result
            };
            if let Some((old, new)) = change {
                vm.note_capture_symbol_write(pc, *slot, operation, Some(old), new);
            }
            Ok(result)
        }
    }
}

pub(super) fn store_var_impl(vm: &mut Vm, idx: usize, name: &str, keep: bool) -> WqResult<()> {
    let val = if keep {
        crate::vm::last_clone_stack(&vm.stack, || format!("store into variable '{name}'"))?
    } else {
        pop1_stack(&mut vm.stack, || format!("store into variable '{name}'"))?
    };
    let track = vm.symbol_trackers_enabled();
    let old = if track {
        vm.lookup_global_ref(name).cloned()
    } else {
        None
    };
    let new = track.then(|| val.clone());
    if let Some(slot) = vm.inline_cache[idx].slot {
        vm.assign_global_at_slot(name, slot, val);
    } else {
        let slot = vm.assign_global_and_slot(name, val);
        vm.inline_cache[idx].slot = Some(slot);
    }
    if let Some(new) = new {
        vm.note_global_symbol_write(idx, name, SymbolMutationKind::Store, old, new);
    }
    Ok(())
}

pub(super) fn store_local_impl(vm: &mut Vm, idx: usize, i: u16, keep: bool) -> WqResult<()> {
    let slot = usize::from(i);
    let val = if keep {
        crate::vm::last_clone_stack(&vm.stack, || "store into local slot".into())
            .map_err(|e| vm.attach_local_slot_note(slot, e))?
    } else {
        pop1_stack(&mut vm.stack, || "store into local slot".into())
            .map_err(|e| vm.attach_local_slot_note(slot, e))?
    };
    let track = vm.symbol_trackers_enabled();
    let new = track.then(|| val.clone());
    let mut old = None;
    if let Some(frame) = vm.locals.last_mut() {
        if let Some(dest) = frame.get_mut(slot) {
            if track {
                old = Some(dest.read());
            }
            dest.write(val);
        } else {
            return Err(vm.attach_local_slot_note(slot, vm_err("invalid local slot")));
        }
    } else {
        return Err(vm_err("no local frame"));
    }
    if let Some(new) = new {
        vm.note_local_symbol_write(idx, i, SymbolMutationKind::Store, old, new);
    }
    Ok(())
}
