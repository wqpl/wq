use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use crate::{
    astnode::{BinaryOperator, UnaryOperator},
    builtins::Builtins,
    colored::Colorize,
    value::Value,
};

#[derive(Debug, Clone, PartialEq)]
pub enum Capture {
    Local(u16),
    // LocalShared(u16),
    #[allow(clippy::enum_variant_names)]
    FromCapture(u16),
    Global(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Instruction {
    LoadConst(Value),
    /// Load a closure capturing current local slots
    LoadClosure {
        params: Option<Arc<[String]>>,
        locals: u16,
        captures: Vec<Capture>,
        instructions: Arc<[Instruction]>,
        /// Statement spans for the function body (byte start,end in source)
        dbg_stmt_spans: Arc<[(usize, usize)]>,
        /// Local variable names by slot index (for wqdb)
        dbg_local_names: Arc<[String]>,
    },
    /// Load a global variable or builtin by name
    LoadVar(String),
    /// Load a captured value by index from the current closure frame
    LoadCapture(u16),
    /// Load the current function/closure being executed
    LoadSelf,
    /// Store into a global variable by name
    StoreVar(String),
    /// Store into a global variable and keep value on stack
    StoreVarKeep(String),
    /// Load a local variable from an index-based slot
    LoadLocal(u16),
    /// Store into a local variable slot
    StoreLocal(u16),
    /// Store into a local slot and keep value on stack
    StoreLocalKeep(u16),
    BinaryOp(BinaryOperator),
    /// Evaluate a chain of comparison operators; expects N+1 operands
    CmpChain(Vec<BinaryOperator>),
    UnaryOp(UnaryOperator),
    /// Compute floor(left / right)
    FloorDiv,
    // CallBuiltin(String, usize),
    /// Builtin call resolved to an ID at compile time for faster dispatch
    CallBuiltinId(u16, u16),
    /// Call a function stored in a local slot
    CallLocal(u16, usize),
    CallUser(String, usize),
    CallAnon(usize),
    /// Call if the object is a function, otherwise index
    CallOrIndex(usize),
    MakeList(usize),
    MakeDict(usize),
    MakeRange {
        inclusive: bool,
        has_step: bool,
    },
    Index,
    IndexAssign,
    IndexAssignLocal(u16),
    /// Like IndexAssign, but does not push the assigned value
    IndexAssignDrop,
    /// Like IndexAssignLocal, but does not push the assigned value
    IndexAssignLocalDrop(u16),
    Jump(usize),
    JumpIfFalse(usize),
    /// Jump if left >= right (pops two operands)
    JumpIfGE(usize),
    /// Jump if local slot value <= 0
    JumpIfLEZLocal(u16, usize),
    /// Increment a local slot by 1 (no stack effect)
    Inc1Local(u16),
    /// Increment a local slot by 1 and keep the result on stack
    Inc1LocalKeep(u16),
    /// Increment a global variable by 1 (no stack effect)
    Inc1Var(String),
    /// Set variable `dst` to (`src` + 1) (no stack effect)
    Inc1VarFromVar {
        src: String,
        dst: String,
    },
    /// Increment a global variable by 1 and keep the result on stack
    Inc1VarKeep(String),
    /// Set local `dst` to (`src` + 1) (no stack effect)
    Inc1LocalFromLocal {
        src: u16,
        dst: u16,
    },
    Pop,
    // Assert,
    Return,
    Try(usize),
}

#[derive(Copy, Clone, Debug)]
enum InstClass {
    Load,
    Store,
    Call,
    Jump,
    Stack,     // Pop, Return, etc.
    Op,        // UnaryOp / BinaryOp
    Indexing,  // Index / IndexAssign*
    Construct, // MakeList / MakeDict / MakeRange if you have it
    Try,       // Try(...)
               // Other,
}

fn classify(inst: &Instruction) -> (InstClass, bool /*is_special*/) {
    use InstClass::*;
    use Instruction as I;
    match inst {
        // Loads
        I::LoadConst(_)
        | I::LoadLocal(_)
        | I::LoadCapture(_)
        | I::LoadClosure { .. }
        | I::LoadVar(_) => (Load, false),
        I::LoadSelf => (Load, true), // special: bold this one

        // Stores
        I::StoreLocal(_)
        | I::StoreLocalKeep(_)
        | I::StoreVar(_)
        | I::StoreVarKeep(_) => (Store, false),

        // Calls
        I::CallBuiltinId(_, _)
        | I::CallLocal(_, _)
        | I::CallOrIndex(_)
        | I::CallAnon(_)
        | I::CallUser(_, _) => (Call, false),

        // Jumps / branches
        I::Jump(_)
        | I::JumpIfFalse(_)
        | I::JumpIfGE(_)
        | I::JumpIfLEZLocal(_, _) => (Jump, false),

        // Increments (mutations)
        I::Inc1Local(_)
        | I::Inc1LocalKeep(_)
        | I::Inc1Var(_)
        | I::Inc1VarKeep(_)
        | I::Inc1VarFromVar { .. }
        | I::Inc1LocalFromLocal { .. } => (Store, false),

        // Stack-ish
        I::Pop | I::Return => (Stack, false),

        // Arithmetic / logic
        I::UnaryOp(_) | I::BinaryOp(_) | I::CmpChain(_) | I::FloorDiv => (Op, false),

        // Indexing
        I::Index | I::IndexAssign | I::IndexAssignLocal(_) | I::IndexAssignDrop | I::IndexAssignLocalDrop(_) => {
            (Indexing, false)
        }

        // Constructors
        I::MakeList(_)
        | I::MakeDict(_)
        | I::MakeRange { .. }
        // If you have MakeRange{..} etc. add here
        => (Construct, false),

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
}

impl InstPrettyDumper {
    pub fn new(show_builtin_names: bool, colorize: bool) -> Self {
        Self {
            lines: Vec::new(),
            show_builtin_names,
            colorize,
        }
    }

    pub fn render(mut self, instructions: &[Instruction]) -> Vec<String> {
        self.dump_chunk(instructions, 0, None, None);
        self.lines
    }

    fn style_opcode_with_class(&self, opcode: &str, class: InstClass, is_special: bool) -> String {
        if !self.colorize {
            return opcode.to_string();
        }
        let styled = match class {
            InstClass::Load => opcode.red(),
            InstClass::Store => opcode.green(),
            InstClass::Call => opcode.blue(),
            InstClass::Jump => opcode.yellow(),
            InstClass::Stack => opcode.bright_black(),
            InstClass::Op => opcode.magenta(),
            InstClass::Indexing => opcode.purple(),
            InstClass::Construct => opcode.bright_red(),
            InstClass::Try => opcode.bright_yellow(),
            // InstClass::Other => opcode.normal(),
        };
        if is_special {
            styled.bold().italic().to_string()
        } else {
            styled.to_string()
        }
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
                self.push_line(indent, format!("{label}:"));
            }
            self.dump_one(
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
        inst: &Instruction,
        indent: usize,
        labels: &HashMap<usize, String>,
        chunk_len: usize,
        locals_names: Option<&[String]>,
        captures_spec: Option<&[Capture]>,
    ) {
        match inst {
            Instruction::LoadConst(Value::CompiledFunction {
                params,
                locals,
                instructions,
                dbg_local_names,
                ..
            }) => {
                let opcode = self.style_opcode_with_class("LoadConst", InstClass::Load, false);
                let header = format!(
                    "{opcode}(CompiledFunction): params={} locals={} names={}",
                    Self::format_params(params.as_deref()),
                    locals,
                    Self::format_names(dbg_local_names.as_deref()),
                );
                self.push_line(indent, header);
                self.push_line(indent, "{".to_string());
                self.dump_chunk(
                    instructions.as_ref(),
                    indent + 2,
                    dbg_local_names.as_deref(),
                    None,
                );
                self.push_line(indent, "}".to_string());
            }
            Instruction::LoadClosure {
                params,
                locals,
                captures,
                instructions,
                dbg_local_names,
                ..
            } => {
                let opcode = self.style_opcode_with_class("LoadClosure", InstClass::Load, false);
                let header = format!(
                    "{opcode}: params={} locals={} captures={} names={}",
                    Self::format_params(params.as_deref()),
                    locals,
                    Self::format_captures(captures),
                    Self::format_names(Some(dbg_local_names.as_ref())),
                );
                self.push_line(indent, header);
                self.push_line(indent, "{".to_string());
                self.dump_chunk(
                    instructions.as_ref(),
                    indent + 2,
                    Some(dbg_local_names.as_ref()),
                    Some(captures.as_ref()),
                );
                self.push_line(indent, "}".to_string());
            }
            _ => {
                let base = self.highlight_inst(inst);
                let comments =
                    self.comment_for(inst, labels, chunk_len, locals_names, captures_spec);
                let line = self.attach_comment(base, comments);
                self.push_line(indent, line);
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
            && let Instruction::CallBuiltinId(id, _) = inst
        {
            let idx = *id as usize;
            if let Some(name) = Builtins::NAMES.get(idx) {
                parts.push((*name).to_string());
            }
        }

        // Local slot names
        match *inst {
            Instruction::Inc1LocalFromLocal { src, dst } => {
                if let Some(name) = Self::local_name(src, locals_names) {
                    parts.push(name.into());
                }
                if let Some(name) = Self::local_name(dst, locals_names) {
                    parts.push(name.into());
                }
            }
            Instruction::LoadLocal(slot)
            | Instruction::StoreLocal(slot)
            | Instruction::StoreLocalKeep(slot)
            | Instruction::IndexAssignLocal(slot)
            | Instruction::JumpIfLEZLocal(slot, _)
            | Instruction::Inc1Local(slot)
            | Instruction::Inc1LocalKeep(slot)
            | Instruction::CallLocal(slot, _) => {
                if let Some(name) = Self::local_name(slot, locals_names) {
                    // parts.push(format!("{slot}: {name}"));
                    parts.push(name.into());
                }
            }
            _ => {}
        }

        // capture indices
        if let Instruction::LoadCapture(i) = *inst {
            let desc =
                Self::capture_desc(i, captures_spec).unwrap_or_else(|| "capture".to_string());
            parts.push(desc);
        }

        parts
    }

    fn local_name(slot: u16, names: Option<&[String]>) -> Option<&str> {
        let idx = slot as usize;
        let ns = names?;
        if idx < ns.len() && !ns[idx].is_empty() {
            Some(ns[idx].as_str())
        } else {
            None
        }
    }

    fn capture_desc(i: u16, caps: Option<&[Capture]>) -> Option<String> {
        let caps = caps?;
        let j = i as usize;
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
            Capture::FromCapture(slot) => format!("FromCapture({slot})"),
            Capture::Global(name) => format!("Global({name})"),
        }
    }

    fn attach_comment(&self, base: String, comments: Vec<String>) -> String {
        if comments.is_empty() {
            base
        } else if self.colorize {
            format!(
                "{base}  {}",
                format!("// {}", comments.join("; ")).bright_black()
            )
        } else {
            format!("{base}  // {}", comments.join("; "))
        }
    }

    // fn highlight_opcode(&self, text: &str) -> String {
    //     if !self.colorize {
    //         return text.to_string();
    //     }
    //     let mut split_pos = text.len();
    //     for (idx, ch) in text.char_indices() {
    //         if ch == '(' || ch == ' ' {
    //             split_pos = idx;
    //             break;
    //         }
    //     }
    //     let (head, tail) = text.split_at(split_pos);
    //     if head.is_empty() {
    //         text.to_string()
    //     } else {
    //         format!("{}{}", head.green(), tail)
    //     }
    // }

    fn highlight_inst(&self, inst: &Instruction) -> String {
        let s = format!("{inst:?}");
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

    fn push_line(&mut self, indent: usize, text: String) {
        let mut line = String::with_capacity(indent + text.len());
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
            | Instruction::JumpIfLEZLocal(_, target) => Some(*target),
            _ => None,
        }
    }
}
