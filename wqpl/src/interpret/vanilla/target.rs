use super::vm_err;
use crate::value::{Value, WqResult};
use crate::vm::Vm;

pub(super) fn read_local_target(vm: &mut Vm, slot: usize) -> WqResult<Value> {
    let frame = vm.locals.last().ok_or_else(|| vm_err("no local frame"))?;
    let slot_ref = frame.get(slot).ok_or_else(|| {
        vm.attach_local_slot_note(slot, vm_err(format!("invalid local slot {slot}")))
    })?;
    Ok(slot_ref.read())
}

pub(super) fn read_capture_target(vm: &mut Vm, slot: usize) -> WqResult<Value> {
    let captures = vm
        .captures
        .last()
        .ok_or_else(|| vm_err("no capture frame"))?;
    let cell = captures
        .get(slot)
        .ok_or_else(|| vm_err(format!("invalid capture slot {slot}")))?;
    Ok(cell.lock().expect("poisoned capture").clone())
}
