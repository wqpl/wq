pub mod dbglog;
pub mod stdio;

use colored::Colorize;

use crate::astnode::AstNode;
use crate::builtins::BuiltinPreset;
use crate::cst::{GreenChild, GreenNode, GreenNodeBuilder, GreenToken, SyntaxKind};
use crate::compile::Compiler;
use crate::interpret::InterpreterKind;
use crate::interpret::profiler::ProfilerInterpreter;
use crate::interpret::sample::SampleInterpreter;
use crate::interpret::vanilla::VanillaInterpreter;
use crate::lex::Lexer;
use crate::parse::resolve::Resolver;
use crate::parse::{Parser, fold};
use crate::script::{ScriptItem, ScriptSpan, parse_script_items};
use crate::session::dbglog::{DebugLogFlags, get_debug_log_flags};
use crate::session::stdio::wqstderr_println;
use crate::symbol::SymbolIndex;
use crate::token::{FmtPart, Token, TokenType, fmt_tokens_table};
use crate::value::{Value, WqResult};
use crate::vm::inst::{InstPrettyDumper, Instruction};
use crate::vm::{GlobalMap, Vm};
use crate::wqdb::build::{
    apply_stmt_debug_exact_offs, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
    register_function_chunks,
};
use crate::wqdb::data::DebugInfo;
use crate::wqdb::{self};
use crate::wqerror::WqError;
use crate::wqerror::WqErrorType;

pub struct Session {
    vm: Vm,
    // debug_flags: DebugFlags,
    dry_mode: bool,
    // Arm entering the wqdb on the next eval call
    wqdb_arm_next: bool,
    // Optional debug source context for next eval (path, full_text)
    dbg_source_ctx: Option<(String, String)>,
    // Byte offset into dbg_source_ctx where current snippet starts
    dbg_source_offs: usize,
    // Backtrace mode (minimal debug mapping for errors)
    bt_mode: bool,
    interpreter: InterpreterKind,
    profiler: ProfilerInterpreter,
}

impl Session {
    /// Create a new evaluator with an empty environment.
    pub fn new() -> Self {
        let mut vm = Vm::new(Vec::new());
        vm.set_bt_mode(true);
        Session {
            vm,
            // debug_flags: DebugFlags::empty(),
            dry_mode: false,
            wqdb_arm_next: false,
            dbg_source_ctx: None,
            dbg_source_offs: 0,
            bt_mode: true,
            interpreter: InterpreterKind::Vanilla,
            profiler: ProfilerInterpreter::default(),
        }
    }

    pub fn env_vars(&self) -> GlobalMap {
        self.environment()
    }

    pub fn is_wqdb_enabled(&self) -> bool {
        self.vm.wqdb.enabled
    }

    pub fn reset_session(&mut self) {
        self.vm.reset_globals();
        self.vm.debug_info = DebugInfo::default();
        let on_pause = self.vm.wqdb.on_pause;
        self.vm.wqdb = wqdb::Wqdb::default();
        self.vm.wqdb.on_pause = on_pause;
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

    pub fn set_dry_mode(&mut self, flag: bool) {
        self.dry_mode = flag;
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
            // Don't clear on_pause - keep the callback registered for
            // re-enabling
        }
    }

    pub fn set_pause_callback(&mut self, cb: Option<fn(&mut Vm)>) {
        self.vm.wqdb.on_pause = cb;
    }

