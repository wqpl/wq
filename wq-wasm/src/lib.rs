use std::cell::{Cell, RefCell};
use std::fmt::Write as _;

#[cfg(target_arch = "wasm32")]
use js_sys::Function;
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::console;
use wqpl::builtins::{BuiltinPreset, Builtins};
use wqpl::display::{BoxPrintConfig, apply_box_spec, format_print_result, format_xray_info};
use wqpl::doc::{self, DocKind, DocRenderTarget};
use wqpl::highlight::{HighlightEvent, HighlightName, Highlighter};
use wqpl::interpret::InterpreterKind;
use wqpl::session::Session;
use wqpl::session::dbglog::DebugLogFlags;
#[cfg(target_arch = "wasm32")]
use wqpl::session::stdio::{
    WqStderr, WqStdin, WqStdinError, WqStdout, set_wqstderr, set_wqstdin, set_wqstdout,
};
use wqpl::style::ColorMode;
use wqpl::symbol::{DefKind, SymbolIndex, SymbolProvenanceKind, UseKind};
use wqpl::value::{Value, WqResult};
use wqpl::vm::Vm;
use wqpl::wqerror::{WqError, WqErrorType};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn main_js() {
    // std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    console::log_1(&"Hello from Rust!".into());
    colored::control::set_override(true);
}

// JS stream adapters
// ====================================================================================

#[cfg(target_arch = "wasm32")]
struct JsStdout {
    cb: Function,
}

#[cfg(target_arch = "wasm32")]
struct JsStderr {
    cb: Function,
}

#[cfg(target_arch = "wasm32")]
struct JsStdin {
    cb: Function,
    highlight: bool,
}

// Default console loggers used when no JS callback is provided
#[cfg(target_arch = "wasm32")]
struct ConsoleStdout;
#[cfg(target_arch = "wasm32")]
struct ConsoleStderr;

