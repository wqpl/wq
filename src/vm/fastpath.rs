use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::{Value, WqError};
use std::collections::{HashSet, VecDeque};

pub struct FastPath;

#[derive(Debug)]
#[allow(dead_code)]
//todo
pub enum TryRunOutcome {
    Ok(Value),
    VmError(WqError),
    Bail(BailReason),
}

#[allow(dead_code)]
//todo
#[derive(Debug)]
pub enum BailReason {
    UnsupportedAt(usize),
    InvalidJump { pc: usize, target: usize },
    StackUnderflow(usize),
    StepLimit,
    Overflow(usize),
    BuiltinError(WqError),
}

#[inline]
fn pop(stack: &mut Vec<Value>) -> Option<Value> {
    stack.pop()
}

#[inline]
fn pop2(stack: &mut Vec<Value>) -> Option<(Value, Value)> {
    let b = stack.pop()?;
    let a = stack.pop()?;
    Some((a, b))
}

#[inline]
fn take_args(stack: &[Value], argc: usize) -> Option<&[Value]> {
    if stack.len() < argc {
        None
    } else {
        Some(&stack[stack.len() - argc..])
    }
}

#[inline]
fn is_falsey(v: &Value) -> bool {
    match v {
        Value::Bool(false) => true,
        Value::Int(0) => true,
        Value::Float(f) if *f == 0.0 => true,
        _ => false,
    }
}

#[inline]
fn check_target(pc: usize, target: usize, len: usize) -> Result<usize, BailReason> {
    if target >= len {
        Err(BailReason::InvalidJump { pc, target })
    } else {
        Ok(target)
    }
}

struct Analysis {
    locals: usize,
    max_stack: usize,
}