    pub fn set_wqdb_batch_cmds(&mut self, cmds: Vec<String>) {
        self.vm.wqdb.batch_cmds = cmds;
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
        let mut lexer = if let Some((_, full_text)) = self.dbg_source_ctx.as_ref() {
            Lexer::new(input).with_ctx(full_text, self.dbg_source_offs)
        } else {
            Lexer::new(input)
        };
        if let Some((path, _)) = self.dbg_source_ctx.as_ref() {
            lexer.set_source_path(path.clone());
        }
        let tokens = lexer.tokenize()?;
        if get_debug_log_flags().contains(DebugLogFlags::TOKEN) {
            let header = "TOKEN".bold().underline().to_string();
            wqstderr_println(header);
            wqstderr_println(fmt_tokens_table(&tokens));
            wqstderr_println("");
        }

        // Use global debug source + offset when available to improve error spans
        let builtins = self.vm.builtins.clone();
        let mut parser = if let Some((_, full_text)) = self.dbg_source_ctx.as_ref() {
            Parser::new_with_ctx(
                tokens,
                input.to_string(),
                Some(full_text.clone()),
                self.dbg_source_offs,
                builtins.clone(),
            )
        } else {
            Parser::new_with_builtins(tokens, input.to_string(), builtins.clone())
        };
        if let Some((path, _)) = self.dbg_source_ctx.as_ref() {
            parser.set_source_path(path.clone());
        }
        let dump_cst = get_debug_log_flags().contains(DebugLogFlags::CST);
        if dump_cst {
            parser.enable_cst();
        }
        let ast_src = self
            .dbg_source_ctx
            .as_ref()
            .map(|(_, t)| t.as_str())
            .unwrap_or(input);

        let ast = parser.parse()?;
        if let Some(eof_err) = parser.eof_error() {
            return Err(eof_err.clone());
        }
        if dump_cst {
            let cst = parser
                .take_cst()
                .expect("enable_cst was just called, so take_cst yields Some");
            let header = "CST".bold().underline().to_string();
            wqstderr_println(header);
            wqstderr_println(crate::cst::SyntaxNode::new_root(cst).pretty_print());
            wqstderr_println("");
        }

        let dump_ast = |s: &str, ast: &AstNode, flag: u16| {
            if get_debug_log_flags().contains(flag) {
                let header = s.bold().underline().to_string();
                wqstderr_println(header);
                wqstderr_println(ast.sexpr_pretty_with_source(ast_src));
                wqstderr_println("");
            }
        };

        dump_ast("AST - original", &ast, DebugLogFlags::AST);

        let env = self.environment();
        let mut resolver = Resolver::from_env(env.clone(), builtins.clone());
        let ast = resolver.resolve(ast);
        dump_ast("AST @ resolve", &ast, DebugLogFlags::AST_VERBOSE);

        let ast = fold::fold(ast);
        dump_ast("AST @ fold - final", &ast, DebugLogFlags::AST_VERBOSE);

        let mut compiler = Compiler::new_with_builtins(builtins);
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.set_source(input.to_string());
        if let Some((path, _)) = self.dbg_source_ctx.as_ref() {
            compiler.set_source_path(path.clone());
        }
        compiler.set_stmt_spans(parser.stmt_spans_top().to_vec());
        compiler.compile(&ast)?;
        compiler.instructions.push(Instruction::Return);

        let dump_inst = |s: &str, inst: &Vec<Instruction>, flag: u16| {
            if get_debug_log_flags().contains(flag) {
                let header = s.bold().underline().to_string();
                wqstderr_println(header);
                let lines = InstPrettyDumper::new(true, true).with_pc().render(inst);
                for line in lines {
                    wqstderr_println(line);
                }
                wqstderr_println("");
            }
        };

        dump_inst(
            "Inst - original",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        );

        compiler.propagate_constants_with_globals(&env);
        dump_inst(
            "Inst @ constprop",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        );

        compiler.rewrite_tail_calls();
        dump_inst(
            "Inst @ tailcall",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        );

        compiler.fuse();
        dump_inst(
            "Inst @ fuse - final",
            &compiler.instructions,
            DebugLogFlags::INST,
        );

        if self.dry_mode {
            return Ok(Value::unit());
        }

        self.vm.clear_last_bt();
        self.vm.set_runtime_debug_info(compiler.has_runtime_debug);
        self.vm.reset_inst_and_state(compiler.instructions);
        // Prepare debug artifacts when wqdb or backtrace mode is on
        let temp_wqdb_on = if self.wqdb_arm_next {
            self.wqdb_arm_next = false;
            true
        } else {
            false
        };

        if self.vm.debug_artifacts_enabled() || temp_wqdb_on {
            // Prepare debug mapping for this top-level script
            let (src_path, src_text) = if let Some((p, t)) = self.dbg_source_ctx.as_ref() {
                (p.clone(), t.clone())
            } else {
                ("<eval>".to_string(), input.to_string())
            };
            self.vm.script_prepare_debug(&src_path, &src_text);
            // Set base offset into the source file for this snippet
            self.vm.set_debug_src_offset(self.dbg_source_offs);
            // Mark statements using a combination of parser spans and heuristics
            {
                let chunk = self.vm.current_chunk_id();
                // Compute file_id first to avoid borrow conflicts
                let file_id = self.vm.debug_info.chunk(chunk).file_id;
                // First mark all likely statement PCs
                {
                    let code = &self.vm.instructions;
                    let line_table = &mut self.vm.debug_info.chunk_mut(chunk).line_table;
                    if !compiler.dbg_pc_spans.is_empty() && !compiler.dbg_stmt_marks.is_empty() {
                        let mut pc_spans = compiler.dbg_pc_spans.clone();
                        pc_spans.resize(code.len(), None);
                        apply_stmt_debug_exact_offs(
                            line_table,
                            file_id,
                            &pc_spans,
                            &compiler.dbg_stmt_marks,
                            self.dbg_source_offs,
                        );
                    } else {
                        mark_stmt_heuristic(line_table, code);
                        // Overlay exact mapping for top-level spans across candidates
                        apply_stmt_spans_exact_offs(
                            line_table,
                            code,
                            file_id,
                            parser.stmt_spans_top(),
                            self.dbg_source_offs,
                        );
                    }
                }
                // Recursively register chunks for nested non-capturing functions
                let instructions = std::sync::Arc::make_mut(&mut self.vm.instructions);
                register_function_chunks(
                    &mut self.vm.debug_info,
                    file_id,
                    instructions,
                    self.dbg_source_offs,
                );
            }
        }
        // Drop AST and parser before execution to release Arc refs to
        // literal constants that would otherwise inflate strong counts
        // and cause unnecessary COW deep clones during mutation.
        drop(ast);
        drop(parser);
        // If wqdb was newly enabled, stop at the next eval entry.  After the
        // user continues, later streaming-loader chunks should not re-enter
        // the debugger unless a breakpoint, explicit @p, or step mode asks
        // for it.
        // Note: on_pause callback must be set externally via set_pause_callback()
        if temp_wqdb_on {
            self.vm.dbg_step_in();
        }
        self.vm.interpreter_kind = self.interpreter;
        let result = match self.interpreter {
            InterpreterKind::Sample => {
                let mut sample = SampleInterpreter::default();
                self.vm.run_with_interpreter(&mut sample)
            }
            InterpreterKind::Vanilla => self.vm.run_with_interpreter(&mut VanillaInterpreter),
            InterpreterKind::Profiler => self.vm.run_with_interpreter(&mut self.profiler),
        };
        if get_debug_log_flags().contains(DebugLogFlags::VALUE)
            && let Ok(v) = &result
        {
            wqstderr_println(format!("{v:?}"));
        }
        result
    }

