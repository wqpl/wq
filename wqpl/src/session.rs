pub mod dbglog;
pub mod stdio;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ast::AstNode;
use crate::builtins::BuiltinPreset;
use crate::compile::Compiler;
use crate::debugger::{DebugResume, Debugger, PauseEvent};
use crate::interpret::InterpreterKind;
use crate::interpret::profiler::ProfilerInterpreter;
use crate::interpret::sample::SampleInterpreter;
use crate::interpret::vanilla::VanillaInterpreter;
use crate::lex::Lexer;
use crate::parse::resolve::Resolver;
use crate::parse::{Parser, fold};
use crate::script::{ScriptDirective, ScriptItem, ScriptSpan, parse_script_items};
use crate::session::dbglog::DebugLogFlags;
use crate::session::stdio::{WqInputHandle, WqIoError, WqOutputHandle};
use crate::style::{ColorMode, TextStyle, paint};
use crate::token::{fmt_tokens_table, rebase_token};
use crate::value::{Value, WqResult};
use crate::vm::inst::{InstPrettyDumper, Instruction};
use crate::vm::{GlobalMap, PreparedInstructions, Vm};
use crate::wqdb::build::{
    apply_stmt_debug_exact_offs, apply_stmt_spans_exact_offs, mark_stmt_heuristic,
    register_function_chunks,
};
use crate::wqdb::data::{CrashSnapshot, DebugInfo};
use crate::wqdb::{self};
use crate::wqerror::{WqError, WqErrorType};

/// Snapshot of user-defined global bindings owned by a [`Session`].
pub type Bindings = ahash::AHashMap<String, Value>;

/// Thread-safe handle for requesting a controlled stop of a running session.
#[derive(Clone, Debug)]
pub struct SessionInterruptHandle {
    requested: Arc<AtomicBool>,
}

impl SessionInterruptHandle {
    /// Request that the session stop at the next instruction boundary.
    pub fn interrupt(&self) {
        self.requested.store(true, Ordering::Release);
    }
}

fn debug_header_with_color_mode(text: &str, color_mode: ColorMode) -> String {
    paint(text, TextStyle::new().bold().underline(), color_mode)
}

/// Source code and its containing file context for one evaluation.
///
/// A source unit is immutable and scoped to a single [`Session::eval_source`]
/// call. This prevents a file path or byte offset from leaking into a later
/// evaluation on the same session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceUnit<'source> {
    code: &'source str,
    path: &'source str,
    full_text: &'source str,
    base_offset: usize,
}

impl<'source> SourceUnit<'source> {
    /// Create an unnamed evaluator snippet.
    pub fn snippet(code: &'source str) -> Self {
        Self::named("<eval>", code)
    }

    /// Create a complete named source file.
    pub fn named(path: &'source str, code: &'source str) -> Self {
        Self {
            code,
            path,
            full_text: code,
            base_offset: 0,
        }
    }

    /// Create a source fragment whose spans refer to `full_text`.
    pub fn fragment(
        path: &'source str,
        full_text: &'source str,
        span: ScriptSpan,
    ) -> Result<Self, SourceUnitError> {
        if span.start > span.end || span.end > full_text.len() {
            return Err(SourceUnitError::OutOfBounds {
                start: span.start,
                end: span.end,
                source_len: full_text.len(),
            });
        }
        if !full_text.is_char_boundary(span.start) {
            return Err(SourceUnitError::NotUtf8Boundary { index: span.start });
        }
        if !full_text.is_char_boundary(span.end) {
            return Err(SourceUnitError::NotUtf8Boundary { index: span.end });
        }
        Ok(Self {
            code: &full_text[span.as_range()],
            path,
            full_text,
            base_offset: span.start,
        })
    }

    pub fn code(self) -> &'source str {
        self.code
    }

    pub fn path(self) -> &'source str {
        self.path
    }

    pub fn full_text(self) -> &'source str {
        self.full_text
    }

    pub fn base_offset(self) -> usize {
        self.base_offset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluationPhase {
    Lex,
    Parse,
    Compile,
    Execute,
    Host,
}

/// An evaluation error together with the phase and immutable crash state from
/// the same evaluation.
#[derive(Debug, Clone)]
pub struct EvaluationFailure {
    pub error: Box<WqError>,
    pub phase: EvaluationPhase,
    crash: Option<Arc<CrashSnapshot>>,
}

impl EvaluationFailure {
    fn new(mut error: WqError, phase: EvaluationPhase) -> Self {
        let crash = error.take_crash();
        let phase = if error.host_failure {
            EvaluationPhase::Host
        } else {
            phase
        };
        Self {
            error: Box::new(error),
            phase,
            crash,
        }
    }

    pub fn crash(&self) -> Option<&Arc<CrashSnapshot>> {
        self.crash.as_ref()
    }

    /// Return an opaque association that a host can attach when it wraps this
    /// exact evaluation failure in a directive-specific error.
    pub fn postmortem_token(&self) -> Option<PostmortemToken> {
        self.crash.as_ref().map(|crash| PostmortemToken {
            crash: Arc::clone(crash),
        })
    }

    pub fn into_error(self) -> WqError {
        *self.error
    }

    pub fn render_with_color_mode(&self, color_mode: ColorMode, include_crash: bool) -> String {
        let Some(crash) = self.crash.as_ref().filter(|_| include_crash) else {
            return self.error.render_with_color_mode(color_mode);
        };
        let mut error = self.error.clone();
        if crash
            .frames()
            .first()
            .is_some_and(|frame| error_primary_matches_frame(&error, frame))
        {
            error.span = None;
            error.source_ctx = None;
        }

        let mut rendered = error.render_with_color_mode(color_mode);
        for (index, frame) in crash.frames().iter().enumerate() {
            rendered.push('\n');
            rendered.push_str(&wqdb::format_crash_frame(frame, index == 0, color_mode));
        }
        rendered
    }
}

fn error_primary_matches_frame(error: &WqError, frame: &crate::wqdb::data::CrashFrame) -> bool {
    let (Some((start, end)), Some(context)) = (error.span, error.source_ctx.as_deref()) else {
        return false;
    };
    let crate::wqdb::data::CrashFrame::Located {
        source: Some(source),
        ..
    } = frame
    else {
        return false;
    };
    context.path == source.path.as_ref()
        && context.text == source.source.as_ref()
        && start == source.span.start
        && end == source.span.end
}

impl std::ops::Deref for EvaluationFailure {
    type Target = WqError;

    fn deref(&self) -> &Self::Target {
        &self.error
    }
}

impl std::fmt::Display for EvaluationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for EvaluationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.error.as_ref())
    }
}

pub type EvaluationResult<T> = Result<T, EvaluationFailure>;

/// Opaque proof that a host error wraps one exact evaluation crash.
#[derive(Debug, Clone)]
pub struct PostmortemToken {
    crash: Arc<CrashSnapshot>,
}

/// A host directive error with an optional explicit postmortem association.
#[derive(Debug)]
pub struct DirectiveFailure<DirectiveError> {
    error: DirectiveError,
    postmortem: Option<PostmortemToken>,
}

impl<DirectiveError> DirectiveFailure<DirectiveError> {
    pub fn new(error: DirectiveError) -> Self {
        Self {
            error,
            postmortem: None,
        }
    }

    /// Classify an error while it can still be borrowed, then retain only an
    /// opaque token owned by the matching evaluation failure.
    pub fn classify(
        error: DirectiveError,
        classify: impl FnOnce(&DirectiveError) -> Option<PostmortemToken>,
    ) -> Self {
        let postmortem = classify(&error);
        Self { error, postmortem }
    }

    pub fn into_inner(self) -> DirectiveError {
        self.error
    }
}

/// Error returned by [`Session::eval_script_with`] while running code or a
/// host-provided directive.
#[derive(Debug)]
pub enum ScriptRunError<DirectiveError> {
    Evaluation(EvaluationFailure),
    Directive(DirectiveError),
}

impl<DirectiveError: std::fmt::Display> std::fmt::Display for ScriptRunError<DirectiveError> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evaluation(error) => error.fmt(f),
            Self::Directive(error) => error.fmt(f),
        }
    }
}

impl<DirectiveError> std::error::Error for ScriptRunError<DirectiveError>
where
    DirectiveError: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evaluation(error) => Some(error),
            Self::Directive(error) => Some(error),
        }
    }
}

fn rebase_script_span(span: ScriptSpan, base_offset: usize) -> ScriptSpan {
    ScriptSpan {
        start: span.start + base_offset,
        end: span.end + base_offset,
    }
}

