use super::compiler::Compiler;
use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};
use indexmap::IndexMap;
use std::collections::HashMap;

#[derive(Clone, Default)]
struct InlineCache {
    version: u64,
    value: Option<Value>,
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
            stack: Vec::with_capacity(256),
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

    fn assign_global(&mut self, name: &str, value: Value) {
        self.globals.insert(name.to_string(), value);
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
        // Preserve stack allocation to avoid reallocations during the call
        let prev_cap = self.stack.capacity();
        let mut saved_stack = std::mem::replace(
            &mut self.stack,
            Vec::with_capacity(std::cmp::max(prev_cap, 256)),
        );
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
        self.execute()
    }

    fn execute(&mut self) -> WqResult<Value> {
        while self.pc < self.instructions.len() {
            let idx = self.pc;
            // Borrow current instruction by reference to avoid cloning per step
            let op_ref = unsafe { self.instructions.get_unchecked(idx) };
            // advance program counter
            self.pc += 1;

            match op_ref {
                Instruction::LoadConst(v) => self.stack.push(v.clone()),
                Instruction::LoadVar(name) => {
                    let (cver, cval) = {
                        let c = &self.inline_cache[idx];
                        (c.version, c.value.as_ref())
                    };
                    if cver == self.global_version {
                        if let Some(v) = cval {
                            self.stack.push(v.clone());
                            continue;
                        }
                    }
                    let (val, ver) = if let Some(val) = self.lookup_global(name) {
                        (val, self.global_version)
                    } else if self.builtins.has_function(name) {
                        (Value::BuiltinFunction(name.clone()), u64::MAX)
                    } else {
                        return Err(WqError::ValueError(format!(
                            "Undefined variable: '{name}'",
                        )));
                    };
                    {
                        let c = &mut self.inline_cache[idx];
                        c.version = ver;
                        c.value = Some(val.clone());
                    }
                    self.stack.push(val);
                }
                Instruction::StoreVar(name) => {
                    let name_owned = name.clone();
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into variable '{name_owned}'",
                        ))
                    })?;
                    self.assign_global(&name_owned, val);
                }
                Instruction::StoreVarKeep(name) => {
                    let name_owned = name.clone();
                    let val = self.stack.last().cloned().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into variable '{name_owned}'",
                        ))
                    })?;
                    self.assign_global(&name_owned, val);
                }
                Instruction::LoadLocal(i) => {
                    let val = self
                        .locals
                        .last()
                        .and_then(|f| f.get(*i as usize))
                        .ok_or_else(|| WqError::RuntimeError(format!("Invalid local slot {i}")))?;
                    self.stack.push(val.clone());
                }
                Instruction::StoreLocal(i) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into local slot {i}",
                        ))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(slot) = frame.get_mut(*i as usize) {
                            *slot = val;
                        } else {
                            return Err(WqError::RuntimeError(format!("Invalid local slot {i}")));
                        }
                    } else {
                        return Err(WqError::RuntimeError("No local frame".into()));
                    }
                }
                Instruction::StoreLocalKeep(i) => {
                    let val = self.stack.last().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into local slot {i}",
                        ))
                    })?;
                    if let Some(frame) = self.locals.last_mut() {
                        if let Some(slot) = frame.get_mut(*i as usize) {
                            *slot = val.clone();
                        } else {
                            return Err(WqError::RuntimeError(format!("Invalid local slot {i}")));
                        }
                    } else {
                        return Err(WqError::RuntimeError("No local frame".into()));
                    }
                }
                Instruction::BinaryOp(op) => {
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
                Instruction::UnaryOp(op) => {
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
                Instruction::CallBuiltin(name, argc) => {
                    let argc_val = *argc;
                    if self.stack.len() < argc_val {
                        return Err(WqError::RuntimeError(format!(
                            "Stack underflow: expected {argc_val} arguments for builtin '{name}'",
                        )));
                    }
                    let base = self.stack.len() - argc_val;
                    let result = self.builtins.call(name, &self.stack[base..])?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Instruction::CallBuiltinId(id, argc) => {
                    if self.stack.len() < *argc {
                        return Err(WqError::RuntimeError(format!(
                            "Stack underflow: expected {argc} arguments for builtin ID {id}",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let result = self.builtins.call_id(*id as usize, &self.stack[base..])?;
                    self.stack.truncate(base);
                    self.stack.push(result);
                }
                Instruction::CallLocal(slot, argc) => {
                    if self.stack.len() < *argc {
                        return Err(WqError::RuntimeError(format!(
                            "Stack underflow: expected {argc} arguments for local function at slot {slot}",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let args: Vec<Value> = self.stack.drain(base..).collect();
                    // Walk frames top-down; compile once and replace
                    let maybe_compiled = {
                        let mut compiled: Option<(Option<Vec<String>>, u16, Vec<Instruction>)> =
                            None;
                        for frame in self.locals.iter_mut().rev() {
                            if let Some(v) = frame.get_mut(*slot as usize) {
                                match v {
                                    Value::BytecodeFunction {
                                        params,
                                        locals,
                                        instructions,
                                    } => {
                                        compiled = Some((params.clone(), *locals, instructions.clone()));
                                    }
                                    Value::Function { params, body } => {
                                        let mut c = Compiler::new();
                                        c.compile(body)?;
                                        c.instructions.push(Instruction::Return);
                                        let locals = c.local_count();
                                        let instrs = std::mem::take(&mut c.instructions);
                                        let compiled_params = params.clone();
                                        *v = Value::BytecodeFunction {
                                            params: compiled_params.clone(),
                                            locals,
                                            instructions: instrs.clone(),
                                        };
                                        compiled = Some((compiled_params, locals, instrs));
                                    }
                                    other => {
                                        return Err(WqError::TypeError(format!(
                                            "Cannot call local slot {slot}: expected function, found {}",
                                            other.type_name(),
                                        )));
                                    }
                                }
                                break;
                            }
                        }
                        compiled
                    };
                    let (params, locals, instructions) = maybe_compiled.ok_or_else(|| {
                        WqError::RuntimeError(format!("Invalid local slot {slot}"))
                    })?;
                    let res = self.call_function(instructions, params, locals, args)?;
                    self.stack.push(res);
                }
                Instruction::CallUser(name, argc) => {
                    let name_owned = name.clone();
                    let argc_val = *argc;
                    if self.stack.len() < argc_val {
                        return Err(WqError::RuntimeError(format!(
                            "Stack underflow: expected {argc_val} arguments for function '{name_owned}'",
                        )));
                    }
                    let base = self.stack.len() - argc_val;
                    let args: Vec<Value> = self.stack.drain(base..).collect();

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
                        let v = self.lookup_global(&name_owned).ok_or_else(|| {
                            WqError::ValueError(format!("Function '{name_owned}' is not defined"))
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
                                self.call_function(instructions, params, locals, args)?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let locals = c.local_count();
                            let instrs = c.instructions.clone();
                            // Replace in globals to avoid recompilation on next lookup
                            if let Some(slot) = self.globals.get_mut(&name_owned) {
                                *slot = Value::BytecodeFunction {
                                    params: params.clone(),
                                    locals,
                                    instructions: instrs.clone(),
                                };
                            }
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
                                "Cannot call '{name_owned}': expected function, found {}",
                                other.type_name(),
                            )));
                        }
                    }
                }
                Instruction::CallAnon(argc) => {
                    if self.stack.len() < *argc + 1 {
                        return Err(WqError::RuntimeError(format!(
                            "Stack underflow: expected {argc} arguments and a function",
                        )));
                    }
                    let base = self.stack.len() - *argc;
                    let args: Vec<Value> = self.stack.drain(base..).collect();
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
                Instruction::MakeList(n) => {
                    if self.stack.len() < *n {
                        return Err(WqError::RuntimeError(format!(
                            "Stack underflow: expected {n} list items",
                        )));
                    }
                    let base = self.stack.len() - *n;
                    let items: Vec<Value> = self.stack.drain(base..).collect();
                    let all_ints = items.iter().all(|v| matches!(v, Value::Int(_)));
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
                Instruction::MakeDict(n) => {
                    let mut map = IndexMap::new();
                    for _ in 0..*n {
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
                Instruction::Index => {
                    // Hot path: IntList/List with integer index
                    match (self.stack.pop().unwrap(), self.stack.pop().unwrap()) {
                        (Value::Int(i), Value::IntList(items)) => {
                            let len = items.len() as i64;
                            let ii = if i < 0 { len + i } else { i };
                            if ii >= 0 && ii < len {
                                let idx = ii as usize;
                                self.stack.push(Value::Int(items[idx]));
                            } else {
                                return Err(WqError::IndexError(format!(
                                    "Invalid index: attempted to access index {i} in int-list of len {}",
                                    items.len()
                                )));
                            }
                        }
                        (Value::Int(i), Value::List(items)) => {
                            let len = items.len() as i64;
                            let ii = if i < 0 { len + i } else { i };
                            if ii >= 0 && ii < len {
                                let idx = ii as usize;
                                let v = items.get(idx).cloned().unwrap();
                                self.stack.push(v);
                            } else {
                                return Err(WqError::IndexError(format!(
                                    "Invalid index: attempted to access index {i} in list of len {}",
                                    items.len()
                                )));
                            }
                        }
                        (idx, obj) => match obj.index(&idx) {
                            Some(v) => self.stack.push(v),
                            None => {
                                return Err(WqError::IndexError(format!(
                                    "Invalid index: attempted to access index {idx} in {obj}"
                                )));
                            }
                        },
                    }
                }
                Instruction::IndexAssign => {
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
                                // Fast path: int-list/list with int index
                                match (&mut *obj, &idx, &val) {
                                    (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                        let len = items.len() as i64;
                                        let ii = if *i < 0 { len + *i } else { *i };
                                        if ii >= 0 && ii < len {
                                            items[ii as usize] = *v;
                                            self.stack.push(val);
                                        } else {
                                            return Err(WqError::IndexError(format!(
                                                "Failed assigning to {name}[{i}], out of bounds",
                                            )));
                                        }
                                    }
                                    (Value::List(items), Value::Int(i), _) => {
                                        let len = items.len() as i64;
                                        let ii = if *i < 0 { len + *i } else { *i };
                                        if ii >= 0 && ii < len {
                                            items[ii as usize] = val.clone();
                                            self.stack.push(val);
                                        } else {
                                            return Err(WqError::IndexError(format!(
                                                "Failed assigning to {name}[{i}], out of bounds",
                                            )));
                                        }
                                    }
                                    _ => {
                                        if (*obj).set_index(&idx, val.clone()).is_some() {
                                            self.stack.push(val);
                                        } else {
                                            return Err(WqError::IndexError(format!(
                                                "Failed assigning to {name}[{idx}], index invalid or not supported",
                                            )));
                                        }
                                    }
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
                Instruction::IndexAssignDrop => {
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
                            Some(obj) => match (&mut *obj, &idx, &val) {
                                (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = *v;
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to {name}[{i}], out of bounds",
                                        )));
                                    }
                                }
                                (Value::List(items), Value::Int(i), _) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = val.clone();
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to {name}[{i}], out of bounds",
                                        )));
                                    }
                                }
                                _ => {
                                    if (*obj).set_index(&idx, val.clone()).is_none() {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to {name}[{idx}], index invalid or not supported",
                                        )));
                                    }
                                }
                            },
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
                Instruction::IndexAssignLocal(slot) => {
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
                        if let Some(obj) = frame.get_mut(*slot as usize) {
                            match (&mut *obj, &idx, &val) {
                                (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = *v;
                                        self.stack.push(val);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                (Value::List(items), Value::Int(i), _) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = val.clone();
                                        self.stack.push(val);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                _ => {
                                    if (*obj).set_index(&idx, val.clone()).is_some() {
                                        self.stack.push(val);
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to local[{slot}][{idx}], index invalid or not supported",
                                        )));
                                    }
                                }
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
                Instruction::IndexAssignLocalDrop(slot) => {
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
                        if let Some(obj) = frame.get_mut(*slot as usize) {
                            match (&mut *obj, &idx, &val) {
                                (Value::IntList(items), Value::Int(i), Value::Int(v)) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = *v;
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                (Value::List(items), Value::Int(i), _) => {
                                    let len = items.len() as i64;
                                    let ii = if *i < 0 { len + *i } else { *i };
                                    if ii >= 0 && ii < len {
                                        items[ii as usize] = val.clone();
                                    } else {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to local[{slot}][{i}], out of bounds",
                                        )));
                                    }
                                }
                                _ => {
                                    if (*obj).set_index(&idx, val.clone()).is_none() {
                                        return Err(WqError::IndexError(format!(
                                            "Failed assigning to local[{slot}][{idx}], index invalid or not supported",
                                        )));
                                    }
                                }
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
                Instruction::Jump(pos) => self.pc = *pos,
                Instruction::JumpIfFalse(pos) => {
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
                        self.pc = *pos;
                    }
                }
                Instruction::JumpIfGE(pos) => {
                    // Pop right then left, jump if left >= right
                    let right = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing right operand for compare-jump".into(),
                        )
                    })?;
                    let left = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing left operand for compare-jump".into(),
                        )
                    })?;
                    // emulate !(left < right)
                    let lt = left.less_than(&right);
                    let cond = match lt {
                        Value::Bool(b) => !b,
                        _ => {
                            return Err(WqError::TypeError(
                                "Invalid condition type in control flow, expected bool, int, or float".into(),
                            ));
                        }
                    };
                    if cond {
                        self.pc = *pos;
                    }
                }
                Instruction::JumpIfLEZLocal(slot, pos) => {
                    // Jump if local[slot] <= 0
                    let v = self
                        .locals
                        .last()
                        .and_then(|f| f.get(*slot as usize))
                        .cloned()
                        .ok_or_else(|| {
                            WqError::RuntimeError(format!("Invalid local slot {slot}"))
                        })?;
                    let is_le_zero = match v {
                        Value::Int(n) => n <= 0,
                        Value::Float(f) => f <= 0.0,
                        _ => {
                            return Err(WqError::TypeError(
                                "Invalid condition type in control flow, expected bool, int, or float".into(),
                            ));
                        }
                    };
                    if is_le_zero {
                        self.pc = *pos;
                    }
                }
                Instruction::Pop => {
                    self.stack.pop();
                }
                Instruction::Assert => {
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
                Instruction::Return => break,
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
