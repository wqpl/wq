mod compiler;
mod fastpath;
pub mod instruction;
mod vmi;

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::valuei::{Value, WqResult};
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use vmi::Vm;

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
) -> String {
    let canonical = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if !loading.insert(canonical.clone()) {
        // already in loading stack -> cycle
        println!("Cycle detected: {}", canonical.display());
        return String::new();
    }
    if visited.contains(&canonical) {
        loading.remove(&canonical);
        return String::new();
    }
    visited.insert(canonical.clone());
    let content = fs::read_to_string(path).expect("cannot read script");
    let mut result = String::new();
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") {
            continue;
        }
        if let Some(fname) = parse_load_filename(trimmed) {
            let sub = resolve_load_path(parent, fname);
            result.push_str(&expand_script(&sub, loading, visited));
        } else {
            result.push_str(trimmed);
            result.push('\n');
        }
    }
    loading.remove(&canonical);
    result
}

pub fn vm_get_ins(path: &String) -> WqResult<Vec<Instruction>> {
    let path = Path::new(path);
    let mut loading = HashSet::new();
    let mut visited = HashSet::new();
    let src = expand_script(path, &mut loading, &mut visited);
    let mut lexer = Lexer::new(&src);
    let tokens = lexer.tokenize()?;
    use crate::resolver::Resolver;
    let mut parser = Parser::new(tokens, src.clone());
    let ast = parser.parse().expect("parse error");
    let mut resolver = Resolver::new();
    let ast = resolver.resolve(ast);
    let mut compiler = Compiler::new();
    compiler.compile(&ast)?;
    Ok(compiler.instructions)
}

pub fn vm_exec_script(path: &String, debug: bool) {
    let ins = match vm_get_ins(path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            return;
        }
    };
    if debug {
        eprintln!("{ins:#?}");
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

/// A stateful evaluator for the bytecode VM used by the REPL.
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
            eprintln!("=====TOKENS=====");
            eprintln!("{tokens:#?}");
        }
        use crate::resolver::Resolver;
        let mut parser = Parser::new(tokens, input.to_string());
        let ast = parser.parse()?;
        let mut resolver = Resolver::from_env(self.environment());
        let ast = resolver.resolve(ast);
        if self.debug {
            eprintln!("=====AST=====");
            eprintln!("{ast:#?}");
        }
        let mut compiler = Compiler::new();
        compiler.compile(&ast)?;
        compiler.instructions.push(Instruction::Return);
        if self.debug {
            eprintln!("=====BC=====");
            eprintln!("{:#?}", compiler.instructions);
        }
        self.vm.reset(compiler.instructions);
        self.vm.run_no_fastpath()
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
    use crate::value::valuei::{Value, WqError};

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
}
