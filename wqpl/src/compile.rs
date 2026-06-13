mod cp;
mod fuse;
mod tce;

use std::collections::BTreeSet;
use std::convert::TryFrom;
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

use crate::astnode::{AstNode, AstSpan, BinaryOperator, Parameter};
use crate::builtins::{BuiltinDepthSugar, Builtins};
use crate::value::func::FunctionData;
use crate::value::{Value, WqResult};
use crate::vm::inst::{Capture, DebugStmtMark, Instruction, MutationOp, Operand, StoreTarget};
use crate::wqerror::{WqError, WqErrorType};

#[derive(Clone, Copy)]
enum MethodDispatchKind {
    Postfix,
    Call,
}

pub(crate) struct Compiler {
    pub(crate) instructions: Vec<Instruction>,
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
    // mapping of outer bindings captured explicitly by reference
    ref_capture_map: IndexMap<String, u16>,
    // bare names in a `'{...}` function that should behave like explicit `'name`
    ref_default_names: IndexSet<String>,
    // names of locals known to be functions
    fn_locals: IndexSet<String>,
    // information on what to capture when creating the closure
    captures: Vec<Capture>,
    backward_jump_targets: BTreeSet<usize>,
    // if this compiler builds the body of a function assigned to a name
    defining_name: Option<String>,
    // Debug: stream of function-body statement spans in encounter order
    fn_spans_stream: Vec<Vec<(usize, usize)>>,
    fn_spans_idx: usize,
    // Pretty error reporting: full source text of the script being compiled
    src_text: Option<String>,
    // Pretty error reporting: source file path / label
    src_path: Option<String>,
    // Pretty error reporting: current statement spans (byte offsets) and cursor
    cur_stmt_spans: Vec<(usize, usize)>,
    cur_stmt_idx: usize,
    current_stmt_span: Option<(usize, usize)>,
    pub(crate) dbg_pc_spans: Vec<Option<(usize, usize)>>,
    pub(crate) dbg_stmt_marks: Vec<DebugStmtMark>,
    pub(crate) has_runtime_debug: bool,
    trace_symbol_operands: bool,
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

impl Compiler {
    pub(crate) fn new() -> Self {
        Self::new_with_builtins(Builtins::new())
    }

    pub(crate) fn new_with_builtins(builtins: Builtins) -> Self {
        Self {
            instructions: Vec::new(),
            builtins,
            loop_stack: Vec::new(),
            value_needed: true,
            fn_depth: 0,
            gensym: 0,
            locals: IndexMap::new(),
            capture_map: IndexMap::new(),
            ref_capture_map: IndexMap::new(),
            ref_default_names: IndexSet::new(),
            fn_locals: IndexSet::new(),
            captures: Vec::new(),
            backward_jump_targets: BTreeSet::new(),
            defining_name: None,
            fn_spans_stream: Vec::new(),
            fn_spans_idx: 0,
            src_text: None,
            src_path: None,
            cur_stmt_spans: Vec::new(),
            cur_stmt_idx: 0,
            current_stmt_span: None,
            dbg_pc_spans: Vec::new(),
            dbg_stmt_marks: Vec::new(),
            has_runtime_debug: false,
            trace_symbol_operands: false,
        }
    }

    pub(crate) fn compile(&mut self, node: &AstNode) -> WqResult<()> {
        self.compile_stmt_sequence(node, true)
    }

    fn compile_expr(&mut self, node: &AstNode) -> WqResult<()> {
        let start = self.instructions.len();
        let result = self.compile_in_context(node, true);
        let end = self.instructions.len();
        self.fill_span_range(start, end, Self::ast_node_span(node));
        result
    }

    /// Compile call arguments to the stack.  If any are `NamedArg` nodes,
    /// collect the metadata and emit `SetupNamedCall` for the VM.
    fn compile_call_args(&mut self, args: &[AstNode]) -> WqResult<()> {
        let named_args: Vec<(u16, Arc<str>)> = args
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                if let AstNode::NamedArg { name, .. } = a {
                    Some((i as u16, Arc::from(name.as_str())))
                } else {
                    None
                }
            })
            .collect();

        let pos_count = (args.len() - named_args.len()) as u16;

        // Evaluate all arguments left-to-right, preserving source order
        for arg in args {
            match arg {
                AstNode::NamedArg { value, .. } => self.compile_expr(value)?,
                other => self.compile_expr(other)?,
            }
        }

        if !named_args.is_empty() {
            self.instructions
                .push(Instruction::PrepareNamedArgs(Box::new(
                    crate::vm::inst::NamedArgMeta {
                        pos_count,
                        named: named_args.into_boxed_slice(),
                    },
                )));
        }

