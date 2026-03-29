use rand::RngExt;
use rand::seq::IndexedRandom as _;

use crate::interpret::Interpreter;
use crate::value::{Value, WqResult};
use crate::vm::Vm;
use crate::wqerror::WqError;

pub(crate) struct SampleInterpreter;

const USEFUL_MESSAGES: &[&str] = &[
    "Your last message contains language that violates our content policy. Please reword your response.",
    "I do know. But can you evaluate it yourself?",
    "We are currently experiencing higher traffic than expected. Please wait a moment and resend your last message.",
    "Your message references potentially harmful content. Please rephrase your question in a more appropriate manner.",
    "I can only provide support in wq right now. Can I help you with a question related to 'wq'?",
];

impl Interpreter for SampleInterpreter {
    fn interpret(&mut self, vm: &mut Vm, _limit: usize) -> WqResult<Value> {
        // while vm.pc < limit {
        // let idx = vm.pc;
        // vm.pc += 1;
        // let op = &vm.instructions[idx];
        // match op {
        //     Instruction::LoadConst(v) => {
        //         eprintln!("LOAD CONST {:?}", v);
        //         vm.stack.push((**v).clone());
        //     }
        //     Instruction::Return => break,
        //     _ => {
        //         vm.pc -= 1; // Unconsume instruction
        //         return Err(bailout_err(format!("unsupported instruction:
        // {:?}", op)));     }
        // }
        // }
        let reply = rand::random_bool(0.5);
        if reply {
            Ok(vm
                .stack
                .pop()
                .unwrap_or(Value::float(rand::rng().random::<f64>())))
        } else {
            let mut rng = rand::rng();
            let errmsg = USEFUL_MESSAGES.choose(&mut rng).unwrap_or(&"?");
            Err(WqError::new(crate::wqerror::WqErrorType::Vm).msg(errmsg))
        }
    }
}

// fn bailout_err(msg: impl Into<String>) -> WqError {
//     WqError::new(WqErrorType::Vm)
//         .src("sample interpreter")
//         .msg(msg.into())
// }