fn rebase_script_directive(mut directive: ScriptDirective, base_offset: usize) -> ScriptDirective {
    let span = match &mut directive {
        ScriptDirective::PreludeAlias { span }
        | ScriptDirective::LoadEmbeddedOrFile { span, .. }
        | ScriptDirective::LoadPath { span, .. }
        | ScriptDirective::Unknown { span, .. } => span,
    };
    *span = rebase_script_span(*span, base_offset);
    directive
}

fn script_directive_requires_host(source: SourceUnit<'_>, directive: &ScriptDirective) -> WqError {
    let span = rebase_script_span(directive.span(), source.base_offset);
    WqError::new(WqErrorType::Syntax)
        .src("script directive")
        .msg("script directive requires a host loader")
        .span(Some((span.start, span.end)))
        .source_ctx(source.full_text, source.path)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceUnitError {
    OutOfBounds {
        start: usize,
        end: usize,
        source_len: usize,
    },
    NotUtf8Boundary {
        index: usize,
    },
}

impl std::fmt::Display for SourceUnitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfBounds {
                start,
                end,
                source_len,
            } => write!(
                f,
                "source range {start}..{end} is outside source length {source_len}"
            ),
            Self::NotUtf8Boundary { index } => {
                write!(f, "source range index {index} is not a UTF-8 boundary")
            }
        }
    }
}

impl std::error::Error for SourceUnitError {}

pub struct Session {
    vm: Vm,
    // debug_flags: DebugFlags,
    dry_mode: bool,
    // Arm entering the wqdb on the next eval call
    wqdb_arm_next: bool,
    profiler: ProfilerInterpreter,
}

impl Session {
    /// Create a new evaluator with an empty environment.
    pub fn new() -> Self {
        let mut vm = Vm::new(Vec::new());
        vm.set_backtrace_enabled(true);
        Session {
            vm,
            // debug_flags: DebugFlags::empty(),
            dry_mode: false,
            wqdb_arm_next: false,
            profiler: ProfilerInterpreter::default(),
        }
    }

    /// Configure the input used by the `input` builtin for this session.
    pub fn set_input(&mut self, input: WqInputHandle) {
        self.vm.runtime_io.set_input(input);
    }

    /// Remove the configured program input from this session.
    pub fn clear_input(&mut self) {
        self.vm.runtime_io.clear_input();
    }

    /// Configure program stdout for this session.
    pub fn set_stdout(&mut self, output: WqOutputHandle) {
        self.vm.runtime_io.set_stdout(output);
    }

    /// Configure program stderr and evaluator diagnostics for this session.
    pub fn set_stderr(&mut self, output: WqOutputHandle) {
        self.vm.runtime_io.set_stderr(output);
        self.vm
            .debug_log
            .set_output(self.vm.runtime_io.stderr_output());
    }

    pub fn debug_flags(&self) -> DebugLogFlags {
        self.vm.debug_log.flags()
    }

    pub fn set_debug_flags(&mut self, flags: DebugLogFlags) {
        self.vm.debug_log.set_flags(flags);
    }

    pub fn color_mode(&self) -> ColorMode {
        self.vm.color_mode
    }

    /// Effective color mode for this session's configured stdout.
    pub fn stdout_color_mode(&self) -> ColorMode {
        self.vm.stdout_color_mode()
    }

    /// Effective color mode for this session's configured stderr.
    pub fn stderr_color_mode(&self) -> ColorMode {
        self.vm.stderr_color_mode()
    }

    pub fn set_color_mode(&mut self, color_mode: ColorMode) {
        self.vm.color_mode = color_mode;
    }

    /// Return a snapshot of the current user-defined global bindings.
    pub fn bindings(&self) -> Bindings {
        self.vm.global_env()
    }

    /// Set the command-line arguments exposed to wq code through `argv[]`.
    pub fn set_argv(&mut self, argv: Vec<String>) {
        self.vm.argv = argv.into();
    }

    /// Seed the default generator used by `rand`.
    pub fn seed_rng(&mut self, seed: i64) {
        self.vm.default_rng = crate::value::rng::RngState::from_seed(seed);
    }

    /// Return the status requested by a controlled `cliargs` halt.
    pub fn halt_status(&self) -> Option<i32> {
        self.vm.halt_status
    }

    /// Take the status requested by a controlled `cliargs` halt.
    pub fn take_halt_status(&mut self) -> Option<i32> {
        self.vm.halt_status.take()
    }

    /// Return a handle which can interrupt this session from another thread.
    pub fn interrupt_handle(&self) -> SessionInterruptHandle {
        SessionInterruptHandle {
            requested: Arc::clone(&self.vm.interrupt_requested),
        }
    }

    pub fn is_wqdb_enabled(&self) -> bool {
        self.vm.wqdb.is_enabled()
    }

    /// Clear compiled instructions, transient stacks, and diagnostics while
    /// preserving host configuration and bindings.
    ///
    /// Debug metadata referenced by bound compiled functions is retained so
    /// those functions remain callable after the reset. Breakpoints, stepping,
    /// trackers, and other transient debugger state are cleared.
    pub fn reset_execution_state(&mut self) {
        self.vm
            .reset_with_prepared_instructions(PreparedInstructions::new(Vec::new()));
        self.vm.halt_status = None;
        self.vm.interrupt_requested.store(false, Ordering::Release);
        self.vm.wqdb.reset_execution_state();
        self.vm.current_chunk = None;
        self.vm.runtime_debug_info = false;
        self.vm.debug_src_offset = 0;
        self.vm.last_crash = None;
        self.wqdb_arm_next = self.vm.wqdb.is_enabled();
        self.profiler = ProfilerInterpreter::default();
    }

    /// Clear all user-defined global bindings without changing execution or
    /// host configuration.
    pub fn clear_bindings(&mut self) {
        self.vm.reset_globals();
    }

    /// Reset the interactive workspace while preserving host configuration.
    ///
    /// Input/output handles, debug flags, color mode, interpreter selection,
    /// builtin preset, arguments, RNG state, and pause handler are retained.
    pub fn reset_workspace(&mut self) {
        self.reset_execution_state();
        self.clear_bindings();
        self.vm.debug_info = DebugInfo::default();
        self.vm.current_chunk = None;
    }

    pub fn set_interpreter(&mut self, kind: InterpreterKind) {
        self.vm.interpreter_kind = kind;
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
        self.vm.interpreter_kind.name()
    }

    pub fn set_dry_mode(&mut self, flag: bool) {
        self.dry_mode = flag;
    }

    pub fn dry_mode(&self) -> bool {
        self.dry_mode
    }

    pub fn backtrace_enabled(&self) -> bool {
        self.vm.bt_mode
    }

    pub fn set_backtrace_enabled(&mut self, flag: bool) {
        self.vm.set_backtrace_enabled(flag);
    }

    pub fn set_wqdb(&mut self, flag: bool) {
        self.vm.wqdb.set_enabled(flag);
        if self.vm.wqdb.is_enabled() {
            self.wqdb_arm_next = true;
        } else {
            self.wqdb_arm_next = false;
            self.vm.wqdb.clear_mode();
        }
    }