impl FastPath {
    fn analyze(instructions: &[Instruction]) -> Result<Analysis, BailReason> {
        // check supported and collect local slot count
        let mut locals = 0usize;
        for (pc, ins) in instructions.iter().enumerate() {
            match ins {
                Instruction::LoadConst(_)
                | Instruction::LoadLocal(_)
                | Instruction::StoreLocal(_)
                | Instruction::BinaryOp(_)
                | Instruction::UnaryOp(_)
                | Instruction::CallBuiltinId(_, _)
                | Instruction::CallBuiltin(_, _)
                | Instruction::Jump(_)
                | Instruction::JumpIfFalse(_)
                | Instruction::Pop
                | Instruction::Return => {}
                _ => return Err(BailReason::UnsupportedAt(pc)),
            }
            if let Instruction::LoadLocal(i) | Instruction::StoreLocal(i) = ins {
                locals = locals.max(*i as usize + 1);
            }
            if matches!(ins, Instruction::LoadVar(_) | Instruction::StoreVar(_)) {
                return Err(BailReason::UnsupportedAt(pc));
            }
        }
        let len = instructions.len();
        for (pc, ins) in instructions.iter().enumerate() {
            if let Instruction::Jump(t) | Instruction::JumpIfFalse(t) = ins {
                if *t >= len {
                    return Err(BailReason::InvalidJump { pc, target: *t });
                }
            }
        }

        // stack analysis via BFS
        let mut queue = VecDeque::new();
        let mut visited = HashSet::new();
        queue.push_back((0usize, 0usize));
        let mut max_stack = 0usize;
        while let Some((pc, depth)) = queue.pop_front() {
            if pc >= len {
                continue;
            }
            if !visited.insert((pc, depth)) {
                continue;
            }
            if depth > max_stack {
                max_stack = depth;
            }
            let ins = &instructions[pc];
            match ins {
                Instruction::LoadConst(_) => queue.push_back((pc + 1, depth + 1)),
                Instruction::LoadLocal(_) => queue.push_back((pc + 1, depth + 1)),
                Instruction::StoreLocal(_) => {
                    if depth == 0 {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                    queue.push_back((pc + 1, depth - 1));
                }
                Instruction::BinaryOp(_) => {
                    if depth < 2 {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                    queue.push_back((pc + 1, depth - 1));
                }
                Instruction::UnaryOp(_) => {
                    if depth < 1 {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                    queue.push_back((pc + 1, depth));
                }
                Instruction::CallBuiltinId(_, argc) | Instruction::CallBuiltin(_, argc) => {
                    if depth < *argc {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                    queue.push_back((pc + 1, depth + 1 - *argc));
                }
                Instruction::Jump(t) => queue.push_back((*t, depth)),
                Instruction::JumpIfFalse(t) => {
                    if depth < 1 {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                    let next_depth = depth - 1;
                    queue.push_back((pc + 1, next_depth));
                    queue.push_back((*t, next_depth));
                }
                Instruction::Pop => {
                    if depth < 1 {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                    queue.push_back((pc + 1, depth - 1));
                }
                Instruction::Return => {
                    if depth < 1 {
                        return Err(BailReason::StackUnderflow(pc));
                    }
                }
                _ => return Err(BailReason::UnsupportedAt(pc)),
            }
        }
        Ok(Analysis { locals, max_stack })
    }

    pub fn try_run(
        instructions: &[Instruction],
        builtins: &Builtins,
        step_limit: Option<usize>,
    ) -> TryRunOutcome {
        let analysis = match Self::analyze(instructions) {
            Ok(a) => a,
            Err(br) => return TryRunOutcome::Bail(br),
        };
        let mut stack: Vec<Value> = Vec::with_capacity(analysis.max_stack.max(32));
        let mut locals: Vec<Value> = vec![Value::Null; analysis.locals];
        let mut pc = 0usize;
        let mut steps = 0usize;
        let hard_limit = step_limit.unwrap_or(100_000);

        while pc < instructions.len() {
            steps += 1;
            if steps > hard_limit {
                return TryRunOutcome::Bail(BailReason::StepLimit);
            }
            match &instructions[pc] {
                Instruction::LoadConst(v) => stack.push(v.clone()),
                Instruction::LoadLocal(i) => {
                    stack.push(locals[*i as usize].clone());
                }
                Instruction::StoreLocal(i) => {
                    let v = match pop(&mut stack) {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::StackUnderflow(pc)),
                    };
                    locals[*i as usize] = v;
                }
                Instruction::BinaryOp(op) => {
                    let (a, b) = match pop2(&mut stack) {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::StackUnderflow(pc)),
                    };
                    let res = match op {
                        BinaryOperator::Add => a.add(&b),
                        BinaryOperator::Subtract => a.subtract(&b),
                        BinaryOperator::Multiply => a.multiply(&b),
                        BinaryOperator::Power => a.power(&b),
                        BinaryOperator::Divide => a.divide(&b),
                        BinaryOperator::DivideDot => a.divide_dot(&b),
                        BinaryOperator::Modulo => a.modulo(&b),
                        BinaryOperator::ModuloDot => a.modulo_dot(&b),
                        _ => return TryRunOutcome::Bail(BailReason::UnsupportedAt(pc)),
                    };
                    let res = match res {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::UnsupportedAt(pc)),
                    };
                    stack.push(res);
                }
                Instruction::UnaryOp(op) => {
                    let v = match pop(&mut stack) {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::StackUnderflow(pc)),
                    };
                    let res = match op {
                        UnaryOperator::Negate => match v.neg_value() {
                            Some(val) => val,
                            None => return TryRunOutcome::Bail(BailReason::UnsupportedAt(pc)),
                        },
                        UnaryOperator::Count => match v {
                            Value::List(l) => Value::Int(l.len() as i64),
                            Value::IntList(a) => Value::Int(a.len() as i64),
                            _ => return TryRunOutcome::Bail(BailReason::UnsupportedAt(pc)),
                        },
                    };
                    stack.push(res);
                }
                Instruction::CallBuiltinId(id, argc) => {
                    let argc = *argc;
                    let args = match take_args(&stack, argc) {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::StackUnderflow(pc)),
                    };
                    let res = match builtins.call_id(*id as usize, args) {
                        Ok(v) => v,
                        Err(e) => return TryRunOutcome::Bail(BailReason::BuiltinError(e)),
                    };
                    let new_len = stack.len() - argc;
                    stack.truncate(new_len);
                    stack.push(res);
                }
                Instruction::CallBuiltin(name, argc) => {
                    let argc = *argc;
                    let args = match take_args(&stack, argc) {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::StackUnderflow(pc)),
                    };
                    let res = match builtins.call(name, args) {
                        Ok(v) => v,
                        Err(e) => return TryRunOutcome::Bail(BailReason::BuiltinError(e)),
                    };
                    let new_len = stack.len() - argc;
                    stack.truncate(new_len);
                    stack.push(res);
                }
                Instruction::Jump(t) => {
                    pc = match check_target(pc, *t, instructions.len()) {
                        Ok(v) => v,
                        Err(br) => return TryRunOutcome::Bail(br),
                    };
                    continue;
                }
                Instruction::JumpIfFalse(t) => {
                    let v = match pop(&mut stack) {
                        Some(v) => v,
                        None => return TryRunOutcome::Bail(BailReason::StackUnderflow(pc)),
                    };
                    if is_falsey(&v) {
                        pc = match check_target(pc, *t, instructions.len()) {
                            Ok(v) => v,
                            Err(br) => return TryRunOutcome::Bail(br),
                        };
                        continue;
                    }
                }
                Instruction::Pop => {
                    if pop(&mut stack).is_none() {
                        return TryRunOutcome::Bail(BailReason::StackUnderflow(pc));
                    }
                }
                Instruction::Return => break,
                _ => return TryRunOutcome::Bail(BailReason::UnsupportedAt(pc)),
            }
            pc += 1;
        }
        let result = stack.pop().unwrap_or(Value::Null);
        TryRunOutcome::Ok(result)
    }
}
