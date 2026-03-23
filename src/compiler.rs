mod fuse;

use std::{convert::TryFrom, sync::Arc};

use crate::{
    astnode::{AstNode, BinaryOperator, UnaryOperator, UnpackItem},
    builtins::Builtins,
    value::{IntoWqValue, Value, WqResult},
    vm::instruction::{Capture, Instruction},
    wqerror::{WqError, WqErrorType},
};

use indexmap::{IndexMap, IndexSet};

pub struct Compiler {
    pub instructions: Vec<Instruction>,
    builtins: Builtins,
    loop_stack: Vec<LoopInfo>,
    value_needed: bool,
    fn_depth: usize,
    // Monotonic ID seed for generating unique internal names (e.g., N-loop temps)
    gensym: usize,
    // mapping of names defined in this function to final slot index
    locals: IndexMap<String, u16>,
    // mapping of captured names to their index in the captured vector
    capture_map: IndexMap<String, u16>,
    // names of locals known to be functions
    fn_locals: IndexSet<String>,
    // information on what to capture when creating the closure
    captures: Vec<Capture>,
    // if this compiler builds the body of a function assigned to a name
    defining_name: Option<String>,
    // Debug: stream of function-body statement spans in encounter order
    fn_spans_stream: Vec<Vec<(usize, usize)>>,
    fn_spans_idx: usize,
    // Pretty error reporting: full source text of the script being compiled
    src_text: Option<String>,
    // Pretty error reporting: current statement spans (byte offsets) and cursor
    cur_stmt_spans: Vec<(usize, usize)>,
    cur_stmt_idx: usize,
    // Use top-level stmt spans only for the first Block compiled
    top_spans_active: bool,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct LoopInfo {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

impl Compiler {
    pub fn new() -> Self {
        Self::new_with_builtins(Builtins::new())
    }

    pub fn new_with_builtins(builtins: Builtins) -> Self {
        Self {
            instructions: Vec::new(),
            builtins,
            loop_stack: Vec::new(),
            value_needed: true,
            fn_depth: 0,
            gensym: 0,
            locals: IndexMap::new(),
            capture_map: IndexMap::new(),
            fn_locals: IndexSet::new(),
            captures: Vec::new(),
            defining_name: None,
            fn_spans_stream: Vec::new(),
            fn_spans_idx: 0,
            src_text: None,
            cur_stmt_spans: Vec::new(),
            cur_stmt_idx: 0,
            top_spans_active: true,
        }
    }

    pub fn compile(&mut self, node: &AstNode) -> WqResult<()> {
        self.compile_in_context(node, true)
    }

    fn compile_in_context(&mut self, node: &AstNode, value_needed: bool) -> WqResult<()> {
        let old_value_needed = self.value_needed;
        self.value_needed = value_needed;
        let result = self.compile_node(node);
        self.value_needed = old_value_needed;
        result
    }

    fn compile_node(&mut self, node: &AstNode) -> WqResult<()> {
        match node {
            AstNode::Literal(v) => self.instructions.push(Instruction::LoadConst(v.clone())),
            AstNode::Variable(name) => self.emit_load(name),
            AstNode::Ellipsis => {
                return Err(self.syntax_err_here(
                    "'...' placeholder is only valid in unpack assignment pattern",
                ));
            }
            AstNode::Assignment { name, value } => {
                if let AstNode::Function { params, body } = &**value {
                    // Reserve slot for recursion when in a local scope
                    if self.fn_depth > 0 {
                        self.local_slot(name);
                    }
                    let mut c = Compiler::new();
                    c.fn_depth = self.fn_depth + 1;
                    c.defining_name = Some(name.clone());
                    // prepare captures from current locals if inside a function
                    if self.fn_depth > 0 {
                        // collect parent locals sorted by slot index
                        let mut pairs: Vec<(String, u16)> =
                            self.locals.iter().map(|(k, &v)| (k.clone(), v)).collect();
                        pairs.sort_by_key(|(_, idx)| *idx);
                        for (i, (k, v)) in pairs.iter().enumerate() {
                            if k == name {
                                continue;
                            }
                            c.capture_map.insert(k.clone(), i as u16);
                            // let shared = self.fn_locals.contains(k);
                            // let capture = if shared {
                            //     Capture::LocalShared(*v)
                            // } else {
                            //     Capture::Local(*v)
                            // };
                            let capture = Capture::Local(*v);
                            c.captures.push(capture);
                        }
                        // Re-expose captured names to the child,
                        // so the child can capture them from our capture vector
                        // without falling back to globals.
                        // Keep a stable order by the parent's capture index.
                        let mut parent_caps: Vec<(String, u16)> = self
                            .capture_map
                            .iter()
                            .map(|(k, &i)| (k.clone(), i))
                            .collect();
                        parent_caps.sort_by_key(|(_, i)| *i);
                        for (k, i_parent) in parent_caps {
                            if c.capture_map.contains_key(&k) {
                                continue; // local shadows captured
                            }
                            let idx = c.capture_map.len() as u16;
                            c.capture_map.insert(k.clone(), idx);
                            c.captures.push(Capture::FromCapture(i_parent));
                        }
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
                    // Prepare spans stream for nested functions: child starts at next entry
                    let spans_for_fn = self.current_fn_spans();
                    // Propagate pretty error context to child compiler
                    if let Some(src) = &self.src_text {
                        c.set_source(src.clone());
                    }
                    c.set_stmt_spans(spans_for_fn.clone());
                    c.fn_spans_stream = self.fn_spans_stream.clone();
                    c.fn_spans_idx = self.fn_spans_idx.saturating_add(1);
                    c.compile(body)?;
                    // Advance our index past what child consumed
                    self.fn_spans_idx = c.fn_spans_idx;
                    let locals = c.local_count();
                    let dbg_local_names = c.local_names_vec();
                    let mut func_instructions = c.instructions;
                    func_instructions.push(Instruction::Return);
                    let func_arc: Arc<[Instruction]> = func_instructions.into();
                    let params_arc = params.as_ref().map(|p| Arc::<[String]>::from(p.clone()));
                    let spans_arc: Arc<[(usize, usize)]> = Arc::from(spans_for_fn.clone());
                    let local_names_arc: Arc<[String]> = Arc::from(dbg_local_names.clone());
                    if !c.captures.is_empty() {
                        self.instructions.push(Instruction::LoadClosure(Box::new(
                            crate::vm::instruction::ClosurePayload {
                                params: params_arc.clone(),
                                locals,
                                captures: c.captures.clone(),
                                instructions: func_arc,
                                dbg_stmt_spans: spans_arc,
                                dbg_local_names: local_names_arc,
                            },
                        )));
                    } else {
                        self.instructions
                            .push(Instruction::LoadConst(Value::CompiledFunction(Arc::new(
                                crate::value::FunctionData {
                                    params: params_arc,
                                    locals,
                                    instructions: func_arc,
                                    dbg_chunk: None,
                                    dbg_stmt_spans: Some(spans_arc),
                                    dbg_local_names: Some(local_names_arc),
                                },
                            ))));
                    }
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name);
                    if self.fn_depth > 0 {
                        self.fn_locals.insert(name.clone());
                    }
                } else {
                    self.compile(value)?;
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name);
                }
            }
            AstNode::UnpackAssign { pattern, value } => {
                // Evaluate RHS once and keep it in a temp slot, then assign accordingly.
                let tmp_id = {
                    let v = self.gensym;
                    self.gensym = self.gensym.wrapping_add(1);
                    v
                };
                let tmp_name = format!("__unpack_{tmp_id}");
                self.compile(value)?;
                // Store and keep the full RHS on stack for expr result
                self.emit_store_keep(&tmp_name);
                // Find ellipsis position (if any)
                let ellipsis_idx = pattern
                    .iter()
                    .position(|p| matches!(p, UnpackItem::Ellipsis));
                if let Some(ei) = ellipsis_idx {
                    // Prefix: indices [0 .. ei)
                    for (i, item) in pattern.iter().enumerate().take(ei) {
                        if let UnpackItem::Bind(name) = item {
                            self.emit_load(&tmp_name);
                            self.instructions
                                .push(Instruction::LoadConst(i.into_wq_value()));
                            self.instructions.push(Instruction::Index);
                            self.emit_store(name);
                        }
                    }
                    // Suffix: align last T items with tail of RHS
                    let t = pattern.len().saturating_sub(ei + 1);
                    for (suf_idx, item) in pattern.iter().enumerate().skip(ei + 1) {
                        if let UnpackItem::Bind(name) = item {
                            // Stack sequence: obj, idx
                            // Compute idx = count(TMP) - T + (suf_idx - (ei+1))
                            let s = (suf_idx - (ei + 1)).into_wq_value();
                            // Push object first
                            self.emit_load(&tmp_name);
                            // Then compute index while keeping obj below
                            self.emit_load(&tmp_name); // arg for count
                            self.instructions
                                .push(Instruction::CallBuiltinId(Builtins::LEN, 1u16)); // -> len
                            self.instructions
                                .push(Instruction::LoadConst(t.into_wq_value()));
                            self.instructions
                                .push(Instruction::BinaryOp(BinaryOperator::Subtract));
                            self.instructions.push(Instruction::LoadConst(s));
                            self.instructions
                                .push(Instruction::BinaryOp(BinaryOperator::Add));
                            // Index and store
                            self.instructions.push(Instruction::Index);
                            self.emit_store(name);
                        }
                    }
                } else {
                    // Simple positional unpack
                    for (i, item) in pattern.iter().enumerate() {
                        if let UnpackItem::Bind(name) = item {
                            self.emit_load(&tmp_name);
                            self.instructions
                                .push(Instruction::LoadConst(i.into_wq_value()));
                            self.instructions.push(Instruction::Index);
                            self.emit_store(name);
                        }
                    }
                }
            }
            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => {
                self.compile(left)?;
                self.compile(right)?;
                self.instructions.push(Instruction::BinaryOp(*operator));
            }
            AstNode::ComparisonChain { first, rest } => {
                self.compile(first)?;
                let mut ops: Vec<BinaryOperator> = Vec::with_capacity(rest.len());
                for (op, node) in rest {
                    self.compile(node)?;
                    ops.push(*op);
                }
                self.instructions.push(Instruction::CmpChain(ops.into_boxed_slice()));
            }
            AstNode::Range {
                start,
                end,
                step,
                inclusive,
            } => {
                self.compile(start)?;
                self.compile(end)?;
                if let Some(step_expr) = step {
                    self.compile(step_expr)?;
                    self.instructions.push(Instruction::MakeRange {
                        inclusive: *inclusive,
                        has_step: true,
                    });
                } else {
                    self.instructions.push(Instruction::MakeRange {
                        inclusive: *inclusive,
                        has_step: false,
                    });
                }
            }
            AstNode::UnaryOp { operator, operand } => {
                self.compile(operand)?;
                self.instructions.push(Instruction::UnaryOp(*operator));
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
                    for arg in args {
                        self.compile(arg)?;
                    }
                    self.instructions.push(Instruction::CallBuiltinId(
                        id.try_into().expect("builtin id overflow"),
                        args.len().try_into().expect("argc overflow"),
                    ));
                } else if self.fn_depth > 0 {
                    // Inside a function
                    //  locals => CallLocal
                    //  everything else => emit_load => LoadSelf/LoadCapture/LoadVar
                    if self.is_local(name) && self.fn_locals.contains(name) {
                        for arg in args {
                            self.compile(arg)?;
                        }
                        let slot = self.locals[name];
                        self.instructions
                            .push(Instruction::CallLocal(slot, args.len()));
                    } else {
                        self.emit_load(name);
                        for arg in args {
                            self.compile(arg)?;
                        }
                        self.instructions.push(Instruction::CallOrIndex(args.len()));
                    }
                } else {
                    for arg in args {
                        self.compile(arg)?;
                    }
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
                    self.instructions.push(Instruction::CallBuiltinId(
                        u16::try_from(id).expect("builtin id overflow"),
                        u16::try_from(items.len()).expect("argc overflow"),
                    ));
                } else {
                    // Non-builtin: compile the callee first, then the args
                    let mut optimized = false;
                    if let AstNode::Variable(name) = object.as_ref() {
                        if self.is_local(name) {
                            let slot = self.locals[name];
                            for item in items {
                                self.compile(item)?;
                            }
                            self.instructions
                                .push(Instruction::CallOrIndexLocal(slot, items.len()));
                            optimized = true;
                        } else if self.fn_depth == 0 {
                            for item in items {
                                self.compile(item)?;
                            }
                            self.instructions
                                .push(Instruction::CallOrIndexVar(name.clone(), items.len()));
                            optimized = true;
                        }
                    }
                    if !optimized {
                        self.compile(object)?;
                        for item in items {
                            self.compile(item)?;
                        }
                        self.instructions
                            .push(Instruction::CallOrIndex(items.len()));
                    }
                }
            }
            AstNode::Break => {
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    let pos = self.instructions.len();
                    self.instructions.push(Instruction::Jump(0));
                    loop_info.break_jumps.push(pos);
                } else {
                    return Err(self.syntax_err_here("@b outside loop"));
                }
            }
            AstNode::Continue => {
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    let pos = self.instructions.len();
                    self.instructions.push(Instruction::Jump(0));
                    loop_info.continue_jumps.push(pos);
                } else {
                    return Err(self.syntax_err_here("@c outside loop"));
                }
            }
            AstNode::Return(expr) => {
                if self.fn_depth == 0 {
                    return Err(self.syntax_err_here("@r outside function"));
                }
                if let Some(e) = expr {
                    self.compile(e)?;
                } else {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                }
                self.instructions.push(Instruction::Return);
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
                let mut optimized = false;
                if let AstNode::Variable(name) = &**object {
                    if self.is_local(name) {
                        let slot = self.locals[name];
                        self.compile(index)?;
                        self.instructions.push(Instruction::IndexLoadLocal(slot));
                        optimized = true;
                    } else if self.fn_depth == 0 {
                        self.compile(index)?;
                        self.instructions
                            .push(Instruction::IndexLoadVar(name.clone()));
                        optimized = true;
                    }
                }
                if !optimized {
                    self.compile(object)?;
                    self.compile(index)?;
                    self.instructions.push(Instruction::Index);
                }
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
                        self.compile(index)?;
                        self.compile(value)?;
                        self.instructions.push(Instruction::IndexAssignVar(name.clone()));
                    }
                } else {
                    return Err(self.syntax_err_here("Invalid index assignment target"));
                }
            }
            AstNode::Function { params, body } => {
                let mut c = Compiler::new();
                c.fn_depth = self.fn_depth + 1;
                if self.fn_depth > 0 {
                    let mut pairs: Vec<(String, u16)> =
                        self.locals.iter().map(|(k, &v)| (k.clone(), v)).collect();
                    pairs.sort_by_key(|(_, idx)| *idx);
                    for (i, (k, v)) in pairs.iter().enumerate() {
                        if self.defining_name.as_ref().is_some_and(|n| n == k) {
                            continue;
                        }
                        c.capture_map.insert(k.clone(), i as u16);
                        // let capture = if self.fn_locals.contains(k) {
                        //     Capture::LocalShared(*v)
                        // } else {
                        //     Capture::Local(*v)
                        // };
                        let capture = Capture::Local(*v);
                        c.captures.push(capture);
                    }
                    // Re-expose captured names to the child
                    let mut parent_caps: Vec<(String, u16)> = self
                        .capture_map
                        .iter()
                        .map(|(k, &i)| (k.clone(), i))
                        .collect();
                    parent_caps.sort_by_key(|(_, i)| *i);
                    for (k, i_parent) in parent_caps {
                        if c.capture_map.contains_key(&k) {
                            continue;
                        }
                        let idx = c.capture_map.len() as u16;
                        c.capture_map.insert(k.clone(), idx);
                        c.captures.push(Capture::FromCapture(i_parent));
                    }
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
                // Prepare spans stream for nested functions: child starts at next entry
                let spans_for_fn = self.current_fn_spans();
                // Propagate pretty error context to child compiler
                if let Some(src) = &self.src_text {
                    c.set_source(src.clone());
                }
                c.set_stmt_spans(spans_for_fn.clone());
                c.fn_spans_stream = self.fn_spans_stream.clone();
                c.fn_spans_idx = self.fn_spans_idx.saturating_add(1);
                c.compile(body)?;
                self.fn_spans_idx = c.fn_spans_idx;
                let locals = c.local_count();
                let dbg_local_names = c.local_names_vec();
                let mut func_instructions = c.instructions;
                func_instructions.push(Instruction::Return);
                let func_arc: Arc<[Instruction]> = func_instructions.into();
                let params_arc = params.as_ref().map(|p| Arc::<[String]>::from(p.clone()));
                let spans_arc: Arc<[(usize, usize)]> = Arc::from(spans_for_fn.clone());
                let local_names_arc: Arc<[String]> = Arc::from(dbg_local_names.clone());
                if !c.captures.is_empty() {
                    self.instructions.push(Instruction::LoadClosure(Box::new(
                        crate::vm::instruction::ClosurePayload {
                            params: params_arc.clone(),
                            locals,
                            captures: c.captures.clone(),
                            instructions: func_arc,
                            dbg_stmt_spans: spans_arc,
                            dbg_local_names: local_names_arc,
                        },
                    )));
                } else {
                    self.instructions
                        .push(Instruction::LoadConst(Value::CompiledFunction(Arc::new(
                            crate::value::FunctionData {
                                params: params_arc,
                                locals,
                                instructions: func_arc,
                                dbg_chunk: None,
                                dbg_stmt_spans: Some(spans_arc),
                                dbg_local_names: Some(local_names_arc),
                            },
                        ))));
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
                self.compile_in_context(true_branch, self.value_needed)?;
                let jump_end_pos = self.instructions.len();
                self.instructions.push(Instruction::Jump(0));
                // patch jump_if_false to here
                let else_start = self.instructions.len();
                self.instructions[jump_if_false_pos] = Instruction::JumpIfFalse(else_start);
                if let Some(fb) = false_branch {
                    self.compile_in_context(fb, self.value_needed)?;
                } else {
                    // when there is no false branch, the conditional
                    // expression should evaluate to null on the false path
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                }
                let end = self.instructions.len();
                self.instructions[jump_end_pos] = Instruction::Jump(end);
            }
            AstNode::WLoop { condition, body } => {
                if self.value_needed
                    && pure_const_body(body)
                    && let Some(value) = const_body_value(body)
                {
                    let start = self.instructions.len();
                    self.compile(condition)?;
                    let jump_pos = self.instructions.len();
                    self.instructions.push(Instruction::JumpIfFalse(0));
                    self.instructions.push(Instruction::Jump(start));
                    let end = self.instructions.len();
                    self.instructions[jump_pos] = Instruction::JumpIfFalse(end);
                    self.instructions.push(Instruction::LoadConst(value));
                    return Ok(());
                }
                let result_var = if self.value_needed {
                    let id = {
                        let v = self.gensym;
                        self.gensym = self.gensym.wrapping_add(1);
                        v
                    };
                    let result_var = format!("--vm-w-loop-res-{id}");
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                    self.emit_store(&result_var);
                    Some(result_var)
                } else {
                    None
                };
                let start = self.instructions.len();
                self.compile(condition)?;
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.loop_stack.push(LoopInfo::default());
                self.compile_in_context(body, self.value_needed)?;
                if let Some(result_var) = &result_var {
                    self.emit_store(result_var);
                } else {
                    self.instructions.push(Instruction::Pop);
                }
                let continue_target = self.instructions.len();
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
                if let Some(result_var) = &result_var {
                    self.emit_load(result_var);
                } else {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                }
            }
            AstNode::NLoop { count, body } => {
                if self.value_needed
                    && pure_const_body(body)
                    && let Some(value) = const_body_value(body)
                {
                    self.compile(count)?;
                    self.instructions.push(Instruction::Pop);
                    self.instructions.push(Instruction::LoadConst(value));
                    return Ok(());
                }
                // Unroll constant loops only when there is no control flow in body
                if let AstNode::Literal(Value::Int(n)) = &**count
                    && *n >= 0
                    && !has_ctrl(body)
                {
                    let limit = 16;
                    if *n <= limit {
                        if *n == 0 {
                            self.instructions
                                .push(Instruction::LoadConst(Value::unit()));
                        } else {
                            for i in 0..*n {
                                self.instructions
                                    .push(Instruction::LoadConst(Value::Int(i)));
                                self.emit_store("_n");
                                self.compile_in_context(body, self.value_needed)?;
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
                                self.compile_in_context(body, self.value_needed)?;
                                self.instructions.push(Instruction::Pop);
                            }
                        }
                        for i in 0..remainder {
                            let idx = full_chunks * 8 + i;
                            self.instructions
                                .push(Instruction::LoadConst(Value::Int(idx)));
                            self.emit_store("_n");
                            self.compile_in_context(body, self.value_needed)?;
                            if i < remainder - 1 {
                                self.instructions.push(Instruction::Pop);
                            }
                        }
                        if *n > 0 {
                            self.instructions.pop();
                        } else {
                            self.instructions
                                .push(Instruction::LoadConst(Value::unit()));
                        }
                        return Ok(());
                    }
                }
                let id = {
                    let v = self.gensym;
                    self.gensym = self.gensym.wrapping_add(1);
                    v
                };
                let count_var = format!("--vm-n-loop-count-{id}");
                let result_var = self.value_needed.then(|| format!("--vm-n-loop-res-{id}"));
                let old_var = format!("--vm-n-loop-old-{id}");
                self.compile(count)?; // -> count on stack
                self.emit_store(&count_var);
                self.instructions
                    .push(Instruction::LoadConst(Value::Int(0)));
                self.emit_store("_n");
                if let Some(result_var) = &result_var {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                    self.emit_store(result_var);
                }
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
                self.compile_in_context(body, self.value_needed)?;
                if let Some(result_var) = &result_var {
                    self.emit_store(result_var);
                } else {
                    self.instructions.push(Instruction::Pop);
                }
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
                if let Some(result_var) = &result_var {
                    self.emit_load(result_var);
                } else {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                }
            }
            AstNode::FLoop { iterable, body } => {
                if self.value_needed
                    && pure_const_body(body)
                    && let Some(value) = const_body_value(body)
                {
                    self.compile(iterable)?;
                    self.instructions.push(Instruction::Pop);
                    self.instructions.push(Instruction::LoadConst(value));
                    return Ok(());
                }
                let id = {
                    let v = self.gensym;
                    self.gensym = self.gensym.wrapping_add(1);
                    v
                };
                let iter_var = format!("--vm-f-loop-iter-{id}");
                let count_var = format!("--vm-f-loop-count-{id}");
                let result_var = self.value_needed.then(|| format!("--vm-f-loop-res-{id}"));
                let old_var = format!("--vm-f-loop-old-{id}");
                self.compile(iterable)?;
                self.emit_store(&iter_var);
                self.emit_load(&iter_var);
                self.instructions
                    .push(Instruction::UnaryOp(UnaryOperator::Count));
                self.emit_store(&count_var);
                self.instructions
                    .push(Instruction::LoadConst(Value::Int(0)));
                self.emit_store("_n");
                if let Some(result_var) = &result_var {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                    self.emit_store(result_var);
                }
                let start = self.instructions.len();
                self.emit_load("_n");
                self.emit_load(&count_var);
                self.instructions
                    .push(Instruction::BinaryOp(BinaryOperator::LessThan));
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.emit_load("_n");
                self.emit_store(&old_var);
                self.emit_load(&iter_var);
                self.emit_load("_n");
                self.instructions.push(Instruction::Index);
                self.emit_store("_f");
                self.loop_stack.push(LoopInfo::default());
                self.compile_in_context(body, self.value_needed)?;
                if let Some(result_var) = &result_var {
                    self.emit_store(result_var);
                } else {
                    self.instructions.push(Instruction::Pop);
                }
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
                if let Some(result_var) = &result_var {
                    self.emit_load(result_var);
                } else {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                }
            }
            AstNode::Block(stmts) => {
                if stmts.is_empty() {
                    // Empty blocks evaluate to null
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                } else {
                    if self.top_spans_active {
                        for (i, stmt) in
                            stmts.iter().enumerate().take(stmts.len().saturating_sub(1))
                        {
                            if i < self.cur_stmt_spans.len() {
                                self.cur_stmt_idx = i;
                            }
                            self.compile_in_context(stmt, false)?;
                            self.instructions.push(Instruction::Pop);
                        }
                        self.top_spans_active = false;
                    } else {
                        for stmt in stmts.iter().take(stmts.len().saturating_sub(1)) {
                            self.compile_in_context(stmt, false)?;
                            self.instructions.push(Instruction::Pop);
                        }
                    }
                    if let Some(last) = stmts.last() {
                        self.compile_in_context(last, self.value_needed)?;
                    } else {
                        self.instructions
                            .push(Instruction::LoadConst(Value::unit()));
                    }
                }
            }
            AstNode::BlockExpr(stmts) => {
                if stmts.is_empty() {
                    self.instructions
                        .push(Instruction::LoadConst(Value::unit()));
                } else {
                    if self.top_spans_active {
                        for (i, stmt) in
                            stmts.iter().enumerate().take(stmts.len().saturating_sub(1))
                        {
                            if i < self.cur_stmt_spans.len() {
                                self.cur_stmt_idx = i;
                            }
                            self.compile_in_context(stmt, false)?;
                            self.instructions.push(Instruction::Pop);
                        }
                        self.top_spans_active = false;
                    } else {
                        for stmt in stmts.iter().take(stmts.len().saturating_sub(1)) {
                            self.compile_in_context(stmt, false)?;
                            self.instructions.push(Instruction::Pop);
                        }
                    }
                    self.compile_in_context(
                        stmts.last().expect("non-empty block expr"),
                        self.value_needed,
                    )?;
                }
            }
        }
        Ok(())
    }

    pub fn set_fn_spans(&mut self, spans: Vec<Vec<(usize, usize)>>) {
        self.fn_spans_stream = spans;
        self.fn_spans_idx = 0;
    }

    fn current_fn_spans(&self) -> Vec<(usize, usize)> {
        if self.fn_spans_idx < self.fn_spans_stream.len() {
            self.fn_spans_stream[self.fn_spans_idx].clone()
        } else {
            Vec::new()
        }
    }

    // Pretty error reporting API
    pub fn set_source(&mut self, src: String) {
        self.src_text = Some(src);
    }

    pub fn set_stmt_spans(&mut self, spans: Vec<(usize, usize)>) {
        self.cur_stmt_spans = spans;
        self.cur_stmt_idx = 0;
    }

    fn syntax_err_here(&self, msg: impl Into<String>) -> WqError {
        let msg = msg.into();
        let e = WqError::new(WqErrorType::Syntax).src("compiler").msg(msg);

        if let (Some(src), Some((byte_start, byte_end))) = (
            self.src_text.as_ref(),
            self.cur_stmt_spans.get(self.cur_stmt_idx).cloned(),
        ) {
            let (line, column) = byte_to_line_col(src, byte_start);
            let src_line = src.lines().nth(line.saturating_sub(1)).unwrap_or("");
            let width = if byte_end > byte_start && byte_end <= src.len() && byte_start <= src.len()
            {
                src[byte_start..byte_end].chars().count().max(1)
            } else {
                1
            };
            let pointer = " ".repeat(column.saturating_sub(1)) + &"~".repeat(width);
            e.attach_note(format!("at {line}:{column}\n{src_line}\n{pointer}",))
        } else {
            e
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

    fn local_names_vec(&self) -> Vec<String> {
        let mut names = vec![String::new(); self.local_count() as usize];
        for (name, &idx) in self.locals.iter() {
            if (idx as usize) < names.len() {
                names[idx as usize] = name.clone();
            }
        }
        names
    }

    fn emit_load(&mut self, name: &str) {
        if self.fn_depth > 0 {
            if self.is_local(name) {
                let idx = self.locals[name];
                self.instructions.push(Instruction::LoadLocal(idx));
                return;
            }
            if self.defining_name.as_ref().is_some_and(|n| n == name) {
                self.instructions.push(Instruction::LoadSelf);
                return;
            }
            if let Some(idx) = self.capture_map.get(name) {
                self.instructions.push(Instruction::LoadCapture(*idx));
                return;
            }
            // If the name refers to a builtin function, do not capture it.
            // Emit a global load so it resolves via builtin lookup at runtime.
            if self.builtins.has_function(name) {
                self.instructions
                    .push(Instruction::LoadVar(name.to_string()));
                return;
            }
            // capture globals by value
            let idx = self.capture_map.len() as u16;
            self.capture_map.insert(name.to_string(), idx);
            self.captures.push(Capture::Global(name.to_string()));
            self.instructions.push(Instruction::LoadCapture(idx));
            return;
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
}

fn has_ctrl(node: &AstNode) -> bool {
    match node {
        AstNode::Break | AstNode::Continue | AstNode::Return(_) => true,
        AstNode::Block(stmts) => stmts.iter().any(has_ctrl),
        AstNode::BlockExpr(stmts) => stmts.iter().any(has_ctrl),
        AstNode::Conditional {
            true_branch,
            false_branch,
            ..
        } => has_ctrl(true_branch) || false_branch.as_ref().is_some_and(|b| has_ctrl(b)),
        AstNode::WLoop { body, .. }
        | AstNode::NLoop { body, .. }
        | AstNode::FLoop { body, .. }
        | AstNode::Function { body, .. } => has_ctrl(body),
        AstNode::UnaryOp { operand, .. } => has_ctrl(operand),
        AstNode::Postfix { object, items, .. } => has_ctrl(object) || items.iter().any(has_ctrl),
        AstNode::BinaryOp { left, right, .. } => has_ctrl(left) || has_ctrl(right),
        AstNode::ComparisonChain { first, rest } => {
            has_ctrl(first) || rest.iter().any(|(_, node)| has_ctrl(node))
        }
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
        AstNode::UnpackAssign { value, .. } => has_ctrl(value),
        AstNode::Range {
            start, end, step, ..
        } => has_ctrl(start) || has_ctrl(end) || step.as_ref().is_some_and(|s| has_ctrl(s)),
        _ => false,
    }
}

fn const_body_value(node: &AstNode) -> Option<Value> {
    match node {
        AstNode::Literal(value) => Some(value.clone()),
        AstNode::Block(stmts) | AstNode::BlockExpr(stmts) => {
            stmts.last().and_then(const_body_value)
        }
        _ => None,
    }
}

fn pure_const_body(node: &AstNode) -> bool {
    if has_ctrl(node) {
        return false;
    }

    match node {
        AstNode::Literal(_) => true,
        AstNode::Block(stmts) | AstNode::BlockExpr(stmts) => stmts.iter().all(pure_const_body),
        _ => false,
    }
}

// Convert a byte offset into (1-based) line and column within `src`.
fn byte_to_line_col(src: &str, byte_pos: usize) -> (usize, usize) {
    let b = byte_pos.min(src.len());
    let prefix = &src[..b];
    let line = prefix.chars().filter(|&c| c == '\n').count() + 1;
    let last_nl = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let col = src[last_nl..b].chars().count() + 1;
    (line, col)
}

// fn int_overflow_err(e: TryFromIntError) -> WqError {
//     WqError::IntOverflow(WqErrCtx {
//         msg: e.to_string().into(),
//         hint: None,
//         source: Some(Cow::Borrowed("compiler")),
//     })
// }
