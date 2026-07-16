use std::sync::{Arc, Mutex};

use wqpl::interpret::InterpreterKind;
use wqpl::script::ScriptSpan;
use wqpl::session::dbglog::DebugLogFlags;
use wqpl::session::stdio::{WqInput, WqIoError, WqOutput};
use wqpl::session::{Session, SourceUnit};
use wqpl::wqdb::data::{ChunkId, CodeLoc};

#[derive(Clone)]
struct CapturedOutput {
    text: Arc<Mutex<String>>,
}

impl WqOutput for CapturedOutput {
    fn write(&mut self, text: &str) -> Result<(), WqIoError> {
        self.text
            .lock()
            .expect("captured output lock should not be poisoned")
            .push_str(text);
        Ok(())
    }
}

fn capture() -> (CapturedOutput, Arc<Mutex<String>>) {
    let text = Arc::new(Mutex::new(String::new()));
    (
        CapturedOutput {
            text: Arc::clone(&text),
        },
        text,
    )
}

#[test]
fn sessions_route_stdout_independently() {
    let (first_output, first_text) = capture();
    let (second_output, second_text) = capture();
    let mut first = Session::new();
    let mut second = Session::new();
    first.set_stdout(Box::new(first_output));
    second.set_stdout(Box::new(second_output));

    first.eval_string("echo \"first\"").expect("first eval");
    second.eval_string("echo \"second\"").expect("second eval");
    first.eval_string("print \"again\"").expect("third eval");

    assert_eq!(
        first_text
            .lock()
            .expect("first output lock should not be poisoned")
            .as_str(),
        "first\nagain",
    );
    assert_eq!(
        second_text
            .lock()
            .expect("second output lock should not be poisoned")
            .as_str(),
        "second\n",
    );
}

