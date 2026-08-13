use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::ast::{BinaryOperator, BoolOperator, UnaryOperator};
use crate::builtins::Builtins;
use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::value::Value;
use crate::value::unpack::UnpackPathSegment;
use crate::wqdb::data::ChunkId;
use crate::wqdb::{DebugInstruction, InstructionClass};

#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    Local(u16),
    LocalShared(u16),
    FromCapture(u16),
    Global(String, Option<(usize, usize)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosurePayload {
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) named_params: Option<Arc<[Arc<str>]>>,
    pub(crate) locals: u16,
    pub(crate) isolated_module: bool,
    pub(crate) captures: Vec<Capture>,
    pub(crate) instructions: Arc<[Instruction]>,
    /// Debug chunk id registered for this closure payload's code.
    pub(crate) dbg_chunk: Option<ChunkId>,
    /// Statement spans for the function body (byte start,end in source)
    pub(crate) dbg_stmt_spans: Arc<[(usize, usize)]>,
    /// Exact per-pc statement spans when available.
    pub(crate) dbg_pc_spans: Arc<[Option<(usize, usize)>]>,
    /// Exact statement-ending PCs for debugger/backtrace mapping.
    pub(crate) dbg_stmt_marks: Arc<[DebugStmtMark]>,
    /// Local variable names by slot index (for wqdb)
    pub(crate) dbg_local_names: Arc<[String]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugStmtMark {
    pub(crate) pc: usize,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

/// Operand that can be embedded directly into an instruction instead of
/// requiring a separate `Load*` instruction.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Operand {
    /// Pop the top value from the stack.
    Stack,
    /// A constant value.
    Const(Box<Value>),
    /// A local variable slot.
    Local(u16),
    /// A captured variable slot.
    Capture(u16),
    /// A global variable or builtin by name.
    Var(Arc<str>),
    /// The current function/closure being executed.
    Self_,
}

/// Named argument metadata for a call site.
///
/// Positional args and named args live on the stack in source order
/// (left-to-right evaluation).  This metadata tells the call machinery
/// which stack positions hold named args and what parameter they target.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NamedArgMeta {
    /// How many of the arguments on the stack are positional.
    pub(crate) pos_count: u16,
    /// Pairs of `(stack_position_in_source_order, param_name)` for each
    /// named argument.
    pub(crate) named: Box<[(u16, Arc<str>)]>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BinaryOpData {
    pub(crate) op: BinaryOperator,
    pub(crate) left: Operand,
    pub(crate) right: Operand,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CatAssignData {
    pub(crate) target: StoreTarget,
    pub(crate) right: Operand,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CmpBranchData {
    pub(crate) op: BinaryOperator,
    pub(crate) left: Operand,
    pub(crate) right: Operand,
    pub(crate) target: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UnaryOpData {
    pub(crate) op: UnaryOperator,
    pub(crate) operand: Operand,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NLoopEnterData {
    pub(crate) index: u16,
    pub(crate) count: u16,
    pub(crate) snapshot: u16,
    pub(crate) target: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NLoopNextData {
    pub(crate) snapshot: u16,
    pub(crate) index: u16,
    pub(crate) target: usize,
}

/// Mutating index operation kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MutationOp {
    Pop,
    Remove,
    Insert,
    InsertAt,
}

/// Storage target for mutation instructions.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StoreTarget {
    Local(u16),
    Capture(u16),
    Var(Arc<str>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImportData {
    pub(crate) specifier: Arc<str>,
    pub(crate) importer: Arc<str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnpackPlan {
    pub(crate) paths: Box<[Box<[UnpackPathSegment]>]>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Instruction {
    LoadConst(Box<Value>),
    /// Load a constant moved into the VM-owned one-shot constant pool.
    LoadOwnedConst(usize),
    /// Load a closure capturing current local slots
    LoadClosure(Box<ClosurePayload>),
    /// Load a global variable or builtin by name
    LoadVar(Arc<str>),
    /// Snapshot a named call target without adding a separate trace probe.
    LoadCallTarget(Operand),
    /// Push whether a global variable is currently bound
    LoadVarExists(Arc<str>),
    /// Load a captured value by index from the current closure frame
    LoadCapture(u16),
    /// Load the current function/closure being executed
    LoadSelf,
    /// Store into a global variable by name
    StoreVar(Arc<str>),
    /// Store into a global variable and keep value on stack
    StoreVarKeep(Arc<str>),
    /// Remove a global variable binding
    // DeleteVar(Arc<str>),
    /// Load a local variable from an index-based slot
    LoadLocal(u16),
    /// Store into a local variable slot
    StoreLocal(u16),
    /// Store into a local slot and keep value on stack
    StoreLocalKeep(u16),
    /// Store into a captured variable slot and keep value on stack
    StoreCaptureKeep(u16),
    /// Extract every path from one source value before exposing any result.
    Unpack(Box<UnpackPlan>),
    /// Load one value from the active anonymous unpack frame.
    LoadUnpack(usize),
    /// Discard the active anonymous unpack frame.
    EndUnpack,
    BinaryOp(Box<BinaryOpData>),
    /// Concatenate into an existing binding while retaining assignment value
    /// semantics.
    CatAssign(Box<CatAssignData>),
    /// Evaluate a chain of comparison operators; expects N+1 operands
    CmpChain(Box<[BinaryOperator]>),
    /// Concatenate N items from the stack into a single value
    Cat(usize),
    UnaryOp(Box<UnaryOpData>),
    /// Short-circuit bool and lazy check (A[...])
    BoolAndLazy(usize),
    /// Short-circuit bool or lazy check (O[...])
    BoolOrLazy(usize),
    /// Combine two already-evaluated lazy bool operands from the stack.
    BoolCombine(BoolOperator),
    // CallBuiltin(String, usize),
    /// Builtin call resolved to an ID at compile time for faster dispatch
    CallBuiltinId(u16, u16),
    /// Builtin call whose result is only needed as an empty list because the
    /// expression result will be discarded.
    CallBuiltinDiscardId(u16, u16),
    /// Call a function stored in a local slot
    CallLocal(u16, usize),
    CallUser(Arc<str>, usize),
    TailCallLocal(u16, usize),
    TailCallUser(Arc<str>, usize),
    CallAnon(usize),
    TailCallAnon(usize),
    /// Call if the object is a function, otherwise index
    Postfix(usize),
    TailPostfix(usize),
    MakeList(usize),
    MakeDict(usize),

    MakeRange {
        inclusive: bool,
        has_next: bool,
    },
    Index,
    IndexMany(usize),
    CheckAtomPathIndex,
    IndexLoadLocal(u16),
    IndexLoadCapture(u16),
    IndexLoadVar(Arc<str>),
    IndexManyLoadLocal(u16, usize),
    IndexManyLoadCapture(u16, usize),
    IndexManyLoadVar(Arc<str>, usize),
    IndexAssignVar(Arc<str>),
    IndexAssignLocal(u16),
    IndexAssignCapture(u16),
    IndexManyAssignVar(Arc<str>, usize),
    IndexManyAssignLocal(u16, usize),
    IndexManyAssignCapture(u16, usize),
    IndexAssignVarDrop(Arc<str>),
    IndexAssignLocalDrop(u16),
    IndexAssignCaptureDrop(u16),
    IndexManyAssignVarDrop(Arc<str>, usize),
    IndexManyAssignLocalDrop(u16, usize),
    IndexManyAssignCaptureDrop(u16, usize),
    Jump(usize),
    JumpIfFalse(usize),
    /// Evaluate a comparison and jump if its bool result is false.
    JumpIfCmpFalse(Box<CmpBranchData>),
    /// Enter one local N-loop iteration or jump to the loop exit.
    NLoopEnter(Box<NLoopEnterData>),
    /// Advance a local N-loop index from its iteration snapshot and jump back.
    NLoopNext(Box<NLoopNextData>),
    /// Jump if left >= right (pops two operands)
    JumpIfGE(usize),
    /// Jump if local slot value <= 0
    JumpIfLEZLocal(u16, usize),
    /// Jump when bit N is set in the hidden named-argument mask local.
    JumpIfNamedProvided(u16, u8, usize),
    /// Unified mutating index instruction: pop, remove, or insert.
    IndexMutate {
        target: StoreTarget,
        op: MutationOp,
    },
    Pop,
    Return,
    /// Open a value-provenance trace scope.  The closing `Debug` instruction
    /// renders the recorded probes as a tree and prints the final value.
    TraceBegin,
    Debug,
    Pause,
    Try(usize),
    Import(Box<ImportData>),
    /// A call carrying its named-argument metadata.
    NamedCall {
        call: Box<Instruction>,
        meta: Arc<NamedArgMeta>,
    },
    /// Store into a captured variable slot
    StoreCapture(u16),
}

impl Instruction {
    pub(crate) fn load_const(v: Value) -> Self {
        Self::LoadConst(Box::new(v))
    }

    pub(crate) fn load_closure(payload: ClosurePayload) -> Self {
        Self::LoadClosure(Box::new(payload))
    }

    pub(crate) fn with_named_args(self, meta: Option<Arc<NamedArgMeta>>) -> Self {
        match meta {
            Some(meta) => Self::NamedCall {
                call: Box::new(self),
                meta,
            },
            None => self,
        }
    }

    pub(crate) fn call_instruction(&self) -> &Self {
        match self {
            Self::NamedCall { call, .. } => call,
            _ => self,
        }
    }

    pub(crate) fn binary_op(op: BinaryOperator, left: Operand, right: Operand) -> Self {
        Self::BinaryOp(Box::new(BinaryOpData { op, left, right }))
    }

    pub(crate) fn cat_assign(target: StoreTarget, right: Operand) -> Self {
        Self::CatAssign(Box::new(CatAssignData { target, right }))
    }

    pub(crate) fn jump_if_cmp_false(
        op: BinaryOperator,
        left: Operand,
        right: Operand,
        target: usize,
    ) -> Self {
        Self::JumpIfCmpFalse(Box::new(CmpBranchData {
            op,
            left,
            right,
            target,
        }))
    }

    pub(crate) fn n_loop_enter(index: u16, count: u16, snapshot: u16, target: usize) -> Self {
        Self::NLoopEnter(Box::new(NLoopEnterData {
            index,
            count,
            snapshot,
            target,
        }))
    }

    pub(crate) fn n_loop_next(snapshot: u16, index: u16, target: usize) -> Self {
        Self::NLoopNext(Box::new(NLoopNextData {
            snapshot,
            index,
            target,
        }))
    }

    pub(crate) fn unary_op(op: UnaryOperator, operand: Operand) -> Self {
        Self::UnaryOp(Box::new(UnaryOpData { op, operand }))
    }

    /// Whether this instruction's result should be recorded as a probe when a
    /// `@d` trace scope is active.
    ///
    /// The set is "symbol reads + ops + calls + indexes": local/capture/global
    /// loads, arithmetic, comparison chains, concatenation, every call/postfix
    /// flavor, every load/assign-and-keep indexing flavor, and in-place
    /// mutating index ops. Bare literals are skipped to keep traces readable.
    ///
    /// Tail-call variants do not produce a value in the current frame (the
    /// result is delivered to the caller's stack instead), so they are
    /// excluded.
    pub(crate) fn is_trace_interesting(&self) -> bool {
        if let Self::NamedCall { call, .. } = self {
            return call.is_trace_interesting();
        }
        use Instruction as I;
        matches!(
            self,
            I::BinaryOp(_)
                | I::CatAssign(_)
                | I::BoolCombine(_)
                | I::LoadVar(_)
                | I::LoadLocal(_)
                | I::LoadCapture(_)
                | I::UnaryOp(_)
                | I::CmpChain(_)
                | I::Cat(_)
                | I::CallBuiltinId(_, _)
                | I::CallBuiltinDiscardId(_, _)
                | I::CallUser(_, _)
                | I::CallAnon(_)
                | I::CallLocal(_, _)
                | I::Postfix(_)
                | I::Index
                | I::IndexMany(_)
                | I::IndexLoadLocal(_)
                | I::IndexLoadCapture(_)
                | I::IndexLoadVar(_)
                | I::IndexManyLoadLocal(_, _)
                | I::IndexManyLoadCapture(_, _)
                | I::IndexManyLoadVar(_, _)
                | I::IndexAssignVar(_)
                | I::IndexAssignLocal(_)
                | I::IndexAssignCapture(_)
                | I::IndexManyAssignVar(_, _)
                | I::IndexManyAssignLocal(_, _)
                | I::IndexManyAssignCapture(_, _)
                | I::IndexMutate { .. }
        )
    }
}

impl Operand {
    pub(crate) fn const_val(v: Value) -> Self {
        Self::Const(Box::new(v))
    }
}

#[derive(Copy, Clone, Debug)]
enum InstClass {
    Load,
    Store,
    Call,
    Jump,
    Stack,
    Op,
    Indexing,
    Construct,
    Try,
}

fn classify(inst: &Instruction) -> (InstClass, bool /* is_special */) {
    if let Instruction::NamedCall { call, .. } = inst {
        return classify(call);
    }
    use InstClass::*;
    use Instruction as I;
    match inst {
        // Loads
        I::LoadConst(_)
        | I::LoadOwnedConst(_)
        | I::LoadLocal(_)
        | I::LoadCapture(_)
        | I::LoadClosure(_)
        | I::LoadVar(_)
        | I::LoadCallTarget(_)
        | I::LoadVarExists(_)
        | I::LoadUnpack(_)
        => (Load, false),
        I::LoadSelf => (Load, true),

        // Stores
        I::StoreLocal(_)
        | I::StoreLocalKeep(_)
        | I::StoreCapture(_)
        | I::StoreCaptureKeep(_)
        | I::StoreVar(_)
        // | I::DeleteVar(_)
        | I::StoreVarKeep(_)
        | I::CatAssign(_)
        => (Store, false),

        // Calls
        I::CallBuiltinId(_, _)
        | I::CallBuiltinDiscardId(_, _)
        | I::CallLocal(_, _)
        | I::TailCallLocal(_, _)
        | I::Postfix(_)
        | I::TailPostfix(_)
        | I::CallAnon(_)
        | I::TailCallAnon(_)
        | I::CallUser(_, _)
        | I::TailCallUser(_, _)
        | I::Import(_) => (Call, false),

        // Jumps / branches
        I::Jump(_)
        | I::JumpIfFalse(_)
        | I::JumpIfCmpFalse(_)
        | I::NLoopEnter(_)
        | I::NLoopNext(_)
        | I::JumpIfGE(_)
        | I::JumpIfLEZLocal(_, _)
        | I::JumpIfNamedProvided(_, _, _)
        | I::BoolAndLazy(_)
        | I::BoolOrLazy(_) => (Jump, false),

        I::IndexMutate { .. } => (Store, false),

        // Stack-ish
        I::Unpack(_)
        | I::EndUnpack
        | I::Pop
        | I::Return
        | I::Debug
        | I::Pause
        | I::TraceBegin => (Stack, false),

        // Arithmetic / logic
        I::UnaryOp(_) | I::BinaryOp(_) | I::CmpChain(_) | I::BoolCombine(_) => (Op, false),

        // Indexing
        I::Index | I::IndexMany(_) | I::CheckAtomPathIndex
        | I::IndexLoadLocal(_)
        | I::IndexLoadCapture(_)
        | I::IndexLoadVar(_)
        | I::IndexManyLoadLocal(_, _)
        | I::IndexManyLoadCapture(_, _)
        | I::IndexManyLoadVar(_, _)
        | I::IndexAssignVar(_)
        | I::IndexAssignVarDrop(_)
        | I::IndexAssignLocal(_)
        | I::IndexAssignLocalDrop(_)
        | I::IndexAssignCapture(_)
        | I::IndexAssignCaptureDrop(_)
        | I::IndexManyAssignVar(_, _)
        | I::IndexManyAssignVarDrop(_, _)
        | I::IndexManyAssignLocal(_, _)
        | I::IndexManyAssignLocalDrop(_, _)
        | I::IndexManyAssignCapture(_, _)
        | I::IndexManyAssignCaptureDrop(_, _) => (Indexing, false),

        // Constructors
        I::MakeList(_) | I::MakeDict(_) | I::MakeRange { .. } | I::Cat(_) => (Construct, false),

        // Try
        I::Try(_) => (Try, false),
        I::NamedCall { .. } => unreachable!("named call was unwrapped before classification"),
        // Fallback
        // _ => (Other, false),
    }
}

fn public_instruction_class(class: InstClass) -> InstructionClass {
    match class {
        InstClass::Load => InstructionClass::Load,
        InstClass::Store => InstructionClass::Store,
        InstClass::Call => InstructionClass::Call,
        InstClass::Jump => InstructionClass::Jump,
        InstClass::Stack => InstructionClass::Stack,
        InstClass::Op => InstructionClass::Operator,
        InstClass::Indexing => InstructionClass::Indexing,
        InstClass::Construct => InstructionClass::Construct,
        InstClass::Try => InstructionClass::Try,
    }
}

pub struct InstPrettyDumper {
    lines: Vec<String>,
    show_builtin_names: bool,
    colorize: bool,
    show_pc: bool,
}

impl InstPrettyDumper {
    pub fn new(show_builtin_names: bool, colorize: bool) -> Self {
        Self {
            lines: Vec::new(),
            show_builtin_names,
            colorize,
            show_pc: false,
        }
    }

    pub fn with_pc(mut self) -> Self {
        self.show_pc = true;
        self
    }

    pub(crate) fn render(mut self, instructions: &[Instruction]) -> Vec<String> {
        self.dump_chunk(instructions, 0, None, None);
        self.lines
    }

    pub(crate) fn render_at(
        mut self,
        instructions: &[Instruction],
        pc: usize,
        local_names: Option<&[String]>,
    ) -> Option<String> {
        let instruction = instructions.get(pc)?;
        let labels = Self::build_label_map(instructions);
        self.dump_one(
            pc,
            instruction,
            0,
            &labels,
            instructions.len(),
            local_names,
            None,
        );
        self.lines.into_iter().next()
    }

    pub(crate) fn describe_at(
        instructions: &[Instruction],
        pc: usize,
        local_names: Option<&[String]>,
    ) -> Option<DebugInstruction> {
        let instruction = instructions.get(pc)?;
        let (class, is_special) = classify(instruction);
        let line = Self::new(true, false).render_at(instructions, pc, local_names)?;
        let (base, annotations) =
            line.split_once("  // ")
                .map_or((line.as_str(), Vec::new()), |(base, annotations)| {
                    (
                        base,
                        annotations
                            .split("; ")
                            .map(str::to_string)
                            .collect::<Vec<_>>(),
                    )
                });
        let opcode_end = base
            .char_indices()
            .find_map(|(index, ch)| matches!(ch, '(' | ':' | ' ').then_some(index))
            .unwrap_or(base.len());

        Some(DebugInstruction {
            pc,
            opcode: base[..opcode_end].to_string(),
            operands: base[opcode_end..].to_string(),
            annotations,
            class: public_instruction_class(class),
            is_special,
        })
    }

    fn style_opcode_with_class(&self, opcode: &str, class: InstClass, is_special: bool) -> String {
        if !self.colorize {
            return opcode.to_string();
        }
        let color = match class {
            InstClass::Load => AnsiColor::Red,
            InstClass::Store => AnsiColor::Green,
            InstClass::Call => AnsiColor::Blue,
            InstClass::Jump => AnsiColor::Yellow,
            InstClass::Stack => AnsiColor::BrightBlack,
            InstClass::Op => AnsiColor::Magenta,
            InstClass::Indexing => AnsiColor::Purple,
            InstClass::Construct => AnsiColor::BrightRed,
            InstClass::Try => AnsiColor::BrightYellow,
        };
        let mut style = TextStyle::new().fg(color);
        if is_special {
            style = style.bold().italic();
        }
        paint(opcode, style, ColorMode::Always)
    }

    fn dump_chunk(
        &mut self,
        instructions: &[Instruction],
        indent: usize,
        locals_names: Option<&[String]>,
        captures_spec: Option<&[Capture]>,
    ) {
        let label_map = Self::build_label_map(instructions);
        let chunk_len = instructions.len();
        for (pc, inst) in instructions.iter().enumerate() {
            if let Some(label) = label_map.get(&pc) {
                self.push_line(indent, None, format!("{label}:"));
            }
            self.dump_one(
                pc,
                inst,
                indent,
                &label_map,
                chunk_len,
                locals_names,
                captures_spec,
            );
        }
    }

    fn dump_one(
        &mut self,
        pc: usize,
        inst: &Instruction,
        indent: usize,
        labels: &HashMap<usize, String>,
        chunk_len: usize,
        locals_names: Option<&[String]>,
        captures_spec: Option<&[Capture]>,
    ) {
        match inst {
            Instruction::LoadConst(box_val) if matches!(&**box_val, Value::CompiledFunction(_)) => {
                if let Value::CompiledFunction(f) = &**box_val {
                    let opcode = self.style_opcode_with_class("LoadConst", InstClass::Load, false);
                    let header = format!(
                        "{opcode}(CompiledFunction): params={} locals={} names={}",
                        Self::format_params(f.params.as_deref()),
                        f.locals,
                        Self::format_names(f.dbg_local_names.as_deref()),
                    );
                    self.push_line(indent, Some(pc), header);
                    self.push_line(indent, None, "{".to_string());
                    self.dump_chunk(
                        f.instructions.as_ref(),
                        indent + 2,
                        f.dbg_local_names.as_deref(),
                        None,
                    );
                    self.push_line(indent, None, "}".to_string());
                }
            }
            Instruction::LoadClosure(payload) => {
                let opcode = self.style_opcode_with_class("LoadClosure", InstClass::Load, false);
                let header = format!(
                    "{opcode}: params={} locals={} captures={} names={}",
                    Self::format_params(payload.params.as_deref()),
                    payload.locals,
                    Self::format_captures(&payload.captures),
                    Self::format_names(Some(payload.dbg_local_names.as_ref())),
                );
                self.push_line(indent, Some(pc), header);
                self.push_line(indent, None, "{".to_string());
                self.dump_chunk(
                    payload.instructions.as_ref(),
                    indent + 2,
                    Some(payload.dbg_local_names.as_ref()),
                    Some(payload.captures.as_ref()),
                );
                self.push_line(indent, None, "}".to_string());
            }
            _ => {
                let base = self.highlight_inst(inst);
                let comments =
                    self.comment_for(inst, labels, chunk_len, locals_names, captures_spec);
                let line = self.attach_comment(base, comments);
                self.push_line(indent, Some(pc), line);
            }
        }
    }

    fn comment_for(
        &self,
        inst: &Instruction,
        labels: &HashMap<usize, String>,
        chunk_len: usize,
        locals_names: Option<&[String]>,
        captures_spec: Option<&[Capture]>,
    ) -> Vec<String> {
        let named_meta = match inst {
            Instruction::NamedCall { meta, .. } => Some(meta),
            _ => None,
        };
        let inst = inst.call_instruction();
        let mut parts = Vec::new();

        // Jump label
        if let Some(target) = Self::jump_target(inst) {
            if let Some(label) = labels.get(&target) {
                parts.push(format!("-> {label}"));
            } else if target == chunk_len {
                parts.push("-> <end>".to_string());
            } else {
                parts.push(format!("-> {target}"));
            }
        }

        // Builtin name
        if self.show_builtin_names
            && let Instruction::CallBuiltinId(id, _) | Instruction::CallBuiltinDiscardId(id, _) =
                inst
        {
            let idx = usize::from(*id);
            if let Some(name) = Builtins::NAMES.get(idx) {
                parts.push((*name).to_string());
            }
        }

        if let Some(meta) = named_meta {
            parts.push(format!(
                "named(pos={}, count={})",
                meta.pos_count,
                meta.named.len()
            ));
        }

        // Local slot names
        match *inst {
            Instruction::LoadLocal(slot)
            | Instruction::StoreLocal(slot)
            | Instruction::StoreLocalKeep(slot)
            | Instruction::IndexLoadLocal(slot)
            | Instruction::IndexManyLoadLocal(slot, _)
            | Instruction::IndexAssignLocal(slot)
            | Instruction::IndexManyAssignLocal(slot, _)
            | Instruction::IndexManyAssignLocalDrop(slot, _)
            | Instruction::IndexMutate {
                target: StoreTarget::Local(slot),
                ..
            }
            | Instruction::JumpIfLEZLocal(slot, _)
            | Instruction::JumpIfNamedProvided(slot, _, _)
            | Instruction::TailCallLocal(slot, _)
            | Instruction::CallLocal(slot, _) => {
                if let Some(name) = Self::local_name(slot, locals_names) {
                    // parts.push(format!("{slot}: {name}"));
                    parts.push(name.into());
                }
            }
            _ => {}
        }
        if let Instruction::LoadCallTarget(Operand::Local(slot)) = inst
            && let Some(name) = Self::local_name(*slot, locals_names)
        {
            parts.push(name.into());
        }
        if let Instruction::CatAssign(data) = inst
            && let StoreTarget::Local(slot) = &data.target
            && let Some(name) = Self::local_name(*slot, locals_names)
        {
            parts.push(name.into());
        }

        // capture indices
        if let Instruction::LoadCapture(i)
        | Instruction::StoreCapture(i)
        | Instruction::StoreCaptureKeep(i)
        | Instruction::IndexLoadCapture(i)
        | Instruction::IndexManyLoadCapture(i, _)
        | Instruction::IndexManyAssignCapture(i, _)
        | Instruction::IndexManyAssignCaptureDrop(i, _)
        | Instruction::IndexMutate {
            target: StoreTarget::Capture(i),
            ..
        } = *inst
        {
            let desc =
                Self::capture_desc(i, captures_spec).unwrap_or_else(|| "capture".to_string());
            parts.push(desc);
        }
        if let Instruction::LoadCallTarget(Operand::Capture(slot)) = inst {
            let desc =
                Self::capture_desc(*slot, captures_spec).unwrap_or_else(|| "capture".to_string());
            parts.push(desc);
        }
        if let Instruction::CatAssign(data) = inst
            && let StoreTarget::Capture(slot) = &data.target
        {
            let desc =
                Self::capture_desc(*slot, captures_spec).unwrap_or_else(|| "capture".to_string());
            parts.push(desc);
        }

        parts
    }

    fn local_name(slot: u16, names: Option<&[String]>) -> Option<&str> {
        let idx = usize::from(slot);
        let ns = names?;
        if idx < ns.len() && !ns[idx].is_empty() {
            Some(ns[idx].as_str())
        } else {
            None
        }
    }

    fn capture_desc(i: u16, caps: Option<&[Capture]>) -> Option<String> {
        let caps = caps?;
        let j = usize::from(i);
        if j >= caps.len() {
            return None;
        }
        Some(Self::format_one_capture(&caps[j]))
    }

    fn format_captures(captures: &[Capture]) -> String {
        if captures.is_empty() {
            "[]".to_string()
        } else {
            let parts: Vec<String> = captures.iter().map(Self::format_one_capture).collect();
            format!("[{}]", parts.join(", "))
        }
    }

    fn format_one_capture(capture: &Capture) -> String {
        match capture {
            Capture::Local(slot) => format!("Local({slot})"),
            Capture::LocalShared(slot) => format!("LocalShared({slot})"),
            Capture::FromCapture(slot) => format!("FromCapture({slot})"),
            Capture::Global(name, _) => format!("Global({name})"),
        }
    }

    fn attach_comment(&self, base: String, comments: Vec<String>) -> String {
        if comments.is_empty() {
            base
        } else if self.colorize {
            format!(
                "{base}  {}",
                paint(
                    &format!("// {}", comments.join("; ")),
                    TextStyle::new().fg(AnsiColor::BrightBlack),
                    ColorMode::Always,
                )
            )
        } else {
            format!("{base}  // {}", comments.join("; "))
        }
    }

    pub(crate) fn highlight_inst(&self, inst: &Instruction) -> String {
        let inst = inst.call_instruction();
        let s = format!("{inst:?}");
        let s: String = s.chars().collect();
        // Split off the opcode token to style only it
        let mut split_pos = s.len();
        for (idx, ch) in s.char_indices() {
            if ch == '(' || ch == ' ' {
                split_pos = idx;
                break;
            }
        }
        let (head, tail) = s.split_at(split_pos);
        let (class, special) = classify(inst);
        let head = self.style_opcode_with_class(head, class, special);
        format!("{head}{tail}")
    }

    fn push_line(&mut self, indent: usize, pc: Option<usize>, text: String) {
        let mut line = String::with_capacity(8 + indent + text.len());
        if self.show_pc {
            if let Some(pc) = pc {
                line.push_str(&format!("{:>4} ", pc));
            } else {
                line.push_str("     ");
            }
        }
        line.push_str(&" ".repeat(indent));
        line.push_str(&text);
        self.lines.push(line);
    }

    fn format_params(params: Option<&[String]>) -> String {
        match params {
            Some(list) if !list.is_empty() => format!("[{}]", list.join(", ")),
            Some(_) => "[]".to_string(),
            None => "None".to_string(),
        }
    }

    fn format_names(names: Option<&[String]>) -> String {
        match names {
            Some(list) if !list.is_empty() => format!("[{}]", list.join(", ")),
            Some(_) => "[]".to_string(),
            None => "None".to_string(),
        }
    }

    fn build_label_map(instructions: &[Instruction]) -> HashMap<usize, String> {
        let mut targets = BTreeSet::new();
        for inst in instructions {
            if let Some(target) = Self::jump_target(inst) {
                targets.insert(target);
            }
        }
        targets
            .into_iter()
            .enumerate()
            .map(|(idx, target)| (target, format!("L{idx}")))
            .collect()
    }

    fn jump_target(inst: &Instruction) -> Option<usize> {
        match inst {
            Instruction::NamedCall { call, .. } => Self::jump_target(call),
            Instruction::Jump(target)
            | Instruction::JumpIfFalse(target)
            | Instruction::JumpIfGE(target)
            | Instruction::JumpIfLEZLocal(_, target)
            | Instruction::JumpIfNamedProvided(_, _, target)
            | Instruction::BoolAndLazy(target)
            | Instruction::BoolOrLazy(target) => Some(*target),
            Instruction::JumpIfCmpFalse(data) => Some(data.target),
            Instruction::NLoopEnter(data) => Some(data.target),
            Instruction::NLoopNext(data) => Some(data.target),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn interpreter_enums_keep_their_expected_layout() {
        assert_eq!(std::mem::size_of::<Instruction>(), 32);
        assert_eq!(std::mem::size_of::<Operand>(), 24);
        assert_eq!(std::mem::size_of::<StoreTarget>(), 24);
        assert_eq!(std::mem::size_of::<Capture>(), 48);
        assert_eq!(std::mem::size_of::<MutationOp>(), 1);
    }

    #[test]
    fn highlight_inst_uses_explicit_opcode_color() {
        let dumper = InstPrettyDumper::new(true, true);
        let rendered = dumper.highlight_inst(&Instruction::load_const(Value::Int(1)));

        assert_eq!(rendered, "\x1b[31mLoadConst\x1b[0m(Int(1))");
    }

    #[test]
    fn highlight_inst_styles_special_opcodes_bold_italic() {
        let dumper = InstPrettyDumper::new(true, true);
        let rendered = dumper.highlight_inst(&Instruction::LoadSelf);

        assert_eq!(rendered, "\x1b[1;3;31mLoadSelf\x1b[0m");
    }

    #[test]
    fn highlight_inst_can_render_plain_opcode() {
        let dumper = InstPrettyDumper::new(true, false);
        let rendered = dumper.highlight_inst(&Instruction::load_const(Value::Int(1)));

        assert_eq!(rendered, "LoadConst(Int(1))");
    }

    #[test]
    fn describe_at_separates_instruction_fields() {
        let instructions = [Instruction::LoadLocal(0)];
        let local_names = ["value".to_string()];

        let instruction = InstPrettyDumper::describe_at(&instructions, 0, Some(&local_names))
            .expect("instruction should exist");

        assert_eq!(instruction.pc, 0);
        assert_eq!(instruction.opcode, "LoadLocal");
        assert_eq!(instruction.operands, "(0)");
        assert_eq!(instruction.annotations, ["value"]);
        assert_eq!(instruction.class, InstructionClass::Load);
        assert!(!instruction.is_special);
    }

    #[test]
    fn render_at_uses_pretty_printer_context() {
        let instructions = [Instruction::LoadLocal(0), Instruction::Return];
        let names = ["answer".to_string()];
        let rendered = InstPrettyDumper::new(true, false)
            .render_at(&instructions, 0, Some(&names))
            .expect("instruction should render");

        assert_eq!(rendered, "LoadLocal(0)  // answer");
    }

    #[test]
    fn named_call_renders_and_classifies_as_its_call() {
        let instructions = [
            Instruction::CallUser(Arc::from("f"), 2).with_named_args(Some(Arc::new(
                NamedArgMeta {
                    pos_count: 1,
                    named: vec![(1, Arc::from("limit"))].into_boxed_slice(),
                },
            ))),
        ];

        let rendered = InstPrettyDumper::new(true, false)
            .render_at(&instructions, 0, None)
            .expect("instruction should render");
        let described = InstPrettyDumper::describe_at(&instructions, 0, None)
            .expect("instruction should be described");

        assert_eq!(rendered, "CallUser(\"f\", 2)  // named(pos=1, count=1)");
        assert_eq!(described.opcode, "CallUser");
        assert_eq!(described.class, InstructionClass::Call);
    }
}
