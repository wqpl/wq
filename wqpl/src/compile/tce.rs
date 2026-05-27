use std::sync::Arc;

use crate::compile::Compiler;
use crate::value::Value;
use crate::value::func::FunctionData;
use crate::vm::inst::Instruction;

impl Compiler {
    pub(crate) fn rewrite_tail_calls(&mut self) {
        for inst in &mut self.instructions {
            match inst {
                Instruction::LoadConst(boxed) => {
                    rewrite_tail_calls_in_value(boxed.as_mut());
                }
                Instruction::LoadClosure(closure) => {
                    rewrite_tail_calls_in_closure_payload(closure);
                }
                _ => {}
            }
        }
    }
}

fn rewrite_tail_calls_in_value(value: &mut Value) {
    if let Value::CompiledFunction(func_data) = value {
        rewrite_tail_calls_in_function_data(Arc::make_mut(func_data));
    }
}

/// Recursively rewrite tail calls in a FunctionData.
fn rewrite_tail_calls_in_function_data(func_data: &mut FunctionData) {
    let instructions = Arc::make_mut(&mut func_data.instructions);

    // First, recursively process any nested functions in this function's
    // instructions
    for inst in instructions.iter_mut() {
        match inst {
            Instruction::LoadConst(boxed) => {
                rewrite_tail_calls_in_value(boxed.as_mut());
            }
            Instruction::LoadClosure(closure) => {
                rewrite_tail_calls_in_closure_payload(closure);
            }
            _ => {}
        }
    }

    // Then apply tail call rewriting to this function's instructions
    rewrite_tail_calls_in_slice(instructions);
}

/// Recursively rewrite tail calls in a ClosurePayload.
fn rewrite_tail_calls_in_closure_payload(payload: &mut crate::vm::inst::ClosurePayload) {
    let instructions = Arc::make_mut(&mut payload.instructions);

    // First, recursively process any nested functions
    for inst in instructions.iter_mut() {
        match inst {
            Instruction::LoadConst(boxed) => {
                rewrite_tail_calls_in_value(boxed.as_mut());
            }
            Instruction::LoadClosure(closure) => {
                rewrite_tail_calls_in_closure_payload(closure);
            }
            _ => {}
        }
    }

    // Then apply tail call rewriting
    rewrite_tail_calls_in_slice(instructions);
}

