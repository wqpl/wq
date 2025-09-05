pub mod repl_engine;
pub mod stdio;
pub mod wqdb_shell;

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::Value;
use crate::value::WqResult;
use crate::vm::Vm;
use crate::vm::compiler::Compiler;
use crate::vm::instruction::Instruction;
use crate::wqdb::apply_stmt_spans_exact_offs;
use crate::wqdb::mark_stmt_heuristic;
use crate::wqdb::register_function_chunks;
use crate::wqdb::{self, DebugHost};
use crate::wqerror::WqError;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU8, Ordering};

#[cfg(not(target_arch = "wasm32"))]
use colored::Colorize;
use repl_engine::ReplEngine;
use stdio::ReplStdin;
use stdio::set_stdin;
use stdio::stderr_println;

// Global verbose level for debug logging across modules (0=off, 1=inst, 2=inst+ast+debug logs, 3=+tokens)
static DEBUG_LEVEL: AtomicU8 = AtomicU8::new(0);
pub fn set_debug_level(level: u8) {
    DEBUG_LEVEL.store(level, Ordering::Relaxed);
}
pub fn get_debug_level() -> u8 {
    DEBUG_LEVEL.load(Ordering::Relaxed)
}

pub struct VmEvaluator {
    vm: Vm,
    debug_level: u8,
    // Arm entering the wqdb on the next eval call
    wqdb_arm_next: bool,
    // Optional debug source context for next eval (path, full_text)
    dbg_source_ctx: Option<(String, String)>,
    // Byte offset into dbg_source_ctx where current snippet starts
    dbg_source_offs: usize,
    // Backtrace mode (minimal debug mapping for errors)
    bt_mode: bool,
}

impl Default for VmEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl ReplEngine for VmEvaluator {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError> {
        VmEvaluator::eval_string(self, input)
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

    fn set_stdin(&mut self, stdin: Box<dyn ReplStdin>) {
        set_stdin(stdin);
    }
    fn arm_wqdb_next(&mut self) {
        VmEvaluator::arm_wqdb_next(self)
    }
    fn dbg_set_source(&mut self, path: &str, full_text: &str) {
        VmEvaluator::dbg_set_source(self, path, full_text)
    }
    fn dbg_set_offset(&mut self, offset: usize) {
        VmEvaluator::dbg_set_offset(self, offset)
    }
    fn dbg_print_bt(&mut self) {
        VmEvaluator::dbg_print_bt(self)
    }
    fn set_bt_mode(&mut self, flag: bool) {
        VmEvaluator::set_bt_mode(self, flag)
    }
    fn set_wqdb(&mut self, flag: bool) {
        VmEvaluator::set_wqdb(self, flag)
    }
    fn set_debug_level(&mut self, level: u8) {
        VmEvaluator::set_debug_level(self, level)
    }
    fn get_debug_level(&mut self) -> u8 {
        VmEvaluator::get_debug_level(self)
    }
    fn is_wqdb_on(&self) -> bool {
        self.vm.wqdb.enabled
    }
    fn reset_session(&mut self) {
        VmEvaluator::reset_session(self)
    }
}

impl VmEvaluator {
    /// Create a new evaluator with an empty environment.
    pub fn new() -> Self {
        let mut vm = Vm::new(Vec::new());
        vm.set_bt_mode(true);
        VmEvaluator {
            vm,
            debug_level: 0,
            wqdb_arm_next: false,
            dbg_source_ctx: None,
            dbg_source_offs: 0,
            bt_mode: true,
        }
    }
    pub fn set_debug_level(&mut self, level: u8) {
        self.debug_level = level;
        set_debug_level(level);
    }
    pub fn get_debug_level(&mut self) -> u8 {
        self.debug_level
    }
    pub fn set_bt_mode(&mut self, flag: bool) {
        self.bt_mode = flag;
        self.vm.set_bt_mode(flag);
    }
    pub fn set_wqdb(&mut self, flag: bool) {
        self.vm.wqdb.enabled = flag;
        if self.vm.wqdb.enabled {
            self.vm.wqdb.on_pause = Some(repl_on_pause);
            self.wqdb_arm_next = true;
        } else {
            self.vm.wqdb.clear_mode();
            self.vm.wqdb.on_pause = None;
        }
    }

