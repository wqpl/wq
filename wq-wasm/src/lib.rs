use std::cell::{Cell, Ref, RefCell, RefMut};

#[cfg(target_arch = "wasm32")]
use js_sys::Function;
use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::console;
use wqpl::builtins::BuiltinPreset;
use wqpl::debugger::{DebugResume, Debugger, PauseEvent};
use wqpl::display::{BoxPrintConfig, apply_box_spec, format_print_result, format_xray_info};
use wqpl::doc::{self, DocKind, DocRenderTarget};
use wqpl::format::{FormatConfig, Formatter};
use wqpl::frontend::{Frontend, SyntaxDisplayKind};
use wqpl::highlight::{
    CursorContext, HighlightEvent, HighlightName, Highlighter,
    cursor_context_at as wqpl_cursor_context_at,
};
use wqpl::interpret::InterpreterKind;
use wqpl::session::dbglog::DebugLogFlags;
#[cfg(target_arch = "wasm32")]
use wqpl::session::stdio::{WqInput, WqIoError, WqOutput};
use wqpl::session::{
    EvaluationFailure, EvaluationPoll as SessionEvaluationPoll, ScriptEvaluation,
    ScriptInputResponse, Session, SourceUnit,
};
use wqpl::style::ColorMode;
use wqpl::symbol::{DefKind, SymbolIndex, SymbolProvenanceKind, UseKind};
use wqpl::value::Value;
use wqpl::wqdb::data::CrashFrame;
use wqpl::wqerror::WqError;

#[wasm_bindgen(typescript_custom_section)]
const TYPESCRIPT_API_TYPES: &str = r#"
/** Half-open UTF-8 byte offsets into the corresponding wq source string. */
export type WqByteSpan = [number, number];

/** Compatibility alias for the original public span name. */
export type WqSpan = WqByteSpan;

export interface WqDiagnosticDataValue {
    display: string;
    category: string;
}

export interface WqStackFrame {
    function: string;
    path: string | null;
    line: number | null;
    column: number | null;
    byte: number | null;
}

export interface WqDiagnostic {
    version: 2;
    kind: string;
    message: string;
    rendered: string;
    source: string | null;
    span: WqByteSpan | null;
    path: string | null;
    notes: string[];
    data: Record<string, WqDiagnosticDataValue>;
    stack: WqStackFrame[];
    cause: WqDiagnostic | null;
}

export interface RenderedValue {
    display: string;
    is_cas: boolean;
    category: string;
    xray: string;
}

export type EvaluationSlice =
    | { status: "yielded"; work_units: number }
    | { status: "awaiting_input"; request_id: string; prompt: string }
    | { status: "ready"; value: RenderedValue };

export interface GlobalBinding {
    name: string;
    display: string;
    category: string;
}

export interface DocTopicInfo {
    id: string;
    title: string;
    kind: "builtin" | "keyword" | "syntax" | "guide";
    group: string;
    summary: string;
    usage: string | null;
    aliases: string[];
}

export interface SymbolDefinition {
    index: number;
    name: string;
    kind: string;
    span: WqByteSpan | null;
    name_span: WqByteSpan | null;
    params: string[] | null;
    parent: number | null;
    provenance: string;
    origin: string | null;
    read_count: number;
    write_count: number;
    occurrence_count: number;
    ref_capture_count: number;
}

export interface SymbolOccurrence {
    span: WqByteSpan;
    def: number;
    kind: string;
}

export interface SymbolError {
    span: WqByteSpan | null;
    kind: string;
    message: string;
}

export interface SymbolAnalysis {
    defs: SymbolDefinition[];
    occurrences: SymbolOccurrence[];
    errors: SymbolError[];
}

export type FrontendDiagnostic = SymbolError;

export interface HighlightSpan {
    span: WqByteSpan;
    kind: string;
}

export type WqCursorContext =
    | "code"
    | "comment"
    | "string"
    | "tag"
    | "fstring-text"
    | "fstring-expression"
    | "meta";
"#;

// JS stream adapters
// ====================================================================================

#[cfg(target_arch = "wasm32")]
struct JsOutput {
    cb: Function,
}

#[cfg(target_arch = "wasm32")]
struct JsStdin {
    cb: Function,
}

// Default console loggers used when no JS callback is provided
#[cfg(target_arch = "wasm32")]
struct ConsoleStdout;
#[cfg(target_arch = "wasm32")]
struct ConsoleStderr;

#[cfg(target_arch = "wasm32")]
impl WqOutput for JsOutput {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        self.cb
            .call1(&JsValue::NULL, &JsValue::from_str(text))
            .map(|_| ())
            .map_err(|error| WqIoError::Other(format!("{error:?}")))
    }
}

#[cfg(target_arch = "wasm32")]
impl WqOutput for ConsoleStdout {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        console::log_1(&text.into());
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
impl WqOutput for ConsoleStderr {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        console::error_1(&text.into());
        Ok(())
    }
}

#[cfg(target_arch = "wasm32")]
impl WqInput for JsStdin {
    fn read_line(&mut self, prompt: &str) -> Result<String, WqIoError> {
        match self.cb.call1(&JsValue::NULL, &JsValue::from_str(prompt)) {
            Ok(val) if val.is_undefined() || val.is_null() => Err(WqIoError::Eof),
            Ok(val) => val.as_string().ok_or_else(|| {
                WqIoError::Other(
                    "stdin callback must return a string, null, or undefined".to_string(),
                )
            }),
            Err(error) => Err(WqIoError::Other(format!("{error:?}"))),
        }
    }
}

// wq frontend API
// ====================================================================================

/// Reusable, evaluator-free language tooling configured with one builtin
/// preset.
#[wasm_bindgen]
pub struct WasmFrontend {
    frontend: Frontend,
    preset: BuiltinPreset,
}

#[wasm_bindgen]
impl WasmFrontend {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_builtins_preset(&self) -> String {
        self.preset.name().to_string()
    }

    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn builtin_preset_names(&self) -> Array {
        strings_to_array(BuiltinPreset::names().iter().copied())
    }

    pub fn set_builtins_preset(&mut self, name: &str) -> Result<String, JsValue> {
        let preset = parse_builtin_preset(name)?;
        self.frontend = Frontend::with_preset(preset);
        self.preset = preset;
        Ok(preset.name().to_string())
    }

    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn builtin_names(&self) -> Array {
        let names = builtin_name_data(&self.frontend);
        strings_to_array(names.iter().map(String::as_str))
    }

    #[wasm_bindgen(unchecked_return_type = "SymbolAnalysis")]
    pub fn analyze_symbols(&self, src: &str) -> Object {
        symbol_analysis_to_js(&symbol_analysis_data(&self.frontend, src))
    }

    pub fn is_complete_input(&self, src: &str) -> bool {
        self.frontend.is_complete_input(src)
    }

    #[wasm_bindgen(unchecked_return_type = "FrontendDiagnostic[]")]
    pub fn diagnostics(&self, src: &str) -> Array {
        frontend_diagnostics_to_js(&frontend_diagnostic_data(&self.frontend, src))
    }

    #[wasm_bindgen(unchecked_return_type = "HighlightSpan[]")]
    pub fn highlight_spans(&self, src: &str) -> Array {
        highlight_spans_to_js(&highlight_span_data(&self.frontend, src))
    }

    pub fn format_wq(&self, src: &str) -> Result<String, JsValue> {
        format_wq_data(src).map_err(|err| wq_error_js(&err, ColorMode::Never))
    }

    #[wasm_bindgen(unchecked_return_type = "WqCursorContext")]
    pub fn cursor_context_at(&self, src: &str, byte_offset: usize) -> String {
        cursor_context_name(wqpl_cursor_context_at(src, byte_offset)).to_string()
    }

    pub fn get_wq_syntax_display(&self, src: &str, kind: &str) -> Result<String, JsValue> {
        let kind = SyntaxDisplayKind::from_name(kind).ok_or_else(|| {
            api_error_js(
                "invalid-syntax-display-kind",
                &format!("unknown syntax display kind '{kind}'; expected ast or cst"),
            )
        })?;
        self.frontend
            .format_syntax_display(src, kind, ColorMode::Always)
            .map_err(|err| wq_error_js(&err, ColorMode::Always))
    }

    pub fn highlight_wq(&self, src: &str) -> String {
        highlight_wq_data(&self.frontend, src)
    }
}

