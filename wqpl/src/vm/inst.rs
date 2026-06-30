use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use crate::astnode::{BinaryOperator, UnaryOperator};
use crate::builtins::Builtins;
use crate::style::{AnsiColor, ColorMode, TextStyle, paint};
use crate::value::Value;
use crate::wqdb::data::ChunkId;

#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    Local(u16),
    LocalShared(u16),
    Outer(u16), // FromCapture
    Global(String, Option<(usize, usize)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClosurePayload {
    pub(crate) params: Option<Arc<[String]>>,
    pub(crate) named_params: Option<Arc<[Arc<str>]>>,
    pub(crate) locals: u16,
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

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Instruction {
    LoadConst(Box<Value>),
    /// Load a constant moved into the VM-owned one-shot constant pool.
    LoadOwnedConst(usize),
    /// Load a closure capturing current local slots
    LoadClosure(Box<ClosurePayload>),
    /// Load a global variable or builtin by name
    LoadVar(Arc<str>),
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
    /// Store into a captured variable slot
    // StoreCapture(u16),
    /// Store into a captured variable slot and keep value on stack
    StoreCaptureKeep(u16),
    BinaryOp(Box<BinaryOpData>),
    /// Evaluate a chain of comparison operators; expects N+1 operands
    CmpChain(Box<[BinaryOperator]>),
    /// Concatenate N items from the stack into a single value
    Cat(usize),
    UnaryOp(Box<UnaryOpData>),
    /// Short-circuit boolean and lazy check (A[...])
    BoolAndLazy(usize),
    /// Short-circuit boolean or lazy check (O[...])
    BoolOrLazy(usize),
    // CallBuiltin(String, usize),
    /// Builtin call resolved to an ID at compile time for faster dispatch
    CallBuiltinId(u16, u16),
    /// Builtin call whose result is only needed as unit because the expression
    /// result will be discarded.
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
    /// Call or index a local variable, avoiding cloning if indexing
    PostfixLocal(u16, usize),
    TailPostfixLocal(u16, usize),
    /// Look up a constant-tag method on a local dict, then call or index it.
    PostfixMethodLocal(u16, Arc<str>, usize),
    TailPostfixMethodLocal(u16, Arc<str>, usize),
    /// Look up a constant-tag method on a local dict, then call it.
    CallMethodLocal(u16, Arc<str>, usize),
    TailCallMethodLocal(u16, Arc<str>, usize),
    /// Call or index a captured variable, avoiding cloning if indexing
    PostfixCapture(u16, usize),
    TailPostfixCapture(u16, usize),
    /// Look up a constant-tag method on a captured dict, then call or index it.
    PostfixMethodCapture(u16, Arc<str>, usize),
    TailPostfixMethodCapture(u16, Arc<str>, usize),
    /// Look up a constant-tag method on a captured dict, then call it.
    CallMethodCapture(u16, Arc<str>, usize),
    TailCallMethodCapture(u16, Arc<str>, usize),
    /// Call or index a global variable, avoiding cloning if indexing
    PostfixVar(Arc<str>, usize),
    TailPostfixVar(Arc<str>, usize),
    /// Look up a constant-tag method on a global dict, then call or index it.
    PostfixMethodVar(Arc<str>, Arc<str>, usize),
    TailPostfixMethodVar(Arc<str>, Arc<str>, usize),
    /// Look up a constant-tag method on a global dict, then call it.
    CallMethodVar(Arc<str>, Arc<str>, usize),
    TailCallMethodVar(Arc<str>, Arc<str>, usize),
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
    /// Evaluate a comparison and jump if its boolean result is false.
    JumpIfCmpFalse(Box<CmpBranchData>),
    /// Jump if left >= right (pops two operands)
    JumpIfGE(usize),
    /// Jump if local slot value <= 0
    JumpIfLEZLocal(u16, usize),
    /// Unified mutating index instruction: pop, remove, or insert.
    IndexMutate {
        target: StoreTarget,
        op: MutationOp,
    },
    Pop,
    Return,
    Assert,
    /// Open a value-provenance trace scope.  The closing `Debug` instruction
    /// renders the recorded probes as a tree and prints the final value.
    TraceBegin,
    Debug,
    Pause,
    Try(usize),
    /// Store named-argument metadata into the VM.  The next call
    /// instruction consumes it (and clears it after the call).
    PrepareNamedArgs(Box<NamedArgMeta>),
    /// Read the hidden `--named-mask` local slot, test bit N, push bool.
    /// Used in function prologues for named parameters with defaults.
    LoadNamedArgsProvided(u8),
}

impl Instruction {
    pub(crate) fn load_const(v: Value) -> Self {
        Self::LoadConst(Box::new(v))
    }

    pub(crate) fn load_closure(payload: ClosurePayload) -> Self {
        Self::LoadClosure(Box::new(payload))
    }

    pub(crate) fn binary_op(op: BinaryOperator, left: Operand, right: Operand) -> Self {
        Self::BinaryOp(Box::new(BinaryOpData { op, left, right }))
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
        use Instruction as I;
        matches!(
            self,
            I::BinaryOp(_)
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
                | I::PostfixLocal(_, _)
                | I::PostfixMethodLocal(_, _, _)
                | I::CallMethodLocal(_, _, _)
                | I::PostfixCapture(_, _)
                | I::PostfixMethodCapture(_, _, _)
                | I::CallMethodCapture(_, _, _)
                | I::PostfixVar(_, _)
                | I::PostfixMethodVar(_, _, _)
                | I::CallMethodVar(_, _, _)
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
        | I::LoadVarExists(_)
        => (Load, false),
        I::LoadSelf => (Load, true),

        // Stores
        I::StoreLocal(_)
        | I::StoreLocalKeep(_)
        // | I::StoreCapture(_)
        | I::StoreCaptureKeep(_)
        | I::StoreVar(_)
        // | I::DeleteVar(_)
        | I::StoreVarKeep(_)
        => (Store, false),

        // Calls
        I::CallBuiltinId(_, _)
        | I::CallBuiltinDiscardId(_, _)
        | I::CallLocal(_, _)
        | I::TailCallLocal(_, _)
        | I::Postfix(_)
        | I::TailPostfix(_)
        | I::PostfixLocal(_, _)
        | I::TailPostfixLocal(_, _)
        | I::PostfixMethodLocal(_, _, _)
        | I::TailPostfixMethodLocal(_, _, _)
        | I::CallMethodLocal(_, _, _)
        | I::TailCallMethodLocal(_, _, _)
        | I::PostfixCapture(_, _)
        | I::TailPostfixCapture(_, _)
        | I::PostfixMethodCapture(_, _, _)
        | I::TailPostfixMethodCapture(_, _, _)
        | I::CallMethodCapture(_, _, _)
        | I::TailCallMethodCapture(_, _, _)
        | I::PostfixVar(_, _)
        | I::TailPostfixVar(_, _)
        | I::PostfixMethodVar(_, _, _)
        | I::TailPostfixMethodVar(_, _, _)
        | I::CallMethodVar(_, _, _)
        | I::TailCallMethodVar(_, _, _)
        | I::CallAnon(_)
        | I::TailCallAnon(_)
        | I::CallUser(_, _)
        | I::TailCallUser(_, _) => (Call, false),

        // Jumps / branches
        I::Jump(_)
        | I::JumpIfFalse(_)
        | I::JumpIfCmpFalse(_)
        | I::JumpIfGE(_)
        | I::JumpIfLEZLocal(_, _)
        | I::BoolAndLazy(_)
        | I::BoolOrLazy(_) => (Jump, false),

        I::IndexMutate { .. } => (Store, false),

        I::PrepareNamedArgs(_) => (Stack, false),
        I::LoadNamedArgsProvided(_) => (Load, false),

        // Stack-ish
        I::Pop | I::Return | I::Debug | I::Pause | I::Assert | I::TraceBegin => (Stack, false),

        // Arithmetic / logic
        I::UnaryOp(_) | I::BinaryOp(_) | I::CmpChain(_) => (Op, false),

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
        // Fallback
        // _ => (Other, false),
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

        // Local slot names
        match *inst {
            Instruction::LoadLocal(slot)
            | Instruction::StoreLocal(slot)
            | Instruction::StoreLocalKeep(slot)
            | Instruction::IndexLoadLocal(slot)
            | Instruction::IndexManyLoadLocal(slot, _)
            | Instruction::PostfixLocal(slot, _)
            | Instruction::TailPostfixLocal(slot, _)
            | Instruction::PostfixMethodLocal(slot, _, _)
            | Instruction::TailPostfixMethodLocal(slot, _, _)
            | Instruction::CallMethodLocal(slot, _, _)
            | Instruction::TailCallMethodLocal(slot, _, _)
            | Instruction::IndexAssignLocal(slot)
            | Instruction::IndexManyAssignLocal(slot, _)
            | Instruction::IndexManyAssignLocalDrop(slot, _)
            | Instruction::IndexMutate {
                target: StoreTarget::Local(slot),
                ..
            }
            | Instruction::JumpIfLEZLocal(slot, _)
            | Instruction::TailCallLocal(slot, _)
            | Instruction::CallLocal(slot, _) => {
                if let Some(name) = Self::local_name(slot, locals_names) {
                    // parts.push(format!("{slot}: {name}"));
                    parts.push(name.into());
                }
            }
            _ => {}
        }

        // capture indices
        if let Instruction::LoadCapture(i)
        // | Instruction::StoreCapture(i)
        | Instruction::StoreCaptureKeep(i)
        | Instruction::IndexLoadCapture(i)
        | Instruction::IndexManyLoadCapture(i, _)
        | Instruction::PostfixCapture(i, _)
        | Instruction::TailPostfixCapture(i, _)
        | Instruction::IndexManyAssignCapture(i, _)
        | Instruction::IndexManyAssignCaptureDrop(i, _)
        | Instruction::IndexMutate { target: StoreTarget::Capture(i), .. } = *inst
        {
            let desc =
                Self::capture_desc(i, captures_spec).unwrap_or_else(|| "capture".to_string());
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
            Capture::Outer(slot) => format!("FromCapture({slot})"),
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
            Instruction::Jump(target)
            | Instruction::JumpIfFalse(target)
            | Instruction::JumpIfGE(target)
            | Instruction::JumpIfLEZLocal(_, target)
            | Instruction::BoolAndLazy(target)
            | Instruction::BoolOrLazy(target) => Some(*target),
            Instruction::JumpIfCmpFalse(data) => Some(data.target),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
