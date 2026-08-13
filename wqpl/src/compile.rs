mod cp;
mod fuse;
mod tce;

use std::collections::BTreeSet;
use std::convert::TryFrom;
use std::sync::Arc;

use indexmap::{IndexMap, IndexSet};

use crate::ast::{AstNode, AstSpan, BinaryOperator, BoolOperator, Parameter};
use crate::builtins::{BuiltinDepthSugar, Builtins};
use crate::value::func::FunctionData;
use crate::value::unpack::{UnpackPathSegment, extract_path as extract_unpack_path};
use crate::value::{Value, WqResult};
use crate::vm::inst::{
    Capture, DebugStmtMark, ImportData, Instruction, MutationOp, Operand, StoreTarget, UnpackPlan,
};
use crate::wqerror::{WqError, WqErrorType};

#[derive(Clone, Copy)]
enum IndexPathRoot<'a> {
    Variable(&'a str),
    OuterVariable(&'a str, AstSpan),
}

struct IndexPathTarget<'a> {
    root: IndexPathRoot<'a>,
    indices: Vec<&'a AstNode>,
}

struct PlannedUnpack {
    paths: Vec<Box<[UnpackPathSegment]>>,
    writes: Vec<PlannedUnpackWrite>,
}

struct PlannedUnpackWrite {
    target: AstNode,
    path_index: usize,
}

enum IndexArgPlan {
    Temps(Vec<String>),
    ConstKey(Value),
}

