use std::sync::Arc;

use indexmap::IndexMap;

use crate::compile::Compiler;
use crate::value::cmp::eval_cmp_chain;
use crate::value::func::FunctionData;
use crate::value::{Value, eval_binary, eval_unary};
use crate::vm::inst::{Capture, ClosurePayload, Instruction, MutationOp, Operand, StoreTarget};

impl Compiler {
    pub(crate) fn propagate_constants(&mut self) {
        propagate_nested(&mut self.instructions);
        let local_count = self.local_count();
        propagate_instructions(&mut self.instructions, local_count, Vec::new());
    }
}

fn propagate_nested(code: &mut [Instruction]) {
    for inst in code {
        match inst {
            Instruction::LoadConst(value) => {
                if let Value::CompiledFunction(func) = value.as_mut() {
                    propagate_function(Arc::make_mut(func));
                }
            }
            Instruction::LoadClosure(payload) => {
                propagate_closure_payload(payload, vec![None; payload.captures.len()]);
            }
            _ => {}
        }
    }
}

fn propagate_function(func: &mut FunctionData) {
    let instructions = Arc::make_mut(&mut func.instructions);
    propagate_nested(instructions);
    propagate_instructions(instructions, func.locals, Vec::new());
}

fn propagate_closure_payload(payload: &mut ClosurePayload, capture_values: Vec<Option<Value>>) {
    let instructions = Arc::make_mut(&mut payload.instructions);
    propagate_nested(instructions);
    propagate_instructions(instructions, payload.locals, capture_values);
}