struct FixedInput(&'static str);

impl WqInput for FixedInput {
    fn read_line(&mut self, _prompt: &str) -> Result<String, WqIoError> {
        Ok(self.0.to_string())
    }
}

#[test]
fn sessions_read_input_independently() {
    let mut first = Session::new();
    let mut second = Session::new();
    first.set_input(Box::new(FixedInput("first")));
    second.set_input(Box::new(FixedInput("second")));

    let first_value = first.eval_string("input[]").expect("first input");
    let second_value = second.eval_string("input[]").expect("second input");

    assert_eq!(first_value.to_string(), "\"first\"");
    assert_eq!(second_value.to_string(), "\"second\"");
}

#[test]
fn sessions_keep_debug_flags_and_stderr_independent() {
    let (first_error, first_text) = capture();
    let (second_error, second_text) = capture();
    let mut first = Session::new();
    let mut second = Session::new();
    first.set_stderr(Box::new(first_error));
    second.set_stderr(Box::new(second_error));
    first.set_debug_flags(DebugLogFlags::parse("value").expect("debug flags"));

    first.eval_string("1+1").expect("first eval");
    second.eval_string("2+2").expect("second eval");

    assert!(
        first_text
            .lock()
            .expect("first error lock should not be poisoned")
            .contains("Int(2)"),
    );
    assert!(
        second_text
            .lock()
            .expect("second error lock should not be poisoned")
            .is_empty(),
    );
}

#[test]
fn sessions_route_cas_debug_output_to_the_owning_session() {
    let (first_error, first_text) = capture();
    let (second_error, second_text) = capture();
    let mut first = Session::new();
    let mut second = Session::new();
    first.set_stderr(Box::new(first_error));
    second.set_stderr(Box::new(second_error));
    first.set_debug_flags(DebugLogFlags::parse("cas").expect("debug flags"));

    first
        .eval_string("simplify[@s x/x]")
        .expect("first CAS eval");
    second
        .eval_string("simplify[@s x/x]")
        .expect("second CAS eval");

    let first_output = first_text
        .lock()
        .expect("first error lock should not be poisoned")
        .clone();
    assert!(first_output.contains("[cas] simplify enter:"));
    assert!(first_output.contains("[cas] simplify exit:"));
    assert!(
        second_text
            .lock()
            .expect("second error lock should not be poisoned")
            .is_empty(),
    );
}

#[test]
fn debugger_operations_log_through_the_owning_session_outside_evaluation() {
    let (output, stderr) = capture();
    let mut session = Session::new();
    session.set_stderr(Box::new(output));
    session.set_debug_flags(DebugLogFlags::parse("wqdb").expect("debug flags"));

    session.debugger().set_temporary_breakpoint(CodeLoc {
        chunk: ChunkId(0),
        pc: 7,
    });

    assert!(
        stderr
            .lock()
            .expect("stderr lock")
            .contains("adding temp break")
    );
}

#[test]
fn sessions_render_diagnostics_with_independent_color_modes() {
    let (colored_output, colored_text) = capture();
    let (plain_output, plain_text) = capture();
    let flags = DebugLogFlags::parse("token").expect("debug flags");
    let mut colored = Session::new();
    let mut plain = Session::new();
    colored.set_stderr(Box::new(colored_output));
    plain.set_stderr(Box::new(plain_output));
    colored.set_debug_flags(flags);
    plain.set_debug_flags(flags);
    colored.set_color_mode(wqpl::style::ColorMode::Always);
    plain.set_color_mode(wqpl::style::ColorMode::Never);

    colored.eval_string("1").expect("colored eval");
    plain.eval_string("1").expect("plain eval");

    assert!(colored_text.lock().expect("colored lock").contains("\x1b["));
    assert!(!plain_text.lock().expect("plain lock").contains("\x1b["));
}

#[test]
fn fragment_token_debug_output_uses_file_wide_coordinates() {
    let (output, stderr) = capture();
    let mut session = Session::new();
    session.set_stderr(Box::new(output));
    session.set_debug_flags(DebugLogFlags::parse("token").expect("debug flags"));
    session.set_color_mode(wqpl::style::ColorMode::Never);
    let full_source = "🦀 prefix\n  42";
    let source = SourceUnit::fragment(
        "fragment.wq",
        full_source,
        ScriptSpan {
            start: "🦀 prefix\n  ".len(),
            end: full_source.len(),
        },
    )
    .expect("valid fragment");

    session.eval_source(source).expect("fragment evaluation");

    let output = stderr.lock().expect("stderr lock").clone();
    let integer_row = output
        .lines()
        .find(|line| line.contains("Integer(42)"))
        .expect("integer token row");
    assert!(
        integer_row
            .split_whitespace()
            .eq(["Integer(42)", "12", "2", "3", "14", "16"])
    );
}

#[test]
fn source_context_is_scoped_to_one_evaluation() {
    let mut session = Session::new();
    let full_source = "prefix\n1+";
    let source = SourceUnit::fragment(
        "nested/example.wq",
        full_source,
        ScriptSpan {
            start: 7,
            end: full_source.len(),
        },
    )
    .expect("valid source fragment");

    let fragment_error = session
        .eval_source(source)
        .expect_err("incomplete fragment should fail");
    let fragment_context = fragment_error
        .source_ctx
        .expect("fragment error should retain source context");
    assert_eq!(fragment_context.path, "nested/example.wq");
    assert_eq!(fragment_context.text, full_source);

    let snippet_error = session
        .eval_string("2+")
        .expect_err("incomplete snippet should fail");
    let snippet_context = snippet_error
        .source_ctx
        .expect("snippet error should retain source context");
    assert_eq!(snippet_context.path, "<eval>");
    assert_eq!(snippet_context.text, "2+");
}

#[test]
fn source_fragment_compiler_errors_use_full_source_coordinates() {
    let mut session = Session::new();
    let full_source = "#!/usr/bin/env wq\n@b\n";
    let start = full_source.find("@b").expect("break expression");
    let source = SourceUnit::fragment(
        "nested/compiler.wq",
        full_source,
        ScriptSpan {
            start,
            end: full_source.len(),
        },
    )
    .expect("valid source fragment");

    let error = session
        .eval_source(source)
        .expect_err("break outside a loop should fail during compilation");

    assert_eq!(error.span, Some((start, start + 2)));
    let context = error
        .source_ctx
        .expect("compiler error should retain source context");
    assert_eq!(context.path, "nested/compiler.wq");
    assert_eq!(context.text, full_source);
}

#[test]
fn calling_a_bound_function_preserves_its_defining_source_context() {
    let mut session = Session::new();
    session.set_backtrace_enabled(true);
    session
        .eval_source(SourceUnit::named("definition.wq", "fail:{1/0}"))
        .expect("define function");

    let error = session
        .eval_source(SourceUnit::named("caller.wq", "fail[]"))
        .expect_err("bound function should fail");
    let context = error
        .source_ctx
        .expect("runtime error should retain defining source context");

    assert_eq!(context.path, "definition.wq");
    assert_eq!(context.text, "fail:{1/0}");
}

#[test]
fn source_fragments_reject_invalid_utf8_ranges() {
    let source = "🦀+1";
    let error = SourceUnit::fragment("utf8.wq", source, ScriptSpan { start: 1, end: 4 })
        .expect_err("range starts inside a Unicode scalar");

    assert!(error.to_string().contains("UTF-8 boundary"));
}

#[test]
fn profiler_reports_through_the_session_stderr() {
    let (output, stderr) = capture();
    let mut session = Session::new();
    session.set_stderr(Box::new(output));
    session.set_color_mode(wqpl::style::ColorMode::Never);
    session.set_interpreter(InterpreterKind::Profiler);

    session
        .eval_string("sum 1..4")
        .expect("profiled evaluation should succeed");

    let report = stderr.lock().expect("stderr lock").clone();
    assert!(report.contains("PROFILE"));
    assert!(report.contains("Top Inst Variants"));
}

#[test]
fn core_script_evaluation_handles_shebangs_and_returns_the_last_value() {
    let mut session = Session::new();
    let source = SourceUnit::named("script.wq", "#!/usr/bin/env wq\na:1\nb:2\na+b\n");

    let value = session
        .eval_script(source)
        .expect("script should evaluate through core pipeline");

    assert_eq!(value, wqpl::value::Value::Int(3));
}

#[test]
fn host_script_evaluation_owns_fragments_directives_and_last_value() {
    let mut session = Session::new();
    let full_source = "ignored prefix\n#!/usr/bin/env wq\na:1\n\\load host-value\na+1\n";
    let base_offset = full_source.find("#!").expect("script start");
    let source = SourceUnit::fragment(
        "host-script.wq",
        full_source,
        ScriptSpan {
            start: base_offset,
            end: full_source.len(),
        },
    )
    .expect("valid script fragment");
    let expected_directive_start = full_source.find("\\load").expect("directive");
    let mut handled = false;

    let value = session
        .eval_script_with(source, |_, directive| {
            handled = true;
            assert_eq!(directive.span().start, expected_directive_start);
            Ok::<_, std::convert::Infallible>(Some(wqpl::value::Value::Int(41)))
        })
        .expect("host script evaluation")
        .expect("script result");

    assert!(handled);
    assert_eq!(value, wqpl::value::Value::Int(2));
}

#[test]
fn core_script_evaluation_reports_host_resolved_directives() {
    let mut session = Session::new();
    let source = SourceUnit::named("script.wq", "a:1\n\\l ./library.wq\na\n");

    let error = session
        .eval_script(source)
        .expect_err("load directive requires a host loader");

    assert_eq!(error.err_type, wqpl::wqerror::WqErrorType::Syntax);
    assert!(
        error
            .msg
            .as_deref()
            .is_some_and(|message| message.contains("host loader"))
    );
    assert_eq!(
        error.source_ctx.expect("directive source context").path,
        "script.wq"
    );
    assert!(!session.bindings().contains_key("a"));
}

#[test]
fn execution_reset_preserves_bindings_and_host_configuration() {
    let (output, stderr) = capture();
    let flags = DebugLogFlags::parse("value").expect("debug flags");
    let mut session = Session::new();
    session.set_stderr(Box::new(output));
    session.set_debug_flags(flags);
    session.set_color_mode(wqpl::style::ColorMode::Never);
    session.eval_string("answer:42").expect("bind answer");
    session
        .eval_string("increment:{[x]x+1}")
        .expect("bind compiled function");
    session.set_wqdb(true);

    session.reset_execution_state();

    assert_eq!(session.debug_flags(), flags);
    assert_eq!(session.color_mode(), wqpl::style::ColorMode::Never);
    assert!(session.is_wqdb_enabled());
    assert_eq!(
        session
            .eval_string("answer")
            .expect("binding survives reset"),
        wqpl::value::Value::Int(42)
    );
    assert_eq!(
        session
            .eval_string("increment 2")
            .expect("compiled binding survives reset"),
        wqpl::value::Value::Int(3)
    );
    assert!(stderr.lock().expect("stderr lock").contains("Int(42)"));
}

#[test]
fn workspace_reset_clears_bindings_but_preserves_modes() {
    let flags = DebugLogFlags::parse("ast").expect("debug flags");
    let (output, _) = capture();
    let mut session = Session::new();
    session.set_stderr(Box::new(output));
    session.set_debug_flags(flags);
    session.set_color_mode(wqpl::style::ColorMode::Always);
    session.set_dry_mode(true);
    session.eval_string("answer:42").expect("dry evaluation");
    session.set_dry_mode(false);
    session.eval_string("answer:42").expect("bind answer");
    session.set_dry_mode(true);

    session.reset_workspace();

    assert_eq!(session.debug_flags(), flags);
    assert_eq!(session.color_mode(), wqpl::style::ColorMode::Always);
    assert!(session.dry_mode());
    assert!(!session.bindings().contains_key("answer"));
}