impl IndexArgPlan {
    fn argc(&self) -> usize {
        match self {
            Self::Temps(names) => names.len(),
            Self::ConstKey(_) => 1,
        }
    }
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
    // Pretty error reporting: byte offset of this compiler's source fragment
    src_base_offset: usize,
    // Pretty error reporting: source file path / label
    src_path: Option<String>,
    import_origin: Option<String>,
    // Pretty error reporting: current statement spans (byte offsets) and cursor
    cur_stmt_spans: Vec<(usize, usize)>,
    cur_stmt_idx: usize,
    current_stmt_span: Option<(usize, usize)>,
    pub(crate) dbg_pc_spans: Vec<Option<(usize, usize)>>,
    pub(crate) dbg_stmt_marks: Vec<DebugStmtMark>,
    pub(crate) has_runtime_debug: bool,
    trace_symbol_operands: bool,
    module_root: bool,
    isolated_module: bool,
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
            src_base_offset: 0,
            src_path: None,
            import_origin: None,
            cur_stmt_spans: Vec::new(),
            cur_stmt_idx: 0,
            current_stmt_span: None,
            dbg_pc_spans: Vec::new(),
            dbg_stmt_marks: Vec::new(),
            has_runtime_debug: false,
            trace_symbol_operands: false,
            module_root: false,
            isolated_module: false,
        }
    }

    pub(crate) fn compile(&mut self, node: &AstNode) -> WqResult<()> {
        self.compile_stmt_sequence(node, true)
    }

    pub(crate) fn compile_module_initializer(mut self, node: &AstNode) -> WqResult<FunctionData> {
        let stmt_spans = self.cur_stmt_spans.clone();
        self.fn_depth = 1;
        self.module_root = true;
        self.isolated_module = true;
        self.compile(node)?;
        if let Some(Capture::Global(name, span)) = self.captures.first() {
            return Err(self.error_at(
                WqErrorType::NotBound,
                *span,
                format!("'{name}' has not been bound to a value"),
            ));
        }
        if let Some((name, span)) = self.first_ambient_global(&self.instructions) {
            return Err(self.error_at(
                WqErrorType::NotBound,
                span,
                format!("'{name}' has not been bound to a value"),
            ));
        }
        self.instructions.push(Instruction::Return);
        self.propagate_constants_with_globals(&crate::vm::GlobalMap::default());
        self.rewrite_tail_calls();
        self.fuse();

        let locals = self.local_count();
        let local_names: Arc<[String]> = Arc::from(self.local_names_vec());
        let instructions: Arc<[Instruction]> = self.instructions.into();
        let mut pc_spans = self.dbg_pc_spans;
        pc_spans.resize(instructions.len(), None);
        Ok(FunctionData {
            params: Some(Arc::from([])),
            named_params: None,
            locals,
            isolated_module: self.isolated_module,
            instructions,
            dbg_chunk: None,
            dbg_stmt_spans: Some(Arc::from(stmt_spans)),
            dbg_source_base_offset: self.src_base_offset,
            dbg_pc_spans: Some(Arc::from(pc_spans)),
            dbg_stmt_marks: Some(Arc::from(self.dbg_stmt_marks)),
            dbg_local_names: Some(local_names),
            dbg_provenance: None,
        })
    }

    fn first_ambient_global(&self, instructions: &[Instruction]) -> Option<(String, AstSpan)> {
        for instruction in instructions {
            let direct = match instruction {
                Instruction::LoadVar(name)
                | Instruction::StoreVar(name)
                | Instruction::StoreVarKeep(name)
                | Instruction::IndexLoadVar(name)
                | Instruction::IndexAssignVar(name)
                | Instruction::IndexAssignVarDrop(name) => {
                    (!self.builtins.has_function(name)).then(|| (name.to_string(), None))
                }
                Instruction::IndexManyLoadVar(name, _)
                | Instruction::IndexManyAssignVar(name, _)
                | Instruction::IndexManyAssignVarDrop(name, _) => {
                    (!self.builtins.has_function(name)).then(|| (name.to_string(), None))
                }
                Instruction::LoadCallTarget(operand) => self.ambient_global_from_operand(operand),
                Instruction::UnaryOp(data) => self.ambient_global_from_operand(&data.operand),
                Instruction::BinaryOp(data) => self
                    .ambient_global_from_operand(&data.left)
                    .or_else(|| self.ambient_global_from_operand(&data.right)),
                Instruction::CatAssign(data) => {
                    let target = match &data.target {
                        StoreTarget::Var(name) if !self.builtins.has_function(name) => {
                            Some((name.to_string(), None))
                        }
                        _ => None,
                    };
                    target.or_else(|| self.ambient_global_from_operand(&data.right))
                }
                Instruction::JumpIfCmpFalse(data) => self
                    .ambient_global_from_operand(&data.left)
                    .or_else(|| self.ambient_global_from_operand(&data.right)),
                Instruction::IndexMutate {
                    target: StoreTarget::Var(name),
                    ..
                } => Some((name.to_string(), None)),
                _ => None,
            };
            if direct.is_some() {
                return direct;
            }

            match instruction {
                Instruction::LoadClosure(payload) => {
                    if let Some(Capture::Global(name, span)) = payload
                        .captures
                        .iter()
                        .find(|capture| matches!(capture, Capture::Global(..)))
                    {
                        return Some((name.clone(), *span));
                    }
                    if let Some(found) = self.first_ambient_global(&payload.instructions) {
                        return Some(found);
                    }
                }
                Instruction::LoadConst(value) => {
                    if let Value::CompiledFunction(function) = value.as_ref()
                        && let Some(found) = self.first_ambient_global(&function.instructions)
                    {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn ambient_global_from_operand(&self, operand: &Operand) -> Option<(String, AstSpan)> {
        match operand {
            Operand::Var(name) if !self.builtins.has_function(name) => {
                Some((name.to_string(), None))
            }
            _ => None,
        }
    }

    fn compile_expr(&mut self, node: &AstNode) -> WqResult<()> {
        let start = self.instructions.len();
        let result = self.compile_in_context(node, true);
        let end = self.instructions.len();
        self.fill_span_range(start, end, Self::ast_node_span(node));
        result
    }

    fn compile_unpack_assignment(
        &mut self,
        lhs: &[AstNode],
        op: Option<BinaryOperator>,
        rhs: &AstNode,
        span: AstSpan,
    ) -> WqResult<()> {
        let mut plan = PlannedUnpack {
            paths: Vec::new(),
            writes: Vec::new(),
        };
        if let [AstNode::DictUnpackPattern(..)] = lhs {
            Self::plan_unpack_target(&lhs[0], Vec::new(), &mut plan);
        } else {
            Self::plan_list_unpack(lhs, Vec::new(), &mut plan);
        }

        // A literal source has no runtime effects. Preflight every path with
        // the shared value extractor, then write only the extracted constants.
        // If any path is invalid, retain the runtime plan and its standard
        // diagnostic.
        if let AstNode::Literal(source, rhs_span) = rhs
            && let Ok(values) = plan
                .paths
                .iter()
                .map(|path| extract_unpack_path(source, path))
                .collect::<Result<Vec<_>, _>>()
        {
            if plan.writes.is_empty() {
                self.compile_expr(rhs)?;
            } else {
                let writes = plan
                    .writes
                    .iter()
                    .map(|write| {
                        self.unpack_write(
                            &write.target,
                            AstNode::Literal(values[write.path_index].clone(), *rhs_span),
                            op,
                            span,
                        )
                    })
                    .collect::<WqResult<Vec<_>>>()?;
                self.compile_stmt_sequence_inner(&AstNode::Block(writes, span), self.value_needed)?;
            }
            return Ok(());
        }

        let writes = plan
            .writes
            .iter()
            .map(|write| {
                self.unpack_write(
                    &write.target,
                    AstNode::UnpackValue {
                        slot: write.path_index + 1,
                        span,
                    },
                    op,
                    span,
                )
            })
            .collect::<WqResult<Vec<_>>>()?;

        self.compile_expr(rhs)?;
        self.instructions
            .push(Instruction::Unpack(Box::new(UnpackPlan {
                paths: plan.paths.into_boxed_slice(),
            })));

        if writes.is_empty() {
            self.instructions.push(Instruction::LoadUnpack(0));
        } else {
            self.compile_stmt_sequence_inner(&AstNode::Block(writes, span), self.value_needed)?;
        }
        self.instructions.push(Instruction::EndUnpack);
        Ok(())
    }

    fn plan_list_unpack(
        items: &[AstNode],
        prefix: Vec<UnpackPathSegment>,
        plan: &mut PlannedUnpack,
    ) {
        let ellipsis = items
            .iter()
            .position(|item| matches!(item, AstNode::Ellipsis(_)));
        let prefix_len = ellipsis.unwrap_or(items.len());
        for (position, item) in items.iter().take(prefix_len).enumerate() {
            let mut path = prefix.clone();
            path.push(UnpackPathSegment::Index(position as i64));
            Self::plan_unpack_target(item, path, plan);
        }
        if let Some(ellipsis) = ellipsis {
            let suffix = &items[ellipsis + 1..];
            for (offset, item) in suffix.iter().enumerate() {
                let distance_from_end = suffix.len() - offset;
                let mut path = prefix.clone();
                path.push(UnpackPathSegment::Index(-(distance_from_end as i64)));
                Self::plan_unpack_target(item, path, plan);
            }
        }
    }

    fn plan_unpack_target(
        target: &AstNode,
        path: Vec<UnpackPathSegment>,
        plan: &mut PlannedUnpack,
    ) {
        match target {
            AstNode::DictUnpackPattern(entries, _) => {
                for entry in entries {
                    let mut entry_path = path.clone();
                    entry_path.push(UnpackPathSegment::Key(entry.key.clone().into()));
                    Self::plan_unpack_target(&entry.target, entry_path, plan);
                }
            }
            AstNode::List(items, _) => {
                Self::plan_list_unpack(items, path, plan);
            }
            AstNode::Ellipsis(_) => {}
            target => {
                let path_index = plan.paths.len();
                plan.paths.push(path.into_boxed_slice());
                plan.writes.push(PlannedUnpackWrite {
                    target: target.clone(),
                    path_index,
                });
            }
        }
    }

    fn unpack_write(
        &self,
        target: &AstNode,
        value: AstNode,
        op: Option<BinaryOperator>,
        span: AstSpan,
    ) -> WqResult<AstNode> {
        let value = Box::new(value);
        match target {
            AstNode::Variable(name, name_span) => Ok(AstNode::Assignment {
                name: name.clone(),
                op,
                value,
                span: *name_span,
                name_span: *name_span,
            }),
            AstNode::Index { object, index, .. } => Ok(AstNode::IndexAssign {
                object: object.clone(),
                index: index.clone(),
                op,
                value,
                span,
            }),
            AstNode::Postfix {
                object,
                items,
                explicit_call: false,
                ..
            } => {
                let index = if items.len() == 1 {
                    Box::new(items[0].clone())
                } else {
                    Box::new(AstNode::List(items.clone(), None))
                };
                Ok(AstNode::IndexAssign {
                    object: object.clone(),
                    index,
                    op,
                    value,
                    span,
                })
            }
            _ => Err(self.internal_err_here(
                "internal compiler error while compiling an unpack assignment target",
            )),
        }
    }

    /// Compile call arguments to the stack.  If any are `NamedArg` nodes,
    /// collect the metadata and emit `SetupNamedCall` for the VM.
    fn compile_call_args(&mut self, args: &[AstNode]) -> WqResult<()> {
        let mut named_args: Vec<(u16, Arc<str>)> = Vec::new();
        let mut seen_named = IndexSet::new();
        for (i, arg) in args.iter().enumerate() {
            if let AstNode::NamedArg { name, span, .. } = arg {
                if !seen_named.insert(name.as_str()) {
                    return Err(
                        self.syntax_err_at(*span, format!("duplicate named argument '{name}'"))
                    );
                }
                let pos = u16::try_from(i)
                    .map_err(|_| self.syntax_err_here("call has too many arguments"))?;
                named_args.push((pos, Arc::from(name.as_str())));
            }
        }

        let pos_count = u16::try_from(args.len() - named_args.len())
            .map_err(|_| self.syntax_err_here("call has too many positional arguments"))?;

        // Evaluate all arguments left-to-right, preserving source order
        for arg in args {
            match arg {
                AstNode::NamedArg { value, .. } => self.compile_expr(value)?,
                other => self.compile_expr(other)?,
            }
        }

        if !named_args.is_empty() {
            self.instructions
                .push(Instruction::PrepareNamedArgs(Arc::new(
                    crate::vm::inst::NamedArgMeta {
                        pos_count,
                        named: named_args.into_boxed_slice(),
                    },
                )));
        }

        Ok(())
    }

    fn compile_index_args(&mut self, index: &AstNode) -> WqResult<usize> {
        if let Some(items) = Self::synthetic_index_items(index) {
            self.compile_call_args(items)?;
            Ok(items.len())
        } else {
            self.compile_expr(index)?;
            Ok(1)
        }
    }

    fn compile_index_args_for_assign(&mut self, index: &AstNode) -> WqResult<usize> {
        if let Some(key) = Self::literal_int_synthetic_index_key(index) {
            self.emit_load_const(key);
            Ok(1)
        } else {
            self.compile_index_args(index)
        }
    }

    fn compile_index_arg_plan(&mut self, index: &AstNode) -> WqResult<IndexArgPlan> {
        if let Some(key) = Self::literal_int_synthetic_index_key(index) {
            Ok(IndexArgPlan::ConstKey(key))
        } else {
            self.compile_index_arg_temps(index).map(IndexArgPlan::Temps)
        }
    }

    fn compile_index_arg_temps(&mut self, index: &AstNode) -> WqResult<Vec<String>> {
        let items = Self::synthetic_index_items(index).unwrap_or(std::slice::from_ref(index));
        let mut names = Vec::with_capacity(items.len());
        for item in items {
            let name = self.next_temp_name("idx-arg");
            self.compile_expr(item)?;
            self.emit_store(&name)?;
            names.push(name);
        }
        Ok(names)
    }

    fn emit_index_arg_plan_loads(&mut self, plan: &IndexArgPlan) -> WqResult<()> {
        match plan {
            IndexArgPlan::Temps(names) => self.emit_index_arg_loads(names),
            IndexArgPlan::ConstKey(key) => {
                self.emit_load_const(key.clone());
                Ok(())
            }
        }
    }

    fn emit_index_arg_loads(&mut self, names: &[String]) -> WqResult<()> {
        for name in names {
            self.emit_load(name, None)?;
        }
        Ok(())
    }

    fn synthetic_index_items(index: &AstNode) -> Option<&[AstNode]> {
        match index {
            AstNode::List(items, None) => Some(items.as_slice()),
            _ => None,
        }
    }

    fn literal_int_synthetic_index_key(index: &AstNode) -> Option<Value> {
        let items = Self::synthetic_index_items(index)?;
        if items.len() < 2 {
            return None;
        }
        let mut idxs = Vec::with_capacity(items.len());
        for item in items {
            match item {
                AstNode::Literal(Value::Int(idx), _) => idxs.push(*idx),
                _ => return None,
            }
        }
        Some(Value::IntList(Arc::new(idxs)))
    }

    fn index_load_local_inst(slot: u16, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexLoadLocal(slot)
        } else {
            Instruction::IndexManyLoadLocal(slot, argc)
        }
    }

    fn index_load_capture_inst(slot: u16, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexLoadCapture(slot)
        } else {
            Instruction::IndexManyLoadCapture(slot, argc)
        }
    }

    fn index_load_var_inst(name: Arc<str>, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexLoadVar(name)
        } else {
            Instruction::IndexManyLoadVar(name, argc)
        }
    }

    fn index_inst(argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::Index
        } else {
            Instruction::IndexMany(argc)
        }
    }

    fn index_assign_local_inst(slot: u16, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexAssignLocal(slot)
        } else {
            Instruction::IndexManyAssignLocal(slot, argc)
        }
    }

    fn index_assign_capture_inst(slot: u16, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexAssignCapture(slot)
        } else {
            Instruction::IndexManyAssignCapture(slot, argc)
        }
    }

    fn index_assign_var_inst(name: Arc<str>, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexAssignVar(name)
        } else {
            Instruction::IndexManyAssignVar(name, argc)
        }
    }

    fn index_assign_local_drop_inst(slot: u16, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexAssignLocalDrop(slot)
        } else {
            Instruction::IndexManyAssignLocalDrop(slot, argc)
        }
    }

    fn index_assign_capture_drop_inst(slot: u16, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexAssignCaptureDrop(slot)
        } else {
            Instruction::IndexManyAssignCaptureDrop(slot, argc)
        }
    }

    fn index_assign_var_drop_inst(name: Arc<str>, argc: usize) -> Instruction {
        debug_assert!(argc > 0);
        if argc == 1 {
            Instruction::IndexAssignVarDrop(name)
        } else {
            Instruction::IndexManyAssignVarDrop(name, argc)
        }
    }

    fn next_temp_name(&mut self, prefix: &str) -> String {
        let id = self.gensym;
        self.gensym = self.gensym.wrapping_add(1);
        format!("--vm-{prefix}-{id}")
    }

    fn collect_index_path<'a>(
        &self,
        object: &'a AstNode,
        index: &'a AstNode,
        span: AstSpan,
    ) -> WqResult<Option<IndexPathTarget<'a>>> {
        let mut indices = vec![index];
        let mut current = object;
        loop {
            match current {
                AstNode::Index { object, index, .. } => {
                    if Self::is_bulk_path_prefix_index(index) {
                        return Err(self.syntax_err_at(
                            span,
                            "bulk index cannot appear before the final path segment",
                        ));
                    }
                    indices.push(index);
                    current = object;
                }
                AstNode::Postfix {
                    object,
                    items,
                    explicit_call: false,
                    depth: None,
                    ..
                } => {
                    let [item] = items.as_slice() else {
                        return Err(self.syntax_err_at(
                            span,
                            "bulk index cannot appear before the final path segment",
                        ));
                    };
                    indices.push(item);
                    current = object;
                }
                AstNode::Variable(name, _) => {
                    if indices.len() < 2 {
                        return Ok(None);
                    }
                    indices.reverse();
                    return Ok(Some(IndexPathTarget {
                        root: IndexPathRoot::Variable(name),
                        indices,
                    }));
                }
                AstNode::OuterVariable(name, name_span) => {
                    if indices.len() < 2 {
                        return Ok(None);
                    }
                    indices.reverse();
                    return Ok(Some(IndexPathTarget {
                        root: IndexPathRoot::OuterVariable(name, *name_span),
                        indices,
                    }));
                }
                _ => return Ok(None),
            }
        }
    }

    fn is_bulk_path_prefix_index(index: &AstNode) -> bool {
        matches!(
            index,
            AstNode::List(..) | AstNode::Literal(Value::List(_) | Value::IntList(_), _)
        )
    }

    fn compile_index_path_assign(
        &mut self,
        target: IndexPathTarget<'_>,
        op: Option<BinaryOperator>,
        value: &AstNode,
    ) -> WqResult<()> {
        let mut index_args = Vec::with_capacity(target.indices.len());
        let final_index_pos = target.indices.len() - 1;
        for (pos, index) in target.indices.iter().enumerate() {
            if pos == final_index_pos {
                index_args.push(self.compile_index_arg_plan(index)?);
            } else {
                let name = self.next_temp_name("idx-path-index");
                self.compile_expr(index)?;
                self.instructions.push(Instruction::CheckAtomPathIndex);
                self.emit_store(&name)?;
                index_args.push(IndexArgPlan::Temps(vec![name]));
            }
        }

        let value_name = self.next_temp_name("idx-path-value");
        if let Some(op) = op {
            self.emit_index_path_load(target.root, &index_args)?;
            self.compile_expr(value)?;
            self.instructions
                .push(Instruction::binary_op(op, Operand::Stack, Operand::Stack));
            self.emit_store(&value_name)?;
        } else {
            self.compile_expr(value)?;
            self.emit_store(&value_name)?;
        }

        let mut child_name = value_name.clone();
        for level in (1..index_args.len()).rev() {
            let parent_name = self.next_temp_name("idx-path-parent");
            self.emit_index_path_load(target.root, &index_args[..level])?;
            self.emit_store(&parent_name)?;
            self.emit_index_arg_plan_loads(&index_args[level])?;
            self.emit_load(&child_name, None)?;
            self.push_index_assign_name_drop(&parent_name, index_args[level].argc());
            child_name = parent_name;
        }

        self.emit_index_arg_plan_loads(&index_args[0])?;
        self.emit_load(&child_name, None)?;
        self.push_index_assign_root_drop(target.root, index_args[0].argc());
        self.emit_load(&value_name, None)?;
        Ok(())
    }

    fn emit_index_path_load(
        &mut self,
        root: IndexPathRoot<'_>,
        index_args: &[IndexArgPlan],
    ) -> WqResult<()> {
        match root {
            IndexPathRoot::Variable(name) => self.emit_load(name, None)?,
            IndexPathRoot::OuterVariable(name, name_span) => {
                self.emit_outer_load(name, name_span)?
            }
        }
        for args in index_args {
            self.emit_index_arg_plan_loads(args)?;
            self.instructions.push(Self::index_inst(args.argc()));
        }
        Ok(())
    }

    fn push_index_assign_name_drop(&mut self, name: &str, argc: usize) {
        if self.fn_depth > 0 && self.is_local(name) {
            let slot = self.locals[name];
            self.instructions
                .push(Self::index_assign_local_drop_inst(slot, argc));
        } else {
            self.instructions
                .push(Self::index_assign_var_drop_inst(name.into(), argc));
        }
    }

    fn push_index_assign_root_drop(&mut self, root: IndexPathRoot<'_>, argc: usize) {
        match root {
            IndexPathRoot::Variable(name) => {
                if self.fn_depth > 0 && self.is_local(name) {
                    let slot = self.locals[name];
                    self.instructions
                        .push(Self::index_assign_local_drop_inst(slot, argc));
                } else if self.is_ref_default_name(name) {
                    if let Some(idx) = self.ref_capture_map.get(name) {
                        self.instructions
                            .push(Self::index_assign_capture_drop_inst(*idx, argc));
                    } else {
                        self.instructions
                            .push(Self::index_assign_var_drop_inst(name.into(), argc));
                    }
                } else {
                    self.instructions
                        .push(Self::index_assign_var_drop_inst(name.into(), argc));
                }
            }
            IndexPathRoot::OuterVariable(name, _) => {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    self.instructions
                        .push(Self::index_assign_capture_drop_inst(*idx, argc));
                } else {
                    self.instructions
                        .push(Self::index_assign_var_drop_inst(name.into(), argc));
                }
            }
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
                    let argument = if expected == 1 {
                        "argument"
                    } else {
                        "arguments"
                    };
                    return Err(self.syntax_err_at(
                        span,
                        format!("'{name}@{depth}' expects {expected} {argument}"),
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
                        format!("'{name}@{depth}' expects {required} or {optional} arguments"),
                    )),
                }
            }
            BuiltinDepthSugar::None => Err(self.syntax_err_at(
                span,
                format!("depth modifier can only be used on depth-aware builtins, got '{name}'"),
            )),
        }
    }

    /// Allocate parameter slots, including the hidden `--named-mask` slot, and
    /// emit default values for omitted named parameters.
    fn emit_params_and_prologue(&mut self, params: &Option<Vec<Parameter>>) -> WqResult<()> {
        let mut named_prologue: Vec<(u16, u8, Box<AstNode>)> = Vec::new();
        let mut param_list_span: Option<(usize, usize)> = None;
        if let Some(ps) = params {
            let max_named_params = usize::try_from(i64::BITS).expect("i64::BITS fits in usize");
            let mut named_idx = 0usize;
            for p in ps {
                let slot = self.local_slot(p.name())?;
                if let Parameter::Named {
                    default: Some(default_expr),
                    ..
                } = p
                {
                    if named_idx >= max_named_params {
                        return Err(
                            self.syntax_err_at(p.span(), "function has too many named parameters")
                        );
                    }
                    let bit_idx = u8::try_from(named_idx)
                        .expect("named parameter index is limited to i64::BITS");
                    named_prologue.push((slot, bit_idx, default_expr.clone()));
                }
                if matches!(p, Parameter::Named { .. }) {
                    if named_idx >= max_named_params {
                        return Err(
                            self.syntax_err_at(p.span(), "function has too many named parameters")
                        );
                    }
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
            self.local_slot("x")?;
            self.local_slot("y")?;
            self.local_slot("z")?;
        }

        let mask_slot = if params
            .as_ref()
            .is_some_and(|ps| ps.iter().any(|p| matches!(p, Parameter::Named { .. })))
        {
            Some(self.local_slot("--named-mask")?)
        } else {
            None
        };

        if let Some(mask_slot) = mask_slot {
            for (slot, bit_idx, default_expr) in &named_prologue {
                let jump_idx = self.instructions.len();
                self.instructions
                    .push(Instruction::JumpIfNamedProvided(mask_slot, *bit_idx, 0));
                self.compile_expr(default_expr)?;
                self.instructions.push(Instruction::StoreLocal(*slot));
                self.instructions.push(Instruction::Pop);
                let end = self.instructions.len();
                if let Instruction::JumpIfNamedProvided(_, _, ref mut target) =
                    self.instructions[jump_idx]
                {
                    *target = end;
                }
            }
        }
        // Associate only generated parameter-prologue instructions with the
        // parameter list. Reserving PC 0 when no prologue exists would claim
        // the first body instruction and hide its exact expression span.
        if let Some(span) = param_list_span {
            self.dbg_pc_spans.resize(self.instructions.len(), None);
            for pc_span in &mut self.dbg_pc_spans {
                if pc_span.is_none() {
                    *pc_span = Some(span);
                }
            }
        }

        Ok(())
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

    fn builtin_call_inst(&self, id: usize, argc: usize) -> Instruction {
        let id = u16::try_from(id).expect("builtin id overflow");
        let argc = u16::try_from(argc).expect("argc overflow");
        if !self.value_needed && Builtins::has_discard_fn_from_id(id) {
            Instruction::CallBuiltinDiscardId(id, argc)
        } else {
            Instruction::CallBuiltinId(id, argc)
        }
    }

    fn stmt_span_count(node: &AstNode) -> usize {
        match node {
            AstNode::Block(stmts, _) => stmts.iter().map(Self::stmt_span_count).sum(),
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
        node.span()
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
            AstNode::Block(stmts, _) => {
                if stmts.is_empty() {
                    self.emit_load_const(Value::empty_list());
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
                    self.emit_load_const(Value::empty_list());
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
            AstNode::Import { specifier, .. } => {
                let importer = self
                    .import_origin
                    .as_deref()
                    .or(self.src_path.as_deref())
                    .unwrap_or("<eval>");
                self.push_inst(Instruction::Import(Box::new(ImportData {
                    specifier: Arc::from(specifier.as_str()),
                    importer: Arc::from(importer),
                })));
            }
            AstNode::Variable(name, span) => self.emit_load(name, *span)?,
            AstNode::OuterVariable(name, span) => self.emit_outer_load(name, *span)?,
            AstNode::UnpackValue { slot, .. } => {
                self.instructions.push(Instruction::LoadUnpack(*slot));
            }
            AstNode::PipeInput => {
                return Err(self.syntax_err_here("pipe input placeholder escaped its pipe context"));
            }
            AstNode::NamedArg { value, .. } => {
                return self.compile_node(value);
            }
            AstNode::Ellipsis(_) => {
                return Err(self.syntax_err_here(
                    "'...' placeholder is only valid in unpack assignment pattern",
                ));
            }
            AstNode::DictUnpackPattern(..) => {
                return Err(self.syntax_err_here(
                    "dict unpack pattern is only valid on the left side of an assignment",
                ));
            }
            AstNode::Assignment {
                name,
                op,
                value,
                name_span,
                ..
            } => {
                if let Some(op) = op {
                    if *op == BinaryOperator::Cat
                        && self.can_embed_cat_assign_rhs(value)
                        && let Some(target) = self.cat_assign_target(name)
                    {
                        let right = self.compile_expr_as_operand(value)?;
                        debug_assert!(!matches!(right, Operand::Stack));
                        self.instructions
                            .push(Instruction::cat_assign(target, right));
                    } else {
                        if *op == BinaryOperator::Cat {
                            // Snapshot the old value before evaluating an RHS
                            // that may mutate this binding.
                            self.compile_expr(&AstNode::Variable(name.clone(), *name_span))?;
                            self.compile_expr(value)?;
                            self.instructions.push(Instruction::Cat(2));
                        } else {
                            let left = if Self::rhs_cannot_mutate_bindings(value) {
                                self.operand_for_name(name)?
                            } else {
                                // Snapshot the old value before evaluating an RHS
                                // that may mutate this binding directly or through
                                // a reference capture.
                                self.compile_expr(&AstNode::Variable(name.clone(), *name_span))?;
                                Operand::Stack
                            };
                            let right = self.compile_expr_as_operand(value)?;
                            self.instructions
                                .push(Instruction::binary_op(*op, left, right));
                        }
                        self.emit_store_keep(name)?;
                    }
                } else if let AstNode::Function {
                    params,
                    ref_capture,
                    body,
                    ..
                } = &**value
                {
                    // Reserve slot for recursion when in a local scope
                    if self.fn_depth > 0 {
                        self.local_slot(name)?;
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
                    let mut c = Compiler::new_with_builtins(self.builtins.clone());
                    c.fn_depth = self.fn_depth + 1;
                    c.isolated_module = self.isolated_module;
                    c.defining_name = Some(name.clone());
                    if *ref_capture {
                        c.ref_default_names = capture_needs.by_ref.clone();
                    }
                    // prepare captures from current locals if inside a function
                    if self.fn_depth > 0 {
                        self.seed_child_captures(&mut c, &capture_needs, Some(name))?;
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
                        c.set_source_base_offset(self.src_base_offset);
                    }
                    if let Some(path) = &self.src_path {
                        c.set_source_path(path.clone());
                    }
                    if let Some(origin) = &self.import_origin {
                        c.set_import_origin(origin.clone());
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
                                isolated_module: c.isolated_module,
                                captures: c.captures.clone(),
                                instructions: func_arc,
                                dbg_chunk: None,
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
                            isolated_module: c.isolated_module,
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
                    self.emit_store_keep(name)?;
                    if self.fn_depth > 0 {
                        self.fn_locals.insert(name.clone());
                    }
                } else {
                    self.compile_expr(value)?;
                    // Store and keep the value on the stack for expression result
                    self.emit_store_keep(name)?;
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
                    if *op == BinaryOperator::Cat
                        && self.can_embed_cat_assign_rhs(value)
                        && let Some(target) = self.outer_cat_assign_target(name)
                    {
                        let right = self.compile_expr_as_operand(value)?;
                        debug_assert!(!matches!(right, Operand::Stack));
                        self.instructions
                            .push(Instruction::cat_assign(target, right));
                    } else {
                        if *op == BinaryOperator::Cat {
                            self.compile_expr(&AstNode::OuterVariable(name.clone(), *name_span))?;
                            self.compile_expr(value)?;
                            self.instructions.push(Instruction::Cat(2));
                        } else {
                            let left = if Self::rhs_cannot_mutate_bindings(value) {
                                if let Some(idx) = self.ref_capture_map.get(name) {
                                    Operand::Capture(*idx)
                                } else {
                                    Operand::Var(name.clone().into())
                                }
                            } else {
                                // Keep augmented assignment evaluation consistent
                                // with ordinary binary expressions when the RHS
                                // can have effects.
                                self.compile_expr(&AstNode::OuterVariable(
                                    name.clone(),
                                    *name_span,
                                ))?;
                                Operand::Stack
                            };
                            let right = self.compile_expr_as_operand(value)?;
                            self.instructions
                                .push(Instruction::binary_op(*op, left, right));
                        }
                        self.emit_outer_store_keep(name, *name_span)?;
                    }
                } else {
                    self.compile_expr(value)?;
                    self.emit_outer_store_keep(name, *name_span)?;
                }
            }
            AstNode::BinaryOp {
                left,
                operator,
                right,
                ..
            } => self.compile_binary_chain(left, *operator, right)?,
            AstNode::LazyBool {
                operator, operands, ..
            } => self.compile_lazy_bool(*operator, operands)?,
            AstNode::ComparisonChain { first, rest, .. } => {
                self.compile_expr(first)?;
                let mut ops: Vec<BinaryOperator> = Vec::with_capacity(rest.len());
                for (op, node) in rest {
                    self.compile_expr(node)?;
                    ops.push(*op);
                }
                self.instructions
                    .push(Instruction::CmpChain(ops.into_boxed_slice()));
            }
            AstNode::Cat(items, _) => {
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
                ..
            } => {
                self.compile_expr(start)?;
                if let Some(next_expr) = step {
                    self.compile_expr(next_expr)?;
                    self.compile_expr(end)?;
                    self.instructions.push(Instruction::MakeRange {
                        inclusive: *inclusive,
                        has_next: true,
                    });
                } else {
                    self.compile_expr(end)?;
                    self.instructions.push(Instruction::MakeRange {
                        inclusive: *inclusive,
                        has_next: false,
                    });
                }
            }
            AstNode::UnaryOp {
                operator, operand, ..
            } => {
                let op = self.compile_expr_as_operand(operand)?;
                self.instructions.push(Instruction::unary_op(*operator, op));
            }
            AstNode::List(elements, _) => {
                for elem in elements {
                    self.compile_expr(elem)?;
                }
                self.instructions
                    .push(Instruction::MakeList(elements.len()));
            }
            AstNode::Dict(pairs, _) => {
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
                    self.instructions
                        .push(self.builtin_call_inst(id, args.len()));
                } else {
                    let target = self.operand_for_name(name)?;
                    self.instructions.push(Instruction::LoadCallTarget(target));
                    self.compile_call_args(args)?;
                    if self.fn_depth == 0 {
                        self.instructions
                            .push(Instruction::CallUser(name.clone().into(), args.len()));
                    } else if self.is_local(name) && self.fn_locals.contains(name) {
                        self.instructions
                            .push(Instruction::CallLocal(self.locals[name], args.len()));
                    } else {
                        self.instructions.push(Instruction::Postfix(args.len()));
                    }
                }
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::CallAnonymous { object, args, span } => {
                let start = self.instructions.len();
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
                ..
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
                    self.instructions
                        .push(self.builtin_call_inst(id, args.len()));
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
                    self.instructions
                        .push(self.builtin_call_inst(id, items.len()));
                } else {
                    // Non-builtin: compile the callee first, then the args
                    self.compile_expr(object)?;
                    self.compile_call_args(items)?;
                    self.instructions.push(Instruction::Postfix(items.len()));
                }
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::Pipe { .. } => {
                return Err(self.internal_err_here(
                    "internal compiler error while compiling an unresolved pipe",
                ));
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
                self.emit_store(&temp_name)?;
                let effect = replace_pipe_input(effect, &temp_name);
                self.compile_in_context(&effect, true)?;
                self.instructions.push(Instruction::Pop);
                self.emit_load(&temp_name, None)?;
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::Break(_) => {
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    let pos = self.instructions.len();
                    self.instructions.push(Instruction::Jump(0));
                    loop_info.break_jumps.push(pos);
                } else {
                    return Err(self.syntax_err_here("@b outside loop"));
                }
            }
            AstNode::Continue(_) => {
                if let Some(loop_info) = self.loop_stack.last_mut() {
                    let pos = self.instructions.len();
                    self.instructions.push(Instruction::Jump(0));
                    loop_info.continue_jumps.push(pos);
                } else {
                    return Err(self.syntax_err_here("@c outside loop"));
                }
            }
            AstNode::Return(expr, _) => {
                if self.fn_depth == 0 || self.module_root {
                    return Err(self.syntax_err_here("@r outside function"));
                }
                if let Some(e) = expr {
                    self.compile_expr(e)?;
                } else {
                    self.emit_load_const(Value::empty_list());
                }
                self.instructions.push(Instruction::Return);
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
                self.has_runtime_debug = true;
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
                    self.emit_load_const(Value::empty_list());
                }
            }
            AstNode::Try(expr, _) => {
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
                ..
            } => {
                let start = self.instructions.len();
                let mut optimized = false;
                if let AstNode::Variable(name, _) = &**object {
                    if self.is_local(name) {
                        let slot = self.locals[name];
                        let argc = self.compile_index_args(index)?;
                        self.instructions
                            .push(Self::index_load_local_inst(slot, argc));
                        optimized = true;
                    } else if self.is_ref_default_name(name) {
                        let argc = self.compile_index_args(index)?;
                        self.push_ref_default_index_load(name, argc);
                        optimized = true;
                    } else if self.fn_depth == 0 {
                        let argc = self.compile_index_args(index)?;
                        self.instructions
                            .push(Self::index_load_var_inst(name.clone().into(), argc));
                        optimized = true;
                    }
                } else if let AstNode::OuterVariable(name, _) = &**object
                    && let Some(idx) = self.ref_capture_map.get(name).copied()
                {
                    let argc = self.compile_index_args(index)?;
                    self.instructions
                        .push(Self::index_load_capture_inst(idx, argc));
                    optimized = true;
                }
                if !optimized {
                    self.compile_expr(object)?;
                    let argc = self.compile_index_args(index)?;
                    self.instructions.push(Self::index_inst(argc));
                }
                let end = self.instructions.len();
                self.fill_span_range(start, end, *span);
            }
            AstNode::IndexAssign {
                object,
                index,
                op,
                value,
                span,
            } => {
                if self.module_root {
                    match object.as_ref() {
                        AstNode::Variable(name, span) if !self.is_local(name) => {
                            return Err(self.error_at(
                                WqErrorType::NotBound,
                                *span,
                                format!("'{name}' has not been bound to a value"),
                            ));
                        }
                        AstNode::OuterVariable(_, span) => {
                            return Err(self.syntax_err_at(
                                *span,
                                "outer binding reference requires a closure",
                            ));
                        }
                        _ => {}
                    }
                }
                if let Some(target) = self.collect_index_path(object, index, *span)? {
                    self.compile_index_path_assign(target, *op, value)?;
                    return Ok(());
                }

                match &**object {
                    AstNode::Variable(name, _) => {
                        if let Some(op) = op {
                            let index_args = self.compile_index_arg_plan(index)?;
                            let argc = index_args.argc();
                            self.emit_index_arg_plan_loads(&index_args)?; // for the assignment
                            self.emit_index_arg_plan_loads(&index_args)?; // for the load
                            if self.fn_depth > 0 && self.is_local(name) {
                                let slot = self.locals[name];
                                self.instructions
                                    .push(Self::index_load_local_inst(slot, argc));
                            } else if self.is_ref_default_name(name) {
                                self.push_ref_default_index_load(name, argc);
                            } else {
                                self.instructions
                                    .push(Self::index_load_var_inst(name.clone().into(), argc));
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
                                let slot = self.locals[name];
                                self.instructions
                                    .push(Self::index_assign_local_inst(slot, argc));
                            } else if self.is_ref_default_name(name) {
                                self.push_ref_default_index_assign(name, argc);
                            } else {
                                self.instructions
                                    .push(Self::index_assign_var_inst(name.clone().into(), argc));
                            }
                        } else {
                            if self.fn_depth > 0 && self.is_local(name) {
                                let slot = self.locals[name];
                                let argc = self.compile_index_args_for_assign(index)?;
                                self.compile_expr(value)?;
                                self.instructions
                                    .push(Self::index_assign_local_inst(slot, argc));
                            } else if self.is_ref_default_name(name) {
                                let argc = self.compile_index_args_for_assign(index)?;
                                self.compile_expr(value)?;
                                self.push_ref_default_index_assign(name, argc);
                            } else {
                                let argc = self.compile_index_args_for_assign(index)?;
                                self.compile_expr(value)?;
                                self.instructions
                                    .push(Self::index_assign_var_inst(name.clone().into(), argc));
                            }
                        }
                    }
                    AstNode::OuterVariable(name, _) => {
                        if let Some(op) = op {
                            let index_args = self.compile_index_arg_plan(index)?;
                            let argc = index_args.argc();
                            self.emit_index_arg_plan_loads(&index_args)?; // for assignment
                            if let Some(idx) = self.ref_capture_map.get(name).copied() {
                                self.emit_index_arg_plan_loads(&index_args)?; // for load
                                self.instructions
                                    .push(Self::index_load_capture_inst(idx, argc));
                            } else {
                                self.emit_index_arg_plan_loads(&index_args)?; // for load
                                self.instructions
                                    .push(Self::index_load_var_inst(name.clone().into(), argc));
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
                                    .push(Self::index_assign_capture_inst(*idx, argc));
                            } else {
                                self.instructions
                                    .push(Self::index_assign_var_inst(name.clone().into(), argc));
                            }
                        } else {
                            let argc = self.compile_index_args_for_assign(index)?;
                            self.compile_expr(value)?;
                            if let Some(idx) = self.ref_capture_map.get(name) {
                                self.instructions
                                    .push(Self::index_assign_capture_inst(*idx, argc));
                            } else {
                                self.instructions
                                    .push(Self::index_assign_var_inst(name.clone().into(), argc));
                            }
                        }
                    }
                    _ => {
                        return Err(self.internal_err_here(
                            "internal compiler error while compiling an index assignment",
                        ));
                    }
                }
            }
            AstNode::Function {
                params,
                ref_capture,
                body,
                ..
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
                let mut c = Compiler::new_with_builtins(self.builtins.clone());
                c.fn_depth = self.fn_depth + 1;
                c.isolated_module = self.isolated_module;
                if *ref_capture {
                    c.ref_default_names = capture_needs.by_ref.clone();
                }
                if self.fn_depth > 0 {
                    self.seed_child_captures(
                        &mut c,
                        &capture_needs,
                        self.defining_name.as_deref(),
                    )?;
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
                    c.set_source_base_offset(self.src_base_offset);
                }
                if let Some(path) = &self.src_path {
                    c.set_source_path(path.clone());
                }
                if let Some(origin) = &self.import_origin {
                    c.set_import_origin(origin.clone());
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
                            isolated_module: c.isolated_module,
                            captures: c.captures.clone(),
                            instructions: func_arc,
                            dbg_chunk: None,
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
                        isolated_module: c.isolated_module,
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
                    // expression should evaluate to an empty list on the false path
                    self.emit_load_const(Value::empty_list());
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
                    self.emit_load_const(Value::empty_list());
                    self.emit_store(&result_var)?;
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
                    self.emit_store(result_var)?;
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
                    self.emit_load(result_var, None)?;
                } else {
                    self.emit_load_const(Value::empty_list());
                }
            }
            AstNode::NLoop { count, body, .. } => {
                let count_span = Self::last_expr_span(count);
                // Reject non-int literal counts at compile time
                if let AstNode::Literal(value, _) = &**count
                    && !matches!(value, Value::Int(_) | Value::BigInt(_))
                {
                    return Err(self
                        .syntax_err_at(count_span, "expected int")
                        .attach_note("for an n-loop count")
                        .got1(value));
                }
                let body_spans = self.take_stmt_spans_for(body);
                // Unroll constant loops only when there is no control flow in body
                if let AstNode::Literal(Value::Int(n), _) = &**count
                    && *n >= 0
                    && !has_ctrl(body)
                {
                    let limit = 64;
                    if *n <= limit {
                        if *n == 0 {
                            self.emit_load_const(Value::empty_list());
                        } else {
                            let restore = self.begin_loop_var_restore("_n")?;
                            for i in 0..*n {
                                let iter_start = self.instructions.len();
                                self.emit_load_const(Value::Int(i));
                                self.emit_store("_n")?;
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
                            self.finish_loop_var_restore("_n", &restore)?;
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
                self.emit_store(&count_var)?;
                let restore = self.begin_loop_var_restore("_n")?;
                self.emit_load_const(Value::Int(0));
                self.emit_store("_n")?;
                if let Some(result_var) = &result_var {
                    self.emit_load_const(Value::empty_list());
                    self.emit_store(result_var)?;
                }
                let local_loop_slots = if self.fn_depth > 0 && !self.trace_symbol_operands {
                    let index = self
                        .locals
                        .get("_n")
                        .copied()
                        .expect("N-loop index local should exist");
                    let count = self
                        .locals
                        .get(&count_var)
                        .copied()
                        .expect("N-loop count local should exist");
                    let snapshot = self.local_slot(&old_var)?;
                    Some((index, count, snapshot))
                } else {
                    None
                };
                let cmp_start = self.instructions.len();
                self.backward_jump_targets.insert(cmp_start);
                let jump_pos = if let Some((index, count, snapshot)) = local_loop_slots {
                    self.instructions
                        .push(Instruction::n_loop_enter(index, count, snapshot, 0));
                    cmp_start
                } else {
                    let left = self.operand_for_name("_n")?;
                    let right = self.operand_for_name(&count_var)?;
                    self.instructions
                        .push(Instruction::binary_op(BinaryOperator::Lt, left, right));
                    let jump_pos = self.instructions.len();
                    self.instructions.push(Instruction::JumpIfFalse(0));
                    jump_pos
                };
                self.dbg_pc_spans.resize(self.instructions.len(), None);
                if let Some(span) = count_span {
                    self.dbg_pc_spans[cmp_start] = Some(span);
                    self.dbg_pc_spans[jump_pos] = Some(span);
                }
                self.mark_current_stmt_pc(cmp_start);
                if local_loop_slots.is_none() {
                    self.emit_load("_n", None)?;
                    self.emit_store(&old_var)?;
                }
                self.loop_stack.push(LoopInfo::default());
                self.compile_stmt_sequence_with_spans(body, self.value_needed, &body_spans)?;
                if let Some(result_var) = &result_var {
                    self.emit_store(result_var)?;
                } else {
                    self.instructions.push(Instruction::Pop);
                }
                let continue_target = self.instructions.len();
                if let Some((index, _, snapshot)) = local_loop_slots {
                    self.instructions
                        .push(Instruction::n_loop_next(snapshot, index, cmp_start));
                } else {
                    let left = self.operand_for_name(&old_var)?;
                    self.instructions.push(Instruction::binary_op(
                        BinaryOperator::Add,
                        left,
                        Operand::const_val(Value::Int(1)),
                    ));
                    self.emit_store("_n")?;
                    self.instructions.push(Instruction::Jump(cmp_start));
                }
                let end = self.instructions.len();
                match &mut self.instructions[jump_pos] {
                    Instruction::NLoopEnter(data) => data.target = end,
                    instruction => *instruction = Instruction::JumpIfFalse(end),
                }
                if let Some(info) = self.loop_stack.pop() {
                    for pos in info.break_jumps {
                        self.instructions[pos] = Instruction::Jump(end);
                    }
                    for pos in info.continue_jumps {
                        self.instructions[pos] = Instruction::Jump(continue_target);
                    }
                }
                self.finish_loop_var_restore("_n", &restore)?;
                if let Some(result_var) = &result_var {
                    self.emit_load(result_var, None)?;
                } else {
                    self.emit_load_const(Value::empty_list());
                }
            }
            AstNode::Block(..) | AstNode::BlockExpr(..) => {
                self.compile_stmt_sequence_inner(node, self.value_needed)?;
            }
            AstNode::UnpackAssignment { lhs, op, rhs, span } => {
                self.compile_unpack_assignment(lhs, *op, rhs, *span)?
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
            AstNode::List(items, _) => items.is_empty(),
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
                } else if self.module_root {
                    Err(self.error_at(
                        WqErrorType::NotBound,
                        object.span(),
                        format!("'{name}' has not been bound to a value"),
                    ))
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
        self.src_base_offset = 0;
    }

    pub(crate) fn set_source_base_offset(&mut self, base_offset: usize) {
        self.src_base_offset = base_offset;
    }

    pub(crate) fn set_source_path(&mut self, path: String) {
        self.src_path = Some(path);
    }

    pub(crate) fn set_stmt_spans(&mut self, spans: Vec<(usize, usize)>) {
        self.cur_stmt_spans = spans;
        self.cur_stmt_idx = 0;
        self.current_stmt_span = None;
    }

    pub(crate) fn set_import_origin(&mut self, origin: impl Into<String>) {
        self.import_origin = Some(origin.into());
    }

    fn error_at(
        &self,
        err_type: WqErrorType,
        span: Option<(usize, usize)>,
        msg: impl Into<String>,
    ) -> WqError {
        let msg = msg.into();
        let mut e = WqError::new(err_type).src("compiler").msg(msg);

        if let (Some(src), Some((byte_start, byte_end))) = (
            self.src_text.as_ref(),
            span.or(self.current_stmt_span)
                .or_else(|| self.cur_stmt_spans.get(self.cur_stmt_idx).cloned()),
        ) {
            let path = self.src_path.clone().unwrap_or_else(|| "?".to_string());
            let byte_start = byte_start.saturating_add(self.src_base_offset);
            let byte_end = byte_end.saturating_add(self.src_base_offset);
            e = e
                .span(Some((byte_start, byte_end)))
                .source_ctx(src.clone(), path);
        }
        e
    }

    fn syntax_err_at(&self, span: Option<(usize, usize)>, msg: impl Into<String>) -> WqError {
        self.error_at(WqErrorType::Syntax, span, msg)
    }

    fn syntax_err_here(&self, msg: impl Into<String>) -> WqError {
        self.syntax_err_at(None, msg)
    }

    fn internal_err_here(&self, msg: impl Into<String>) -> WqError {
        self.error_at(WqErrorType::Vm, None, msg)
    }

    fn local_slot(&mut self, name: &str) -> WqResult<u16> {
        if let Some(&i) = self.locals.get(name) {
            Ok(i)
        } else {
            let idx = u16::try_from(self.locals.len())
                .map_err(|_| self.syntax_err_here("function has too many local slots"))?;
            self.locals.insert(name.to_string(), idx);
            Ok(idx)
        }
    }

    fn next_capture_slot(&self) -> WqResult<u16> {
        u16::try_from(self.captures.len())
            .map_err(|_| self.syntax_err_here("function captures too many bindings"))
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

    fn push_ref_default_index_load(&mut self, name: &str, argc: usize) {
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions
                .push(Self::index_load_capture_inst(*idx, argc));
        } else {
            self.instructions
                .push(Self::index_load_var_inst(name.to_string().into(), argc));
        }
    }

    fn push_ref_default_index_assign(&mut self, name: &str, argc: usize) {
        if let Some(idx) = self.ref_capture_map.get(name) {
            self.instructions
                .push(Self::index_assign_capture_inst(*idx, argc));
        } else {
            self.instructions
                .push(Self::index_assign_var_inst(name.to_string().into(), argc));
        }
    }

    pub(crate) fn local_count(&self) -> u16 {
        u16::try_from(self.locals.len()).expect("local slot count checked during allocation")
    }

    fn local_names_vec(&self) -> Vec<String> {
        let mut names = vec![String::new(); usize::from(self.local_count())];
        for (name, &idx) in self.locals.iter() {
            let idx = usize::from(idx);
            if idx < names.len() {
                names[idx] = name.clone();
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
    ) -> WqResult<()> {
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
            let idx = child.next_capture_slot()?;
            child.capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::Local(*v));
        }

        for (k, v) in &pairs {
            if !needs.by_ref.contains(k) {
                continue;
            }
            let idx = child.next_capture_slot()?;
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
            let idx = child.next_capture_slot()?;
            child.capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::FromCapture(i_parent));
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
            let idx = child.next_capture_slot()?;
            child.ref_capture_map.insert(k.clone(), idx);
            child.captures.push(Capture::FromCapture(i_parent));
        }
        Ok(())
    }

    #[inline]
    fn emit_load_const(&mut self, value: Value) {
        self.push_inst(Instruction::load_const(value));
    }

    fn emit_load(&mut self, name: &str, span: Option<(usize, usize)>) -> WqResult<()> {
        if self.fn_depth > 0 {
            if self.is_local(name) {
                let idx = self.locals[name];
                self.instructions.push(Instruction::LoadLocal(idx));
                return Ok(());
            }
            if self.defining_name.as_ref().is_some_and(|n| n == name) {
                self.instructions.push(Instruction::LoadSelf);
                return Ok(());
            }
            if self.push_ref_default_load(name) {
                return Ok(());
            }
            if let Some(idx) = self.capture_map.get(name) {
                self.instructions.push(Instruction::LoadCapture(*idx));
                return Ok(());
            }
            // If the name refers to a builtin-function, do not capture it.
            // Emit a global load so it resolves via builtin lookup at runtime.
            if self.builtins.has_function(name) {
                self.instructions
                    .push(Instruction::LoadVar(name.to_string().into()));
                return Ok(());
            }
            // capture globals by value
            let idx = self.next_capture_slot()?;
            self.capture_map.insert(name.to_string(), idx);
            self.captures.push(Capture::Global(name.to_string(), span));
            self.instructions.push(Instruction::LoadCapture(idx));
            return Ok(());
        }
        // fn_depth == 0: top-level global
        self.instructions
            .push(Instruction::LoadVar(name.to_string().into()));
        Ok(())
    }

    fn operand_for_name(&mut self, name: &str) -> WqResult<Operand> {
        if self.fn_depth > 0 {
            if self.is_local(name) {
                return Ok(Operand::Local(self.locals[name]));
            }
            if self.defining_name.as_ref().is_some_and(|n| n == name) {
                return Ok(Operand::Self_);
            }
            if let Some(operand) = self.ref_default_operand(name) {
                return Ok(operand);
            }
            if let Some(idx) = self.capture_map.get(name) {
                return Ok(Operand::Capture(*idx));
            }
            if self.builtins.has_function(name) {
                return Ok(Operand::Var(name.to_string().into()));
            }
            let idx = self.next_capture_slot()?;
            self.capture_map.insert(name.to_string(), idx);
            self.captures.push(Capture::Global(name.to_string(), None));
            return Ok(Operand::Capture(idx));
        }
        // fn_depth == 0: top-level global
        if self.builtins.has_function(name) {
            return Ok(Operand::Var(name.to_string().into()));
        }
        Ok(Operand::Var(name.to_string().into()))
    }

    fn compile_expr_as_operand(&mut self, node: &AstNode) -> WqResult<Operand> {
        match node {
            AstNode::Literal(v, ..) => Ok(Operand::const_val(v.clone())),
            AstNode::Variable(..) | AstNode::OuterVariable(..) if self.trace_symbol_operands => {
                self.compile_expr(node)?;
                Ok(Operand::Stack)
            }
            AstNode::Variable(name, _) => self.operand_for_name(name),
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

    fn compile_expr_as_ordered_left_operand(&mut self, node: &AstNode) -> WqResult<Operand> {
        if let AstNode::Literal(value, ..) = node {
            return Ok(Operand::const_val(value.clone()));
        }
        self.compile_expr(node)?;
        Ok(Operand::Stack)
    }

    fn rhs_cannot_mutate_bindings(node: &AstNode) -> bool {
        matches!(
            node,
            AstNode::Literal(..)
                | AstNode::Variable(..)
                | AstNode::OuterVariable(..)
                | AstNode::UnpackValue { .. }
        )
    }

    fn can_embed_cat_assign_rhs(&self, node: &AstNode) -> bool {
        !self.trace_symbol_operands
            && matches!(
                node,
                AstNode::Literal(..) | AstNode::Variable(..) | AstNode::OuterVariable(..)
            )
    }

    fn cat_assign_target(&self, name: &str) -> Option<StoreTarget> {
        if self.fn_depth == 0 {
            return None;
        }
        if self.is_ref_default_name(name) {
            return self
                .ref_capture_map
                .get(name)
                .copied()
                .map(StoreTarget::Capture);
        }
        self.locals.get(name).copied().map(StoreTarget::Local)
    }

    fn outer_cat_assign_target(&self, name: &str) -> Option<StoreTarget> {
        self.ref_capture_map
            .get(name)
            .copied()
            .map(StoreTarget::Capture)
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
            ..
        } = left
        {
            chain.push((*next_operator, next_right));
            left = next_left;
        }

        chain.reverse();
        let first_right = chain.first().expect("binary chain is not empty").1;
        let mut left_op = if Self::rhs_cannot_mutate_bindings(first_right) {
            self.compile_expr_as_operand(left)?
        } else {
            // Deferring a variable as Operand::Local/Var/Capture would let a
            // side-effecting RHS change which value the left operand observes.
            self.compile_expr_as_ordered_left_operand(left)?
        };
        for (op, right) in chain {
            let right_op = self.compile_expr_as_operand(right)?;
            self.instructions
                .push(Instruction::binary_op(op, left_op, right_op));
            left_op = Operand::Stack;
        }
        Ok(())
    }

    fn compile_lazy_bool(&mut self, operator: BoolOperator, operands: &[AstNode]) -> WqResult<()> {
        let (first, rest) = operands
            .split_first()
            .expect("parser requires at least two lazy bool operands");
        self.compile_expr(first)?;
        let mut lazy_positions = Vec::with_capacity(rest.len());
        for right in rest {
            let lazy_pos = self.instructions.len();
            match operator {
                BoolOperator::And => self.instructions.push(Instruction::BoolAndLazy(0)),
                BoolOperator::Or => self.instructions.push(Instruction::BoolOrLazy(0)),
            }
            lazy_positions.push(lazy_pos);
            self.compile_expr(right)?;
            self.instructions.push(Instruction::BoolCombine(operator));
        }
        let end = self.instructions.len();
        for lazy_pos in lazy_positions {
            match operator {
                BoolOperator::And => {
                    self.instructions[lazy_pos] = Instruction::BoolAndLazy(end);
                }
                BoolOperator::Or => {
                    self.instructions[lazy_pos] = Instruction::BoolOrLazy(end);
                }
            }
        }
        Ok(())
    }

    fn begin_loop_var_restore(&mut self, name: &str) -> WqResult<LoopVarRestore> {
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
            self.emit_store_keep(&was_bound_var)?;

            let skip_save = self.instructions.len();
            self.instructions.push(Instruction::JumpIfFalse(0));
            self.instructions
                .push(Instruction::LoadVar(name.to_string().into()));
            self.emit_store(&old_var)?;
            let after_save = self.instructions.len();
            self.instructions[skip_save] = Instruction::JumpIfFalse(after_save);

            return Ok(LoopVarRestore::TopLevel {
                old_var,
                was_bound_var,
            });
        }

        if self.is_local(name)
            || self.defining_name.as_ref().is_some_and(|n| n == name)
            || self.capture_map.contains_key(name)
            || self.ref_capture_map.contains_key(name)
            || self.is_ref_default_name(name)
        {
            self.emit_load_const(Value::empty_list());
            self.emit_store(&old_var)?;
            self.emit_load(name, None)?;
            self.emit_store(&old_var)?;
            Ok(LoopVarRestore::Function {
                old_var,
                was_bound_var: None,
            })
        } else if self.isolated_module {
            let was_bound_var = format!("--vm-loop-was-bound-{name}-{id}");
            self.emit_load_const(Value::Bool(false));
            self.emit_store(&was_bound_var)?;
            self.emit_load_const(Value::empty_list());
            self.emit_store(&old_var)?;
            Ok(LoopVarRestore::Function {
                old_var,
                was_bound_var: Some(was_bound_var),
            })
        } else {
            let was_bound_var = format!("--vm-loop-was-bound-{name}-{id}");
            self.instructions
                .push(Instruction::LoadVarExists(name.to_string().into()));
            self.emit_store_keep(&was_bound_var)?;

            self.emit_load_const(Value::empty_list());
            self.emit_store(&old_var)?;

            self.instructions
                .push(Instruction::LoadVarExists(name.to_string().into()));
            let skip_save = self.instructions.len();
            self.instructions.push(Instruction::JumpIfFalse(0));
            self.instructions
                .push(Instruction::LoadVar(name.to_string().into()));
            self.emit_store(&old_var)?;
            let after_save = self.instructions.len();
            self.instructions[skip_save] = Instruction::JumpIfFalse(after_save);

            Ok(LoopVarRestore::Function {
                old_var,
                was_bound_var: Some(was_bound_var),
            })
        }
    }

    fn finish_loop_var_restore(&mut self, name: &str, restore: &LoopVarRestore) -> WqResult<()> {
        match restore {
            LoopVarRestore::TopLevel {
                old_var,
                was_bound_var,
            } => {
                self.emit_load(was_bound_var, None)?;
                let skip_restore = self.instructions.len();
                self.instructions.push(Instruction::JumpIfFalse(0));
                self.emit_load(old_var, None)?;
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
                    self.emit_load(was_bound_var, None)?;
                    let skip_restore = self.instructions.len();
                    self.instructions.push(Instruction::JumpIfFalse(0));
                    self.emit_load(old_var, None)?;
                    self.emit_store(name)?;
                    let end = self.instructions.len();
                    self.instructions[skip_restore] = Instruction::JumpIfFalse(end);
                } else {
                    self.emit_load(old_var, None)?;
                    self.emit_store(name)?;
                }
            }
        }
        Ok(())
    }

    fn emit_outer_load(&mut self, name: &str, span: Option<(usize, usize)>) -> WqResult<()> {
        if self.fn_depth == 0 || self.module_root {
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

    fn emit_store(&mut self, name: &str) -> WqResult<()> {
        if self.fn_depth > 0 {
            if self.is_ref_default_name(name) {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    self.instructions.push(Instruction::StoreCapture(*idx));
                } else {
                    self.instructions
                        .push(Instruction::StoreVar(name.to_string().into()));
                }
                return Ok(());
            }
            let idx = self.local_slot(name)?;
            self.instructions.push(Instruction::StoreLocal(idx));
        } else {
            self.instructions
                .push(Instruction::StoreVar(name.to_string().into()));
        }
        Ok(())
    }

    fn emit_store_keep(&mut self, name: &str) -> WqResult<()> {
        if self.fn_depth > 0 {
            if self.is_ref_default_name(name) {
                if let Some(idx) = self.ref_capture_map.get(name) {
                    self.instructions.push(Instruction::StoreCaptureKeep(*idx));
                } else {
                    self.instructions
                        .push(Instruction::StoreVarKeep(name.to_string().into()));
                }
                return Ok(());
            }
            let idx = self.local_slot(name)?;
            self.instructions.push(Instruction::StoreLocalKeep(idx));
        } else {
            self.instructions
                .push(Instruction::StoreVarKeep(name.to_string().into()));
        }
        Ok(())
    }

    fn emit_outer_store_keep(&mut self, name: &str, span: Option<(usize, usize)>) -> WqResult<()> {
        if self.fn_depth == 0 || self.module_root {
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
        | AstNode::UnpackValue { .. }
        | AstNode::Ellipsis(_)
        | AstNode::DictUnpackPattern(..)
        | AstNode::PipeInput
        | AstNode::Break(_)
        | AstNode::Continue(_)
        | AstNode::Import { .. }
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
        AstNode::LazyBool { operands, .. } => {
            for operand in operands {
                collect_ref_default_assignment_needs_inner(operand, available, excluded, needs);
            }
        }
        AstNode::ComparisonChain { first, rest, .. } => {
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
        AstNode::Cat(items, _) | AstNode::List(items, _) | AstNode::Block(items, _) => {
            for item in items {
                collect_ref_default_assignment_needs_inner(item, available, excluded, needs);
            }
        }
        AstNode::Dict(pairs, _) => {
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
        AstNode::NamedArg { value, .. } | AstNode::Debug { expr: value, .. } => {
            collect_ref_default_assignment_needs_inner(value, available, excluded, needs);
        }
        AstNode::Pause { expr, .. } | AstNode::Return(expr, _) => {
            if let Some(expr) = expr {
                collect_ref_default_assignment_needs_inner(expr, available, excluded, needs);
            }
        }
        AstNode::Try(expr, _) => {
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
        AstNode::UnpackAssignment { lhs, rhs, .. } => {
            collect_ref_default_assignment_needs_inner(rhs, available, excluded, needs);
            for target in lhs {
                collect_ref_default_unpack_target(target, available, excluded, needs);
            }
        }
        AstNode::FString { .. } => {
            unreachable!(
                "FString should have been resolved before collect_ref_default_assignment_needs"
            )
        }
    }
}

fn collect_ref_default_unpack_target(
    target: &AstNode,
    available: &IndexSet<String>,
    excluded: &IndexSet<String>,
    needs: &mut CaptureNeeds,
) {
    match target {
        AstNode::Variable(name, _) => {
            if available.contains(name) && !excluded.contains(name) {
                needs.by_ref.insert(name.clone());
            }
        }
        AstNode::Index { object, index, .. } => {
            collect_ref_default_assignment_needs_inner(object, available, excluded, needs);
            collect_ref_default_assignment_needs_inner(index, available, excluded, needs);
        }
        AstNode::Postfix { object, items, .. } => {
            collect_ref_default_assignment_needs_inner(object, available, excluded, needs);
            for item in items {
                collect_ref_default_assignment_needs_inner(item, available, excluded, needs);
            }
        }
        AstNode::List(items, _) => {
            for item in items {
                collect_ref_default_unpack_target(item, available, excluded, needs);
            }
        }
        AstNode::DictUnpackPattern(entries, _) => {
            for entry in entries {
                collect_ref_default_unpack_target(&entry.target, available, excluded, needs);
            }
        }
        AstNode::Ellipsis(_) => {}
        _ => collect_ref_default_assignment_needs_inner(target, available, excluded, needs),
    }
}

fn has_ctrl(node: &AstNode) -> bool {
    match node {
        AstNode::Break(_) | AstNode::Continue(_) | AstNode::Return(..) => true,
        AstNode::Debug { expr, .. } => has_ctrl(expr),
        AstNode::Pause { .. } => true,
        AstNode::Block(stmts, _)
        | AstNode::BlockExpr(stmts, _)
        | AstNode::Cat(stmts, _)
        | AstNode::List(stmts, _) => stmts.iter().any(has_ctrl),
        AstNode::Conditional {
            condition,
            true_branch,
            false_branch,
            ..
        } => {
            has_ctrl(condition)
                || has_ctrl(true_branch)
                || false_branch.as_ref().is_some_and(|branch| has_ctrl(branch))
        }
        AstNode::WLoop {
            condition, body, ..
        } => has_ctrl(condition) || has_ctrl(body),
        AstNode::NLoop { count, body, .. } => has_ctrl(count) || has_ctrl(body),
        AstNode::Function { body, .. } => has_ctrl(body),
        AstNode::UnaryOp { operand, .. } => has_ctrl(operand),
        AstNode::Pipe { input, effect, .. } => has_ctrl(input) || has_ctrl(effect),
        AstNode::PipeTap { input, effect, .. } => has_ctrl(input) || has_ctrl(effect),
        AstNode::Postfix { object, items, .. } => has_ctrl(object) || items.iter().any(has_ctrl),
        AstNode::BinaryOp { left, right, .. } => has_ctrl(left) || has_ctrl(right),
        AstNode::LazyBool { operands, .. } => operands.iter().any(has_ctrl),
        AstNode::ComparisonChain { first, rest, .. } => {
            has_ctrl(first) || rest.iter().any(|(_, node)| has_ctrl(node))
        }
        AstNode::CallName { args, .. } => args.iter().any(has_ctrl),
        AstNode::CallAnonymous { object, args, .. } => {
            has_ctrl(object) || args.iter().any(has_ctrl)
        }
        AstNode::Dict(pairs, _) => pairs.iter().any(|(_, v)| has_ctrl(v)),
        AstNode::DictUnpackPattern(entries, _) => {
            entries.iter().any(|entry| has_ctrl(&entry.target))
        }
        AstNode::ConditionalChain { .. } => {
            unreachable!("ConditionalChain should have been resolved before compilation")
        }
        AstNode::ConditionalDot { .. } => {
            unreachable!("ConditionalDot should have been resolved before compilation")
        }
        AstNode::Index { object, index, .. } | AstNode::MutatingIndex { object, index, .. } => {
            has_ctrl(object) || has_ctrl(index)
        }
        AstNode::Assignment { value, .. } | AstNode::OuterAssignment { value, .. } => {
            has_ctrl(value)
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
        } => has_ctrl(object) || has_ctrl(index) || has_ctrl(value),
        AstNode::Range {
            start, end, step, ..
        } => has_ctrl(start) || has_ctrl(end) || step.as_ref().is_some_and(|s| has_ctrl(s)),
        AstNode::NamedArg { value, .. } => has_ctrl(value),
        AstNode::Try(expr, _) => has_ctrl(expr),
        AstNode::Group { expr, .. } => has_ctrl(expr),
        AstNode::UnpackAssignment { lhs, rhs, .. } => has_ctrl(rhs) || lhs.iter().any(has_ctrl),
        AstNode::FString { .. } => {
            unreachable!("FString should have been resolved before compilation")
        }
        AstNode::Error(..)
        | AstNode::Literal(..)
        | AstNode::Import { .. }
        | AstNode::Variable(..)
        | AstNode::OuterVariable(..)
        | AstNode::UnpackValue { .. }
        | AstNode::Ellipsis(_)
        | AstNode::PipeInput => false,
    }
}

fn const_body_value(node: &AstNode) -> Option<Value> {
    match node {
        AstNode::Literal(value, ..) => Some(value.clone()),
        AstNode::Block(stmts, _) | AstNode::BlockExpr(stmts, _) => {
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
        AstNode::Block(stmts, _) | AstNode::BlockExpr(stmts, _) => {
            stmts.iter().all(pure_const_body)
        }
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
        | AstNode::Import { .. }
        | AstNode::Variable(_, _)
        | AstNode::OuterVariable(_, _)
        | AstNode::UnpackValue { .. }
        | AstNode::Ellipsis(_)
        | AstNode::Break(_)
        | AstNode::Continue(_) => node.clone(),
        AstNode::BinaryOp {
            left,
            operator,
            right,
            span,
        } => AstNode::BinaryOp {
            left: Box::new(replace_pipe_input(left, temp_name)),
            operator: *operator,
            right: Box::new(replace_pipe_input(right, temp_name)),
            span: *span,
        },
        AstNode::LazyBool {
            operator,
            operands,
            span,
        } => AstNode::LazyBool {
            operator: *operator,
            operands: operands
                .iter()
                .map(|operand| replace_pipe_input(operand, temp_name))
                .collect(),
            span: *span,
        },
        AstNode::ComparisonChain { first, rest, span } => AstNode::ComparisonChain {
            first: Box::new(replace_pipe_input(first, temp_name)),
            rest: rest
                .iter()
                .map(|(op, node)| (*op, replace_pipe_input(node, temp_name)))
                .collect(),
            span: *span,
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
            span,
        } => AstNode::Range {
            start: Box::new(replace_pipe_input(start, temp_name)),
            end: Box::new(replace_pipe_input(end, temp_name)),
            step: step
                .as_ref()
                .map(|step| Box::new(replace_pipe_input(step, temp_name))),
            inclusive: *inclusive,
            span: *span,
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
        AstNode::Cat(items, span) => AstNode::Cat(
            items
                .iter()
                .map(|item| replace_pipe_input(item, temp_name))
                .collect(),
            *span,
        ),
        AstNode::List(items, span) => AstNode::List(
            items
                .iter()
                .map(|item| replace_pipe_input(item, temp_name))
                .collect(),
            *span,
        ),
        AstNode::Dict(pairs, span) => AstNode::Dict(
            pairs
                .iter()
                .map(|(k, v)| (k.clone(), replace_pipe_input(v, temp_name)))
                .collect(),
            *span,
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
            span,
        } => AstNode::Function {
            params: params.clone(),
            ref_capture: *ref_capture,
            body: Box::new(replace_pipe_input(body, temp_name)),
            span: *span,
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
        AstNode::Return(expr, span) => AstNode::Return(
            expr.as_ref()
                .map(|expr| Box::new(replace_pipe_input(expr, temp_name))),
            *span,
        ),
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
        AstNode::Try(expr, span) => {
            AstNode::Try(Box::new(replace_pipe_input(expr, temp_name)), *span)
        }
        AstNode::Block(stmts, span) => AstNode::Block(
            stmts
                .iter()
                .map(|stmt| replace_pipe_input(stmt, temp_name))
                .collect(),
            *span,
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
        AstNode::DictUnpackPattern(entries, span) => AstNode::DictUnpackPattern(
            entries
                .iter()
                .map(|entry| crate::ast::DictUnpackEntry {
                    key: entry.key.clone(),
                    key_span: entry.key_span,
                    target: replace_pipe_input(&entry.target, temp_name),
                })
                .collect(),
            *span,
        ),
        AstNode::NamedArg { name, value, span } => AstNode::NamedArg {
            name: name.clone(),
            value: Box::new(replace_pipe_input(value, temp_name)),
            span: *span,
        },
        AstNode::UnpackAssignment { lhs, op, rhs, span } => AstNode::UnpackAssignment {
            lhs: lhs
                .iter()
                .map(|target| replace_pipe_input(target, temp_name))
                .collect(),
            op: *op,
            rhs: Box::new(replace_pipe_input(rhs, temp_name)),
            span: *span,
        },
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
        | AstNode::Import { .. }
        | AstNode::UnpackValue { .. }
        | AstNode::Ellipsis(_)
        | AstNode::DictUnpackPattern(..)
        | AstNode::PipeInput
        | AstNode::Break(_)
        | AstNode::Continue(_) => {}
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
        AstNode::LazyBool { operands, .. } => {
            for operand in operands {
                collect_capture_needs(operand, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::ComparisonChain { first, rest, .. } => {
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
                    ..
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
        AstNode::Cat(items, _) | AstNode::List(items, _) => {
            for item in items {
                collect_capture_needs(item, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Dict(pairs, _) => {
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
            ..
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
        AstNode::Return(expr, _) => {
            if let Some(expr) = expr {
                collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Debug { expr, .. } => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::Pause { expr, .. } => {
            if let Some(expr) = expr {
                collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Try(expr, _) => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::Block(stmts, _) | AstNode::BlockExpr(stmts, _) => {
            for stmt in stmts {
                collect_capture_needs(stmt, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::Group { expr, .. } => {
            collect_capture_needs(expr, locals, needs, ref_capture, defining_name);
        }
        AstNode::UnpackAssignment { lhs, op, rhs, .. } => {
            collect_capture_needs(rhs, locals, needs, ref_capture, defining_name);
            for target in lhs {
                collect_unpack_target_capture_needs(
                    target,
                    *op,
                    locals,
                    needs,
                    ref_capture,
                    defining_name,
                );
            }
        }
        AstNode::FString { .. } => {
            unreachable!("FString should have been resolved before collect_capture_needs")
        }
    }
}

fn collect_unpack_target_capture_needs(
    target: &AstNode,
    op: Option<BinaryOperator>,
    locals: &mut IndexSet<String>,
    needs: &mut CaptureNeeds,
    ref_capture: bool,
    defining_name: Option<&str>,
) {
    match target {
        AstNode::Variable(name, _) => {
            if op.is_some() && !scope_has(locals, name) && defining_name != Some(name.as_str()) {
                if ref_capture {
                    needs.by_ref.insert(name.clone());
                } else {
                    needs.by_value.insert(name.clone());
                }
            }
            locals.insert(name.clone());
        }
        AstNode::Index { object, index, .. } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            collect_capture_needs(index, locals, needs, ref_capture, defining_name);
        }
        AstNode::Postfix { object, items, .. } => {
            collect_capture_needs(object, locals, needs, ref_capture, defining_name);
            for item in items {
                collect_capture_needs(item, locals, needs, ref_capture, defining_name);
            }
        }
        AstNode::List(items, _) => {
            for item in items {
                collect_unpack_target_capture_needs(
                    item,
                    op,
                    locals,
                    needs,
                    ref_capture,
                    defining_name,
                );
            }
        }
        AstNode::DictUnpackPattern(entries, _) => {
            for entry in entries {
                collect_unpack_target_capture_needs(
                    &entry.target,
                    op,
                    locals,
                    needs,
                    ref_capture,
                    defining_name,
                );
            }
        }
        AstNode::Ellipsis(_) => {}
        _ => collect_capture_needs(target, locals, needs, ref_capture, defining_name),
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
        compile_source_with_globals(src, Default::default())
    }

    fn compile_source_with_globals(
        src: &str,
        globals: crate::vm::GlobalMap,
    ) -> (Vec<Instruction>, Vec<Option<(usize, usize)>>) {
        let mut lexer = Lexer::new(src);
        let tokens = lexer.tokenize().expect("tokenize");
        let builtins = crate::builtins::Builtins::new();
        let mut parser = Parser::new_with_builtins(tokens, src.to_string(), builtins.clone());
        let ast = parser.parse().expect("parse");
        let mut resolver = Resolver::from_env(globals.clone(), builtins.clone());
        let ast = resolver.resolve(ast);
        let ast = fold::fold(ast);
        let mut compiler = Compiler::new_with_builtins(builtins);
        compiler.compile(&ast).expect("compile");
        compiler.propagate_constants_with_globals(&globals);
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

    #[test]
    fn unresolved_pipe_is_an_internal_compiler_error() {
        let node = AstNode::Pipe {
            input: Box::new(AstNode::Literal(Value::Int(1), None)),
            effect: Box::new(AstNode::Variable("echo".to_string(), None)),
            kind: crate::ast::PipeKind::Pipe,
            span: None,
        };
        let err = Compiler::new()
            .compile(&node)
            .expect_err("unresolved pipe should fail compilation");

        assert_eq!(err.err_type, WqErrorType::Vm);
        assert_eq!(
            err.msg.as_deref(),
            Some("internal compiler error while compiling an unresolved pipe")
        );
    }

    #[test]
    fn invalid_index_assignment_target_is_an_internal_compiler_error() {
        let node = AstNode::IndexAssign {
            object: Box::new(AstNode::Literal(Value::Int(1), None)),
            index: Box::new(AstNode::Literal(Value::Int(0), None)),
            op: None,
            value: Box::new(AstNode::Literal(Value::Int(2), None)),
            span: None,
        };
        let err = Compiler::new()
            .compile(&node)
            .expect_err("invalid index assignment target should fail compilation");

        assert_eq!(err.err_type, WqErrorType::Vm);
        assert_eq!(
            err.msg.as_deref(),
            Some("internal compiler error while compiling an index assignment")
        );
    }

    #[test]
    fn destructuring_compiles_to_an_anonymous_extraction_plan() {
        let insts = compile_source("source:(1;(`key:2));(a;(`key:b)):source");
        let unpack = insts
            .iter()
            .find_map(|inst| match inst {
                Instruction::Unpack(plan) => Some(plan.as_ref()),
                _ => None,
            })
            .expect("destructuring should emit an unpack plan");

        assert_eq!(
            unpack.paths.as_ref(),
            [
                Box::from([UnpackPathSegment::Index(0)]),
                Box::from([
                    UnpackPathSegment::Index(1),
                    UnpackPathSegment::Key(Arc::from("key")),
                ]),
            ]
        );
        assert!(
            insts
                .iter()
                .any(|inst| matches!(inst, Instruction::LoadUnpack(1)))
        );
        assert!(
            insts
                .iter()
                .any(|inst| matches!(inst, Instruction::LoadUnpack(2)))
        );
        assert!(
            insts.iter().all(|inst| !matches!(
                inst,
                Instruction::StoreVar(name) | Instruction::StoreVarKeep(name)
                    if name.starts_with("--")
            )),
            "unpack values should never be stored under synthetic names: {insts:#?}"
        );
        assert!(
            insts
                .iter()
                .any(|inst| matches!(inst, Instruction::EndUnpack))
        );
    }

    #[test]
    fn valid_literal_destructuring_compiles_to_direct_constant_stores() {
        let insts = compile_source("(a;_;(`key:b)):(1;2;(`key:3))");

        assert!(
            insts.iter().all(|inst| !matches!(
                inst,
                Instruction::Unpack(_)
                    | Instruction::LoadUnpack(_)
                    | Instruction::EndUnpack
                    | Instruction::MakeList(_)
            )),
            "literal destructuring should not create a runtime extraction plan: {insts:#?}"
        );
        assert!(
            insts.iter().any(
                |inst| matches!(inst, Instruction::LoadConst(value) if value.as_ref() == &Value::Int(1))
            ),
            "first extracted constant should be loaded directly: {insts:#?}"
        );
        assert!(
            insts.iter().any(
                |inst| matches!(inst, Instruction::LoadConst(value) if value.as_ref() == &Value::Int(3))
            ),
            "nested extracted constant should be loaded directly: {insts:#?}"
        );
        assert!(
            insts.iter().any(|inst| matches!(
                inst,
                Instruction::StoreVar(name) | Instruction::StoreVarKeep(name) if &**name == "a"
            )),
            "first target should use a direct store: {insts:#?}"
        );
        assert!(
            insts
                .iter()
                .any(|inst| matches!(inst, Instruction::StoreVarKeep(name) if &**name == "b")),
            "last target should use a direct store that preserves the expression value: {insts:#?}"
        );
    }

    #[test]
    fn invalid_literal_destructuring_keeps_runtime_preflight() {
        let insts = compile_source("(a;b):,1");

        assert!(
            insts
                .iter()
                .any(|inst| matches!(inst, Instruction::Unpack(_))),
            "an invalid literal path should retain runtime unpack diagnostics: {insts:#?}"
        );
    }

    #[test]
    fn depth_arity_error_quotes_the_source_form() {
        let err = compile_source_err("zip@1[(1;2)]");

        assert_eq!(err.msg.as_deref(), Some("'zip@1' expects 2 arguments"));
    }

    #[test]
    fn n_loop_count_uses_public_int_term() {
        let err = compile_source_err("N[1.5;1]");

        assert_eq!(err.msg.as_deref(), Some("expected int"));
        assert_eq!(
            err.notes.as_slice(),
            ["for an n-loop count", "got 1.5 (float)"]
        );
    }

    #[test]
    fn dynamic_function_n_loop_uses_local_control_instructions() {
        let insts = compile_source("run:{[n]total:0;N[n;total+:_n];total}");
        let func = compiled_function_in(&insts);

        assert!(
            func.instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::NLoopEnter(_))),
            "expected fused N-loop entry in {:#?}",
            func.instructions
        );
        assert!(
            func.instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::NLoopNext(_))),
            "expected fused N-loop advance in {:#?}",
            func.instructions
        );
    }

    fn builtin_id(name: &str) -> u16 {
        let id = crate::builtins::Builtins::new()
            .get_id(name)
            .unwrap_or_else(|| panic!("missing builtin {name}"));
        u16::try_from(id).expect("builtin id fits in u16")
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

    #[test]
    fn captured_store_drop_is_fused_without_a_following_pop() {
        let top = compile_source("f:{a:1;g:'{[]a:2;0};g}");
        let mut compiler = Compiler::new();
        compiler.dbg_pc_spans.resize(top.len(), None);
        compiler.instructions = top;
        compiler.fuse();
        let outer = compiled_function_in(&compiler.instructions);
        let inner = first_closure_payload(outer.instructions.as_ref());

        assert!(
            inner
                .instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::StoreCapture(0))),
            "expected direct captured drop store: {:#?}",
            inner.instructions
        );
        assert!(
            !inner
                .instructions
                .windows(2)
                .any(|pair| matches!(pair, [Instruction::StoreCaptureKeep(_), Instruction::Pop]))
        );
    }

    #[test]
    fn direct_captured_drop_store_is_one_instruction() {
        let mut compiler = Compiler::new();
        compiler.fn_depth = 1;
        compiler.ref_default_names.insert("a".to_string());
        compiler.ref_capture_map.insert("a".to_string(), 3);
        compiler.emit_load_const(Value::Int(9));

        compiler.emit_store("a").expect("emit captured store");

        assert_eq!(
            compiler.instructions,
            vec![
                Instruction::load_const(Value::Int(9)),
                Instruction::StoreCapture(3),
            ]
        );
    }

    #[test]
    fn final_captured_store_keeps_its_expression_value() {
        let top = compile_source("f:{a:1;g:'{[]a:2};g}");
        let outer = compiled_function_in(&top);
        let inner = first_closure_payload(outer.instructions.as_ref());

        assert!(
            inner
                .instructions
                .iter()
                .any(|inst| matches!(inst, Instruction::StoreCaptureKeep(0)))
        );
    }

    fn last_closure_payload(insts: &[Instruction]) -> &crate::vm::inst::ClosurePayload {
        insts
            .iter()
            .filter_map(|inst| match inst {
                Instruction::LoadClosure(payload) => Some(payload.as_ref()),
                _ => None,
            })
            .next_back()
            .expect("expected closure payload")
    }

    fn slot_named(names: &[String], name: &str) -> u16 {
        let slot = names
            .iter()
            .position(|local| local == name)
            .expect("expected local slot");
        u16::try_from(slot).expect("local slot fits in u16")
    }

    #[test]
    fn local_cat_assignment_uses_owned_update_instruction() {
        let top = compile_source("f:{xs:();xs,:1;xs}");
        let function = compiled_function_in(&top);
        let xs_slot = slot_named(
            function
                .dbg_local_names
                .as_deref()
                .expect("local names should exist"),
            "xs",
        );

        assert!(function.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::CatAssign(data)
                    if data.target == StoreTarget::Local(xs_slot)
                        && matches!(&data.right, Operand::Const(value) if **value == Value::Int(1))
            )
        }));
    }

    #[test]
    fn effectful_cat_assignment_keeps_snapshot_lowering() {
        let top = compile_source("f:{xs:(1;2);next:{xs:9;3};xs,:next[];xs}");
        let function = compiled_function_in(&top);

        assert!(
            function
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Cat(2)))
        );
        assert!(
            !function
                .instructions
                .iter()
                .any(|instruction| matches!(instruction, Instruction::CatAssign(_)))
        );
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
        assert_eq!(h.captures, vec![Capture::FromCapture(0)]);
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
    fn ref_rebinding_function_name_loads_target_before_dynamic_postfix() {
        let top = compile_source("f:{'f:1};f[0];f[0]");

        let dynamic_dispatches = top
            .iter()
            .filter(|inst| matches!(inst, Instruction::Postfix(1)))
            .count();

        assert_eq!(dynamic_dispatches, 2);
        assert!(
            !top.iter()
                .any(|inst| matches!(inst, Instruction::CallUser(name, 1) if name.as_ref() == "f"))
        );
    }

    #[test]
    fn constant_tag_method_calls_use_loaded_targets() {
        let top = compile_source("d:(`f:{[x]x+1});d[`f][2];d[`f][]");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::Postfix(1)))
        );
        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::CallAnon(0)))
        );
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
    fn discarded_map_uses_discard_builtin_call() {
        let top = compile_source("til 3|M{x};42");
        let map_alias_id = builtin_id("M");

        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::CallBuiltinDiscardId(id, argc)
                    if *id == map_alias_id && *argc == 2)
            ),
            "expected discarded M call: {top:#?}",
        );
    }

    #[test]
    fn discarded_apply_uses_discard_builtin_call() {
        let top = compile_source("apply[({x};{x+1});3];42");
        let apply_id = builtin_id("apply");

        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::CallBuiltinDiscardId(id, argc)
                    if *id == apply_id && *argc == 2)
            ),
            "expected discarded apply call: {top:#?}",
        );
    }

    #[test]
    fn discarded_filter_uses_discard_builtin_call() {
        let top = compile_source("filter[(1;2;3);{x>1}];42");
        let filter_id = builtin_id("filter");

        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::CallBuiltinDiscardId(id, argc)
                    if *id == filter_id && *argc == 2)
            ),
            "expected discarded filter call: {top:#?}",
        );
    }

    #[test]
    fn value_needed_map_uses_normal_builtin_call() {
        let top = compile_source("til 3|M{x}");
        let map_alias_id = builtin_id("M");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::CallBuiltinId(id, argc)
                    if *id == map_alias_id && *argc == 2)),
            "expected value-producing M call: {top:#?}",
        );
        assert!(
            !top.iter().any(
                |inst| matches!(inst, Instruction::CallBuiltinDiscardId(id, _)
                    if *id == map_alias_id)
            ),
            "final M call should keep its value: {top:#?}",
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
    fn optimized_multi_index_load_avoids_helper_list_materialization() {
        let top = compile_source("xs:(10;20;30); xs[0;2]");

        assert!(
            top.windows(3).any(|insts| matches!(
                insts,
                [
                    Instruction::LoadConst(first),
                    Instruction::LoadConst(second),
                    Instruction::IndexManyLoadVar(name, 2),
                ] if **first == Value::Int(0)
                    && **second == Value::Int(2)
                    && name.as_ref() == "xs"
            )),
            "expected atom index args followed by IndexManyLoadVar: {top:#?}",
        );
        assert!(
            !top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [0, 2])
            )),
            "synthetic multi-index args should not materialize as a list: {top:#?}",
        );
    }

    #[test]
    fn explicit_list_index_still_materializes_list_key() {
        let top = compile_source("xs:(10;20;30); xs[(0;2)]");

        assert!(
            top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [0, 2])
            )),
            "explicit list index should stay a list key: {top:#?}",
        );
        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::IndexLoadVar(name) if name.as_ref() == "xs")
            ),
            "explicit list index should use single-index load: {top:#?}",
        );
    }

    #[test]
    fn literal_multi_index_assign_uses_packed_const_key() {
        let top = compile_source("xs:(10;20;30); xs[0;2]:99");

        assert!(
            top.windows(3).any(|insts| matches!(
                insts,
                [
                    Instruction::LoadConst(key),
                    Instruction::LoadConst(value),
                    Instruction::IndexAssignVar(name),
                ] if matches!(&**key, Value::IntList(items) if items.as_slice() == [0, 2])
                    && **value == Value::Int(99)
                    && name.as_ref() == "xs"
            )),
            "literal integer multi-index assignment should use a packed const key: {top:#?}",
        );
    }

    #[test]
    fn dynamic_multi_index_assign_avoids_helper_list_materialization() {
        let top = compile_source("i:0;xs:(10;20;30); xs[i;2]:99");

        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::IndexManyAssignVar(name, 2)
                    if name.as_ref() == "xs")
            ),
            "expected IndexManyAssignVar for dynamic args: {top:#?}",
        );
        assert!(
            !top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [0, 2])
            )),
            "synthetic multi-index assignment args should not materialize as a list: {top:#?}",
        );
    }

    #[test]
    fn literal_multi_index_augmented_assign_uses_packed_const_key() {
        let top = compile_source("xs:(10;20;30); xs[0;2]+:1");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexLoadVar(name)
                    if name.as_ref() == "xs")),
            "expected IndexLoadVar for literal-key augmented assignment read: {top:#?}",
        );
        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexAssignVar(name)
                    if name.as_ref() == "xs")),
            "expected IndexAssignVar for literal-key augmented assignment write: {top:#?}",
        );
        let key_loads = top
            .iter()
            .filter(|inst| {
                matches!(
                    inst,
                    Instruction::LoadConst(value)
                        if matches!(&**value, Value::IntList(items) if items.as_slice() == [0, 2])
                )
            })
            .count();
        assert!(
            key_loads >= 2,
            "literal augmented assignment should reload the packed key for read and write: {top:#?}",
        );
    }

    #[test]
    fn dynamic_multi_index_augmented_assign_avoids_helper_list_materialization() {
        let top = compile_source("i:0;xs:(10;20;30); xs[i;2]+:1");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexManyLoadVar(name, 2)
                    if name.as_ref() == "xs")),
            "expected IndexManyLoadVar for dynamic augmented assignment read: {top:#?}",
        );
        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::IndexManyAssignVar(name, 2)
                    if name.as_ref() == "xs")
            ),
            "expected IndexManyAssignVar for dynamic augmented assignment write: {top:#?}",
        );
        assert!(
            !top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [0, 2])
            )),
            "synthetic augmented assignment args should not materialize as a list: {top:#?}",
        );
    }

    #[test]
    fn explicit_list_index_assign_still_materializes_list_key() {
        let top = compile_source("xs:(10;20;30); xs[(0;2)]:99");

        assert!(
            top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [0, 2])
            )),
            "explicit list assignment index should stay a list key: {top:#?}",
        );
        assert!(
            top.iter().any(
                |inst| matches!(inst, Instruction::IndexAssignVar(name) if name.as_ref() == "xs")
            ),
            "explicit list assignment index should use single-index assign: {top:#?}",
        );
    }

    #[test]
    fn nested_path_final_literal_multi_index_assign_uses_packed_const_key() {
        let top = compile_source("xs:((10;20;30);(40;50;60)); xs[0][1;2]:99");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexAssignVarDrop(_))),
            "expected final path segment to use single-key assignment: {top:#?}",
        );
        assert!(
            top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [1, 2])
            )),
            "literal final path index should compile to a packed const key: {top:#?}",
        );
    }

    #[test]
    fn nested_path_final_dynamic_multi_index_assign_avoids_helper_list_materialization() {
        let top = compile_source("j:1;xs:((10;20;30);(40;50;60)); xs[0][j;2]:99");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexManyAssignVarDrop(_, 2))),
            "expected final dynamic path segment to use IndexManyAssignVarDrop: {top:#?}",
        );
        assert!(
            !top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [1, 2])
            )),
            "synthetic final path index should not materialize as a list: {top:#?}",
        );
    }

    #[test]
    fn nested_path_explicit_list_final_index_still_materializes_list_key() {
        let top = compile_source("xs:((10;20;30);(40;50;60)); xs[0][(1;2)]:99");

        assert!(
            top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [1, 2])
            )),
            "explicit final path list index should stay a list key: {top:#?}",
        );
        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexAssignVarDrop(_))),
            "explicit final path list index should use single-index assignment: {top:#?}",
        );
    }

    #[test]
    fn nested_path_final_literal_multi_index_augmented_assign_uses_packed_const_key() {
        let top = compile_source("xs:((10;20;30);(40;50;60)); xs[0][1;2]+:1");

        assert!(
            top.iter().any(|inst| matches!(inst, Instruction::Index)),
            "expected final path augmented read to use single-key index: {top:#?}",
        );
        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexAssignVarDrop(_))),
            "expected final path augmented write to use single-key assignment: {top:#?}",
        );
        assert!(
            top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [1, 2])
            )),
            "literal final path augmented index should compile to a packed const key: {top:#?}",
        );
    }

    #[test]
    fn nested_path_final_dynamic_multi_index_augmented_assign_avoids_helper_list_materialization() {
        let top = compile_source("j:1;xs:((10;20;30);(40;50;60)); xs[0][j;2]+:1");

        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexMany(2))),
            "expected final dynamic path augmented read to use IndexMany: {top:#?}",
        );
        assert!(
            top.iter()
                .any(|inst| matches!(inst, Instruction::IndexManyAssignVarDrop(_, 2))),
            "expected final dynamic path augmented write to use IndexManyAssignVarDrop: {top:#?}",
        );
        assert!(
            !top.iter().any(|inst| matches!(
                inst,
                Instruction::LoadConst(value)
                    if matches!(&**value, Value::IntList(items) if items.as_slice() == [1, 2])
            )),
            "synthetic final path augmented index should not materialize as a list: {top:#?}",
        );
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
    fn const_propagation_clears_global_facts_across_composed_callable_calls() {
        let top = compile_source("a:1;f:{[x]'a:2;x}+1;g:f;g[0];h:{a+3};h");
        let func = last_closure_payload(&top);

        assert!(
            func.instructions.iter().any(|inst| matches!(
                inst,
                Instruction::BinaryOp(data)
                    if matches!(data.left, Operand::Capture(_))
                        || matches!(data.right, Operand::Capture(_))
            )),
            "composed callable call can mutate globals, so captured a must remain dynamic: {:#?}",
            func.instructions
        );
        assert!(
            !func.instructions.iter().any(
                |inst| matches!(inst, Instruction::LoadConst(value) if **value == Value::Int(4))
            ),
            "captured a must not fold through composed callable call: {:#?}",
            func.instructions
        );
    }

    #[test]
    fn const_propagation_uses_seeded_global_values() {
        let globals = crate::vm::GlobalMap::from_iter([("a".to_string(), Value::Int(1))]);
        let (top, _) = compile_source_with_globals("b:a+2\nb", globals);

        assert!(
            top.windows(2).any(|pair| matches!(
                (&pair[0], &pair[1]),
                (Instruction::LoadConst(value), Instruction::StoreVarKeep(name))
                    if **value == Value::Int(3) && name.as_ref() == "b"
            )),
            "streamed assignment should fold using seeded global a: {top:#?}",
        );
    }

    #[test]
    fn const_propagation_seeds_global_values_into_closure_captures() {
        let globals = crate::vm::GlobalMap::from_iter([("a".to_string(), Value::Int(1))]);
        let (top, _) = compile_source_with_globals("f:{a+2}\nf", globals);

        let func = first_closure_payload(&top);
        assert!(
            matches!(
                func.captures.as_slice(),
                [Capture::Global(name, _)] if name.as_str() == "a"
            ),
            "expected closure to capture global a: {:#?}",
            func.captures,
        );
        assert!(
            func.instructions.iter().any(
                |inst| matches!(inst, Instruction::LoadConst(value) if **value == Value::Int(3))
            ),
            "streamed closure should fold captured global a: {:#?}",
            func.instructions,
        );
    }

    #[test]
    fn last_function_statement_reports_its_own_span() {
        let err = compile_source_err("f:{x:1;W[true;x:2]x*:2}");
        let display = err.to_string();
        assert!(err.span.is_some(), "expected span");
        assert!(display.contains("at ?:1:20"), "display was: {display}");
    }

    #[test]
    fn named_parameter_mask_rejects_more_than_sixty_four_names() {
        let params = (0..65)
            .map(|idx| format!("`p{idx}:0"))
            .collect::<Vec<_>>()
            .join(";");
        let src = format!("f:{{[{params}]0}}");

        let err = compile_source_err(&src);

        assert!(
            err.to_string()
                .contains("function has too many named parameters"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn named_default_prologue_uses_fused_mask_branch() {
        let insts = compile_source("f:{[`x:1]x}");
        let func = compiled_function_in(&insts);

        assert_eq!(func.instructions.len(), 6);
        assert_eq!(
            func.instructions[0],
            Instruction::JumpIfNamedProvided(1, 0, 4)
        );
        assert_eq!(func.instructions[1], Instruction::load_const(Value::Int(1)));
        assert_eq!(func.instructions[2], Instruction::StoreLocal(0));
        assert_eq!(func.instructions[3], Instruction::Pop);
    }

    #[test]
    fn call_rejects_duplicate_named_arguments() {
        let err = compile_source_err("f:{[`x:0]x};f[`x:1;`x:2]");

        assert_eq!(err.err_type, WqErrorType::Syntax);
        assert_eq!(err.msg.as_deref(), Some("duplicate named argument 'x'"));
    }

    #[test]
    fn local_slot_allocation_rejects_more_than_u16_max_slots() {
        let mut compiler = Compiler::new();
        for idx in 0..=u16::MAX {
            compiler
                .local_slot(&format!("slot{idx}"))
                .expect("slot should fit");
        }

        let err = compiler
            .local_slot("overflow")
            .expect_err("extra slot should fail");

        assert!(
            err.to_string()
                .contains("function has too many local slots"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn capture_slot_allocation_rejects_more_than_u16_max_slots() {
        let mut compiler = Compiler::new();
        for idx in 0..=u16::MAX {
            compiler
                .captures
                .push(Capture::Global(format!("g{idx}"), None));
        }

        let err = compiler
            .next_capture_slot()
            .expect_err("extra capture should fail");

        assert!(
            err.to_string()
                .contains("function captures too many bindings"),
            "unexpected error: {err}"
        );
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
