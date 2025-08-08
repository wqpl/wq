use super::compiler::Compiler;
use super::fastpath::{FastPath, TryRunOutcome};
use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};
use indexmap::IndexMap;
use std::collections::HashMap;

#[derive(Clone)]
struct InlineCache {
    version: u64,
    value: Option<Value>,
}

impl Default for InlineCache {
    fn default() -> Self {
        InlineCache {
            version: 0,
            value: None,
        }
    }
}

pub struct Vm {
    pub instructions: Vec<Instruction>,
    pc: usize,
    stack: Vec<Value>,
    /// Global variables
    globals: HashMap<String, Value>,
    builtins: Builtins,
    /// Stack of local slot frames
    locals: Vec<Vec<Value>>,
    /// Inline caches for global lookups and call sites
    inline_cache: Vec<InlineCache>,
    /// Version number bumped whenever globals change
    global_version: u64,
}

impl Vm {
    pub fn new(instructions: Vec<Instruction>) -> Self {
        let len = instructions.len();
        Vm {
            instructions,
            pc: 0,
            stack: Vec::new(),
            globals: HashMap::new(),
            builtins: Builtins::new(),
            locals: Vec::new(),
            inline_cache: vec![InlineCache::default(); len],
            global_version: 0,
        }
    }

    /// Replace instructions and reset execution state.
    pub fn reset(&mut self, instructions: Vec<Instruction>) {
        self.instructions = instructions;
        self.pc = 0;
        self.stack.clear();
        self.locals.clear();
        self.inline_cache = vec![InlineCache::default(); self.instructions.len()];
    }

    /// Access the global environment.
    pub fn global_env(&self) -> &HashMap<String, Value> {
        &self.globals
    }