        Ok(())
    }

    fn method_lookup_target(object: &AstNode) -> Option<(&AstNode, Arc<str>)> {
        match object {
            AstNode::Postfix {
                object,
                items,
                depth: None,
                ..
            } => {
                let [AstNode::Literal(Value::Tag(name), _)] = items.as_slice() else {
                    return None;
                };
                Some((object.as_ref(), Arc::clone(name)))
            }
            AstNode::Index { object, index, .. } => {
                let AstNode::Literal(Value::Tag(name), _) = index.as_ref() else {
                    return None;
                };
                Some((object.as_ref(), Arc::clone(name)))
            }
            _ => None,
        }
    }

    fn compile_method_dispatch(
        &mut self,
        receiver: &AstNode,
        method: Arc<str>,
        args: &[AstNode],
        kind: MethodDispatchKind,
    ) -> WqResult<bool> {
        let AstNode::Variable(name, _) = receiver else {
            return Ok(false);
        };

        if self.fn_depth > 0 {
            if self.is_local(name) {
                let slot = self.locals[name];
                self.compile_call_args(args)?;
                self.instructions
                    .push(Self::method_local_inst(kind, slot, method, args.len()));
                return Ok(true);
            }
            if self.is_ref_default_name(name) {
                self.compile_call_args(args)?;
                if let Some(idx) = self.ref_capture_map.get(name).copied() {
                    self.instructions.push(Self::method_capture_inst(
                        kind,
                        idx,
                        method,
                        args.len(),
                    ));
                } else {
                    self.instructions.push(Self::method_var_inst(
                        kind,
                        name.clone().into(),
                        method,
                        args.len(),
                    ));
                }
                return Ok(true);
            }
            if let Some(idx) = self.capture_map.get(name).copied() {
                self.compile_call_args(args)?;
                self.instructions
                    .push(Self::method_capture_inst(kind, idx, method, args.len()));
                return Ok(true);
            }
            return Ok(false);
        }

        self.compile_call_args(args)?;
        self.instructions.push(Self::method_var_inst(
            kind,
            name.clone().into(),
            method,
            args.len(),
        ));
        Ok(true)
    }

    fn method_local_inst(
        kind: MethodDispatchKind,
        slot: u16,
        method: Arc<str>,
        argc: usize,
    ) -> Instruction {
        match kind {
            MethodDispatchKind::Postfix => Instruction::PostfixMethodLocal(slot, method, argc),
            MethodDispatchKind::Call => Instruction::CallMethodLocal(slot, method, argc),
        }
    }

    fn method_capture_inst(
        kind: MethodDispatchKind,
        slot: u16,
        method: Arc<str>,
        argc: usize,
    ) -> Instruction {
        match kind {
            MethodDispatchKind::Postfix => Instruction::PostfixMethodCapture(slot, method, argc),
            MethodDispatchKind::Call => Instruction::CallMethodCapture(slot, method, argc),
        }
    }

    fn method_var_inst(
        kind: MethodDispatchKind,
        receiver: Arc<str>,
        method: Arc<str>,
        argc: usize,
    ) -> Instruction {
        match kind {
            MethodDispatchKind::Postfix => Instruction::PostfixMethodVar(receiver, method, argc),
            MethodDispatchKind::Call => Instruction::CallMethodVar(receiver, method, argc),
        }
    }

    fn expand_depth_sugar_args(
        &self,
        name: &str,
        sugar: BuiltinDepthSugar,
        mut args: Vec<AstNode>,
        depth: i64,
        span: AstSpan,
    ) -> WqResult<Vec<AstNode>> {
        let depth_arg = || AstNode::Literal(Value::Int(depth), None);
        match sugar {
            BuiltinDepthSugar::Append { non_depth_argc } => {
                let expected = usize::from(non_depth_argc);
                if args.len() != expected {
                    return Err(self.syntax_err_at(
                        span,
                        format!("{name}@{depth} expects {expected} non-depth arguments"),
                    ));
                }
                args.push(depth_arg());
                Ok(args)
            }
            BuiltinDepthSugar::AppendDefaultInt {
                required_argc,
                optional_argc,
                default,
            } => {
                let required = usize::from(required_argc);
                let optional = usize::from(optional_argc);
                match args.len() {
                    n if n == required => {
                        args.push(AstNode::Literal(Value::Int(default), None));
                        args.push(depth_arg());
                        Ok(args)
                    }
                    n if n == optional => {
                        args.push(depth_arg());
                        Ok(args)
                    }
                    _ => Err(self.syntax_err_at(
                        span,
                        format!(
                            "{name}@{depth} expects {required} or {optional} non-depth arguments"
                        ),
                    )),
                }
            }
            BuiltinDepthSugar::None => Err(self.syntax_err_at(
                span,
                format!("depth modifier can only be used on depth-aware builtins, got '{name}'"),
            )),
        }
    }

    /// Allocate local slots for function parameters (including the hidden
    /// `--named-mask` slot when named parameters are present) and emit the
    /// prologue that evaluates default values for omitted named parameters.
    ///
    /// Returns the span of the parameter list so the caller can associate
    /// prologue instructions with the definition site (rather than the first
    /// body expression) for arity-error reporting.
    fn emit_params_and_prologue(
        &mut self,
        params: &Option<Vec<Parameter>>,
    ) -> WqResult<Option<(usize, usize)>> {
        let mut named_prologue: Vec<(u16, u8, Box<AstNode>)> = Vec::new();
        let mut param_list_span: Option<(usize, usize)> = None;
        if let Some(ps) = params {
            let mut named_idx = 0u8;
            for p in ps {
                let slot = self.local_slot(p.name());
                if let Parameter::Named {
                    default: Some(default_expr),
                    ..
                } = p
                {
                    named_prologue.push((slot, named_idx, default_expr.clone()));
                }
                if matches!(p, Parameter::Named { .. }) {
                    named_idx += 1;
                }
                // Union all parameter spans into a param-list span.
                if let Some(pspan) = p.span() {
                    param_list_span = Some(match param_list_span {
                        Some((s, e)) => (s.min(pspan.0), e.max(pspan.1)),
                        None => pspan,
                    });
                }
            }
        } else {
            self.local_slot("x");
            self.local_slot("y");
            self.local_slot("z");
        }

        let mask_slot = if params
            .as_ref()
            .is_some_and(|ps| ps.iter().any(|p| matches!(p, Parameter::Named { .. })))
        {
            Some(self.local_slot("--named-mask"))
        } else {
            None
        };

        if let Some(mask_slot) = mask_slot {
            for (slot, bit_idx, default_expr) in &named_prologue {
                self.instructions.push(Instruction::LoadLocal(mask_slot));
                self.instructions
                    .push(Instruction::LoadNamedArgsProvided(*bit_idx));
                let jmp_false_idx = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                let jmp_skip_idx = self.instructions.len();
                self.instructions.push(Instruction::Jump(0));
                let eval_start = self.instructions.len();
                self.compile_expr(default_expr)?;
                self.instructions.push(Instruction::StoreLocal(*slot));
                self.instructions.push(Instruction::Pop);
                let end = self.instructions.len();
                if let Instruction::JumpIfFalse(ref mut target) = self.instructions[jmp_false_idx] {
                    *target = eval_start;
                }
                if let Instruction::Jump(ref mut target) = self.instructions[jmp_skip_idx] {
                    *target = end;
                }
            }
        }
        // Ensure PC 0 maps to the param list so arity errors point to
        // the definition site rather than the first body expression.
        if let Some(span) = param_list_span {
            if self.dbg_pc_spans.is_empty() {
                self.dbg_pc_spans.resize(1, None);
            }
            self.dbg_pc_spans[0] = Some(span);
        }

        Ok(param_list_span)
    }

    fn compile_in_context(&mut self, node: &AstNode, value_needed: bool) -> WqResult<()> {
        let old_value_needed = self.value_needed;
        self.value_needed = value_needed;
        let result = self.compile_node(node);
        self.value_needed = old_value_needed;
        result
    }

    fn push_inst(&mut self, inst: Instruction) {
        self.instructions.push(inst);
        self.dbg_pc_spans.push(None);
    }

    fn stmt_span_count(node: &AstNode) -> usize {
        match node {
            AstNode::Block(stmts) => stmts.iter().map(Self::stmt_span_count).sum(),
            AstNode::BlockExpr(stmts, _) => {
                1 + stmts.iter().map(Self::stmt_span_count).sum::<usize>()
            }
            AstNode::Conditional {
                true_branch,
                false_branch,
                ..
            } => {
                1 + Self::stmt_span_count(true_branch)
                    + false_branch.as_deref().map_or(0, Self::stmt_span_count)
            }
            AstNode::ConditionalDot { true_branch, .. } => 1 + Self::stmt_span_count(true_branch),
            AstNode::WLoop { body, .. } | AstNode::NLoop { body, .. } => {
                1 + Self::stmt_span_count(body)
            }
            _ => 1,
        }
    }

    fn take_stmt_spans(&mut self, count: usize) -> Vec<(usize, usize)> {
        if count == 0 || self.cur_stmt_idx >= self.cur_stmt_spans.len() {
            return Vec::new();
        }
        let end = (self.cur_stmt_idx + count).min(self.cur_stmt_spans.len());
        let out = self.cur_stmt_spans[self.cur_stmt_idx..end].to_vec();
        self.cur_stmt_idx = end;
        out
    }

    fn take_stmt_spans_for(&mut self, node: &AstNode) -> Vec<(usize, usize)> {
        self.take_stmt_spans(Self::stmt_span_count(node))
    }

    fn with_stmt_spans<T>(
        &mut self,
        spans: &[(usize, usize)],
        f: impl FnOnce(&mut Self) -> WqResult<T>,
    ) -> WqResult<T> {
        let saved_spans = std::mem::replace(&mut self.cur_stmt_spans, spans.to_vec());
        let saved_idx = self.cur_stmt_idx;
        let saved_current = self.current_stmt_span;
        self.cur_stmt_idx = 0;
        self.current_stmt_span = None;
        let result = f(self);
        self.cur_stmt_spans = saved_spans;
        self.cur_stmt_idx = saved_idx;
        self.current_stmt_span = saved_current;
        result
    }

    fn fill_span_range(&mut self, start_pc: usize, end_pc: usize, span: Option<(usize, usize)>) {
        let Some(span) = span else {
            return;
        };
        self.dbg_pc_spans.resize(self.instructions.len(), None);
        if start_pc < end_pc && start_pc < self.dbg_pc_spans.len() {
            let end = end_pc.min(self.dbg_pc_spans.len());
            for slot in &mut self.dbg_pc_spans[start_pc..end] {
                if slot.is_none() {
                    *slot = Some(span);
                }
            }
        }
    }

    fn mark_stmt_pc(&mut self, pc: usize, span: (usize, usize)) {
        self.dbg_stmt_marks.push(DebugStmtMark {
            pc,
            start: span.0,
            end: span.1,
        });
    }

    fn mark_current_stmt_pc(&mut self, pc: usize) {
        if let Some(span) = self.current_stmt_span {
            self.mark_stmt_pc(pc, span);
        }
    }

    fn ast_node_span(node: &AstNode) -> Option<(usize, usize)> {
        match node {
            AstNode::Literal(_, span)
            | AstNode::Variable(_, span)
            | AstNode::OuterVariable(_, span)
            | AstNode::Assignment { span, .. }
            | AstNode::OuterAssignment { span, .. }
            | AstNode::Index { span, .. }
            | AstNode::IndexAssign { span, .. }
            | AstNode::Postfix { span, .. }
            | AstNode::PipeTap { span, .. }
            | AstNode::Pipe { span, .. }
            | AstNode::CallName { span, .. }
            | AstNode::CallAnonymous { span, .. }
            | AstNode::Assert { span, .. }
            | AstNode::Debug { span, .. }
            | AstNode::Pause { span, .. }
            | AstNode::FString { span, .. } => *span,
            AstNode::Block(stmts) | AstNode::BlockExpr(stmts, _) => {
                stmts.last().and_then(Self::ast_node_span)
            }
            AstNode::UnpackAssignment { .. } => None,
            other => other.span(),
        }
    }

    /// Return the span of the last expression in an AST node.
    /// For blocks, this recurses into the last statement.
    fn last_expr_span(node: &AstNode) -> Option<(usize, usize)> {
        Self::ast_node_span(node)
    }

    fn compile_stmt_with_spans(
        &mut self,
        node: &AstNode,
        value_needed: bool,
        spans: &[(usize, usize)],
    ) -> WqResult<()> {
        self.with_stmt_spans(spans, |this| {
            let start_pc = this.instructions.len();
            let start_mark_count = this.dbg_stmt_marks.len();
            let stmt_span = this.cur_stmt_spans.first().copied();
            let fallback_span = stmt_span
                .is_none()
                .then(|| Self::ast_node_span(node))
                .flatten();
            let effective_span = stmt_span.or(fallback_span);
            let saved_current = this.current_stmt_span;
            this.current_stmt_span = effective_span.or(saved_current);
            this.cur_stmt_idx = usize::from(stmt_span.is_some());
            let result = this.compile_in_context(node, value_needed);
            let end_pc = this.instructions.len();
            this.dbg_pc_spans.resize(end_pc, None);
            if result.is_ok() {
                this.fill_span_range(start_pc, end_pc, effective_span);
                if let Some(span) = effective_span {
                    let has_stmt_mark = this.dbg_stmt_marks[start_mark_count..]
                        .iter()
                        .any(|mark| mark.start == span.0 && mark.end == span.1);
                    if end_pc > start_pc && !has_stmt_mark {
                        this.mark_stmt_pc(end_pc - 1, span);
                    }
                }
            }
            this.current_stmt_span = saved_current;
            result
        })
    }

    fn compile_stmt_sequence_with_spans(
        &mut self,
        node: &AstNode,
        value_needed: bool,
        spans: &[(usize, usize)],
    ) -> WqResult<()> {
        self.with_stmt_spans(spans, |this| {
            this.compile_stmt_sequence_inner(node, value_needed)
        })
    }

    fn compile_stmt_sequence(&mut self, node: &AstNode, value_needed: bool) -> WqResult<()> {
        let spans = self.take_stmt_spans_for(node);
        self.compile_stmt_sequence_with_spans(node, value_needed, &spans)
    }

    fn compile_stmt_sequence_inner(&mut self, node: &AstNode, value_needed: bool) -> WqResult<()> {
        match node {
            AstNode::Block(stmts) => {
                if stmts.is_empty() {
                    self.emit_load_const(Value::unit());
                    return Ok(());
                }
                for stmt in stmts.iter().take(stmts.len().saturating_sub(1)) {
                    let spans = self.take_stmt_spans_for(stmt);
                    self.compile_stmt_with_spans(stmt, false, &spans)?;
                    self.push_inst(Instruction::Pop);
                }
                let spans = self.take_stmt_spans_for(stmts.last().expect("non-empty block"));
                self.compile_stmt_with_spans(
                    stmts.last().expect("non-empty block"),
                    value_needed,
                    &spans,
                )
            }
            AstNode::BlockExpr(stmts, _) => {
                let saved_current = self.current_stmt_span;
                if self.current_stmt_span.is_none()
                    && let Some(span) = self.cur_stmt_spans.get(self.cur_stmt_idx).copied()
                {
                    self.current_stmt_span = Some(span);
                    self.cur_stmt_idx += 1;
                }
                let result = if stmts.is_empty() {
                    self.emit_load_const(Value::unit());
                    Ok(())
                } else {
                    for stmt in stmts.iter().take(stmts.len().saturating_sub(1)) {
                        let spans = self.take_stmt_spans_for(stmt);
                        self.compile_stmt_with_spans(stmt, false, &spans)?;
                        self.push_inst(Instruction::Pop);
                    }
                    let spans =
                        self.take_stmt_spans_for(stmts.last().expect("non-empty block expr"));
                    self.compile_stmt_with_spans(
                        stmts.last().expect("non-empty block expr"),
                        value_needed,
                        &spans,
                    )
                };
                self.current_stmt_span = saved_current;
                result
            }
            _ => {
                let spans = self.take_stmt_spans_for(node);
                self.compile_stmt_with_spans(node, value_needed, &spans)
            }
        }
    }

    fn compile_node(&mut self, node: &AstNode) -> WqResult<()> {
        match node {
            AstNode::Error(err, _) => {
                return Err(err.clone());
            }
            AstNode::Literal(v, ..) => self.emit_load_const(v.clone()),
            AstNode::Variable(name, span) => self.emit_load(name, *span),
            AstNode::OuterVariable(name, span) => self.emit_outer_load(name, *span)?,
            AstNode::PipeInput => {
                return Err(self.syntax_err_here("pipe input placeholder escaped its pipe context"));
            }
            AstNode::NamedArg { value, .. } => {
                return self.compile_node(value);
            }
            AstNode::Ellipsis => {
                return Err(self.syntax_err_here(
                    "'...' placeholder is only valid in unpack assignment pattern",
                ));
            }
            AstNode::Assignment {
                name, op, value, ..
            } => {
                if let Some(op) = op {
                    if *op == BinaryOperator::Cat {
                        // Cat assignment: name ,: rhs → name = name, rhs
                        // Compile both operands as stack values for Cat(n)
                        let left_name = name.clone();
                        self.compile_expr(&AstNode::Variable(left_name, None))?;
                        self.compile_expr(value)?;
                        self.instructions.push(Instruction::Cat(2));
                    } else {
                        let left = self.operand_for_name(name);
                        let right = self.compile_expr_as_operand(value)?;
                        self.instructions
                            .push(Instruction::binary_op(*op, left, right));
                    }
                    self.emit_store_keep(name);
                } else if let AstNode::Function {
                    params,
                    ref_capture,
                    body,
                } = &**value
                {
                    // Reserve slot for recursion when in a local scope
                    if self.fn_depth > 0 {
                        self.local_slot(name);
                    }
                    let mut capture_needs =
                        function_capture_needs(body, params.as_deref(), *ref_capture, Some(name));
                    if *ref_capture {
                        let available = self.ref_default_available_names();
                        collect_ref_default_assignment_needs(
                            body,
                            params.as_deref(),
                            &available,
                            Some(name),
                            &mut capture_needs,
                        );
                    }
                    let mut c = Compiler::new();
                    c.fn_depth = self.fn_depth + 1;
                    c.defining_name = Some(name.clone());
                    if *ref_capture {
                        c.ref_default_names = capture_needs.by_ref.clone();
                    }
                    // prepare captures from current locals if inside a function
                    if self.fn_depth > 0 {
                        self.seed_child_captures(&mut c, &capture_needs, Some(name));
                    }
                    c.emit_params_and_prologue(params)?;
                    // Prepare spans stream for nested functions: child starts at next entry
                    let mut spans_for_fn = self.current_fn_spans();
                    if spans_for_fn.is_empty()
                        && let Some(span) = self.current_stmt_span
                    {
                        spans_for_fn.push(span);
                    }
                    // Propagate pretty error context to child compiler
                    if let Some(src) = &self.src_text {
                        c.set_source(src.clone());
                    }
                    if let Some(path) = &self.src_path {
                        c.set_source_path(path.clone());
                    }
                    c.set_stmt_spans(spans_for_fn.clone());
                    c.fn_spans_stream = self.fn_spans_stream.clone();
                    c.fn_spans_idx = self.fn_spans_idx.saturating_add(1);
                    c.trace_symbol_operands = self.trace_symbol_operands;
                    c.compile(body)?;
                    self.has_runtime_debug |= c.has_runtime_debug;

                    // Advance our index past what child consumed
                    self.fn_spans_idx = c.fn_spans_idx;
                    let locals = c.local_count();
                    let dbg_local_names = c.local_names_vec();
                    let mut func_instructions = c.instructions;
                    func_instructions.push(Instruction::Return);
                    let func_arc: Arc<[Instruction]> = func_instructions.into();
                    let params_arc = params.as_ref().map(|ps| {
                        Arc::<[String]>::from(
                            ps.iter()
                                .filter_map(|p| match p {
                                    Parameter::Pos { name, .. } => Some(name.clone()),
                                    _ => None,
                                })
                                .collect::<Vec<_>>(),
                        )
                    });
                    let named_params_arc: Option<Arc<[Arc<str>]>> =
                        params.as_ref().and_then(|ps| {
                            let names: Vec<Arc<str>> = ps
                                .iter()
                                .filter_map(|p| match p {
                                    Parameter::Named { name, .. } => Some(Arc::from(name.as_str())),
                                    _ => None,
                                })
                                .collect();
                            if names.is_empty() {
                                None
                            } else {
                                Some(Arc::from(names))
                            }
                        });
                    let spans_arc: Arc<[(usize, usize)]> = Arc::from(spans_for_fn.clone());
                    let mut dbg_pc_spans = c.dbg_pc_spans;
                    dbg_pc_spans.resize(func_arc.len(), None);
                    let dbg_pc_spans_arc: Arc<[Option<(usize, usize)>]> = Arc::from(dbg_pc_spans);
                    let dbg_stmt_marks_arc: Arc<[DebugStmtMark]> = Arc::from(c.dbg_stmt_marks);
                    let local_names_arc: Arc<[String]> = Arc::from(dbg_local_names.clone());
                    if !c.captures.is_empty() {
                        self.instructions.push(Instruction::load_closure(
                            crate::vm::inst::ClosurePayload {
                                params: params_arc.clone(),
                                named_params: named_params_arc.clone(),
                                locals,
                                captures: c.captures.clone(),
                                instructions: func_arc,
                                dbg_stmt_spans: spans_arc,
                                dbg_pc_spans: dbg_pc_spans_arc,
                                dbg_stmt_marks: dbg_stmt_marks_arc,
                                dbg_local_names: local_names_arc,
                            },
                        ));
                    } else {
                        self.emit_load_const(Value::CompiledFunction(Arc::new(FunctionData {
                            params: params_arc,
                            named_params: named_params_arc.clone(),
                            locals,
                            instructions: func_arc,
                            dbg_chunk: None,
                            dbg_stmt_spans: Some(spans_arc),
                            dbg_source_base_offset: 0,
                            dbg_pc_spans: Some(dbg_pc_spans_arc),
                            dbg_stmt_marks: Some(dbg_stmt_marks_arc),
                            dbg_local_names: Some(local_names_arc),
                            dbg_provenance: None,
                        })));
                    }
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name);
                    if self.fn_depth > 0 {
                        self.fn_locals.insert(name.clone());
                    }
                } else {
                    self.compile_expr(value)?;
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name);
                }
            }
            AstNode::OuterAssignment {
                name,
                op,
                value,
                name_span,
                ..
            } => {
                if let Some(op) = op {
                    if *op == BinaryOperator::Cat {
                        self.compile_expr(&AstNode::OuterVariable(name.clone(), *name_span))?;
                        self.compile_expr(value)?;
                        self.instructions.push(Instruction::Cat(2));
                    } else {
                        let left = if let Some(idx) = self.ref_capture_map.get(name) {
                            Operand::Capture(*idx)
                        } else {
                            Operand::Var(name.clone().into())
                        };
                        let right = self.compile_expr_as_operand(value)?;
                        self.instructions
                            .push(Instruction::binary_op(*op, left, right));
                    }
                    self.emit_outer_store_keep(name, *name_span)?;
                } else {
                    self.compile_expr(value)?;
                    self.emit_outer_store_keep(name, *name_span)?;
                }
            }
            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => match operator {
                BinaryOperator::BoolAnd => {
                    self.compile_lazy_binary_chain(left, *operator, right)?;
                }
                BinaryOperator::BoolOr => {
                    self.compile_lazy_binary_chain(left, *operator, right)?;
                }
                _ => self.compile_binary_chain(left, *operator, right)?,
            },
            AstNode::ComparisonChain { first, rest } => {
                self.compile_expr(first)?;
                let mut ops: Vec<BinaryOperator> = Vec::with_capacity(rest.len());
                for (op, node) in rest {
                    self.compile_expr(node)?;
                    ops.push(*op);
                }
                self.instructions
                    .push(Instruction::CmpChain(ops.into_boxed_slice()));
            }
            AstNode::Cat(items) => {
                for item in items {
                    self.compile_expr(item)?;
                }
                self.instructions.push(Instruction::Cat(items.len()));
            }
            AstNode::Range {
                start,
                end,
                step,
                inclusive,
            } => {
                self.compile_expr(start)?;
                self.compile_expr(end)?;
                if let Some(step_expr) = step {
                    self.compile_expr(step_expr)?;
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
            AstNode::UnaryOp {
                operator, operand, ..
            } => {
                let op = self.compile_expr_as_operand(operand)?;
                self.instructions.push(Instruction::unary_op(*operator, op));
            }
            AstNode::List(elements) => {
                for elem in elements {
                    self.compile_expr(elem)?;
                }
                self.instructions
                    .push(Instruction::MakeList(elements.len()));
            }
            AstNode::Dict(pairs) => {
                for (k, v) in pairs {
                    self.emit_load_const(Value::Tag(k.clone().into()));
                    self.compile_expr(v)?;
                }
                self.instructions.push(Instruction::MakeDict(pairs.len()));
            }

            AstNode::CallName {
                name, args, span, ..
            } => {
                let start = self.instructions.len();
                if let Some(id) = self.builtins.get_id(name) {
                    self.compile_call_args(args)?;
                    self.instructions.push(Instruction::CallBuiltinId(
                        id.try_into().expect("builtin id overflow"),
                        args.len().try_into().expect("argc overflow"),
                    ));
                } else if self.fn_depth > 0 {
                    // Inside a function
                    //  locals => CallLocal
                    //  everything else => emit_load => LoadSelf/LoadCapture/LoadVar
                    if self.is_local(name) && self.fn_locals.contains(name) {
                        self.compile_call_args(args)?;
                        let slot = self.locals[name];
                        self.instructions
                            .push(Instruction::CallLocal(slot, args.len()));
                    } else {
                        self.emit_load(name, None);
                        self.compile_call_args(args)?;
                        self.instructions.push(Instruction::Postfix(args.len()));
                    }
                } else {
                    self.compile_call_args(args)?;
                    self.instructions
                        .push(Instruction::CallUser(name.clone().into(), args.len()));
                }
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::CallAnonymous { object, args, span } => {
                let start = self.instructions.len();
                if let Some((receiver, method)) = Self::method_lookup_target(object.as_ref())
                    && self.compile_method_dispatch(
                        receiver,
                        method,
                        args,
                        MethodDispatchKind::Call,
                    )?
                {
                    let end = self.instructions.len();
                    self.fill_span_range(start, end, *span);
                    return Ok(());
                }
                self.compile_expr(object)?;
                self.compile_call_args(args)?;
                self.instructions.push(Instruction::CallAnon(args.len()));
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::Postfix {
                object,
                items,
                explicit_call: _,
                depth,
                span,
            } => {
                let start = self.instructions.len();
                if let Some(depth) = depth {
                    let AstNode::Variable(name, _) = object.as_ref() else {
                        return Err(self.syntax_err_at(
                            *span,
                            "depth modifier can only be used on depth-aware builtins",
                        ));
                    };
                    let id = self.builtins.get_id(name).ok_or_else(|| {
                        self.syntax_err_at(
                            *span,
                            format!("depth modifier can only be used on depth-aware builtins, got '{name}'"),
                        )
                    })?;
                    let sugar = self.builtins.depth_sugar_from_id(id);
                    let args =
                        self.expand_depth_sugar_args(name, sugar, items.to_vec(), *depth, *span)?;
                    self.compile_call_args(&args)?;
                    self.instructions.push(Instruction::CallBuiltinId(
                        u16::try_from(id).expect("builtin id overflow"),
                        u16::try_from(args.len()).expect("argc overflow"),
                    ));
                    let end = self.instructions.len();
                    self.fill_span_range(start, end, *span);
                    return Ok(());
                }
                if let Some((receiver, method)) = Self::method_lookup_target(object.as_ref())
                    && self.compile_method_dispatch(
                        receiver,
                        method,
                        items,
                        MethodDispatchKind::Postfix,
                    )?
                {
                    let end = self.instructions.len();
                    self.fill_span_range(start, end, *span);
                    return Ok(());
                }
                let builtin_id = match object.as_ref() {
                    AstNode::Variable(name, _) => self.builtins.get_id(name),
                    _ => None,
                };

                if let Some(id) = builtin_id {
                    // Builtin call: don't compile the callee, only the args
                    self.compile_call_args(items)?;
                    self.instructions.push(Instruction::CallBuiltinId(
                        u16::try_from(id).expect("builtin id overflow"),
                        u16::try_from(items.len()).expect("argc overflow"),
                    ));
                } else {
                    // Non-builtin: compile the callee first, then the args
                    let mut optimized = false;
                    if let AstNode::Variable(name, _) = object.as_ref() {
                        if self.is_local(name) {
                            let slot = self.locals[name];
                            self.compile_call_args(items)?;
                            self.instructions
                                .push(Instruction::PostfixLocal(slot, items.len()));
                            optimized = true;
                        } else if self.is_ref_default_name(name) {
                            self.compile_call_args(items)?;
                            if let Some(idx) = self.ref_capture_map.get(name).copied() {
                                self.instructions
                                    .push(Instruction::PostfixCapture(idx, items.len()));
                            } else {
                                self.instructions.push(Instruction::PostfixVar(
                                    name.clone().into(),
                                    items.len(),
                                ));
                            }
                            optimized = true;
                        } else if let Some(idx) = self.capture_map.get(name).copied() {
                            self.compile_call_args(items)?;
                            self.instructions
                                .push(Instruction::PostfixCapture(idx, items.len()));
                            optimized = true;
                        } else if self.fn_depth == 0 {
                            self.compile_call_args(items)?;
                            self.instructions
                                .push(Instruction::PostfixVar(name.clone().into(), items.len()));
                            optimized = true;
                        }
                    }
                    if !optimized {
                        self.compile_expr(object)?;
                        self.compile_call_args(items)?;
                        self.instructions.push(Instruction::Postfix(items.len()));
                    }
                }
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::Pipe { .. } => {
                return Err(
                    self.syntax_err_here("Pipe node should have been resolved before compilation")
                );
            }
            AstNode::PipeTap {
                input,
                effect,
                span,
            } => {
                let start = self.instructions.len();
                let id = {
                    let v = self.gensym;
                    self.gensym = self.gensym.wrapping_add(1);
                    v
                };
                let temp_name = format!("--vm-pipe-tap-{id}");
                self.compile_expr(input)?;
                self.emit_store(&temp_name);
                let effect = replace_pipe_input(effect, &temp_name);
                self.compile_in_context(&effect, true)?;
                self.instructions.push(Instruction::Pop);
                self.emit_load(&temp_name, None);
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
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
                    self.compile_expr(e)?;
                } else {
                    self.emit_load_const(Value::unit());
                }
                self.instructions.push(Instruction::Return);
            }
            AstNode::Assert { expr, span } => {
                self.has_runtime_debug = true;
                self.compile_expr(expr)?;
                let pc = self.instructions.len();
                self.push_inst(Instruction::Assert);
                if let Some(slot) = self.dbg_pc_spans.get_mut(pc) {
                    *slot = *span;
                }
            }
            AstNode::Debug { expr, span } => {
                self.has_runtime_debug = true;
                let begin_pc = self.instructions.len();
                self.push_inst(Instruction::TraceBegin);
                if let Some(slot) = self.dbg_pc_spans.get_mut(begin_pc) {
                    *slot = *span;
                }
                let prev_trace_symbol_operands = self.trace_symbol_operands;
                self.trace_symbol_operands = true;
                let result = self.compile_expr(expr);
                self.trace_symbol_operands = prev_trace_symbol_operands;
                result?;
                let pc = self.instructions.len();
                self.push_inst(Instruction::Debug);
                if let Some(slot) = self.dbg_pc_spans.get_mut(pc) {
                    *slot = *span;
                }
            }
            AstNode::Pause { expr, span } => {
                let has_expr = expr.is_some();
                if let Some(expr) = expr {
                    self.compile_expr(expr)?;
                }
                let pc = self.instructions.len();
                self.push_inst(Instruction::Pause);
                if let Some(slot) = self.dbg_pc_spans.get_mut(pc) {
                    *slot = *span;
                }
                if !has_expr && self.value_needed {
                    self.emit_load_const(Value::unit());
                }
            }
            AstNode::Try(expr) => {
                let pos = self.instructions.len();
                self.instructions.push(Instruction::Try(0));
                self.compile_expr(expr)?;
                let len = self.instructions.len() - pos - 1;
                if let Instruction::Try(ref mut l) = self.instructions[pos] {
                    *l = len;
                }
            }
            AstNode::Index {
                object,
                index,
                span,
            } => {
                let start = self.instructions.len();
                let mut optimized = false;
                if let AstNode::Variable(name, _) = &**object {
                    if self.is_local(name) {
                        let slot = self.locals[name];
                        self.compile_expr(index)?;
                        self.instructions.push(Instruction::IndexLoadLocal(slot));
                        optimized = true;
                    } else if self.is_ref_default_name(name) {
                        self.compile_expr(index)?;
                        self.push_ref_default_index_load(name);
                        optimized = true;
                    } else if self.fn_depth == 0 {
                        self.compile_expr(index)?;
                        self.instructions
                            .push(Instruction::IndexLoadVar(name.clone().into()));
                        optimized = true;
                    }
                } else if let AstNode::OuterVariable(name, _) = &**object
                    && let Some(idx) = self.ref_capture_map.get(name).copied()
                {
                    self.compile_expr(index)?;
                    self.instructions.push(Instruction::IndexLoadCapture(idx));
                    optimized = true;
                }
                if !optimized {
                    self.compile_expr(object)?;
                    self.compile_expr(index)?;
                    self.instructions.push(Instruction::Index);
                }
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::IndexAssign {
                object,
                index,
                op,
                value,
                ..
            } => match &**object {
                AstNode::Variable(name, _) => {
                    if let Some(op) = op {
                        let tmp_id = {
                            let v = self.gensym;
                            self.gensym = self.gensym.wrapping_add(1);
                            v
                        };
                        let tmp_name = format!("--vm-idx-assign-{tmp_id}");
                        self.compile_expr(index)?;
                        self.emit_store(&tmp_name);
                        self.emit_load(&tmp_name, None); // for the assignment
                        self.emit_load(&tmp_name, None); // for the load
                        if self.fn_depth > 0 && self.is_local(name) {
                            let slot = self.local_slot(name);
                            self.instructions.push(Instruction::IndexLoadLocal(slot));
                        } else if self.is_ref_default_name(name) {
                            self.push_ref_default_index_load(name);
                        } else {
                            self.instructions
                                .push(Instruction::IndexLoadVar(name.clone().into()));
                        }
                        if *op == BinaryOperator::Cat {
                            self.compile_expr(value)?;
                            self.instructions.push(Instruction::Cat(2));
                        } else {
                            let right_op = self.compile_expr_as_operand(value)?;
                            self.instructions.push(Instruction::binary_op(
                                *op,
                                Operand::Stack,
                                right_op,
                            ));
                        }
                        if self.fn_depth > 0 && self.is_local(name) {
                            let slot = self.local_slot(name);
                            self.instructions.push(Instruction::IndexAssignLocal(slot));
                        } else if self.is_ref_default_name(name) {
                            self.push_ref_default_index_assign(name);
                        } else {
                            self.instructions
                                .push(Instruction::IndexAssignVar(name.clone().into()));
                        }
                    } else {
                        if self.fn_depth > 0 && self.is_local(name) {
                            let slot = self.local_slot(name);
                            self.compile_expr(index)?;
                            self.compile_expr(value)?;
                            self.instructions.push(Instruction::IndexAssignLocal(slot));
                        } else if self.is_ref_default_name(name) {
                            self.compile_expr(index)?;
                            self.compile_expr(value)?;
                            self.push_ref_default_index_assign(name);
                        } else {
                            self.compile_expr(index)?;
                            self.compile_expr(value)?;
                            self.instructions
                                .push(Instruction::IndexAssignVar(name.clone().into()));
                        }
                    }
                }
                AstNode::OuterVariable(name, _) => {
                    if let Some(op) = op {
                        let tmp_id = {
                            let v = self.gensym;
                            self.gensym = self.gensym.wrapping_add(1);
                            v
                        };
                        let tmp_name = format!("--vm-idx-assign-{tmp_id}");
                        self.compile_expr(index)?;
                        self.emit_store(&tmp_name);
                        self.emit_load(&tmp_name, None); // for assignment
                        if let Some(idx) = self.ref_capture_map.get(name).copied() {
                            self.emit_load(&tmp_name, None); // for load
                            self.instructions.push(Instruction::IndexLoadCapture(idx));
                        } else {
                            self.emit_load(&tmp_name, None); // for load
                            self.instructions
                                .push(Instruction::IndexLoadVar(name.clone().into()));
                        }
                        if *op == BinaryOperator::Cat {
                            self.compile_expr(value)?;
                            self.instructions.push(Instruction::Cat(2));
                        } else {
                            let right_op = self.compile_expr_as_operand(value)?;
                            self.instructions.push(Instruction::binary_op(
                                *op,
                                Operand::Stack,
                                right_op,
                            ));
                        }
                        if let Some(idx) = self.ref_capture_map.get(name) {
                            self.instructions
                                .push(Instruction::IndexAssignCapture(*idx));
                        } else {
                            self.instructions
                                .push(Instruction::IndexAssignVar(name.clone().into()));
                        }
                    } else {
                        self.compile_expr(index)?;
                        self.compile_expr(value)?;
                        if let Some(idx) = self.ref_capture_map.get(name) {
                            self.instructions
                                .push(Instruction::IndexAssignCapture(*idx));
                        } else {
                            self.instructions
                                .push(Instruction::IndexAssignVar(name.clone().into()));
                        }
                    }
                }
                _ => return Err(self.syntax_err_here("Invalid index assignment target")),
            },
            AstNode::Function {
                params,
                ref_capture,
                body,
            } => {
                let mut capture_needs =
                    function_capture_needs(body, params.as_deref(), *ref_capture, None);
                if *ref_capture {
                    let available = self.ref_default_available_names();
                    collect_ref_default_assignment_needs(
                        body,
                        params.as_deref(),
                        &available,
                        None,
                        &mut capture_needs,
                    );
                }
                let mut c = Compiler::new();
                c.fn_depth = self.fn_depth + 1;
                if *ref_capture {
                    c.ref_default_names = capture_needs.by_ref.clone();
                }
                if self.fn_depth > 0 {
                    self.seed_child_captures(&mut c, &capture_needs, self.defining_name.as_deref());
                }
                c.emit_params_and_prologue(params)?;
                // Prepare spans stream for nested functions: child starts at next entry
                let spans_for_fn = self.current_fn_spans();
                // Note: we intentionally do NOT fall back to self.current_stmt_span here.
                // current_stmt_span belongs to the outer statement (e.g. a loop body) and
                // may include braces like `{t}`.
                // If the parser didn't record spans for
                // this function, the child compiler will use ast_node_span fallback
                // instead, which gives the exact expression span without braces.
                // Propagate pretty error context to child compiler
                if let Some(src) = &self.src_text {
                    c.set_source(src.clone());
                }
                if let Some(path) = &self.src_path {
                    c.set_source_path(path.clone());
                }
                c.set_stmt_spans(spans_for_fn.clone());
                c.fn_spans_stream = self.fn_spans_stream.clone();
                c.fn_spans_idx = self.fn_spans_idx.saturating_add(1);
                c.trace_symbol_operands = self.trace_symbol_operands;
                c.compile(body)?;
                self.has_runtime_debug |= c.has_runtime_debug;

                self.fn_spans_idx = c.fn_spans_idx;
                let locals = c.local_count();
                let dbg_local_names = c.local_names_vec();
                let mut func_instructions = c.instructions;
                func_instructions.push(Instruction::Return);
                let func_arc: Arc<[Instruction]> = func_instructions.into();
                let params_arc = params.as_ref().map(|ps| {
                    Arc::<[String]>::from(
                        ps.iter()
                            .filter_map(|p| match p {
                                Parameter::Pos { name, .. } => Some(name.clone()),
                                _ => None,
                            })
                            .collect::<Vec<_>>(),
                    )
                });
                let named_params_arc: Option<Arc<[Arc<str>]>> = params.as_ref().and_then(|ps| {
                    let names: Vec<Arc<str>> = ps
                        .iter()
                        .filter_map(|p| match p {
                            Parameter::Named { name, .. } => Some(Arc::from(name.as_str())),
                            _ => None,
                        })
                        .collect();
                    if names.is_empty() {
                        None
                    } else {
                        Some(Arc::from(names))
                    }
                });
                let spans_arc: Arc<[(usize, usize)]> = Arc::from(spans_for_fn.clone());
                let mut dbg_pc_spans = c.dbg_pc_spans;
                dbg_pc_spans.resize(func_arc.len(), None);
                let dbg_pc_spans_arc: Arc<[Option<(usize, usize)>]> = Arc::from(dbg_pc_spans);
                let dbg_stmt_marks_arc: Arc<[DebugStmtMark]> = Arc::from(c.dbg_stmt_marks);
                let local_names_arc: Arc<[String]> = Arc::from(dbg_local_names.clone());
                if !c.captures.is_empty() {
                    self.instructions.push(Instruction::load_closure(
                        crate::vm::inst::ClosurePayload {
                            params: params_arc.clone(),
                            named_params: named_params_arc.clone(),
                            locals,
                            captures: c.captures.clone(),
                            instructions: func_arc,
                            dbg_stmt_spans: spans_arc,
                            dbg_pc_spans: dbg_pc_spans_arc,
                            dbg_stmt_marks: dbg_stmt_marks_arc,
                            dbg_local_names: local_names_arc,
                        },
                    ));
                } else {
                    self.emit_load_const(Value::CompiledFunction(Arc::new(FunctionData {
                        params: params_arc,
                        named_params: named_params_arc.clone(),
                        locals,
                        instructions: func_arc,
                        dbg_chunk: None,
                        dbg_stmt_spans: Some(spans_arc),
                        dbg_source_base_offset: 0,
                        dbg_pc_spans: Some(dbg_pc_spans_arc),
                        dbg_stmt_marks: Some(dbg_stmt_marks_arc),
                        dbg_local_names: Some(local_names_arc),
                        dbg_provenance: None,
                    })));
                }
            }
            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
                ..
            } => {
                self.compile_expr(condition)?;
                let jump_if_false_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                let true_spans = self.take_stmt_spans_for(true_branch);
                self.compile_stmt_sequence_with_spans(true_branch, self.value_needed, &true_spans)?;
                let jump_end_pos = self.instructions.len();
                self.instructions.push(Instruction::Jump(0));
                // patch jump_if_false to here
                let else_start = self.instructions.len();
                self.instructions[jump_if_false_pos] = Instruction::JumpIfFalse(else_start);
                if let Some(fb) = false_branch {
                    let false_spans = self.take_stmt_spans_for(fb);
                    self.compile_stmt_sequence_with_spans(fb, self.value_needed, &false_spans)?;
                } else {
                    // when there is no false branch, the conditional
                    // expression should evaluate to unit on the false path
                    self.emit_load_const(Value::unit());
                }
                let end = self.instructions.len();
                self.instructions[jump_end_pos] = Instruction::Jump(end);
            }
            AstNode::ConditionalChain { .. } => {
                unreachable!("ConditionalChain should have been resolved before compilation")
            }
            AstNode::ConditionalDot { .. } => {
                unreachable!("ConditionalDot should have been resolved before compilation")
            }
            AstNode::WLoop {
                condition, body, ..
            } => {
                let cond_span = Self::last_expr_span(condition);
                if self.value_needed
                    && pure_const_body(body)
                    && let Some(value) = const_body_value(body)
                {
                    let start = self.instructions.len();
                    self.backward_jump_targets.insert(start);
                    self.compile_expr(condition)?;
                    let jump_pos = self.instructions.len();
                    self.instructions.push(Instruction::JumpIfFalse(0));
                    self.dbg_pc_spans.resize(self.instructions.len(), None);
                    if let Some(span) = cond_span {
                        self.dbg_pc_spans[jump_pos] = Some(span);
                    }
                    self.instructions.push(Instruction::Jump(start));
                    let end = self.instructions.len();
                    self.instructions[jump_pos] = Instruction::JumpIfFalse(end);
                    self.emit_load_const(value);
                    return Ok(());
                }
                let body_spans = self.take_stmt_spans_for(body);
                let result_var = if self.value_needed {
                    let id = {
                        let v = self.gensym;
                        self.gensym = self.gensym.wrapping_add(1);
                        v
                    };
                    let result_var = format!("--w-loop-res-{id}");
                    self.emit_load_const(Value::unit());
                    self.emit_store(&result_var);
                    Some(result_var)
                } else {
                    None
                };
                let start = self.instructions.len();
                self.backward_jump_targets.insert(start);
                self.compile_expr(condition)?;
                if self.instructions.len() > start {
                    self.mark_current_stmt_pc(start);
                }
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.dbg_pc_spans.resize(self.instructions.len(), None);
                if let Some(span) = cond_span {
                    self.dbg_pc_spans[jump_pos] = Some(span);
                }
                self.loop_stack.push(LoopInfo::default());
                self.compile_stmt_sequence_with_spans(body, self.value_needed, &body_spans)?;
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
                    self.emit_load(result_var, None);
                } else {
                    self.emit_load_const(Value::unit());
                }
            }
            AstNode::NLoop { count, body, .. } => {
                let count_span = Self::last_expr_span(count);
                // Reject non-integer literal counts at compile time
                if let AstNode::Literal(value, _) = &**count
                    && !matches!(value, Value::Int(_) | Value::BigInt(_))
                {
                    return Err(self.syntax_err_at(
                        count_span,
                        format!("n-loop count must be an integer, got {}", value.type_name()),
                    ));
                }
                // if matches!(&**count, AstNode::Block(stmts) | AstNode::BlockExpr(stmts) if
                // stmts.is_empty()) {
                //     return Err(self.syntax_err_here(
                //         "n-loop count must be an integer, got unit"
                //     ));
                // }
                let body_spans = self.take_stmt_spans_for(body);
                // Unroll constant loops only when there is no control flow in body
                if let AstNode::Literal(Value::Int(n), _) = &**count
                    && *n >= 0
                    && !has_ctrl(body)
                {
                    let limit = 16;
                    if *n <= limit {
                        if *n == 0 {
                            self.emit_load_const(Value::unit());
                        } else {
                            let restore = self.begin_loop_var_restore("_n");
                            for i in 0..*n {
                                let iter_start = self.instructions.len();
                                self.emit_load_const(Value::Int(i));
                                self.emit_store("_n");
                                self.mark_current_stmt_pc(iter_start);
                                self.compile_stmt_sequence_with_spans(
                                    body,
                                    self.value_needed,
                                    &body_spans,
                                )?;
                                if i < *n - 1 {
                                    self.instructions.push(Instruction::Pop);
                                }
                            }
                            self.finish_loop_var_restore("_n", &restore);
                        }
                        return Ok(());
                    } else if *n <= 64 {
                        let restore = self.begin_loop_var_restore("_n");
                        let full_chunks = *n / 8;
                        let remainder = *n % 8;
                        for c in 0..full_chunks {
                            for i in 0..8 {
                                let idx = c * 8 + i;
                                let iter_start = self.instructions.len();
                                self.emit_load_const(Value::Int(idx));
                                self.emit_store("_n");
                                self.mark_current_stmt_pc(iter_start);
                                self.compile_stmt_sequence_with_spans(
                                    body,
                                    self.value_needed,
                                    &body_spans,
                                )?;
                                self.instructions.push(Instruction::Pop);
                            }
                        }
                        for i in 0..remainder {
                            let idx = full_chunks * 8 + i;
                            let iter_start = self.instructions.len();
                            self.emit_load_const(Value::Int(idx));
                            self.emit_store("_n");
                            self.mark_current_stmt_pc(iter_start);
                            self.compile_stmt_sequence_with_spans(
                                body,
                                self.value_needed,
                                &body_spans,
                            )?;
                            if i < remainder - 1 {
                                self.instructions.push(Instruction::Pop);
                            }
                        }
                        if *n > 0 {
                            self.instructions.pop();
                            self.finish_loop_var_restore("_n", &restore);
                        } else {
                            self.emit_load_const(Value::unit());
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
                let count_start = self.instructions.len();
                self.compile_expr(count)?; // -> count on stack
                self.fill_span_range(count_start, self.instructions.len(), count_span);
                self.emit_store(&count_var);
                let restore = self.begin_loop_var_restore("_n");
                self.emit_load_const(Value::Int(0));
                self.emit_store("_n");
                if let Some(result_var) = &result_var {
                    self.emit_load_const(Value::unit());
                    self.emit_store(result_var);
                }
                let cmp_start = self.instructions.len();
                self.backward_jump_targets.insert(cmp_start);
                let left = self.operand_for_name("_n");
                let right = self.operand_for_name(&count_var);
                self.instructions
                    .push(Instruction::binary_op(BinaryOperator::Lt, left, right));
                self.dbg_pc_spans.resize(self.instructions.len(), None);
                if let Some(span) = count_span {
                    self.dbg_pc_spans[cmp_start] = Some(span);
                }
                self.mark_current_stmt_pc(cmp_start);
                let jump_pos = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.dbg_pc_spans.resize(self.instructions.len(), None);
                if let Some(span) = count_span {
                    self.dbg_pc_spans[jump_pos] = Some(span);
                }
                self.emit_load("_n", None);
                self.emit_store(&old_var);
                self.loop_stack.push(LoopInfo::default());
                self.compile_stmt_sequence_with_spans(body, self.value_needed, &body_spans)?;
                if let Some(result_var) = &result_var {
                    self.emit_store(result_var);
                } else {
                    self.instructions.push(Instruction::Pop);
                }
                let continue_target = self.instructions.len();
                let left = self.operand_for_name(&old_var);
                self.instructions.push(Instruction::binary_op(
                    BinaryOperator::Add,
                    left,
                    Operand::const_val(Value::Int(1)),
                ));
                self.emit_store("_n");
                self.instructions.push(Instruction::Jump(cmp_start));
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
                self.finish_loop_var_restore("_n", &restore);
                if let Some(result_var) = &result_var {
                    self.emit_load(result_var, None);
                } else {
                    self.emit_load_const(Value::unit());
                }
            }
            AstNode::Block(_) | AstNode::BlockExpr(..) => {
                self.compile_stmt_sequence_inner(node, self.value_needed)?;
            }
            AstNode::UnpackAssignment { .. } => {
                unreachable!("UnpackAssignment should have been resolved before compilation")
            }
            AstNode::Group { expr, .. } => {
                self.compile_expr(expr)?;
            }
            AstNode::FString { .. } => {
                unreachable!("FString should have been resolved before compilation")
            }
            AstNode::MutatingIndex { object, index, .. } => {
                let target = self.resolve_store_target(object)?;
                let is_empty = Self::is_empty_index_expr(index);
                if is_empty {
                    // x[!] => pop 1
                    self.emit_load_const(Value::Int(1));
                    self.instructions.push(Instruction::IndexMutate {
                        target,
                        op: MutationOp::Pop,
                    });
                } else {
                    // x[!i] => remove at i
                    self.compile_expr(index)?;
                    self.instructions.push(Instruction::IndexMutate {
                        target,
                        op: MutationOp::Remove,
                    });
                }
            }
            AstNode::MutatingIndexAssign {
                object,
                index,
                value,
                ..
            } => {
                let target = self.resolve_store_target(object)?;
                let is_empty = Self::is_empty_index_expr(index);
                self.compile_expr(value)?;
                if is_empty {
                    // x[!]:v => insert between
                    self.instructions.push(Instruction::IndexMutate {
                        target,
                        op: MutationOp::Insert,
                    });
                } else {
                    // x[!i]:v => insert at i
                    self.compile_expr(index)?;
                    self.instructions.push(Instruction::IndexMutate {
                        target,
                        op: MutationOp::InsertAt,
                    });
                }
            }
        }
        Ok(())
    }

    fn is_empty_index_expr(index: &AstNode) -> bool {
        match index {
            AstNode::List(items) => items.is_empty(),
            AstNode::Literal(Value::IntList(items), _) => items.is_empty(),
            AstNode::Literal(Value::List(items), _) => items.is_empty(),
            _ => false,
        }
    }

    fn resolve_store_target(&self, object: &AstNode) -> WqResult<StoreTarget> {
        match object {
            AstNode::Variable(name, _) => {
                if self.fn_depth > 0 && self.is_local(name) {
                    Ok(StoreTarget::Local(self.locals[name]))
                } else if self.is_ref_default_name(name) {
                    if let Some(idx) = self.ref_capture_map.get(name) {
                        Ok(StoreTarget::Capture(*idx))
                    } else {
                        Ok(StoreTarget::Var(name.clone().into()))
                    }
                } else {
                    Ok(StoreTarget::Var(name.clone().into()))
                }
            }
            AstNode::OuterVariable(name, _) => {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    Ok(StoreTarget::Capture(*idx))
                } else {
                    Ok(StoreTarget::Var(name.clone().into()))
                }
            }
            _ => unreachable!("mutating index target must be Variable or OuterVariable"),
        }
    }

    pub(crate) fn set_fn_spans(&mut self, spans: Vec<Vec<(usize, usize)>>) {
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
    pub(crate) fn set_source(&mut self, src: String) {
        self.src_text = Some(src);
    }

    pub(crate) fn set_source_path(&mut self, path: String) {
        self.src_path = Some(path);
    }

    pub(crate) fn set_stmt_spans(&mut self, spans: Vec<(usize, usize)>) {
        self.cur_stmt_spans = spans;
        self.cur_stmt_idx = 0;
        self.current_stmt_span = None;
    }

    fn syntax_err_at(&self, span: Option<(usize, usize)>, msg: impl Into<String>) -> WqError {
        let msg = msg.into();
        let mut e = WqError::new(WqErrorType::Syntax).src("compiler").msg(msg);

        if let (Some(src), Some((byte_start, byte_end))) = (
            self.src_text.as_ref(),
            span.or(self.current_stmt_span)
                .or_else(|| self.cur_stmt_spans.get(self.cur_stmt_idx).cloned()),
        ) {
            let path = self.src_path.clone().unwrap_or_else(|| "?".to_string());
            e = e
                .span(Some((byte_start, byte_end)))
                .source_ctx(src.clone(), path);
        }
        e
    }

    fn syntax_err_here(&self, msg: impl Into<String>) -> WqError {
        self.syntax_err_at(None, msg)
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

    fn is_ref_default_name(&self, name: &str) -> bool {
        self.fn_depth > 0 && self.ref_default_names.contains(name)
    }

    fn ref_default_operand(&self, name: &str) -> Option<Operand> {
        if !self.is_ref_default_name(name) {
            return None;
        }
        if let Some(idx) = self.ref_capture_map.get(name) {
            Some(Operand::Capture(*idx))
        } else {
            Some(Operand::Var(name.to_string().into()))
        }
    }

    fn push_ref_default_load(&mut self, name: &str) -> bool {
        if !self.is_ref_default_name(name) {
            return false;
        }
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions.push(Instruction::LoadCapture(*idx));
        } else {
            self.instructions
                .push(Instruction::LoadVar(name.to_string().into()));
        }
        true
    }

    fn push_ref_default_index_load(&mut self, name: &str) {
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions.push(Instruction::IndexLoadCapture(*idx));
        } else {
            self.instructions
                .push(Instruction::IndexLoadVar(name.to_string().into()));
        }
    }

    fn push_ref_default_index_assign(&mut self, name: &str) {
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions
                .push(Instruction::IndexAssignCapture(*idx));
        } else {
            self.instructions
                .push(Instruction::IndexAssignVar(name.to_string().into()));
        }
    }

    pub(crate) fn local_count(&self) -> u16 {
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

    fn ref_default_available_names(&self) -> IndexSet<String> {
        let mut names = IndexSet::new();
        names.extend(self.locals.keys().cloned());
        names.extend(self.capture_map.keys().cloned());
        names.extend(self.ref_capture_map.keys().cloned());
        names
    }

    fn seed_child_captures(
        &self,
        child: &mut Compiler,
        needs: &CaptureNeeds,
        skip_value_local: Option<&str>,
    ) {
        let mut pairs: Vec<(String, u16)> =
            self.locals.iter().map(|(k, &v)| (k.clone(), v)).collect();
        pairs.sort_by_key(|(_, idx)| *idx);

        for (k, v) in &pairs {
            if !needs.by_value.contains(k) {
                continue;
            }
            if skip_value_local.is_some_and(|name| name == k) {
                continue;
            }
            let idx = child.captures.len() as u16;
            child.capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::Local(*v));
        }

        for (k, v) in &pairs {
            if !needs.by_ref.contains(k) {
                continue;
            }
            let idx = child.captures.len() as u16;
            child.ref_capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::LocalShared(*v));
        }

        let mut parent_caps: Vec<(String, u16)> = self
            .capture_map
            .iter()
            .map(|(k, &i)| (k.clone(), i))
            .collect();
        parent_caps.sort_by_key(|(_, i)| *i);
        for (k, i_parent) in parent_caps {
            if !needs.by_value.contains(&k) {
                continue;
            }
            if child.capture_map.contains_key(&k) {
                continue;
            }
            let idx = child.captures.len() as u16;
            child.capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::Outer(i_parent));
        }

        let mut parent_ref_caps: Vec<(String, u16)> = self
            .ref_capture_map
            .iter()
            .map(|(k, &i)| (k.clone(), i))
            .collect();
        parent_ref_caps.sort_by_key(|(_, i)| *i);
        for (k, i_parent) in parent_ref_caps {
            if !needs.by_ref.contains(&k) {
                continue;
            }
            if child.ref_capture_map.contains_key(&k) {
                continue;
            }
            let idx = child.captures.len() as u16;
            child.ref_capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::Outer(i_parent));
        }
    }

    #[inline]
    fn emit_load_const(&mut self, value: Value) {
        self.push_inst(Instruction::load_const(value));
    }

    fn emit_load(&mut self, name: &str, span: Option<(usize, usize)>) {
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
            if self.push_ref_default_load(name) {
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
                    .push(Instruction::LoadVar(name.to_string().into()));
                return;
            }
            // capture globals by value
            let idx = self.captures.len() as u16;
            self.capture_map.insert(name.to_string(), idx);
            self.captures.push(Capture::Global(name.to_string(), span));
            self.instructions.push(Instruction::LoadCapture(idx));
            return;
        }
        // fn_depth == 0: top-level global
        self.instructions
            .push(Instruction::LoadVar(name.to_string().into()));
    }

    fn operand_for_name(&mut self, name: &str) -> Operand {
        if self.fn_depth > 0 {
            if self.is_local(name) {
                return Operand::Local(self.locals[name]);
            }
            if self.defining_name.as_ref().is_some_and(|n| n == name) {
                return Operand::Self_;
            }
            if let Some(operand) = self.ref_default_operand(name) {
                return operand;
            }
            if let Some(idx) = self.capture_map.get(name) {
                return Operand::Capture(*idx);
            }
            if self.builtins.has_function(name) {
                return Operand::Var(name.to_string().into());
            }
            let idx = self.captures.len() as u16;
            self.capture_map.insert(name.to_string(), idx);
            self.captures.push(Capture::Global(name.to_string(), None));
            return Operand::Capture(idx);
        }
        // fn_depth == 0: top-level global
        if self.builtins.has_function(name) {
            return Operand::Var(name.to_string().into());
        }
        Operand::Var(name.to_string().into())
    }

    fn compile_expr_as_operand(&mut self, node: &AstNode) -> WqResult<Operand> {
        match node {
            AstNode::Literal(v, ..) => Ok(Operand::const_val(v.clone())),
            AstNode::Variable(..) | AstNode::OuterVariable(..) if self.trace_symbol_operands => {
                self.compile_expr(node)?;
                Ok(Operand::Stack)
            }
            AstNode::Variable(name, _) => Ok(self.operand_for_name(name)),
            AstNode::OuterVariable(name, _) => {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    Ok(Operand::Capture(*idx))
                } else {
                    Ok(Operand::Var(name.to_string().into()))
                }
            }
            _ => {
                self.compile_expr(node)?;
                Ok(Operand::Stack)
            }
        }
    }

    fn compile_binary_chain(
        &mut self,
        mut left: &AstNode,
        operator: BinaryOperator,
        right: &AstNode,
    ) -> WqResult<()> {
        let mut chain = vec![(operator, right)];
        while let AstNode::BinaryOp {
            left: next_left,
            operator: next_operator,
            right: next_right,
        } = left
        {
            if matches!(
                next_operator,
                BinaryOperator::BoolAnd | BinaryOperator::BoolOr
            ) {
                break;
            }
            chain.push((*next_operator, next_right));
            left = next_left;
        }

        let mut left_op = self.compile_expr_as_operand(left)?;
        for (op, right) in chain.into_iter().rev() {
            let right_op = self.compile_expr_as_operand(right)?;
            self.instructions
                .push(Instruction::binary_op(op, left_op, right_op));
            left_op = Operand::Stack;
        }
        Ok(())
    }

    fn compile_lazy_binary_chain(
        &mut self,
        mut left: &AstNode,
        operator: BinaryOperator,
        right: &AstNode,
    ) -> WqResult<()> {
        let mut rights = vec![right];
        while let AstNode::BinaryOp {
            left: next_left,
            operator: next_operator,
            right: next_right,
        } = left
        {
            if *next_operator != operator {
                break;
            }
            rights.push(next_right);
            left = next_left;
        }

        self.compile_expr(left)?;
        for right in rights.into_iter().rev() {
            let lazy_pos = self.instructions.len();
            match operator {
                BinaryOperator::BoolAnd => self.instructions.push(Instruction::BoolAndLazy(0)),
                BinaryOperator::BoolOr => self.instructions.push(Instruction::BoolOrLazy(0)),
                _ => unreachable!("lazy binary chain only accepts bool operators"),
            }
            self.compile_expr(right)?;
            self.instructions
                .push(Instruction::binary_op(operator, Operand::Stack, Operand::Stack));
            let end = self.instructions.len();
            match operator {
                BinaryOperator::BoolAnd => {
                    self.instructions[lazy_pos] = Instruction::BoolAndLazy(end);
                }
                BinaryOperator::BoolOr => {
                    self.instructions[lazy_pos] = Instruction::BoolOrLazy(end);
                }
                _ => unreachable!("lazy binary chain only accepts bool operators"),
            }
        }
        Ok(())
    }

    fn begin_loop_var_restore(&mut self, name: &str) -> LoopVarRestore {
        let id = {
            let v = self.gensym;
            self.gensym = self.gensym.wrapping_add(1);
            v
        };
        let old_var = format!("--vm-loop-old-{name}-{id}");

        if self.fn_depth == 0 {
            let was_bound_var = format!("--vm-loop-was-bound-{name}-{id}");
            self.instructions
                .push(Instruction::LoadVarExists(name.to_string().into()));
            self.emit_store_keep(&was_bound_var);

            let skip_save = self.instructions.len();
            self.instructions.push(Instruction::JumpIfFalse(0));
            self.instructions
                .push(Instruction::LoadVar(name.to_string().into()));
            self.emit_store(&old_var);
            let after_save = self.instructions.len();
            self.instructions[skip_save] = Instruction::JumpIfFalse(after_save);

            return LoopVarRestore::TopLevel {
                old_var,
                was_bound_var,
            };
        }

        if self.is_local(name)
            || self.defining_name.as_ref().is_some_and(|n| n == name)
            || self.capture_map.contains_key(name)
            || self.ref_capture_map.contains_key(name)
            || self.is_ref_default_name(name)
        {
            self.emit_load_const(Value::unit());
            self.emit_store(&old_var);
            self.emit_load(name, None);
            self.emit_store(&old_var);
            LoopVarRestore::Function {
                old_var,
                was_bound_var: None,
            }
        } else {
            let was_bound_var = format!("--vm-loop-was-bound-{name}-{id}");
            self.instructions
                .push(Instruction::LoadVarExists(name.to_string().into()));
            self.emit_store_keep(&was_bound_var);

            self.emit_load_const(Value::unit());
            self.emit_store(&old_var);

            self.instructions
                .push(Instruction::LoadVarExists(name.to_string().into()));
            let skip_save = self.instructions.len();
            self.instructions.push(Instruction::JumpIfFalse(0));
            self.instructions
                .push(Instruction::LoadVar(name.to_string().into()));
            self.emit_store(&old_var);
            let after_save = self.instructions.len();
            self.instructions[skip_save] = Instruction::JumpIfFalse(after_save);

            LoopVarRestore::Function {
                old_var,
                was_bound_var: Some(was_bound_var),
            }
        }
    }

    fn finish_loop_var_restore(&mut self, name: &str, restore: &LoopVarRestore) {
        match restore {
            LoopVarRestore::TopLevel {
                old_var,
                was_bound_var,
            } => {
                self.emit_load(was_bound_var, None);
                let skip_restore = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.emit_load(old_var, None);
                self.instructions
                    .push(Instruction::StoreVar(name.to_string().into()));
                let end = self.instructions.len();
                self.instructions[skip_restore] = Instruction::JumpIfFalse(end);
            }
            LoopVarRestore::Function {
                old_var,
                was_bound_var,
            } => {
                if let Some(was_bound_var) = was_bound_var {
                    self.emit_load(was_bound_var, None);
                    let skip_restore = self.instructions.len();
                    self.instructions.push(Instruction::JumpIfFalse(0));
                    self.emit_load(old_var, None);
                    self.emit_store(name);
                    let end = self.instructions.len();
                    self.instructions[skip_restore] = Instruction::JumpIfFalse(end);
                } else {
                    self.emit_load(old_var, None);
                    self.emit_store(name);
                }
            }
        }
    }

    fn emit_outer_load(&mut self, name: &str, span: Option<(usize, usize)>) -> WqResult<()> {
        if self.fn_depth == 0 {
            return Err(self.syntax_err_at(span, "outer binding reference requires a closure"));
        }
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions.push(Instruction::LoadCapture(*idx));
        } else {
            self.instructions
                .push(Instruction::LoadVar(name.to_string().into()));
        }
        Ok(())
    }

    fn emit_store(&mut self, name: &str) {
        if self.fn_depth > 0 {
            if self.is_ref_default_name(name) {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    self.instructions.push(Instruction::StoreCaptureKeep(*idx));
                    self.instructions.push(Instruction::Pop);
                } else {
                    self.instructions
                        .push(Instruction::StoreVar(name.to_string().into()));
                }
                return;
            }
            let idx = self.local_slot(name);
            self.instructions.push(Instruction::StoreLocal(idx));
        } else {
            self.instructions
                .push(Instruction::StoreVar(name.to_string().into()));
        }
    }

    fn emit_store_keep(&mut self, name: &str) {
        if self.fn_depth > 0 {
            if self.is_ref_default_name(name) {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    self.instructions.push(Instruction::StoreCaptureKeep(*idx));
                } else {
                    self.instructions
                        .push(Instruction::StoreVarKeep(name.to_string().into()));
                }
                return;
            }
            let idx = self.local_slot(name);
            self.instructions.push(Instruction::StoreLocalKeep(idx));
        } else {
            self.instructions
                .push(Instruction::StoreVarKeep(name.to_string().into()));
        }
    }

    fn emit_outer_store_keep(&mut self, name: &str, span: Option<(usize, usize)>) -> WqResult<()> {
        if self.fn_depth == 0 {
            return Err(self.syntax_err_at(span, "outer binding reference requires a closure"));
        }
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions.push(Instruction::StoreCaptureKeep(*idx));
        } else {
            self.instructions
                .push(Instruction::StoreVarKeep(name.to_string().into()));
        }
        Ok(())
    }
}