impl Default for WasmFrontend {
    fn default() -> Self {
        let preset = BuiltinPreset::DEFAULT;
        Self {
            frontend: Frontend::with_preset(preset),
            preset,
        }
    }
}

// wq Session API
// ====================================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderedValueData {
    display: String,
    is_cas: bool,
    category: String,
    xray: String,
}

/// Low-level wasm-bindgen session core.
///
/// Browser consumers should construct `WasmWqSession` from `browser.js`, which
/// safely defers disposal requested from a synchronous runtime callback.
#[wasm_bindgen(js_name = WasmWqSessionCore)]
pub struct WasmWqSession {
    box_config: Cell<BoxPrintConfig>,
    session: RefCell<Session>,
    evaluation: RefCell<Option<ScriptEvaluation>>,
}

impl WasmWqSession {
    fn try_session(&self) -> Result<Ref<'_, Session>, JsValue> {
        self.session
            .try_borrow()
            .map_err(|_| reentrant_session_error_js())
    }

    fn try_session_mut(&self) -> Result<RefMut<'_, Session>, JsValue> {
        self.session
            .try_borrow_mut()
            .map_err(|_| reentrant_session_error_js())
    }

    fn ensure_session_idle(&self) -> Result<(), JsValue> {
        let evaluation = self
            .evaluation
            .try_borrow()
            .map_err(|_| reentrant_session_error_js())?;
        if evaluation.is_some() {
            return Err(evaluation_in_progress_error_js());
        }
        drop(evaluation);
        drop(self.try_session()?);
        Ok(())
    }
}

