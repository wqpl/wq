use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};
use std::collections::HashMap;

pub struct FastPath;

impl FastPath {
    /// try to execute a sequence of instructions directly without full VM
    /// dispatch. Returns `None` if an unsupported instruction is encountered.
    pub fn try_run(instructions: &[Instruction], builtins: &Builtins) -> Option<WqResult<Value>> {
        // pre-scan for unsupported instructions to avoid executing builtins with side-effect
        for ins in instructions {
            match ins {
                Instruction::LoadConst(_)
                | Instruction::LoadVar(_)
                | Instruction::StoreVar(_)
                | Instruction::BinaryOp(_)
                | Instruction::UnaryOp(_)
                | Instruction::CallBuiltinId(_, _)
                | Instruction::CallBuiltin(_, _)
                | Instruction::Jump(_)
                | Instruction::JumpIfFalse(_)
                | Instruction::Pop
                | Instruction::Return => {}
                _ => return None,
            }
        }

        let mut stack: Vec<Value> = Vec::new();
        let mut vars: HashMap<String, Value> = HashMap::new();
        let mut pc = 0;
        while pc < instructions.len() {
            match &instructions[pc] {
                Instruction::LoadConst(v) => stack.push(v.clone()),
                Instruction::LoadVar(name) => match vars.get(name) {
                    Some(v) => stack.push(v.clone()),
                    None => {
                        return Some(Err(WqError::DomainError(format!(
                            "Undefined variable: {name}"
                        ))));
                    }
                },
                Instruction::StoreVar(name) => {
                    let v = stack.pop()?;
                    vars.insert(name.clone(), v);
                }
                Instruction::BinaryOp(op) => {
                    let b = stack.pop()?;
                    let a = stack.pop()?;
                    let res = match op {
                        BinaryOperator::Add => a.add(&b),
                        BinaryOperator::Subtract => a.subtract(&b),
                        BinaryOperator::Multiply => a.multiply(&b),
                        BinaryOperator::Divide => a.divide(&b),
                        BinaryOperator::Modulo => a.modulo(&b),
                        _ => return None,
                    }?;
                    stack.push(res);
                }
                Instruction::UnaryOp(op) => {
                    let v = stack.pop()?;
                    let res = match op {
                        UnaryOperator::Negate => match v {
                            Value::Int(n) => Value::Int(-n),
                            Value::Float(f) => Value::Float(-f),
                            _ => return None,
                        },
                        UnaryOperator::Count => match v {
                            Value::List(l) => Value::Int(l.len() as i64),
                            Value::IntList(a) => Value::Int(a.len() as i64),
                            _ => return None,
                        },
                    };
                    stack.push(res);
                }
                Instruction::CallBuiltinId(id, argc) => {
                    let mut args = Vec::with_capacity(*argc);
                    for _ in 0..*argc {
                        args.push(stack.pop()?);
                    }
                    args.reverse();
                    let res = match builtins.call_id(*id as usize, &args) {
                        Ok(v) => v,
                        Err(_) => return None,
                    };
                    stack.push(res);
                }
                Instruction::CallBuiltin(name, argc) => {
                    let mut args = Vec::with_capacity(*argc);
                    for _ in 0..*argc {
                        args.push(stack.pop()?);
                    }
                    args.reverse();
                    let res = match builtins.call(name, &args) {
                        Ok(v) => v,
                        Err(_) => return None,
                    };
                    stack.push(res);
                }
                Instruction::Jump(target) => {
                    pc = *target;
                    continue;
                }
                Instruction::JumpIfFalse(target) => {
                    let v = stack.pop()?;
                    let falsey =
                        matches!(v, Value::Bool(false) | Value::Int(0) | Value::Float(0.0));
                    if falsey {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::Pop => {
                    stack.pop();
                }
                Instruction::Return => break,
                _ => return None,
            }
            pc += 1;
        }
        Some(Ok(stack.pop().unwrap_or(Value::Null)))
    }
}