    /// Evaluate a string of source code and return the resulting value.
    pub fn eval_string(&mut self, input: &str) -> WqResult<Value> {
        // If a wqdb entry was armed, record it for the upcoming run.
        let _enter_wqdb = if self.wqdb_arm_next {
            self.wqdb_arm_next = false;
            true
        } else {
            false
        };
        let mut lexer = Lexer::new(input);
        let tokens = lexer.tokenize()?;
        #[cfg(not(target_arch = "wasm32"))]
        if self.debug_level >= 3 {
            stderr_println("~ Tokens ~".red().underline().to_string().as_str());
            stderr_println(format!("{tokens:?}").as_str());
        }
        #[cfg(target_arch = "wasm32")]
        if self.debug_level >= 3 {
            stderr_println("~ Tokens ~");
            stderr_println(format!("{tokens:?}").as_str());
        }

        use crate::post_parser::folder;
        use crate::post_parser::resolver::Resolver;
        let mut parser = Parser::new(tokens, input.to_string());
        let ast = parser.parse()?;
        let mut resolver = Resolver::from_env(self.environment());
        let ast = resolver.resolve(ast);
        let ast = folder::fold(ast);

        #[cfg(not(target_arch = "wasm32"))]
        if self.debug_level >= 2 {
            stderr_println("~ AST ~".yellow().underline().to_string().as_str());
            // stderr_println(format!("{ast:?}").as_str());
            stderr_println(format!("{ast}").as_str());
        }
        #[cfg(target_arch = "wasm32")]
        if self.debug_level >= 2 {
            stderr_println("~ AST ~");
            stderr_println(format!("{ast}").as_str());
        }

        let mut compiler = Compiler::new();
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.compile(&ast)?;
        compiler.fuse();
        compiler.instructions.push(Instruction::Return);

        #[cfg(not(target_arch = "wasm32"))]
        if self.debug_level >= 1 {
            stderr_println("~ Inst ~".green().underline().to_string().as_str());
            for inst in &compiler.instructions {
                let s = format!("{inst:?}");
                if let Some((name, rest)) = s.split_once('(') {
                    stderr_println(format!("{}({rest}", name.green()).as_str());
                } else {
                    stderr_println(format!("{}", s.green()).as_str());
                }
            }
        }
        #[cfg(target_arch = "wasm32")]
        if self.debug_level >= 1 {
            stderr_println("~ Inst ~");
            for inst in &compiler.instructions {
                let s = format!("{inst:?}");
                if let Some((name, rest)) = s.split_once('(') {
                    stderr_println(format!("{name}({rest}").as_str());
                } else {
                    stderr_println(s.as_str());
                }
            }
        }

        self.vm.clear_last_bt();

        self.vm.reset(compiler.instructions);
        // Prepare debug artifacts when wqdb or backtrace mode is on
        if self.vm.wqdb.enabled || _enter_wqdb || self.bt_mode {
            // Prepare debug mapping for this top-level script
            let (src_path, src_text) = if let Some((p, t)) = self.dbg_source_ctx.as_ref() {
                (p.clone(), t.clone())
            } else {
                ("<repl>".to_string(), input.to_string())
            };
            self.vm.debug_prepare_script(&src_path, &src_text);
            // Set base offset into the source file for this snippet
            self.vm.set_debug_src_offset(self.dbg_source_offs);
            // Mark statements using a combination of parser spans and heuristics
            {
                let chunk = self.vm.current_chunk_id();
                let code = &self.vm.instructions;
                // Compute file_id first to avoid borrow conflicts
                let file_id = self.vm.debug_info.chunk(chunk).file_id;
                // First mark all likely statement PCs
                let line_table = &mut self.vm.debug_info.chunk_mut(chunk).line_table;
                mark_stmt_heuristic(line_table, code);
                // Overlay exact mapping for top-level spans across candidates
                apply_stmt_spans_exact_offs(
                    line_table,
                    code,
                    file_id,
                    parser.stmt_spans_top(),
                    self.dbg_source_offs,
                );
                // Recursively register chunks for nested non-capturing functions
                register_function_chunks(
                    &mut self.vm.debug_info,
                    file_id,
                    code,
                    self.dbg_source_offs,
                );
            }
        }
        // If wqdb is enabled (persistently or armed just once), attach hook and step-in
        if self.vm.wqdb.enabled || _enter_wqdb {
            if self.vm.wqdb.on_pause.is_none() {
                self.vm.wqdb.on_pause = Some(repl_on_pause);
            }
            self.vm.dbg_step_in();
        }
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

    /// Arm wqdb for the next eval.
    pub fn arm_wqdb_next(&mut self) {
        self.wqdb_arm_next = true;
    }
    pub fn dbg_set_source(&mut self, path: &str, full_text: &str) {
        self.dbg_source_ctx = Some((path.to_string(), full_text.to_string()));
    }
    pub fn dbg_set_offset(&mut self, offset: usize) {
        self.dbg_source_offs = offset;
    }

    pub fn dbg_print_bt(&mut self) {
        // try captured (innermost) first; else fall back to asking live VM
        let frames = self
            .vm
            .take_last_bt()
            .unwrap_or_else(|| <Vm as DebugHost>::bt_frames(&self.vm));
        let di = &self.vm.debug_info;
        for (loc, name) in frames {
            eprint!("{}", wqdb::format_frame(di, loc, &name));
        }
    }

    /// Reset REPL session state: clear environment and virtual debug sources
    pub fn reset_session(&mut self) {
        self.environment_mut().clear();
        self.vm.debug_info = wqdb::DebugInfo::default();
        self.vm.wqdb = wqdb::Wqdb::default();
    }
}

/// REPL pause hook: run the interactive wqdb shell
fn repl_on_pause(host: &mut dyn DebugHost) {
    wqdb_shell::wqdb_shell(host);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn undefined_variable_errors() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("a");
        assert!(matches!(res, Err(WqError::ValueError(_))));
    }

    #[test]
    fn empty_conditional_branches_dont_panic() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("$[true;;]");
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

        // Too many args should error
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
        let res = eval.eval_string("rg[2;4]").unwrap();
        assert_eq!(res, Value::IntList(vec![2, 3]));
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
                    assert_eq!(*code, WqError::DomainError(String::new()).code() as i64);
                }
                _ => panic!("expected error code"),
            }
            assert!(items[0].to_string().contains("DOMAIN ERROR"));
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

    #[test]
    fn recursive_function_with_postfix() {
        let mut eval = VmEvaluator::new();
        let res = eval.eval_string("a:{[n]$[n<4;a[n+1];n]};a 0").unwrap();
        assert_eq!(res, Value::Int(4));
    }
}