    /// Parse and analyze symbols in `input` without executing.
    /// Returns a `SymbolIndex` that can be queried for definitions and uses.
    pub fn analyze_symbols(&self, input: &str) -> WqResult<SymbolIndex> {
        if input.contains('!') {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                let (ast, eof_errors) = self.parse_script_ast(input, &items)?;
                let ast = Resolver::with_builtins(self.vm.builtins.clone()).resolve(ast);
                let mut index = SymbolIndex::analyze(&ast, &self.vm.builtins);
                for eof_err in eof_errors {
                    let span = eof_err.span.unwrap_or((input.len(), input.len()));
                    index.errors.push((span, eof_err));
                }
                return Ok(index);
            }
        }

        self.analyze_symbols_code(input)
    }

    fn analyze_symbols_code(&self, input: &str) -> WqResult<SymbolIndex> {
        let tokens = Lexer::new(input).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.vm.builtins.clone());
        let ast = parser.parse()?;
        let ast = Resolver::with_builtins(self.vm.builtins.clone()).resolve(ast);
        let mut index = SymbolIndex::analyze(&ast, &self.vm.builtins);
        if let Some(eof_err) = parser.eof_error() {
            let span = eof_err.span.unwrap_or((input.len(), input.len()));
            index.errors.push((span, eof_err.clone()));
        }
        Ok(index)
    }

    /// Tokenize input with recovery, returning all tokens including error
    /// tokens. Useful for diagnostics and completions on syntactically
    /// broken code.
    pub fn tokenize_recovery(&self, input: &str) -> Vec<crate::token::Token> {
        if input.contains('!') {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                let mut tokens = Vec::new();
                for item in &items {
                    let ScriptItem::Code { span } = item else {
                        continue;
                    };
                    let mut lexer = Lexer::new(&input[span.as_range()]);
                    let chunk_tokens = lexer.tokenize_recovery();
                    for mut token in chunk_tokens {
                        if matches!(token.token_type, TokenType::Eof) {
                            continue;
                        }
                        offset_token(&mut token, span.start);
                        tokens.push(token);
                    }
                }
                tokens.push(eof_token_for(input));
                return tokens;
            }
        }

        crate::lex::Lexer::new(input).tokenize_recovery()
    }

    /// Parse `input` and return both the AST and the green CST.
    ///
    /// The CST round-trips byte-for-byte: `cst.text() == input` for any
    /// input the lexer accepts. Parse errors are recovered into
    /// [`crate::astnode::AstNode::Error`] in the AST and into
    /// [`crate::cst::SyntaxKind::ErrorNode`] subtrees in the CST, so a
    /// partial parse still yields a usable green tree.
    ///
    /// This is the entry point that the language server (and, eventually,
    /// the formatter) uses for every document. Cost is the same as
    /// [`Self::analyze_symbols`] plus one `Arc` clone of the lexer's token
    /// stream.
    pub fn parse_with_cst(
        &self,
        input: &str,
    ) -> WqResult<(crate::astnode::AstNode, crate::cst::GreenNode)> {
        if input.contains('!') {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                return self.parse_script_with_cst(input, &items);
            }
        }

        let tokens = Lexer::new(input).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.vm.builtins.clone());
        parser.enable_cst();
        let ast = parser.parse()?;
        let cst = parser
            .take_cst()
            .expect("enable_cst was just called, so take_cst yields Some");
        Ok((ast, cst))
    }

    pub fn parse_with_cst_using_cache(
        &self,
        input: &str,
        previous: &crate::cst::GreenNode,
    ) -> WqResult<(crate::astnode::AstNode, crate::cst::GreenNode)> {
        if input.contains('!') {
            let items = parse_script_items(input);
            if has_script_meta(&items) {
                return self.parse_script_with_cst(input, &items);
            }
        }

        let previous_text = previous.text();
        let (old_start, old_end, new_start, new_end) =
            compute_dirty_byte_range(&previous_text, input);
        let tokens = Lexer::new(input).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, input.to_string(), self.vm.builtins.clone());
        parser.enable_cst_with_cache(previous, old_start, old_end, new_start, new_end);
        let ast = parser.parse()?;
        let cst = parser
            .take_cst()
            .expect("enable_cst_with_cache was just called, so take_cst yields Some");
        Ok((ast, cst))
    }

    fn parse_script_ast(
        &self,
        input: &str,
        items: &[ScriptItem],
    ) -> WqResult<(AstNode, Vec<WqError>)> {
        let mut statements = Vec::new();
        let mut eof_errors = Vec::new();
        for item in items {
            let ScriptItem::Code { span } = item else {
                continue;
            };
            let (ast, eof_error) = self.parse_script_code_ast(input, *span)?;
            push_script_ast(&mut statements, ast);
            if let Some(eof_error) = eof_error {
                eof_errors.push(eof_error);
            }
        }
        Ok((script_ast_from_statements(statements), eof_errors))
    }

    fn parse_script_code_ast(
        &self,
        input: &str,
        span: ScriptSpan,
    ) -> WqResult<(AstNode, Option<WqError>)> {
        let source = &input[span.as_range()];
        let tokens = Lexer::new(source).with_ctx(input, span.start).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, source.to_string(), self.vm.builtins.clone());
        let mut ast = match parser.parse() {
            Ok(ast) => ast,
            Err(mut err) => {
                offset_error_span(&mut err, span.start);
                return Err(err);
            }
        };
        Parser::offset_spans(&mut ast, span.start);
        let eof_error = parser.eof_error().cloned().map(|mut err| {
            offset_error_span(&mut err, span.start);
            err
        });
        Ok((ast, eof_error))
    }

    fn parse_script_with_cst(
        &self,
        input: &str,
        items: &[ScriptItem],
    ) -> WqResult<(AstNode, GreenNode)> {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(SyntaxKind::Root);
        let mut cursor = 0usize;
        let mut statements = Vec::new();

        for item in items {
            let span = item.span();
            if span.start > cursor {
                push_script_trivia(&mut builder, &input[cursor..span.start]);
            }

            match item {
                ScriptItem::Shebang { span } => {
                    push_script_line_node(&mut builder, input, *span, SyntaxKind::Shebang);
                }
                ScriptItem::Directive(directive) => {
                    push_script_line_node(
                        &mut builder,
                        input,
                        directive.span(),
                        SyntaxKind::ScriptDirective,
                    );
                }
                ScriptItem::Code { span } => {
                    let (ast, cst) = self.parse_script_code_cst(input, *span)?;
                    push_script_ast(&mut statements, ast);
                    append_root_children(&mut builder, &cst);
                }
            }

            cursor = span.end;
        }

        if cursor < input.len() {
            push_script_trivia(&mut builder, &input[cursor..]);
        }

        builder.finish_node();
        Ok((script_ast_from_statements(statements), builder.finish()))
    }

    fn parse_script_code_cst(
        &self,
        input: &str,
        span: ScriptSpan,
    ) -> WqResult<(AstNode, GreenNode)> {
        let source = &input[span.as_range()];
        let tokens = Lexer::new(source).with_ctx(input, span.start).tokenize()?;
        let mut parser =
            Parser::new_with_builtins(tokens, source.to_string(), self.vm.builtins.clone());
        parser.enable_cst();
        let mut ast = match parser.parse() {
            Ok(ast) => ast,
            Err(mut err) => {
                offset_error_span(&mut err, span.start);
                return Err(err);
            }
        };
        Parser::offset_spans(&mut ast, span.start);
        let cst = parser
            .take_cst()
            .expect("enable_cst was just called, so take_cst yields Some");
        Ok((ast, cst))
    }

    /// Build a snapshot of the environment from slots.
    pub fn environment(&self) -> GlobalMap {
        self.vm.global_env()
    }

    /// Clear all global bindings.
    pub fn clear_environment(&mut self) {
        self.vm.global_slots.clear();
        self.vm.global_slot_versions.clear();
        self.vm.global_slot_map.clear();
    }

    /// Check whether `input` forms a syntactically complete wq snippet.
    /// Returns `false` when the lexer or parser signals an EOF error,
    /// indicating more input is expected.
    pub fn is_complete_input(input: &str) -> bool {
        let mut lexer = Lexer::new(input);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => return e.err_type != WqErrorType::Eof,
        };
        let mut parser = Parser::new(tokens, input.to_string());
        match parser.parse() {
            Ok(_) => parser.eof_error().is_none(),
            Err(e) => e.err_type != WqErrorType::Eof,
        }
    }

    // Arm wqdb for the next eval.
    // pub fn arm_wqdb_next(&mut self) {
    //     self.wqdb_arm_next = true;
    // }

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
            let is_current = idx == 0;
            wqstderr_println(wqdb::format_frame(di, *loc, name, is_current));
        }
    }

    /// Assign a value to a global variable by name via the slot-based global
    /// table.
    pub fn assign_global(&mut self, name: &str, value: Value) {
        self.vm.assign_global_and_slot(name, value);
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

fn has_script_meta(items: &[ScriptItem]) -> bool {
    items
        .iter()
        .any(|item| matches!(item, ScriptItem::Shebang { .. } | ScriptItem::Directive(_)))
}

fn push_script_ast(statements: &mut Vec<AstNode>, ast: AstNode) {
    match ast {
        AstNode::Block(nodes) => statements.extend(nodes),
        node => statements.push(node),
    }
}

fn script_ast_from_statements(mut statements: Vec<AstNode>) -> AstNode {
    if statements.len() == 1 {
        statements.remove(0)
    } else {
        AstNode::Block(statements)
    }
}

fn push_script_line_node(
    builder: &mut GreenNodeBuilder,
    input: &str,
    span: ScriptSpan,
    kind: SyntaxKind,
) {
    let text = &input[span.as_range()];
    let (line, newline) = text
        .strip_suffix('\n')
        .map_or((text, None), |line| (line, Some("\n")));
    builder.start_node(kind);
    if !line.is_empty() {
        builder.token(SyntaxKind::ScriptLine, line);
    }
    builder.finish_node();
    if let Some(newline) = newline {
        builder.token(SyntaxKind::Newline, newline);
    }
}

fn push_script_trivia(builder: &mut GreenNodeBuilder, text: &str) {
    let mut line_start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch != '\n' {
            continue;
        }
        push_script_trivia_line(builder, &text[line_start..idx]);
        builder.token(SyntaxKind::Newline, "\n");
        line_start = idx + ch.len_utf8();
    }
    if line_start < text.len() {
        push_script_trivia_line(builder, &text[line_start..]);
    }
}

