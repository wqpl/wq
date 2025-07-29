use super::compiler::Compiler;
use super::fastpath::{FastPath, TryRunOutcome};
use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};
use indexmap::IndexMap;
use std::collections::HashMap;

pub struct Vm {
    pub instructions: Vec<Instruction>,
    pc: usize,
    stack: Vec<Value>,
    env_stack: Vec<HashMap<String, Value>>,
    builtins: Builtins,
}

impl Vm {
    pub fn new(instructions: Vec<Instruction>) -> Self {
        Vm {
            instructions,
            pc: 0,
            stack: Vec::new(),
            env_stack: vec![HashMap::new()],
            builtins: Builtins::new(),
        }
    }

    /// Replace instructions and reset execution state.
    pub fn reset(&mut self, instructions: Vec<Instruction>) {
        self.instructions = instructions;
        self.pc = 0;
        self.stack.clear();
    }

    /// Access the global environment frame.
    pub fn global_env(&self) -> &HashMap<String, Value> {
        &self.env_stack[0]
    }

    /// Mutable access to the global environment frame.
    pub fn global_env_mut(&mut self) -> &mut HashMap<String, Value> {
        &mut self.env_stack[0]
    }

    fn env(&mut self) -> &mut HashMap<String, Value> {
        self.env_stack.last_mut().unwrap()
    }

    fn lookup(&self, name: &str) -> Option<Value> {
        for env in self.env_stack.iter().rev() {
            if let Some(v) = env.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn lookup_mut(&mut self, name: &str) -> Option<&mut Value> {
        for env in self.env_stack.iter_mut().rev() {
            if env.contains_key(name) {
                return env.get_mut(name);
            }
        }
        None
    }

    fn assign(&mut self, name: String, value: Value) {
        for env in self.env_stack.iter_mut().rev() {
            if env.contains_key(&name) {
                env.insert(name.clone(), value);
                return;
            }
        }
        self.env().insert(name, value);
    }

    fn push_frame(&mut self, frame: HashMap<String, Value>) {
        self.env_stack.push(frame);
    }

    fn pop_frame(&mut self) {
        self.env_stack.pop();
    }

    fn call_function(
        &mut self,
        instructions: Vec<Instruction>,
        params: Option<Vec<String>>,
        args: Vec<Value>,
    ) -> WqResult<Value> {
        let saved_instructions = std::mem::replace(&mut self.instructions, instructions);
        let saved_pc = self.pc;
        let mut saved_stack = Vec::new();
        std::mem::swap(&mut self.stack, &mut saved_stack);
        self.pc = 0;

        let mut frame = HashMap::new();
        if let Some(p) = params {
            if args.len() != p.len() {
                return Err(WqError::ArityError(format!(
                    "Function expects {} arguments, got {}",
                    p.len(),
                    args.len()
                )));
            }
            for (param, arg) in p.into_iter().zip(args.into_iter()) {
                frame.insert(param, arg);
            }
        } else {
            if args.len() > 3 {
                return Err(WqError::ArityError(
                    "Implicit function expects up to 3 arguments".to_string(),
                ));
            }
            if let Some(arg) = args.first() {
                frame.insert("x".to_string(), arg.clone());
            }
            if let Some(arg) = args.get(1) {
                frame.insert("y".to_string(), arg.clone());
            }
            if let Some(arg) = args.get(2) {
                frame.insert("z".to_string(), arg.clone());
            }
        }
        self.push_frame(frame);
        let res = self.execute();
        self.pop_frame();
        let result = res?;
        std::mem::swap(&mut self.stack, &mut saved_stack);
        self.instructions = saved_instructions;
        self.pc = saved_pc;
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
        while self.pc < self.instructions.len() {
            let instr = self.instructions[self.pc].clone();
            self.pc += 1;
            match instr {
                Instruction::LoadConst(v) => self.stack.push(v),
                Instruction::LoadVar(name) => match self.lookup(&name) {
                    Some(val) => self.stack.push(val),
                    None => {
                        if self.builtins.has_function(&name) {
                            self.stack.push(Value::BuiltinFunction(name));
                        } else {
                            return Err(WqError::ValueError(format!(
                                "Undefined variable: '{name}'"
                            )));
                        }
                    }
                },
                Instruction::StoreVar(name) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into variable '{name}'"
                        ))
                    })?;
                    self.assign(name.clone(), val);
                }

