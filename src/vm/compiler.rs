use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{AstNode, BinaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};

pub struct Compiler {
    pub instructions: Vec<Instruction>,
    builtins: Builtins,
    loop_id: usize,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            builtins: Builtins::new(),
            loop_id: 0,
        }
    }

    pub fn compile(&mut self, node: &AstNode) -> WqResult<()> {
        match node {
            AstNode::Literal(v) => self.instructions.push(Instruction::LoadConst(v.clone())),
            AstNode::Variable(name) => self.instructions.push(Instruction::LoadVar(name.clone())),
            AstNode::Assignment { name, value } => {
                self.compile(value)?;
                self.instructions.push(Instruction::StoreVar(name.clone()));
                self.instructions.push(Instruction::LoadVar(name.clone()));
            }
            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => {
                self.compile(left)?;
                self.compile(right)?;
                self.instructions
                    .push(Instruction::BinaryOp(operator.clone()));
            }
            AstNode::UnaryOp { operator, operand } => {
                self.compile(operand)?;
                self.instructions
                    .push(Instruction::UnaryOp(operator.clone()));
            }
            AstNode::List(elements) => {
                for elem in elements {
                    self.compile(elem)?;
                }
                self.instructions
                    .push(Instruction::MakeList(elements.len()));
            }
            AstNode::Dict(pairs) => {
                for (k, v) in pairs {
                    self.instructions
                        .push(Instruction::LoadConst(Value::Symbol(k.clone())));
                    self.compile(v)?;
                }
                self.instructions.push(Instruction::MakeDict(pairs.len()));
            }
            AstNode::Call { name, args } => {
                for arg in args {
                    self.compile(arg)?;
                }
                if let Some(id) = self.builtins.get_id(name) {
                    self.instructions
                        .push(Instruction::CallBuiltinId(id as u8, args.len()));
                } else {
                    self.instructions
                        .push(Instruction::CallUser(name.clone(), args.len()));
                }
            }
            AstNode::CallAnonymous { object, args } => {
                self.compile(object)?;
                for arg in args {
                    self.compile(arg)?;
                }
                self.instructions.push(Instruction::CallAnon(args.len()));
            }
            AstNode::Index { object, index } => {
                self.compile(object)?;
                self.compile(index)?;
                self.instructions.push(Instruction::Index);
            }
            AstNode::IndexAssign {
                object,
                index,
                value,
            } => {
                if let AstNode::Variable(name) = &**object {
                    self.instructions
                        .push(Instruction::LoadConst(Value::Symbol(name.clone())));
                } else {
                    return Err(WqError::DomainError(
                        "Invalid index assignment target".into(),
                    ));
                }
                self.compile(index)?;
                self.compile(value)?;
                self.instructions.push(Instruction::IndexAssign);
            }
            AstNode::Function { params, body } => {
                let mut c = Compiler::new();
                c.compile(body)?;
                let mut func_instructions = c.instructions;
                func_instructions.push(Instruction::Return);
                self.instructions
                    .push(Instruction::LoadConst(Value::BytecodeFunction {
                        params: params.clone(),
                        instructions: func_instructions,
                    }));
            }
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                self.compile(condition)?;
                let jump_if_false_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.compile(true_branch)?;
                let jump_end_pos = self.instructions.len();
                self.instructions.push(Instruction::Jump(0));
                // patch jump_if_false to here
                let else_start = self.instructions.len();
                self.instructions[jump_if_false_pos] = Instruction::JumpIfFalse(else_start);
                if let Some(fb) = false_branch {
                    self.compile(fb)?;
                }
                let end = self.instructions.len();
                self.instructions[jump_end_pos] = Instruction::Jump(end);
            }
            AstNode::WhileLoop { condition, body } => {
                let start = self.instructions.len();
                self.compile(condition)?;
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.compile(body)?;
                self.instructions.push(Instruction::Pop);
                self.instructions.push(Instruction::Jump(start));
                let end = self.instructions.len();
                self.instructions[jump_pos] = Instruction::JumpIfFalse(end);
                self.instructions.push(Instruction::LoadConst(Value::Null));
            }
            AstNode::ForLoop { count, body } => {
                // Unroll constant loops. Fully unroll up to 16
                // iterations and partially unroll larger loops in chunks of 8.
                if let AstNode::Literal(Value::Int(n)) = &**count {
                    if *n >= 0 {
                        let limit = 16;
                        if *n <= limit {
                            if *n == 0 {
                                self.instructions.push(Instruction::LoadConst(Value::Null));
                            } else {
                                for i in 0..*n {
                                    self.instructions
                                        .push(Instruction::LoadConst(Value::Int(i)));
                                    self.instructions
                                        .push(Instruction::StoreVar("_n".to_string()));
                                    self.compile(body)?;
                                    if i < *n - 1 {
                                        self.instructions.push(Instruction::Pop);
                                    }
                                }
                            }
                            return Ok(());
                        } else if *n <= 64 {
                            // unroll in chunks of 8
                            let full_chunks = *n / 8;
                            let remainder = *n % 8;
                            for c in 0..full_chunks {
                                for i in 0..8 {
                                    let idx = c * 8 + i;
                                    self.instructions
                                        .push(Instruction::LoadConst(Value::Int(idx)));
                                    self.instructions
                                        .push(Instruction::StoreVar("_n".to_string()));
                                    self.compile(body)?;
                                    self.instructions.push(Instruction::Pop);
                                }
                            }
                            for i in 0..remainder {
                                let idx = full_chunks * 8 + i;
                                self.instructions
                                    .push(Instruction::LoadConst(Value::Int(idx)));
                                self.instructions
                                    .push(Instruction::StoreVar("_n".to_string()));
                                self.compile(body)?;
                                if i < remainder - 1 {
                                    self.instructions.push(Instruction::Pop);
                                }
                            }
                            if *n > 0 {
                                self.instructions.pop();
                            } else {
                                self.instructions.push(Instruction::LoadConst(Value::Null));
                            }
                            return Ok(());
                        }
                    }
                }

                let id = self.loop_id;
                self.loop_id += 1;
                let count_var = format!("__count{id}");
                let result_var = format!("__for_result{id}");
                let old_var = format!("__old_n{id}");

                self.compile(count)?; // -> count on stack
                self.instructions
                    .push(Instruction::StoreVar(count_var.clone()));
                self.instructions
                    .push(Instruction::LoadConst(Value::Int(0)));
                self.instructions
                    .push(Instruction::StoreVar("_n".to_string()));
                self.instructions.push(Instruction::LoadConst(Value::Null));
                self.instructions
                    .push(Instruction::StoreVar(result_var.clone()));
                let start = self.instructions.len();
                self.instructions
                    .push(Instruction::LoadVar("_n".to_string()));
                self.instructions
                    .push(Instruction::LoadVar(count_var.clone()));
                self.instructions
                    .push(Instruction::BinaryOp(BinaryOperator::LessThan));
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.instructions
                    .push(Instruction::LoadVar("_n".to_string()));
                self.instructions
                    .push(Instruction::StoreVar(old_var.clone()));
                self.compile(body)?;
                self.instructions
                    .push(Instruction::StoreVar(result_var.clone()));
                self.instructions
                    .push(Instruction::LoadVar(old_var.clone()));
                self.instructions
                    .push(Instruction::LoadConst(Value::Int(1)));
                self.instructions
                    .push(Instruction::BinaryOp(BinaryOperator::Add));
                self.instructions
                    .push(Instruction::StoreVar("_n".to_string()));
                self.instructions.push(Instruction::Jump(start));
                let end = self.instructions.len();
                self.instructions[jump_pos] = Instruction::JumpIfFalse(end);
                self.instructions.push(Instruction::LoadVar(result_var));
            }
            AstNode::Block(stmts) => {
                if stmts.is_empty() {
                    // Empty blocks evaluate to null
                    self.instructions.push(Instruction::LoadConst(Value::Null));
                } else {
                    for stmt in stmts {
                        self.compile(stmt)?;
                        self.instructions.push(Instruction::Pop);
                    }
                    // remove last pop to keep result of final statement
                    self.instructions.pop();
                }
            }
        }
        Ok(())
    }
}
