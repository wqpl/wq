use crate::{
    apps::evaluator::Evaluator,
    builtins::BuiltinPreset,
    colored::Colorize,
    compiler::Compiler,
    debug_flags::{DebugFlags, set_debug_flags},
    interpreters::{
        InterpreterKind, default::DefaultInterpreter, profiler::ProfilerInterpreter,
        sample::SampleInterpreter,
    },
    lexer::Lexer,
    parser::Parser,
    post_parser::{folder, resolver::Resolver},
    stdio::{WqStdin, set_stdin, stderr_println},
    token::fmt_tokens_table,
    value::{Value, WqResult},
    vm::{
        GlobalMap, Vm,
        instruction::{InstPrettyDumper, Instruction},
    },
    wqdb::{
        self, DebugHost, apply_stmt_spans_exact_offs, mark_stmt_heuristic, register_function_chunks,
    },
    wqerror::WqError,
};

pub struct DefaultEvaluator {
    vm: Vm,
    debug_flags: DebugFlags,
    // Arm entering the wqdb on the next eval call
    wqdb_arm_next: bool,
    // Optional debug source context for next eval (path, full_text)
    dbg_source_ctx: Option<(String, String)>,
    // Byte offset into dbg_source_ctx where current snippet starts
    dbg_source_offs: usize,
    // Backtrace mode (minimal debug mapping for errors)
    bt_mode: bool,
    interpreter: InterpreterKind,
}

impl Evaluator for DefaultEvaluator {
    fn eval_string(&mut self, input: &str) -> Result<Value, WqError> {
        DefaultEvaluator::eval_string(self, input)
    }

    fn get_environment(&self) -> Option<&GlobalMap> {
        DefaultEvaluator::get_environment(self)
    }

    fn clear_environment(&mut self) {
        self.environment_mut().clear();
    }

    fn env_vars(&self) -> &GlobalMap {
        self.environment()
    }

    fn set_stdin(&mut self, stdin: Box<dyn WqStdin>) {
        set_stdin(stdin);
    }

    fn arm_wqdb_next(&mut self) {
        DefaultEvaluator::arm_wqdb_next(self)
    }

    fn dbg_set_source(&mut self, path: &str, full_text: &str) {
        DefaultEvaluator::dbg_set_source(self, path, full_text)
    }

    fn dbg_set_offset(&mut self, offset: usize) {
        DefaultEvaluator::dbg_set_offset(self, offset)
    }

    fn dbg_print_bt(&mut self) {
        DefaultEvaluator::dbg_print_bt(self)
    }

    fn set_bt_mode(&mut self, flag: bool) {
        DefaultEvaluator::set_bt_mode(self, flag)
    }

    fn set_wqdb(&mut self, flag: bool) {
        DefaultEvaluator::set_wqdb(self, flag)
    }

    fn set_debug_flags(&mut self, flags: DebugFlags) {
        DefaultEvaluator::set_debug_flags(self, flags)
    }

    fn get_debug_flags(&mut self) -> DebugFlags {
        DefaultEvaluator::get_debug_flags(self)
    }

    fn is_wqdb_enabled(&self) -> bool {
        self.vm.wqdb.enabled
    }

    fn reset_session(&mut self) {
        DefaultEvaluator::reset_session(self)
    }
}

impl DefaultEvaluator {
    /// Create a new evaluator with an empty environment.
    pub fn new() -> Self {
        let mut vm = Vm::new(Vec::new());
        vm.set_bt_mode(true);
        DefaultEvaluator {
            vm,
            debug_flags: DebugFlags::empty(),
            wqdb_arm_next: false,
            dbg_source_ctx: None,
            dbg_source_offs: 0,
            bt_mode: true,
            interpreter: InterpreterKind::Default,
        }
    }

    pub fn set_interpreter(&mut self, kind: InterpreterKind) {
        self.interpreter = kind;
    }