                Instruction::StoreLocalVar(name) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(format!(
                            "Stack underflow: cannot store into local variable '{name}'"
                        ))
                    })?;
                    self.env().insert(name.clone(), val);
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
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }
                Instruction::UnaryOp(op) => {
                    let val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing operand for unary operator".into(),
                        )
                    })?;

                    let result = match op {
                        UnaryOperator::Negate => match val {
                            Value::Int(n) => n
                                .checked_neg()
                                .map(Value::Int)
                                .ok_or_else(|| WqError::DomainError("negation overflow".into())),
                            Value::Float(f) => Ok(Value::Float(-f)),
                            _ => Err(WqError::TypeError("Cannot negate this type".into())),
                        },
                        UnaryOperator::Count => Ok(Value::Int(val.len() as i64)),
                    };

                    let v = result?; // Propagate the error as-is
                    self.stack.push(v);
                }
                Instruction::CallBuiltin(name, argc) => {
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

                Instruction::CallBuiltinId(id, argc) => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {} arguments for builtin ID {} but only found fewer",
                                argc, id
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();
                    let result = self.builtins.call_id(id as usize, &args)?;

                    self.stack.push(result);
                }
                Instruction::CallUser(name, argc) => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {} arguments for function '{}' but only found fewer",
                                argc, name
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();
                    let func = self.lookup(&name);
                    match func {
                        Some(Value::BytecodeFunction {
                            params,
                            instructions,
                        }) => {
                            let res = self.call_function(instructions, params, args)?;
                            self.stack.push(res);
                        }
                        Some(Value::Function { params, body }) => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let res = self.call_function(c.instructions, params, args)?;
                            self.stack.push(res);
                        }
                        Some(other) => {
                            return Err(WqError::TypeError(format!(
                                "Cannot call '{}': expected function, found {}",
                                name,
                                other.type_name()
                            )));
                        }
                        None => {
                            return Err(WqError::ValueError(format!(
                                "Function '{}' is not defined",
                                name
                            )));
                        }
                    }
                }
                Instruction::CallAnon(argc) => {
                    let mut args = Vec::with_capacity(argc);

                    // Pop arguments in reverse order for proper evaluation
                    for _ in 0..argc {
                        let arg = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {} arguments but only found fewer",
                                argc
                            ))
                        })?;
                        args.push(arg);
                    }
                    args.reverse();
                    // Pop function value
                    let func_val = self.stack.pop().ok_or_else(|| {
                        WqError::RuntimeError(
                            "Stack underflow: missing function value for call".into(),
                        )
                    })?;

                    match func_val {
                        Value::BytecodeFunction {
                            params,
                            instructions,
                        } => {
                            let res = self.call_function(instructions, params, args)?;
                            self.stack.push(res);
                        }
                        Value::Function { params, body } => {
                            let mut c = Compiler::new();
                            c.compile(&body)?;
                            c.instructions.push(Instruction::Return);
                            let res = self.call_function(c.instructions, params, args)?;
                            self.stack.push(res);
                        }
                        other => {
                            return Err(WqError::TypeError(format!(
                                "Cannot call value of type {:?} as a function",
                                other.type_name()
                            )));
                        }
                    }
                }
                Instruction::MakeList(n) => {
                    let mut items = Vec::with_capacity(n);
                    for i in (0..n).rev() {
                        let item = self.stack.pop().ok_or_else(|| {
                            WqError::RuntimeError(format!(
                                "Stack underflow: expected {} list items, only got {}",
                                n,
                                n - i
                            ))
                        })?;
                        items.push(item);
                    }
                    items.reverse(); // Optional: only needed if preserving original order matters
                    self.stack.push(Value::List(items));
                }
                Instruction::MakeDict(n) => {
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
                                    "Invalid dict key: expected Symbol, got {:?}",
                                    other
                                )));
                            }
                        }
                    }
                    self.stack.push(Value::Dict(map));
                }
                Instruction::Index => {
                    let idx = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match obj.index(&idx) {
                        Some(v) => self.stack.push(v),
                        None => {
                            return Err(WqError::IndexError(format!(
                                "Invalid index: attempted to access index {} in {}",
                                idx, obj
                            )));
                        }
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
                        Value::Symbol(name) => match self.lookup_mut(&name) {
                            Some(obj) => {
                                if obj.set_index(&idx, val.clone()).is_some() {
                                    self.stack.push(val);
                                } else {
                                    return Err(WqError::IndexError(format!(
                                        "Failed assigning to {name}[{idx}], index invalid or not supported"
                                    )));
                                }
                            }
                            None => {
                                return Err(WqError::ValueError(format!(
                                    "Cannot assign to {name}[{idx}], variable not found"
                                )));
                            }
                        },
                        other => {
                            return Err(WqError::TypeError(format!(
                                "Invalid index assignment target: expected Symbol, got {}",
                                other.type_name()
                            )));
                        }
                    }
                }
                Instruction::Jump(pos) => self.pc = pos,
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
                        self.pc = pos;
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
