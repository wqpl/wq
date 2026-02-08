use crate::{
    interpreters::Interpreter,
    value::{Value, WqResult},
    vm::{Vm, instruction::Instruction},
    wqerror::{WqError, WqErrorType},
};

pub struct SampleInterpreter;

impl Interpreter for SampleInterpreter {
    fn execute(&mut self, vm: &mut Vm, limit: usize) -> WqResult<Value> {
        while vm.pc < limit {
            let idx = vm.pc;
            vm.pc += 1;
            let op = &vm.instructions[idx];

            match op {
                Instruction::LoadConst(v) => {
                    eprintln!("LOAD CONST {:?}", v);
                    vm.stack.push(v.clone());
                }
                Instruction::Return => break,
                _ => {
                    vm.pc -= 1; // Unconsume instruction
                    return Err(bailout_err(format!("unsupported instruction: {:?}", op)));
                }
            }
        }
        Ok(vm.stack.pop().unwrap_or(Value::unit()))
    }
}

fn bailout_err(msg: impl Into<String>) -> WqError {
    WqError::new(WqErrorType::Vm)
        .src("sample interpreter")
        .msg(msg.into())
}