    /// Install a stateful handler that runs whenever execution pauses.
    ///
    /// The handler receives a constrained debugger facade and is retained by
    /// this session across evaluations. Installing a new handler replaces the
    /// previous one.
    pub fn set_pause_handler<F>(&mut self, handler: F)
    where
        F: for<'vm> FnMut(PauseEvent, &mut Debugger<'vm>) -> DebugResume + 'static,
    {
        self.vm.set_pause_handler(handler);
    }

    pub fn clear_pause_handler(&mut self) {
        self.vm.clear_pause_handler();
    }

    pub fn set_wqdb_batch_cmds(&mut self, cmds: Vec<String>) {
        self.vm.wqdb.replace_batch_commands(cmds);
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

    pub fn debugger(&mut self) -> Debugger<'_> {
        Debugger::new(&mut self.vm)
    }

    fn write_diagnostic(&self, text: impl AsRef<str>) -> WqResult<()> {
        self.vm
            .write_stderr_line(text.as_ref())
            .map_err(|error| host_io_error("evaluator diagnostics", error))
    }

    /// Evaluate a string of source code and return the resulting value.
    pub fn eval_string(&mut self, input: &str) -> EvaluationResult<Value> {
        self.eval_source(SourceUnit::snippet(input))
    }

    /// Evaluate one explicitly scoped source unit.
    pub fn eval_source(&mut self, source: SourceUnit<'_>) -> EvaluationResult<Value> {
        self.vm.begin_evaluation();
        self.vm.debug_log.clear_error();
        let mut phase = EvaluationPhase::Lex;
        let result = self.eval_source_inner(source, &mut phase);
        let result = if let Some(error) = self.vm.debug_log.take_error() {
            Err(EvaluationFailure::new(
                host_io_error("debug output", error),
                EvaluationPhase::Host,
            ))
        } else {
            result.map_err(|error| {
                EvaluationFailure::new(contextualize_source_error(error, source), phase)
            })
        };
        let crash = result
            .as_ref()
            .err()
            .and_then(|failure| failure.crash().cloned());
        self.vm.publish_crash(crash);
        self.vm.end_evaluation();
        result
    }

    /// Evaluate a complete script using the core script-item pipeline.
    ///
    /// Shebangs are ignored and code regions are evaluated in order. Loader
    /// directives require host-specific path and resource resolution, so this
    /// method reports them explicitly instead of silently treating them as wq
    /// code.
    pub fn eval_script(&mut self, source: SourceUnit<'_>) -> EvaluationResult<Value> {
        let items = parse_script_items(source.code);
        if let Some(directive) = items.iter().find_map(|item| match item {
            ScriptItem::Directive(directive) => Some(directive),
            ScriptItem::Shebang { .. } | ScriptItem::Code { .. } => None,
        }) {
            self.vm.begin_evaluation();
            let failure = EvaluationFailure::new(
                script_directive_requires_host(source, directive),
                EvaluationPhase::Parse,
            );
            self.vm.publish_crash(None);
            self.vm.end_evaluation();
            return Err(failure);
        }

        match self.eval_script_with(source, |_, _| -> Result<_, std::convert::Infallible> {
            unreachable!("directives were rejected before evaluation")
        }) {
            Ok(value) => Ok(value.unwrap_or_else(Value::unit)),
            Err(ScriptRunError::Evaluation(error)) => Err(error),
            Err(ScriptRunError::Directive(never)) => match never {},
        }
    }

    /// Evaluate a script while delegating loader directives to the host.
    ///
    /// The session owns shebang handling, code-region source scoping, halt
    /// behavior, and last-value selection. The host callback resolves only
    /// directives and may recursively evaluate scripts with the same session.
    /// Returning `Some(value)` makes that directive contribute to the script's
    /// last value. An empty script returns `Ok(None)`.
    pub fn eval_script_with<DirectiveError>(
        &mut self,
        source: SourceUnit<'_>,
        mut handle_directive: impl FnMut(
            &mut Session,
            ScriptDirective,
        ) -> Result<Option<Value>, DirectiveError>,
    ) -> Result<Option<Value>, ScriptRunError<DirectiveError>> {
        self.eval_script_with_postmortem(source, |session, directive| {
            handle_directive(session, directive).map_err(DirectiveFailure::new)
        })
    }

    /// Evaluate a host-driven script while allowing directive errors to carry
    /// an explicit association with an evaluation failure they wrap.
    pub fn eval_script_with_postmortem<DirectiveError>(
        &mut self,
        source: SourceUnit<'_>,
        mut handle_directive: impl FnMut(
            &mut Session,
            ScriptDirective,
        )
            -> Result<Option<Value>, DirectiveFailure<DirectiveError>>,
    ) -> Result<Option<Value>, ScriptRunError<DirectiveError>> {
        self.vm.begin_evaluation();
        let items = parse_script_items(source.code);
        let result = self.eval_script_items_with(source, items, &mut handle_directive);
        let crash = match &result {
            Err(ScriptRunError::Evaluation(failure)) => failure.crash().cloned(),
            Err(ScriptRunError::Directive(failure)) => failure
                .postmortem
                .as_ref()
                .filter(|token| {
                    self.vm
                        .last_crash
                        .as_ref()
                        .is_some_and(|current| Arc::ptr_eq(current, &token.crash))
                })
                .map(|token| Arc::clone(&token.crash)),
            Ok(_) => None,
        };
        self.vm.publish_crash(crash);
        self.vm.end_evaluation();
        result.map_err(|error| match error {
            ScriptRunError::Evaluation(failure) => ScriptRunError::Evaluation(failure),
            ScriptRunError::Directive(failure) => ScriptRunError::Directive(failure.into_inner()),
        })
    }

    fn eval_script_items_with<DirectiveError>(
        &mut self,
        source: SourceUnit<'_>,
        items: Vec<ScriptItem>,
        mut handle_directive: impl FnMut(
            &mut Session,
            ScriptDirective,
        )
            -> Result<Option<Value>, DirectiveFailure<DirectiveError>>,
    ) -> Result<Option<Value>, ScriptRunError<DirectiveFailure<DirectiveError>>> {
        let mut last_value = None;
        for item in items {
            if self.halt_status().is_some() {
                break;
            }
            match item {
                ScriptItem::Shebang { .. } => {}
                ScriptItem::Code { span } => {
                    let absolute_span = rebase_script_span(span, source.base_offset);
                    let fragment =
                        SourceUnit::fragment(source.path, source.full_text, absolute_span)
                            .expect("script parser should yield valid source spans");
                    if !fragment.code.trim().is_empty() {
                        last_value = Some(
                            self.eval_source(fragment)
                                .map_err(ScriptRunError::Evaluation)?,
                        );
                    }
                }
                ScriptItem::Directive(directive) => {
                    let directive = rebase_script_directive(directive, source.base_offset);
                    if let Some(value) =
                        handle_directive(self, directive).map_err(ScriptRunError::Directive)?
                    {
                        last_value = Some(value);
                    }
                }
            }
        }
        Ok(last_value)
    }

    fn eval_source_inner(
        &mut self,
        source: SourceUnit<'_>,
        phase: &mut EvaluationPhase,
    ) -> WqResult<Value> {
        let input = source.code;
        *phase = EvaluationPhase::Lex;
        // If a wqdb entry was armed, record it for the upcoming run.
        let mut lexer = Lexer::new(input).with_ctx(source.full_text, source.base_offset);
        lexer.set_source_path(source.path.to_string());
        let tokens = lexer.tokenize()?;
        if self.vm.debug_log.enabled(DebugLogFlags::TOKEN) {
            let mut display_tokens = tokens.clone();
            if source.base_offset != 0 {
                for token in &mut display_tokens {
                    rebase_token(token, source.full_text, source.base_offset);
                }
            }
            self.write_diagnostic(debug_header_with_color_mode(
                "TOKEN",
                self.vm.stderr_color_mode(),
            ))?;
            self.write_diagnostic(fmt_tokens_table(&display_tokens))?;
            self.write_diagnostic("")?;
        }

        *phase = EvaluationPhase::Parse;
        let builtins = self.vm.builtins.clone();
        let mut parser = Parser::new_with_builtins(tokens, input.to_string(), builtins.clone());
        parser.set_source_path(source.path.to_string());
        let dump_cst = self.vm.debug_log.enabled(DebugLogFlags::CST);
        if dump_cst {
            parser.enable_cst();
        }
        let ast_src = source.full_text;

        let ast = parser.parse()?;
        if let Some(eof_err) = parser.eof_error() {
            return Err(eof_err.clone());
        }
        if dump_cst {
            let cst = parser
                .take_cst()
                .expect("enable_cst was just called, so take_cst yields Some");
            self.write_diagnostic(debug_header_with_color_mode(
                "CST",
                self.vm.stderr_color_mode(),
            ))?;
            self.write_diagnostic(crate::cst::SyntaxNode::new_root(cst).pretty_print())?;
            self.write_diagnostic("")?;
        }

        let dump_ast = |s: &str, ast: &AstNode, flag: u16| -> WqResult<()> {
            if self.vm.debug_log.enabled(flag) {
                let mut display_ast = ast.clone();
                Parser::offset_spans(&mut display_ast, source.base_offset);
                self.write_diagnostic(debug_header_with_color_mode(
                    s,
                    self.vm.stderr_color_mode(),
                ))?;
                self.write_diagnostic(display_ast.sexpr_pretty_with_source(ast_src))?;
                self.write_diagnostic("")?;
            }
            Ok(())
        };

        dump_ast("AST - original", &ast, DebugLogFlags::AST_VERBOSE)?;

        let env = self.environment();
        let mut resolver = Resolver::from_env(env.clone(), builtins.clone());
        let ast = resolver.resolve(ast);
        dump_ast("AST @ resolve", &ast, DebugLogFlags::AST_VERBOSE)?;

        let ast = fold::fold(ast);
        dump_ast("AST @ fold - final", &ast, DebugLogFlags::AST)?;

        *phase = EvaluationPhase::Compile;
        let mut compiler = Compiler::new_with_builtins(builtins);
        compiler.set_fn_spans(parser.fn_body_spans_all().clone());
        compiler.set_source(source.full_text.to_string());
        compiler.set_source_base_offset(source.base_offset);
        compiler.set_source_path(source.path.to_string());
        compiler.set_stmt_spans(parser.stmt_spans_top().to_vec());
        compiler.compile(&ast)?;
        compiler.instructions.push(Instruction::Return);

        let dump_inst = |s: &str, inst: &[Instruction], flag: u16| -> WqResult<()> {
            if self.vm.debug_log.enabled(flag) {
                let color_mode = self.vm.stderr_color_mode();
                self.write_diagnostic(debug_header_with_color_mode(s, color_mode))?;
                let lines = InstPrettyDumper::new(true, color_mode.should_colorize())
                    .with_pc()
                    .render(inst);
                for line in lines {
                    self.write_diagnostic(line)?;
                }
                self.write_diagnostic("")?;
            }
            Ok(())
        };

        dump_inst(
            "Inst - original",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        )?;

        compiler.propagate_constants_with_globals(&env);
        dump_inst(
            "Inst @ constprop",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        )?;

        compiler.rewrite_tail_calls();
        dump_inst(
            "Inst @ tailcall",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        )?;

        compiler.fuse();
        dump_inst(
            "Inst @ fuse",
            &compiler.instructions,
            DebugLogFlags::INST_VERBOSE,
        )?;
        let prepared_instructions = PreparedInstructions::with_owned_const_extraction(
            std::mem::take(&mut compiler.instructions),
        );
        dump_inst(
            "Inst @ owned consts - final",
            prepared_instructions.instructions(),
            DebugLogFlags::INST,
        )?;

        if self.dry_mode {
            return Ok(Value::unit());
        }

        *phase = EvaluationPhase::Execute;
        self.vm.set_runtime_debug_info(compiler.has_runtime_debug);
        self.vm
            .reset_with_prepared_instructions(prepared_instructions);
        // Prepare debug artifacts when wqdb or backtrace mode is on
        let temp_wqdb_on = if self.wqdb_arm_next {
            self.wqdb_arm_next = false;
            true
        } else {
            false
        };

        if self.vm.debug_artifacts_enabled() || temp_wqdb_on {
            // Prepare debug mapping for this top-level script
            self.vm.script_prepare_debug(source.path, source.full_text);
            // Set base offset into the source file for this snippet
            self.vm.set_debug_src_offset(source.base_offset);
            // Mark statements using a combination of parser spans and heuristics
            {
                let chunk = self.vm.expect_current_chunk();
                // Compute file_id first to avoid borrow conflicts
                let file_id = self.vm.debug_info.expect_chunk(chunk).file_id;
                // First mark all likely statement PCs
                {
                    let code = &self.vm.instructions;
                    let line_table = &mut self.vm.debug_info.expect_chunk_mut(chunk).line_table;
                    if !compiler.dbg_pc_spans.is_empty() && !compiler.dbg_stmt_marks.is_empty() {
                        let mut pc_spans = compiler.dbg_pc_spans.clone();
                        pc_spans.resize(code.len(), None);
                        let spans = apply_stmt_debug_exact_offs(
                            line_table,
                            file_id,
                            &pc_spans,
                            &compiler.dbg_stmt_marks,
                            source.base_offset,
                            Some(&self.vm.debug_log),
                        );
                        self.vm
                            .debug_info
                            .expect_chunk_mut(chunk)
                            .note_debug_spans(spans.0, spans.1);
                    } else {
                        mark_stmt_heuristic(line_table, code, Some(&self.vm.debug_log));
                        // Overlay exact mapping for top-level spans across candidates
                        let has_real = apply_stmt_spans_exact_offs(
                            line_table,
                            code,
                            file_id,
                            parser.stmt_spans_top(),
                            source.base_offset,
                            Some(&self.vm.debug_log),
                        );
                        self.vm
                            .debug_info
                            .expect_chunk_mut(chunk)
                            .note_debug_spans(false, has_real);
                    }
                }
                // Recursively register chunks for nested non-capturing functions
                let instructions = std::sync::Arc::make_mut(&mut self.vm.instructions);
                register_function_chunks(
                    &mut self.vm.debug_info,
                    file_id,
                    instructions,
                    source.base_offset,
                    Some(&self.vm.debug_log),
                );
            }
            self.vm.wqdb.resolve_source_breakpoints(&self.vm.debug_info);
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
        // A pause handler is configured independently through set_pause_handler().
        if temp_wqdb_on {
            self.vm.dbg_step_in();
        }
        let result = match self.vm.interpreter_kind {
            InterpreterKind::Sample => {
                let mut sample = SampleInterpreter::default();
                self.vm.run_with_interpreter(&mut sample)
            }
            InterpreterKind::Vanilla => self.vm.run_with_interpreter(&mut VanillaInterpreter),
            InterpreterKind::Profiler => {
                let result = self.vm.run_with_interpreter(&mut self.profiler);
                self.profiler
                    .finish_report(self.vm.stderr_color_mode(), &self.vm.debug_log);
                result
            }
        };
        if self.vm.debug_log.enabled(DebugLogFlags::VALUE)
            && let Ok(v) = &result
        {
            self.write_diagnostic(format!("{v:?}"))?;
        }
        result
    }

    /// Build a snapshot of the environment from slots.
    fn environment(&self) -> GlobalMap {
        self.vm.global_env()
    }

    /// Enter wqdb at the start of the next evaluation when it is enabled.
    pub fn arm_wqdb_next(&mut self) {
        if self.vm.wqdb.is_enabled() {
            self.wqdb_arm_next = true;
        }
    }

    pub fn postmortem_available(&self, failure: &EvaluationFailure) -> bool {
        let (Some(failure_crash), Some(session_crash)) =
            (failure.crash(), self.vm.last_crash.as_ref())
        else {
            return false;
        };
        Arc::ptr_eq(failure_crash, session_crash)
    }

    pub fn postmortem_debugger(&mut self, failure: &EvaluationFailure) -> Option<Debugger<'_>> {
        if !self.postmortem_available(failure) {
            return None;
        }
        Some(Debugger::new(&mut self.vm))
    }

    /// Assign a value to a global variable by name via the slot-based global
    /// table.
    pub fn assign_global(&mut self, name: &str, value: Value) {
        self.vm.assign_global_and_slot(name, value);
    }
}

