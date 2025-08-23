use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::WqResult;
use crate::value::{Value, WqError};
use crate::vm::Vm;
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;
use colored::Colorize;
use once_cell::sync::Lazy;
use rustyline::DefaultEditor;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
pub static RUSTYLINE: Lazy<Mutex<Option<DefaultEditor>>> = Lazy::new(|| Mutex::new(None));

pub trait ReplEngine {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError>;
    fn set_debug(&mut self, flag: bool);
    fn is_debug(&self) -> bool;
    fn get_environment(&self) -> Option<&std::collections::HashMap<String, Value>>;
    fn clear_environment(&mut self);
    fn env_vars(&self) -> &std::collections::HashMap<String, Value>;
}

impl ReplEngine for VmEvaluator {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError> {
        VmEvaluator::eval_string(self, input)
    }
    fn set_debug(&mut self, flag: bool) {
        VmEvaluator::set_debug(self, flag)
    }
    fn is_debug(&self) -> bool {
        VmEvaluator::is_debug(self)
    }
    fn get_environment(&self) -> Option<&std::collections::HashMap<String, Value>> {
        VmEvaluator::get_environment(self)
    }
    fn clear_environment(&mut self) {
        self.environment_mut().clear();
    }
    fn env_vars(&self) -> &std::collections::HashMap<String, Value> {
        self.environment()
    }
}

fn parse_load_filename(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("load ") {
        Some(rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix("\\l ") {
        Some(rest.trim())
    } else {
        None
    }
}

fn resolve_load_path(base: &Path, fname: &str) -> PathBuf {
    let path = Path::new(fname);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn expand_script(
    path: &Path,
    loading: &mut HashSet<PathBuf>,
    visited: &mut HashSet<PathBuf>,
) -> std::io::Result<String> {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !loading.insert(canonical.clone()) {
        // already in loading stack -> cycle
        println!("Cycle detected: {}", canonical.display());
        return Ok(String::new());
    }
    if visited.contains(&canonical) {
        loading.remove(&canonical);
        return Ok(String::new());
    }
    visited.insert(canonical.clone());
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            loading.remove(&canonical);
            return Err(e);
        }
    };
    let mut result = String::new();
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    for (i, line) in content.lines().enumerate() {
        if i == 0 && line.starts_with("#!") {
            continue;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(fname) = parse_load_filename(trimmed) {
            let sub = resolve_load_path(parent, fname);
            result.push_str(&expand_script(&sub, loading, visited)?);
        } else {
            result.push_str(trimmed);
            result.push('\n');
        }
    }
    loading.remove(&canonical);
    Ok(result)
}

pub fn vm_get_ins(path: &String, debug: bool) -> WqResult<Vec<Instruction>> {
    let path = Path::new(path);
    let mut loading = HashSet::new();
    let mut visited = HashSet::new();
    let src = match expand_script(path, &mut loading, &mut visited) {
        Ok(s) => s,
        Err(e) => {
            return Err(crate::value::WqError::RuntimeError(format!(
                "Cannot read {}: {}",
                path.display(),
                e
            )));
        }
    };
    let mut lexer = Lexer::new(&src);
    let tokens = lexer.tokenize()?;
    if debug {
        eprintln!("{tokens:?}");
        eprintln!();
    }
    use crate::resolver::Resolver;
    let mut parser = Parser::new(tokens, src.clone());
    let ast = parser.parse()?;
    if debug {
        eprintln!("{ast:?}");
        eprintln!();
    }
    let mut resolver = Resolver::new();
    let ast = resolver.resolve(ast);
    let mut compiler = Compiler::new();
    compiler.compile(&ast)?;
    compiler.fuse();
    Ok(compiler.instructions)
}

pub fn vm_exec_script(path: &String, debug: bool) {
    let ins = match vm_get_ins(path, debug) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return;
        }
    };
    if debug {
        eprintln!("{ins:?}");
        eprintln!();
    }
    let mut vm = Vm::new(ins);
    match vm.run() {
        Ok(_) => {
            //if val != Value::Null {
            // println!("{val}");
            //}
        }
        Err(e) => eprintln!("error: {e}"),
    }
}

pub struct VmEvaluator {
    vm: Vm,
    debug: bool,
}

impl Default for VmEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl VmEvaluator {
    /// Create a new evaluator with an empty environment.
    pub fn new() -> Self {
        VmEvaluator {
            vm: Vm::new(Vec::new()),
            debug: false,
        }
    }

    /// Enable or disable debug mode.
    pub fn set_debug(&mut self, flag: bool) {
        self.debug = flag;
    }

    /// Check if debug mode is active.
    pub fn is_debug(&self) -> bool {
        self.debug
    }

    /// Evaluate a string of source code and return the resulting value.
    pub fn eval_string(&mut self, input: &str) -> WqResult<Value> {
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        if self.debug {
            eprintln!("=====TOK=====");
            eprintln!("{tokens:?}");
        }
        use crate::resolver::Resolver;
        let mut parser = Parser::new(tokens, input.to_string());
        let ast = parser.parse()?;
        let mut resolver = Resolver::from_env(self.environment());
        let ast = resolver.resolve(ast);
        if self.debug {
            eprintln!("=====AST=====");
            eprintln!("{ast:?}");
        }
        let mut compiler = Compiler::new();
        compiler.compile(&ast)?;
        compiler.fuse();
        compiler.instructions.push(Instruction::Return);
        if self.debug {
            eprintln!("=====INST=====");
            for inst in &compiler.instructions {
                let s = format!("{inst:?}");
                if let Some((name, rest)) = s.split_once('(') {
                    eprintln!("{}({}", name.blue().bold(), rest);
                } else {
                    eprintln!("{}", s.blue().bold());
                }
            }
        }
        self.vm.reset(compiler.instructions);
        self.vm.run()
    }