fn propagate_instructions(
    code: &mut [Instruction],
    local_count: u16,
    capture_values: Vec<Option<Value>>,
) {
    if code.is_empty() {
        return;
    }

    let local_count = inferred_local_count(code).max(usize::from(local_count));
    let capture_count = inferred_capture_count(code).max(capture_values.len());
    let in_states = analyze(code, local_count, capture_count, capture_values);
    for (pc, inst) in code.iter_mut().enumerate() {
        if let Some(state) = &in_states[pc] {
            rewrite_instruction(inst, state);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct State {
    locals: Vec<Option<Value>>,
    captures: Vec<Option<Value>>,
    globals: IndexMap<String, Value>,
    stack: Option<Vec<Option<Value>>>,
}

impl State {
    fn new(local_count: usize, capture_count: usize, capture_values: Vec<Option<Value>>) -> Self {
        let mut captures = vec![None; capture_count];
        for (slot, value) in capture_values.into_iter().enumerate().take(capture_count) {
            captures[slot] = value.and_then(trackable_value);
        }
        Self {
            locals: vec![None; local_count],
            captures,
            globals: IndexMap::new(),
            stack: Some(Vec::new()),
        }
    }

    fn local(&self, slot: u16) -> Option<Value> {
        self.locals
            .get(usize::from(slot))
            .and_then(|value| value.clone())
    }

    fn set_local(&mut self, slot: u16, value: Option<Value>) {
        if let Some(local) = self.locals.get_mut(usize::from(slot)) {
            *local = value.and_then(trackable_value);
        }
    }

    fn capture(&self, slot: u16) -> Option<Value> {
        self.captures
            .get(usize::from(slot))
            .and_then(|value| value.clone())
    }

    fn set_capture(&mut self, slot: u16, value: Option<Value>) {
        if let Some(capture) = self.captures.get_mut(usize::from(slot)) {
            *capture = value.and_then(trackable_value);
        }
    }

    fn global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    fn set_global(&mut self, name: &str, value: Option<Value>) {
        if let Some(value) = value.and_then(trackable_value) {
            self.globals.insert(name.to_string(), value);
        } else {
            self.globals.swap_remove(name);
        }
    }

    fn clear_locals(&mut self) {
        for local in &mut self.locals {
            *local = None;
        }
    }

    fn clear_captures(&mut self) {
        for capture in &mut self.captures {
            *capture = None;
        }
    }

    fn clear_volatile_facts(&mut self) {
        self.clear_locals();
        self.clear_captures();
        self.globals.clear();
    }

    fn push(&mut self, value: Option<Value>) {
        if let Some(stack) = &mut self.stack {
            stack.push(value.and_then(trackable_value));
        }
    }

    fn push_unknown(&mut self) {
        self.push(None);
    }

    fn pop(&mut self) -> Option<Value> {
        let Some(stack) = &mut self.stack else {
            return None;
        };
        match stack.pop() {
            Some(value) => value,
            None => {
                self.stack = None;
                None
            }
        }
    }

    fn peek(&self) -> Option<Value> {
        self.stack
            .as_ref()
            .and_then(|stack| stack.last())
            .and_then(|value| value.clone())
    }

    fn pop_args(&mut self, argc: usize) -> Option<Vec<Value>> {
        let Some(stack) = &mut self.stack else {
            return None;
        };
        if stack.len() < argc {
            self.stack = None;
            return None;
        }
        let base = stack.len() - argc;
        let drained: Vec<Option<Value>> = stack.drain(base..).collect();
        collect_known(drained)
    }

    fn pop_call_target_and_args(&mut self, argc: usize) -> (Option<Value>, Option<Vec<Value>>) {
        let Some(stack) = &mut self.stack else {
            return (None, None);
        };
        if stack.len() < argc + 1 {
            self.stack = None;
            return (None, None);
        }
        let target_idx = stack.len() - argc - 1;
        let target = stack.remove(target_idx);
        let args = if argc == 0 {
            Some(Vec::new())
        } else {
            let base = stack.len() - argc;
            let drained: Vec<Option<Value>> = stack.drain(base..).collect();
            collect_known(drained)
        };
        (target, args)
    }

    fn pop_many(&mut self, count: usize) -> Option<Vec<Value>> {
        self.pop_args(count)
    }

    fn merge_from(&mut self, other: &State) -> bool {
        let mut changed = false;
        for (slot, incoming) in self.locals.iter_mut().zip(&other.locals) {
            let merged = meet_const(slot.as_ref(), incoming.as_ref());
            if *slot != merged {
                *slot = merged;
                changed = true;
            }
        }

        for (slot, incoming) in self.captures.iter_mut().zip(&other.captures) {
            let merged = meet_const(slot.as_ref(), incoming.as_ref());
            if *slot != merged {
                *slot = merged;
                changed = true;
            }
        }

        let old_globals = self.globals.clone();
        self.globals
            .retain(|name, value| other.globals.get(name) == Some(value));
        if self.globals != old_globals {
            changed = true;
        }

        let merged_stack = match (&self.stack, &other.stack) {
            (Some(left), Some(right)) if left.len() == right.len() => {
                let mut out = Vec::with_capacity(left.len());
                for (left_value, right_value) in left.iter().zip(right) {
                    out.push(meet_const(left_value.as_ref(), right_value.as_ref()));
                }
                Some(out)
            }
            _ => None,
        };
        if self.stack != merged_stack {
            self.stack = merged_stack;
            changed = true;
        }
        changed
    }
}

fn analyze(
    code: &[Instruction],
    local_count: usize,
    capture_count: usize,
    capture_values: Vec<Option<Value>>,
) -> Vec<Option<State>> {
    let mut states = vec![None; code.len()];
    states[0] = Some(State::new(local_count, capture_count, capture_values));
    let mut worklist = vec![0usize];

    while let Some(pc) = worklist.pop() {
        let Some(state) = states[pc].clone() else {
            continue;
        };
        for (target, next_state) in transfer(pc, &code[pc], state) {
            if target >= code.len() {
                continue;
            }
            let changed = match &mut states[target] {
                Some(existing) => existing.merge_from(&next_state),
                slot @ None => {
                    *slot = Some(next_state);
                    true
                }
            };
            if changed {
                worklist.push(target);
            }
        }
    }

    states
}

fn transfer(pc: usize, inst: &Instruction, mut state: State) -> Vec<(usize, State)> {
    use Instruction as I;
    match inst {
        I::LoadConst(value) => {
            state.push(Some((**value).clone()));
            fallthrough(pc, state)
        }
        I::LoadLocal(slot) => {
            state.push(state.local(*slot));
            fallthrough(pc, state)
        }
        I::LoadCapture(slot) => {
            state.push(state.capture(*slot));
            fallthrough(pc, state)
        }
        I::LoadVar(name) => {
            state.push(state.global(name));
            fallthrough(pc, state)
        }
        I::LoadVarExists(_) | I::LoadSelf => {
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::LoadClosure(_) => {
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::StoreLocal(slot) => {
            let value = state.pop();
            state.set_local(*slot, value);
            fallthrough(pc, state)
        }
        I::StoreLocalKeep(slot) => {
            state.set_local(*slot, state.peek());
            fallthrough(pc, state)
        }
        I::StoreVar(name) => {
            let value = state.pop();
            state.set_global(name, value);
            fallthrough(pc, state)
        }
        I::StoreVarKeep(name) => {
            state.set_global(name, state.peek());
            fallthrough(pc, state)
        }
        I::StoreCaptureKeep(slot) => {
            state.set_capture(*slot, state.peek());
            fallthrough(pc, state)
        }
        I::BinaryOp(data) => {
            let right = resolve_transfer_operand(&mut state, &data.right);
            let left = resolve_transfer_operand(&mut state, &data.left);
            let result = left
                .zip(right)
                .and_then(|(left, right)| eval_binary(&data.op, &left, &right).ok())
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::UnaryOp(data) => {
            let value = resolve_transfer_operand(&mut state, &data.operand);
            let result = value
                .and_then(|value| eval_unary(&data.op, &value).ok())
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::CmpChain(ops) => {
            let values = state.pop_many(ops.len() + 1);
            let result = values
                .and_then(|values| eval_cmp_chain(ops, &values).ok())
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::Cat(count) => {
            let result = state
                .pop_many(*count)
                .map(Value::cat_many)
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::MakeList(count) => {
            let result = state
                .pop_many(*count)
                .map(Value::from_items)
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::MakeDict(count) => {
            let result = state
                .pop_many(count * 2)
                .and_then(|values| make_const_dict(values, *count))
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }

        I::MakeRange { has_step, .. } => {
            if *has_step {
                state.pop();
            }
            state.pop();
            state.pop();
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::Index => {
            let index = state.pop();
            let object = state.pop();
            let result = object
                .zip(index)
                .and_then(|(object, index)| object.index(&index))
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::IndexLoadLocal(slot) => {
            let index = state.pop();
            let result = state
                .local(*slot)
                .zip(index)
                .and_then(|(object, index)| object.index(&index))
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::IndexLoadCapture(slot) => {
            let index = state.pop();
            let result = state
                .capture(*slot)
                .zip(index)
                .and_then(|(object, index)| object.index(&index))
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::IndexLoadVar(name) => {
            let index = state.pop();
            let result = state
                .global(name)
                .zip(index)
                .and_then(|(object, index)| object.index(&index))
                .and_then(trackable_value);
            state.push(result);
            fallthrough(pc, state)
        }
        I::IndexAssignLocal(slot) => {
            let value = state.pop();
            let index = state.pop();
            assign_local_index(&mut state, *slot, index, value.clone());
            state.push(value);
            fallthrough(pc, state)
        }
        I::IndexAssignLocalDrop(slot) => {
            let value = state.pop();
            let index = state.pop();
            assign_local_index(&mut state, *slot, index, value);
            fallthrough(pc, state)
        }
        I::IndexAssignCapture(slot) => {
            let value = state.pop();
            let index = state.pop();
            assign_capture_index(&mut state, *slot, index, value.clone());
            state.push(value);
            fallthrough(pc, state)
        }
        I::IndexAssignVar(name) => {
            let value = state.pop();
            state.pop();
            state.set_global(name, None);
            state.push(value);
            fallthrough(pc, state)
        }
        I::IndexAssignCaptureDrop(slot) => {
            let value = state.pop();
            let index = state.pop();
            assign_capture_index(&mut state, *slot, index, value);
            fallthrough(pc, state)
        }
        I::IndexAssignVarDrop(name) => {
            state.pop();
            state.pop();
            state.set_global(name, None);
            fallthrough(pc, state)
        }
        I::IndexMutate { target, op } => {
            match op {
                MutationOp::Pop => {
                    state.pop();
                }
                MutationOp::Remove => {
                    state.pop();
                }
                MutationOp::Insert => {
                    state.pop();
                }
                MutationOp::InsertAt => {
                    state.pop();
                    state.pop();
                }
            }
            match target {
                StoreTarget::Local(slot) => state.set_local(*slot, None),
                StoreTarget::Capture(slot) => state.set_capture(*slot, None),
                StoreTarget::Var(name) => state.set_global(name, None),
            }
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::CallBuiltinId(_, argc) => {
            state.pop_args(usize::from(*argc));
            state.clear_volatile_facts();
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::CallLocal(_, argc) | I::CallUser(_, argc) => {
            state.pop_args(*argc);
            state.clear_volatile_facts();
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::CallAnon(argc) => {
            state.pop_call_target_and_args(*argc);
            state.clear_volatile_facts();
            state.push_unknown();
            fallthrough(pc, state)
        }
        I::PostfixLocal(slot, argc) => {
            let args = state.pop_args(*argc);
            let target = state.local(*slot);
            let result = target
                .as_ref()
                .zip(args.as_ref())
                .and_then(|(target, args)| postfix_index(target, args))
                .and_then(trackable_value);
            if target.is_none() {
                state.clear_volatile_facts();
            }
            state.push(result);
            fallthrough(pc, state)
        }
        I::Postfix(argc) => {
            let (target, args) = state.pop_call_target_and_args(*argc);
            let result = target
                .as_ref()
                .zip(args.as_ref())
                .and_then(|(target, args)| postfix_index(target, args))
                .and_then(trackable_value);
            if target.is_none() {
                state.clear_volatile_facts();
            }
            state.push(result);
            fallthrough(pc, state)
        }
        I::PostfixCapture(slot, argc) => {
            let args = state.pop_args(*argc);
            let target = state.capture(*slot);
            let result = target
                .as_ref()
                .zip(args.as_ref())
                .and_then(|(target, args)| postfix_index(target, args))
                .and_then(trackable_value);
            if target.is_none() {
                state.clear_volatile_facts();
            }
            state.push(result);
            fallthrough(pc, state)
        }
        I::PostfixVar(name, argc) => {
            let args = state.pop_args(*argc);
            let target = state.global(name);
            let result = target
                .as_ref()
                .zip(args.as_ref())
                .and_then(|(target, args)| postfix_index(target, args))
                .and_then(trackable_value);
            if target.is_none() {
                state.clear_volatile_facts();
            }
            state.push(result);
            fallthrough(pc, state)
        }
        I::TailCallLocal(_, argc) | I::TailCallUser(_, argc) => {
            state.pop_args(*argc);
            Vec::new()
        }
        I::TailCallAnon(argc) | I::TailPostfix(argc) => {
            state.pop_call_target_and_args(*argc);
            Vec::new()
        }
        I::TailPostfixLocal(_, argc)
        | I::TailPostfixCapture(_, argc)
        | I::TailPostfixVar(_, argc) => {
            state.pop_args(*argc);
            Vec::new()
        }
        I::Jump(target) => vec![(*target, state)],
        I::JumpIfFalse(target) => {
            let cond = state.pop();
            match cond.and_then(|value| value.try_to_rust_bool()) {
                Some(true) => fallthrough(pc, state),
                Some(false) => vec![(*target, state)],
                None => vec![(pc + 1, state.clone()), (*target, state)],
            }
        }
        I::JumpIfGE(target) => {
            state.pop();
            state.pop();
            vec![(pc + 1, state.clone()), (*target, state)]
        }
        I::JumpIfLEZLocal(slot, target) => match state.local(*slot).and_then(le_zero) {
            Some(true) => vec![(*target, state)],
            Some(false) => fallthrough(pc, state),
            None => vec![(pc + 1, state.clone()), (*target, state)],
        },
        I::BoolAndLazy(target) => match state.peek().and_then(|value| value.try_to_rust_bool()) {
            Some(false) => {
                replace_stack_top(&mut state, Some(Value::Bool(false)));
                vec![(*target, state)]
            }
            Some(true) => fallthrough(pc, state),
            None => {
                let mut short = state.clone();
                replace_stack_top(&mut short, Some(Value::Bool(false)));
                vec![(pc + 1, state), (*target, short)]
            }
        },
        I::BoolOrLazy(target) => match state.peek().and_then(|value| value.try_to_rust_bool()) {
            Some(true) => {
                replace_stack_top(&mut state, Some(Value::Bool(true)));
                vec![(*target, state)]
            }
            Some(false) => fallthrough(pc, state),
            None => {
                let mut short = state.clone();
                replace_stack_top(&mut short, Some(Value::Bool(true)));
                vec![(pc + 1, state), (*target, short)]
            }
        },
        I::Pop => {
            state.pop();
            fallthrough(pc, state)
        }
        I::Return => Vec::new(),
        I::Assert | I::Debug | I::Pause | I::TraceBegin | I::PrepareNamedArgs(_) => {
            fallthrough(pc, state)
        }
        I::Try(len) => {
            let end = pc + 1 + len;
            let mut out = Vec::new();
            if *len > 0 {
                out.push((pc + 1, state.clone()));
            }
            let mut after_try = state;
            after_try.clear_volatile_facts();
            if let Some(stack) = &mut after_try.stack {
                stack.push(None);
            }
            out.push((end, after_try));
            out
        }
        I::LoadNamedArgsProvided(bit) => {
            let mask = state.pop();
            let provided = match mask {
                Some(Value::Int(mask)) => Some(Value::Bool((mask & (1i64 << bit)) != 0)),
                Some(_) | None => None,
            };
            state.push(provided);
            fallthrough(pc, state)
        }
    }
}

fn fallthrough(pc: usize, state: State) -> Vec<(usize, State)> {
    vec![(pc + 1, state)]
}

fn resolve_transfer_operand(state: &mut State, operand: &Operand) -> Option<Value> {
    match operand {
        Operand::Stack => state.pop(),
        Operand::Const(value) => trackable_value((**value).clone()),
        Operand::Local(slot) => state.local(*slot),
        Operand::Capture(slot) => state.capture(*slot),
        Operand::Var(name) => state.global(name),
        Operand::Self_ => None,
    }
}

fn rewrite_instruction(inst: &mut Instruction, state: &State) -> bool {
    match inst {
        Instruction::LoadLocal(slot) => {
            if let Some(value) = state.local(*slot) {
                *inst = Instruction::load_const(value);
                true
            } else {
                false
            }
        }
        Instruction::LoadCapture(slot) => {
            if let Some(value) = state.capture(*slot) {
                *inst = Instruction::load_const(value);
                true
            } else {
                false
            }
        }
        Instruction::LoadVar(name) => {
            if let Some(value) = state.global(name) {
                *inst = Instruction::load_const(value);
                true
            } else {
                false
            }
        }
        Instruction::LoadClosure(payload) => {
            let capture_values = closure_capture_values(state, &payload.captures);
            propagate_closure_payload(payload, capture_values);
            true
        }
        Instruction::BinaryOp(data) => {
            let mut changed = rewrite_operand(&mut data.left, state);
            changed |= rewrite_operand(&mut data.right, state);
            if !operand_uses_stack(&data.left)
                && !operand_uses_stack(&data.right)
                && let (Some(left), Some(right)) =
                    (operand_const(&data.left), operand_const(&data.right))
                && let Ok(value) = eval_binary(&data.op, left, right)
                && let Some(value) = trackable_value(value)
            {
                *inst = Instruction::load_const(value);
                return true;
            }
            changed
        }
        Instruction::UnaryOp(data) => {
            let mut changed = rewrite_operand(&mut data.operand, state);
            if !operand_uses_stack(&data.operand)
                && let Some(value) = operand_const(&data.operand)
                && let Ok(value) = eval_unary(&data.op, value)
                && let Some(value) = trackable_value(value)
            {
                *inst = Instruction::load_const(value);
                changed = true;
            }
            changed
        }
        _ => false,
    }
}

fn rewrite_operand(operand: &mut Operand, state: &State) -> bool {
    if let Operand::Local(slot) = operand
        && let Some(value) = state.local(*slot)
    {
        *operand = Operand::const_val(value);
        return true;
    }
    if let Operand::Capture(slot) = operand
        && let Some(value) = state.capture(*slot)
    {
        *operand = Operand::const_val(value);
        return true;
    }
    if let Operand::Var(name) = operand
        && let Some(value) = state.global(name)
    {
        *operand = Operand::const_val(value);
        return true;
    }
    false
}

fn operand_const(operand: &Operand) -> Option<&Value> {
    if let Operand::Const(value) = operand {
        Some(value)
    } else {
        None
    }
}

fn operand_uses_stack(operand: &Operand) -> bool {
    matches!(operand, Operand::Stack)
}

fn closure_capture_values(state: &State, captures: &[Capture]) -> Vec<Option<Value>> {
    captures
        .iter()
        .map(|capture| match capture {
            Capture::Local(slot) => state.local(*slot),
            Capture::Global(name, _) => state.global(name),
            Capture::LocalShared(_) | Capture::Outer(_) => None,
        })
        .collect()
}

fn assign_local_index(state: &mut State, slot: u16, index: Option<Value>, value: Option<Value>) {
    let Some(index) = index else {
        state.set_local(slot, None);
        return;
    };
    let Some(value) = value else {
        state.set_local(slot, None);
        return;
    };
    let Some(mut target) = state.local(slot) else {
        state.set_local(slot, None);
        return;
    };
    if target.assign_by_index(&index, value).is_some() {
        state.set_local(slot, Some(target));
    } else {
        state.set_local(slot, None);
    }
}

fn assign_capture_index(state: &mut State, slot: u16, index: Option<Value>, value: Option<Value>) {
    let Some(index) = index else {
        state.set_capture(slot, None);
        return;
    };
    let Some(value) = value else {
        state.set_capture(slot, None);
        return;
    };
    let Some(mut target) = state.capture(slot) else {
        state.set_capture(slot, None);
        return;
    };
    if target.assign_by_index(&index, value).is_some() {
        state.set_capture(slot, Some(target));
    } else {
        state.set_capture(slot, None);
    }
}

fn make_const_dict(values: Vec<Value>, count: usize) -> Option<Value> {
    if values.len() != count * 2 {
        return None;
    }
    let mut map = IndexMap::with_capacity(count);
    let mut iter = values.into_iter();
    for _ in 0..count {
        let key = iter.next()?;
        let value = iter.next()?;
        if let Value::Tag(key) = key {
            map.insert(key, value);
        } else {
            return None;
        }
    }
    Some(Value::Dict(Arc::new(map)))
}

fn postfix_index(target: &Value, args: &[Value]) -> Option<Value> {
    match args {
        [arg] => target.index(arg),
        args => target.index_many(args),
    }
}

fn collect_known(values: Vec<Option<Value>>) -> Option<Vec<Value>> {
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        out.push(value?);
    }
    Some(out)
}

fn replace_stack_top(state: &mut State, value: Option<Value>) {
    let Some(stack) = &mut state.stack else {
        return;
    };
    if let Some(top) = stack.last_mut() {
        *top = value.and_then(trackable_value);
    }
}

fn meet_const(left: Option<&Value>, right: Option<&Value>) -> Option<Value> {
    match (left, right) {
        (Some(left), Some(right)) if left == right => Some(left.clone()),
        _ => None,
    }
}

fn trackable_value(value: Value) -> Option<Value> {
    match value {
        Value::CompiledFunction(_)
        | Value::Closure(_)
        | Value::BuiltinFunction { .. }
        | Value::Stream(_) => None,
        other => Some(other),
    }
}

fn le_zero(value: Value) -> Option<bool> {
    match value {
        Value::Int(n) => Some(n <= 0),
        Value::Float(n) => Some(*n <= 0.0),
        _ => None,
    }
}

fn inferred_local_count(code: &[Instruction]) -> usize {
    let mut count = 0usize;
    for inst in code {
        note_inst_locals(inst, &mut count);
    }
    count
}

fn inferred_capture_count(code: &[Instruction]) -> usize {
    let mut count = 0usize;
    for inst in code {
        note_inst_captures(inst, &mut count);
    }
    count
}

fn note_inst_locals(inst: &Instruction, count: &mut usize) {
    use Instruction as I;
    match inst {
        I::LoadLocal(slot)
        | I::StoreLocal(slot)
        | I::StoreLocalKeep(slot)
        | I::CallLocal(slot, _)
        | I::TailCallLocal(slot, _)
        | I::PostfixLocal(slot, _)
        | I::TailPostfixLocal(slot, _)
        | I::IndexLoadLocal(slot)
        | I::IndexAssignLocal(slot)
        | I::IndexAssignLocalDrop(slot)
        | I::JumpIfLEZLocal(slot, _) => note_slot(*slot, count),
        I::BinaryOp(data) => {
            note_operand_locals(&data.left, count);
            note_operand_locals(&data.right, count);
        }
        I::UnaryOp(data) => note_operand_locals(&data.operand, count),
        I::IndexMutate {
            target: StoreTarget::Local(slot),
            ..
        } => note_slot(*slot, count),
        _ => {}
    }
}

fn note_operand_locals(operand: &Operand, count: &mut usize) {
    if let Operand::Local(slot) = operand {
        note_slot(*slot, count);
    }
}

fn note_inst_captures(inst: &Instruction, count: &mut usize) {
    use Instruction as I;
    match inst {
        I::LoadCapture(slot)
        | I::StoreCaptureKeep(slot)
        | I::PostfixCapture(slot, _)
        | I::TailPostfixCapture(slot, _)
        | I::IndexLoadCapture(slot)
        | I::IndexAssignCapture(slot)
        | I::IndexAssignCaptureDrop(slot) => note_slot(*slot, count),
        I::BinaryOp(data) => {
            note_operand_captures(&data.left, count);
            note_operand_captures(&data.right, count);
        }
        I::UnaryOp(data) => note_operand_captures(&data.operand, count),
        I::IndexMutate {
            target: StoreTarget::Capture(slot),
            ..
        } => note_slot(*slot, count),
        _ => {}
    }
}

fn note_operand_captures(operand: &Operand, count: &mut usize) {
    if let Operand::Capture(slot) = operand {
        note_slot(*slot, count);
    }
}

fn note_slot(slot: u16, count: &mut usize) {
    *count = (*count).max(usize::from(slot) + 1);
}