fn host_io_error(source: &str, error: WqIoError) -> WqError {
    WqError::new(WqErrorType::Io)
        .src(source)
        .host_failure()
        .attach_note(format!("host I/O error: {error}"))
}

fn contextualize_source_error(mut error: WqError, source: SourceUnit<'_>) -> WqError {
    if let Some(context) = error.source_ctx.as_deref() {
        if context.path == source.path && context.text == source.full_text {
            return error;
        }
        if context.path != source.path || context.text != source.code {
            return error;
        }
    }
    if let Some((start, end)) = &mut error.span {
        *start = start.saturating_add(source.base_offset);
        *end = end.saturating_add(source.base_offset);
    }
    error.source_ctx = Some(Box::new(crate::wqerror::SourceCtx {
        text: source.full_text.to_string(),
        path: source.path.to_string(),
    }));
    error
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::debugger::{DebugResume, PauseReason};

    #[test]
    fn debug_header_renders_with_explicit_color_mode() {
        assert_eq!(
            debug_header_with_color_mode("TOKEN", ColorMode::Never),
            "TOKEN"
        );
        assert_eq!(
            debug_header_with_color_mode("TOKEN", ColorMode::Always),
            "\x1b[1;4mTOKEN\x1b[0m"
        );
    }

    #[test]
    fn test_assign_global_new_var() {
        let mut session = Session::new();
        session.assign_global("x", Value::Int(42));
        assert_eq!(session.bindings().get("x"), Some(&Value::Int(42)));
    }

    #[test]
    fn test_assign_global_overwrite() {
        let mut session = Session::new();
        session.assign_global("x", Value::Int(1));
        session.assign_global("x", Value::Int(2));
        assert_eq!(session.bindings().get("x"), Some(&Value::Int(2)));
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
    fn discarded_map_preserves_callback_side_effects() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{n:0;til 4|M{[x]'n+:x};n};f[]")
            .expect("discarded map should eval");

        assert_eq!(result, Value::Int(6));
    }

    #[test]
    fn discarded_apply_preserves_callback_side_effects() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{n:0;apply[({[x]'n+:x};{[x]'n+:x*2});3];n};f[]")
            .expect("discarded apply should eval");

        assert_eq!(result, Value::Int(9));
    }

    #[test]
    fn discarded_filter_preserves_callback_side_effects() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{n:0;filter[(1;2;3);{[x]'n+:x;x%2=1}];n};f[]")
            .expect("discarded filter should eval");

        assert_eq!(result, Value::Int(6));
    }

    #[test]
    fn named_forced_call_accepts_lifted_callable() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{x+1};g:f+1;g[2;]")
            .expect("forced call should eval a lifted callable");

        assert_eq!(result, Value::Int(4));
    }

    #[test]
    fn runtime_seeded_lifted_callable_lowers_to_call() {
        let mut session = Session::new();
        session
            .eval_string("f:{x+1};g:f+1")
            .expect("lifted callable binding should eval");
        let result = session
            .eval_string("g[2]")
            .expect("runtime-seeded lifted callable should eval");

        assert_eq!(result, Value::Int(4));
    }

    #[test]
    fn symbolic_expression_positional_call_infers_single_var() {
        let mut session = Session::new();
        let result = session
            .eval_string("(@s x^2+2*x+1)[3]")
            .expect("symbolic expression should accept one positional arg");

        assert_eq!(result, Value::Int(16));
    }

    #[test]
    fn symbolic_binding_can_be_called_by_name() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:@s x^2;f[4]")
            .expect("bound symbolic expression should be callable");

        assert_eq!(result, Value::Int(16));
    }

    #[test]
    fn mixed_lifted_callable_calls_symbolic_operand() {
        let mut session = Session::new();
        let result = session
            .eval_string("a:{x^2}+@s 2x;a[2]")
            .expect("mixed function and symbolic expression should eval");

        assert_eq!(result, Value::Int(8));
    }

    #[test]
    fn symbolic_expression_call_combines_named_and_positional_bindings() {
        let mut session = Session::new();
        let result = session
            .eval_string("(@s x*y+y)[3;`y:2]")
            .expect("symbolic expression should bind named args before positional arg");

        assert_eq!(result, Value::Int(8));
    }

    #[test]
    fn symbolic_expression_can_be_used_as_map_callback() {
        let mut session = Session::new();
        let result = session
            .eval_string("map[(1;2;3);@s x^2]")
            .expect("map should accept symbolic expression as callback");

        assert_eq!(result, Value::IntList(Arc::new(vec![1, 4, 9])));
    }

    #[test]
    fn index_path_assignment_updates_nested_value() {
        let mut session = Session::new();
        let result = session
            .eval_string("lst:((0;0);(0;0));lst[0][0]:1;lst")
            .expect("deep index assignment should eval");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1, 0])),
                Value::IntList(Arc::new(vec![0, 0])),
            ]))
        );
    }

    #[test]
    fn index_path_assignment_keeps_final_bulk_semantics() {
        let mut session = Session::new();
        let result = session
            .eval_string("lst:((0;0);(0;0));lst[0][0;1]:(2;3);lst")
            .expect("deep bulk index assignment should eval");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![2, 3])),
                Value::IntList(Arc::new(vec![0, 0])),
            ]))
        );
    }

    #[test]
    fn index_path_assignment_keeps_top_level_bulk_semantics() {
        let mut session = Session::new();
        let result = session
            .eval_string("lst:((0;0);(0;0));lst[0;1]:(0;1);lst")
            .expect("top-level bulk index assignment should eval");

        assert_eq!(result, Value::IntList(Arc::new(vec![0, 1])));
    }

    #[test]
    fn index_path_update_reads_and_writes_nested_value() {
        let mut session = Session::new();
        let result = session
            .eval_string("lst:((1;2);(3;4));lst[1][0]+:10;lst")
            .expect("deep augmented index assignment should eval");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1, 2])),
                Value::IntList(Arc::new(vec![13, 4])),
            ]))
        );
    }

    #[test]
    fn index_path_update_handles_outer_variable() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{m:((1;2);(3;4));g:{'m[0][1]+:10};g[];m};f[]")
            .expect("deep assignment through an outer variable should eval");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1, 12])),
                Value::IntList(Arc::new(vec![3, 4])),
            ]))
        );
    }

    #[test]
    fn whitespace_after_bracket_assigns_through_index_path() {
        let mut session = Session::new();
        let result = session
            .eval_string("lst:((0;0);(0;0));lst[0] 0:1;lst")
            .expect("whitespace final segment should assign through the path");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![1, 0])),
                Value::IntList(Arc::new(vec![0, 0])),
            ]))
        );
    }

    #[test]
    fn whitespace_postfix_stays_single_final_path_segment() {
        let mut session = Session::new();
        let result = session
            .eval_string("lst:(((0;0);(0;0));((0;0);(0;0)));lst[0][1](,0) 0:9;lst")
            .expect("whitespace postfix expression should be the final path segment");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::List(Arc::new(vec![
                    Value::IntList(Arc::new(vec![0, 0])),
                    Value::IntList(Arc::new(vec![9, 0])),
                ])),
                Value::List(Arc::new(vec![
                    Value::IntList(Arc::new(vec![0, 0])),
                    Value::IntList(Arc::new(vec![0, 0])),
                ])),
            ]))
        );
    }

    #[test]
    fn index_path_assignment_rejects_bulk_prefix() {
        let mut session = Session::new();
        let err = session
            .eval_string("lst:((0;0);(0;0));lst[0;1][0]:1")
            .expect_err("bulk prefix should not become deep assignment");

        assert!(
            err.to_string()
                .contains("bulk index cannot appear before the final path segment"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn index_path_assignment_rejects_dynamic_bulk_prefix() {
        let mut session = Session::new();
        let err = session
            .eval_string("i:(0;1);lst:((0;0);(0;0));lst[i][0]:9")
            .expect_err("dynamic bulk prefix should not become deep assignment");

        assert!(
            err.to_string()
                .contains("bulk index cannot appear before the final path segment"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn multiline_symbolic_assignment_is_available_to_next_statement() {
        let mut session = Session::new();
        let result = session
            .eval_string("expr:@s x^2+2*x+1\nexpr")
            .expect("symbolic assignment should bind before the next statement");

        assert_eq!(result.to_string(), "x^2 + 2*x + 1");
        assert!(session.bindings().contains_key("expr"));
    }

    #[test]
    fn reset_workspace_clears_globals() {
        let mut session = Session::new();
        session.eval_string("a:1").unwrap();
        assert_eq!(session.bindings().get("a"), Some(&Value::Int(1)));
        session.reset_workspace();
        assert!(session.bindings().get("a").is_none());
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
    fn reset_workspace_preserves_pause_handler() {
        let pauses = Arc::new(AtomicUsize::new(0));
        let captured_pauses = Arc::clone(&pauses);
        let mut session = Session::new();
        session.set_pause_handler(move |_, _| {
            captured_pauses.fetch_add(1, Ordering::SeqCst);
            DebugResume::Continue
        });
        session.reset_workspace();
        session.set_wqdb(true);
        session.eval_string("1").expect("evaluation after reset");

        assert_eq!(pauses.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn wqdb_auto_entry_is_armed_once() {
        let reasons = Arc::new(Mutex::new(Vec::new()));
        let captured_reasons = Arc::clone(&reasons);
        let mut session = Session::new();
        session.set_pause_handler(move |event, _| {
            captured_reasons
                .lock()
                .expect("pause reasons lock")
                .push(event.reason);
            DebugResume::Continue
        });
        session.set_wqdb(true);

        session.eval_string("x:1").expect("first eval should run");
        assert_eq!(reasons.lock().expect("pause reasons lock").len(), 1);

        session.eval_string("x+:1").expect("second eval should run");
        assert_eq!(reasons.lock().expect("pause reasons lock").len(), 1);

        session
            .eval_string("@p x")
            .expect("explicit pause should run");
        let reasons = reasons.lock().expect("pause reasons lock");
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0], PauseReason::Entry);
        assert!(matches!(reasons[1], PauseReason::ExplicitPause { .. }));
    }

    #[test]
    fn pending_source_breakpoint_resolves_in_a_later_script_region() {
        let reasons = Arc::new(Mutex::new(Vec::new()));
        let captured_reasons = Arc::clone(&reasons);
        let resolved = Arc::new(AtomicUsize::new(0));
        let captured_resolved = Arc::clone(&resolved);
        let mut session = Session::new();
        session.set_pause_handler(move |event, debugger| {
            captured_reasons
                .lock()
                .expect("pause reasons lock")
                .push(event.reason);
            if event.reason == PauseReason::Entry {
                let breakpoints = debugger.set_source_breakpoints("pending.wq", &[3]);
                assert_eq!(breakpoints.len(), 1);
                assert!(breakpoints[0].location.is_none());
            } else if matches!(event.reason, PauseReason::Breakpoint { .. }) {
                let breakpoints = debugger.take_resolved_source_breakpoints();
                assert_eq!(breakpoints.len(), 1);
                assert!(breakpoints[0].location.is_some());
                captured_resolved.fetch_add(1, Ordering::SeqCst);
            }
            DebugResume::Continue
        });
        session.set_wqdb(true);

        session
            .eval_script_with(
                SourceUnit::named("pending.wq", "first:1\n\\p\nsecond:2\n"),
                |_, _| Ok::<_, ()>(None),
            )
            .expect("streamed script should evaluate");

        let reasons = reasons.lock().expect("pause reasons lock");
        assert_eq!(reasons.len(), 2);
        assert_eq!(reasons[0], PauseReason::Entry);
        assert!(matches!(reasons[1], PauseReason::Breakpoint { .. }));
        assert_eq!(resolved.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn interrupt_handle_requests_a_controlled_halt() {
        let mut session = Session::new();
        session.interrupt_handle().interrupt();

        session
            .eval_string("W[true;0]")
            .expect("an interrupted evaluation should halt cleanly");

        assert_eq!(session.take_halt_status(), Some(0));
        assert_eq!(
            session
                .eval_string("1")
                .expect("a consumed interrupt should not affect the next evaluation"),
            Value::Int(1)
        );
    }

    #[test]
    fn disabling_wqdb_cancels_pending_auto_entry() {
        let pauses = Arc::new(AtomicUsize::new(0));
        let captured_pauses = Arc::clone(&pauses);
        let mut session = Session::new();
        session.set_pause_handler(move |_, _| {
            captured_pauses.fetch_add(1, Ordering::SeqCst);
            DebugResume::Continue
        });
        session.set_backtrace_enabled(false);
        session.set_wqdb(true);
        session.set_wqdb(false);

        session.eval_string("1").expect("evaluation should run");

        assert_eq!(pauses.load(Ordering::SeqCst), 0);
        assert!(
            session
                .vm
                .debug_info
                .get_chunk(crate::wqdb::data::ChunkId(0))
                .is_none()
        );
    }

    #[test]
    fn enabled_wqdb_can_be_rearmed_for_another_eval() {
        let pauses = Arc::new(AtomicUsize::new(0));
        let captured_pauses = Arc::clone(&pauses);
        let mut session = Session::new();
        session.set_pause_handler(move |_, _| {
            captured_pauses.fetch_add(1, Ordering::SeqCst);
            DebugResume::Continue
        });
        session.set_wqdb(true);

        session.eval_string("x:1").expect("first eval should run");
        session.arm_wqdb_next();
        session.eval_string("x+:1").expect("second eval should run");

        assert_eq!(pauses.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn global_function_aliases_share_a_breakpointable_chunk() {
        let mut session = Session::new();

        session
            .eval_string("f:{x+1};g:f")
            .expect("function aliases should evaluate");

        let f = session
            .vm
            .debug_info
            .function_chunk("f")
            .expect("f function chunk");
        let g = session
            .vm
            .debug_info
            .function_chunk("g")
            .expect("g function chunk");
        assert_eq!(f, g);
        assert!(session.vm.debug_info.function_chunk("<fn>").is_none());
    }

    #[test]
    fn returned_closure_is_registered_under_its_global_name() {
        let mut session = Session::new();

        session
            .eval_string("factory:{a:1;{x+a}};f:factory[]")
            .expect("returned closure should evaluate");

        assert!(session.vm.debug_info.function_chunk("f").is_some());
        assert!(session.vm.debug_info.function_chunk("<fn>").is_none());
    }

    #[test]
    fn overwriting_function_global_only_removes_that_breakpoint_name() {
        let mut session = Session::new();
        session
            .eval_string("f:{x+1};g:f")
            .expect("function aliases should evaluate");

        session
            .eval_string("f:1")
            .expect("function overwrite should evaluate");

        assert!(session.vm.debug_info.function_chunk("f").is_none());
        assert!(session.vm.debug_info.function_chunk("g").is_some());
    }

    #[test]
    fn wqdb_breakpoints_disable_pure_callback_fast_path() {
        let pauses = Arc::new(AtomicUsize::new(0));
        let captured_pauses = Arc::clone(&pauses);
        let mut session = Session::new();
        session.set_pause_handler(move |_, debugger| {
            let pause = captured_pauses.fetch_add(1, Ordering::SeqCst);
            if pause == 0 {
                let chunk = debugger
                    .debug_info()
                    .function_chunk("f")
                    .expect("f function chunk");
                let meta = debugger.debug_info().expect_chunk(chunk);
                let pc = (0..meta.len)
                    .find(|pc| meta.line_table.is_stmt(*pc))
                    .unwrap_or(0);
                debugger.set_breakpoint(crate::wqdb::data::CodeLoc { chunk, pc });
            }
            DebugResume::Continue
        });
        session.set_wqdb(true);

        let result = session
            .eval_string("f:{x+1};map[(1;2);f]")
            .expect("debugged map should evaluate");

        assert_eq!(result, Value::IntList(Arc::new(vec![2, 3])));
        assert_eq!(pauses.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn instruction_step_on_pause_instruction_does_not_stop_twice_at_that_pc() {
        let pauses = Arc::new(AtomicUsize::new(0));
        let captured_pauses = Arc::clone(&pauses);
        let mut session = Session::new();
        session.set_pause_handler(move |_, debugger| {
            let stop = captured_pauses.fetch_add(1, Ordering::SeqCst) + 1;
            if stop == 1 {
                debugger.set_step_granularity(crate::wqdb::model::StepGranularity::Inst);
                DebugResume::StepIn
            } else {
                DebugResume::Continue
            }
        });
        session.set_wqdb(true);

        session
            .eval_string("@p 1")
            .expect("pause expression should run");

        assert_eq!(pauses.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn line_stepping_uses_compiled_lines_and_revisits_loop_lines() {
        let lines = Arc::new(Mutex::new(Vec::new()));
        let captured_lines = Arc::clone(&lines);
        let mut session = Session::new();
        session.set_pause_handler(move |event, debugger| {
            let loc = event.location;
            let meta = debugger.debug_info().expect_chunk(loc.chunk);
            let span = meta.line_table.context_span_at(loc.pc);
            if let Some(source) = debugger.debug_info().file(span.file_id) {
                captured_lines
                    .lock()
                    .expect("line stops lock")
                    .push(source.line_col(span.start).0);
            }
            debugger.set_step_granularity(crate::wqdb::model::StepGranularity::Line);
            DebugResume::StepIn
        });
        session.set_wqdb(true);

        let result = session
            .eval_string("i:0;a:1\nW[i<2;\n  i+:1]\ni")
            .expect("line stepping program should run");

        assert_eq!(result, Value::Int(2));
        assert_eq!(
            *lines.lock().expect("line stops lock"),
            vec![1, 2, 3, 2, 3, 2, 4]
        );
    }

    #[test]
    fn instruction_next_and_finish_cross_real_call_frames() {
        let phase = Arc::new(AtomicUsize::new(0));
        let targets = Arc::new(Mutex::new(Vec::new()));
        let captured_phase = Arc::clone(&phase);
        let captured_targets = Arc::clone(&targets);
        let mut session = Session::new();
        session.set_pause_handler(move |event, debugger| {
            debugger.set_step_granularity(crate::wqdb::model::StepGranularity::Inst);
            let loc = event.location;
            let name = debugger.function_name(loc.chunk);
            let instruction = debugger.instruction_at(loc.pc).unwrap_or_default();
            match captured_phase.load(Ordering::SeqCst) {
                0 if name == "g" && instruction.starts_with("Postfix(1)") => {
                    captured_phase.store(1, Ordering::SeqCst);
                    DebugResume::StepOver
                }
                1 => {
                    captured_targets
                        .lock()
                        .expect("call targets lock")
                        .push(format!("{name}:{instruction}"));
                    captured_phase.store(2, Ordering::SeqCst);
                    DebugResume::StepIn
                }
                2 if name == "g" && instruction.starts_with("Postfix(1)") => {
                    captured_phase.store(3, Ordering::SeqCst);
                    DebugResume::StepIn
                }
                3 => {
                    captured_targets
                        .lock()
                        .expect("call targets lock")
                        .push(format!("{name}:{instruction}"));
                    captured_phase.store(4, Ordering::SeqCst);
                    DebugResume::StepOut
                }
                4 => {
                    captured_targets
                        .lock()
                        .expect("call targets lock")
                        .push(format!("{name}:{instruction}"));
                    captured_phase.store(5, Ordering::SeqCst);
                    DebugResume::Continue
                }
                _ => DebugResume::StepIn,
            }
        });
        session.set_wqdb(true);

        let result = session
            .eval_string("f:{x+1};g:{y:f x;y+1};a:g 3;b:g 4;b")
            .expect("call stepping program should run");

        assert_eq!(result, Value::Int(6));
        assert_eq!(phase.load(Ordering::SeqCst), 5);
        let targets = targets.lock().expect("call targets lock");
        assert!(targets[0].starts_with("g:StoreLocal"), "{targets:?}");
        assert!(targets[1].starts_with("f:BinaryOp"), "{targets:?}");
        assert!(targets[2].starts_with("g:StoreLocal"), "{targets:?}");
    }

    #[test]
    fn instruction_next_steps_over_tail_called_frames() {
        let phase = Arc::new(AtomicUsize::new(0));
        let target = Arc::new(Mutex::new(None));
        let captured_phase = Arc::clone(&phase);
        let captured_target = Arc::clone(&target);
        let mut session = Session::new();
        session.set_pause_handler(move |event, debugger| {
            debugger.set_step_granularity(crate::wqdb::model::StepGranularity::Inst);
            let loc = event.location;
            let name = debugger.function_name(loc.chunk);
            let instruction = debugger.instruction_at(loc.pc).unwrap_or_default();
            if captured_phase.load(Ordering::SeqCst) == 0 {
                if name == "g" && instruction.starts_with("TailPostfix(1)") {
                    captured_phase.store(1, Ordering::SeqCst);
                    DebugResume::StepOver
                } else {
                    DebugResume::StepIn
                }
            } else {
                *captured_target.lock().expect("tail target lock") =
                    Some(format!("{name}:{instruction}"));
                captured_phase.store(2, Ordering::SeqCst);
                DebugResume::Continue
            }
        });
        session.set_wqdb(true);

        let result = session
            .eval_string("f:{x+1};g:{f x};a:g 3;a")
            .expect("tail-call stepping program should run");

        assert_eq!(result, Value::Int(4));
        assert_eq!(phase.load(Ordering::SeqCst), 2);
        let target = target
            .lock()
            .expect("tail target lock")
            .clone()
            .expect("step-over target");
        assert!(target.starts_with("<script>:"), "target was {target}");
    }

    #[test]
    fn instruction_next_steps_over_tail_calls_beyond_backtrace_capacity() {
        let tail_calls = Arc::new(AtomicUsize::new(0));
        let phase = Arc::new(AtomicUsize::new(0));
        let target = Arc::new(Mutex::new(None));
        let captured_tail_calls = Arc::clone(&tail_calls);
        let captured_phase = Arc::clone(&phase);
        let captured_target = Arc::clone(&target);
        let mut session = Session::new();
        session.set_pause_handler(move |event, debugger| {
            debugger.set_step_granularity(crate::wqdb::model::StepGranularity::Inst);
            let loc = event.location;
            let name = debugger.function_name(loc.chunk);
            let instruction = debugger.instruction_at(loc.pc).unwrap_or_default();
            if captured_phase.load(Ordering::SeqCst) == 0 {
                if name == "f" && instruction.starts_with("TailPostfix(1)") {
                    let tail_call = captured_tail_calls.fetch_add(1, Ordering::SeqCst) + 1;
                    if tail_call == 129 {
                        captured_phase.store(1, Ordering::SeqCst);
                        return DebugResume::StepOver;
                    }
                }
                DebugResume::StepIn
            } else {
                *captured_target.lock().expect("deep tail target lock") =
                    Some(format!("{name}:{instruction}"));
                captured_phase.store(2, Ordering::SeqCst);
                DebugResume::Continue
            }
        });
        session.set_wqdb(true);

        let result = session
            .eval_string("f:{[n]$[n=0;0;f[n-1]]};a:f 130;a")
            .expect("deep tail-call stepping program should run");

        assert_eq!(result, Value::Int(0));
        assert_eq!(phase.load(Ordering::SeqCst), 2);
        assert_eq!(tail_calls.load(Ordering::SeqCst), 129);
        let target = target
            .lock()
            .expect("deep tail target lock")
            .clone()
            .expect("deep step-over target");
        assert!(target.starts_with("<script>:"), "target was {target}");
    }

    #[test]
    fn idle_pause_callback_does_not_prepare_debug_artifacts_when_bt_is_off() {
        let mut session = Session::new();
        session.set_pause_handler(|_, _| DebugResume::Continue);
        session.set_backtrace_enabled(false);
        session.set_wqdb(false);

        let result = session
            .eval_string("f:{x+1}; f 1")
            .expect("plain eval should run with idle pause callback");

        assert_eq!(result, Value::Int(2));
        assert!(
            session
                .vm
                .debug_info
                .get_chunk(crate::wqdb::data::ChunkId(0))
                .is_none(),
            "idle pause callback should not create debug chunks when bt and wqdb are off"
        );
    }

    #[test]
    fn pause_instruction_prepares_debug_artifacts_even_when_bt_is_off() {
        let pauses = Arc::new(AtomicUsize::new(0));
        let captured_pauses = Arc::clone(&pauses);
        let mut session = Session::new();
        session.set_pause_handler(move |_, _| {
            captured_pauses.fetch_add(1, Ordering::SeqCst);
            DebugResume::Continue
        });
        session.set_backtrace_enabled(false);
        session.set_wqdb(false);

        let result = session
            .eval_string("@p 1")
            .expect("explicit pause should run without bt");

        assert_eq!(result, Value::Int(1));
        assert_eq!(pauses.load(Ordering::SeqCst), 1);
        assert!(
            session
                .vm
                .debug_info
                .get_chunk(crate::wqdb::data::ChunkId(0))
                .is_some(),
            "@p should still request debug artifacts for the pause callback"
        );
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
    fn parses_newline_after_binary_operator_as_continuation() {
        let mut session = Session::new();

        assert_eq!(session.eval_string("1+\n  2").unwrap(), Value::Int(3));
        assert_eq!(session.eval_string("32*\n  2").unwrap(), Value::Int(64));
        assert_eq!(
            session.eval_string("1<\n  2<\n  3").unwrap(),
            Value::Bool(true)
        );
    }

    #[test]
    fn evals_long_nary_lazy_bool_form() {
        let terms = std::iter::once("true")
            .chain(std::iter::repeat_n("missing", 512))
            .collect::<Vec<_>>()
            .join(";");
        let code = format!("c:O[{terms}]");
        let mut session = Session::new();

        let result = session
            .eval_string(&code)
            .expect("long lazy bool chain should eval");

        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn fused_try_region_does_not_consume_following_expression() {
        let mut session = Session::new();

        let result = session
            .eval_string("f:{[x](@t $[x=1;42;0];99)};f[1]")
            .expect("fused try expression should remain bounded");

        assert_eq!(
            result,
            Value::from_items(vec![
                Value::List(Arc::new(vec![Value::Tag(Arc::from("ok")), Value::Int(42),])),
                Value::Int(99),
            ])
        );
    }

    #[test]
    fn try_returns_tagged_success_with_the_protected_value() {
        let mut session = Session::new();

        let result = session.eval_string("@t 1+1").expect("try should succeed");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::Tag(Arc::from("ok")), Value::Int(2),]))
        );
    }

    #[test]
    fn try_returns_stable_structured_error() {
        let mut session = Session::new();

        let result = session
            .eval_string("@t 1/0")
            .expect("try should catch divide error");
        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        assert_eq!(result.first(), Some(&Value::Tag(Arc::from("error"))));
        let Some(Value::Dict(error)) = result.get(1) else {
            panic!("expected structured error payload");
        };
        assert_eq!(error.get("version"), Some(&Value::Int(1)));
        assert_eq!(error.get("kind"), Some(&Value::Tag(Arc::from("zero-div"))));
        assert!(matches!(error.get("message"), Some(Value::String(_))));
        assert!(error.contains_key("source"));
        assert!(error.contains_key("span"));
        assert!(matches!(error.get("notes"), Some(Value::List(_))));
        assert!(matches!(error.get("data"), Some(Value::Dict(_))));
        assert!(matches!(error.get("stack"), Some(Value::List(_))));
        assert!(error.contains_key("cause"));
    }

    #[test]
    fn assert_builtins_return_the_checked_value_on_success() {
        let mut session = Session::new();

        assert_eq!(
            session.eval_string("assert T").expect("true assertion"),
            Value::Bool(true)
        );
        assert_eq!(
            session
                .eval_string("assert_eq[(1;2);(1;2)]")
                .expect("equal assertion"),
            Value::IntList(Arc::new(vec![1, 2]))
        );
    }

    #[test]
    fn assert_failure_has_structured_truth_data() {
        let mut session = Session::new();

        let result = session
            .eval_string("@t assert[F;\"expected readiness\";`context:`startup]")
            .expect("try should catch assertion failure");
        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        assert_eq!(result.first(), Some(&Value::Tag(Arc::from("error"))));
        let Some(Value::Dict(error)) = result.get(1) else {
            panic!("expected structured error payload");
        };
        assert_eq!(error.get("kind"), Some(&Value::Tag(Arc::from("assert"))));
        assert_eq!(
            error.get("message"),
            Some(&Value::String(Arc::new("expected readiness".to_string())))
        );
        let Some(Value::Dict(data)) = error.get("data") else {
            panic!("expected assertion data");
        };
        assert_eq!(data.get("check"), Some(&Value::Tag(Arc::from("truth"))));
        assert_eq!(data.get("condition"), Some(&Value::Bool(false)));
        assert_eq!(data.get("context"), Some(&Value::Tag(Arc::from("startup"))));
    }

    #[test]
    fn assert_eq_failure_has_structured_comparison_data() {
        let mut session = Session::new();

        let result = session
            .eval_string("@t assert_eq[3;4;\"numbers differ\";`context:42]")
            .expect("try should catch assertion failure");
        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        let Some(Value::Dict(error)) = result.get(1) else {
            panic!("expected structured error payload");
        };
        assert_eq!(error.get("kind"), Some(&Value::Tag(Arc::from("assert"))));
        let Some(Value::Dict(data)) = error.get("data") else {
            panic!("expected assertion data");
        };
        assert_eq!(data.get("check"), Some(&Value::Tag(Arc::from("equal"))));
        assert_eq!(data.get("actual"), Some(&Value::Int(3)));
        assert_eq!(data.get("expected"), Some(&Value::Int(4)));
        assert_eq!(data.get("context"), Some(&Value::Int(42)));
    }

    #[test]
    fn assert_requires_a_bool_condition() {
        let mut session = Session::new();

        let error = session
            .eval_string("assert 1")
            .expect_err("non-bool assertion should fail");

        assert_eq!(error.err_type, crate::wqerror::WqErrorType::Domain);
    }

    #[test]
    fn protected_recursion_uses_the_normal_vm_depth_limit() {
        let mut session = Session::new();

        let result = session
            .eval_string("f:{[n]$[n=0;0;1+f[n-1]]};@t f[16]")
            .expect("recursive try should remain inside the wq error model");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![Value::Tag(Arc::from("ok")), Value::Int(16),]))
        );
    }

    #[test]
    fn protected_recursion_is_caught_at_the_configured_vm_limit() {
        let mut session = Session::new();
        session.vm.max_call_depth = 12;

        let result = session
            .eval_string("f:{[n]$[n=0;0;1+f[n-1]]};@t f[100]")
            .expect("recursion limit should be caught by try");
        let Value::List(result) = result else {
            panic!("expected tagged try result");
        };
        assert_eq!(result.first(), Some(&Value::Tag(Arc::from("error"))));
        let Some(Value::Dict(error)) = result.get(1) else {
            panic!("expected error payload");
        };
        assert_eq!(error.get("kind"), Some(&Value::Tag(Arc::from("recursion"))));
    }

    #[test]
    fn named_call_evaluates_target_before_arguments() {
        let mut session = Session::new();

        let result = session
            .eval_string("f:{[x]1};a:f[(f:{[x]2};0)];f:{[x]1};b:($[T;f;f])[(f:{[x]2};0)];(a;b)")
            .expect("call target evaluation order should be stable");

        assert_eq!(
            result,
            Value::from_items(vec![Value::Int(1), Value::Int(1)])
        );
    }

    #[test]
    fn method_call_evaluates_target_before_arguments() {
        let mut session = Session::new();

        let result = session
            .eval_string(
                "d:(`f:{[x]1});a:d[`f][(d[`f]:{[x]2};0)];d:(`f:{[x]1});b:($[T;d;d])[`f][(d[`f]:{[x]2};0)];(a;b)",
            )
            .expect("method target evaluation order should be stable");

        assert_eq!(
            result,
            Value::from_items(vec![Value::Int(1), Value::Int(1)])
        );
    }

    #[test]
    fn binary_operators_read_left_operand_before_rhs_effects() {
        let mut session = Session::new();

        let local = session
            .eval_string("f:{[]n:10;bump:{[]'n+:1;5};r:n+bump[]+1;(r;n)};f[]")
            .expect("local binary evaluation should succeed");
        assert_eq!(
            local,
            Value::from_items(vec![Value::Int(16), Value::Int(11)])
        );

        let outer = session
            .eval_string("f:{[]n:10;bump:{[]'n+:1;5};calc:{[]r:'n+bump[];(r;'n)};calc[]};f[]")
            .expect("captured binary evaluation should succeed");
        assert_eq!(
            outer,
            Value::from_items(vec![Value::Int(15), Value::Int(11)])
        );

        let global = session
            .eval_string("n:10;bump:{[]'n+:1;5};r:n+bump[];(r;n)")
            .expect("global binary evaluation should succeed");
        assert_eq!(
            global,
            Value::from_items(vec![Value::Int(15), Value::Int(11)])
        );

        let multiply = session
            .eval_string("f:{[]n:10;bump:{[]'n+:1;5};r:n*bump[];(r;n)};f[]")
            .expect("multiply evaluation should succeed");
        assert_eq!(
            multiply,
            Value::from_items(vec![Value::Int(50), Value::Int(11)])
        );
    }

    #[test]
    fn augmented_assignments_read_old_value_before_rhs_effects() {
        let mut session = Session::new();

        let add = session
            .eval_string("f:{[]n:10;bump:{[]'n+:1;5};n+:bump[];n};f[]")
            .expect("add assignment should succeed");
        assert_eq!(add, Value::Int(15));

        let outer_add = session
            .eval_string("f:{[]n:10;bump:{[]'n+:1;5};calc:{[]'n+:bump[];'n};calc[]};f[]")
            .expect("captured add assignment should succeed");
        assert_eq!(outer_add, Value::Int(15));

        let global_add = session
            .eval_string("n:10;bump:{[]'n+:1;5};n+:bump[];n")
            .expect("global add assignment should succeed");
        assert_eq!(global_add, Value::Int(15));

        let multiply = session
            .eval_string("f:{[]n:10;bump:{[]'n+:1;5};n*:bump[];n};f[]")
            .expect("multiply assignment should succeed");
        assert_eq!(multiply, Value::Int(50));

        let cat = session
            .eval_string("f:{[]s:(3;4);both:{[]'s[!-2;-1]|fold(+)};s,:both[];s};f[]")
            .expect("cat assignment should succeed");
        assert_eq!(
            cat,
            Value::from_items(vec![Value::Int(3), Value::Int(4), Value::Int(7)])
        );
    }

    #[test]
    fn unicode_scalar_literals_are_distinct_from_strings() {
        let mut session = Session::new();

        assert_eq!(
            session
                .eval_string("\"a\"")
                .expect("string should evaluate"),
            crate::value::into_wq_string("a")
        );
        assert_eq!(
            session
                .eval_string("@u\"a\"")
                .expect("unicode scalar should evaluate"),
            Value::Char('a')
        );
        assert_eq!(
            session
                .eval_string("\"a\"=@u\"a\"")
                .expect("comparison should evaluate"),
            Value::Bool(false)
        );
    }

    #[test]
    fn apply_always_preserves_a_function_result_frame() {
        let mut session = Session::new();

        assert_eq!(
            session
                .eval_string("apply[abs;-3]")
                .expect("single function apply should succeed"),
            Value::from_items(vec![Value::Int(3)])
        );
        assert_eq!(
            session
                .eval_string("apply[{,x};1]")
                .expect("container result apply should succeed"),
            Value::from_items(vec![Value::from_items(vec![Value::Int(1)])])
        );
        assert_eq!(
            session
                .eval_string("apply[{()};1]")
                .expect("empty container result apply should succeed"),
            Value::List(std::sync::Arc::new(vec![Value::unit()]))
        );
    }

    #[test]
    fn find_builtins_always_preserve_a_path_result_frame() {
        let mut session = Session::new();
        let one_path = Value::List(std::sync::Arc::new(vec![Value::from_items(vec![
            Value::Int(1),
        ])]));

        for source in [
            "find[(1;2;3);2]",
            "rfind[(1;2;3);2]",
            "findw[(1;2;3);{x=2}]",
            "rfindw[(1;2;3);{x=2}]",
        ] {
            assert_eq!(
                session.eval_string(source).expect("find should succeed"),
                one_path,
                "{source} should preserve its result frame"
            );
        }
    }
}