    /// Access the environment holding user-defined bindings.
    pub fn environment(&self) -> &HashMap<String, Value> {
        self.vm.global_env()
    }

    /// Optionally get the environment if it contains any bindings.
    pub fn get_environment(&self) -> Option<&HashMap<String, Value>> {
        let env = self.vm.global_env();
        if env.is_empty() { None } else { Some(env) }
    }

    /// Mutable access to the environment.
    pub fn environment_mut(&mut self) -> &mut HashMap<String, Value> {
        self.vm.global_env_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::{Value, WqError};

    #[test]
    fn undefined_variable_errors() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("a");
        assert!(matches!(res, Err(WqError::ValueError(_))));
    }

    #[test]
    fn empty_conditional_branches_dont_panic() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("$[1;;]");
        assert!(res.is_ok());
    }

    #[test]
    fn empty_loop_body_dont_panic() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("N[3;]");
        assert!(res.is_ok());
    }

    #[test]
    fn break_and_continue() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("n:0;N[5;$[n=2;@c;];n:n+1;];n").unwrap();
        assert_eq!(res, Value::Int(2));
    }

    #[test]
    fn return_in_function() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("f:{@r 3;1};f[]").unwrap();
        assert_eq!(res, Value::Int(3));
    }

    #[test]
    fn assert_fails() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("@a 1=2;");
        assert!(matches!(res, Err(WqError::AssertionError(_))));
    }

    #[test]
    fn implicit_arg_order_and_arity() {
        let mut eval = VmEvaluator::new();
        // Test argument order with three implicit parameters
        let res = eval.eval_string("f:{100*x+10*y+z};f[1;2;3]").unwrap();
        assert_eq!(res, Value::Int(123));

        // Too many arguments should error
        let res = eval.eval_string("f[1;2;3;4]");
        assert!(matches!(res, Err(WqError::ArityError(_))));
    }

    #[test]
    fn arity_error_too_many_args() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("f:{[a;b;c]a+b+c};f[1;2;3;4]");
        assert!(matches!(res, Err(WqError::ArityError(_))));
    }

    #[test]
    fn intlist_literal_inferred_and_list_interop() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("(1;2;3)").unwrap();
        assert_eq!(res, Value::IntList(vec![1, 2, 3]));

        eval.eval_string("a:alloc 3").unwrap();
        eval.eval_string("b:(0;0;0)").unwrap();
        let sum = eval.eval_string("a+b").unwrap();
        assert_eq!(sum, Value::IntList(vec![0, 0, 0]));
        let cmp = eval.eval_string("a=b").unwrap();
        assert_eq!(
            cmp,
            Value::List(vec![
                Value::Bool(true),
                Value::Bool(true),
                Value::Bool(true)
            ])
        );
    }

    #[test]
    fn nested_function_calls_access_locals() {
        let mut eval = VmEvaluator::new();
        let code = "fib:{fib_:{[n;a;b]$[n=0;a;fib_[n-1;b;a+b]]};fib_[x;0;1]};fib 10";
        let res = eval.eval_string(code).unwrap();
        assert_eq!(res, Value::Int(55));
    }

    #[test]
    fn local_function_compiles_once_and_works_twice() {
        let mut eval = VmEvaluator::new();
        // Define a local function 'g' inside 'h' and call it twice
        let code = "h:{g:{[n]n+1}; g 1 + g 2}; h[]";
        let res = eval.eval_string(code).unwrap();
        assert_eq!(res, Value::Int(5));
    }

    #[test]
    fn builtin_arg_order_preserved() {
        let mut eval = VmEvaluator::new();
        // 'take' takes (list, n) and returns first n items
        let res = eval.eval_string("take[2;(1;2;3;4)]").unwrap();
        assert_eq!(res, Value::IntList(vec![1, 2]));
    }

    #[test]
    fn builtin_function_can_be_passed_and_called() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("a:{[x]x[]};a[rand]").unwrap();
        assert!(matches!(res, Value::Float(_)));
    }

    #[test]
    fn closure_captures_global_by_value() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("a:3;f:{a};a:4;f[]").unwrap();
        assert_eq!(res, Value::Int(3));
    }

    #[test]
    fn closure_captures_local_by_value() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("f:{a:4;f2:{a};a:5;f2};f[][]").unwrap();
        assert_eq!(res, Value::Int(4));
    }

    #[test]
    fn try_returns_status() {
        let mut eval = VmEvaluator::new();
        let ok = eval.eval_string("@t 1+2").unwrap();
        assert_eq!(ok, Value::List(vec![Value::Int(3), Value::Int(0)]));

        let err = eval.eval_string("@t 1+\"a\"").unwrap();
        if let Value::List(items) = err {
            assert_eq!(items.len(), 2);
            match &items[1] {
                Value::Int(code) => {
                    assert_eq!(*code, WqError::TypeError(String::new()).code() as i64);
                }
                _ => panic!("expected error code"),
            }
            assert!(items[0].to_string().contains("TYPE ERROR"));
        } else {
            panic!("expected list result");
        }
    }
    #[test]
    fn long_chain_of_negation_does_not_overflow() {
        let mut eval = VmEvaluator::new();
        let hyphens = "-".repeat(10000);
        let expr = format!("{hyphens}10");
        let res = eval.eval_string(&expr).unwrap();
        // 10000 is even, so the result remains positive
        assert_eq!(res, Value::Int(10));
    }

    #[test]
    fn passed_function_resolves_correctly() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("a:{2*x};b:{x[3]};b[a]").unwrap();
        assert_eq!(res, Value::Int(6));
        let res = eval.eval_string("a:iota 10;b[a]").unwrap();
        assert_eq!(res, Value::Int(3));
    }
}