#[wasm_bindgen(js_class = WasmWqSessionCore)]
impl WasmWqSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmWqSession {
        let mut session = Session::new();
        session.set_pause_handler(wasm_wqdb_pause_handler);
        session.set_color_mode(ColorMode::Always);
        WasmWqSession {
            box_config: Cell::new(BoxPrintConfig::default()),
            session: RefCell::new(session),
            evaluation: RefCell::new(None),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_stdout_callback(&self, callback: Option<Function>) -> Result<(), JsValue> {
        let output: Box<dyn WqOutput> = match callback {
            Some(callback) => Box::new(JsOutput { cb: callback }),
            None => Box::new(ConsoleStdout),
        };
        self.try_session_mut()?.set_stdout(output);
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_stderr_callback(&self, callback: Option<Function>) -> Result<(), JsValue> {
        let output: Box<dyn WqOutput> = match callback {
            Some(callback) => Box::new(JsOutput { cb: callback }),
            None => Box::new(ConsoleStderr),
        };
        self.try_session_mut()?.set_stderr(output);
        Ok(())
    }

    #[cfg(target_arch = "wasm32")]
    pub fn set_stdin_callback(&self, callback: Option<Function>) -> Result<(), JsValue> {
        let mut session = self.try_session_mut()?;
        if let Some(callback) = callback {
            session.set_input(Box::new(JsStdin { cb: callback }));
        } else {
            session.clear_input();
        }
        Ok(())
    }

    /// Evaluate source and return its rendered value plus display metadata.
    #[wasm_bindgen(unchecked_return_type = "RenderedValue")]
    pub fn eval_wq(&self, src: &str) -> Result<JsValue, JsValue> {
        self.ensure_session_idle()?;
        let (result, color_mode) = {
            let mut session = self.try_session_mut()?;
            let color_mode = session.stderr_color_mode();
            let result = session
                .eval_script(SourceUnit::named("<wasm>", src))
                .map(|value| render_value(&value, self.box_config.get()));
            (result, color_mode)
        };
        match result {
            Ok(rendered) => Ok(rendered_value_to_js(&rendered).into()),
            Err(failure) => Err(evaluation_failure_js(&failure, color_mode)),
        }
    }

    /// Start a cooperatively evaluated script.
    pub fn start_eval_wq(&self, src: &str) -> Result<(), JsValue> {
        let mut evaluation = self
            .evaluation
            .try_borrow_mut()
            .map_err(|_| reentrant_session_error_js())?;
        if evaluation.is_some() {
            return Err(evaluation_in_progress_error_js());
        }

        let (result, color_mode) = {
            let mut session = self.try_session_mut()?;
            let color_mode = session.stderr_color_mode();
            let result = session.start_script_evaluation("<wasm>", src);
            (result, color_mode)
        };
        match result {
            Ok(started) => {
                *evaluation = Some(started);
                Ok(())
            }
            Err(failure) => Err(evaluation_failure_js(&failure, color_mode)),
        }
    }

    /// Run one bounded work slice of the active evaluation.
    #[wasm_bindgen(unchecked_return_type = "EvaluationSlice")]
    pub fn run_eval_wq_slice(&self, work_budget: usize) -> Result<JsValue, JsValue> {
        if work_budget == 0 {
            return Err(api_error_js(
                "invalid-evaluation-budget",
                "evaluation work budget must be greater than 0",
            ));
        }

        let mut evaluation = self
            .evaluation
            .try_borrow_mut()
            .map_err(|_| reentrant_session_error_js())?;
        let active = evaluation
            .as_mut()
            .ok_or_else(no_active_evaluation_error_js)?;
        let (result, color_mode) = {
            let mut session = self.try_session_mut()?;
            let color_mode = session.stderr_color_mode();
            let result = session.poll_script_evaluation(active, work_budget);
            (result, color_mode)
        };

        match result {
            Ok(SessionEvaluationPoll::Yielded { work_units }) => {
                Ok(evaluation_yielded_to_js(work_units).into())
            }
            Ok(SessionEvaluationPoll::AwaitingInput { request_id, prompt }) => {
                Ok(evaluation_awaiting_input_to_js(request_id, &prompt).into())
            }
            Ok(SessionEvaluationPoll::Ready(value)) => {
                evaluation.take();
                let rendered = render_value(&value, self.box_config.get());
                Ok(evaluation_ready_to_js(&rendered).into())
            }
            Err(failure) => {
                evaluation.take();
                Err(evaluation_failure_js(&failure, color_mode))
            }
        }
    }

    /// Cancel the active cooperative evaluation, if one exists.
    pub fn cancel_eval_wq(&self) -> Result<bool, JsValue> {
        let mut evaluation = self
            .evaluation
            .try_borrow_mut()
            .map_err(|_| reentrant_session_error_js())?;
        let Some(active) = evaluation.as_mut() else {
            return Ok(false);
        };
        let cancelled = self.try_session_mut()?.cancel_script_evaluation(active);
        evaluation.take();
        Ok(cancelled)
    }

    pub fn resume_eval_wq_input(
        &self,
        request_id: &str,
        response_kind: &str,
        value: Option<String>,
    ) -> Result<(), JsValue> {
        let request_id = request_id.parse::<u64>().map_err(|_| {
            api_error_js(
                "invalid-input-request",
                "input request identifier must be an unsigned decimal integer",
            )
        })?;
        let response = match response_kind {
            "line" => ScriptInputResponse::Line(value.ok_or_else(|| {
                api_error_js(
                    "invalid-input-response",
                    "line input response requires a string value",
                )
            })?),
            "eof" => ScriptInputResponse::Eof,
            "interrupted" => ScriptInputResponse::Interrupted,
            "error" => ScriptInputResponse::Error(value.ok_or_else(|| {
                api_error_js(
                    "invalid-input-response",
                    "error input response requires a message",
                )
            })?),
            _ => {
                return Err(api_error_js(
                    "invalid-input-response",
                    "input response kind must be 'line', 'eof', 'interrupted', or 'error'",
                ));
            }
        };

        let mut evaluation = self
            .evaluation
            .try_borrow_mut()
            .map_err(|_| reentrant_session_error_js())?;
        let active = evaluation
            .as_mut()
            .ok_or_else(no_active_evaluation_error_js)?;
        let (result, color_mode) = {
            let mut session = self.try_session_mut()?;
            let color_mode = session.stderr_color_mode();
            let result = session.resume_script_input(active, request_id, response);
            (result, color_mode)
        };
        result.map_err(|failure| evaluation_failure_js(&failure, color_mode))
    }

    /// Configure ANSI styling for runtime output and diagnostics.
    pub fn set_ansi_styles_enabled(&self, on: bool) -> Result<(), JsValue> {
        self.try_session_mut()?.set_color_mode(if on {
            ColorMode::Always
        } else {
            ColorMode::Never
        });
        Ok(())
    }

    pub fn set_debug_flags(&self, spec: &str) -> Result<(), JsValue> {
        let spec = if spec.trim() == "off" { "0" } else { spec };
        match DebugLogFlags::parse(spec) {
            Ok(flags) => {
                self.try_session_mut()?.set_debug_flags(flags);
                Ok(())
            }
            Err(e) => Err(api_error_js("invalid-debug-flags", &e)),
        }
    }

    pub fn apply_debug_flags(&self, spec: &str) -> Result<(), JsValue> {
        let mut session = self.try_session_mut()?;
        let mut flags = session.debug_flags();
        match flags.apply_spec(spec) {
            Ok(()) => {
                session.set_debug_flags(flags);
                Ok(())
            }
            Err(e) => Err(api_error_js("invalid-debug-flags", &e)),
        }
    }

    pub fn get_debug_flags(&self) -> Result<String, JsValue> {
        let flags = self.try_session()?.debug_flags();
        let names = flags.display_names();
        if names.is_empty() {
            Ok("off".to_string())
        } else {
            Ok(names.join(","))
        }
    }

    pub fn backtrace_enabled(&self) -> Result<bool, JsValue> {
        Ok(self.try_session()?.backtrace_enabled())
    }

    pub fn set_backtrace_enabled(&self, on: bool) -> Result<(), JsValue> {
        self.try_session_mut()?.set_backtrace_enabled(on);
        Ok(())
    }

    pub fn get_wqdb_mode(&self) -> Result<bool, JsValue> {
        Ok(self.try_session()?.is_wqdb_enabled())
    }

    pub fn set_wqdb_mode(&self, on: bool) -> Result<(), JsValue> {
        self.try_session_mut()?.set_wqdb(on);
        Ok(())
    }

    pub fn get_dry_mode(&self) -> Result<bool, JsValue> {
        Ok(self.try_session()?.dry_mode())
    }

    pub fn set_dry_mode(&self, on: bool) -> Result<(), JsValue> {
        self.try_session_mut()?.set_dry_mode(on);
        Ok(())
    }

    pub fn reset_workspace(&self) -> Result<(), JsValue> {
        self.try_session_mut()?.reset_workspace();
        Ok(())
    }

    pub fn reset_execution_state(&self) -> Result<(), JsValue> {
        self.try_session_mut()?.reset_execution_state();
        Ok(())
    }

    pub fn get_interpreter_name(&self) -> Result<String, JsValue> {
        Ok(self.try_session()?.interpreter_name().to_string())
    }

    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn interpreter_names(&self) -> Array {
        strings_to_array(InterpreterKind::names().iter().copied())
    }

    pub fn set_interpreter_by_name(&self, name: &str) -> Result<String, JsValue> {
        self.try_session_mut()?
            .set_interpreter_by_name(name)
            .map(str::to_string)
            .map_err(|err| api_error_js("invalid-interpreter", &err))
    }

    pub fn get_builtins_preset(&self) -> Result<String, JsValue> {
        Ok(self.try_session()?.builtins_preset().name().to_string())
    }

    #[wasm_bindgen(unchecked_return_type = "string[]")]
    pub fn builtin_preset_names(&self) -> Array {
        strings_to_array(BuiltinPreset::names().iter().copied())
    }

    pub fn set_builtins_preset(&self, name: &str) -> Result<String, JsValue> {
        let preset = parse_builtin_preset(name)?;
        self.try_session_mut()?.set_builtins_preset(preset);
        Ok(preset.name().to_string())
    }

    /// Return user-defined global bindings sorted by name.
    #[wasm_bindgen(unchecked_return_type = "GlobalBinding[]")]
    pub fn globals(&self) -> Result<Array, JsValue> {
        let session = self.try_session()?;
        Ok(globals_to_js(&global_binding_data(&session)))
    }

    /// Clear user-defined bindings while preserving debug state.
    pub fn clear_bindings(&self) -> Result<(), JsValue> {
        self.try_session_mut()?.clear_bindings();
        Ok(())
    }

    pub fn get_box_flags(&self) -> String {
        self.box_config.get().spec()
    }

    pub fn get_box_summary(&self) -> String {
        self.box_config.get().summary()
    }

    pub fn set_box_flags(&self, spec: &str) -> Result<(), JsValue> {
        let spec = spec.trim();
        let mut config = BoxPrintConfig::off();
        if !spec.is_empty() && spec != "0" && spec != "off" {
            apply_box_spec(&mut config, spec).map_err(|e| api_error_js("invalid-box-flags", &e))?;
        }
        self.ensure_session_idle()?;
        self.box_config.set(config);
        Ok(())
    }

    pub fn apply_box_flags(&self, spec: &str) -> Result<(), JsValue> {
        let mut config = self.box_config.get();
        apply_box_spec(&mut config, spec).map_err(|e| api_error_js("invalid-box-flags", &e))?;
        self.ensure_session_idle()?;
        self.box_config.set(config);
        Ok(())
    }
}

impl Default for WasmWqSession {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_builtin_preset(name: &str) -> Result<BuiltinPreset, JsValue> {
    BuiltinPreset::from_name(name).ok_or_else(|| {
        api_error_js(
            "invalid-builtin-preset",
            &format!(
                "unknown builtin preset '{name}'\nAvailable: {}",
                BuiltinPreset::names().join(", ")
            ),
        )
    })
}

#[cfg(test)]
type TestEvaluationResult<T> = Result<T, Box<EvaluationFailure>>;

#[cfg(test)]
fn eval_wq_script_value(session: &WasmWqSession, src: &str) -> TestEvaluationResult<Value> {
    session
        .session
        .borrow_mut()
        .eval_script(SourceUnit::named("<wasm>", src))
        .map_err(Box::new)
}

fn render_value(value: &Value, config: BoxPrintConfig) -> RenderedValueData {
    RenderedValueData {
        display: format_print_result(value, &config, config.color),
        is_cas: value.is_cas(),
        category: value.category().to_string(),
        xray: format_xray_info(value, config.color),
    }
}

#[cfg(test)]
fn eval_rendered_value(
    session: &WasmWqSession,
    src: &str,
) -> TestEvaluationResult<RenderedValueData> {
    let value = eval_wq_script_value(session, src)?;
    Ok(render_value(&value, session.box_config.get()))
}

fn format_wasm_error(err: &WqError, color_mode: ColorMode) -> String {
    err.render_with_color_mode(color_mode)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GlobalBindingData {
    name: String,
    display: String,
    category: String,
}

const WQ_DIAGNOSTIC_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticValueData {
    display: String,
    category: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticStackFrameData {
    function: String,
    path: Option<String>,
    line: Option<usize>,
    column: Option<usize>,
    byte: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiagnosticData {
    version: u8,
    kind: String,
    message: String,
    rendered: String,
    source: Option<String>,
    span: Option<(usize, usize)>,
    path: Option<String>,
    notes: Vec<String>,
    data: Vec<(String, DiagnosticValueData)>,
    stack: Vec<DiagnosticStackFrameData>,
    cause: Option<Box<DiagnosticData>>,
}

fn global_binding_data(session: &Session) -> Vec<GlobalBindingData> {
    let mut bindings = session
        .bindings()
        .into_iter()
        .map(|(name, value)| GlobalBindingData {
            name,
            display: value.to_string(),
            category: value.category().to_string(),
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    bindings
}

fn evaluation_failure_stack_data(failure: &EvaluationFailure) -> Vec<DiagnosticStackFrameData> {
    let Some(crash) = failure.crash() else {
        return Vec::new();
    };
    crash.frames().iter().map(crash_frame_data).collect()
}

fn crash_frame_data(frame: &CrashFrame) -> DiagnosticStackFrameData {
    let CrashFrame::Located {
        function, source, ..
    } = frame
    else {
        return DiagnosticStackFrameData {
            function: frame.function().to_string(),
            path: None,
            line: None,
            column: None,
            byte: None,
        };
    };
    let (path, line, column, byte) = source.as_ref().map_or((None, None, None, None), |source| {
        let byte = clamp_byte_boundary(&source.source, source.span.start);
        (
            Some(source.path.to_string()),
            Some(source.line),
            Some(source.column),
            Some(byte),
        )
    });
    DiagnosticStackFrameData {
        function: function.to_string(),
        path,
        line,
        column,
        byte,
    }
}

fn clamp_byte_boundary(source: &str, byte: usize) -> usize {
    let mut byte = byte.min(source.len());
    while !source.is_char_boundary(byte) {
        byte = byte.saturating_sub(1);
    }
    byte
}

fn diagnostic_data(
    err: &WqError,
    color_mode: ColorMode,
    stack: Vec<DiagnosticStackFrameData>,
) -> DiagnosticData {
    DiagnosticData {
        version: WQ_DIAGNOSTIC_VERSION,
        kind: err.err_type.name().to_string(),
        message: err
            .msg
            .clone()
            .unwrap_or_else(|| err.err_type.name().to_string()),
        rendered: format_wasm_error(err, color_mode),
        source: err.src.clone(),
        span: err.span,
        path: err.source_ctx.as_deref().map(|source| source.path.clone()),
        notes: err.notes.as_ref().clone(),
        data: err
            .data
            .iter()
            .map(|(name, value)| {
                (
                    name.to_string(),
                    DiagnosticValueData {
                        display: value.to_string(),
                        category: value.category().to_string(),
                    },
                )
            })
            .collect(),
        stack,
        cause: None,
    }
}

fn evaluation_failure_diagnostic_data(
    failure: &EvaluationFailure,
    color_mode: ColorMode,
) -> DiagnosticData {
    diagnostic_data(
        &failure.error,
        color_mode,
        evaluation_failure_stack_data(failure),
    )
}

fn api_diagnostic_data(kind: &str, message: &str) -> DiagnosticData {
    DiagnosticData {
        version: WQ_DIAGNOSTIC_VERSION,
        kind: kind.to_string(),
        message: message.to_string(),
        rendered: message.to_string(),
        source: None,
        span: None,
        path: None,
        notes: Vec::new(),
        data: Vec::new(),
        stack: Vec::new(),
        cause: None,
    }
}

fn set_js_property(object: &Object, key: &str, value: &JsValue) {
    Reflect::set(object, &JsValue::from_str(key), value)
        .expect("setting a property on a plain JavaScript object should succeed");
}

fn strings_to_array<'a>(values: impl IntoIterator<Item = &'a str>) -> Array {
    let array = Array::new();
    for value in values {
        array.push(&JsValue::from_str(value));
    }
    array
}

fn optional_string_js(value: Option<&str>) -> JsValue {
    value.map_or(JsValue::NULL, JsValue::from_str)
}

fn usize_js(value: usize) -> JsValue {
    JsValue::from_f64(value as f64)
}

fn optional_usize_js(value: Option<usize>) -> JsValue {
    value.map_or(JsValue::NULL, usize_js)
}

fn span_js(span: (usize, usize)) -> Array {
    let value = Array::new_with_length(2);
    value.set(0, usize_js(span.0));
    value.set(1, usize_js(span.1));
    value
}

fn optional_span_js(span: Option<(usize, usize)>) -> JsValue {
    span.map_or(JsValue::NULL, |span| span_js(span).into())
}

fn globals_to_js(bindings: &[GlobalBindingData]) -> Array {
    let result = Array::new();
    for binding in bindings {
        let object = Object::new();
        set_js_property(&object, "name", &JsValue::from_str(&binding.name));
        set_js_property(&object, "display", &JsValue::from_str(&binding.display));
        set_js_property(&object, "category", &JsValue::from_str(&binding.category));
        result.push(&object);
    }
    result
}

fn rendered_value_to_js(value: &RenderedValueData) -> Object {
    let object = Object::new();
    set_js_property(&object, "display", &JsValue::from_str(&value.display));
    set_js_property(&object, "is_cas", &JsValue::from_bool(value.is_cas));
    set_js_property(&object, "category", &JsValue::from_str(&value.category));
    set_js_property(&object, "xray", &JsValue::from_str(&value.xray));
    object
}

fn evaluation_yielded_to_js(work_units: usize) -> Object {
    let object = Object::new();
    set_js_property(&object, "status", &JsValue::from_str("yielded"));
    set_js_property(&object, "work_units", &usize_js(work_units));
    object
}

fn evaluation_awaiting_input_to_js(request_id: u64, prompt: &str) -> Object {
    let object = Object::new();
    set_js_property(&object, "status", &JsValue::from_str("awaiting_input"));
    set_js_property(
        &object,
        "request_id",
        &JsValue::from_str(&request_id.to_string()),
    );
    set_js_property(&object, "prompt", &JsValue::from_str(prompt));
    object
}

fn evaluation_ready_to_js(value: &RenderedValueData) -> Object {
    let object = Object::new();
    set_js_property(&object, "status", &JsValue::from_str("ready"));
    set_js_property(&object, "value", &rendered_value_to_js(value).into());
    object
}

fn api_error_js(kind: &str, message: &str) -> JsValue {
    diagnostic_to_js(&api_diagnostic_data(kind, message)).into()
}

fn reentrant_session_error_js() -> JsValue {
    api_error_js(
        "reentrant-session-access",
        "session methods cannot be called reentrantly from an active session callback",
    )
}

fn evaluation_in_progress_error_js() -> JsValue {
    api_error_js(
        "evaluation-in-progress",
        "session already has an active evaluation",
    )
}

fn no_active_evaluation_error_js() -> JsValue {
    api_error_js(
        "no-active-evaluation",
        "session does not have an active evaluation",
    )
}

fn wq_error_js(err: &WqError, color_mode: ColorMode) -> JsValue {
    wq_error_with_stack_js(err, color_mode, Vec::new())
}

fn evaluation_failure_js(failure: &EvaluationFailure, color_mode: ColorMode) -> JsValue {
    diagnostic_to_js(&evaluation_failure_diagnostic_data(failure, color_mode)).into()
}

fn wq_error_with_stack_js(
    err: &WqError,
    color_mode: ColorMode,
    stack: Vec<DiagnosticStackFrameData>,
) -> JsValue {
    diagnostic_to_js(&diagnostic_data(err, color_mode, stack)).into()
}

fn diagnostic_to_js(diagnostic: &DiagnosticData) -> Object {
    let object = Object::new();
    set_js_property(
        &object,
        "version",
        &usize_js(usize::from(diagnostic.version)),
    );
    set_js_property(&object, "kind", &JsValue::from_str(&diagnostic.kind));
    set_js_property(&object, "message", &JsValue::from_str(&diagnostic.message));
    set_js_property(
        &object,
        "rendered",
        &JsValue::from_str(&diagnostic.rendered),
    );
    set_js_property(
        &object,
        "source",
        &optional_string_js(diagnostic.source.as_deref()),
    );
    set_js_property(&object, "span", &optional_span_js(diagnostic.span));
    set_js_property(
        &object,
        "path",
        &optional_string_js(diagnostic.path.as_deref()),
    );
    let notes = strings_to_array(diagnostic.notes.iter().map(String::as_str));
    set_js_property(&object, "notes", &notes);

    let data = Object::new();
    for (name, value) in &diagnostic.data {
        let item = Object::new();
        set_js_property(&item, "display", &JsValue::from_str(&value.display));
        set_js_property(&item, "category", &JsValue::from_str(&value.category));
        set_js_property(&data, name, &item);
    }
    set_js_property(&object, "data", &data);

    let stack = Array::new();
    for frame in &diagnostic.stack {
        let item = Object::new();
        set_js_property(&item, "function", &JsValue::from_str(&frame.function));
        set_js_property(&item, "path", &optional_string_js(frame.path.as_deref()));
        set_js_property(&item, "line", &optional_usize_js(frame.line));
        set_js_property(&item, "column", &optional_usize_js(frame.column));
        set_js_property(&item, "byte", &optional_usize_js(frame.byte));
        stack.push(&item);
    }
    set_js_property(&object, "stack", &stack);

    let cause = diagnostic
        .cause
        .as_deref()
        .map_or(JsValue::NULL, |cause| diagnostic_to_js(cause).into());
    set_js_property(&object, "cause", &cause);
    object
}

fn wasm_wqdb_pause_handler(event: PauseEvent, debugger: &mut Debugger<'_>) -> DebugResume {
    let loc = event.location;
    let name = debugger.function_name(loc.chunk);
    let _ = debugger.write_stderr_line(
        "wqdb: paused; interactive browser debugger shell is not available, continuing",
    );
    let _ = debugger.write_stderr_line(&wqpl::wqdb::format_frame(
        debugger.debug_info(),
        loc,
        &name,
        true,
        debugger.color_mode(),
    ));
    DebugResume::Continue
}

/// Version string for splash and title
#[wasm_bindgen]
pub fn get_wq_ver() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[wasm_bindgen]
pub fn get_doc_markdown(query: &str) -> Result<String, JsValue> {
    let topic = doc::resolve(query).ok_or_else(|| {
        api_error_js("unknown-doc-topic", &format!("unknown doc topic '{query}'"))
    })?;
    Ok(doc::render_markdown(&topic, DocRenderTarget::Web))
}

#[wasm_bindgen(unchecked_return_type = "DocTopicInfo[]")]
pub fn doc_index() -> Array {
    doc_index_to_js(&doc_index_data())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DocTopicData {
    id: String,
    title: String,
    kind: &'static str,
    group: String,
    summary: String,
    usage: Option<String>,
    aliases: Vec<String>,
}

fn builtin_name_data(frontend: &Frontend) -> Vec<String> {
    let mut names = frontend.builtins().list_functions();
    names.sort();
    names
}

fn doc_index_data() -> Vec<DocTopicData> {
    doc::all_topics()
        .into_iter()
        .map(|topic| DocTopicData {
            id: topic.id,
            title: topic.title,
            kind: doc_kind_name(topic.kind),
            group: topic.group,
            summary: topic.summary,
            usage: topic.builtin.map(|builtin| builtin.usage().to_string()),
            aliases: topic.aliases,
        })
        .collect()
}

fn doc_index_to_js(topics: &[DocTopicData]) -> Array {
    let result = Array::new();
    for topic in topics {
        let object = Object::new();
        set_js_property(&object, "id", &JsValue::from_str(&topic.id));
        set_js_property(&object, "title", &JsValue::from_str(&topic.title));
        set_js_property(&object, "kind", &JsValue::from_str(topic.kind));
        set_js_property(&object, "group", &JsValue::from_str(&topic.group));
        set_js_property(&object, "summary", &JsValue::from_str(&topic.summary));
        set_js_property(
            &object,
            "usage",
            &optional_string_js(topic.usage.as_deref()),
        );
        let aliases = strings_to_array(topic.aliases.iter().map(String::as_str));
        set_js_property(&object, "aliases", &aliases);
        result.push(&object);
    }
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolDefinitionData {
    index: usize,
    name: String,
    kind: &'static str,
    span: Option<(usize, usize)>,
    name_span: Option<(usize, usize)>,
    params: Option<Vec<String>>,
    parent: Option<usize>,
    provenance: &'static str,
    origin: Option<String>,
    read_count: usize,
    write_count: usize,
    occurrence_count: usize,
    ref_capture_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolOccurrenceData {
    span: (usize, usize),
    def: usize,
    kind: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolErrorData {
    span: Option<(usize, usize)>,
    kind: &'static str,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolAnalysisData {
    defs: Vec<SymbolDefinitionData>,
    occurrences: Vec<SymbolOccurrenceData>,
    errors: Vec<SymbolErrorData>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HighlightSpanData {
    span: (usize, usize),
    kind: &'static str,
}

fn symbol_analysis_data(frontend: &Frontend, src: &str) -> SymbolAnalysisData {
    match frontend.analyze_symbols(src) {
        Ok(index) => symbol_index_data(&index),
        Err(err) => SymbolAnalysisData {
            defs: Vec::new(),
            occurrences: Vec::new(),
            errors: vec![symbol_error_data(&err, err.span)],
        },
    }
}

fn symbol_error_data(error: &WqError, span: Option<(usize, usize)>) -> SymbolErrorData {
    SymbolErrorData {
        span,
        kind: error.err_type.name(),
        message: error.msg.clone().unwrap_or_default(),
    }
}

fn frontend_diagnostic_data(frontend: &Frontend, src: &str) -> Vec<SymbolErrorData> {
    match frontend.analyze_symbols(src) {
        Ok(index) => index
            .errors
            .iter()
            .map(|(span, err)| symbol_error_data(err, Some(*span)))
            .collect(),
        Err(err) => vec![symbol_error_data(&err, err.span)],
    }
}

fn symbol_index_data(index: &SymbolIndex) -> SymbolAnalysisData {
    let occurrences = index.occurrences();
    let mut defs = Vec::new();
    for (def_idx, def) in index.defs.iter().enumerate() {
        if !is_user_symbol(def.kind, &def.name) {
            continue;
        }
        let provenance = index.def_provenance(def_idx);
        let read_count = occurrences
            .iter()
            .filter(|occurrence| occurrence.def_idx == def_idx && occurrence.kind.is_read())
            .count();
        let write_count = occurrences
            .iter()
            .filter(|occurrence| occurrence.def_idx == def_idx && occurrence.kind.is_write())
            .count();
        let occurrence_count = occurrences
            .iter()
            .filter(|occurrence| occurrence.def_idx == def_idx)
            .count();

        defs.push(SymbolDefinitionData {
            index: def_idx,
            name: def.name.clone(),
            kind: def_kind_name(def.kind),
            span: def.span,
            name_span: def.name_span,
            params: def.params.clone(),
            parent: def.parent,
            provenance: provenance
                .as_ref()
                .map(|p| provenance_kind_name(p.kind))
                .unwrap_or("unknown"),
            origin: provenance.and_then(|p| p.origin.map(|origin| origin.to_string())),
            read_count,
            write_count,
            occurrence_count,
            ref_capture_count: index.ref_capture_count(def_idx),
        });
    }

    let mut user_occurrences = Vec::new();
    for occurrence in &occurrences {
        let Some(def) = index.defs.get(occurrence.def_idx) else {
            continue;
        };
        if !is_user_symbol(def.kind, &def.name) {
            continue;
        }
        user_occurrences.push(SymbolOccurrenceData {
            span: occurrence.span,
            def: occurrence.def_idx,
            kind: use_kind_name(occurrence.kind),
        });
    }

    let errors = index
        .errors
        .iter()
        .map(|(span, err)| symbol_error_data(err, Some(*span)))
        .collect();

    SymbolAnalysisData {
        defs,
        occurrences: user_occurrences,
        errors,
    }
}

fn symbol_analysis_to_js(data: &SymbolAnalysisData) -> Object {
    let object = Object::new();
    let defs = Array::new();
    for def in &data.defs {
        let item = Object::new();
        set_js_property(&item, "index", &usize_js(def.index));
        set_js_property(&item, "name", &JsValue::from_str(&def.name));
        set_js_property(&item, "kind", &JsValue::from_str(def.kind));
        set_js_property(&item, "span", &optional_span_js(def.span));
        set_js_property(&item, "name_span", &optional_span_js(def.name_span));
        let params = def.params.as_ref().map_or(JsValue::NULL, |params| {
            strings_to_array(params.iter().map(String::as_str)).into()
        });
        set_js_property(&item, "params", &params);
        set_js_property(&item, "parent", &optional_usize_js(def.parent));
        set_js_property(&item, "provenance", &JsValue::from_str(def.provenance));
        set_js_property(&item, "origin", &optional_string_js(def.origin.as_deref()));
        set_js_property(&item, "read_count", &usize_js(def.read_count));
        set_js_property(&item, "write_count", &usize_js(def.write_count));
        set_js_property(&item, "occurrence_count", &usize_js(def.occurrence_count));
        set_js_property(&item, "ref_capture_count", &usize_js(def.ref_capture_count));
        defs.push(&item);
    }

    let occurrences = Array::new();
    for occurrence in &data.occurrences {
        let item = Object::new();
        set_js_property(&item, "span", &span_js(occurrence.span));
        set_js_property(&item, "def", &usize_js(occurrence.def));
        set_js_property(&item, "kind", &JsValue::from_str(occurrence.kind));
        occurrences.push(&item);
    }

    let errors = frontend_diagnostics_to_js(&data.errors);

    set_js_property(&object, "defs", &defs);
    set_js_property(&object, "occurrences", &occurrences);
    set_js_property(&object, "errors", &errors);
    object
}

fn frontend_diagnostics_to_js(diagnostics: &[SymbolErrorData]) -> Array {
    let result = Array::new();
    for diagnostic in diagnostics {
        let item = Object::new();
        set_js_property(&item, "span", &optional_span_js(diagnostic.span));
        set_js_property(&item, "kind", &JsValue::from_str(diagnostic.kind));
        set_js_property(&item, "message", &JsValue::from_str(&diagnostic.message));
        result.push(&item);
    }
    result
}

fn highlight_spans_to_js(spans: &[HighlightSpanData]) -> Array {
    let result = Array::new();
    for span in spans {
        let item = Object::new();
        set_js_property(&item, "span", &span_js(span.span));
        set_js_property(&item, "kind", &JsValue::from_str(span.kind));
        result.push(&item);
    }
    result
}

fn is_user_symbol(kind: DefKind, name: &str) -> bool {
    kind != DefKind::Builtin && !name.starts_with("--")
}

fn def_kind_name(kind: DefKind) -> &'static str {
    match kind {
        DefKind::Assignment => "assignment",
        DefKind::Function => "function",
        DefKind::Parameter => "parameter",
        DefKind::ImplicitParam => "implicit-parameter",
        DefKind::LoopCounter => "loop-counter",
        DefKind::Builtin => "builtin",
    }
}

fn use_kind_name(kind: UseKind) -> &'static str {
    match kind {
        UseKind::Read => "read",
        UseKind::Write => "write",
        UseKind::OuterRead => "outer-read",
        UseKind::OuterWrite => "outer-write",
        UseKind::RefCaptureRead => "ref-read",
        UseKind::RefCaptureWrite => "ref-write",
    }
}

fn provenance_kind_name(kind: SymbolProvenanceKind) -> &'static str {
    match kind {
        SymbolProvenanceKind::Builtin => "builtin",
        SymbolProvenanceKind::Global => "global",
        SymbolProvenanceKind::Local => "local",
        SymbolProvenanceKind::Parameter => "parameter",
        SymbolProvenanceKind::ImplicitParameter => "implicit-parameter",
        SymbolProvenanceKind::LoopCounter => "loop-counter",
    }
}

fn doc_kind_name(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Builtin => "builtin",
        DocKind::Keyword => "keyword",
        DocKind::Syntax => "syntax",
        DocKind::Guide => "guide",
    }
}

fn format_wq_data(src: &str) -> Result<String, WqError> {
    Formatter::new(FormatConfig::default()).format_script(src)
}

fn cursor_context_name(context: CursorContext) -> &'static str {
    match context {
        CursorContext::Code => "code",
        CursorContext::Comment => "comment",
        CursorContext::String => "string",
        CursorContext::Tag => "tag",
        CursorContext::FStringText => "fstring-text",
        CursorContext::FStringExpr => "fstring-expression",
        CursorContext::Meta => "meta",
    }
}

fn highlight_events(frontend: &Frontend, src: &str) -> Vec<HighlightEvent> {
    let highlighter = Highlighter::with_builtins(frontend.builtins().clone());
    let semantic_spans = if src.contains('{') || src.contains('\'') {
        frontend
            .analyze_symbols(src)
            .map(|index| index.semantic_highlight_spans())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    highlighter.highlight_with_semantic_spans(src, &semantic_spans)
}

fn highlight_span_data(frontend: &Frontend, src: &str) -> Vec<HighlightSpanData> {
    let events = highlight_events(frontend, src);
    let mut spans = Vec::new();
    let mut stack: Vec<HighlightName> = Vec::new();

    for event in events {
        match event {
            HighlightEvent::HighlightStart(name) => stack.push(name),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                if let Some(&name) = stack.last() {
                    spans.push(HighlightSpanData {
                        span: (start, end),
                        kind: highlight_kind_name(name),
                    });
                }
            }
        }
    }
    spans
}

fn highlight_wq_data(frontend: &Frontend, src: &str) -> String {
    let events = highlight_events(frontend, src);
    let mut out = String::with_capacity(src.len() * 2);
    let bytes = src.as_bytes();
    let mut stack: Vec<HighlightName> = Vec::new();

    for ev in events {
        match ev {
            HighlightEvent::HighlightStart(h) => stack.push(h),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                let s = std::str::from_utf8(&bytes[start..end]).unwrap_or("");
                if let Some(&name) = stack.last() {
                    out.push_str("<span class=\"");
                    out.push_str(class_for_name(name));
                    out.push_str("\">");
                    out.push_str(&escape_html(s));
                    out.push_str("</span>");
                } else {
                    out.push_str(&escape_html(s));
                }
            }
        }
    }
    out
}

fn highlight_kind_name(name: HighlightName) -> &'static str {
    class_for_name(name)
        .strip_prefix("hl-")
        .expect("every wq highlight class starts with 'hl-'")
}

fn class_for_name(name: HighlightName) -> &'static str {
    match name {
        HighlightName::Comment => "hl-comment",
        HighlightName::ConstantBuiltin => "hl-constant-builtin",
        HighlightName::FunctionBuiltin => "hl-function-builtin",
        HighlightName::CasSpecial => "hl-cas-special",
        HighlightName::CasConstant => "hl-cas-constant",
        HighlightName::CasFunction => "hl-cas-function",
        HighlightName::CasVariable => "hl-cas-variable",
        HighlightName::Keyword => "hl-keyword",
        HighlightName::KeywordReturn => "hl-keyword-return",
        HighlightName::KeywordDebug => "hl-keyword-debug",
        HighlightName::Number => "hl-number",
        HighlightName::Bool => "hl-bool",
        HighlightName::Operator => "hl-operator",
        HighlightName::OperatorPipe => "hl-operator-pipe",
        HighlightName::PunctuationBracket => "hl-punctuation-bracket",
        HighlightName::PunctuationBracket1 => "hl-punctuation-bracket-1",
        HighlightName::PunctuationBracket2 => "hl-punctuation-bracket-2",
        HighlightName::PunctuationBracket3 => "hl-punctuation-bracket-3",
        HighlightName::PunctuationBracket4 => "hl-punctuation-bracket-4",
        HighlightName::PunctuationBracket5 => "hl-punctuation-bracket-5",
        HighlightName::PunctuationBracket6 => "hl-punctuation-bracket-6",
        HighlightName::PunctuationDelimiter => "hl-punctuation-delimiter",
        HighlightName::PunctuationSpecial => "hl-punctuation-special",
        HighlightName::String => "hl-string",
        HighlightName::StringEscape => "hl-string-escape",
        HighlightName::InvalidString => "hl-string-invalid",
        HighlightName::Character => "hl-character",
        HighlightName::InvalidCharacter => "hl-character-invalid",
        HighlightName::Tag => "hl-tag",
        HighlightName::Variable => "hl-variable",
        HighlightName::VariableRefCapture => "hl-variable-ref-capture",
        HighlightName::VariableParameter => "hl-variable-parameter",
        HighlightName::Meta => "hl-meta",
    }
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use wqpl::session::stdio::{WqIoError, WqOutput};

    use super::*;

    fn default_frontend() -> Frontend {
        Frontend::default()
    }

    struct CapturedOutput {
        out: Arc<Mutex<String>>,
    }

    impl WqOutput for CapturedOutput {
        fn write(&mut self, text: &str) -> Result<(), WqIoError> {
            self.out
                .lock()
                .expect("stderr capture lock should not be poisoned")
                .push_str(text);
            Ok(())
        }
    }

    #[test]
    fn doc_exports_smoke() {
        let topics = doc_index_data();
        let map = topics
            .iter()
            .find(|topic| topic.id == "builtin.map")
            .expect("map topic should be exported");
        assert_eq!(map.usage.as_deref(), Some("map[xs;f;d?]"));
        assert!(topics.iter().any(|topic| topic.id == "at-return"));

        let markdown = get_doc_markdown("map").expect("map doc renders");
        assert!(markdown.contains("map builtin"));
        assert!(markdown.contains("map[xs;f;d?]"));
    }

    #[test]
    fn symbol_analysis_exports_user_symbol_details() {
        let frontend = default_frontend();
        let analysis = symbol_analysis_data(&frontend, "g:1; f:{[x] y:x+g; y}; f[2]");

        let function = analysis
            .defs
            .iter()
            .find(|def| def.name == "f")
            .expect("function definition should be exported");
        assert_eq!(function.kind, "function");
        assert_eq!(
            function.params.as_deref(),
            Some(["x".to_string()].as_slice())
        );

        let local = analysis
            .defs
            .iter()
            .find(|def| def.name == "y")
            .expect("local definition should be exported");
        assert_eq!(local.provenance, "local");
        assert_eq!(local.origin.as_deref(), Some("f"));
        assert!(analysis.occurrences.iter().any(|item| item.kind == "read"));
        assert!(analysis.occurrences.iter().any(|item| item.kind == "write"));
    }

    #[test]
    fn symbol_analysis_returns_structured_parse_errors() {
        let frontend = default_frontend();
        let analysis = symbol_analysis_data(&frontend, "a:1\nd:'{a}\nb:\"");

        assert!(analysis.defs.is_empty());
        assert!(analysis.occurrences.is_empty());
        assert_eq!(analysis.errors.len(), 1);
        assert_eq!(analysis.errors[0].kind, "eof");
        assert_eq!(
            analysis.errors[0].message,
            "string is not properly terminated"
        );
        assert!(!analysis.errors[0].message.contains('\u{1b}'));
    }

    #[test]
    fn globals_are_sorted_structured_data() {
        let session = WasmWqSession::new();
        eval_wq_script_value(&session, "z:2;a:1").expect("bindings should evaluate");

        let bindings = global_binding_data(&session.session.borrow());
        let names = bindings
            .iter()
            .map(|binding| binding.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, ["a", "z"]);
        assert_eq!(bindings[0].display, "1");
        assert_eq!(bindings[0].category, "int");
    }

    #[test]
    fn builtin_names_are_sorted_data() {
        let frontend = default_frontend();
        let names = builtin_name_data(&frontend);
        assert!(names.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(names.iter().any(|name| name == "map"));
    }

    #[test]
    fn reusable_frontend_replaces_its_builtin_configuration() {
        let mut frontend = WasmFrontend::new();
        assert_eq!(frontend.get_builtins_preset(), "all");
        assert!(frontend.frontend.builtins().is_enabled_name("print"));

        let selected = frontend
            .set_builtins_preset("minimal")
            .expect("minimal preset should be accepted");

        assert_eq!(selected, "minimal");
        assert_eq!(frontend.get_builtins_preset(), "minimal");
        assert!(!frontend.frontend.builtins().is_enabled_name("print"));
        assert!(
            !builtin_name_data(&frontend.frontend)
                .iter()
                .any(|name| name == "print")
        );
    }

    #[test]
    fn frontend_completeness_distinguishes_eof_from_complete_errors() {
        let frontend = WasmFrontend::new();

        assert!(!frontend.is_complete_input("f:{[x]"));
        assert!(frontend.is_complete_input(")"));
    }

    #[test]
    fn frontend_diagnostics_preserve_recoverable_error_details() {
        let frontend = default_frontend();
        let diagnostics = frontend_diagnostic_data(&frontend, "a:1\nd:'{a}\nb:\"");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "eof");
        assert_eq!(diagnostics[0].message, "string is not properly terminated");
        assert_eq!(diagnostics[0].span, Some((13, 14)));
    }

    #[test]
    fn structured_highlights_include_semantic_identifier_spans() {
        let frontend = default_frontend();
        let source = "a:1; f:'{[x] x+a}";
        let spans = highlight_span_data(&frontend, source);

        let parameter_sources = spans
            .iter()
            .filter(|span| span.kind == "variable-parameter")
            .map(|span| &source[span.span.0..span.span.1])
            .collect::<Vec<_>>();
        let ref_sources = spans
            .iter()
            .filter(|span| span.kind == "variable-ref-capture")
            .map(|span| &source[span.span.0..span.span.1])
            .collect::<Vec<_>>();

        assert_eq!(parameter_sources, ["x", "x"]);
        assert_eq!(ref_sources, ["a"]);
    }

    #[test]
    fn frontend_formatting_uses_the_canonical_formatter() {
        let formatted = format_wq_data("(1;2)|has?@1[2]\ntil(2;2;2)|has?@2 2")
            .expect("formatting should succeed");

        assert_eq!(formatted, "(1;2)|has?@1 2\ntil (2;2;2)|has?@2 2");
    }

    #[test]
    fn frontend_cursor_context_names_match_editor_contexts() {
        assert_eq!(
            cursor_context_name(wqpl_cursor_context_at("\"abc", 2)),
            "string"
        );

        let source = "@f\"value {x}\"";
        let x = source.find('x').expect("format expression identifier");
        assert_eq!(
            cursor_context_name(wqpl_cursor_context_at(source, x)),
            "fstring-expression"
        );
    }

    #[test]
    fn html_highlighting_honors_the_frontend_builtin_preset() {
        let all = Frontend::with_preset(BuiltinPreset::All);
        let minimal = Frontend::with_preset(BuiltinPreset::Minimal);

        assert!(highlight_wq_data(&all, "print[]").contains("hl-function-builtin"));
        assert!(!highlight_wq_data(&minimal, "print[]").contains("hl-function-builtin"));
        assert!(highlight_wq_data(&minimal, "print[]").contains("hl-variable"));
    }

    #[test]
    fn sessions_keep_debug_flags_and_output_isolated() {
        let first_output = Arc::new(Mutex::new(String::new()));
        let second_output = Arc::new(Mutex::new(String::new()));
        let first_diagnostics = Arc::new(Mutex::new(String::new()));
        let second_diagnostics = Arc::new(Mutex::new(String::new()));
        let first = WasmWqSession::new();
        let second = WasmWqSession::new();
        first
            .session
            .borrow_mut()
            .set_stdout(Box::new(CapturedOutput {
                out: Arc::clone(&first_output),
            }));
        second
            .session
            .borrow_mut()
            .set_stdout(Box::new(CapturedOutput {
                out: Arc::clone(&second_output),
            }));
        first
            .session
            .borrow_mut()
            .set_stderr(Box::new(CapturedOutput {
                out: Arc::clone(&first_diagnostics),
            }));
        second
            .session
            .borrow_mut()
            .set_stderr(Box::new(CapturedOutput {
                out: Arc::clone(&second_diagnostics),
            }));

        first
            .set_debug_flags("ast")
            .expect("first session should accept debug flags");
        assert_eq!(
            first.get_debug_flags().expect("session should be idle"),
            "ast"
        );
        assert_eq!(
            second.get_debug_flags().expect("session should be idle"),
            "off"
        );

        eval_wq_script_value(&first, "echo \"first\"").expect("first output should succeed");
        eval_wq_script_value(&second, "echo \"second\"").expect("second output should succeed");
        assert_eq!(&*first_output.lock().expect("first output lock"), "first\n");
        assert_eq!(
            &*second_output.lock().expect("second output lock"),
            "second\n"
        );
        assert!(
            first_diagnostics
                .lock()
                .expect("first diagnostics lock")
                .contains("AST @ fold - final")
        );
        assert!(
            second_diagnostics
                .lock()
                .expect("second diagnostics lock")
                .is_empty()
        );
    }

    #[test]
    fn lifecycle_operations_have_explicit_scopes() {
        let session = WasmWqSession::new();
        eval_wq_script_value(&session, "a:1").expect("binding should evaluate");
        session
            .set_debug_flags("ast")
            .expect("debug flags should be accepted");
        session.set_dry_mode(false).expect("session should be idle");

        session
            .reset_execution_state()
            .expect("session should be idle");
        assert_eq!(global_binding_data(&session.session.borrow()).len(), 1);

        session.reset_workspace().expect("session should be idle");
        assert!(global_binding_data(&session.session.borrow()).is_empty());
        assert_eq!(
            session.get_debug_flags().expect("session should be idle"),
            "ast"
        );
        assert!(!session.get_dry_mode().expect("session should be idle"));
    }

    #[test]
    fn syntax_display_export_returns_parse_trees_without_dry_output() {
        let frontend = WasmFrontend::new();
        let ast = frontend
            .get_wq_syntax_display("1+2", "ast")
            .expect("AST display should parse");
        assert!(ast.contains("AST @ fold - final"));
        assert!(ast.contains("LIT[Int(3)]"));
        assert!(ast.contains("\x1b["));
        assert!(!ast.contains("dry: skipped execution"));

        let cst = frontend
            .get_wq_syntax_display("1+2", "cst")
            .expect("CST display should parse");
        assert!(cst.contains("CST"));
        assert!(cst.contains("BINARY_EXPR"));
        assert!(cst.contains("\x1b["));
        assert!(!cst.contains("dry: skipped execution"));
    }

    #[test]
    fn wasm_error_formatting_follows_general_color_mode() {
        let session = WasmWqSession::new();
        let err = eval_wq_script_value(&session, "1+").expect_err("incomplete input should error");

        let colored = format_wasm_error(&err, ColorMode::Always);
        let plain = format_wasm_error(&err, ColorMode::Never);

        assert!(colored.contains('\u{1b}'));
        assert!(!plain.contains('\u{1b}'));
        assert_eq!(
            err.source_ctx.as_deref().map(|source| source.path.as_str()),
            Some("<wasm>")
        );
        assert_eq!(err.span, Some((1, 2)));
    }

    #[test]
    fn ansi_styles_and_box_color_are_independent() {
        let session = WasmWqSession::new();

        session
            .set_ansi_styles_enabled(false)
            .expect("ANSI styles should turn off");
        session
            .set_box_flags("box,axis,color")
            .expect("box color should turn on");
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Never);
        assert!(session.box_config.get().color);

        session
            .set_ansi_styles_enabled(true)
            .expect("ANSI styles should turn on");
        session
            .apply_box_flags("-color")
            .expect("box color should turn off");
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Always);
        assert!(!session.box_config.get().color);
    }

    #[test]
    fn box_settings_preserve_session_color_mode() {
        let session = WasmWqSession::new();
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Always);

        session
            .set_box_flags("0")
            .expect("box flags should turn off");
        assert_eq!(session.get_box_flags(), "");
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Always);

        session
            .set_box_flags("box,axis,color")
            .expect("box flags should restore defaults");
        assert_eq!(session.get_box_flags(), "box,axis,color");
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Always);

