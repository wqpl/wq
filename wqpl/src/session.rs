pub mod dbglog;
pub mod stdio;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::ast::AstNode;
use crate::builtins::BuiltinPreset;
use crate::compile::Compiler;
use crate::interpret::InterpreterKind;
use crate::interpret::profiler::ProfilerInterpreter;
use crate::interpret::sample::SampleInterpreter;
use crate::interpret::vanilla::{InterpretPoll, VanillaInterpreter};
use crate::lex::Lexer;
use crate::module::ModuleResolver;
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
use crate::wqdb::{
    DebugPause, DebugPauseId, DebugResume, Debugger, PauseEvent, ResumeAction, state,
};
use crate::wqerror::{WqError, WqErrorType};

/// Snapshot of user-defined global bindings owned by a [`Session`].
pub type Bindings = ahash::AHashMap<String, Value>;

/// Thread-safe handle for requesting a controlled stop of a running session.
#[derive(Clone, Debug)]
pub struct SessionInterruptHandle {
    requested: Arc<AtomicBool>,
}

impl SessionInterruptHandle {
    /// Request that the session stop at the next cooperative safe point.
    ///
    /// Safe points occur before instructions and inside interruptible host
    /// algorithms. The running instruction remains responsible for leaving
    /// observable state consistent before it stops.
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
    import_origin: &'source str,
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
            import_origin: path,
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
            import_origin: path,
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

    /// Set the lexical origin used by `@i` imports in this source.
    pub fn with_import_origin(mut self, origin: &'source str) -> Self {
        self.import_origin = origin;
        self
    }

    pub fn import_origin(self) -> &'source str {
        self.import_origin
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
            rendered.push_str(&state::format_crash_frame(frame, index == 0, color_mode));
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

#[derive(Debug, Clone, PartialEq)]
pub enum EvaluationPoll {
    Yielded { work_units: usize },
    AwaitingInput { request_id: u64, prompt: String },
    Paused(DebugPause),
    Ready(Value),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptInputResponse {
    Line(String),
    Eof,
    Interrupted,
    Error(String),
}

/// Owned state for one cooperatively evaluated script.
///
/// The source is retained so diagnostics and debug mappings remain valid
/// across calls to [`Session::poll_script_evaluation`].
pub struct ScriptEvaluation {
    token: Arc<()>,
    path: String,
    source: String,
    items: Vec<ScriptItem>,
    next_item: usize,
    active_fragment: bool,
    phase: EvaluationPhase,
    last_value: Option<Value>,
    finished: bool,
}

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
    sample: SampleInterpreter,
    profiler: ProfilerInterpreter,
    active_script_evaluation: Option<Arc<()>>,
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
            sample: SampleInterpreter::default(),
            profiler: ProfilerInterpreter::default(),
            active_script_evaluation: None,
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
        self.vm.halt_status()
    }

    /// Take the status requested by a controlled `cliargs` halt.
    pub fn take_halt_status(&mut self) -> Option<i32> {
        self.vm.take_halt_status()
    }

    /// Take a pending or observed interruption request.
    ///
    /// Interruption is distinct from a controlled `cliargs` halt and does not
    /// carry a process exit status.
    pub fn take_interrupt(&mut self) -> bool {
        self.vm.take_interrupt()
    }

    /// Return a handle which can interrupt this session from another thread.
    pub fn interrupt_handle(&self) -> SessionInterruptHandle {
        SessionInterruptHandle {
            requested: Arc::clone(&self.vm.interrupt_requested),
        }
    }

    pub fn is_wqdb_enabled(&self) -> bool {
        self.vm.debug_state.is_enabled()
    }

    /// Clear compiled instructions, transient stacks, and diagnostics while
    /// preserving host configuration and bindings.
    ///
    /// Debug metadata referenced by bound compiled functions is retained so
    /// those functions remain callable after the reset. Breakpoints, stepping,
    /// trackers, and other transient debugger state are cleared.
    pub fn reset_execution_state(&mut self) {
        self.active_script_evaluation = None;
        self.vm
            .reset_with_prepared_instructions(PreparedInstructions::new(Vec::new()));
        self.vm.halt_reason = None;
        self.vm.interrupt_requested.store(false, Ordering::Release);
        self.vm.debug_state.reset_execution_state();
        self.vm.current_chunk = None;
        self.vm.runtime_debug_info = false;
        self.vm.debug_src_offset = 0;
        self.vm.last_crash = None;
        self.wqdb_arm_next = self.vm.debug_state.is_enabled();
        self.sample = SampleInterpreter::default();
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
        self.vm.module_cache.clear();
        self.vm.module_loading.clear();
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
        self.vm.debug_state.set_enabled(flag);
        if self.vm.debug_state.is_enabled() {
            self.wqdb_arm_next = true;
        } else {
            self.wqdb_arm_next = false;
            self.vm.debug_state.clear_mode();
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

    pub fn builtins(&self) -> &crate::builtins::Builtins {
        &self.vm.builtins
    }

    pub fn builtins_preset(&self) -> BuiltinPreset {
        self.vm.builtins_preset
    }

    pub fn set_builtins_preset(&mut self, preset: BuiltinPreset) {
        self.vm.builtins.apply_preset(preset);
        self.vm.builtins_preset = preset;
        self.vm.module_cache.clear();
        self.vm.module_loading.clear();
    }

    /// Install the host resolver used by `@i` expressions.
    ///
    /// Replacing a resolver clears the module cache because stable identities
    /// belong to the resolver that produced them.
    pub fn set_module_resolver(&mut self, resolver: impl ModuleResolver + 'static) {
        self.vm.module_resolver = Some(Arc::new(resolver));
        self.vm.module_cache.clear();
        self.vm.module_loading.clear();
    }

    /// Install an already shared host resolver used by `@i` expressions.
    pub fn set_shared_module_resolver(&mut self, resolver: Arc<dyn ModuleResolver>) {
        self.vm.module_resolver = Some(resolver);
        self.vm.module_cache.clear();
        self.vm.module_loading.clear();
    }

    /// Remove the module resolver and all cached module values.
    pub fn clear_module_resolver(&mut self) {
        self.vm.module_resolver = None;
        self.vm.module_cache.clear();
        self.vm.module_loading.clear();
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
            Ok(value) => Ok(value.unwrap_or_else(Value::empty_list)),
            Err(ScriptRunError::Evaluation(error)) => Err(error),
            Err(ScriptRunError::Directive(never)) => match never {},
        }
    }

    /// Create owned state for cooperatively evaluating a complete script.
    ///
    /// Loader directives are rejected with the same diagnostic as
    /// [`Session::eval_script`]. Parsing and execution begin on the first poll.
    /// The evaluation must complete or be cancelled before another cooperative
    /// evaluation starts on this session.
    pub fn start_script_evaluation(
        &mut self,
        path: impl Into<String>,
        source: impl Into<String>,
    ) -> EvaluationResult<ScriptEvaluation> {
        if self.active_script_evaluation.is_some() {
            return Err(EvaluationFailure::new(
                WqError::new(WqErrorType::Vm)
                    .msg("session already has an active script evaluation"),
                EvaluationPhase::Execute,
            ));
        }
        let path = path.into();
        let source = source.into();
        let items = parse_script_items(&source);
        if let Some(directive) = items.iter().find_map(|item| match item {
            ScriptItem::Directive(directive) => Some(directive),
            ScriptItem::Shebang { .. } | ScriptItem::Code { .. } => None,
        }) {
            self.vm.begin_evaluation();
            let source_unit = SourceUnit::named(&path, &source);
            let failure = EvaluationFailure::new(
                script_directive_requires_host(source_unit, directive),
                EvaluationPhase::Parse,
            );
            self.vm.publish_crash(None);
            self.vm.end_evaluation();
            return Err(failure);
        }

        let token = Arc::new(());
        self.active_script_evaluation = Some(Arc::clone(&token));
        Ok(ScriptEvaluation {
            token,
            path,
            source,
            items,
            next_item: 0,
            active_fragment: false,
            phase: EvaluationPhase::Lex,
            last_value: None,
            finished: false,
        })
    }

    /// Execute at most one work budget from a cooperative script.
    ///
    /// Higher-order context builtins and every interpreter kind consume the
    /// same work budget. Other context builtin algorithms still execute as one
    /// work unit until they provide their own resumable state.
    pub fn poll_script_evaluation(
        &mut self,
        evaluation: &mut ScriptEvaluation,
        work_budget: usize,
    ) -> EvaluationResult<EvaluationPoll> {
        if evaluation.finished {
            return Err(EvaluationFailure::new(
                WqError::new(WqErrorType::Vm).msg("evaluation has already completed"),
                EvaluationPhase::Execute,
            ));
        }
        if !self
            .active_script_evaluation
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &evaluation.token))
        {
            return Err(EvaluationFailure::new(
                WqError::new(WqErrorType::Vm)
                    .msg("script evaluation is not active for this session"),
                EvaluationPhase::Execute,
            ));
        }
        if work_budget == 0 {
            return Err(EvaluationFailure::new(
                WqError::new(WqErrorType::Vm)
                    .msg("script evaluation work budget must be greater than 0"),
                EvaluationPhase::Execute,
            ));
        }

