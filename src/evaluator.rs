use crate::builtins::Builtins;
use crate::parser::{AstNode, BinaryOperator, UnaryOperator};
use crate::value::{Value, WqError, WqResult};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Environment {
    variables: HashMap<String, Value>,
}

#[derive(Debug, Clone)]
struct StackFrame {
    variables: HashMap<String, Value>,
}

#[derive(Debug)]
struct CallStack {
    frames: Vec<StackFrame>,
    max_depth: usize,
}

impl CallStack {
    fn new() -> Self {
        CallStack {
            frames: Vec::new(),
            max_depth: 9999,
        }
    }

    fn push_frame(&mut self, frame: StackFrame) -> WqResult<()> {
        if self.frames.len() >= self.max_depth {
            // Reuse the most recent frame
            let last_idx = self.frames.len() - 1;
            self.frames[last_idx] = frame;
        } else {
            self.frames.push(frame);
        }
        Ok(())
    }

    fn pop_frame(&mut self) -> Option<StackFrame> {
        self.frames.pop()
    }

    fn current_frame_mut(&mut self) -> Option<&mut StackFrame> {
        self.frames.last_mut()
    }

    fn current_frame(&self) -> Option<&StackFrame> {
        self.frames.last()
    }

    /// Look up a variable by searching from the most recent frame downward
    fn lookup(&self, name: &str) -> Option<Value> {
        for frame in self.frames.iter().rev() {
            if let Some(val) = frame.variables.get(name) {
                return Some(val.clone());
            }
        }
        None
    }