        session
            .set_box_flags("xray")
            .expect("xray-only mode should be accepted");
        assert_eq!(session.get_box_flags(), "xray");

        session
            .set_box_flags("0")
            .expect("box flags should turn off");
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Always);

        session
            .set_box_flags("box,axis,color")
            .expect("box flags should restore defaults");
        session
            .apply_box_flags("-color")
            .expect("box color should turn off");
        assert_eq!(session.session.borrow().color_mode(), ColorMode::Always);
    }

    #[test]
    fn box_off_preserves_streamed_asciiplot_colors() {
        let captured = Arc::new(Mutex::new(String::new()));
        let session = WasmWqSession::new();
        session
            .session
            .borrow_mut()
            .set_stdout(Box::new(CapturedOutput {
                out: Arc::clone(&captured),
            }));
        session
            .set_box_flags("0")
            .expect("box flags should turn off");

        let result = eval_rendered_value(
            &session,
            "asciiplot[(1;2;3);(3;2;1);`size:(12;5);`color:(\"red\";\"blue\")]",
        )
        .expect("asciiplot should render");

        assert_eq!(result.display, "()");
        assert!(
            captured
                .lock()
                .expect("stdout capture lock should not be poisoned")
                .contains("\x1b[")
        );
    }

    #[test]
    fn versioned_diagnostics_preserve_error_data_stack_and_cause() {
        let session = WasmWqSession::new();
        let err = eval_wq_script_value(&session, "f:{assert_eq[1;2]};f[]")
            .expect_err("assertion should fail");
        let diagnostic = evaluation_failure_diagnostic_data(&err, ColorMode::Never);

        assert_eq!(diagnostic.version, 2);
        assert_eq!(diagnostic.kind, "assert");
        assert_eq!(diagnostic.cause, None);
        assert_eq!(
            diagnostic
                .data
                .iter()
                .find(|(name, _)| name == "actual")
                .map(|(_, value)| (value.display.as_str(), value.category.as_str())),
            Some(("1", "int"))
        );
        assert_eq!(
            diagnostic
                .data
                .iter()
                .find(|(name, _)| name == "expected")
                .map(|(_, value)| (value.display.as_str(), value.category.as_str())),
            Some(("2", "int"))
        );
        assert!(diagnostic.stack.iter().any(|frame| frame.function == "f"));
        assert!(
            diagnostic
                .stack
                .iter()
                .filter_map(|frame| frame.path.as_deref())
                .any(|path| path == "<wasm>")
        );

        let api = api_diagnostic_data("invalid-option", "bad option");
        assert_eq!(api.version, 2);
        assert!(api.data.is_empty());
        assert!(api.stack.is_empty());
        assert_eq!(api.cause, None);
    }

    #[test]
    fn diagnostic_stack_belongs_to_its_evaluation_failure() {
        let session = WasmWqSession::new();
        let runtime_failure = eval_wq_script_value(&session, "f:{1/0};f[]")
            .expect_err("division by zero should fail");
        assert!(
            evaluation_failure_stack_data(&runtime_failure)
                .iter()
                .any(|frame| frame.function == "f")
        );

        let syntax_failure =
            eval_wq_script_value(&session, "1+").expect_err("incomplete input should fail");
        assert!(evaluation_failure_stack_data(&syntax_failure).is_empty());
        assert!(
            evaluation_failure_stack_data(&runtime_failure)
                .iter()
                .any(|frame| frame.function == "f")
        );
    }

    #[test]
    fn wasm_scripts_reject_loader_directives_with_scoped_diagnostics() {
        let session = WasmWqSession::new();
        let err = eval_wq_script_value(&session, "a:1\n\\l lib.wq\na")
            .expect_err("host loader directives should not be evaluated as source");

        assert_eq!(err.err_type.name(), "syntax");
        assert_eq!(
            err.msg.as_deref(),
            Some("script directive requires a host loader")
        );
        assert_eq!(
            err.source_ctx.as_deref().map(|source| source.path.as_str()),
            Some("<wasm>")
        );
        assert_eq!(err.span, Some((4, 14)));
    }

    #[test]
    fn wqdb_mode_reports_pause_and_continues() {
        let captured = Arc::new(Mutex::new(String::new()));
        let session = WasmWqSession::new();
        session
            .session
            .borrow_mut()
            .set_stderr(Box::new(CapturedOutput {
                out: Arc::clone(&captured),
            }));
        session.set_wqdb_mode(true).expect("session should be idle");
        let result = eval_rendered_value(&session, "1").expect("wqdb mode eval should continue");

        assert_eq!(result.display, "1");
        let stderr = captured
            .lock()
            .expect("stderr capture lock should not be poisoned")
            .clone();
        assert!(stderr.contains("wqdb: paused"));
        assert!(stderr.contains("interactive browser debugger shell is not available"));
    }

    #[test]
    fn session_eval_accumulates_incomplete_blocks() {
        let session = WasmWqSession::new();
        let result = eval_rendered_value(&session, "f:{[x]\n  x+1\n}\nf 2")
            .expect("session eval should accumulate incomplete blocks");

        assert_eq!(result.display, "3");
    }

    #[test]
    fn session_eval_streams_multiline_cas_script() {
        let session = WasmWqSession::new();
        let result = eval_rendered_value(&session, "expr:@s x^2+2*x+1\nexpr")
            .expect("session eval should run article-style scripts");

        assert_eq!(result.display, "x^2 + 2*x + 1");
        assert!(result.is_cas);
    }

    #[test]
    fn html_highlighter_uses_string_escape_class_only_for_valid_escapes() {
        let frontend = default_frontend();
        let html = highlight_wq_data(&frontend, r#""a\nb \u{1f4a9}" "\u{d800}" "\q" @l"\n""#);

        assert_eq!(html.matches("class=\"hl-string-escape\"").count(), 2);
        assert!(html.contains("<span class=\"hl-string-escape\">\\n</span>"));
        assert!(html.contains("<span class=\"hl-string-escape\">\\u{1f4a9}</span>"));
    }

    #[test]
    fn html_highlighter_distinguishes_valid_and_invalid_unicode_scalars() {
        let frontend = default_frontend();
        let html = highlight_wq_data(&frontend, r#""a" @u"a" @u"\n" @u"" @u"ab" @u"\q" @u"x"#);

        assert!(html.contains("<span class=\"hl-string\">&quot;a&quot;</span>"));
        assert!(html.contains("<span class=\"hl-character\">@u&quot;a&quot;</span>"));
        assert!(html.contains("<span class=\"hl-string-escape\">\\n</span>"));
        assert_eq!(html.matches("class=\"hl-character-invalid\"").count(), 4);
    }

    #[test]
    fn html_highlighter_marks_invalid_strings() {
        let frontend = default_frontend();
        let html = highlight_wq_data(&frontend, r#""ok" "\x" "\u{}z" @f"\x""#);

        assert!(html.contains("<span class=\"hl-string\">&quot;ok&quot;</span>"));
        assert_eq!(html.matches("class=\"hl-string-invalid\"").count(), 3);
    }

    #[test]
    fn html_highlighter_uses_bool_class() {
        let frontend = default_frontend();
        let html = highlight_wq_data(&frontend, "T F");

        assert_eq!(html.matches("class=\"hl-bool\"").count(), 2);
    }
}