fn push_script_trivia_line(builder: &mut GreenNodeBuilder, line: &str) {
    let trimmed = line.trim_start_matches([' ', '\t', '\r']);
    let leading_len = line.len() - trimmed.len();
    let rest = &line[leading_len..];
    if rest.starts_with("//") {
        if leading_len > 0 {
            builder.token(SyntaxKind::Whitespace, &line[..leading_len]);
        }
        builder.token(SyntaxKind::Comment, rest);
    } else if !line.is_empty() {
        builder.token(SyntaxKind::Whitespace, line);
    }
}

fn append_root_children(builder: &mut GreenNodeBuilder, root: &GreenNode) {
    for child in root.children() {
        append_green_child(builder, child);
    }
}

fn append_green_child(builder: &mut GreenNodeBuilder, child: &GreenChild) {
    match child {
        GreenChild::Node(node) => builder.append_node(node.clone()),
        GreenChild::Token(token) => append_green_token(builder, token),
    }
}

fn append_green_token(builder: &mut GreenNodeBuilder, token: &GreenToken) {
    builder.token(token.kind(), token.text());
}

fn offset_error_span(err: &mut WqError, offset: usize) {
    if let Some((start, end)) = &mut err.span {
        *start += offset;
        *end += offset;
    }
}

fn offset_token(token: &mut Token, offset: usize) {
    token.byte_start += offset;
    token.byte_end += offset;
    if let TokenType::FormatString(parts, open_quote, close_quote) = &mut token.token_type {
        *open_quote += offset;
        *close_quote += offset;
        for part in parts {
            match part {
                FmtPart::Text { start, end, .. } | FmtPart::Expr { start, end, .. } => {
                    *start += offset;
                    *end += offset;
                }
            }
        }
    }
}