#[cfg(target_arch = "wasm32")]
impl WqStdout for JsStdout {
    fn print(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn println(&mut self, s: &str) {
        let mut out = s.to_owned();
        out.push('\n');
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(&out));
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStderr for JsStderr {
    fn eprint(&mut self, s: &str) {
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(s));
    }
    fn eprintln(&mut self, s: &str) {
        let mut out = s.to_owned();
        out.push('\n');
        let _ = self.cb.call1(&JsValue::NULL, &JsValue::from_str(&out));
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStdout for ConsoleStdout {
    fn print(&mut self, s: &str) {
        console::log_1(&s.into());
    }
    fn println(&mut self, s: &str) {
        console::log_1(&format!("{s}\n").into());
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStderr for ConsoleStderr {
    fn eprint(&mut self, s: &str) {
        console::error_1(&s.into());
    }
    fn eprintln(&mut self, s: &str) {
        console::error_1(&format!("{s}\n").into());
    }
}

#[cfg(target_arch = "wasm32")]
impl WqStdin for JsStdin {
    fn readline(&mut self, prompt: &str) -> Result<String, WqStdinError> {
        match self.cb.call1(&JsValue::NULL, &JsValue::from_str(prompt)) {
            Ok(val) => {
                if val.is_undefined() || val.is_null() {
                    Err(WqStdinError::Eof)
                } else {
                    Ok(val.as_string().unwrap_or_default())
                }
            }
            Err(e) => Err(WqStdinError::Other(format!("{e:?}"))),
        }
    }

    fn add_history(&mut self, _line: &str) {}

    fn set_highlight(&mut self, on: bool) {
        self.highlight = on;
    }

    fn highlight_enabled(&self) -> bool {
        self.highlight
    }
}

// Global std stream setters
// ===============================================================

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_stdout_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_wqstdout(Some(Box::new(JsStdout { cb: f }))),
        None => set_wqstdout(Some(Box::new(ConsoleStdout))),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_stderr_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_wqstderr(Some(Box::new(JsStderr { cb: f }))),
        None => set_wqstderr(Some(Box::new(ConsoleStderr))),
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_stdin_callback(cb: Option<Function>) {
    match cb {
        Some(f) => set_wqstdin(Box::new(JsStdin {
            cb: f,
            highlight: true,
        })),
        None => {
            // Reset to no custom stdin; subsequent reads will error
            // There's no setter to clear stdin, so set a reader that always EOFs.
            set_wqstdin(Box::new(JsStdin {
                cb: Function::new_no_args("return null;"),
                highlight: true,
            }));
        }
    }
}

// wq Session API
// ====================================================================================

#[wasm_bindgen]
pub struct EvalResult {
    value: String,
    is_cas: bool,
    type_name: String,
    xray: String,
}

#[wasm_bindgen]
impl EvalResult {
    #[wasm_bindgen(getter)]
    pub fn value(&self) -> String {
        self.value.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn is_cas(&self) -> bool {
        self.is_cas
    }

    #[wasm_bindgen(getter)]
    pub fn type_name(&self) -> String {
        self.type_name.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn xray(&self) -> String {
        self.xray.clone()
    }
}

#[wasm_bindgen]
pub struct WasmWqSession {
    box_config: Cell<BoxPrintConfig>,
    dry_mode: Cell<bool>,
    session: RefCell<Session>,
}

#[wasm_bindgen]
impl WasmWqSession {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WasmWqSession {
        let mut session = Session::new();
        session.set_pause_callback(Some(wasm_wqdb_pause_handler));
        WasmWqSession {
            box_config: Cell::new(BoxPrintConfig::default()),
            dry_mode: Cell::new(false),
            session: RefCell::new(session),
        }
    }

    /// Evaluate a source string and return the value's string form.
    #[wasm_bindgen]
    pub fn eval_wq(&self, src: &str) -> Result<String, JsValue> {
        match eval_wq_script_value(self, src) {
            Ok(v) => {
                let config = self.box_config.get();
                let s = format_print_result(&v, &config, config.color);
                Ok(s)
            }
            Err(e) => {
                if e.err_type.is_runtime() {
                    self.session.borrow_mut().dbg_print_bt();
                }
                let config = self.box_config.get();
                Err(JsValue::from_str(&format_wasm_error(&e, config)))
            }
        }
    }

    /// Evaluate a source string and return both the string form and whether it
    /// is a CAS expression.
    #[wasm_bindgen]
    pub fn eval_wq_result(&self, src: &str) -> Result<EvalResult, JsValue> {
        match eval_wq_script_value(self, src) {
            Ok(v) => {
                let is_cas = v.is_cas();
                let config = self.box_config.get();
                let s = format_print_result(&v, &config, config.color);
                let type_name = v.type_name().to_string();
                let xray = format_xray_info(&v, config.color);
                Ok(EvalResult {
                    value: s,
                    is_cas,
                    type_name,
                    xray,
                })
            }
            Err(e) => {
                if e.err_type.is_runtime() {
                    self.session.borrow_mut().dbg_print_bt();
                }
                let config = self.box_config.get();
                Err(JsValue::from_str(&format_wasm_error(&e, config)))
            }
        }
    }

    pub fn set_debug_flags(&self, spec: &str) -> Result<(), JsValue> {
        let spec = if spec.trim() == "off" { "0" } else { spec };
        match DebugLogFlags::parse(spec) {
            Ok(flags) => {
                wqpl::session::dbglog::set_debug_log_flags(flags);
                Ok(())
            }
            Err(e) => Err(JsValue::from_str(&e)),
        }
    }

    pub fn apply_debug_flags(&self, spec: &str) -> Result<(), JsValue> {
        let mut flags = wqpl::session::dbglog::get_debug_log_flags();
        match flags.apply_spec(spec) {
            Ok(()) => {
                wqpl::session::dbglog::set_debug_log_flags(flags);
                Ok(())
            }
            Err(e) => Err(JsValue::from_str(&e)),
        }
    }

    pub fn get_debug_flags(&self) -> String {
        let flags = wqpl::session::dbglog::get_debug_log_flags();
        let names = flags.display_names();
        if names.is_empty() {
            "off".to_string()
        } else {
            names.join(",")
        }
    }

    pub fn get_bt_mode(&self) -> bool {
        self.session.borrow().get_bt_mode()
    }

    pub fn set_bt_mode(&self, on: bool) {
        self.session.borrow_mut().set_bt_mode(on);
    }

    pub fn get_wqdb_mode(&self) -> bool {
        self.session.borrow().is_wqdb_enabled()
    }

    pub fn set_wqdb_mode(&self, on: bool) {
        self.session.borrow_mut().set_wqdb(on);
    }

    pub fn get_dry_mode(&self) -> bool {
        self.dry_mode.get()
    }

    pub fn set_dry_mode(&self, on: bool) {
        self.dry_mode.set(on);
        self.session.borrow_mut().set_dry_mode(on);
    }

    pub fn toggle_dry_mode(&self) -> bool {
        let next = !self.dry_mode.get();
        self.set_dry_mode(next);
        next
    }

    pub fn reset_session(&self) {
        self.session.borrow_mut().reset_session();
    }

    pub fn get_interpreter_name(&self) -> String {
        self.session.borrow().interpreter_name().to_string()
    }

    pub fn get_interpreter_names(&self) -> String {
        InterpreterKind::names().join(", ")
    }

    pub fn set_interpreter_by_name(&self, name: &str) -> Result<String, JsValue> {
        self.session
            .borrow_mut()
            .set_interpreter_by_name(name)
            .map(str::to_string)
            .map_err(|err| JsValue::from_str(&err))
    }

    pub fn get_builtins_preset(&self) -> String {
        self.session.borrow().builtins_preset().name().to_string()
    }

    pub fn get_builtins_preset_names(&self) -> String {
        BuiltinPreset::names().join(", ")
    }

    pub fn set_builtins_preset(&self, name: &str) -> Result<String, JsValue> {
        let preset = BuiltinPreset::from_name(name).ok_or_else(|| {
            JsValue::from_str(&format!(
                "unknown bfn preset '{name}'\nAvailable: {}",
                BuiltinPreset::names().join(", ")
            ))
        })?;
        self.session.borrow_mut().set_builtins_preset(preset);
        Ok(preset.name().to_string())
    }

    pub fn get_env_table(&self) -> String {
        let env = self.session.borrow().env_vars();
        if env.is_empty() {
            return "no global bindings".to_string();
        }

        let mut name_w = "name".len();
        let mut value_w = "value".len();
        let mut type_w = "type".len();
        for (name, v) in &env {
            name_w = name_w.max(name.len());
            value_w = value_w.max(v.to_string().len());
            type_w = type_w.max(v.type_name().len());
        }

        let mut out = String::new();
        let _ = writeln!(
            out,
            "{:<name_w$}  {:<value_w$}  {:<type_w$}",
            "name",
            "value",
            "type",
            name_w = name_w,
            value_w = value_w,
            type_w = type_w
        );
        let _ = writeln!(
            out,
            "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
            "",
            "",
            "",
            name_w = name_w,
            value_w = value_w,
            type_w = type_w
        );
        for (name, v) in &env {
            let _ = writeln!(
                out,
                "{:<name_w$}  {:<value_w$}  {:<type_w$}",
                name,
                v.to_string(),
                v.type_name(),
                name_w = name_w,
                value_w = value_w,
                type_w = type_w
            );
        }
        if out.ends_with('\n') {
            out.pop();
        }
        out
    }

    // ///Return a formatted view of user-defined global bindings.
    // pub fn get_env(&self) -> String {
    //     use std::fmt::Write as _;
    //     let vm = self.vm.borrow();
    //     if let Some(env) = vm.get_environment() {
    //         let mut name_w = "name".len();
    //         let mut value_w = "value".len();
    //         let mut type_w = "type".len();
    //         for (name, v) in env {
    //             name_w = name_w.max(name.len());
    //             value_w = value_w.max(v.to_string().len());
    //             type_w = type_w.max(v.type_name().len());
    //         }
    //         let mut out = String::new();
    //         let _ = writeln!(
    //             out,
    //             "{:<name_w$}  {:<value_w$}  {:<type_w$}",
    //             "name",
    //             "value",
    //             "type",
    //             name_w = name_w,
    //             value_w = value_w,
    //             type_w = type_w
    //         );
    //         let _ = writeln!(
    //             out,
    //             "{:-<name_w$}  {:-<value_w$}  {:-<type_w$}",
    //             "",
    //             "",
    //             "",
    //             name_w = name_w,
    //             value_w = value_w,
    //             type_w = type_w
    //         );
    //         for (name, v) in env {
    //             let _ = writeln!(
    //                 out,
    //                 "{:<name_w$}  {:<value_w$}  {:<type_w$}",
    //                 name,
    //                 v.to_string(),
    //                 v.type_name(),
    //                 name_w = name_w,
    //                 value_w = value_w,
    //                 type_w = type_w
    //             );
    //         }
    //         if out.ends_with('\n') {
    //             out.pop();
    //         }
    //         out
    //     } else {
    //         "no global bindings".to_string()
    //     }
    // }

    /// Clear user-defined bindings while preserving debug state.
    pub fn clear_env(&self) {
        self.session.borrow_mut().clear_environment();
    }

    /// Toggle boxed display of evaluation results. Returns the new state.
    pub fn toggle_box_mode(&self) -> bool {
        let mut config = self.box_config.get();
        config.toggle_box();
        self.box_config.set(config);
        config.boxed
    }

    pub fn get_box_mode(&self) -> bool {
        self.box_config.get().boxed
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
            apply_box_spec(&mut config, spec).map_err(|e| JsValue::from_str(&e))?;
        }
        self.box_config.set(config);
        Ok(())
    }

    pub fn apply_box_flags(&self, spec: &str) -> Result<(), JsValue> {
        let mut config = self.box_config.get();
        apply_box_spec(&mut config, spec).map_err(|e| JsValue::from_str(&e))?;
        self.box_config.set(config);
        Ok(())
    }
}

impl Default for WasmWqSession {
    fn default() -> Self {
        Self::new()
    }
}

fn eval_wq_script_value(session: &WasmWqSession, src: &str) -> WqResult<Value> {
    let mut line_starts = Vec::with_capacity(128);
    line_starts.push(0);
    for (i, b) in src.as_bytes().iter().enumerate() {
        if *b == b'\n' {
            line_starts.push(i + 1);
        }
    }
    if *line_starts.last().expect("line_starts is seeded") != src.len() {
        line_starts.push(src.len());
    }

    let mut buffer = String::new();
    let mut buffer_has_code = false;
    let mut consumed_bytes = 0usize;
    let mut last_result = Value::unit();

    for (i, raw_line) in src.lines().enumerate() {
        let next_consumed = *line_starts.get(i + 1).unwrap_or(&src.len());
        let trimmed_all = raw_line.trim();

        buffer.push_str(raw_line);
        buffer.push('\n');

        if trimmed_all.is_empty() || trimmed_all.starts_with("//") {
            continue;
        }

        buffer_has_code = true;
        let result = {
            let mut vm = session.session.borrow_mut();
            vm.dbg_set_source("<wasm>", src);
            vm.dbg_set_offset(consumed_bytes);
            vm.eval_string(&buffer)
        };

        match result {
            Ok(value) => {
                last_result = value;
                buffer.clear();
                buffer_has_code = false;
                consumed_bytes = next_consumed;
            }
            Err(err) if err.err_type == WqErrorType::Eof => {}
            Err(err) => return Err(err),
        }
    }

    if buffer_has_code || !buffer.trim().is_empty() {
        let mut vm = session.session.borrow_mut();
        vm.dbg_set_source("<wasm>", src);
        vm.dbg_set_offset(consumed_bytes);
        last_result = vm.eval_string(&buffer)?;
    }

    Ok(last_result)
}

fn wasm_color_mode(config: BoxPrintConfig) -> ColorMode {
    if config.color {
        ColorMode::Always
    } else {
        ColorMode::Never
    }
}

fn format_wasm_error(err: &WqError, config: BoxPrintConfig) -> String {
    err.render_with_color_mode(wasm_color_mode(config))
}

fn wasm_wqdb_pause_handler(host: &mut Vm) {
    let loc = host.loc();
    let name = host.func_name_for_chunk(loc.chunk);
    wqpl::session::stdio::wqstderr_println(
        "wqdb: paused; interactive browser debugger shell is not available, continuing",
    );
    wqpl::session::stdio::wqstderr_println(wqpl::wqdb::format_frame(
        host.debug_info(),
        loc,
        &name,
        true,
    ));
    host.dbg_continue();
}

// ===================== Convenience one-offs =====================

/// Evaluate a string in a fresh VM and return the result as a string.
#[wasm_bindgen]
pub fn eval_wq(code: &str) -> Result<String, JsValue> {
    let session = WasmWqSession::new();
    session.eval_wq(code)
}

/// Version string for splash and title
#[wasm_bindgen]
pub fn get_wq_ver() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

// #[wasm_bindgen]
// pub fn get_help_doc() -> String {
//     include_str!("../d/refcard").to_string()
// }

/// Builtin function names (columns)
#[wasm_bindgen]
pub fn get_builtins() -> String {
    let mut funcs = Builtins::new().list_functions();
    funcs.sort();
    col_wrap(&funcs, 6, 2)
}

#[wasm_bindgen]
pub fn get_doc_markdown(query: &str) -> Result<String, JsValue> {
    let topic = doc::resolve(query)
        .ok_or_else(|| JsValue::from_str(&format!("unknown doc topic '{query}'")))?;
    Ok(doc::render_markdown(&topic, DocRenderTarget::Web))
}

#[wasm_bindgen]
pub fn get_doc_index_json() -> String {
    let topics = doc::all_topics();
    let mut out = String::from("[");
    for (idx, topic) in topics.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('{');
        push_json_field(&mut out, "id", &topic.id);
        out.push(',');
        push_json_field(&mut out, "title", &topic.title);
        out.push(',');
        push_json_field(&mut out, "kind", doc_kind_name(topic.kind));
        out.push(',');
        push_json_field(&mut out, "group", &topic.group);
        out.push(',');
        push_json_field(&mut out, "summary", &topic.summary);
        out.push(',');
        if let Some(builtin) = topic.builtin {
            push_json_field(&mut out, "usage", builtin.usage());
        } else {
            push_json_field(&mut out, "usage", "");
        }
        out.push(',');
        out.push_str("\"aliases\":[");
        for (alias_idx, alias) in topic.aliases.iter().enumerate() {
            if alias_idx > 0 {
                out.push(',');
            }
            push_json_string(&mut out, alias);
        }
        out.push(']');
        out.push('}');
    }
    out.push(']');
    out
}

#[wasm_bindgen]
pub fn get_symbol_index_json(src: &str) -> String {
    let session = Session::new();
    match session.analyze_symbols(src) {
        Ok(index) => symbol_index_json(&index),
        Err(err) => symbol_error_json(&err),
    }
}

fn symbol_error_json(err: &wqpl::wqerror::WqError) -> String {
    let mut out = String::from("{\"defs\":[],\"occurrences\":[],\"errors\":[{");
    push_json_opt_span_field(&mut out, "span", err.span);
    out.push(',');
    push_json_field(&mut out, "kind", err.err_type.name());
    out.push(',');
    push_json_field(&mut out, "message", err.msg.as_deref().unwrap_or(""));
    out.push_str("}]}");
    out
}

fn symbol_index_json(index: &SymbolIndex) -> String {
    let occurrences = index.occurrences();
    let mut out = String::from("{\"defs\":[");
    let mut first_def = true;
    for (def_idx, def) in index.defs.iter().enumerate() {
        if !is_user_symbol(def.kind, &def.name) {
            continue;
        }
        if !first_def {
            out.push(',');
        }
        first_def = false;

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

        out.push('{');
        push_json_usize_field(&mut out, "index", def_idx);
        out.push(',');
        push_json_field(&mut out, "name", &def.name);
        out.push(',');
        push_json_field(&mut out, "kind", def_kind_name(def.kind));
        out.push(',');
        push_json_opt_span_field(&mut out, "span", def.span);
        out.push(',');
        push_json_opt_span_field(&mut out, "name_span", def.name_span);
        out.push(',');
        push_json_params_field(&mut out, def.params.as_deref());
        out.push(',');
        push_json_opt_usize_field(&mut out, "parent", def.parent);
        out.push(',');
        push_json_field(
            &mut out,
            "provenance",
            provenance
                .as_ref()
                .map(|p| provenance_kind_name(p.kind))
                .unwrap_or("unknown"),
        );
        out.push(',');
        push_json_opt_string_field(
            &mut out,
            "origin",
            provenance.as_ref().and_then(|p| p.origin.as_deref()),
        );
        out.push(',');
        push_json_usize_field(&mut out, "read_count", read_count);
        out.push(',');
        push_json_usize_field(&mut out, "write_count", write_count);
        out.push(',');
        push_json_usize_field(&mut out, "occurrence_count", occurrence_count);
        out.push(',');
        push_json_usize_field(
            &mut out,
            "ref_capture_count",
            index.ref_capture_count(def_idx),
        );
        out.push('}');
    }

    out.push_str("],\"occurrences\":[");
    let mut first_occurrence = true;
    for occurrence in &occurrences {
        let Some(def) = index.defs.get(occurrence.def_idx) else {
            continue;
        };
        if !is_user_symbol(def.kind, &def.name) {
            continue;
        }
        if !first_occurrence {
            out.push(',');
        }
        first_occurrence = false;

        out.push('{');
        push_json_span_field(&mut out, "span", occurrence.span);
        out.push(',');
        push_json_usize_field(&mut out, "def", occurrence.def_idx);
        out.push(',');
        push_json_field(&mut out, "kind", use_kind_name(occurrence.kind));
        out.push('}');
    }

    out.push_str("],\"errors\":[");
    for (idx, (span, err)) in index.errors.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push('{');
        push_json_span_field(&mut out, "span", *span);
        out.push(',');
        push_json_field(&mut out, "kind", err.err_type.name());
        out.push(',');
        push_json_field(&mut out, "message", err.msg.as_deref().unwrap_or(""));
        out.push('}');
    }
    out.push_str("]}");
    out
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

fn push_json_usize_field(out: &mut String, key: &str, value: usize) {
    push_json_string(out, key);
    out.push(':');
    let _ = write!(out, "{value}");
}

fn push_json_opt_usize_field(out: &mut String, key: &str, value: Option<usize>) {
    push_json_string(out, key);
    out.push(':');
    if let Some(value) = value {
        let _ = write!(out, "{value}");
    } else {
        out.push_str("null");
    }
}

fn push_json_span_field(out: &mut String, key: &str, span: (usize, usize)) {
    push_json_string(out, key);
    out.push(':');
    push_json_span(out, span);
}

fn push_json_opt_span_field(out: &mut String, key: &str, span: Option<(usize, usize)>) {
    push_json_string(out, key);
    out.push(':');
    if let Some(span) = span {
        push_json_span(out, span);
    } else {
        out.push_str("null");
    }
}

fn push_json_span(out: &mut String, span: (usize, usize)) {
    let _ = write!(out, "[{},{}]", span.0, span.1);
}

fn push_json_params_field(out: &mut String, params: Option<&[String]>) {
    push_json_string(out, "params");
    out.push(':');
    if let Some(params) = params {
        out.push('[');
        for (idx, param) in params.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            push_json_string(out, param);
        }
        out.push(']');
    } else {
        out.push_str("null");
    }
}

fn push_json_opt_string_field(out: &mut String, key: &str, value: Option<&str>) {
    push_json_string(out, key);
    out.push(':');
    if let Some(value) = value {
        push_json_string(out, value);
    } else {
        out.push_str("null");
    }
}

fn push_json_field(out: &mut String, key: &str, value: &str) {
    push_json_string(out, key);
    out.push(':');
    push_json_string(out, value);
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                use std::fmt::Write as _;
                write!(out, "\\u{:04x}", ch as u32).expect("writing to string should not fail");
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

fn doc_kind_name(kind: DocKind) -> &'static str {
    match kind {
        DocKind::Builtin => "builtin",
        DocKind::Keyword => "keyword",
        DocKind::Syntax => "syntax",
        DocKind::Guide => "guide",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use wqpl::session::stdio::{WqStderr, set_wqstderr};

    use super::*;

    struct CapturedStderr {
        out: Arc<Mutex<String>>,
    }

    impl WqStderr for CapturedStderr {
        fn eprint(&mut self, s: &str) {
            self.out
                .lock()
                .expect("stderr capture lock should not be poisoned")
                .push_str(s);
        }

        fn eprintln(&mut self, s: &str) {
            let mut out = self
                .out
                .lock()
                .expect("stderr capture lock should not be poisoned");
            out.push_str(s);
            out.push('\n');
        }
    }

    struct ResetStderr;

    impl Drop for ResetStderr {
        fn drop(&mut self) {
            set_wqstderr(None);
        }
    }

    #[test]
    fn doc_exports_smoke() {
        let index = get_doc_index_json();
        assert!(index.contains("\"id\":\"builtin.map\""));
        assert!(index.contains("\"usage\":\"map[xs;f;d?]\""));
        assert!(index.contains("\"id\":\"at-return\""));

        let markdown = get_doc_markdown("map").expect("map doc renders");
        assert!(markdown.contains("map builtin"));
        assert!(markdown.contains("map[xs;f;d?]"));
    }

    #[test]
    fn symbol_index_json_exports_user_symbol_details() {
        let json = get_symbol_index_json("g:1; f:{[x] y:x+g; y}; f[2]");

        assert!(json.contains("\"name\":\"f\""));
        assert!(json.contains("\"kind\":\"function\""));
        assert!(json.contains("\"params\":[\"x\"]"));
        assert!(json.contains("\"name\":\"y\""));
        assert!(json.contains("\"provenance\":\"local\""));
        assert!(json.contains("\"origin\":\"f\""));
        assert!(json.contains("\"kind\":\"read\""));
        assert!(json.contains("\"kind\":\"write\""));
    }

    #[test]
    fn symbol_index_json_returns_structured_parse_errors() {
        let json = get_symbol_index_json("a:1\nd:'{a}\nb:\"");

        assert!(json.contains("\"defs\":[]"));
        assert!(json.contains("\"kind\":\"eof\""));
        assert!(json.contains("\"message\":\"string is not properly terminated\""));
        assert!(
            !json.contains('\u{1b}'),
            "symbol JSON should not contain terminal escape sequences: {json:?}",
        );
    }

    #[test]
    fn wasm_error_formatting_follows_box_color_flag() {
        let session = WasmWqSession::new();
        let err = eval_wq_script_value(&session, "1+").expect_err("incomplete input should error");
        let color_config = BoxPrintConfig::default();
        let plain_config = BoxPrintConfig {
            color: false,
            ..BoxPrintConfig::default()
        };

        let colored = format_wasm_error(&err, color_config);
        let plain = format_wasm_error(&err, plain_config);

        assert!(colored.contains('\u{1b}'));
        assert!(!plain.contains('\u{1b}'));
    }

    #[test]
    fn wqdb_mode_reports_pause_and_continues() {
        let captured = Arc::new(Mutex::new(String::new()));
        let _reset = ResetStderr;
        set_wqstderr(Some(Box::new(CapturedStderr {
            out: captured.clone(),
        })));

        let session = WasmWqSession::new();
        session.set_wqdb_mode(true);
        let result = session
            .eval_wq_result("1")
            .expect("wqdb mode eval should continue");

        assert_eq!(result.value(), "1");
        let stderr = captured
            .lock()
            .expect("stderr capture lock should not be poisoned")
            .clone();
        assert!(stderr.contains("wqdb: paused"));
        assert!(stderr.contains("interactive browser debugger shell is not available"));
    }

    #[test]
    fn one_shot_eval_streams_multiline_cas_script() {
        let result = eval_wq("expr:@s x^2+2*x+1\nexpr")
            .expect("one-shot eval should run article-style scripts");

        assert_eq!(result, "x^2 + 2*x + 1");
    }

    #[test]
    fn one_shot_eval_accumulates_incomplete_blocks() {
        let result = eval_wq("f:{[x]\n  x+1\n}\nf 2")
            .expect("one-shot eval should accumulate incomplete blocks");

        assert_eq!(result, "3");
    }

    #[test]
    fn session_eval_result_streams_multiline_cas_script() {
        let session = WasmWqSession::new();
        let result = session
            .eval_wq_result("expr:@s x^2+2*x+1\nexpr")
            .expect("session eval result should run article-style scripts");

        assert_eq!(result.value(), "x^2 + 2*x + 1");
        assert!(result.is_cas());
    }
}

// /// Error codes and names for quick reference
// #[wasm_bindgen]
// pub fn get_err_codes() -> String {
//     let mut out = String::new();
//     let all: &[(u16, &str)] = &[
//         (WqErrorType::Vm.to_code(), WqErrorType::Vm.name()),
//         (WqErrorType::Eof.to_code(), WqErrorType::Eof.name()),
//         (WqErrorType::Syntax.to_code(), WqErrorType::Syntax.name()),
//         (
//             WqErrorType::NotBound.to_code(),
//             WqErrorType::NotBound.name(),
//         ),
//         (WqErrorType::Index.to_code(), WqErrorType::Index.name()),
//         (WqErrorType::Call.to_code(), WqErrorType::Call.name()),
//         (WqErrorType::Arity.to_code(), WqErrorType::Arity.name()),
//         (WqErrorType::Domain.to_code(), WqErrorType::Domain.name()),
//         (WqErrorType::Length.to_code(), WqErrorType::Length.name()),
//         (
//             WqErrorType::NumericOverflow.to_code(),
//             WqErrorType::NumericOverflow.name(),
//         ),
//         (WqErrorType::ZeroDiv.to_code(), WqErrorType::ZeroDiv.name()),
//         (WqErrorType::Io.to_code(), WqErrorType::Io.name()),
//         (WqErrorType::Encode.to_code(), WqErrorType::Encode.name()),
//         (WqErrorType::Exec.to_code(), WqErrorType::Exec.name()),
//         (WqErrorType::Raise.to_code(), WqErrorType::Raise.name()),
//     ];
//     // width calc
//     let w_code = all
//         .iter()
//         .map(|(c, _)| c.to_string().len())
//         .max()
//         .unwrap_or(1);
//     let w_name = all.iter().map(|(_, n)| n.len()).max().unwrap_or(1);
//     out.push_str(&format!("{:<w_code$}  {:<w_name$}\n", "code", "name"));
//     out.push_str(&format!("{:-<w_code$}  {:-<w_name$}\n", "", ""));
//     for (code, name) in all {
//         out.push_str(&format!("{code:<w_code$}  {name:<w_name$}\n"));
//     }
//     if out.ends_with('\n') {
//         out.pop();
//     }
//     out
// }

fn col_wrap(items: &[String], columns: usize, gutter: usize) -> String {
    if items.is_empty() {
        return String::new();
    }
    let max_len = items.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (i, name) in items.iter().enumerate() {
        let _ = write!(&mut out, "{:<w$}", name, w = max_len + gutter);
        if (i + 1) % columns == 0 {
            out.push('\n');
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if out.ends_with('\n') {
        out.pop();
    }
    out
}

#[wasm_bindgen]
pub fn set_ansi_styles_enabled(on: bool) {
    colored::control::set_override(on);
}

/// Highlight wq source code and return HTML with CSS class names.
#[wasm_bindgen]
pub fn highlight_wq(src: &str) -> String {
    let highlighter = Highlighter::new();
    let semantic_spans = if src.contains('{') || src.contains('\'') {
        Session::new()
            .analyze_symbols(src)
            .map(|index| index.semantic_highlight_spans())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let events = highlighter.highlight_with_semantic_spans(src, &semantic_spans);
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

fn class_for_name(name: HighlightName) -> &'static str {
    match name {
        HighlightName::Comment => "hl-comment",
        HighlightName::Constant => "hl-constant",
        HighlightName::ConstantBuiltin => "hl-constant-builtin",
        HighlightName::Function => "hl-function",
        HighlightName::FunctionCall => "hl-function-call",
        HighlightName::FunctionBuiltin => "hl-function-builtin",
        HighlightName::Keyword => "hl-keyword",
        HighlightName::KeywordReturn => "hl-keyword-return",
        HighlightName::KeywordDebug => "hl-keyword-debug",
        HighlightName::Module => "hl-module",
        HighlightName::Number => "hl-number",
        HighlightName::Boolean => "hl-boolean",
        HighlightName::Operator => "hl-operator",
        HighlightName::OperatorPipe => "hl-operator-pipe",
        HighlightName::Property => "hl-property",
        HighlightName::PropertyBuiltin => "hl-property-builtin",
        HighlightName::Punctuation => "hl-punctuation",
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
        HighlightName::StringSpecial => "hl-string-special",
        HighlightName::Tag => "hl-tag",
        HighlightName::Type => "hl-type",
        HighlightName::TypeBuiltin => "hl-type-builtin",
        HighlightName::Variable => "hl-variable",
        HighlightName::VariableOuter => "hl-variable-outer",
        HighlightName::VariableRefCapture => "hl-variable-ref-capture",
        HighlightName::VariableBuiltin => "hl-variable-builtin",
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
