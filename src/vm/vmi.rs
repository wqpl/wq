use super::compiler::Compiler;
use super::fastpath::FastPath;
use super::instruction::Instruction;
use crate::builtins::Builtins;
use crate::parser::{BinaryOperator, UnaryOperator};
use crate::value::valuei::{Value, WqError, WqResult};
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
            for (param, arg) in p.into_iter().zip(args.into_iter()) {
                frame.insert(param, arg);
            }
        } else {
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
        if let Some(res) = FastPath::try_run(&self.instructions, &self.builtins) {
            return res;
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
                        return Err(WqError::DomainError(format!("Undefined variable: {name}")));
                    }
                },
                Instruction::StoreVar(name) => {
                    if let Some(v) = self.stack.pop() {
                        self.assign(name.clone(), v);
                    }
                }
                Instruction::BinaryOp(op) => {
                    let right = self.stack.pop().unwrap();
                    let left = self.stack.pop().unwrap();
                    let result = match op {
                        BinaryOperator::Add => match left.add(&right) {
                            Some(v) => Ok(v),
                            None => {
                                if matches!(left, Value::Int(_) | Value::IntList(_))
                                    && matches!(right, Value::Int(_) | Value::IntList(_))
                                {
                                    Err(WqError::ArithmeticOverflowError(
                                        "addition overflow".into(),
                                    ))
                                } else {
                                    Err(WqError::TypeError("Cannot add these types".to_string()))
                                }
                            }
                        },
                        BinaryOperator::Subtract => match left.subtract(&right) {
                            Some(v) => Ok(v),
                            None => {
                                if matches!(left, Value::Int(_) | Value::IntList(_))
                                    && matches!(right, Value::Int(_) | Value::IntList(_))
                                {
                                    Err(WqError::ArithmeticOverflowError(
                                        "subtraction overflow".into(),
                                    ))
                                } else {
                                    Err(WqError::TypeError(
                                        "Cannot subtract these types".to_string(),
                                    ))
                                }
                            }
                        },
                        BinaryOperator::Multiply => match left.multiply(&right) {
                            Some(v) => Ok(v),
                            None => {
                                if matches!(left, Value::Int(_) | Value::IntList(_))
                                    && matches!(right, Value::Int(_) | Value::IntList(_))
                                {
                                    Err(WqError::ArithmeticOverflowError(
                                        "multiplication overflow".into(),
                                    ))
                                } else {
                                    Err(WqError::TypeError(
                                        "Cannot multiply these types".to_string(),
                                    ))
                                }
                            }
                        },
                        BinaryOperator::Divide => match left.divide(&right) {
                            Some(v) => Ok(v),
                            None => {
                                let zero_div = match &right {
                                    Value::Int(0) => true,
                                    Value::Float(f) if *f == 0.0 => true,
                                    Value::IntList(vec) if vec.contains(&0) => true,
                                    _ => false,
                                };
                                if zero_div {
                                    Err(WqError::ZeroDivisionError(
                                        "Division by zero or invalid types".to_string(),
                                    ))
                                } else if matches!(left, Value::Int(_) | Value::IntList(_))
                                    && matches!(right, Value::Int(_) | Value::IntList(_))
                                {
                                    Err(WqError::ArithmeticOverflowError(
                                        "division overflow".into(),
                                    ))
                                } else {
                                    Err(WqError::ZeroDivisionError(
                                        "Division by zero or invalid types".to_string(),
                                    ))
                                }
                            }
                        },
                        BinaryOperator::DivideDot => left
                            .divide_dot(&right)
                            .ok_or_else(|| WqError::DomainError("Invalid types".to_string())),
                        BinaryOperator::Modulo => left.modulo(&right).ok_or_else(|| {
                            WqError::ZeroDivisionError(
                                "Modulo by zero or invalid types".to_string(),
                            )
                        }),
                        BinaryOperator::ModuloDot => left
                            .modulo_dot(&right)
                            .ok_or_else(|| WqError::DomainError("Invalid types".to_string())),
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
                            return Err(WqError::TypeError(format!(
                                "Failed to execute binary operator {op:#?} - {e:#?}"
                            )));
                        }
                    }
                }
                Instruction::UnaryOp(op) => {
                    let val = self.stack.pop().unwrap();
                    let result = match op {
                        UnaryOperator::Negate => match val {
                            Value::Int(n) => n.checked_neg().map(Value::Int).ok_or_else(|| {
                                WqError::ArithmeticOverflowError("negation overflow".into())
                            })?,
                            Value::Float(f) => Value::Float(-f),
                            _ => return Err(WqError::TypeError("Cannot negate this type".into())),
                        },
                        UnaryOperator::Count => Value::Int(val.len() as i64),
                    };
                    self.stack.push(result);
                }
                Instruction::CallBuiltin(name, argc) => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap());
                    }
                    args.reverse();
                    let result = self.builtins.call(&name, &args)?;
                    self.stack.push(result);
                }
                Instruction::CallBuiltinId(id, argc) => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap());
                    }
                    args.reverse();
                    let result = self.builtins.call_id(id as usize, &args)?;
                    self.stack.push(result);
                }
                Instruction::CallUser(name, argc) => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap());
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
                        _ => {
                            return Err(WqError::DomainError(format!(
                                "Unknown function {name} with {argc} params"
                            )));
                        }
                    }
                }
                Instruction::CallAnon(argc) => {
                    let mut args = Vec::with_capacity(argc);
                    for _ in 0..argc {
                        args.push(self.stack.pop().unwrap());
                    }
                    args.reverse();
                    let func_val = self.stack.pop().unwrap();
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
                        _ => {
                            return Err(WqError::DomainError(
                                "Failed calling anonymous function - not a function".into(),
                            ));
                        }
                    }
                }
                Instruction::MakeList(n) => {
                    let mut items = Vec::with_capacity(n);
                    for _ in 0..n {
                        items.push(self.stack.pop().unwrap());
                    }
                    items.reverse();
                    self.stack.push(Value::List(items));
                }
                Instruction::MakeDict(n) => {
                    let mut map = HashMap::new();
                    for _ in 0..n {
                        let val = self.stack.pop().unwrap();
                        let key = self.stack.pop().unwrap();
                        if let Value::Symbol(k) = key {
                            map.insert(k, val);
                        } else {
                            return Err(WqError::TypeError("Invalid dict key".into()));
                        }
                    }
                    self.stack.push(Value::Dict(map));
                }
                Instruction::Index => {
                    let idx = self.stack.pop().unwrap();
                    let obj = self.stack.pop().unwrap();
                    match obj.index(&idx) {
                        Some(v) => self.stack.push(v),
                        None => return Err(WqError::IndexError("Invalid index".into())),
                    }
                }
                Instruction::IndexAssign => {
                    let val = self.stack.pop().unwrap();
                    let idx = self.stack.pop().unwrap();
                    let obj_name = self.stack.pop().unwrap();
                    if let Value::Symbol(name) = obj_name {
                        if let Some(obj) = self.lookup_mut(&name) {
                            if obj.set_index(&idx, val.clone()).is_some() {
                                self.stack.push(val);
                            } else {
                                return Err(WqError::IndexError(format!(
                                    "Failed when assigning to {name}, index = {idx}"
                                )));
                            }
                        } else {
                            return Err(WqError::DomainError(format!(
                                "Failed when assigning to {name} - unknown variable",
                            )));
                        }
                    } else {
                        return Err(WqError::DomainError(
                            "Failed when assigning - invalid index assign".to_string(),
                        ));
                    }
                }
                Instruction::Jump(pos) => self.pc = pos,
                Instruction::JumpIfFalse(pos) => {
                    let v = self.stack.pop().unwrap();
                    let is_false = match v {
                        Value::Bool(b) => !b,
                        Value::Int(n) => n == 0,
                        Value::Float(f) => f == 0.0,
                        _ => {
                            return Err(WqError::TypeError(
                                "Invalid type for conditionals or loops".into(),
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
                    let v = self.stack.pop().unwrap();
                    let ok = match v {
                        Value::Bool(b) => b,
                        Value::Int(n) => n != 0,
                        Value::Float(f) => f != 0.0,
                        _ => {
                            return Err(WqError::TypeError("Invalid type for assert".into()));
                        }
                    };
                    if !ok {
                        return Err(WqError::AssertionFailError("assertion failed".into()));
                    }
                    self.stack.push(Value::Null);
                }
                Instruction::Return => break,
            }
        }
        Ok(self.stack.pop().unwrap_or(Value::Null))
    }
}
