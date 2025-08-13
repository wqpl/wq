use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{AstNode, BinaryOperator};
use crate::value::{Value, WqError, WqResult};
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
    // mapping of names defined in this function to final slot index (offset by capture_offset)
    locals: IndexMap<String, u16>,
    // number of slots reserved at the beginning of the frame for captured outer locals
        // mapping of captured names to their slot index in this function's frame (0..capture_offset-1)
    capture_map: IndexMap<String, u16>,
    // list of parent slot indices to copy into this frame in order [0..capture_offset)
    captures_parent_slots: Vec<u16>,
    // if this compiler was created for a function assigned to a name in the parent scope,
    // record that name and its parent-slot to support recursion rewrites
    parent_fn_name: Option<String>,
    parent_fn_slot: Option<u16>,
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
                        capture_map: IndexMap::new(),
            captures_parent_slots: Vec::new(),
            parent_fn_name: None,
            parent_fn_slot: None,
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

    pub fn local_count(&self) -> u16 { self.locals.len() as u16 }

    fn emit_load(&mut self, name: &str) {
        if self.fn_depth > 0 {
            if self.is_local(name) {
                let idx = self.locals[name];
                self.instructions.push(Instruction::LoadLocal(idx));
                return;
            }
            if let Some(idx) = self.capture_map.get(name) {
                self.instructions.push(Instruction::LoadCapture(*idx));
                return;
            }
        }
        self.instructions
            .push(Instruction::LoadVar(name.to_string()));
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

    fn emit_store_keep(&mut self, name: &str) {
        if self.fn_depth > 0 {
            let idx = self.local_slot(name);
            self.instructions.push(Instruction::StoreLocalKeep(idx));
        } else {
            self.instructions
                .push(Instruction::StoreVarKeep(name.to_string()));
        }
    }

    pub fn compile(&mut self, node: &AstNode) -> WqResult<()> {
        match node {
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
                    // prepare captures from current locals if inside a function
                    if self.fn_depth > 0 {
                        // collect parent locals sorted by slot index, excluding the name being defined (avoid self-capture)
                        let mut pairs: Vec<(String, u16)> = self
                            .locals
                            .iter()
                            .filter(|(k, _)| k.as_str() != name.as_str())
                            .map(|(k, &v)| (k.clone(), v))
                            .collect();
                        pairs.sort_by_key(|(_, idx)| *idx);
                        for (i, (k, _v)) in pairs.iter().enumerate() {
                            c.capture_map.insert(k.clone(), i as u16);
                        }
                        c.captures_parent_slots = pairs.into_iter().map(|(_, v)| v).collect();
                        
                        // record recursion context
                        c.parent_fn_name = Some(name.clone());
                        c.parent_fn_slot = Some(slot);
                    }
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
                    if !c.captures_parent_slots.is_empty() {
                        self.instructions.push(Instruction::LoadClosure {
                            params: params.clone(),
                            locals,
                            captures: c.captures_parent_slots.clone(),
                            instructions: func_instructions,
                        });
                    } else {
                        self.instructions
                            .push(Instruction::LoadConst(Value::CompiledFunction {
                                params: params.clone(),
                                locals,
                                instructions: func_instructions,
                            }));
                    }
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name);
                } else {
                    self.compile(value)?;
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name);
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
                if let Some(id) = self.builtins.get_id(name) {
                    for arg in args { self.compile(arg)?; }
                    self.instructions.push(Instruction::CallBuiltinId(id as u8, args.len()));
                } else if self.fn_depth > 0 && self.is_local(name) {
                    for arg in args { self.compile(arg)?; }
                    let slot = self.locals[name];
                    self.instructions.push(Instruction::CallLocal(slot, args.len()));
                } else if self.fn_depth > 0
                    && self.parent_fn_name.as_ref().is_some_and(|n| n == name)
                    && self.parent_fn_slot.is_some()
                {
                    for arg in args { self.compile(arg)?; }
                    self.instructions
                        .push(Instruction::CallLocal(self.parent_fn_slot.unwrap(), args.len()));
                } else if self.fn_depth > 0 && self.capture_map.contains_key(name) {
                    // load captured callee then args, and call anonymously
                    self.emit_load(name);
                    for arg in args { self.compile(arg)?; }
                    self.instructions.push(Instruction::CallAnon(args.len()));
                } else {
                    for arg in args { self.compile(arg)?; }
                    self.instructions.push(Instruction::CallUser(name.clone(), args.len()));
                }
            }
            AstNode::CallAnonymous { object, args } => {
                self.compile(object)?;
                for arg in args {
                    self.compile(arg)?;
                }
                self.instructions.push(Instruction::CallAnon(args.len()));
            }
            AstNode::Postfix {
                object,
                items,
                explicit_call: _,
            } => {
                let builtin_id = match object.as_ref() {
                    AstNode::Variable(name) => self.builtins.get_id(name),
                    _ => None,
                };

                if let Some(id) = builtin_id {
                    // Builtin call: don't compile the callee, only the args
                    for item in items {
                        self.compile(item)?;
                    }
                    self.instructions
                        .push(Instruction::CallBuiltinId(id as u8, items.len()));
                } else if let AstNode::Variable(vname) = object.as_ref() {
                    // If this is a recursive reference to the parent-defined name, call by slot
                    if self.fn_depth > 0
                        && self.parent_fn_name.as_ref().is_some_and(|n| n == vname)
                        && self.parent_fn_slot.is_some()
                    {
                        for item in items { self.compile(item)?; }
                        self.instructions
                            .push(Instruction::CallLocal(self.parent_fn_slot.unwrap(), items.len()));
                    } else {
                        // Non-builtin: compile the callee first, then the args
                        self.compile(object)?;
                        for item in items {
                            self.compile(item)?;
                        }
                        self.instructions
                            .push(Instruction::CallOrIndex(items.len()));
                    }
                } else {
                    // Non-builtin: compile the callee first, then the args
                    self.compile(object)?;
                    for item in items {
                        self.compile(item)?;
                    }
                    self.instructions
                        .push(Instruction::CallOrIndex(items.len()));
                }
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
            AstNode::Try(expr) => {
                let pos = self.instructions.len();
                self.instructions.push(Instruction::Try(0));
                self.compile(expr)?;
                let len = self.instructions.len() - pos - 1;
                if let Instruction::Try(ref mut l) = self.instructions[pos] {
                    *l = len;
                }
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
                if self.fn_depth > 0 {
                    let mut pairs: Vec<(String, u16)> =
                        self.locals.iter().map(|(k, &v)| (k.clone(), v)).collect();
                    pairs.sort_by_key(|(_, idx)| *idx);
                    for (i, (k, _v)) in pairs.iter().enumerate() {
                        c.capture_map.insert(k.clone(), i as u16);
                    }
                    c.captures_parent_slots = pairs.into_iter().map(|(_, v)| v).collect();
                    
                }
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
                if !c.captures_parent_slots.is_empty() {
                    self.instructions.push(Instruction::LoadClosure {
                        params: params.clone(),
                        locals,
                        captures: c.captures_parent_slots.clone(),
                        instructions: func_instructions,
                    });
                } else {
                    self.instructions
                        .push(Instruction::LoadConst(Value::CompiledFunction {
                            params: params.clone(),
                            locals,
                            instructions: func_instructions,
                        }));
                }
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

    pub fn fuse(&mut self) {
        #[derive(Default, Clone)]
        struct Stats {
            slk_pop: usize,
            svk_pop: usize,
            idx_local_pop: usize,
            idx_global_pop: usize,
            lt_jifalse: usize,
            ll0_gt_jifalse: usize,
        }

        fn fuse_once(code: &mut Vec<Instruction>, stats: &mut Stats) -> bool {
            use Instruction::*;
            use Value::CompiledFunction;
            let mut changed_any = false;

            // Recurse into nested code objects first
            for ins in code.iter_mut() {
                if let LoadConst(CompiledFunction { instructions, .. }) = ins {
                    if fuse_once(instructions, stats) {
                        changed_any = true;
                    }
                }
            }

            let old: Vec<Instruction> = code.clone();
            let n = old.len();
            if n == 0 {
                return changed_any;
            }

            let mut keep = vec![true; n];
            let mut out: Vec<Instruction> = Vec::with_capacity(n);
            let mut origin: Vec<usize> = Vec::with_capacity(n);

            let mut i = 0;
            while i < n {
                // Early: eliminate StoreKeep; Pop and IndexAssign*; Pop
                if i + 1 < n {
                    match (&old[i], &old[i + 1]) {
                        (StoreLocalKeep(slot), Pop) => {
                            out.push(StoreLocal(*slot));
                            origin.push(i);
                            keep[i] = true;
                            keep[i + 1] = false;
                            stats.slk_pop += 1;
                            changed_any = true;
                            i += 2;
                            continue;
                        }
                        (StoreVarKeep(name), Pop) => {
                            out.push(StoreVar(name.clone()));
                            origin.push(i);
                            keep[i] = true;
                            keep[i + 1] = false;
                            stats.svk_pop += 1;
                            changed_any = true;
                            i += 2;
                            continue;
                        }
                        (IndexAssignLocal(slot), Pop) => {
                            out.push(IndexAssignLocalDrop(*slot));
                            origin.push(i);
                            keep[i] = true;
                            keep[i + 1] = false;
                            stats.idx_local_pop += 1;
                            changed_any = true;
                            i += 2;
                            continue;
                        }
                        (IndexAssign, Pop) => {
                            out.push(IndexAssignDrop);
                            origin.push(i);
                            keep[i] = true;
                            keep[i + 1] = false;
                            stats.idx_global_pop += 1;
                            changed_any = true;
                            i += 2;
                            continue;
                        }
                        _ => {}
                    }
                }

                // 4-op fusion: LL j; LC 0; GreaterThan; JIFalse T -> JumpIfLEZLocal(j, T)
                if i + 3 < n {
                    if let (
                        LoadLocal(slot),
                        LoadConst(Value::Int(0)),
                        BinaryOp(BinaryOperator::GreaterThan),
                        JumpIfFalse(pos),
                    ) = (&old[i], &old[i + 1], &old[i + 2], &old[i + 3])
                    {
                        out.push(JumpIfLEZLocal(*slot, *pos));
                        origin.push(i);
                        keep[i] = true;
                        keep[i + 1] = false;
                        keep[i + 2] = false;
                        keep[i + 3] = false;
                        stats.ll0_gt_jifalse += 1;
                        changed_any = true;
                        i += 4;
                        continue;
                    }
                }

                // cmp+branch: LT; JIFalse -> JGE (stack-based)
                if i + 1 < n {
                    if let (BinaryOp(BinaryOperator::LessThan), JumpIfFalse(pos)) =
                        (&old[i], &old[i + 1])
                    {
                        out.push(JumpIfGE(*pos));
                        origin.push(i);
                        keep[i] = true;
                        keep[i + 1] = false;
                        stats.lt_jifalse += 1;
                        changed_any = true;
                        i += 2;
                        continue;
                    }
                }

                out.push(old[i].clone());
                origin.push(i);
                keep[i] = true;
                i += 1;
            }

            if changed_any {
                // Build mapping from old index -> new index of first kept instruction at or after old index
                let mut old_to_new: Vec<usize> = vec![out.len(); n + 1];
                let mut next_kept = vec![n; n + 1];
                let mut next = n;
                for idx in (0..n).rev() {
                    if keep[idx] {
                        next = idx;
                    }
                    next_kept[idx] = next;
                }
                next_kept[n] = n;
                let mut kept_to_new: Vec<isize> = vec![-1; n];
                for (new_idx, &orig) in origin.iter().enumerate() {
                    kept_to_new[orig] = new_idx as isize;
                }
                for old_idx in 0..=n {
                    let nk = next_kept[old_idx.min(n)];
                    if nk < n {
                        old_to_new[old_idx] = kept_to_new[nk] as usize;
                    } else {
                        old_to_new[old_idx] = out.len();
                    }
                }
                // Remap jump targets to new indices
                for ins in &mut out {
                    match ins {
                        Jump(pos) | JumpIfFalse(pos) | JumpIfGE(pos) => {
                            *pos = old_to_new[*pos];
                        }
                        JumpIfLEZLocal(_, pos) => {
                            *pos = old_to_new[*pos];
                        }
                        _ => {}
                    }
                }
                *code = out;
            }

            changed_any
        }

        let mut stats = Stats::default();
        loop {
            let changed = fuse_once(&mut self.instructions, &mut stats);
            if !changed {
                break;
            }
        }
    }
}