    pub fn set_interpreter_by_name(&mut self, name: &str) -> Result<&'static str, String> {
        if let Some(kind) = InterpreterKind::from_name(name) {
            self.set_interpreter(kind);
            Ok(kind.name())
        } else {
            Err(format!("unknown interpreter '{name}'"))
        }
    }

    pub fn interpreter_name(&self) -> &'static str {
        self.interpreter.name()
    }

    pub fn get_debug_flags(&self) -> DebugFlags {
        self.debug_flags
    }

    pub fn set_debug_flags(&mut self, flags: DebugFlags) {
        self.debug_flags = flags;
        set_debug_flags(flags);
    }

    pub fn get_bt_mode(&self) -> bool {
        self.bt_mode
    }

    pub fn set_bt_mode(&mut self, flag: bool) {
        self.bt_mode = flag;
        self.vm.set_bt_mode(flag);
    }

    pub fn set_wqdb(&mut self, flag: bool) {
        self.vm.wqdb.enabled = flag;
        if self.vm.wqdb.enabled {
            self.wqdb_arm_next = true;
        } else {
            self.vm.wqdb.clear_mode();
            // Don't clear on_pause - keep the callback registered for re-enabling
        }
    }

    pub fn set_pause_callback(&mut self, cb: Option<fn(&mut dyn DebugHost)>) {
        self.vm.wqdb.on_pause = cb;
    }

    pub fn builtins(&self) -> &crate::builtins::Builtins {
        &self.vm.builtins
    }

    pub fn builtins_preset(&self) -> BuiltinPreset {
        self.vm.builtins_preset
    }

    pub fn set_builtins_preset(&mut self, preset: BuiltinPreset) {
        self.vm.builtins.apply_preset(preset);
        self.vm.builtins_preset = preset;
    }

    /// Get mutable access to the VM for debugger integration
    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.vm
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
        let mut lexer = if let Some((_, full_text)) = self.dbg_source_ctx.as_ref() {
            Lexer::new(input).with_ctx(full_text, self.dbg_source_offs)
        } else {
            Lexer::new(input)
        };
        let tokens = lexer.tokenize()?;
        if self.debug_flags.contains(DebugFlags::TOKEN) {
            let header = "TOKENS".bold().underline().to_string();
            stderr_println(header);
            stderr_println(fmt_tokens_table(&tokens));
            stderr_println("");
        }

        // Use global debug source + offset when available to improve error spans
        let builtins = self.vm.builtins.clone();
        let mut parser = if let Some((_, full_text)) = self.dbg_source_ctx.as_ref() {
            Parser::new_with_ctx_and_builtins(
                tokens,
                input.to_string(),
                Some(full_text.clone()),
                self.dbg_source_offs,
                builtins.clone(),
            )
        } else {
            Parser::new_with_builtins(tokens, input.to_string(), builtins.clone())
        };
        let ast = parser.parse()?;
        let mut resolver = Resolver::from_env_and_builtins(self.environment(), builtins.clone());
        let ast = resolver.resolve(ast);
        let ast = folder::fold(ast);

        if self.debug_flags.contains(DebugFlags::AST) {
            let header = "AST".bold().underline().to_string();
            stderr_println(header);
            stderr_println(format!("{ast}").as_str());
            stderr_println("");
        }

        let mut compiler = Compiler::new_with_builtins(builtins);
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.set_source(input.to_string());
        compiler.set_stmt_spans(parser.stmt_spans_top().to_vec());
        compiler.compile(&ast)?;
        compiler.fuse();
        compiler.instructions.push(Instruction::Return);

        if self.debug_flags.contains(DebugFlags::INST) {
            let header = "INSTRUCTIONS".bold().underline().to_string();
            stderr_println(header);
            let lines = InstPrettyDumper::new(true, true).render(&compiler.instructions);
            for line in lines {
                stderr_println(line.as_str());
            }
            stderr_println("");
        }

        self.vm.clear_last_bt();
        self.vm.reset(compiler.instructions);
        // Prepare debug artifacts when wqdb or backtrace mode is on
        if self.vm.wqdb.enabled || _enter_wqdb || self.bt_mode {
            // Prepare debug mapping for this top-level script
            let (src_path, src_text) = if let Some((p, t)) = self.dbg_source_ctx.as_ref() {
                (p.clone(), t.clone())
            } else {
                ("<eval>".to_string(), input.to_string())
            };
            self.vm.repl_debug_prepare_script(&src_path, &src_text);
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
        // If wqdb is enabled (persistently or armed just once), step-in
        // Note: on_pause callback must be set externally via set_pause_callback()
        if self.vm.wqdb.enabled || _enter_wqdb {
            self.vm.dbg_step_in();
        }
        match self.interpreter {
            InterpreterKind::Sample => self.vm.run_with_interpreter(&mut SampleInterpreter),
            InterpreterKind::Default => self.vm.run_with_interpreter(&mut DefaultInterpreter),
            InterpreterKind::Profiler => self
                .vm
                .run_with_interpreter(&mut ProfilerInterpreter::default()),
        }
    }

    /// Access the environment holding user-defined bindings.
    pub fn environment(&self) -> &GlobalMap {
        self.vm.global_env()
    }

    /// Optionally get the environment if it contains any bindings.
    pub fn get_environment(&self) -> Option<&GlobalMap> {
        let env = self.vm.global_env();
        if env.is_empty() { None } else { Some(env) }
    }

    /// Mutable access to the environment.
    pub fn environment_mut(&mut self) -> &mut GlobalMap {
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
            .unwrap_or_else(|| self.vm.bt_frames());
        let di = &self.vm.debug_info;
        for (idx, (loc, name)) in frames.iter().enumerate() {
            let is_last = idx + 1 == frames.len();
            stderr_println(wqdb::format_frame(di, *loc, name, is_last));
        }
    }

    /// Reset REPL session state: clear environment and virtual debug sources
    pub fn reset_session(&mut self) {
        self.environment_mut().clear();
        self.vm.debug_info = wqdb::DebugInfo::default();
        self.vm.wqdb = wqdb::Wqdb::default();
    }
}