    /// Assign a variable to the nearest existing frame containing it. Returns
    /// true if an existing binding was updated.
    fn assign(&mut self, name: &str, value: Value) -> bool {
        for frame in self.frames.iter_mut().rev() {
            if frame.variables.contains_key(name) {
                frame.variables.insert(name.to_string(), value);
                return true;
            }
        }
        false
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

impl Environment {
    pub fn new() -> Self {
        Environment {
            variables: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    pub fn set(&mut self, name: String, value: Value) {
        self.variables.insert(name, value);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    pub fn clear(&mut self) {
        self.variables.clear();
    }

    pub fn variables(&self) -> &HashMap<String, Value> {
        &self.variables
    }
}

pub struct Evaluator {
    environment: Environment,
    builtins: Builtins,
    call_stack: CallStack,
}

impl Evaluator {
    pub fn new() -> Self {
        Evaluator {
            environment: Environment::new(),
            builtins: Builtins::new(),
            call_stack: CallStack::new(),
        }
    }

    pub fn environment(&self) -> &Environment {
        &self.environment
    }

    pub fn environment_mut(&mut self) -> &mut Environment {
        &mut self.environment
    }

    /// Evaluate an AST node
    pub fn eval(&mut self, node: &AstNode) -> WqResult<Value> {
        match node {
            AstNode::Literal(value) => Ok(value.clone()),

            AstNode::Variable(name) => {
                if let Some(val) = self.call_stack.lookup(name) {
                    return Ok(val);
                }
                if let Some(value) = self.environment.get(name) {
                    Ok(value.clone())
                } else {
                    Err(WqError::DomainError(format!("Undefined variable: {name}")))
                }
            }

            AstNode::BinaryOp {
                left,
                operator,
                right,
            } => {
                let left_val = self.eval(left)?;
                let right_val = self.eval(right)?;
                self.eval_binary_op(&left_val, operator, &right_val)
            }

            AstNode::Conditional {
                condition,
                true_branch,
                false_branch,
            } => {
                let cond_val = self.eval(condition)?;
                let is_true = match cond_val {
                    Value::Bool(b) => b,
                    Value::Int(n) => n != 0,
                    Value::Float(f) => f != 0.0,
                    _ => false,
                };

                if is_true {
                    self.eval(true_branch)
                } else {
                    self.eval(false_branch)
                }
            }

            AstNode::UnaryOp { operator, operand } => {
                let operand_val = self.eval(operand)?;
                self.eval_unary_op(operator, &operand_val)
            }

            AstNode::Assignment { name, value } => {
                let val = self.eval(value)?;
                if !self.call_stack.assign(name, val.clone()) {
                    if let Some(frame) = self.call_stack.current_frame_mut() {
                        frame.variables.insert(name.clone(), val.clone());
                    } else {
                        self.environment.set(name.clone(), val.clone());
                    }
                }
                Ok(val)
            }

            AstNode::List(elements) => {
                let mut values = Vec::new();
                for element in elements {
                    values.push(self.eval(element)?);
                }
                Ok(Value::List(values))
            }

            AstNode::Dict(pairs) => {
                let mut map = HashMap::new();
                for (key, value_node) in pairs {
                    let value = self.eval(value_node)?;
                    map.insert(key.clone(), value);
                }
                Ok(Value::Dict(map))
            }

            AstNode::Call { name, args } => {
                let mut arg_values = Vec::new();
                for arg in args {
                    arg_values.push(self.eval(arg)?);
                }

                // Try built-in functions first
                match self.builtins.call(name, &arg_values) {
                    Ok(result) => Ok(result),
                    Err(WqError::FnArgCountMismatchError(msg)) => {
                        Err(WqError::FnArgCountMismatchError(msg))
                    }
                    Err(WqError::TypeError(msg)) => Err(WqError::TypeError(msg)),
                    Err(WqError::RuntimeError(msg)) => Err(WqError::RuntimeError(msg)),
                    Err(WqError::DomainError(_)) => {
                        // Check if it's a user-defined function in stack and global env
                        let function = self
                            .call_stack
                            .lookup(name)
                            .or_else(|| self.environment.get(name).cloned());

                        match function {
                            Some(Value::Function { params, body }) => {
                                self.call_user_function(params, &body, &arg_values)
                            }
                            _ => Err(WqError::DomainError(format!(
                                "Unknown user-defined function: {name}"
                            ))),
                        }
                    }
                    _ => Err(WqError::DomainError("Unknown error in eval".to_string())),
                }
            }

            AstNode::CallAnonymous { object, args } => {
                let obj_val = self.eval(object)?;
                let mut arg_values = Vec::new();
                for arg in args {
                    arg_values.push(self.eval(arg)?);
                }

                if let Value::Function { params, body } = obj_val {
                    self.call_user_function(params, &body, &arg_values)
                } else {
                    Err(WqError::DomainError(
                        "Failed calling anonymous function".to_string(),
                    ))
                }
            }

            AstNode::Function { params, body } => Ok(Value::Function {
                params: params.clone(),
                body: body.clone(),
            }),

            AstNode::Index { object, index } => {
                let obj_val = self.eval(object)?;
                let idx_val = self.eval(index)?;

                obj_val
                    .index(&idx_val)
                    .ok_or_else(|| WqError::IndexError("Invalid index operation".to_string()))
            }

            AstNode::Block(statements) => {
                let mut result = Value::Null;
                for stmt in statements {
                    result = self.eval(stmt)?;
                }
                Ok(result)
            }
        }
    }

    fn eval_binary_op(
        &self,
        left: &Value,
        operator: &BinaryOperator,
        right: &Value,
    ) -> WqResult<Value> {
        match operator {
            BinaryOperator::Add => left
                .add(right)
                .ok_or_else(|| WqError::TypeError("Cannot add these types".to_string())),
            BinaryOperator::Subtract => left
                .subtract(right)
                .ok_or_else(|| WqError::TypeError("Cannot subtract these types".to_string())),
            BinaryOperator::Multiply => left
                .multiply(right)
                .ok_or_else(|| WqError::TypeError("Cannot multiply these types".to_string())),
            BinaryOperator::Divide => left.divide(right).ok_or_else(|| {
                WqError::DomainError("Division by zero or invalid types".to_string())
            }),
            BinaryOperator::Modulo => match (left, right) {
                (Value::Int(a), Value::Int(b)) => {
                    if *b == 0 {
                        Err(WqError::DomainError("Modulo by zero".to_string()))
                    } else {
                        Ok(Value::Int(a % b))
                    }
                }
                _ => Err(WqError::TypeError("Modulo expects integers".to_string())),
            },
            BinaryOperator::Equal => Ok(left.equals(right)),
            BinaryOperator::NotEqual => Ok(left.not_equals(right)),
            BinaryOperator::LessThan => Ok(left.less_than(right)),
            BinaryOperator::LessThanOrEqual => Ok(left.less_than_or_equal(right)),
            BinaryOperator::GreaterThan => Ok(left.greater_than(right)),
            BinaryOperator::GreaterThanOrEqual => Ok(left.greater_than_or_equal(right)),
        }
    }

    fn eval_unary_op(&self, operator: &UnaryOperator, operand: &Value) -> WqResult<Value> {
        match operator {
            UnaryOperator::Negate => match operand {
                Value::Int(n) => Ok(Value::Int(-n)),
                Value::Float(f) => Ok(Value::Float(-f)),
                _ => Err(WqError::TypeError("Cannot negate this type".to_string())),
            },
            UnaryOperator::Count => Ok(Value::Int(operand.len() as i64)),
        }
    }

    /// Call a user-defined function
    fn call_user_function(
        &mut self,
        params: Option<Vec<String>>,
        body: &AstNode,
        args: &[Value],
    ) -> WqResult<Value> {
        let mut frame = StackFrame {
            variables: HashMap::new(),
        };

        // Bind arguments to parameters
        match params {
            Some(param_names) => {
                if args.len() != param_names.len() {
                    return Err(WqError::FnArgCountMismatchError(format!(
                        "Function expects {} arguments, got {}",
                        param_names.len(),
                        args.len()
                    )));
                }
                for (param, arg) in param_names.iter().zip(args.iter()) {
                    frame.variables.insert(param.clone(), arg.clone());
                }
            }
            None => {
                // Implicit parameters: x y
                match args.len() {
                    0 => {}
                    1 => {
                        frame.variables.insert("x".to_string(), args[0].clone());
                    }
                    2 => {
                        frame.variables.insert("x".to_string(), args[0].clone());
                        frame.variables.insert("y".to_string(), args[1].clone());
                    }
                    _ => {
                        return Err(WqError::FnArgCountMismatchError(
                            "Implicit function expects 0, 1 or 2 arguments".to_string(),
                        ));
                    }
                }
            }
        }

        // Push frame and evaluate body
        self.call_stack.push_frame(frame)?;
        let result = self.eval(body);
        self.call_stack.pop_frame();

        result
    }

    /// Evaluate a string of wq code
    pub fn eval_string(&mut self, input: &str) -> WqResult<Value> {
        use crate::lexer::Lexer;
        use crate::parser::Parser;

        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize();
        let mut parser = Parser::new(tokens);
        let ast = parser.parse()?;
        self.eval(&ast)
    }

    /// Print current environment state
    pub fn show_environment(&self) {
        if self.environment.variables().is_empty() {
            println!("no user-defined bindings");
        } else {
            println!("user-defined bindings:");
            for (name, value) in self.environment.variables() {
                println!("  {name} = {value}");
            }
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_literal() {
        let mut evaluator = Evaluator::new();
        let result = evaluator.eval_string("42").unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_eval_arithmetic() {
        let mut evaluator = Evaluator::new();

        assert_eq!(evaluator.eval_string("1+2").unwrap(), Value::Int(3));
        assert_eq!(evaluator.eval_string("10-3").unwrap(), Value::Int(7));
        assert_eq!(evaluator.eval_string("4*5").unwrap(), Value::Int(20));
        assert_eq!(evaluator.eval_string("15/3").unwrap(), Value::Int(5));
    }

    #[test]
    fn test_eval_assignment() {
        let mut evaluator = Evaluator::new();

        evaluator.eval_string("x:42").unwrap();
        assert_eq!(evaluator.eval_string("x").unwrap(), Value::Int(42));

        evaluator.eval_string("y:x+8").unwrap();
        assert_eq!(evaluator.eval_string("y").unwrap(), Value::Int(50));
    }

    #[test]
    fn test_eval_lists() {
        let mut evaluator = Evaluator::new();

        let result = evaluator.eval_string("(1;2;3)").unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );

        evaluator.eval_string("lst:(10;20;30)").unwrap();
        assert_eq!(evaluator.eval_string("lst[0]").unwrap(), Value::Int(10));
        assert_eq!(evaluator.eval_string("lst[2]").unwrap(), Value::Int(30));
    }

    #[test]
    fn test_eval_functions() {
        let mut evaluator = Evaluator::new();
        evaluator.eval_string("nums:(1;2;3;4;5)").unwrap();

        assert_eq!(evaluator.eval_string("count nums").unwrap(), Value::Int(5));

        assert_eq!(evaluator.eval_string("first nums").unwrap(), Value::Int(1));
        assert_eq!(evaluator.eval_string("last nums").unwrap(), Value::Int(5));

        assert_eq!(evaluator.eval_string("sum nums").unwrap(), Value::Int(15));
        let til_result = evaluator.eval_string("til 3").unwrap();
        assert_eq!(
            til_result,
            Value::List(vec![Value::Int(0), Value::Int(1), Value::Int(2)])
        );
    }

    #[test]
    fn test_operator_precedence() {
        let mut evaluator = Evaluator::new();
        assert_eq!(evaluator.eval_string("1+2*3").unwrap(), Value::Int(7));
        assert_eq!(evaluator.eval_string("(1+2)*3").unwrap(), Value::Int(9));
    }

    #[test]
    fn test_list_arithmetic() {
        let mut evaluator = Evaluator::new();

        let result = evaluator.eval_string("(1;2;3)+(4;5;6)").unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Int(5), Value::Int(7), Value::Int(9)])
        );

        // Test scalar-vector operations
        let result = evaluator.eval_string("(2;4;6)*3").unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Int(6), Value::Int(12), Value::Int(18)])
        );

        // Test vector cycling
        let result = evaluator.eval_string("til 3 + til 2").unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Int(0), Value::Int(2), Value::Int(2)])
        );
    }

    #[test]
    fn test_function_definitions() {
        let mut evaluator = Evaluator::new();

        // Test implicit parameter function
        evaluator.eval_string("f:{x+y}").unwrap();
        let result = evaluator.eval_string("f[3;4;]").unwrap();
        assert_eq!(result, Value::Int(7));

        // Test explicit parameter function
        evaluator.eval_string("g:{[a;b]a*b}").unwrap();
        let result = evaluator.eval_string("g[5;6;]").unwrap();
        assert_eq!(result, Value::Int(30));

        // Test single parameter function
        evaluator.eval_string("square:{x*x}").unwrap();
        let result = evaluator.eval_string("square[7;]").unwrap();
        assert_eq!(result, Value::Int(49));
    }

    #[test]
    fn test_function_with_vectors() {
        let mut evaluator = Evaluator::new();

        // Test function with vector arguments
        evaluator.eval_string("double:{x*2}").unwrap();
        let result = evaluator.eval_string("double[til 5;]").unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::Int(0),
                Value::Int(2),
                Value::Int(4),
                Value::Int(6),
                Value::Int(8)
            ])
        );

        evaluator.eval_string("add:{[a;b]a+b}").unwrap();
        let result = evaluator.eval_string("add[til 3;til 3;]").unwrap();
        assert_eq!(
            result,
            Value::List(vec![Value::Int(0), Value::Int(2), Value::Int(4)])
        );
    }

    #[test]
    fn test_function_scoping() {
        let mut evaluator = Evaluator::new();

        // Test function parameters don't affect global scope
        evaluator.eval_string("x:100").unwrap();
        evaluator.eval_string("f:{x+1}").unwrap();
        let result = evaluator.eval_string("f[5;]").unwrap();
        assert_eq!(result, Value::Int(6));

        // Global x should be unchanged
        let result = evaluator.eval_string("x").unwrap();
        assert_eq!(result, Value::Int(100));

        // Test explicit parameters shadow global variables
        evaluator.eval_string("y:200").unwrap();
        evaluator.eval_string("g:{[y]y*2}").unwrap();
        let result = evaluator.eval_string("g[10;]").unwrap();
        assert_eq!(result, Value::Int(20));

        // Global y should be unchanged
        let result = evaluator.eval_string("y").unwrap();
        assert_eq!(result, Value::Int(200));
    }

    #[test]
    fn test_naive_fibonacci() {
        let mut evaluator = Evaluator::new();

        evaluator
            .eval_string("fib:{[n]$[n<2;n;fib[n-1;]+fib[n-2;]]}")
            .unwrap();

        assert_eq!(evaluator.eval_string("fib[0;]").unwrap(), Value::Int(0));
        assert_eq!(evaluator.eval_string("fib[1;]").unwrap(), Value::Int(1));
        assert_eq!(evaluator.eval_string("fib[2;]").unwrap(), Value::Int(1));
    }

    #[test]
    fn test_tail_recursive_fibonacci() {
        let mut evaluator = Evaluator::new();

        // accumulator pattern
        evaluator
            .eval_string("fibtr:{[n;a;b]$[n=0;a;fibtr[n-1;b;a+b;]]}")
            .unwrap();
        evaluator.eval_string("fib:{[n]fibtr[n;0;1;]}").unwrap();

        assert_eq!(evaluator.eval_string("fib[0;]").unwrap(), Value::Int(0));
        assert_eq!(evaluator.eval_string("fib[1;]").unwrap(), Value::Int(1));
        assert_eq!(evaluator.eval_string("fib[2;]").unwrap(), Value::Int(1));
        assert_eq!(evaluator.eval_string("fib[3;]").unwrap(), Value::Int(2));
        assert_eq!(evaluator.eval_string("fib[4;]").unwrap(), Value::Int(3));
        assert_eq!(evaluator.eval_string("fib[5;]").unwrap(), Value::Int(5));
        assert_eq!(evaluator.eval_string("fib[6;]").unwrap(), Value::Int(8));
    }

    #[test]
    fn test_assignment_inside_function() {
        let mut evaluator = Evaluator::new();
        evaluator.eval_string("add7:{k:7;x+k}").unwrap();
        let result = evaluator.eval_string("add7[5;]").unwrap();
        assert_eq!(result, Value::Int(12));

        // variable k should not leak to global scope
        let err = evaluator.eval_string("k");
        assert!(err.is_err());
    }

    #[test]
    fn test_prime_function() {
        let mut evaluator = Evaluator::new();
        evaluator
            .eval_string("isprime:{[x;i]$[i*i>x;1;$[x%i=0;0;isprime[x;i+1;]]]}")
            .unwrap();
        evaluator.eval_string(
            "primes:{[n]res:();iter:{[i]$[i>n;res;$[isprime[i;2;];res:res,i;0];iter[i+1;]]};iter[2;]}"
        ).unwrap();
        let result = evaluator.eval_string("primes[10;]").unwrap();
        assert_eq!(
            result,
            Value::List(vec![
                Value::Int(2),
                Value::Int(3),
                Value::Int(5),
                Value::Int(7)
            ])
        );
    }
}