    /// Mutable access to the global environment.
    pub fn global_env_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.globals
    }

    fn lookup_global(&self, name: &str) -> Option<Value> {
        self.globals.get(name).cloned()
    }

    fn assign_global(&mut self, name: String, value: Value) {
        self.globals.insert(name, value);
        self.global_version += 1;
    }

    fn call_function(
        &mut self,
        instructions: Vec<Instruction>,
        params: Option<Vec<String>>,
        local_count: u16,
        args: Vec<Value>,
    ) -> WqResult<Value> {
        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        let saved_pc = self.pc;
        let mut saved_stack = Vec::new();
        std::mem::swap(&mut self.stack, &mut saved_stack);
        let saved_cache = std::mem::replace(
            &mut self.inline_cache,
            vec![InlineCache::default(); self.instructions.len()],
        );
        self.pc = 0;

        let mut frame = vec![Value::Null; local_count as usize];
        if let Some(p) = params {
            if args.len() != p.len() {
                return Err(WqError::ArityError(format!(
                    "Function expects {} arguments, got {}",
                    p.len(),
                    args.len()
                )));
            }
            for (i, arg) in args.into_iter().enumerate() {
                frame[i] = arg;
            }
        } else {
            if args.len() > 3 {
                return Err(WqError::ArityError(
                    "Implicit function expects up to 3 arguments".to_string(),
                ));
            }
            for (i, arg) in args.into_iter().enumerate() {
                frame[i] = arg;
            }
        }
        self.locals.push(frame);
        let res = self.execute();
        self.locals.pop();
        let result = res?;
        std::mem::swap(&mut self.stack, &mut saved_stack);
        self.instructions = saved_instructions;
        self.pc = saved_pc;
        self.inline_cache = saved_cache;
        Ok(result)
    }

    pub fn run(&mut self) -> WqResult<Value> {
        match FastPath::try_run(&self.instructions, &self.builtins, None) {
            TryRunOutcome::Ok(v) => return Ok(v),
            TryRunOutcome::VmError(e) => return Err(e),
            TryRunOutcome::Bail(_) => {}
        }
        self.execute()
    }

    /// Execute instructions without attempting fast-path optimization.
    pub fn run_no_fastpath(&mut self) -> WqResult<Value> {
        self.execute()
    }

    fn execute(&mut self) -> WqResult<Value> {
        enum OpView {
            LoadConst(Value),
            LoadVar(String),
            StoreVar(String),
            LoadLocal(u16),
            StoreLocal(u16),
            BinaryOp(BinaryOperator),
            UnaryOp(UnaryOperator),
            CallBuiltin { name: String, argc: usize },
            CallBuiltinId { id: u8, argc: usize },
            CallLocal { slot: u16, argc: usize },
            CallUser { name: String, argc: usize },
            CallAnon(usize),
            MakeList(usize),
            MakeDict(usize),
            Index,
            IndexAssign,
            IndexAssignLocal(u16),
            Jump(usize),
            JumpIfFalse(usize),
            Pop,
            Assert,
            Return,
        }

        while self.pc < self.instructions.len() {
            let idx = self.pc;
            // Phase A: decode the instruction into an owned view
            let op = {
                // SAFETY: pc bounds-checked above
                let instr = unsafe { self.instructions.get_unchecked(idx) };
                match instr {
                    Instruction::LoadConst(v) => OpView::LoadConst(v.clone()),
                    Instruction::LoadVar(name) => OpView::LoadVar(name.clone()),
                    Instruction::StoreVar(name) => OpView::StoreVar(name.clone()),
                    Instruction::LoadLocal(i) => OpView::LoadLocal(*i),
                    Instruction::StoreLocal(i) => OpView::StoreLocal(*i),
                    Instruction::BinaryOp(op) => OpView::BinaryOp(op.clone()),
                    Instruction::UnaryOp(op) => OpView::UnaryOp(op.clone()),
                    Instruction::CallBuiltin(name, argc) => OpView::CallBuiltin {
                        name: name.clone(),
                        argc: *argc,
                    },
                    Instruction::CallBuiltinId(id, argc) => OpView::CallBuiltinId {
                        id: *id,
                        argc: *argc,
                    },
                    Instruction::CallLocal(slot, argc) => OpView::CallLocal {
                        slot: *slot,
                        argc: *argc,
                    },
                    Instruction::CallUser(name, argc) => OpView::CallUser {
                        name: name.clone(),
                        argc: *argc,
                    },
                    Instruction::CallAnon(argc) => OpView::CallAnon(*argc),
                    Instruction::MakeList(n) => OpView::MakeList(*n),
                    Instruction::MakeDict(n) => OpView::MakeDict(*n),
                    Instruction::Index => OpView::Index,
                    Instruction::IndexAssign => OpView::IndexAssign,
                    Instruction::IndexAssignLocal(i) => OpView::IndexAssignLocal(*i),
                    Instruction::Jump(pos) => OpView::Jump(*pos),
                    Instruction::JumpIfFalse(pos) => OpView::JumpIfFalse(*pos),
                    Instruction::Pop => OpView::Pop,
                    Instruction::Assert => OpView::Assert,
                    Instruction::Return => OpView::Return,
                }
            };

            // borrow dropped; advance program counter
            self.pc += 1;

            // Phase B: execute
            match op {
                OpView::LoadConst(v) => self.stack.push(v),
                OpView::LoadVar(name) => {
                    let (cver, cval) = {
                        let c = &self.inline_cache[idx];
                        (c.version, c.value.clone())
                    };
                    if cver == self.global_version {
                        if let Some(v) = cval {
                            self.stack.push(v);
                            continue;
                        }
                    }
                    let (val, ver) = if let Some(val) = self.lookup_global(&name) {
                        (val, self.global_version)
                    } else if self.builtins.has_function(&name) {
                        (Value::BuiltinFunction(name.clone()), u64::MAX)
                    } else {
                        return Err(WqError::ValueError(
                            format!("Undefined variable: '{name}'",),
                        ));
                    };
                    {
                        let c = &mut self.inline_cache[idx];
                        c.version = ver;
                        c.value = Some(val.clone());
                    }
                    self.stack.push(val);
                }
                OpView::StoreVar(name) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into variable '{name}'",
                        ))
                    })?;
                    self.assign_global(name, val);
                }
                OpView::LoadLocal(i) => {
                    let val = self
                        .locals
                        .last()
                        .and_then(|f| f.get(i as usize))
                        .cloned()
                        .ok_or_else(|| WqError::RuntimeError(format!("Invalid local slot {i}")))?;
                    self.stack.push(val);
                }
                OpView::StoreLocal(i) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into local slot {i}",
                        ))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(slot) = frame.get_mut(i as usize) {
                            *slot = val;
                        } else {
                            return Err(WqError::RuntimeError(format!("Invalid local slot {i}")));
                        }
                    } else {
                        return Err(WqError::RuntimeError("No local frame".into()));
                    }
                }
                OpView::BinaryOp(op) => {
                    let right = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError("Stack underflow: missing right operand".into())
                    })?;
                    let left = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError("Stack underflow: missing left operand".into())
                    })?;

                    let result = match op {
                        BinaryOperator::Add => left
                            .add(&right)
                            .ok_or_else(|| classify_arith("add", &left, &right)),
                        BinaryOperator::Subtract => left
                            .subtract(&right)
                            .ok_or_else(|| classify_arith("subtract", &left, &right)),
                        BinaryOperator::Multiply => left
                            .multiply(&right)
                            .ok_or_else(|| classify_arith("multiply", &left, &right)),
                        BinaryOperator::Power => left
                            .power(&right)
                            .ok_or_else(|| classify_arith("exponentiate", &left, &right)),
                        BinaryOperator::Divide => left
                            .divide(&right)
                            .ok_or_else(|| classify_divide(&left, &right)),
                        BinaryOperator::DivideDot => left
                            .divide_dot(&right)
                            .ok_or_else(|| WqError::DomainError("Invalid types".into())),
                        BinaryOperator::Modulo => left.modulo(&right).ok_or_else(|| {
                            WqError::DomainError("Modulo by zero or invalid types".into())
                        }),
                        BinaryOperator::ModuloDot => left
                            .modulo_dot(&right)
                            .ok_or_else(|| WqError::DomainError("Invalid types".into())),
                        BinaryOperator::Equal => Ok(left.equals(&right)),
                        BinaryOperator::NotEqual => Ok(left.not_equals(&right)),
                        BinaryOperator::LessThan => Ok(left.less_than(&right)),
                        BinaryOperator::LessThanOrEqual => Ok(left.less_than_or_equal(&right)),
                        BinaryOperator::GreaterThan => Ok(left.greater_than(&right)),
                        BinaryOperator::GreaterThanOrEqual => {
                            Ok(left.greater_than_or_equal(&right))
                        }
                    };

                    match result {
                        Ok(v) => self.stack.push(v),
                        Err(e) => return Err(e),
                    }
                }
                OpView::UnaryOp(op) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing operand for unary operator".into(),
                        )
                    })?;

                    let result = match op {
                        UnaryOperator::Negate => val
                            .neg_value()
                            .ok_or_else(|| WqError::TypeError("Cannot negate this type".into())),
                        UnaryOperator::Count => Ok(Value::Int(val.len() as i64)),
                    }?;
                    self.stack.push(result);
                }
                OpView::CallBuiltin { name, argc } => {
                    let mut args = Vec::with_capacity(argc);
                    for i in (0..argc).rev() {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {} arguments for builtin '{}', only found {}",
                                argc, name, argc - i
                            ))
                        })?;
                        args.push(arg);
                    }

                    let result = self.builtins.call(&name, &args)?;
                    self.stack.push(result);
                }
                OpView::CallBuiltinId { id, argc } => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {argc} arguments for builtin ID {id} but only found fewer",
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();
                    let result = self.builtins.call_id(id as usize, &args)?;
                    self.stack.push(result);
                }
                OpView::CallLocal { slot, argc } => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {argc} arguments for local function at slot {slot} but only found fewer",
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();
                    let func_val = {
                        let mut found = None;
                        for frame in self.locals.iter().rev() {
                            if let Some(v) = frame.get(slot as usize) {
                                found = Some(v.clone());
                                break;
                            }
                        }
                        found.ok_or_else(|| {
                            WqError::RuntimeError(format!("Invalid local slot {slot}"))
                        })?
                    };
                    match func_val {
                        Value::BytecodeFunction {
                            params,
                            locals,
                            instructions,
                        } => {
                            let res =
                                self.call_function(instructions.clone(), params, locals, args)?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let instrs = c.instructions.clone();
                            let res = self.call_function(instrs, params, locals, args)?;
                            self.stack.push(res);
                        }
                        other => {
                            return Err(WqError::TypeError(format!(
                                "Cannot call local slot {slot}: expected function, found {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                OpView::CallUser { name, argc } => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {argc} arguments for function '{name}' but only found fewer",
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();

                    let (cver, cval) = {
                        let c = &self.inline_cache[idx];
                        (c.version, c.value.clone())
                    };
                    let func_val = if cver == self.global_version {
                        cval
                    } else {
                        None
                    };
                    let func_val = if let Some(v) = func_val {
                        v
                    } else {
                        let v = self.lookup_global(&name).ok_or_else(|| {
                            WqError::ValueError(format!("Function '{name}' is not defined"))
                        })?;
                        {
                            let c = &mut self.inline_cache[idx];
                            c.version = self.global_version;
                            c.value = Some(v.clone());
                        }
                        v
                    };
                    match func_val {
                        Value::BytecodeFunction {
                            params,
                            locals,
                            instructions,
                        } => {
                            let res =
                                self.call_function(instructions.clone(), params, locals, args)?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let instrs = c.instructions.clone();
                            {
                                let entry = &mut self.inline_cache[idx];
                                entry.version = self.global_version;
                                entry.value = Some(Value::BytecodeFunction {
                                    params: params.clone(),
                                    locals,
                                    instructions: instrs.clone(),
                                });
                            }
                            let res = self.call_function(instrs, params, locals, args)?;
                            self.stack.push(res);
                        }
                        other => {
                            return Err(WqError::TypeError(format!(
                                "Cannot call '{name}': expected function, found {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                OpView::CallAnon(argc) => {
                    let mut args = Vec::with_capacity(argc);

                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {argc} arguments but only found fewer",
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();
                    let func_val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing function value for call".into(),
                        )
                    })?;

                    match func_val {
                        Value::BytecodeFunction {
                            params,
                            locals,
                            instructions,
                        } => {
                            let res = self.call_function(instructions, params, locals, args)?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let res = self.call_function(c.instructions, params, locals, args)?;
                            self.stack.push(res);
                        }
                        other => {
                            return Err(WqError::TypeError(format!(
                                "Cannot call value of type {:?} as a function",
                                other.type_name(),
                            )));
                        }
                    }
                }
                OpView::MakeList(n) => {
                    let mut items = Vec::with_capacity(n);
                    let mut all_ints = n > 0;
                    for i in (0..n).rev() {
                        let item = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {} list items, only got {}",
                                n,
                                n - i
                            ))
                        })?;
                        if !matches!(item, Value::Int(_)) {
                            all_ints = false;
                        }
                        items.push(item);
                    }
                    items.reverse();
                    if all_ints {
                        let ints: Vec<i64> = items
                            .into_iter()
                            .map(|v| match v {
                                Value::Int(i) => i,
                                _ => unreachable!(),
                            })
                            .collect();
                        self.stack.push(Value::IntList(ints));
                    } else {
                        self.stack.push(Value::List(items));
                    }
                }
                OpView::MakeDict(n) => {
                    let mut map = IndexMap::new();
                    for _ in 0..n {
                        let val = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError("Stack underflow: expected value for dict".into())
                        })?;
                        let key = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError("Stack underflow: expected key for dict".into())
                        })?;

                        match key {
                            Value::Symbol(k) => {
                                map.insert(k, val);
                            }
                            other => {
                                return Err(WqError::TypeError(format!(
                                    "Invalid dict key: expected Symbol, got {other:?}"
                                )));
                            }
                        }
                    }
                    self.stack.push(Value::Dict(map));
                }
                OpView::Index => {
                    let idx = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match obj.index(&idx) {
                        Some(v) => self.stack.push(v),
                        None => {
                            return Err(WqError::IndexError(format!(
                                "Invalid index: attempted to access index {idx} in {obj}"
                            )));
                        }
                    }
                }
                OpView::IndexAssign => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing value for index assignment".into(),
                        )
                    })?;

                    let idx = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing index for assignment".into(),
                        )
                    })?;

                    let obj_name = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing target object name for index assignment"
                                .into(),
                        )
                    })?;

                    match obj_name {
                        Value::Symbol(name) => match self.globals.get_mut(&name) {
                            Some(obj) => {
                                if obj.set_index(&idx, val.clone()).is_some() {
                                    self.stack.push(val);
                                } else {
                                    return Err(WqError::IndexError(format!(
                                        "Failed assigning to {name}[{idx}], index invalid or not supported",
                                    )));
                                }
                            }
                            None => {
                                return Err(WqError::ValueError(format!(
                                    "Cannot assign to {name}[{idx}], variable not found",
                                )));
                            }
                        },
                        other => {
                            return Err(WqError::TypeError(format!(
                                "Invalid index assignment target: expected Symbol, got {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                OpView::IndexAssignLocal(slot) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing value for index assignment".into(),
                        )
                    })?;
                    let idx = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing index for assignment".into(),
                        )
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(obj) = frame.get_mut(slot as usize) {
                            if obj.set_index(&idx, val.clone()).is_some() {
                                self.stack.push(val);
                            } else {
                                return Err(WqError::IndexError(format!(
                                    "Failed assigning to local[{slot}][{idx}], index invalid or not supported",
                                )));
                            }
                        } else {
                            return Err(WqError::RuntimeError(format!(
                                "Invalid local slot {slot}"
                            )));
                        }
                    } else {
                        return Err(WqError::RuntimeError("No local frame".into()));
                    }
                }
                OpView::Jump(pos) => self.pc = pos,
                OpView::JumpIfFalse(pos) => {
                    let v = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing value for conditional jump".into(),
                        )
                    })?;

                    let is_false = match v {
                        Value::Bool(b) => !b,
                        Value::Int(n) => n == 0,
                        Value::Float(f) => f == 0.0,
                        _ => {
                            return Err(WqError::TypeError(
                                "Invalid condition type in control flow, expected bool, int, or float".into(),
                            ));
                        }
                    };

                    if is_false {
                        self.pc = pos;
                    }
                }
                OpView::Pop => {
                    self.stack.pop();
                }
                OpView::Assert => {
                    let v = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError("Stack underflow: missing value for assert".into())
                    })?;
                    let ok = match v {
                        Value::Bool(b) => b,
                        _ => {
                            return Err(WqError::TypeError(
                                "Invalid type for assert, expected bool".into(),
                            ));
                        }
                    };
                    if !ok {
                        return Err(WqError::AssertionError("Assertion failed".into()));
                    }
                    self.stack.push(Value::Null);
                }
                OpView::Return => break,
            }
        }
        Ok(self.stack.pop().unwrap_or(Value::Null))
    }
}

fn classify_arith(op_name: &str, left: &Value, right: &Value) -> WqError {
    if matches!(left, Value::Int(_) | Value::IntList(_))
        && matches!(right, Value::Int(_) | Value::IntList(_))
    {
        WqError::DomainError(format!("{op_name} overflow"))
    } else {
        WqError::TypeError(format!(
            "Cannot {op_name} {} and {}",
            left.type_name(),
            right.type_name()
        ))
    }
}

fn classify_divide(left: &Value, right: &Value) -> WqError {
    let is_zero = match right {
        Value::Int(0) => true,
        Value::Float(f) if *f == 0.0 => true,
        Value::IntList(v) if v.contains(&0) => true,
        _ => false,
    };

    if is_zero {
        WqError::DomainError("Division by zero".into())
    } else if matches!(left, Value::Int(_) | Value::IntList(_))
        && matches!(right, Value::Int(_) | Value::IntList(_))
    {
        WqError::DomainError("division overflow".into())
    } else {
        WqError::TypeError("Cannot divide these types".into())
    }
}