impl Default for DefaultEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::{apps::evaluator::default::DefaultEvaluator, value::Value};

    #[test]
    fn undefined_variable_errors() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("a");
        assert!(res.is_err());
    }

    #[test]
    fn empty_conditional_branches_dont_panic() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("$[true;;]");
        assert!(res.is_ok());
    }

    #[test]
    fn conditional_chain_picks_first_true() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("x:0;$$[x>0;1;x<0;-1;0]").unwrap();
        assert_eq!(res, Value::Int(0));
        let res = eval.eval_string("x:-2;$$[x>0;1;x<0;-1]").unwrap();
        assert_eq!(res, Value::Int(-1));
    }

    #[test]
    fn block_expr_evaluates_last_statement() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("x:B[a:1;a+1];x").unwrap();
        assert_eq!(res, Value::Int(2));
    }

    #[test]
    fn for_each_sets_f() {
        let mut eval = DefaultEvaluator::new();
        let res = eval
            .eval_string("acc:0;F[(1;2;3);acc:acc+_f;];acc")
            .unwrap();
        assert_eq!(res, Value::Int(6));
    }

    #[test]
    fn empty_loop_body_dont_panic() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("N[3;]");
        assert!(res.is_ok());
    }

    #[test]
    fn empty_unit_pop_is_fused_out_in_loops() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("a:0;N[3;a:a+1;];a").unwrap();
        assert_eq!(res, Value::Int(3));
    }

    #[test]
    fn break_and_continue() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("n:0;N[5;$[n=2;@c;];n:n+1;];n").unwrap();
        assert_eq!(res, Value::Int(2));
    }

    #[test]
    fn return_in_function() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("f:{@r 3;1};f[]").unwrap();
        assert_eq!(res, Value::Int(3));
    }

    // #[test]
    // fn assert_fails() {
    //     let mut eval = VmEvaluator::new();
    //     let res = eval.eval_string("@a 1=2;");
    //     assert!(matches!(res, Err(WqError::Assert(_))));
    // }

    #[test]
    fn implicit_arg_order_and_arity() {
        let mut eval = DefaultEvaluator::new();
        // Test argument order with three implicit parameters
        let res = eval.eval_string("f:{100*x+10*y+z};f[1;2;3]").unwrap();
        assert_eq!(res, Value::Int(123));

        // Too many args should error
        let res = eval.eval_string("f[1;2;3;4]");
        assert!(res.is_err());
    }

    #[test]
    fn arity_error_too_many_args() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("f:{[a;b;c]a+b+c};f[1;2;3;4]");
        assert!(res.is_err());
    }

    #[test]
    fn intlist_literal_inferred_and_list_interop() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("(1;2;3)").unwrap();
        assert_eq!(res, Value::IntList(vec![1, 2, 3]));

        eval.eval_string("a:alloc 3").unwrap();
        eval.eval_string("b:(0;0;0)").unwrap();
        let sum = eval.eval_string("a+b").unwrap();
        assert_eq!(sum, Value::IntList(vec![0, 0, 0]));
        // let cmp = eval.eval_string("a=b").unwrap();
        // assert_eq!(
        //     cmp,
        //     Value::List(vec![
        //         Value::Bool(true),
        //         Value::Bool(true),
        //         Value::Bool(true)
        //     ])
        // );
    }

    #[test]
    fn nested_function_calls_access_locals() {
        let mut eval = DefaultEvaluator::new();
        let code = "fib:{fib_:{[n;a;b]$[n=0;a;fib_[n-1;b;a+b]]};fib_[x;0;1]};fib 10";
        let res = eval.eval_string(code).unwrap();
        assert_eq!(res, Value::Int(55));
    }

    #[test]
    fn local_function_compiles_once_and_works_twice() {
        let mut eval = DefaultEvaluator::new();
        // Define a local function 'g' inside 'h' and call it twice
        let code = "h:{g:{[n]n+1}; g 1 + g 2}; h[]";
        let res = eval.eval_string(code).unwrap();
        assert_eq!(res, Value::Int(5));
    }

    #[test]
    fn builtin_arg_order_preserved() {
        let mut eval = DefaultEvaluator::new();
        // 'take' takes (list, n) and returns first n items
        let res = eval.eval_string("log[100;10]").unwrap();
        assert_eq!(res, Value::Float(2.0));
    }

    #[test]
    fn range_builder_half_open_default_step() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("1..10").unwrap();
        assert_eq!(res, Value::IntList(vec![1, 2, 3, 4, 5, 6, 7, 8, 9]));
    }

    #[test]
    fn range_builder_inclusive_with_step() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("1..=11..2").unwrap();
        assert_eq!(res, Value::IntList(vec![1, 3, 5, 7, 9, 11]));
    }

    // #[test]
    // fn range_builder_step_inference_descending() {
    //     let mut eval = VmEvaluator::new();
    //     let res = eval.eval_string("10..1..2").unwrap();
    //     assert_eq!(res, Value::IntList(vec![10, 8, 6, 4, 2]));
    //     let res_inclusive = eval.eval_string("10..=1").unwrap();
    //     assert_eq!(
    //         res_inclusive,
    //         Value::IntList(vec![10, 9, 8, 7, 6, 5, 4, 3, 2, 1])
    //     );
    // }

    #[test]
    fn builtin_function_can_be_passed_and_called() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("a:{[x]x[]};a[rand]").unwrap();
        assert!(matches!(res, Value::Float(_)));
    }

    #[test]
    fn closure_captures_global_by_value() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("a:3;f:{a};a:4;f[]").unwrap();
        assert_eq!(res, Value::Int(3));
    }

    #[test]
    fn closure_captures_local_by_value() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("f:{a:4;f2:{a};a:5;f2};f[][]").unwrap();
        assert_eq!(res, Value::Int(4));
    }

    #[test]
    fn closure_debug_info_includes_span() {
        let mut eval = DefaultEvaluator::new();
        eval.eval_string("b:{a:1;c:{}}").unwrap();
        eval.eval_string("b[]").unwrap();
        let di = &eval.vm.debug_info;
        let chunk_id = *di.by_name.get("b").expect("chunk for 'b'");
        let chunk = di.chunk(chunk_id);
        let span = chunk.line_table.span_at(0);
        assert_ne!(
            span.file_id,
            u32::MAX,
            "closure chunk should have a resolved source span"
        );
        if let Some(file) = di.file(span.file_id) {
            assert_eq!(file.path.as_ref(), "<eval>");
            let (line, col) = file.line_col(span.start as usize);
            assert_eq!(line, 1);
            assert_eq!(col, 4);
        }
    }

    #[test]
    fn eval_f_string_and_raw() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("@f\"{1+2}\"").unwrap();
        assert_eq!(res, Value::List("3".chars().map(Value::Char).collect()));

        let res2 = eval.eval_string("a:41; @f\"{a+1}\"").unwrap();
        assert_eq!(res2, Value::List("42".chars().map(Value::Char).collect()));

        let res3 = eval.eval_string("@l\"\\n\"").unwrap();
        assert_eq!(res3, Value::List("\\n".chars().map(Value::Char).collect()));
    }

    // #[test]
    // fn try_returns_status() {
    //     let mut eval = VmEvaluator::new();
    //     let ok = eval.eval_string("@t 1+2").unwrap();
    //     assert_eq!(ok, Value::List(vec![Value::Int(3), Value::Int(0)]));

    //     let err = eval.eval_string("@t 1+\"a\"").unwrap();
    //     if let Value::List(items) = err {
    //         assert_eq!(items.len(), 2);
    //         match &items[1] {
    //             Value::Int(code) => {
    //                 assert_eq!(*code, WqError::Domain(String::new()).code() as i64);
    //             }
    //             _ => panic!("expected error code"),
    //         }
    //         assert!(items[0].to_string().contains("DOMAIN ERROR"));
    //     } else {
    //         panic!("expected list result");
    //     }
    // }

    #[test]
    fn long_chain_of_negation_does_not_overflow() {
        let mut eval = DefaultEvaluator::new();
        let hyphens = "-".repeat(10000);
        let expr = format!("{hyphens}10");
        let res = eval.eval_string(&expr).unwrap();
        assert_eq!(res, Value::Int(10));
    }

    #[test]
    fn passed_function_resolves_correctly() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("a:{2*x};b:{x[3]};b[a]").unwrap();
        assert_eq!(res, Value::Int(6));
        let res = eval.eval_string("a:iota 10;b[a]").unwrap();
        assert_eq!(res, Value::Int(3));
    }

    #[test]
    fn recursive_function_with_postfix() {
        let mut eval = DefaultEvaluator::new();
        let res = eval.eval_string("a:{[n]$[n<4;a[n+1];n]};a 0").unwrap();
        assert_eq!(res, Value::Int(4));
    }

    #[test]
    fn backtrace_includes_names_for_captured_function_calls() {
        let mut eval = DefaultEvaluator::new();

        eval.dbg_set_source("wq[1]", "a:{1/0}");
        eval.dbg_set_offset(0);
        eval.eval_string("a:{1/0}").unwrap();

        eval.dbg_set_source("wq[2]", "c:{d:a;a[]}");
        eval.dbg_set_offset(0);
        eval.eval_string("c:{d:a;a[]}").unwrap();

        eval.dbg_set_source("wq[3]", "c[]");
        eval.dbg_set_offset(0);
        let err = eval.eval_string("c[]");
        assert!(err.is_err());

        let frames = eval.vm.take_last_bt().expect("backtrace captured");
        assert!(frames.len() >= 3);

        let frame_names: Vec<&str> = frames.iter().map(|(_, name)| name.as_ref()).collect();
        assert_eq!(frame_names[0], "a");
        assert_eq!(frame_names[1], "c");
        assert_eq!(frame_names.last().copied(), Some("<repl>"));

        let top_span = {
            let loc = frames[0].0;
            eval.vm
                .debug_info
                .chunk(loc.chunk)
                .line_table
                .span_at(loc.pc)
        };
        assert_ne!(top_span.file_id, u32::MAX);

        let di = &eval.vm.debug_info;
        let top_file = di
            .file(di.chunk(frames[0].0.chunk).file_id)
            .unwrap()
            .path
            .as_ref()
            .to_string();
        assert_eq!(top_file, "wq[1]");

        let mid_file = di
            .file(di.chunk(frames[1].0.chunk).file_id)
            .unwrap()
            .path
            .as_ref()
            .to_string();
        assert_eq!(mid_file, "wq[2]");

        let last_loc = frames.last().unwrap().0;
        let last_file = di
            .file(di.chunk(last_loc.chunk).file_id)
            .unwrap()
            .path
            .as_ref();
        assert_eq!(last_file, "wq[3]");
    }
}