#[derive(Default)]
struct LoopInfo {
    break_jumps: Vec<usize>,
    continue_jumps: Vec<usize>,
}

enum LoopVarRestore {
    TopLevel {
        old_var: String,
        was_bound_var: String,
    },
    Function {
        old_var: String,
        was_bound_var: Option<String>,
    },
}

#[derive(Default)]
struct CaptureNeeds {
    by_value: IndexSet<String>,
    by_ref: IndexSet<String>,
}

fn collect_ref_default_assignment_needs(
    node: &AstNode,
    params: ParamList<'_>,
    available: &IndexSet<String>,
    defining_name: Option<&str>,
    needs: &mut CaptureNeeds,
) {
    let mut excluded = IndexSet::new();
    if let Some(params) = params {
        excluded.extend(params.iter().map(|p| p.name().to_string()));
    } else {
        excluded.extend(["x".to_string(), "y".to_string(), "z".to_string()]);
    }
    if let Some(name) = defining_name {
        excluded.insert(name.to_string());
    }
    collect_ref_default_assignment_needs_inner(node, available, &excluded, needs);
}

fn collect_ref_default_assignment_needs_inner(
    node: &AstNode,
    available: &IndexSet<String>,
    excluded: &IndexSet<String>,
    needs: &mut CaptureNeeds,
) {
    match node {
        AstNode::Error(..)
        | AstNode::Literal(..)
        | AstNode::Variable(..)
        | AstNode::OuterVariable(..)
        | AstNode::Ellipsis
        | AstNode::PipeInput
        | AstNode::Break
        | AstNode::Continue
        | AstNode::Function { .. } => {}
        AstNode::Assignment { name, value, .. } => {
            if available.contains(name) && !excluded.contains(name) {
                needs.by_ref.insert(name.clone());
            }
            collect_ref_default_assignment_needs_inner(value, available, excluded, needs);
        }
        AstNode::OuterAssignment { value, .. } => {
            collect_ref_default_assignment_needs_inner(value, available, excluded, needs);
        }
        AstNode::BinaryOp { left, right, .. } => {
            collect_ref_default_assignment_needs_inner(left, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(right, available, excluded, needs);
        }
        AstNode::ComparisonChain { first, rest } => {
            collect_ref_default_assignment_needs_inner(first, available, excluded, needs);
            for (_, node) in rest {
                collect_ref_default_assignment_needs_inner(node, available, excluded, needs);
            }
        }
        AstNode::UnaryOp { operand, .. } | AstNode::Group { expr: operand, .. } => {
            collect_ref_default_assignment_needs_inner(operand, available, excluded, needs);
        }
        AstNode::Range {
            start, end, step, ..
        } => {
            collect_ref_default_assignment_needs_inner(start, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(end, available, excluded, needs);
            if let Some(step) = step {
                collect_ref_default_assignment_needs_inner(step, available, excluded, needs);
            }
        }
        AstNode::Cat(items) | AstNode::List(items) | AstNode::Block(items) => {
            for item in items {
                collect_ref_default_assignment_needs_inner(item, available, excluded, needs);
            }
        }
        AstNode::Dict(pairs) => {
            for (_, value) in pairs {
                collect_ref_default_assignment_needs_inner(value, available, excluded, needs);
            }
        }
        AstNode::BlockExpr(items, _) => {
            for item in items {
                collect_ref_default_assignment_needs_inner(item, available, excluded, needs);
            }
        }
        AstNode::Postfix { object, items, .. } => {
            collect_ref_default_assignment_needs_inner(object, available, excluded, needs);
            for item in items {
                collect_ref_default_assignment_needs_inner(item, available, excluded, needs);
            }
        }
        AstNode::Pipe { input, effect, .. } | AstNode::PipeTap { input, effect, .. } => {
            collect_ref_default_assignment_needs_inner(input, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(effect, available, excluded, needs);
        }
        AstNode::CallName { args, .. } => {
            for arg in args {
                collect_ref_default_assignment_needs_inner(arg, available, excluded, needs);
            }
        }
        AstNode::CallAnonymous { object, args, .. } => {
            collect_ref_default_assignment_needs_inner(object, available, excluded, needs);
            for arg in args {
                collect_ref_default_assignment_needs_inner(arg, available, excluded, needs);
            }
        }
        AstNode::Index { object, index, .. } | AstNode::MutatingIndex { object, index, .. } => {
            collect_ref_default_assignment_needs_inner(object, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(index, available, excluded, needs);
        }
        AstNode::IndexAssign {
            object,
            index,
            value,
            ..
        }
        | AstNode::MutatingIndexAssign {
            object,
            index,
            value,
            ..
        } => {
            collect_ref_default_assignment_needs_inner(object, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(index, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(value, available, excluded, needs);
        }
        AstNode::NamedArg { value, .. }
        | AstNode::Assert { expr: value, .. }
        | AstNode::Debug { expr: value, .. } => {
            collect_ref_default_assignment_needs_inner(value, available, excluded, needs);
        }
        AstNode::Pause { expr, .. } | AstNode::Return(expr) => {
            if let Some(expr) = expr {
                collect_ref_default_assignment_needs_inner(expr, available, excluded, needs);
            }
        }
        AstNode::Try(expr) => {
            collect_ref_default_assignment_needs_inner(expr, available, excluded, needs);
        }
        AstNode::Conditional {
            condition,
            true_branch,
            false_branch,
            ..
        } => {
            collect_ref_default_assignment_needs_inner(condition, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(true_branch, available, excluded, needs);
            if let Some(false_branch) = false_branch {
                collect_ref_default_assignment_needs_inner(
                    false_branch,
                    available,
                    excluded,
                    needs,
                );
            }
        }
        AstNode::ConditionalChain {
            pairs,
            default_branch,
            ..
        } => {
            for (condition, branch) in pairs {
                collect_ref_default_assignment_needs_inner(condition, available, excluded, needs);
                collect_ref_default_assignment_needs_inner(branch, available, excluded, needs);
            }
            collect_ref_default_assignment_needs_inner(default_branch, available, excluded, needs);
        }
        AstNode::ConditionalDot {
            condition,
            true_branch,
            ..
        } => {
            collect_ref_default_assignment_needs_inner(condition, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(true_branch, available, excluded, needs);
        }
        AstNode::WLoop {
            condition, body, ..
        } => {
            collect_ref_default_assignment_needs_inner(condition, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(body, available, excluded, needs);
        }
        AstNode::NLoop { count, body, .. } => {
            collect_ref_default_assignment_needs_inner(count, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(body, available, excluded, needs);
        }
        AstNode::UnpackAssignment { .. } => {
            unreachable!(
                "UnpackAssignment should have been resolved before collect_ref_default_assignment_needs"
            )
        }
        AstNode::FString { .. } => {
            unreachable!(
                "FString should have been resolved before collect_ref_default_assignment_needs"
            )
        }
    }
}

fn has_ctrl(node: &AstNode) -> bool {
    match node {
        AstNode::Break | AstNode::Continue | AstNode::Return(_) => true,
        AstNode::Debug { expr, .. } => has_ctrl(expr),
        AstNode::Pause { .. } => true,
        AstNode::Block(stmts) => stmts.iter().any(has_ctrl),
        AstNode::BlockExpr(stmts, _) => stmts.iter().any(has_ctrl),
        AstNode::Conditional {
            true_branch,
            false_branch,
            ..
        } => has_ctrl(true_branch) || false_branch.as_ref().is_some_and(|b| has_ctrl(b)),
        AstNode::WLoop { body, .. }
        | AstNode::NLoop { body, .. }
        | AstNode::Function { body, .. } => has_ctrl(body),
        AstNode::UnaryOp { operand, .. } => has_ctrl(operand),
        AstNode::Pipe { input, effect, .. } => has_ctrl(input) || has_ctrl(effect),
        AstNode::PipeTap { input, effect, .. } => has_ctrl(input) || has_ctrl(effect),
        AstNode::Postfix { object, items, .. } => has_ctrl(object) || items.iter().any(has_ctrl),
        AstNode::BinaryOp { left, right, .. } => has_ctrl(left) || has_ctrl(right),
        AstNode::ComparisonChain { first, rest } => {
            has_ctrl(first) || rest.iter().any(|(_, node)| has_ctrl(node))
        }
        AstNode::CallName { args, .. }
        | AstNode::CallAnonymous { args, .. }
        | AstNode::Cat(args)
        | AstNode::List(args) => args.iter().any(has_ctrl),
        AstNode::Dict(pairs) => pairs.iter().any(|(_, v)| has_ctrl(v)),
        AstNode::ConditionalChain { .. } => {
            unreachable!("ConditionalChain should have been resolved before compilation")
        }
        AstNode::ConditionalDot { .. } => {
            unreachable!("ConditionalDot should have been resolved before compilation")
        }
        AstNode::Index { object, index, .. } => has_ctrl(object) || has_ctrl(index),
        AstNode::Assignment { value, .. } | AstNode::OuterAssignment { value, .. } => {
            has_ctrl(value)
        }
        AstNode::IndexAssign {
            object,
            index,
            value,
            ..
        } => has_ctrl(object) || has_ctrl(index) || has_ctrl(value),
        AstNode::Range {
            start, end, step, ..
        } => has_ctrl(start) || has_ctrl(end) || step.as_ref().is_some_and(|s| has_ctrl(s)),
        AstNode::OuterVariable(_, _) => false,
        AstNode::Group { expr, .. } => has_ctrl(expr),
        _ => false,
    }
}

fn const_body_value(node: &AstNode) -> Option<Value> {
    match node {
        AstNode::Literal(value, ..) => Some(value.clone()),
        AstNode::Block(stmts) | AstNode::BlockExpr(stmts, _) => {
            stmts.last().and_then(const_body_value)
        }
        AstNode::Group { expr, .. } => const_body_value(expr),
        _ => None,
    }
}

fn pure_const_body(node: &AstNode) -> bool {
    if has_ctrl(node) {
        return false;
    }

    match node {
        AstNode::Literal(..) => true,
        AstNode::Block(stmts) | AstNode::BlockExpr(stmts, _) => stmts.iter().all(pure_const_body),
        AstNode::Group { expr, .. } => pure_const_body(expr),
        AstNode::UnpackAssignment { .. } => false,
        _ => false,
    }
}

fn replace_pipe_input(node: &AstNode, temp_name: &str) -> AstNode {
    match node {
        AstNode::PipeInput => AstNode::Variable(temp_name.to_string(), None),
        AstNode::MutatingIndex {
            object,
            index,
            span,
        } => AstNode::MutatingIndex {
            object: Box::new(replace_pipe_input(object, temp_name)),
            index: Box::new(replace_pipe_input(index, temp_name)),
            span: *span,
        },
        AstNode::MutatingIndexAssign {
            object,
            index,
            value,
            span,
        } => AstNode::MutatingIndexAssign {
            object: Box::new(replace_pipe_input(object, temp_name)),
            index: Box::new(replace_pipe_input(index, temp_name)),
            value: Box::new(replace_pipe_input(value, temp_name)),
            span: *span,
        },
        AstNode::Error(..)
        | AstNode::Literal(..)
        | AstNode::Variable(_, _)
        | AstNode::OuterVariable(_, _)
        | AstNode::Ellipsis
        | AstNode::Break
        | AstNode::Continue => node.clone(),
        AstNode::BinaryOp {
            left,
            operator,
            right,
        } => AstNode::BinaryOp {
            left: Box::new(replace_pipe_input(left, temp_name)),
            operator: *operator,
            right: Box::new(replace_pipe_input(right, temp_name)),
        },
        AstNode::ComparisonChain { first, rest } => AstNode::ComparisonChain {
            first: Box::new(replace_pipe_input(first, temp_name)),
            rest: rest
                .iter()
                .map(|(op, node)| (*op, replace_pipe_input(node, temp_name)))
                .collect(),
        },
        AstNode::UnaryOp {
            operator,
            operand,
            span,
        } => AstNode::UnaryOp {
            operator: *operator,
            operand: Box::new(replace_pipe_input(operand, temp_name)),
            span: *span,
        },
        AstNode::Range {
            start,
            end,
            step,
            inclusive,
        } => AstNode::Range {
            start: Box::new(replace_pipe_input(start, temp_name)),
            end: Box::new(replace_pipe_input(end, temp_name)),
            step: step
                .as_ref()
                .map(|step| Box::new(replace_pipe_input(step, temp_name))),
            inclusive: *inclusive,
        },
        AstNode::Assignment {
            name,
            op,
            value,
            span,
            name_span,
        } => AstNode::Assignment {
            name: name.clone(),
            op: *op,
            value: Box::new(replace_pipe_input(value, temp_name)),
            span: *span,
            name_span: *name_span,
        },
        AstNode::OuterAssignment {
            name,
            op,
            value,
            span,
            name_span,
        } => AstNode::OuterAssignment {
            name: name.clone(),
            op: *op,
            value: Box::new(replace_pipe_input(value, temp_name)),
            span: *span,
            name_span: *name_span,
        },
        AstNode::Cat(items) => AstNode::Cat(
            items
                .iter()
                .map(|item| replace_pipe_input(item, temp_name))
                .collect(),
        ),
        AstNode::List(items) => AstNode::List(
            items
                .iter()
                .map(|item| replace_pipe_input(item, temp_name))
                .collect(),
        ),
        AstNode::Dict(pairs) => AstNode::Dict(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), replace_pipe_input(v, temp_name)))
                .collect(),
        ),

        AstNode::Postfix {
            object,
            items,
            explicit_call,
            depth,
            span,
        } => AstNode::Postfix {
            object: Box::new(replace_pipe_input(object, temp_name)),
            items: items
                .iter()
                .map(|item| replace_pipe_input(item, temp_name))
                .collect(),
            explicit_call: *explicit_call,
            depth: *depth,
            span: *span,
        },
        AstNode::Pipe {
            input,
            effect,
            kind,
            span,
        } => AstNode::Pipe {
            input: Box::new(replace_pipe_input(input, temp_name)),
            effect: Box::new(replace_pipe_input(effect, temp_name)),
            kind: *kind,
            span: *span,
        },
        AstNode::PipeTap {
            input,
            effect,
            span,
        } => AstNode::PipeTap {
            input: Box::new(replace_pipe_input(input, temp_name)),
            effect: Box::new(replace_pipe_input(effect, temp_name)),
            span: *span,
        },
        AstNode::CallName {
            name,
            args,
            span,
            name_span,
        } => AstNode::CallName {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| replace_pipe_input(arg, temp_name))
                .collect(),
            span: *span,
            name_span: *name_span,
        },
        AstNode::CallAnonymous { object, args, span } => AstNode::CallAnonymous {
            object: Box::new(replace_pipe_input(object, temp_name)),
            args: args
                .iter()
                .map(|arg| replace_pipe_input(arg, temp_name))
                .collect(),
            span: *span,
        },
        AstNode::Index {
            object,
            index,
            span,
        } => AstNode::Index {
            object: Box::new(replace_pipe_input(object, temp_name)),
            index: Box::new(replace_pipe_input(index, temp_name)),
            span: *span,
        },
        AstNode::IndexAssign {
            object,
            index,
            op,
            value,
            span,
        } => AstNode::IndexAssign {
            object: Box::new(replace_pipe_input(object, temp_name)),
            index: Box::new(replace_pipe_input(index, temp_name)),
            op: *op,
            value: Box::new(replace_pipe_input(value, temp_name)),
            span: *span,
        },
        AstNode::Function {
            params,
            ref_capture,
            body,
        } => AstNode::Function {
            params: params.clone(),
            ref_capture: *ref_capture,
            body: Box::new(replace_pipe_input(body, temp_name)),
        },
        AstNode::Conditional {
            condition,
            true_branch,
            false_branch,
            span,
        } => AstNode::Conditional {
            condition: Box::new(replace_pipe_input(condition, temp_name)),
            true_branch: Box::new(replace_pipe_input(true_branch, temp_name)),
            false_branch: false_branch
                .as_ref()
                .map(|branch| Box::new(replace_pipe_input(branch, temp_name))),
            span: *span,
        },
        AstNode::WLoop {
            condition,
            body,
            span,
        } => AstNode::WLoop {
            condition: Box::new(replace_pipe_input(condition, temp_name)),
            body: Box::new(replace_pipe_input(body, temp_name)),
            span: *span,
        },
        AstNode::NLoop { count, body, span } => AstNode::NLoop {
            count: Box::new(replace_pipe_input(count, temp_name)),
            body: Box::new(replace_pipe_input(body, temp_name)),
            span: *span,
        },
        AstNode::ConditionalChain { .. } => {
            unreachable!("ConditionalChain should have been resolved before compilation")
        }
        AstNode::ConditionalDot { .. } => {
            unreachable!("ConditionalDot should have been resolved before compilation")
        }
        AstNode::Return(expr) => AstNode::Return(
            expr.as_ref()
                .map(|expr| Box::new(replace_pipe_input(expr, temp_name))),
        ),
        AstNode::Assert { expr, span } => AstNode::Assert {
            expr: Box::new(replace_pipe_input(expr, temp_name)),
            span: *span,
        },
        AstNode::Debug { expr, span } => AstNode::Debug {
            expr: Box::new(replace_pipe_input(expr, temp_name)),
            span: *span,
        },
        AstNode::Pause { expr, span } => AstNode::Pause {
            expr: expr
                .as_ref()
                .map(|expr| Box::new(replace_pipe_input(expr, temp_name))),
            span: *span,
        },
        AstNode::Try(expr) => AstNode::Try(Box::new(replace_pipe_input(expr, temp_name))),
        AstNode::Block(stmts) => AstNode::Block(
            stmts
                .iter()
                .map(|stmt| replace_pipe_input(stmt, temp_name))
                .collect(),
        ),
        AstNode::BlockExpr(stmts, span) => AstNode::BlockExpr(
            stmts
                .iter()
                .map(|stmt| replace_pipe_input(stmt, temp_name))
                .collect(),
            *span,
        ),
        AstNode::Group { expr, span } => AstNode::Group {
            expr: Box::new(replace_pipe_input(expr, temp_name)),
            span: *span,
        },
        AstNode::NamedArg { name, value, span } => AstNode::NamedArg {
            name: name.clone(),
            value: Box::new(replace_pipe_input(value, temp_name)),
            span: *span,
        },
        AstNode::UnpackAssignment { .. } => {
            unreachable!("UnpackAssignment should have been resolved before replace_pipe_input")
        }
        AstNode::FString { .. } => {
            unreachable!("FString should have been resolved before replace_pipe_input")
        }
    }
}

type ParamList<'a> = Option<&'a [Parameter]>;

fn function_capture_needs(
    body: &AstNode,
    params: ParamList<'_>,
    ref_capture: bool,
    defining_name: Option<&str>,
) -> CaptureNeeds {
    let mut locals = IndexSet::new();
    if let Some(params) = params {
        for p in params {
            locals.insert(p.name().to_string());
        }
    } else {
        locals.insert("x".to_string());
        locals.insert("y".to_string());
        locals.insert("z".to_string());
    }

    let mut needs = CaptureNeeds::default();
    collect_capture_needs(body, &mut locals, &mut needs, ref_capture, defining_name);
    // Scan default expressions in named params for captures too
    if let Some(params) = params {
        for p in params {
            if let Parameter::Named {
                default: Some(default_expr),
                ..
            } = p
            {
                collect_capture_needs(
                    default_expr,
                    &mut locals,
                    &mut needs,
                    ref_capture,
                    defining_name,
                );
            }
        }
    }
    needs
}

pub(crate) fn function_ref_capture_names(
    body: &AstNode,
    params: ParamList<'_>,
    ref_capture: bool,
    defining_name: Option<&str>,
) -> IndexSet<String> {
    function_capture_needs(body, params, ref_capture, defining_name).by_ref
}

fn collect_capture_needs(
    node: &AstNode,
    locals: &mut IndexSet<String>,
    needs: &mut CaptureNeeds,
    ref_capture: bool,
    defining_name: Option<&str>,
) {
    match node {
        AstNode::Error(..)
        | AstNode::Literal(..)
        | AstNode::Ellipsis
        | AstNode::PipeInput
        | AstNode::Break
        | AstNode::Continue => {}
        AstNode::Variable(name, _) => {
            if !scope_has(locals, name) && defining_name != Some(name.as_str()) {
                if ref_capture {
                    needs.by_ref.insert(name.clone());
                } else {
                    needs.by_value.insert(name.clone());
                }
            }
        }
        AstNode::OuterVariable(name, _) => {
            needs.by_ref.insert(name.clone());
        }
        AstNode::MutatingIndex { object, index, .. } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            collect_capture_needs(index, locals, needs, ref_capture, defining_name);
        }
        AstNode::MutatingIndexAssign {
            object,
            index,
            value,
            ..
        } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            collect_capture_needs(index, locals, needs, ref_capture, defining_name);
            collect_capture_needs(value, locals, needs, ref_capture, defining_name);
        }
        AstNode::BinaryOp { left, right, .. } => {
            collect_capture_needs(left, locals, needs, ref_capture, defining_name);
            collect_capture_needs(right, locals, needs, ref_capture, defining_name);
        }
        AstNode::ComparisonChain { first, rest } => {
            collect_capture_needs(first, locals, needs, ref_capture, defining_name);
            for (_, node) in rest {
                collect_capture_needs(node, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::UnaryOp { operand, .. } => {
            collect_capture_needs(operand, locals, needs, ref_capture, defining_name);
        }
        AstNode::Range {
            start, end, step, ..
        } => {
            collect_capture_needs(start, locals, needs, ref_capture, defining_name);
            collect_capture_needs(end, locals, needs, ref_capture, defining_name);
            if let Some(step) = step {
                collect_capture_needs(step, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Assignment {
            name, op, value, ..
        } => {
            if op.is_none()
                && let AstNode::Function {
                    params,
                    ref_capture: child_ref_capture,
                    body,
                } = &**value
            {
                locals.insert(name.clone());
                let nested_needs =
                    function_capture_needs(body, params.as_deref(), *child_ref_capture, Some(name));
                merge_child_capture_needs(needs, locals, nested_needs, defining_name);
                return;
            }

            if op.is_some() && !scope_has(locals, name) && defining_name != Some(name.as_str()) {
                if ref_capture {
                    needs.by_ref.insert(name.clone());
                } else {
                    needs.by_value.insert(name.clone());
                }
            }
            collect_capture_needs(value, locals, needs, ref_capture, defining_name);
            locals.insert(name.clone());
        }
        AstNode::OuterAssignment { name, value, .. } => {
            collect_capture_needs(value, locals, needs, ref_capture, defining_name);
            needs.by_ref.insert(name.clone());
        }
        AstNode::Cat(items) | AstNode::List(items) => {
            for item in items {
                collect_capture_needs(item, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Dict(pairs) => {
            for (_, value) in pairs {
                collect_capture_needs(value, locals, needs, ref_capture, defining_name);
            }
        }

        AstNode::Postfix { object, items, .. } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            for item in items {
                collect_capture_needs(item, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Pipe { input, effect, .. } => {
            collect_capture_needs(input, locals, needs, ref_capture, defining_name);
            collect_capture_needs(effect, locals, needs, ref_capture, defining_name);
        }
        AstNode::PipeTap { input, effect, .. } => {
            collect_capture_needs(input, locals, needs, ref_capture, defining_name);
            collect_capture_needs(effect, locals, needs, ref_capture, defining_name);
        }
        AstNode::CallName { name, args, .. } => {
            if !scope_has(locals, name) && defining_name != Some(name.as_str()) {
                if ref_capture {
                    needs.by_ref.insert(name.clone());
                } else {
                    needs.by_value.insert(name.clone());
                }
            }
            for arg in args {
                collect_capture_needs(arg, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::CallAnonymous { object, args, .. } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            for arg in args {
                collect_capture_needs(arg, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Index { object, index, .. } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            collect_capture_needs(index, locals, needs, ref_capture, defining_name);
        }
        AstNode::IndexAssign {
            object,
            index,
            value,
            ..
        } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            collect_capture_needs(index, locals, needs, ref_capture, defining_name);
            collect_capture_needs(value, locals, needs, ref_capture, defining_name);
        }
        AstNode::NamedArg { name: _, value, .. } => {
            collect_capture_needs(value, locals, needs, ref_capture, defining_name);
        }
        AstNode::Function {
            params,
            ref_capture: child_ref_capture,
            body,
        } => {
            let nested_needs =
                function_capture_needs(body, params.as_deref(), *child_ref_capture, None);
            merge_child_capture_needs(needs, locals, nested_needs, defining_name);
        }
        AstNode::Conditional {
            condition,
            true_branch,
            false_branch,
            ..
        } => {
            collect_capture_needs(condition, locals, needs, ref_capture, defining_name);
            collect_capture_needs(true_branch, locals, needs, ref_capture, defining_name);
            if let Some(false_branch) = false_branch {
                collect_capture_needs(false_branch, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::ConditionalChain { .. } => {
            unreachable!("ConditionalChain should have been resolved before compilation")
        }
        AstNode::ConditionalDot { .. } => {
            unreachable!("ConditionalDot should have been resolved before compilation")
        }
        AstNode::WLoop {
            condition, body, ..
        } => {
            collect_capture_needs(condition, locals, needs, ref_capture, defining_name);
            collect_capture_needs(body, locals, needs, ref_capture, defining_name);
        }
        AstNode::NLoop { count, body, .. } => {
            collect_capture_needs(count, locals, needs, ref_capture, defining_name);
            locals.insert("_n".to_string());
            collect_capture_needs(body, locals, needs, ref_capture, defining_name);
        }
        AstNode::Return(expr) => {
            if let Some(expr) = expr {
                collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Assert { expr, .. } => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::Debug { expr, .. } => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::Pause { expr, .. } => {
            if let Some(expr) = expr {
                collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Try(expr) => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::Block(stmts) | AstNode::BlockExpr(stmts, _) => {
            for stmt in stmts {
                collect_capture_needs(stmt, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Group { expr, .. } => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::UnpackAssignment { .. } => {
            unreachable!("UnpackAssignment should have been resolved before collect_capture_needs")
        }
        AstNode::FString { .. } => {
            unreachable!("FString should have been resolved before collect_capture_needs")
        }
    }
}

fn merge_child_capture_needs(
    needs: &mut CaptureNeeds,
    locals: &IndexSet<String>,
    nested_needs: CaptureNeeds,
    defining_name: Option<&str>,
) {
    for name in nested_needs.by_value {
        if scope_has(locals, &name) || defining_name == Some(name.as_str()) {
            continue;
        }
        needs.by_value.insert(name);
    }

    for name in nested_needs.by_ref {
        if scope_has(locals, &name) {
            continue;
        }
        needs.by_ref.insert(name);
    }
}

fn scope_has(locals: &IndexSet<String>, name: &str) -> bool {
    locals.iter().any(|local| local == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::Lexer;
    use crate::parse::resolve::Resolver;
    use crate::parse::{Parser, fold};

    fn compile_source_with_spans(src: &str) -> (Vec<Instruction>, Vec<Option<(usize, usize)>>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        let ast = fold::fold(ast);
        let mut compiler = Compiler::new();
        compiler.compile(&ast).expect("compile");
        compiler.propagate_constants();
        compiler.rewrite_tail_calls();
        (compiler.instructions, compiler.dbg_pc_spans)
    }

    fn compile_source(src: &str) -> Vec<Instruction> {
        compile_source_with_spans(src).0
    }

    fn compile_source_err(src: &str) -> WqError {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        let ast = fold::fold(ast);
        let mut compiler = Compiler::new();
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.set_source(src.to_string());
        compiler.set_stmt_spans(parser.stmt_spans_top().to_vec());
        compiler.compile(&ast).expect_err("expected compile error")
    }

    fn builtin_id(name: &str) -> u16 {
        crate::builtins::Builtins::new()
            .get_id(name)
            .unwrap_or_else(|| panic!("missing builtin {name}")) as u16
    }

    fn compiled_function_in(insts: &[Instruction]) -> Arc<FunctionData> {
        for inst in insts {
            if let Instruction::LoadConst(value) = inst
                && let Value::CompiledFunction(func) = value.as_ref()
            {
                return func.clone();
            }
        }
        panic!("expected compiled function");
    }

    fn first_closure_payload(insts: &[Instruction]) -> &crate::vm::inst::ClosurePayload {
        for inst in insts {
            if let Instruction::LoadClosure(payload) = inst {
                return payload.as_ref();
            }
        }
        panic!("expected closure payload");
    }

    fn slot_named(names: &[String], name: &str) -> u16 {
        names
            .iter()
            .position(|local| local == name)
            .expect("expected local slot") as u16
    }

    #[test]
    fn pause_with_operand_compiles_as_value_transparent_probe() {
        let insts = compile_source("@p 1");

        assert_eq!(insts.len(), 2);
        assert!(matches!(&insts[0], Instruction::LoadConst(value) if **value == Value::Int(1)));
        assert!(matches!(insts[1], Instruction::Pause));
    }

    #[test]
    fn bare_pause_allows_trailing_comment() {
        let insts = compile_source("@p /* probe */; 1");

        assert!(insts.iter().any(|inst| matches!(inst, Instruction::Pause)));
    }

    #[test]
    fn bare_pause_pipe_rhs_loads_pipe_value_before_pause() {
        let insts = compile_source("1|@p|+[2]");

        assert!(
            insts.windows(2).any(|pair| matches!(
                (&pair[0], &pair[1]),
                (Instruction::LoadVar(name), Instruction::Pause)
                    if name.as_ref().starts_with("--vm-pipe-tap-")
            ) || matches!(
                (&pair[0], &pair[1]),
                (Instruction::LoadConst(value), Instruction::Pause) if **value == Value::Int(1)
            )),
            "expected pipe value to be loaded before @p: {insts:#?}"
        );
    }

    #[test]
    fn bare_debug_pipe_rhs_loads_pipe_value_before_debug() {
        let insts = compile_source("1|@d|+[2]");

        assert!(
            insts.windows(3).any(|triple| matches!(
                (&triple[0], &triple[1], &triple[2]),
                (Instruction::TraceBegin, Instruction::LoadVar(name), Instruction::Debug)
                    if name.as_ref().starts_with("--vm-pipe-tap-")
            ) || matches!(
                (&triple[0], &triple[1], &triple[2]),
                (Instruction::TraceBegin, Instruction::LoadConst(value), Instruction::Debug)
                    if **value == Value::Int(1)
            )),
            "expected pipe value to be loaded inside @d trace: {insts:#?}"
        );
    }

    #[test]
    fn debug_trace_keeps_symbol_loads_after_constprop() {
        let insts = compile_source("a:1;b:a+2;@d b;@d (b*3)");

        assert!(
            insts.windows(3).any(|triple| matches!(
                (&triple[0], &triple[1], &triple[2]),
                (Instruction::TraceBegin, Instruction::LoadVar(name), Instruction::Debug)
                    if name.as_ref() == "b"
            )),
            "expected @d b to keep a symbol load: {insts:#?}"
        );
        assert!(
            insts.windows(4).any(|quad| matches!(
                (&quad[0], &quad[1], &quad[2], &quad[3]),
                (
                    Instruction::TraceBegin,
                    Instruction::LoadVar(name),
                    Instruction::BinaryOp(data),
                    Instruction::Debug,
                ) if name.as_ref() == "b"
                    && data.op == BinaryOperator::Multiply
                    && matches!(data.left, Operand::Stack)
                    && matches!(&data.right, Operand::Const(value) if **value == Value::Int(3))
            )),
            "expected @d (b*3) to keep b as a traced operand: {insts:#?}"
        );
    }

    #[test]
    fn closure_only_captures_used_local_slots() {
        let top = compile_source("f:{a:1;b:2;g:{a;'a};g}");
        let outer = compiled_function_in(&top);
        let a_slot = slot_named(outer.dbg_local_names.as_deref().expect("local names"), "a");
        let inner = first_closure_payload(outer.instructions.as_ref());

        assert_eq!(
            inner.captures,
            vec![Capture::Local(a_slot), Capture::LocalShared(a_slot)]
        );
    }

    #[test]
    fn nested_apostrophe_need_is_forwarded_without_extra_locals() {
        let top = compile_source("f:{a:1;b:2;g:{h:{'a};h};g}");
        let outer = compiled_function_in(&top);
        let a_slot = slot_named(outer.dbg_local_names.as_deref().expect("local names"), "a");
        let g = first_closure_payload(outer.instructions.as_ref());
        let h = first_closure_payload(g.instructions.as_ref());

        assert_eq!(g.captures, vec![Capture::LocalShared(a_slot)]);
        assert_eq!(h.captures, vec![Capture::Outer(0)]);
    }

    // #[test]
    // fn captured_index_reads_use_index_load_capture() {
    //     let top = compile_source("f:{a:(1;2;3);g:{'a'[1]};g}");
    //     let outer = compiled_function_in(&top);
    //     let inner = first_closure_payload(outer.instructions.as_ref());

    //     assert!(
    //         dbg!(&inner.instructions)
    //             .iter()
    //             .any(|inst| matches!(inst, Instruction::IndexLoadCapture(_)))
    //     );
    //     assert!(!inner.instructions.windows(2).any(|pair| matches!(
    //         (&pair[0], &pair[1]),
    //         (Instruction::LoadCapture(_), Instruction::Index)
    //     )));
    // }

    #[test]
    fn captured_augmented_index_reads_use_index_load_capture() {
        let top = compile_source("f:{a:(1;2;3);i:1;g:{'a['i]+:1};g}");
        let outer = compiled_function_in(&top);
        let inner = first_closure_payload(outer.instructions.as_ref());

        assert!(
            inner
                .instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::IndexLoadCapture(_)))
        );
        assert!(!inner.instructions.windows(2).any(|pair| matches!(
            (&pair[0], &pair[1]),
            (Instruction::LoadCapture(_), Instruction::Index)
        )));
    }

    #[test]
    fn nested_closure_stmt_spans_follow_source_order() {
        let src = "t:{k:3\n  1/0}\n\na:{\n  N[3\n    N[1\n      {t}\n    ]\n  ]\n}";
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let mut parser = Parser::new(tokens, src.to_string());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::new();
        let ast = resolver.resolve(ast);
        let ast = fold::fold(ast);
        let mut compiler = Compiler::new();
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.set_stmt_spans(parser.stmt_spans_top().to_vec());
        compiler.compile(&ast).expect("compile");

        let outer = compiler
            .instructions
            .iter()
            .filter_map(|inst| match inst {
                Instruction::LoadConst(value) => match value.as_ref() {
                    Value::CompiledFunction(func) => Some(func.clone()),
                    _ => None,
                },
                _ => None,
            })
            .nth(1)
            .expect("outer function");
        let nested = first_closure_payload(outer.instructions.as_ref());
        let span = nested.dbg_stmt_spans.first().copied().expect("nested span");
        let (line, col) = crate::wqerror::byte_to_line_col(src, span.0);

        assert_eq!((line, col), (7, 8));
    }

    #[test]
    fn ref_rebinding_function_name_uses_dynamic_call_or_index() {
        let top = compile_source("f:{'f:1};f[0];f[0]");

        let dynamic_dispatches = top
            .iter()
            .filter(|inst| matches!(inst, Instruction::PostfixVar(name, 1) if name.as_ref() == "f"))
            .count();

        assert_eq!(dynamic_dispatches, 2);
        assert!(
            !top.iter()
                .any(|inst| matches!(inst, Instruction::CallUser(name, 1) if name.as_ref() == "f"))
        );
    }

    #[test]
    fn constant_tag_method_postfix_uses_method_dispatch() {
        let top = compile_source("d:(`f:{[x]x+1});d[`f][2];d[`f][]");

        assert!(top.iter().any(|inst| matches!(
            inst,
            Instruction::PostfixMethodVar(receiver, method, 1)
                if receiver.as_ref() == "d" && method.as_ref() == "f"
        )));
        assert!(top.iter().any(|inst| matches!(
            inst,
            Instruction::CallMethodVar(receiver, method, 0)
                if receiver.as_ref() == "d" && method.as_ref() == "f"
        )));
    }

    #[test]
    fn depth_modifier_compiles_pipe_call_as_builtin_depth_arg() {
        let top = compile_source("(1;2)|has?@1[2]");
        let has_id = builtin_id("has?");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::CallBuiltinId(id, argc)
                    if *id == has_id && *argc == 3)),
            "expected has? call with inserted depth argument: {top:#?}",
        );
    }

    #[test]
    fn depth_modifier_uses_builtin_metadata_for_aliases() {
        let top = compile_source("(1;2)|M@1[{x+1}]");
        let map_alias_id = builtin_id("M");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::CallBuiltinId(id, argc)
                    if *id == map_alias_id && *argc == 3)),
            "expected M alias call with inserted depth argument: {top:#?}",
        );
    }

    #[test]
    fn depth_modifier_adds_findw_default_threshold() {
        let top = compile_source("(1;2)|findw@2[{x=2}]");
        let findw_id = builtin_id("findw");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::CallBuiltinId(id, argc)
                    if *id == findw_id && *argc == 4)),
            "expected findw call with threshold and depth arguments: {top:#?}",
        );
    }

    #[test]
    fn depth_modifier_rejects_non_depth_builtin() {
        let err = compile_source_err("echo@1[2]");
        let display = err.to_string();
        assert!(
            display.contains("depth-aware builtins"),
            "unexpected error: {display}",
        );
    }

    #[test]
    fn optimized_index_load_keeps_full_bracket_span() {
        let src = "xs:(1;2;3); xs[0]";
        let (insts, pc_spans) = compile_source_with_spans(src);
        let index_pc = insts
            .iter()
            .position(
                |inst| matches!(inst, Instruction::IndexLoadVar(name) if name.as_ref() == "xs"),
            )
            .expect("IndexLoadVar");
        let start = src.find("xs[0]").expect("index source");

        assert_eq!(pc_spans[index_pc], Some((start, start + "xs[0]".len())));
    }

    #[test]
    fn const_propagation_folds_local_values() {
        let top = compile_source("f:{a:1;b:a+2;b}");
        let func = compiled_function_in(&top);

        assert!(
            func.instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::LoadConst(v) if **v == Value::Int(3))),
            "expected propagated constant 3 in {:#?}",
            func.instructions
        );
        assert!(
            !func.instructions.iter().any(|inst| matches!(
                inst,
                Instruction::BinaryOp(data)
                    if data.op == BinaryOperator::Add && matches!(data.left, Operand::Local(_))
            )),
            "local operand should have been propagated in {:#?}",
            func.instructions
        );
    }

    #[test]
    fn const_propagation_meets_branch_assignments() {
        let top = compile_source("f:{a:1;$[x;a:2;0];a}");
        let func = compiled_function_in(&top);
        let a_slot = slot_named(func.dbg_local_names.as_deref().expect("local names"), "a");

        assert!(
            func.instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::LoadLocal(slot) if *slot == a_slot)),
            "a can be 1 or 2 after the branch, so it must remain a load: {:#?}",
            func.instructions
        );
    }

    #[test]
    fn const_propagation_invalidates_locals_across_calls() {
        let top = compile_source("f:{a:1;g:{'a:2};g[];a}");
        let func = compiled_function_in(&top);
        let a_slot = slot_named(func.dbg_local_names.as_deref().expect("local names"), "a");

        assert!(
            func.instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::LoadLocal(slot) if *slot == a_slot)),
            "closure call can mutate a by reference, so a must remain dynamic: {:#?}",
            func.instructions
        );
    }

    #[test]
    fn const_propagation_seeds_global_value_captures() {
        let top = compile_source("a:1;f:{a+2};f");
        let func = first_closure_payload(&top);

        assert!(
            func.instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::LoadConst(v) if **v == Value::Int(3))),
            "expected global capture to fold to 3 in {:#?}",
            func.instructions
        );
        assert!(
            !func.instructions.iter().any(|inst| matches!(
                inst,
                Instruction::BinaryOp(data)
                    if matches!(data.left, Operand::Capture(_))
                        || matches!(data.right, Operand::Capture(_))
            )),
            "global capture should have been replaced in {:#?}",
            func.instructions
        );
    }

    #[test]
    fn const_propagation_clears_global_facts_across_calls() {
        let top = compile_source("a:1;echo 0;f:{a+2};f");
        let func = first_closure_payload(&top);

        assert!(
            func.instructions.iter().any(|inst| matches!(
                inst,
                Instruction::BinaryOp(data)
                    if matches!(data.left, Operand::Capture(_))
                        || matches!(data.right, Operand::Capture(_))
            )),
            "call can mutate globals, so captured a must remain dynamic: {:#?}",
            func.instructions
        );
    }

    #[test]
    fn last_function_statement_reports_its_own_span() {
        let err = compile_source_err("f:{x:1;W[true;x:2]x*:2}");
        let display = err.to_string();
        assert!(err.span.is_some(), "expected span");
        assert!(display.contains("at ?:1:8"), "display was: {display}");
    }

    // #[test]
    // fn multiline_statement_underline_stops_at_line_end() {
    //     let err = compile_source_err("f:{x:1;\n  W[true;x:2]x*:2}");
    //     let display = err.to_string();
    //     assert!(err.span.is_some(), "expected span");
    //     assert!(display.contains("at ?:2:3"), "display was: {display}");
    //     assert!(
    //         display.contains("  W[true;x:2]x*:2}"),
    //         "display was: {display}"
    //     );
    // }
}