        loop {
            if evaluation.active_fragment {
                match self.execute_prepared_slice(work_budget) {
                    Ok(InterpretPoll::Yielded { work_units }) => {
                        return Ok(EvaluationPoll::Yielded { work_units });
                    }
                    Ok(InterpretPoll::AwaitingInput { request_id, prompt }) => {
                        return Ok(EvaluationPoll::AwaitingInput { request_id, prompt });
                    }
                    Ok(InterpretPoll::Paused(pause)) => {
                        return Ok(EvaluationPoll::Paused(pause));
                    }
                    Ok(InterpretPoll::Ready(value)) => {
                        let result = if let Some(error) = self.vm.debug_log.take_error() {
                            Err(EvaluationFailure::new(
                                host_io_error("debug output", error),
                                EvaluationPhase::Host,
                            ))
                        } else {
                            Ok(value)
                        };
                        let crash = result
                            .as_ref()
                            .err()
                            .and_then(|failure| failure.crash().cloned());
                        self.vm.publish_crash(crash);
                        self.vm.end_evaluation();
                        evaluation.active_fragment = false;
                        match result {
                            Ok(value) => evaluation.last_value = Some(value),
                            Err(failure) => {
                                evaluation.finished = true;
                                self.active_script_evaluation = None;
                                return Err(failure);
                            }
                        }
                        if self.vm.is_halted() {
                            evaluation.finished = true;
                            self.active_script_evaluation = None;
                            return Ok(EvaluationPoll::Ready(
                                evaluation
                                    .last_value
                                    .take()
                                    .unwrap_or_else(Value::empty_list),
                            ));
                        }
                        if evaluation.next_item < evaluation.items.len() {
                            return Ok(EvaluationPoll::Yielded { work_units: 0 });
                        }
                    }
                    Err(error) => {
                        let span = evaluation
                            .items
                            .get(evaluation.next_item.saturating_sub(1))
                            .map(ScriptItem::span)
                            .expect("active evaluation should have a source item");
                        let source =
                            SourceUnit::fragment(&evaluation.path, &evaluation.source, span)
                                .expect("script parser should yield valid source spans");
                        let failure = EvaluationFailure::new(
                            contextualize_source_error(error, source),
                            evaluation.phase,
                        );
                        let crash = failure.crash().cloned();
                        self.vm.publish_crash(crash);
                        self.vm.end_evaluation();
                        evaluation.active_fragment = false;
                        evaluation.finished = true;
                        self.active_script_evaluation = None;
                        return Err(failure);
                    }
                }
            }

            let Some(item) = evaluation.items.get(evaluation.next_item).cloned() else {
                evaluation.finished = true;
                self.active_script_evaluation = None;
                return Ok(EvaluationPoll::Ready(
                    evaluation
                        .last_value
                        .take()
                        .unwrap_or_else(Value::empty_list),
                ));
            };
            evaluation.next_item += 1;
            match item {
                ScriptItem::Shebang { .. } => {}
                ScriptItem::Directive(_) => {
                    unreachable!("script directives were rejected before evaluation")
                }
                ScriptItem::Code { span } => {
                    let source = SourceUnit::fragment(&evaluation.path, &evaluation.source, span)
                        .expect("script parser should yield valid source spans");
                    if source.code.trim().is_empty() {
                        continue;
                    }
                    self.vm.begin_evaluation();
                    self.vm.debug_log.clear_error();
                    evaluation.phase = EvaluationPhase::Lex;
                    match self.prepare_source_inner(source, &mut evaluation.phase) {
                        Ok(Some(value)) => {
                            let result = if let Some(error) = self.vm.debug_log.take_error() {
                                Err(EvaluationFailure::new(
                                    host_io_error("debug output", error),
                                    EvaluationPhase::Host,
                                ))
                            } else {
                                Ok(value)
                            };
                            let crash = result
                                .as_ref()
                                .err()
                                .and_then(|failure| failure.crash().cloned());
                            self.vm.publish_crash(crash);
                            self.vm.end_evaluation();
                            match result {
                                Ok(value) => evaluation.last_value = Some(value),
                                Err(failure) => {
                                    evaluation.finished = true;
                                    self.active_script_evaluation = None;
                                    return Err(failure);
                                }
                            }
                        }
                        Ok(None) => evaluation.active_fragment = true,
                        Err(error) => {
                            let failure = EvaluationFailure::new(
                                contextualize_source_error(error, source),
                                evaluation.phase,
                            );
                            let crash = failure.crash().cloned();
                            self.vm.publish_crash(crash);
                            self.vm.end_evaluation();
                            evaluation.finished = true;
                            self.active_script_evaluation = None;
                            return Err(failure);
                        }
                    }
                }
            }
        }
    }

    /// Cancel a cooperative script while preserving bindings and host setup.
    pub fn cancel_script_evaluation(&mut self, evaluation: &mut ScriptEvaluation) -> bool {
        if !self
            .active_script_evaluation
            .as_ref()
            .is_some_and(|token| Arc::ptr_eq(token, &evaluation.token))
        {
            return false;
        }
        if evaluation.active_fragment {
            self.vm.end_evaluation();
        }
        evaluation.active_fragment = false;
        evaluation.finished = true;
        self.reset_execution_state();
        true
    }

    pub fn resume_script_input(
        &mut self,
        evaluation: &mut ScriptEvaluation,
        request_id: u64,
        response: ScriptInputResponse,
    ) -> EvaluationResult<()> {
        if evaluation.finished
            || !self
                .active_script_evaluation
                .as_ref()
                .is_some_and(|token| Arc::ptr_eq(token, &evaluation.token))
        {
            return Err(EvaluationFailure::new(
                WqError::new(WqErrorType::Vm)
                    .msg("script evaluation is not active for this session"),
                EvaluationPhase::Execute,
            ));
        }
        let response = match response {
            ScriptInputResponse::Line(line) => Ok(line),
            ScriptInputResponse::Eof => Err(WqIoError::Eof),
            ScriptInputResponse::Interrupted => Err(WqIoError::Interrupted),
            ScriptInputResponse::Error(error) => Err(WqIoError::Other(error)),
        };
        self.vm
            .resume_input(request_id, response)
            .map_err(|error| EvaluationFailure::new(error, EvaluationPhase::Execute))
    }

    pub fn resume_script_debugger(
        &mut self,
        evaluation: &mut ScriptEvaluation,
        pause_id: DebugPauseId,
        action: ResumeAction,
    ) -> EvaluationResult<()> {
        if evaluation.finished
            || !self
                .active_script_evaluation
                .as_ref()
                .is_some_and(|token| Arc::ptr_eq(token, &evaluation.token))
        {
            return Err(EvaluationFailure::new(
                WqError::new(WqErrorType::Vm)
                    .msg("script evaluation is not active for this session"),
                EvaluationPhase::Execute,
            ));
        }
        self.vm
            .resume_debug_pause(pause_id, action)
            .map_err(|error| EvaluationFailure::new(error, EvaluationPhase::Execute))
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
            if self.vm.is_halted() {
                break;
            }
            match item {
                ScriptItem::Shebang { .. } => {}
                ScriptItem::Code { span } => {
                    let absolute_span = rebase_script_span(span, source.base_offset);
                    let fragment =
                        SourceUnit::fragment(source.path, source.full_text, absolute_span)
                            .expect("script parser should yield valid source spans")
                            .with_import_origin(source.import_origin);
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
        if let Some(value) = self.prepare_source_inner(source, phase)? {
            return Ok(value);
        }
        self.execute_prepared()
    }

    fn prepare_source_inner(
        &mut self,
        source: SourceUnit<'_>,
        phase: &mut EvaluationPhase,
    ) -> WqResult<Option<Value>> {
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
        compiler.set_import_origin(source.import_origin);
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
            return Ok(Some(Value::empty_list()));
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
            self.vm
                .debug_state
                .resolve_source_breakpoints(&self.vm.debug_info);
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
            self.vm.dbg_arm_entry();
        }
        Ok(None)
    }

    fn execute_prepared(&mut self) -> WqResult<Value> {
        self.vm.cooperative_execution = false;
        let result = match self.vm.interpreter_kind {
            InterpreterKind::Sample => self.vm.run_with_interpreter(&mut self.sample),
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

    fn execute_prepared_slice(&mut self, work_budget: usize) -> WqResult<InterpretPoll> {
        self.vm.cooperative_execution = true;
        let limit = self.vm.root_instruction_limit();
        let result = match self.vm.interpreter_kind {
            InterpreterKind::Vanilla => {
                VanillaInterpreter.interpret_slice(&mut self.vm, limit, work_budget)
            }
            InterpreterKind::Sample => {
                self.sample
                    .interpret_slice(&mut self.vm, limit, work_budget)
            }
            InterpreterKind::Profiler => {
                let result = self
                    .profiler
                    .interpret_slice(&mut self.vm, limit, work_budget);
                if matches!(result, Ok(InterpretPoll::Ready(_)) | Err(_)) {
                    self.profiler
                        .finish_report(self.vm.stderr_color_mode(), &self.vm.debug_log);
                }
                result
            }
        }?;
        if self.vm.debug_log.enabled(DebugLogFlags::VALUE)
            && let InterpretPoll::Ready(value) = &result
        {
            self.write_diagnostic(format!("{value:?}"))?;
        }
        Ok(result)
    }

    /// Build a snapshot of the environment from slots.
    fn environment(&self) -> GlobalMap {
        self.vm.global_env()
    }

    /// Enter wqdb at the start of the next evaluation when it is enabled.
    pub fn arm_wqdb_next(&mut self) {
        if self.vm.debug_state.is_enabled() {
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
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::module::{ModuleError, ModuleRequest, ModuleResolver, ResolvedModule};
    use crate::session::stdio::{WqInput, WqOutput};
    use crate::wqdb::{
        DebugNotification, PauseReason, ResumeAction, SymbolMutationKind, TrackResult,
    };

    struct CaptureOutput(Arc<Mutex<String>>);

    struct FixedInput(&'static str);

    #[derive(Clone)]
    struct TestModuleResolver {
        modules: Arc<HashMap<&'static str, &'static str>>,
        calls: Arc<AtomicUsize>,
    }

    impl TestModuleResolver {
        fn new(modules: impl IntoIterator<Item = (&'static str, &'static str)>) -> Self {
            Self {
                modules: Arc::new(modules.into_iter().collect()),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    impl ModuleResolver for TestModuleResolver {
        fn resolve(&self, request: &ModuleRequest) -> Result<ResolvedModule, ModuleError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let source = self.modules.get(request.specifier()).ok_or_else(|| {
                ModuleError::new(format!("module '{}' does not exist", request.specifier()))
            })?;
            Ok(ResolvedModule::new(
                request.specifier(),
                request.specifier(),
                *source,
            ))
        }
    }

    impl WqInput for FixedInput {
        fn read_line(&mut self, _prompt: &str) -> Result<String, WqIoError> {
            Ok(self.0.to_string())
        }
    }

    impl WqOutput for CaptureOutput {
        fn write(&mut self, text: &str) -> Result<(), WqIoError> {
            self.0.lock().expect("capture output lock").push_str(text);
            Ok(())
        }
    }

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
    fn import_exports_a_value_without_leaking_module_bindings() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([(
            "module",
            "secret:40;(`answer:secret+2)",
        )]));

        let value = session
            .eval_string("module:@i\"module\";module")
            .expect("module should import");

        assert_eq!(value.to_string(), "(`answer:42)");
        assert!(!session.bindings().contains_key("secret"));
    }

    #[test]
    fn imported_function_captures_private_module_bindings() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("adder", "base:40;{base+x}")]));

        let value = session
            .eval_string("add:@i\"adder\";add[2]")
            .expect("exported closure should be callable");

        assert_eq!(value, Value::Int(42));
    }

    #[test]
    fn imported_function_can_use_builtins() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("length", "{len x}")]));

        let value = session
            .eval_string("length:@i\"length\";length[\"abc\"]")
            .expect("module builtins should remain available");

        assert_eq!(value, Value::Int(3));
    }

    #[test]
    fn imported_n_loop_counter_stays_isolated() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([(
            "counter",
            "counter:{N[3;_n];_n};counter",
        )]));

        let value = session
            .eval_string("_n:99;counter:@i\"counter\";counter[]")
            .expect("module n-loop counter should stay private");

        assert_eq!(value, Value::Int(2));
    }

    #[test]
    fn imported_module_cannot_capture_a_caller_binding() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("module", "{secret}")]));

        let error = session
            .eval_string("secret:42;@i\"module\"")
            .expect_err("module should not see caller bindings");

        assert!(error.to_string().contains("'secret' has not been bound"));
    }

    #[test]
    fn imported_module_rejects_top_level_return() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("module", "@r 1")]));

        let error = session
            .eval_string("@i\"module\"")
            .expect_err("top-level return should be rejected");

        assert!(error.to_string().contains("@r outside function"));
    }

    #[test]
    fn import_accepts_a_raw_string_literal() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("module", "42")]));

        let value = session
            .eval_string("@i @l\"module\"")
            .expect("raw string specifier should import");

        assert_eq!(value, Value::Int(42));
    }

    #[test]
    fn import_works_inside_a_function_body() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("module", "42")]));

        let value = session
            .eval_string("load:{@i\"module\"};load[]")
            .expect("function body should import lazily");

        assert_eq!(value, Value::Int(42));
    }

    #[test]
    fn repeated_import_returns_the_same_stateful_export() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([("counter", "n:0;'{n+:1}")]));

        let value = session
            .eval_string("a:@i\"counter\";b:@i\"counter\";(a[];b[])")
            .expect("cached module should preserve export state");

        assert_eq!(value, Value::IntList(Arc::new(vec![1, 2])));
    }

    #[test]
    fn import_is_lazy_and_catchable() {
        let resolver = TestModuleResolver::new([]);
        let calls = Arc::clone(&resolver.calls);
        let mut session = Session::new();
        session.set_module_resolver(resolver);

        assert_eq!(
            session
                .eval_string("$[F;@i\"missing\";7]")
                .expect("false branch should not resolve"),
            Value::Int(7)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let caught = session
            .eval_string("@t @i\"missing\"")
            .expect("module resolution error should be caught");
        assert!(caught.to_string().starts_with("(`error;"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn import_detects_cycles_and_does_not_cache_failures() {
        let resolver = TestModuleResolver::new([("a", "@i\"b\""), ("b", "@i\"a\"")]);
        let calls = Arc::clone(&resolver.calls);
        let mut session = Session::new();
        session.set_module_resolver(resolver);

        let first = session
            .eval_string("@i\"a\"")
            .expect_err("cycle should fail");
        assert!(first.to_string().contains("a -> b -> a"));
        let first_calls = calls.load(Ordering::SeqCst);

        session
            .eval_string("@i\"a\"")
            .expect_err("failed import should be retried");
        assert!(calls.load(Ordering::SeqCst) > first_calls);
    }

    #[test]
    fn import_initialization_failure_is_catchable_and_retried() {
        let resolver = TestModuleResolver::new([("broken", "1/0")]);
        let calls = Arc::clone(&resolver.calls);
        let mut session = Session::new();
        session.set_module_resolver(resolver);

        let caught = session
            .eval_string("@t @i\"broken\"")
            .expect("initialization error should be caught");
        assert!(caught.to_string().starts_with("(`error;"));
        let first_calls = calls.load(Ordering::SeqCst);

        session
            .eval_string("@i\"broken\"")
            .expect_err("failed initialization should be retried");
        assert!(calls.load(Ordering::SeqCst) > first_calls);
    }

    #[test]
    fn execution_reset_preserves_modules_and_workspace_reset_clears_them() {
        let resolver = TestModuleResolver::new([("counter", "n:0;'{n+:1}")]);
        let mut session = Session::new();
        session.set_module_resolver(resolver);

        assert_eq!(
            session
                .eval_string("counter:@i\"counter\";counter[]")
                .expect("counter should initialize"),
            Value::Int(1)
        );
        session.reset_execution_state();
        assert_eq!(
            session
                .eval_string("counter:@i\"counter\";counter[]")
                .expect("execution reset should preserve module"),
            Value::Int(2)
        );
        session.reset_workspace();
        assert_eq!(
            session
                .eval_string("counter:@i\"counter\";counter[]")
                .expect("workspace reset should reinitialize module"),
            Value::Int(1)
        );
    }

    #[test]
    fn import_requires_a_quoted_literal() {
        let mut session = Session::new();
        let error = session
            .eval_string("path:\"module\";@i path")
            .expect_err("computed specifier should be rejected");

        assert!(
            error
                .to_string()
                .contains("expected quoted literal after '@i'")
        );
    }

    #[test]
    fn import_reports_when_the_host_has_no_resolver() {
        let mut session = Session::new();
        let error = session
            .eval_string("@i\"module\"")
            .expect_err("missing resolver should fail at runtime");

        assert!(
            error
                .to_string()
                .contains("this host has no module resolver")
        );
    }

    #[test]
    fn dry_mode_does_not_resolve_imports() {
        let resolver = TestModuleResolver::new([("module", "42")]);
        let calls = Arc::clone(&resolver.calls);
        let mut session = Session::new();
        session.set_module_resolver(resolver);
        session.set_dry_mode(true);

        assert_eq!(
            session
                .eval_string("@i\"module\"")
                .expect("dry import should compile"),
            Value::empty_list()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn imported_module_rejects_legacy_directives() {
        let mut session = Session::new();
        session.set_module_resolver(TestModuleResolver::new([(
            "module",
            "\\load dependency.wq",
        )]));

        let error = session
            .eval_string("@i\"module\"")
            .expect_err("module directive should be rejected");

        assert!(
            error
                .to_string()
                .contains("legacy script directives are not allowed")
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
    fn unrolled_n_loop_returns_final_remainder_iteration() {
        let mut session = Session::new();

        assert_eq!(session.eval_string("N[17;_n]").unwrap(), Value::Int(16));
        assert_eq!(session.eval_string("N[63;_n]").unwrap(), Value::Int(62));
    }

    #[test]
    fn n_loop_control_in_conditional_condition_disables_unrolling() {
        let mut session = Session::new();
        let result = session
            .eval_string("N[2;$[(@b;T);1;2]]")
            .expect("break in a conditional condition should compile");

        assert_eq!(result, Value::empty_list());
    }

    #[test]
    fn dynamic_function_n_loop_uses_iteration_snapshot() {
        let mut session = Session::new();
        let result = session
            .eval_string("run:{[n]seen:();N[n;seen,:_n;_n:99];seen};run 5")
            .expect("dynamic N-loop should evaluate");

        assert_eq!(result, Value::IntList(Arc::new(vec![0, 1, 2, 3, 4])));
    }

    #[test]
    fn dynamic_function_n_loop_preserves_control_and_outer_index() {
        let mut session = Session::new();
        let result = session
            .eval_string(
                "_n:42;run:{[n]seen:();N[n;$.[_n=1;@c];seen,:_n;$.[_n=3;@b]];seen};\
                 (run 6;_n)",
            )
            .expect("dynamic N-loop control should evaluate");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::IntList(Arc::new(vec![0, 2, 3])),
                Value::Int(42),
            ]))
        );
    }

    #[test]
    fn const_propagation_preserves_branch_dependent_dict_order() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{[c]d:$[c;(`a:1;`b:2);(`b:2;`a:1)];d 0};(f T;f F)")
            .expect("branch-dependent dicts should eval");

        assert_eq!(result, Value::IntList(Arc::new(vec![1, 2])));
    }

    #[test]
    fn const_propagation_preserves_branch_dependent_signed_zero() {
        let mut session = Session::new();
        let result = session
            .eval_string("f:{[c]z:$[c;0.0;-0.0];z};(f T;f F)")
            .expect("branch-dependent signed zeros should eval");

        let Value::FloatList(values) = result else {
            panic!("expected a float list");
        };
        assert_eq!(values[0].to_bits(), 0.0f64.to_bits());
        assert_eq!(values[1].to_bits(), (-0.0f64).to_bits());
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

        assert_eq!(result.to_string(), "@s x^2 + 2*x + 1");
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
    fn interrupt_handle_reports_a_distinct_interruption() {
        let mut session = Session::new();
        session.interrupt_handle().interrupt();

        session
            .eval_string("W[T;0]")
            .expect("an interrupted evaluation should halt cleanly");

        assert!(session.take_interrupt());
        assert!(!session.take_interrupt());
        assert_eq!(session.take_halt_status(), None);
        assert_eq!(
            session
                .eval_string("1")
                .expect("a consumed interrupt should not affect the next evaluation"),
            Value::Int(1)
        );
    }

    #[test]
    fn pending_interrupt_can_be_consumed_without_poisoning_the_next_evaluation() {
        let mut session = Session::new();
        session.interrupt_handle().interrupt();

        assert!(session.take_interrupt());
        assert_eq!(
            session
                .eval_string("1")
                .expect("a consumed pending interrupt should not affect evaluation"),
            Value::Int(1)
        );
    }

    #[cfg(unix)]
    #[test]
    fn interrupt_stops_a_blocking_exec_builtin() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (interrupt_sender, interrupt_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut session = Session::new();
            let _ = interrupt_sender.send(session.interrupt_handle());
            let result = session.eval_string("exec[\"sleep\";\"1\"]");
            let interrupted = session.take_interrupt();
            let _ = result_sender.send((result.is_ok(), interrupted));
        });

        let interrupt = interrupt_receiver
            .recv_timeout(Duration::from_millis(500))
            .expect("worker should publish its interrupt handle");
        std::thread::sleep(Duration::from_millis(100));
        interrupt.interrupt();
        let (clean_halt, interrupted) = result_receiver
            .recv_timeout(Duration::from_millis(500))
            .expect("interrupt should stop the child process promptly");

        assert!(clean_halt, "interrupted execution should halt cleanly");
        assert!(interrupted);
    }

    #[test]
    fn cooperative_script_yields_and_matches_synchronous_evaluation() {
        let source = "f:{[n]$[n=0;0;1+f[n-1]]}\nf[40]";
        let expected = Session::new()
            .eval_script(SourceUnit::named("slice.wq", source))
            .expect("synchronous evaluation");
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("slice.wq", source)
            .expect("start cooperative evaluation");
        let mut yields = 0;

        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 3)
                .expect("poll cooperative evaluation")
            {
                EvaluationPoll::Yielded { work_units } => {
                    assert!(work_units <= 3);
                    yields += 1;
                }
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("test program should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
        assert!(yields > 10, "expected repeated cooperative yields");
    }

    #[test]
    fn cooperative_debugger_pause_is_stable_until_resumed() {
        let mut session = Session::new();
        session.set_wqdb(true);
        let mut evaluation = session
            .start_script_evaluation("debug-pause.wq", "answer:40+2")
            .expect("start cooperative evaluation");

        let pause = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 2)
                .expect("poll cooperative evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("debugger test should not request input")
                }
                EvaluationPoll::Paused(pause) => break pause,
                EvaluationPoll::Ready(_) => panic!("debugger should pause before execution"),
            }
        };
        assert_eq!(pause.event().reason, PauseReason::Entry);

        let repeated = session
            .poll_script_evaluation(&mut evaluation, 2)
            .expect("poll pending debugger pause");
        assert_eq!(repeated, EvaluationPoll::Paused(pause.clone()));
        assert!(!session.bindings().contains_key("answer"));

        session
            .resume_script_debugger(&mut evaluation, pause.id(), ResumeAction::Continue)
            .expect("resume debugger");
        let duplicate = session
            .resume_script_debugger(&mut evaluation, pause.id(), ResumeAction::Continue)
            .expect_err("debugger pause should resume exactly once");
        assert!(
            duplicate
                .to_string()
                .contains("debugger pause is not pending")
        );

        let value = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 2)
                .expect("finish cooperative evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("debugger test should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("continue should not pause again"),
                EvaluationPoll::Ready(value) => break value,
            }
        };
        assert_eq!(value, Value::Int(42));
        assert_eq!(session.bindings().get("answer"), Some(&Value::Int(42)));
    }

    #[test]
    fn cooperative_debugger_resume_advances_past_the_paused_boundary() {
        let mut session = Session::new();
        session.set_wqdb(true);
        let mut evaluation = session
            .start_script_evaluation("resume-boundary.wq", "first:1\nsecond:2\n@p first+second")
            .expect("start cooperative evaluation");
        let mut reasons = Vec::new();

        loop {
            match session
                .poll_script_evaluation(&mut evaluation, 10)
                .expect("poll cooperative evaluation")
            {
                EvaluationPoll::Paused(pause) => {
                    reasons.push(pause.event().reason);
                    if pause.event().reason == PauseReason::Entry {
                        let breakpoints = session
                            .debugger()
                            .set_source_breakpoints("resume-boundary.wq", &[2]);
                        let [breakpoint] = breakpoints.as_slice() else {
                            panic!("expected one source breakpoint");
                        };
                        assert!(breakpoint.location.is_none());
                    }
                    session
                        .resume_script_debugger(&mut evaluation, pause.id(), ResumeAction::Continue)
                        .expect("debugger pause should resume");
                    assert!(reasons.len() <= 3, "pause boundary repeated after resume");
                }
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::Ready(value) => {
                    assert_eq!(value, Value::Int(3));
                    break;
                }
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("debugger test should not request input")
                }
            }
        }

        assert_eq!(reasons.len(), 3);
        assert_eq!(reasons[0], PauseReason::Entry);
        assert!(matches!(reasons[1], PauseReason::Breakpoint { .. }));
        assert!(matches!(reasons[2], PauseReason::ExplicitPause { .. }));
    }

    #[test]
    fn symbol_tracking_produces_typed_mutation_notifications() {
        let mut session = Session::new();
        session.set_pause_handler(|_, debugger| {
            assert!(matches!(
                debugger.track_global_symbol("answer"),
                TrackResult::Added(_)
            ));
            ResumeAction::Continue
        });
        session.set_wqdb(true);
        session
            .eval_string("answer:40+2")
            .expect("tracked assignment should evaluate");

        let notifications = session.debugger().take_notifications();
        let [DebugNotification::SymbolChanged(mutation)] = notifications.as_slice() else {
            panic!("expected one structured symbol mutation");
        };
        assert_eq!(mutation.operation, SymbolMutationKind::Store);
        assert_eq!(mutation.old_value, None);
        assert_eq!(mutation.new_value, Value::Int(42));
    }

    #[test]
    fn cancelling_cooperative_script_preserves_completed_bindings() {
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("cancel.wq", "ready:41\nW[T;ready+:1]")
            .expect("start cooperative evaluation");

        assert!(matches!(
            session
                .poll_script_evaluation(&mut evaluation, 100)
                .expect("complete first source item"),
            EvaluationPoll::Yielded { .. }
        ));
        assert!(session.cancel_script_evaluation(&mut evaluation));

        assert_eq!(session.bindings().get("ready"), Some(&Value::Int(41)));
        assert_eq!(
            session
                .eval_string("ready+1")
                .expect("session should remain reusable after cancellation"),
            Value::Int(42)
        );
    }

    #[test]
    fn cooperative_script_state_is_bound_to_one_session() {
        let mut first = Session::new();
        let mut evaluation = first
            .start_script_evaluation("first.wq", "W[T;1]")
            .expect("start first evaluation");

        let overlap = match first.start_script_evaluation("second.wq", "2") {
            Ok(_) => panic!("same session should reject an overlapping evaluation"),
            Err(error) => error,
        };
        assert!(
            overlap
                .to_string()
                .contains("session already has an active script evaluation")
        );

        let mut second = Session::new();
        let foreign = second
            .poll_script_evaluation(&mut evaluation, 1)
            .expect_err("another session should reject foreign evaluation state");
        assert!(
            foreign
                .to_string()
                .contains("script evaluation is not active for this session")
        );
        assert!(!second.cancel_script_evaluation(&mut evaluation));
        assert!(first.cancel_script_evaluation(&mut evaluation));

        first
            .start_script_evaluation("replacement.wq", "3")
            .expect("cancellation should release the session");
    }

    #[test]
    fn cooperative_script_suspends_for_input_and_resumes_once() {
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("input.wq", "input[\"name> \"]")
            .expect("start input evaluation");

        let (request_id, prompt) = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 2)
                .expect("poll input evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { request_id, prompt } => {
                    break (request_id, prompt);
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(_) => panic!("input should suspend before completion"),
            }
        };
        assert_eq!(prompt, "name> ");

        session
            .resume_script_input(
                &mut evaluation,
                request_id,
                ScriptInputResponse::Line("Ada".to_string()),
            )
            .expect("resume input");
        let duplicate = session
            .resume_script_input(
                &mut evaluation,
                request_id,
                ScriptInputResponse::Line("Grace".to_string()),
            )
            .expect_err("input request should accept exactly one response");
        assert!(
            duplicate
                .to_string()
                .contains("input request is not pending")
        );

        let value = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 2)
                .expect("finish input evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("resumed input should not request another line")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };
        assert_eq!(value.try_to_rust_string().as_deref(), Some("Ada"));
    }

    #[test]
    fn alternate_interpreters_yield_cooperatively() {
        for kind in [InterpreterKind::Sample, InterpreterKind::Profiler] {
            let mut session = Session::new();
            session.set_interpreter(kind);
            let mut evaluation = session
                .start_script_evaluation("alternate.wq", "i:0;W[i<20;i+:1];i")
                .expect("start alternate interpreter evaluation");
            let mut yields = 0;

            let value = loop {
                match session
                    .poll_script_evaluation(&mut evaluation, 2)
                    .expect("poll alternate interpreter")
                {
                    EvaluationPoll::Yielded { work_units } => {
                        assert!(work_units <= 2);
                        yields += 1;
                    }
                    EvaluationPoll::AwaitingInput { .. } => {
                        panic!("test program should not request input")
                    }
                    EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                    EvaluationPoll::Ready(value) => break value,
                }
            };

            assert_eq!(value, Value::Int(20));
            assert!(yields > 2, "{} should yield repeatedly", kind.name());
        }
    }

    #[test]
    fn cooperative_map_charges_callback_work() {
        let source = "map[til 200;{[x]x+1}]";
        let expected = Session::new()
            .eval_script(SourceUnit::named("map-sync.wq", source))
            .expect("synchronous map evaluation");
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("map-sliced.wq", source)
            .expect("start cooperative map evaluation");
        let mut yields = 0;

        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll cooperative map evaluation")
            {
                EvaluationPoll::Yielded { work_units } => {
                    assert_eq!(work_units, 1);
                    yields += 1;
                }
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("map callback should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
        assert!(yields >= 200, "each mapped item should consume work");
    }

    #[test]
    fn cooperative_map_can_suspend_in_stdin_callback() {
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("map-input.wq", "map[(\"first> \";\"second> \");input]")
            .expect("start cooperative map input evaluation");
        let responses = ["Ada", "Grace"];
        let mut response_index = 0;

        let value = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll cooperative map input evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { request_id, prompt } => {
                    let expected_prompt = if response_index == 0 {
                        "first> "
                    } else {
                        "second> "
                    };
                    assert_eq!(prompt, expected_prompt);
                    session
                        .resume_script_input(
                            &mut evaluation,
                            request_id,
                            ScriptInputResponse::Line(responses[response_index].to_string()),
                        )
                        .expect("resume map callback input");
                    response_index += 1;
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(response_index, 2);
        assert_eq!(
            value,
            Value::from_items(
                responses
                    .into_iter()
                    .map(crate::value::into_wq_string)
                    .collect()
            )
        );
    }

    #[test]
    fn cooperative_composed_callback_uses_atomic_nested_call() {
        let source = r#"f:{[x]int input[x]};g:f+1;map[,"value> ";g]"#;
        let mut synchronous = Session::new();
        synchronous.set_input(Box::new(FixedInput("3")));
        let expected = synchronous
            .eval_script(SourceUnit::named("composed-sync.wq", source))
            .expect("synchronous composed callback evaluation");

        let mut session = Session::new();
        session.set_input(Box::new(FixedInput("3")));
        let mut evaluation = session
            .start_script_evaluation("composed-sliced.wq", source)
            .expect("start cooperative composed callback evaluation");
        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll cooperative composed callback evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("composed callback should use the synchronous input adapter")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
    }

    #[test]
    fn cooperative_linear_context_builtins_yield_and_match_sync() {
        let source = r#"
            xs:til 40;
            mapper:map;
            (
                apply[({[x]x+1};{[x]x*2});5];
                fold[xs;{[a;b]a+b};0];
                scan[xs;{[a;b]a+b};0];
                rscan[xs;{[a;b]a+b};0];
                any[xs;{[x]x=20}];
                all[xs;{[x]x<40}];
                filter[xs;{[x]x%2=0}];
                zipw[xs;reverse xs;{[x;y]x+y}];
                splitw[xs;{[x]x%7=0}];
                findw[((1;2);(3;4));{[x]x=3};3;2];
                rfindw[((1;2);(3;4));{[x]x=3};3;2];
                mapper[xs;{[x]x+1}];
                argparse[
                    (`name:"tool";`args:,(`name:`value;`kind:`positional;`multiple:T;`parse:{[s]int s}));
                    (,"1";,"2";,"3";,"4";,"5";,"6";,"7";,"8")
                ];
                argparse[
                    (`name:"tool";`args:,(`name:`value;`kind:`positional;`parse:{[s]1/0}));
                    ,"bad"
                ]
            )
        "#;
        let expected = Session::new()
            .eval_script(SourceUnit::named("linear-sync.wq", source))
            .expect("synchronous higher-order evaluation");
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("linear-sliced.wq", source)
            .expect("start cooperative higher-order evaluation");
        let mut yields = 0;

        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 2)
                .expect("poll cooperative higher-order evaluation")
            {
                EvaluationPoll::Yielded { work_units } => {
                    assert!(work_units <= 2);
                    yields += 1;
                }
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("linear callbacks should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
        assert!(yields >= 100, "higher-order items should consume work");
    }

    #[test]
    fn cooperative_context_frames_nest_and_unwind_into_try() {
        let source = r#"
            nested:map[((1;2);(3;4));{[row]fold[row;{[a;b]a+b};0]}];
            caught:@t map[(1;2);{[x]1/0}];
            (nested;caught 0)
        "#;
        let expected = Session::new()
            .eval_script(SourceUnit::named("frames-sync.wq", source))
            .expect("synchronous nested context evaluation");
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("frames-sliced.wq", source)
            .expect("start nested context evaluation");

        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll nested context evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("nested context callbacks should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
        assert_eq!(
            session
                .eval_string("1+1")
                .expect("session should be reusable after callback unwind"),
            Value::Int(2)
        );
    }

    #[test]
    fn cooperative_broadcast_callback_errors_keep_builtin_path_context() {
        for source in [
            "map[((1;2);(3;4));{[x]1/0};2]",
            "zipw[((1;2);(3;4));10;{[x;y]1/0};2]",
        ] {
            let expected = Session::new()
                .eval_script(SourceUnit::named("error-sync.wq", source))
                .expect_err("synchronous callback should fail");
            let mut session = Session::new();
            let mut evaluation = session
                .start_script_evaluation("error-sliced.wq", source)
                .expect("start cooperative callback failure");
            let actual = loop {
                match session.poll_script_evaluation(&mut evaluation, 1) {
                    Ok(EvaluationPoll::Yielded { .. }) => {}
                    Ok(EvaluationPoll::AwaitingInput { .. }) => {
                        panic!("failing callback should not request input")
                    }
                    Ok(EvaluationPoll::Paused(_)) => panic!("debugger should be disabled"),
                    Ok(EvaluationPoll::Ready(_)) => panic!("callback should fail"),
                    Err(failure) => break failure,
                }
            };

            assert_eq!(actual.error.err_type, expected.error.err_type);
            assert_eq!(actual.error.src, expected.error.src);
            assert_eq!(actual.error.notes, expected.error.notes);
        }
    }

    #[test]
    fn cooperative_callback_input_error_unwinds_builtin_frame() {
        let mut session = Session::new();
        let mut evaluation = session
            .start_script_evaluation("input-error.wq", "@t map[(\"value> \";\"unused> \");input]")
            .expect("start callback input error evaluation");
        let request_id = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll callback input error evaluation")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { request_id, prompt } => {
                    assert_eq!(prompt, "value> ");
                    break request_id;
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(_) => panic!("callback input should suspend"),
            }
        };
        session
            .resume_script_input(
                &mut evaluation,
                request_id,
                ScriptInputResponse::Error("stdin failed".to_string()),
            )
            .expect("inject callback input error");

        let value = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("finish caught callback input error")
            {
                EvaluationPoll::Yielded { .. } => {}
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("failed callback input should not remain pending")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };
        assert_eq!(
            value.index(&Value::Int(0)),
            Some(Value::Tag(Arc::from("error")))
        );
        assert_eq!(
            session
                .eval_string("6*7")
                .expect("session should be reusable after input failure"),
            Value::Int(42)
        );
    }

    #[test]
    fn cooperative_cliargs_custom_parser_yields_and_matches_sync() {
        let source = r#"
            cliargs[(`name:"tool";`args:,(`name:`value;`kind:`positional;`multiple:T;`parse:{[s]int s}))]
        "#;
        let argv = (0..20).map(|value| value.to_string()).collect::<Vec<_>>();
        let mut synchronous = Session::new();
        synchronous.set_argv(argv.clone());
        let expected = synchronous
            .eval_script(SourceUnit::named("cliargs-sync.wq", source))
            .expect("synchronous cliargs evaluation");

        let mut session = Session::new();
        session.set_argv(argv);
        let mut evaluation = session
            .start_script_evaluation("cliargs-sliced.wq", source)
            .expect("start cooperative cliargs evaluation");
        let mut yields = 0;
        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll cooperative cliargs evaluation")
            {
                EvaluationPoll::Yielded { .. } => yields += 1,
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("cliargs parser should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
        assert!(yields >= 20, "each custom parser call should consume work");
        assert_eq!(session.take_halt_status(), None);
    }

    #[test]
    fn cooperative_asciiplot_callable_sampling_yields_and_matches_sync() {
        let source = "asciiplot[{[x]x*x};`samples:20;`size:(20;8);`color:F]";
        let expected_output = Arc::new(Mutex::new(String::new()));
        let mut synchronous = Session::new();
        synchronous.set_stdout(Box::new(CaptureOutput(Arc::clone(&expected_output))));
        let expected = synchronous
            .eval_script(SourceUnit::named("plot-sync.wq", source))
            .expect("synchronous asciiplot evaluation");

        let actual_output = Arc::new(Mutex::new(String::new()));
        let mut session = Session::new();
        session.set_stdout(Box::new(CaptureOutput(Arc::clone(&actual_output))));
        let mut evaluation = session
            .start_script_evaluation("plot-sliced.wq", source)
            .expect("start cooperative asciiplot evaluation");
        let mut yields = 0;
        let actual = loop {
            match session
                .poll_script_evaluation(&mut evaluation, 1)
                .expect("poll cooperative asciiplot evaluation")
            {
                EvaluationPoll::Yielded { .. } => yields += 1,
                EvaluationPoll::AwaitingInput { .. } => {
                    panic!("plot callback should not request input")
                }
                EvaluationPoll::Paused(_) => panic!("debugger should be disabled"),
                EvaluationPoll::Ready(value) => break value,
            }
        };

        assert_eq!(actual, expected);
        assert_eq!(
            *actual_output.lock().expect("actual output lock"),
            *expected_output.lock().expect("expected output lock")
        );
        assert!(yields >= 20, "each callable sample should consume work");
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
        session.arm_wqdb_next();
        session.eval_string("x+:1").expect("second eval should run");

        assert_eq!(
            reasons.lock().expect("pause reasons lock").as_slice(),
            &[PauseReason::Entry, PauseReason::Entry]
        );
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
            let instruction = debugger
                .instruction_at(loc.pc)
                .map(|instruction| format!("{}{}", instruction.opcode, instruction.operands))
                .unwrap_or_default();
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
            let instruction = debugger
                .instruction_at(loc.pc)
                .map(|instruction| format!("{}{}", instruction.opcode, instruction.operands))
                .unwrap_or_default();
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
            let instruction = debugger
                .instruction_at(loc.pc)
                .map(|instruction| format!("{}{}", instruction.opcode, instruction.operands))
                .unwrap_or_default();
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
    fn dict_unpack_binds_keys_independent_of_dict_order() {
        let mut session = Session::new();

        let result = session
            .eval_string("(`a;`b:renamed):(`b:2;`a:1);(a;renamed)")
            .expect("dict unpack should bind requested keys");

        assert_eq!(result, Value::IntList(Arc::new(vec![1, 2])));
    }

    #[test]
    fn dict_unpack_supports_nested_dict_and_list_patterns() {
        let mut session = Session::new();

        let result = session
            .eval_string(
                "(`api:(`start;`stop);`pair:(left;right);`version):\
                 (`pair:(4;5);`version:3;`api:(`stop:2;`start:1));\
                 (start;stop;left;right;version)",
            )
            .expect("nested dict unpack should bind every leaf");

        assert_eq!(result, Value::IntList(Arc::new(vec![1, 2, 4, 5, 3])));
    }

    #[test]
    fn dict_unpack_evaluates_rhs_once() {
        let mut session = Session::new();

        let result = session
            .eval_string("n:0;f:'{[]n+:1;(`a:n;`b:n)};(`a;`b):f[];(n;a;b)")
            .expect("dict unpack should evaluate its rhs once");

        assert_eq!(result, Value::IntList(Arc::new(vec![1, 1, 1])));
    }

    #[test]
    fn dict_unpack_preflights_keys_before_writing_bindings() {
        let mut session = Session::new();

        let result = session
            .eval_string("a:10;b:20;result:@t ((`a;`missing):(`a:1));(a;b;result 0)")
            .expect("try should catch the missing dict key");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::Int(10),
                Value::Int(20),
                Value::Tag(Arc::from("error")),
            ]))
        );
        assert!(session.vm.unpack_frames.is_empty());
        assert!(
            session
                .bindings()
                .keys()
                .all(|name| !name.starts_with("--"))
        );
    }

    #[test]
    fn list_unpack_preflights_positions_before_writing_bindings() {
        let mut session = Session::new();

        let result = session
            .eval_string("a:10;b:20;result:@t ((a;b):,1);(a;b;result 0)")
            .expect("try should catch the missing list position");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::Int(10),
                Value::Int(20),
                Value::Tag(Arc::from("error")),
            ]))
        );
        assert!(session.vm.unpack_frames.is_empty());
        assert!(
            session
                .bindings()
                .keys()
                .all(|name| !name.starts_with("--"))
        );
    }

    #[test]
    fn uncaught_unpack_preflight_failure_leaves_no_anonymous_state() {
        let mut session = Session::new();
        session
            .eval_string("a:10;b:20")
            .expect("initial bindings should succeed");

        session
            .eval_string("(a;b):,1")
            .expect_err("missing list position should fail");

        assert_eq!(session.bindings().get("a"), Some(&Value::Int(10)));
        assert_eq!(session.bindings().get("b"), Some(&Value::Int(20)));
        assert!(session.vm.unpack_frames.is_empty());
        assert!(
            session
                .bindings()
                .keys()
                .all(|name| !name.starts_with("--"))
        );
    }

    #[test]
    fn list_unpack_evaluates_rhs_once_and_orders_ellipsis_suffix_from_the_end() {
        let mut session = Session::new();

        let result = session
            .eval_string("n:0;f:'{[]n+:1;iota 4};(a;...;b;c):f[];(n;a;b;c)")
            .expect("list unpack should evaluate its rhs once");

        assert_eq!(result, Value::IntList(Arc::new(vec![1, 0, 2, 3])));
        assert!(session.vm.unpack_frames.is_empty());
    }

    #[test]
    fn underscore_is_an_ordinary_unpack_binding() {
        let mut session = Session::new();

        assert_eq!(
            session
                .eval_string("(_;_):(1;2);_")
                .expect("underscore unpack targets should bind"),
            Value::Int(2)
        );
        assert_eq!(
            session
                .eval_string("(`_):(`_:3);_")
                .expect("underscore dict shorthand should bind"),
            Value::Int(3)
        );
        assert_eq!(session.bindings().get("_"), Some(&Value::Int(3)));
    }

    #[test]
    fn underscore_unpack_binding_obeys_function_scope() {
        let mut session = Session::new();

        assert_eq!(
            session
                .eval_string("f:{[xs](_;tail):xs;(_;tail)};f (3;4)")
                .expect("underscore unpack targets should bind locally"),
            Value::IntList(Arc::new(vec![3, 4]))
        );
        assert!(!session.bindings().contains_key("_"));
    }

    #[test]
    fn unpacked_bindings_replace_stale_callable_and_container_facts() {
        let mut session = Session::new();

        let result = session
            .eval_string(
                "x:(10;20);callable:'{x+1};(x;unused):(callable;0);called:x 2;\
                 f:'{x+1};(f;unused):((30;40);0);indexed:f 1;(called;indexed)",
            )
            .expect("postfix resolution should follow the unpacked runtime values");

        assert_eq!(result, Value::IntList(Arc::new(vec![3, 40])));
    }

    #[test]
    fn successful_unpacking_exposes_only_user_bindings() {
        let mut session = Session::new();

        session
            .eval_string("(a;b):(1;2);(`c:d):(`c:3)")
            .expect("list and dict unpacking should succeed");

        let bindings = session.bindings();
        assert_eq!(bindings.get("a"), Some(&Value::Int(1)));
        assert_eq!(bindings.get("b"), Some(&Value::Int(2)));
        assert_eq!(bindings.get("d"), Some(&Value::Int(3)));
        assert!(bindings.keys().all(|name| !name.starts_with("--")));
        assert!(session.vm.unpack_frames.is_empty());
    }

    #[test]
    fn identifiers_and_tags_use_nfc() {
        let mut session = Session::new();

        let result = session
            .eval_string("é:41;e\u{301}+:1;d:(`é:é);(é;d`e\u{301};tag[\"e\u{301}\"])")
            .expect("canonically equivalent names should share bindings and keys");

        assert_eq!(
            result,
            Value::List(Arc::new(vec![
                Value::Int(42),
                Value::Int(42),
                Value::Tag(Arc::from("é")),
            ]))
        );
        assert_eq!(session.bindings().get("é"), Some(&Value::Int(42)));
        assert!(!session.bindings().contains_key("e\u{301}"));
    }

    #[test]
    fn bindings_hide_compiler_only_global_slots() {
        let mut session = Session::new();

        session
            .eval_string("xs:(10;20);i:0;xs[i]+:1")
            .expect("dynamic augmented index assignment should succeed");

        let bindings = session.bindings();
        assert_eq!(
            bindings.get("xs"),
            Some(&Value::IntList(Arc::new(vec![11, 20])))
        );
        assert_eq!(bindings.get("i"), Some(&Value::Int(0)));
        assert!(bindings.keys().all(|name| !name.starts_with("--")));
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
    fn cat_assignment_preserves_aliases_results_and_reference_captures() {
        let mut session = Session::new();

        let aliases = session
            .eval_string("f:{[]a:(1;2);b:a;result:(a,:3);(a;b;result)};f[]")
            .expect("cat assignment with an alias should succeed");
        assert_eq!(
            aliases,
            Value::from_items(vec![
                Value::from_items(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
                Value::from_items(vec![Value::Int(1), Value::Int(2)]),
                Value::from_items(vec![Value::Int(1), Value::Int(2), Value::Int(3)]),
            ])
        );

        let captured = session
            .eval_string("f:{[]s:\"\";push:'{[c]s,:c};push \"a\";push \"b\";s};f[]")
            .expect("captured cat assignment should succeed");
        assert_eq!(captured, crate::value::into_wq_string("ab"));
    }

    #[test]
    fn chars_are_distinct_from_singleton_strings() {
        let mut session = Session::new();

        assert_eq!(
            session.eval_string("\"a\"").expect("char should evaluate"),
            Value::Char('a')
        );
        assert_eq!(
            session
                .eval_string(",\"a\"")
                .expect("singleton string should evaluate"),
            crate::value::into_wq_string("a")
        );
        assert_eq!(
            session
                .eval_string("\"a\"=,\"a\"")
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
            Value::List(std::sync::Arc::new(vec![Value::empty_list()]))
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