/// Core tail call rewriting logic.
///
/// For each call instruction followed by Return, replace with tail call
/// variant.
fn rewrite_tail_calls_in_slice(code: &mut [Instruction]) {
    // Early exit for empty or single-instruction sequences
    if code.len() < 2 {
        return;
    }

    let mut idx = 0;
    while idx < code.len() - 1 {
        if let Instruction::Try(len) = code[idx] {
            idx += len + 1;
            continue;
        }

        // Only process instructions followed by Return
        if !matches!(code[idx + 1], Instruction::Return) {
            idx += 1;
            continue;
        }

        let tail = match &code[idx] {
            Instruction::CallLocal(slot, argc) => Some(Instruction::TailCallLocal(*slot, *argc)),
            Instruction::CallUser(name, argc) => {
                Some(Instruction::TailCallUser(name.clone(), *argc))
            }
            Instruction::CallAnon(argc) => Some(Instruction::TailCallAnon(*argc)),
            Instruction::Postfix(argc) => Some(Instruction::TailPostfix(*argc)),
            Instruction::PostfixLocal(slot, argc) => {
                Some(Instruction::TailPostfixLocal(*slot, *argc))
            }
            Instruction::PostfixCapture(slot, argc) => {
                Some(Instruction::TailPostfixCapture(*slot, *argc))
            }
            Instruction::PostfixVar(name, argc) => {
                Some(Instruction::TailPostfixVar(name.clone(), *argc))
            }
            _ => None,
        };

        if let Some(tail_inst) = tail {
            code[idx] = tail_inst;
        }
        idx += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    fn create_test_function(instructions: Vec<Instruction>) -> Value {
        Value::CompiledFunction(Arc::new(FunctionData {
            params: None,
            named_params: None,
            locals: 0,
            instructions: instructions.into(),
            dbg_chunk: None,
            dbg_stmt_spans: None,
            dbg_source_base_offset: 0,
            dbg_pc_spans: None,
            dbg_stmt_marks: None,
            dbg_local_names: None,
            dbg_provenance: None,
        }))
    }

    #[test]
    fn test_tail_call_local_rewrite() {
        // Create function with tail call
        let func = create_test_function(vec![Instruction::CallLocal(0, 2), Instruction::Return]);

        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(func));
        compiler.rewrite_tail_calls();

        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                assert_eq!(f.instructions[0], Instruction::TailCallLocal(0, 2));
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_tail_call_user_rewrite() {
        // Create function with tail call
        let func = create_test_function(vec![
            Instruction::CallUser("fib".into(), 1),
            Instruction::Return,
        ]);

        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(func));
        compiler.rewrite_tail_calls();

        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                assert_eq!(
                    f.instructions[0],
                    Instruction::TailCallUser("fib".into(), 1)
                );
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_no_rewrite_without_return() {
        // Create function where CallLocal is not followed by Return
        let func = create_test_function(vec![
            Instruction::CallLocal(0, 2),
            Instruction::Pop,
            Instruction::Return,
        ]);

        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(func));
        compiler.rewrite_tail_calls();

        // Should remain unchanged since CallLocal is not followed by Return
        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                assert_eq!(f.instructions[0], Instruction::CallLocal(0, 2));
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_nested_function_rewrite() {
        // Create nested function with tail call
        let nested = create_test_function(vec![
            Instruction::CallUser("inner".into(), 0),
            Instruction::Return,
        ]);

        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(nested));

        compiler.rewrite_tail_calls();

        // Check that nested function was processed
        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                assert_eq!(
                    f.instructions[0],
                    Instruction::TailCallUser("inner".into(), 0)
                );
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_closure_rewrite() {
        // Create a closure payload with tail call
        let payload = crate::vm::inst::ClosurePayload {
            params: None,
            named_params: None,
            locals: 1,
            captures: vec![],
            instructions: vec![Instruction::CallLocal(0, 1), Instruction::Return].into(),
            dbg_stmt_spans: vec![].into(),
            dbg_pc_spans: vec![].into(),
            dbg_stmt_marks: vec![].into(),
            dbg_local_names: vec![].into(),
        };

        let mut compiler = Compiler::new();
        compiler
            .instructions
            .push(Instruction::LoadClosure(Box::new(payload)));

        compiler.rewrite_tail_calls();

        // Check that closure body was processed
        if let Instruction::LoadClosure(closure) = &compiler.instructions[0] {
            assert_eq!(closure.instructions[0], Instruction::TailCallLocal(0, 1));
        } else {
            panic!("Expected LoadClosure");
        }
    }

    #[test]
    fn test_all_tail_call_variants() {
        // Test all 6 tail call variants inside a function body
        let test_cases = vec![
            (
                Instruction::CallLocal(0, 2),
                Instruction::TailCallLocal(0, 2),
            ),
            (
                Instruction::CallUser("test".into(), 3),
                Instruction::TailCallUser("test".into(), 3),
            ),
            (Instruction::CallAnon(1), Instruction::TailCallAnon(1)),
            (Instruction::Postfix(2), Instruction::TailPostfix(2)),
            (
                Instruction::PostfixLocal(1, 3),
                Instruction::TailPostfixLocal(1, 3),
            ),
            (
                Instruction::PostfixCapture(2, 4),
                Instruction::TailPostfixCapture(2, 4),
            ),
            (
                Instruction::PostfixVar("f".into(), 1),
                Instruction::TailPostfixVar("f".into(), 1),
            ),
        ];

        for (input, expected) in test_cases {
            let func = create_test_function(vec![input, Instruction::Return]);
            let mut compiler = Compiler::new();
            compiler.instructions.push(Instruction::load_const(func));
            compiler.rewrite_tail_calls();

            if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
                if let Value::CompiledFunction(f) = &**boxed {
                    assert_eq!(f.instructions[0], expected);
                } else {
                    panic!("Expected CompiledFunction");
                }
            } else {
                panic!("Expected LoadConst");
            }
        }
    }

    #[test]
    fn test_no_tail_call_when_not_immediately_before_return() {
        let func = create_test_function(vec![
            Instruction::CallLocal(0, 1),
            Instruction::Pop,
            Instruction::Return,
        ]);
        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(func));
        compiler.rewrite_tail_calls();

        // CallLocal should not be rewritten since Pop is between it and Return
        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                assert_eq!(f.instructions[0], Instruction::CallLocal(0, 1));
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_no_tail_call_for_last_instruction_inside_try() {
        let func = create_test_function(vec![
            Instruction::Try(2),
            Instruction::LoadLocal(0),
            Instruction::Postfix(0),
            Instruction::Return,
        ]);
        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(func));
        compiler.rewrite_tail_calls();

        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                assert_eq!(f.instructions[2], Instruction::Postfix(0));
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_deeply_nested_function_rewrite() {
        // Test that deeply nested functions (functions inside functions inside
        // functions) also get their tail calls rewritten.
        //
        // Structure:
        //   outer_function {
        //     inner_function {
        //       CallOrIndex(3)   <- should become TailCallOrIndex(3)
        //       Return
        //     }
        //     Return
        //   }
        let inner_inner = create_test_function(vec![Instruction::Postfix(3), Instruction::Return]);

        let inner = create_test_function(vec![
            Instruction::load_const(inner_inner),
            Instruction::StoreLocal(0),
            Instruction::LoadLocal(0),
            Instruction::CallLocal(0, 0),
            Instruction::Return,
        ]);

        let mut compiler = Compiler::new();
        compiler.instructions.push(Instruction::load_const(inner));

        compiler.rewrite_tail_calls();

        // Check deeply nested function
        if let Instruction::LoadConst(boxed) = &compiler.instructions[0] {
            if let Value::CompiledFunction(f) = &**boxed {
                // The first instruction should be LoadConst with inner_inner
                if let Instruction::LoadConst(inner_boxed) = &f.instructions[0] {
                    if let Value::CompiledFunction(inner_f) = &**inner_boxed {
                        assert_eq!(
                            inner_f.instructions[0],
                            Instruction::TailPostfix(3),
                            "Deeply nested function should have TailCallOrIndex"
                        );
                    } else {
                        panic!("Expected inner CompiledFunction");
                    }
                } else {
                    panic!("Expected LoadConst at inner function");
                }
            } else {
                panic!("Expected CompiledFunction");
            }
        } else {
            panic!("Expected LoadConst");
        }
    }

    #[test]
    fn test_top_level_not_modified() {
        // Top-level code should NOT be modified (no local frame for tail calls)
        let code = vec![Instruction::CallLocal(0, 2), Instruction::Return];
        let mut compiler = Compiler::new();
        compiler.instructions = code.clone();
        compiler.rewrite_tail_calls();

        // Should remain unchanged - top-level code doesn't get tail call optimization
        assert_eq!(compiler.instructions[0], Instruction::CallLocal(0, 2));
    }
}