fn eof_token_for(input: &str) -> Token {
    let line = input.bytes().filter(|b| *b == b'\n').count() + 1;
    let column = input
        .rsplit('\n')
        .next()
        .map_or(1, |line| line.chars().count() + 1);
    Token::new(
        TokenType::Eof,
        input.chars().count() + 1,
        line,
        column,
        input.len(),
        input.len(),
    )
}

fn compute_dirty_byte_range(old: &str, new: &str) -> (usize, usize, usize, usize) {
    let old_bytes = old.as_bytes();
    let new_bytes = new.as_bytes();

    let mut prefix = 0usize;
    while prefix < old_bytes.len()
        && prefix < new_bytes.len()
        && old_bytes[prefix] == new_bytes[prefix]
    {
        prefix += 1;
    }

    let mut old_end = old_bytes.len();
    let mut new_end = new_bytes.len();
    while old_end > prefix && new_end > prefix && old_bytes[old_end - 1] == new_bytes[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }

    (prefix, old_end, prefix, new_end)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::cst::GreenChild;

    fn root_nodes(root: &crate::cst::GreenNode) -> Vec<crate::cst::GreenNode> {
        root.children()
            .iter()
            .filter_map(|child| match child {
                GreenChild::Node(node) => Some(node.clone()),
                GreenChild::Token(_) => None,
            })
            .collect()
    }

    #[test]
    fn test_assign_global_new_var() {
        let mut session = Session::new();
        session.assign_global("x", Value::Int(42));
        assert_eq!(session.env_vars().get("x"), Some(&Value::Int(42)));
    }

    #[test]
    fn test_assign_global_overwrite() {
        let mut session = Session::new();
        session.assign_global("x", Value::Int(1));
        session.assign_global("x", Value::Int(2));
        assert_eq!(session.env_vars().get("x"), Some(&Value::Int(2)));
    }

    #[test]
    fn test_assign_global_visible_in_eval() {
        let mut session = Session::new();
        let result = session.eval_string("1 + 1").unwrap();
        session.assign_global("_", result);
        let underscore = session.eval_string("_").unwrap();
        assert_eq!(underscore, Value::Int(2));
    }

    #[test]
    fn pipe_into_tilde_uses_unary_not() {
        let mut session = Session::new();
        assert_eq!(session.eval_string("true|~").unwrap(), Value::Bool(false));
        assert_eq!(session.eval_string("1|~").unwrap(), Value::Int(!1));
    }

    #[test]
    fn multiline_symbolic_assignment_is_available_to_next_statement() {
        let mut session = Session::new();
        let result = session
            .eval_string("expr:@s x^2+2*x+1\nexpr")
            .expect("symbolic assignment should bind before the next statement");

        assert_eq!(result.to_string(), "x^2 + 2*x + 1");
        assert!(session.env_vars().contains_key("expr"));
    }

    #[test]
    fn test_reset_session_clears_globals() {
        let mut session = Session::new();
        session.eval_string("a:1").unwrap();
        assert_eq!(session.env_vars().get("a"), Some(&Value::Int(1)));
        session.reset_session();
        assert!(session.env_vars().get("a").is_none());
        // Accessing a after reset should error, not crash with invalid slot
        let result = session.eval_string("a");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("has not been bound to a value"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn test_reset_session_preserves_on_pause() {
        let mut session = Session::new();
        fn dummy_pause(_vm: &mut crate::vm::Vm) {}
        session.set_pause_callback(Some(dummy_pause));
        assert!(session.vm.wqdb.on_pause.is_some());
        session.reset_session();
        assert!(
            session.vm.wqdb.on_pause.is_some(),
            "on_pause callback should survive reset_session"
        );
    }

    #[test]
    fn wqdb_auto_entry_is_armed_once() {
        static PAUSES: AtomicUsize = AtomicUsize::new(0);

        fn count_pause(vm: &mut crate::vm::Vm) {
            PAUSES.fetch_add(1, Ordering::SeqCst);
            vm.dbg_continue();
        }

        PAUSES.store(0, Ordering::SeqCst);
        let mut session = Session::new();
        session.set_pause_callback(Some(count_pause));
        session.set_wqdb(true);

        session.eval_string("x:1").expect("first eval should run");
        assert_eq!(PAUSES.load(Ordering::SeqCst), 1);

        session.eval_string("x+:1").expect("second eval should run");
        assert_eq!(PAUSES.load(Ordering::SeqCst), 1);

        session
            .eval_string("@p x")
            .expect("explicit pause should run");
        assert_eq!(PAUSES.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn evals_long_left_deep_binary_chain() {
        let terms = std::iter::repeat_n("a", 512).collect::<Vec<_>>().join("+");
        let code = format!("a:1\nb:2\nc:{terms}");
        let mut session = Session::new();

        let result = session.eval_string(&code).expect("long chain should eval");

        assert_eq!(result, Value::Int(512));
    }

    #[test]
    fn symbols_handle_long_left_deep_binary_chain() {
        let terms = std::iter::repeat_n("a", 512).collect::<Vec<_>>().join("+");
        let code = format!("a:1\nb:2\nc:{terms}");
        let session = Session::new();

        let index = session
            .analyze_symbols(&code)
            .expect("long chain should produce symbols");
        let a_def = index
            .defs
            .iter()
            .position(|def| {
                def.name == "a" && matches!(def.kind, crate::symbol::DefKind::Assignment)
            })
            .expect("a assignment should be defined");
        let read_count = index
            .occurrences()
            .into_iter()
            .filter(|occurrence| {
                occurrence.def_idx == a_def
                    && matches!(occurrence.kind, crate::symbol::UseKind::Read)
            })
            .count();

        assert_eq!(read_count, 512);
    }

    #[test]
    fn script_directive_cst_round_trips_with_meta_node() {
        let session = Session::new();
        let src = "a:1\n!l ./lib.wq\nb:a\n";
        let (_, green) = session.parse_with_cst(src).expect("script parse");

        assert_eq!(green.text(), src);
        assert!(green.children().iter().any(|child| {
            matches!(
                child,
                GreenChild::Node(node) if node.kind() == crate::cst::SyntaxKind::ScriptDirective
            )
        }));
    }

    #[test]
    fn tokenize_recovery_omits_script_directive_errors() {
        let session = Session::new();
        let tokens = session.tokenize_recovery("a:1\n!l ./lib.wq\nb:2\n");

        assert!(
            !tokens
                .iter()
                .any(|token| matches!(token.token_type, crate::token::TokenType::Error)),
            "directive path should not be lexed as wq code: {tokens:#?}",
        );
    }

    #[test]
    fn script_symbols_keep_offsets_after_directive() {
        let session = Session::new();
        let src = "a:1\n!l ./lib.wq\nb:a\n";
        let index = session
            .analyze_symbols(src)
            .expect("script symbols should analyze");
        let def_idx = index
            .defs
            .iter()
            .position(|def| {
                def.name == "a" && matches!(def.kind, crate::symbol::DefKind::Assignment)
            })
            .expect("a assignment should be defined");
        let use_start = src.rfind('a').expect("source contains a read");

        assert!(index.occurrences().into_iter().any(|occurrence| {
            occurrence.def_idx == def_idx
                && occurrence.span == (use_start, use_start + 1)
                && matches!(occurrence.kind, crate::symbol::UseKind::Read)
        }));
    }

    #[test]
    fn evals_long_left_deep_lazy_bool_chain() {
        let terms = std::iter::once("true")
            .chain(std::iter::repeat_n("missing", 512))
            .collect::<Vec<_>>()
            .join(r"\|");
        let code = format!("c:{terms}");
        let mut session = Session::new();

        let result = session
            .eval_string(&code)
            .expect("long lazy bool chain should eval");

        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_cached_parse_reuses_unchanged_root_statements() {
        let session = Session::new();
        let src = "a:1\nb:2\nc:3\n";
        let (_, first) = session.parse_with_cst(src).expect("initial parse");
        let (_, second) = session
            .parse_with_cst_using_cache(src, &first)
            .expect("cached parse");

        let first_nodes = root_nodes(&first);
        let second_nodes = root_nodes(&second);
        assert_eq!(first_nodes.len(), second_nodes.len());
        assert!(
            first_nodes
                .iter()
                .zip(second_nodes.iter())
                .all(|(lhs, rhs)| lhs.ptr_eq(rhs)),
            "expected unchanged root statements to be reused",
        );
    }

    #[test]
    fn test_cached_parse_reuses_unchanged_prefix_and_suffix_statements() {
        let session = Session::new();
        let old_src = "a:1\nb:2\nc:3\n";
        let new_src = "a:1\nb:20\nc:3\n";
        let (_, first) = session.parse_with_cst(old_src).expect("initial parse");
        let (_, second) = session
            .parse_with_cst_using_cache(new_src, &first)
            .expect("cached parse");

        let first_nodes = root_nodes(&first);
        let second_nodes = root_nodes(&second);
        assert_eq!(first_nodes.len(), second_nodes.len());
        assert!(first_nodes[0].ptr_eq(&second_nodes[0]));
        assert!(!first_nodes[1].ptr_eq(&second_nodes[1]));
        assert!(first_nodes[2].ptr_eq(&second_nodes[2]));
    }
}
