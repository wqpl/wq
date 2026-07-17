use std::sync::{Arc, Mutex};

use crate::value::Value;
use crate::value::cell::ValueCell;

#[derive(Clone)]
pub(crate) enum Slot {
    Value(Value),
    Cell(ValueCell),
}

impl Default for Slot {
    fn default() -> Self {
        Slot::Value(Value::empty_list())
    }
}

impl Slot {
    pub(crate) fn read(&self) -> Value {
        match self {
            Slot::Value(v) => v.clone(),
            Slot::Cell(cell) => cell.lock().expect("poisoned upvalue").clone(),
        }
    }

    pub(crate) fn write(&mut self, val: Value) {
        match self {
            Slot::Value(slot_val) => {
                *slot_val = val;
            }
            Slot::Cell(cell) => {
                *cell.lock().expect("poisoned upvalue") = val;
            }
        }
    }

    pub(crate) fn with_mut<R>(&mut self, f: impl FnOnce(&mut Value) -> R) -> R {
        match self {
            Slot::Value(slot_val) => f(slot_val),
            Slot::Cell(cell) => {
                let mut guard = cell.lock().expect("poisoned upvalue");
                f(&mut guard)
            }
        }
    }

    pub(crate) fn with_ref<R>(&self, f: impl FnOnce(&Value) -> R) -> R {
        match self {
            Slot::Value(slot_val) => f(slot_val),
            Slot::Cell(cell) => {
                let guard = cell.lock().expect("poisoned upvalue");
                f(&guard)
            }
        }
    }

    pub(crate) fn ensure_cell(&mut self) -> ValueCell {
        match self {
            Slot::Cell(cell) => cell.clone(),
            Slot::Value(slot_val) => {
                let current = std::mem::replace(slot_val, Value::empty_list());
                let cell = Arc::new(Mutex::new(current));
                *self = Slot::Cell(cell.clone());
                cell
            }
        }
    }
}
