use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{AstNode, BinaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};
use indexmap::IndexMap;

#[derive(Default)]
struct LoopInfo {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

fn has_ctrl(node: &AstNode) -> bool {
    match node {
        AstNode::Break | AstNode::Continue | AstNode::Return(_) => true,
        AstNode::Block(stmts) => stmts.iter().any(has_ctrl),
        AstNode::Conditional {
            true_branch,
            false_branch,
            ..
        } => has_ctrl(true_branch) || false_branch.as_ref().is_some_and(|b| has_ctrl(b)),
        AstNode::WhileLoop { body, .. }
        | AstNode::ForLoop { body, .. }
        | AstNode::Function { body, .. } => has_ctrl(body),
        AstNode::UnaryOp { operand, .. } => has_ctrl(operand),
        AstNode::Postfix { object, items, .. } => has_ctrl(object) || items.iter().any(has_ctrl),
        AstNode::BinaryOp { left, right, .. } => has_ctrl(left) || has_ctrl(right),
        AstNode::Call { args, .. } | AstNode::CallAnonymous { args, .. } | AstNode::List(args) => {
            args.iter().any(has_ctrl)
        }
        AstNode::Dict(pairs) => pairs.iter().any(|(_, v)| has_ctrl(v)),
        AstNode::Index { object, index } => has_ctrl(object) || has_ctrl(index),
        AstNode::IndexAssign {
            object,
            index,
            value,
        } => has_ctrl(object) || has_ctrl(index) || has_ctrl(value),
        AstNode::Assignment { value, .. } => has_ctrl(value),
        _ => false,
    }
}

pub struct Compiler {
    pub instructions: Vec<Instruction>,
    builtins: Builtins,
    loop_id: usize,
    loop_stack: Vec<LoopInfo>,
    fn_depth: usize,
    locals: IndexMap<String, u16>,
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
            loop_stack: Vec::new(),
            fn_depth: 0,
            locals: IndexMap::new(),
        }
    }

    fn local_slot(&mut self, name: &str) -> u16 {
        if let Some(&i) = self.locals.get(name) {
            i
        } else {
            let idx = self.locals.len() as u16;
            self.locals.insert(name.to_string(), idx);
            idx
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.locals.contains_key(name)
    }

    pub fn local_count(&self) -> u16 {
        self.locals.len() as u16
    }

    fn emit_load(&mut self, name: &str) {
        if self.fn_depth > 0 && self.is_local(name) {
            let idx = self.locals[name];
            self.instructions.push(Instruction::LoadLocal(idx));
        } else {
            self.instructions
                .push(Instruction::LoadVar(name.to_string()));
        }
    }

    fn emit_store(&mut self, name: &str) {
        if self.fn_depth > 0 {
            let idx = self.local_slot(name);
            self.instructions.push(Instruction::StoreLocal(idx));
        } else {
            self.instructions
                .push(Instruction::StoreVar(name.to_string()));
        }
    }

    pub fn compile(&mut self, node: &AstNode) -> WqResult<()> {
        match node {
            AstNode::Postfix { .. } => unreachable!(),
            AstNode::Literal(v) => self.instructions.push(Instruction::LoadConst(v.clone())),
            AstNode::Variable(name) => self.emit_load(name),
            AstNode::Assignment { name, value } => {
                if let AstNode::Function { params, body } = &**value {
                    // Reserve slot for recursion when in a local scope
                    let slot = if self.fn_depth > 0 {
                        self.local_slot(name)
                    } else {
                        0
                    };
                    let mut c = Compiler::new();
                    c.fn_depth = self.fn_depth + 1;
                    if let Some(ps) = params {
                        for p in ps {
                            c.local_slot(p);
                        }
                    } else {
                        c.local_slot("x");
                        c.local_slot("y");
                        c.local_slot("z");
                    }
                    c.compile(body)?;
                    if self.fn_depth > 0 {
                        for instr in c.instructions.iter_mut() {
                            if let Instruction::CallUser(n, argc) = instr {
                                if n == name {
                                    *instr = Instruction::CallLocal(slot, *argc);
                                }
                            }
                        }
                    }
                    let locals = c.local_count();
                    let mut func_instructions = c.instructions;
                    func_instructions.push(Instruction::Return);
                    self.instructions
                        .push(Instruction::LoadConst(Value::BytecodeFunction {
                            params: params.clone(),
                            locals,
                            instructions: func_instructions,
                        }));
                    self.emit_store(name);
                    self.emit_load(name);
                } else {
                    self.compile(value)?;
                    self.emit_store(name);
                    self.emit_load(name);
                }
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
                } else if self.fn_depth > 0 && self.is_local(name) {
                    let slot = self.locals[name];
                    self.instructions
                        .push(Instruction::CallLocal(slot, args.len()));
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
            AstNode::Break => {
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    let pos = self.instructions.len();
                    self.instructions.push(Instruction::Jump(0));
                    loop_info.break_jumps.push(pos);
                } else {
                    return Err(WqError::SyntaxError("@b outside loop".into()));
                }
            }
            AstNode::Continue => {
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    let pos = self.instructions.len();
                    self.instructions.push(Instruction::Jump(0));
                    loop_info.continue_jumps.push(pos);
                } else {
                    return Err(WqError::SyntaxError("@c outside loop".into()));
                }
            }
            AstNode::Return(expr) => {
                if self.fn_depth == 0 {
                    return Err(WqError::SyntaxError("@r outside function".into()));
                }
                if let Some(e) = expr {
                    self.compile(e)?;
                } else {
                    self.instructions.push(Instruction::LoadConst(Value::Null));
                }
                self.instructions.push(Instruction::Return);
            }
            AstNode::Assert(expr) => {
                self.compile(expr)?;
                self.instructions.push(Instruction::Assert);
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
                    if self.fn_depth > 0 && self.is_local(name) {
                        let slot = self.local_slot(name);
                        self.compile(index)?;
                        self.compile(value)?;
                        self.instructions.push(Instruction::IndexAssignLocal(slot));
                    } else {
                        self.instructions
                            .push(Instruction::LoadConst(Value::Symbol(name.clone())));
                        self.compile(index)?;
                        self.compile(value)?;
                        self.instructions.push(Instruction::IndexAssign);
                    }
                } else {
                    return Err(WqError::DomainError(
                        "Invalid index assignment target".into(),
                    ));
                }
            }
            AstNode::Function { params, body } => {
                let mut c = Compiler::new();
                c.fn_depth = self.fn_depth + 1;
                if let Some(ps) = params {
                    for p in ps {
                        c.local_slot(p);
                    }
                } else {
                    c.local_slot("x");
                    c.local_slot("y");
                    c.local_slot("z");
                }
                c.compile(body)?;
                let locals = c.local_count();
                let mut func_instructions = c.instructions;
                func_instructions.push(Instruction::Return);
                self.instructions
                    .push(Instruction::LoadConst(Value::BytecodeFunction {
                        params: params.clone(),
                        locals,
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
                } else {
                    // when there is no false branch, the conditional
                    // expression should evaluate to null on the false path
                    self.instructions.push(Instruction::LoadConst(Value::Null));
                }
                let end = self.instructions.len();
                self.instructions[jump_end_pos] = Instruction::Jump(end);
            }
            AstNode::WhileLoop { condition, body } => {
                let start = self.instructions.len();
                self.compile(condition)?;
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.loop_stack.push(LoopInfo::default());
                self.compile(body)?;
                let continue_target = self.instructions.len();
                self.instructions.push(Instruction::Pop);
                self.instructions.push(Instruction::Jump(start));
                let end = self.instructions.len();
                self.instructions[jump_pos] = Instruction::JumpIfFalse(end);
                if let Some(info) = self.loop_stack.pop() {
                    for pos in info.break_jumps {
                        self.instructions[pos] = Instruction::Jump(end);
                    }
                    for pos in info.continue_jumps {
                        self.instructions[pos] = Instruction::Jump(continue_target);
                    }
                }
                self.instructions.push(Instruction::LoadConst(Value::Null));
            }
            AstNode::ForLoop { count, body } => {
                // Unroll constant loops only when there is no control flow in body
                if let AstNode::Literal(Value::Int(n)) = &**count {
                    if *n >= 0 && !has_ctrl(body) {
                        let limit = 16;
                        if *n <= limit {
                            if *n == 0 {
                                self.instructions.push(Instruction::LoadConst(Value::Null));
                            } else {
                                for i in 0..*n {
                                    self.instructions
                                        .push(Instruction::LoadConst(Value::Int(i)));
                                    self.emit_store("_n");
                                    self.compile(body)?;
                                    if i < *n - 1 {
                                        self.instructions.push(Instruction::Pop);
                                    }
                                }
                            }
                            return Ok(());
                        } else if *n <= 64 {
                            let full_chunks = *n / 8;
                            let remainder = *n % 8;
                            for c in 0..full_chunks {
                                for i in 0..8 {
                                    let idx = c * 8 + i;
                                    self.instructions
                                        .push(Instruction::LoadConst(Value::Int(idx)));
                                    self.emit_store("_n");
                                    self.compile(body)?;
                                    self.instructions.push(Instruction::Pop);
                                }
                            }
                            for i in 0..remainder {
                                let idx = full_chunks * 8 + i;
                                self.instructions
                                    .push(Instruction::LoadConst(Value::Int(idx)));
                                self.emit_store("_n");
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
                self.emit_store(&count_var);
                self.instructions
                    .push(Instruction::LoadConst(Value::Int(0)));
                self.emit_store("_n");
                self.instructions.push(Instruction::LoadConst(Value::Null));
                self.emit_store(&result_var);
                let start = self.instructions.len();
                self.emit_load("_n");
                self.emit_load(&count_var);
                self.instructions
                    .push(Instruction::BinaryOp(BinaryOperator::LessThan));
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.emit_load("_n");
                self.emit_store(&old_var);
                self.loop_stack.push(LoopInfo::default());
                self.compile(body)?;
                self.emit_store(&result_var);
                let continue_target = self.instructions.len();
                self.emit_load(&old_var);
                self.instructions
                    .push(Instruction::LoadConst(Value::Int(1)));
                self.instructions
                    .push(Instruction::BinaryOp(BinaryOperator::Add));
                self.emit_store("_n");
                self.instructions.push(Instruction::Jump(start));
                let end = self.instructions.len();
                self.instructions[jump_pos] = Instruction::JumpIfFalse(end);
                if let Some(info) = self.loop_stack.pop() {
                    for pos in info.break_jumps {
                        self.instructions[pos] = Instruction::Jump(end);
                    }
                    for pos in info.continue_jumps {
                        self.instructions[pos] = Instruction::Jump(continue_target);
                    }
                }
                self.emit_load(&result_var);
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
