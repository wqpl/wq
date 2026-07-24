use std::sync::{Arc, Mutex};

use wqpl::debug::{ChunkId, CodeLoc, CrashFrame};
use wqpl::interpret::InterpreterKind;
use wqpl::script::ScriptSpan;
use wqpl::session::dbglog::DebugLogFlags;
use wqpl::session::stdio::{WqInput, WqIoError, WqOutput};
use wqpl::session::{DirectiveFailure, EvaluationPhase, ScriptRunError, Session, SourceUnit};
use wqpl::value::Value;

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

struct FailingOutput;

impl WqOutput for FailingOutput {
    fn write(&mut self, _text: &str) -> Result<(), WqIoError> {
        Err(WqIoError::Other("diagnostic sink failed".to_string()))
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
        .error
        .source_ctx
        .expect("fragment error should retain source context");
    assert_eq!(fragment_context.path, "nested/example.wq");
    assert_eq!(fragment_context.text, full_source);

    let snippet_error = session
        .eval_string("2+")
        .expect_err("incomplete snippet should fail");
    let snippet_context = snippet_error
        .error
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
        .error
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
        .error
        .source_ctx
        .expect("runtime error should retain defining source context");

    assert_eq!(context.path, "definition.wq");
    assert_eq!(context.text, "fail:{1/0}");
}

#[test]
fn nested_runtime_errors_do_not_require_debug_artifacts() {
    let mut session = Session::new();
    session.set_backtrace_enabled(false);

    let error = session
        .eval_source(SourceUnit::named("no-backtrace.wq", "fail:{1/0};fail[]"))
        .expect_err("nested call should report division by zero");

    assert_eq!(error.err_type, wqpl::wqerror::WqErrorType::ZeroDiv);
    assert!(error.crash().is_none());
}

#[test]
fn disabling_backtraces_prevents_stale_source_attachment() {
    let mut session = Session::new();
    session
        .eval_source(SourceUnit::named("old.wq", "f:{2/1};f[]"))
        .expect("old evaluation should succeed");
    session.set_backtrace_enabled(false);

    let error = session
        .eval_source(SourceUnit::named("new.wq", "f:{1/0};f[]"))
        .expect_err("new function should fail");

    assert_eq!(error.err_type, wqpl::wqerror::WqErrorType::ZeroDiv);
    assert!(error.crash().is_none());
    if let Some(context) = error.source_ctx.as_deref() {
        assert_eq!(context.path, "new.wq");
    }
}

#[test]
fn top_level_runtime_errors_retain_their_source_context() {
    let mut session = Session::new();

    let error = session
        .eval_source(SourceUnit::named("direct.wq", "1/0"))
        .expect_err("division by zero should fail");
    assert_eq!(error.phase, EvaluationPhase::Execute);
    let context = error
        .error
        .source_ctx
        .as_deref()
        .expect("runtime error should retain its source context");

    assert_eq!(context.path, "direct.wq");
    assert_eq!(context.text, "1/0");
    assert_eq!(error.span, Some((0, 3)));
    assert_eq!(
        error
            .crash()
            .expect("execution failure should carry a crash")
            .frames()
            .first()
            .map(CrashFrame::function),
        Some("<script>")
    );
}

#[test]
fn callback_runtime_errors_retain_their_source_context() {
    let mut session = Session::new();
    let source = "f:{[den]1/den};map[(2;0);f]";

    let error = session
        .eval_source(SourceUnit::named("callback.wq", source))
        .expect_err("callback should fail");
    let context = error
        .error
        .source_ctx
        .as_deref()
        .expect("callback error should retain its source context");

    assert_eq!(context.path, "callback.wq");
    assert_eq!(context.text, source);
    assert_eq!(error.span, Some((8, 13)));
    let frames = error
        .crash()
        .expect("callback error should carry a crash")
        .frames();
    assert_eq!(
        frames.iter().map(CrashFrame::function).collect::<Vec<_>>(),
        ["f", "<script>"]
    );
    assert!(
        frames
            .first()
            .and_then(CrashFrame::locals)
            .is_some_and(|locals| locals.contains(&(0, Value::Int(0)))),
        "f should retain the failing den argument"
    );
}

#[test]
fn runtime_failures_match_across_interpreters_without_leaking_postmortem_state() {
    let source_text = "inner:{1/0};outer:{inner[];0};outer[]";
    let mut vanilla_failure = None;

    for kind in [
        InterpreterKind::Vanilla,
        InterpreterKind::Sample,
        InterpreterKind::Profiler,
    ] {
        let (output, _) = capture();
        let mut session = Session::new();
        session.set_stderr(Box::new(output));
        session.set_color_mode(wqpl::style::ColorMode::Never);
        session.set_interpreter(kind);

        let failure = session
            .eval_source(SourceUnit::named("interpreter-parity.wq", source_text))
            .expect_err("nested division should fail");
        let context = failure
            .error
            .source_ctx
            .as_deref()
            .expect("runtime failure should retain source context");
        let observed = (
            failure.phase,
            failure.err_type,
            failure.span,
            context.path.clone(),
            context.text.clone(),
            failure
                .crash()
                .expect("runtime failure should retain a crash")
                .frames()
                .iter()
                .map(|frame| frame.function().to_string())
                .collect::<Vec<_>>(),
        );

        assert_eq!(observed.0, EvaluationPhase::Execute);
        assert_eq!(observed.2, Some((7, 10)));
        assert_eq!(observed.3, "interpreter-parity.wq");
        assert_eq!(observed.4, source_text);
        assert_eq!(observed.5, ["inner", "outer", "<script>"]);
        assert!(session.postmortem_available(&failure));

        if let Some(expected) = &vanilla_failure {
            assert_eq!(&observed, expected, "{} diverged from vanilla", kind.name());
        } else {
            vanilla_failure = Some(observed);
        }

        session
            .eval_source(SourceUnit::named("clean.wq", "42"))
            .expect("later evaluation should succeed");
        assert!(!session.postmortem_available(&failure));
        assert!(session.debugger().backtrace().is_empty());
    }
}

#[test]
fn nested_calls_preserve_tail_call_history_in_logical_order() {
    let mut session = Session::new();
    let failure = session
        .eval_source(SourceUnit::named(
            "tail-history.wq",
            "h:{1/0};g:{h[];0};f:{g[]};f[]",
        ))
        .expect_err("h should divide by zero");
    let frames = failure
        .crash()
        .expect("runtime failure should retain a crash")
        .frames()
        .iter()
        .map(|frame| frame.function())
        .collect::<Vec<_>>();

    assert_eq!(frames, ["h", "g", "f", "<script>"]);
}

#[test]
fn repeated_non_tail_recursion_preserves_every_frame_and_its_locals() {
    let mut session = Session::new();
    let failure = session
        .eval_source(SourceUnit::named(
            "recursive.wq",
            "f:{[n]$[n=0;1/0;1+f[n-1]]};f[3]",
        ))
        .expect_err("base case should divide by zero");
    let ns = failure
        .crash()
        .expect("runtime failure should retain a crash")
        .frames()
        .iter()
        .filter(|frame| frame.function() == "f")
        .map(|frame| {
            frame
                .locals()
                .and_then(|locals| locals.first())
                .map(|(_, value)| value.clone())
                .expect("recursive frame should retain n")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        ns,
        [Value::Int(0), Value::Int(1), Value::Int(2), Value::Int(3)]
    );
}

#[test]
fn repeated_tail_recursion_preserves_ring_entries_and_marks_overflow() {
    let mut session = Session::new();
    let failure = session
        .eval_source(SourceUnit::named(
            "tail-recursive.wq",
            "f:{[n]$[n=0;1/0;f[n-1]]};f[140]",
        ))
        .expect_err("base case should divide by zero");
    let frames = failure
        .crash()
        .expect("runtime failure should retain a crash")
        .frames();

    assert_eq!(
        frames
            .iter()
            .filter(|frame| frame.function() == "f")
            .count(),
        129
    );
    assert_eq!(
        frames
            .iter()
            .filter(|frame| matches!(frame, CrashFrame::TailCallsOmitted))
            .count(),
        1
    );
}

#[test]
fn rejected_tail_call_does_not_add_a_phantom_caller_frame() {
    let mut session = Session::new();
    let failure = session
        .eval_source(SourceUnit::named("bad-tail-call.wq", "f:{[x]x};f[]"))
        .expect_err("tail call should reject the missing argument");
    let frames = failure
        .crash()
        .expect("runtime failure should retain a crash")
        .frames()
        .iter()
        .map(CrashFrame::function)
        .collect::<Vec<_>>();

    assert_eq!(frames, ["<script>"]);
}

#[test]
fn frontend_failure_clears_postmortem_without_mutating_the_old_failure() {
    let mut session = Session::new();
    let runtime = session
        .eval_source(SourceUnit::named("runtime.wq", "f:{1/0};f[]"))
        .expect_err("runtime evaluation should fail");
    let first_render = runtime.render_with_color_mode(wqpl::style::ColorMode::Never, true);

    assert!(runtime.crash().is_some());
    assert!(session.postmortem_available(&runtime));
    assert_eq!(
        first_render,
        runtime.render_with_color_mode(wqpl::style::ColorMode::Never, true)
    );

    let syntax = session
        .eval_source(SourceUnit::named("syntax.wq", "1+"))
        .expect_err("incomplete expression should fail");

    assert!(matches!(
        syntax.phase,
        EvaluationPhase::Parse | EvaluationPhase::Compile
    ));
    assert!(syntax.crash().is_none());
    assert!(!session.postmortem_available(&runtime));
    assert!(!session.postmortem_available(&syntax));
    assert_eq!(
        first_render,
        runtime.render_with_color_mode(wqpl::style::ColorMode::Never, true)
    );
}

#[test]
fn postmortem_frames_keep_aligned_locals_and_owned_source() {
    let mut session = Session::new();
    let failure = session
        .eval_source(SourceUnit::named(
            "locals.wq",
            "inner:{[den]marker:41;marker/den};outer:{[arg]inner[arg];0};outer 0",
        ))
        .expect_err("inner should divide by zero");
    let rendered = failure.render_with_color_mode(wqpl::style::ColorMode::Never, true);

    {
        let debugger = session
            .postmortem_debugger(&failure)
            .expect("matching failure should provide postmortem state");
        let frames = debugger.backtrace();
        let inner_index = frames
            .iter()
            .position(|frame| frame.function() == "inner")
            .expect("inner frame");
        let outer_index = frames
            .iter()
            .position(|frame| frame.function() == "outer")
            .expect("outer frame");
        let inner = debugger
            .frame_locals(inner_index)
            .expect("inner locals should align with inner frame");
        let outer = debugger
            .frame_locals(outer_index)
            .expect("outer locals should align with outer frame");

        assert!(inner.locals.contains(&(0, Value::Int(0))));
        assert!(inner.locals.contains(&(1, Value::Int(41))));
        assert!(outer.locals.contains(&(0, Value::Int(0))));
    }

    session.reset_workspace();
    assert_eq!(
        rendered,
        failure.render_with_color_mode(wqpl::style::ColorMode::Never, true)
    );
}

#[test]
fn host_diagnostic_failure_never_reuses_a_runtime_crash() {
    let mut session = Session::new();
    let runtime = session
        .eval_string("f:{1/0};f[]")
        .expect_err("runtime evaluation should fail");
    assert!(session.postmortem_available(&runtime));

    session.set_stderr(Box::new(FailingOutput));
    session.set_debug_flags(DebugLogFlags::parse("token").expect("debug flags"));
    let host = session
        .eval_string("1")
        .expect_err("diagnostic output should fail");

    assert_eq!(host.phase, EvaluationPhase::Host);
    assert_eq!(host.err_type, wqpl::wqerror::WqErrorType::Io);
    assert!(host.crash().is_none());
    assert!(!session.postmortem_available(&runtime));
}

#[test]
fn caught_errors_expose_their_stack_without_leaving_postmortem_state() {
    let mut session = Session::new();
    let value = session
        .eval_string("f:{1/0};@t f[]")
        .expect("try should catch the function error");
    let Value::List(result) = value else {
        panic!("expected tagged try result");
    };
    let Value::Dict(error) = &result[1] else {
        panic!("expected structured error payload");
    };
    let Value::List(stack) = error.get("stack").expect("error stack") else {
        panic!("expected stack list");
    };

    assert!(!stack.is_empty());
    assert!(session.debugger().backtrace().is_empty());
}

#[test]
fn wqdb_captures_postmortem_state_when_automatic_backtraces_are_off() {
    let mut session = Session::new();
    session.set_backtrace_enabled(false);
    session.set_wqdb(true);

    let failure = session
        .eval_string("f:{[x]y:41;1/0};f 3")
        .expect_err("function should fail");

    assert!(failure.crash().is_some());
    assert!(session.postmortem_available(&failure));
}

#[test]
fn a_caught_error_cannot_poison_a_later_unhandled_error() {
    let mut session = Session::new();

    let failure = session
        .eval_string("f:{1/0};caught:@t f[];missing[]")
        .expect_err("missing function should remain unhandled");
    let frames = failure.crash().expect("unhandled error crash").frames();

    assert_eq!(failure.err_type, wqpl::wqerror::WqErrorType::NotBound);
    assert!(frames.iter().all(|frame| frame.function() != "f"));
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
fn outer_script_success_clears_old_postmortem_for_empty_and_directive_only() {
    let mut session = Session::new();
    let before_empty = session
        .eval_string("f:{1/0};f[]")
        .expect_err("first runtime evaluation should fail");
    assert!(session.postmortem_available(&before_empty));

    let empty = session
        .eval_script_with(SourceUnit::named("empty.wq", ""), |_, _| {
            Ok::<_, &'static str>(None)
        })
        .expect("empty script should succeed");

    assert_eq!(empty, None);
    assert!(!session.postmortem_available(&before_empty));
    assert!(session.debugger().backtrace().is_empty());

    let before_directive = session
        .eval_string("g:{1/0};g[]")
        .expect_err("second runtime evaluation should fail");
    assert!(session.postmortem_available(&before_directive));

    let directive = session
        .eval_script_with(
            SourceUnit::named("directive-only.wq", "\\load value\n"),
            |_, _| Ok::<_, &'static str>(Some(Value::Int(7))),
        )
        .expect("directive-only script should succeed");

    assert_eq!(directive, Some(Value::Int(7)));
    assert!(!session.postmortem_available(&before_directive));
    assert!(session.debugger().backtrace().is_empty());
}

#[test]
fn outer_script_plain_directive_discards_a_swallowed_nested_failure() {
    let mut session = Session::new();
    let result = session.eval_script_with_postmortem(
        SourceUnit::named("outer.wq", "\\load nested\n"),
        |session, _| {
            let nested = session
                .eval_source(SourceUnit::named("nested.wq", "nested:{1/0};nested[]"))
                .expect_err("nested evaluation should fail");
            assert!(session.postmortem_available(&nested));
            Err(DirectiveFailure::new("plain directive failure"))
        },
    );

    assert!(matches!(
        result,
        Err(ScriptRunError::Directive("plain directive failure"))
    ));
    assert!(session.debugger().backtrace().is_empty());
}

#[test]
fn outer_script_associated_directive_preserves_its_exact_nested_failure() {
    let mut session = Session::new();
    let result = session.eval_script_with_postmortem(
        SourceUnit::named("outer.wq", "\\load nested\n"),
        |session, _| {
            let nested = session
                .eval_source(SourceUnit::named("nested.wq", "nested:{1/0};nested[]"))
                .expect_err("nested evaluation should fail");
            Err(DirectiveFailure::classify(nested, |failure| {
                failure.postmortem_token()
            }))
        },
    );
    let nested = match result {
        Err(ScriptRunError::Directive(failure)) => failure,
        Ok(_) => panic!("associated directive should fail"),
        Err(ScriptRunError::Evaluation(_)) => panic!("directive should wrap the nested failure"),
    };

    assert!(session.postmortem_available(&nested));
    assert_eq!(
        nested
            .crash()
            .expect("nested failure should retain its crash")
            .frames()
            .first()
            .map(CrashFrame::function),
        Some("nested")
    );
    assert_eq!(
        nested
            .error
            .source_ctx
            .as_deref()
            .expect("nested source context")
            .path,
        "nested.wq"
    );
}

#[test]
fn outer_script_rejects_a_stale_same_session_postmortem_token() {
    let mut session = Session::new();
    let prior = session
        .eval_string("prior:{1/0};prior[]")
        .expect_err("prior runtime evaluation should fail");
    let token = prior.postmortem_token().expect("prior crash token");

    let result = session.eval_script_with_postmortem(
        SourceUnit::named("outer.wq", "\\load stale\n"),
        |_, _| {
            Err(DirectiveFailure::classify("stale token", |_| {
                Some(token.clone())
            }))
        },
    );

    assert!(matches!(
        result,
        Err(ScriptRunError::Directive("stale token"))
    ));
    assert!(!session.postmortem_available(&prior));
    assert!(session.debugger().backtrace().is_empty());
}

#[test]
fn outer_script_rejects_a_cross_session_postmortem_token() {
    let mut source_session = Session::new();
    let foreign = source_session
        .eval_string("foreign:{1/0};foreign[]")
        .expect_err("foreign runtime evaluation should fail");
    let token = foreign.postmortem_token().expect("foreign crash token");
    let mut target_session = Session::new();

    let result = target_session.eval_script_with_postmortem(
        SourceUnit::named("outer.wq", "\\load foreign\n"),
        |_, _| {
            Err(DirectiveFailure::classify("foreign token", |_| {
                Some(token.clone())
            }))
        },
    );

    assert!(matches!(
        result,
        Err(ScriptRunError::Directive("foreign token"))
    ));
    assert!(source_session.postmortem_available(&foreign));
    assert!(target_session.debugger().backtrace().is_empty());
}

#[test]
fn outer_script_plain_directive_cannot_inherit_a_prior_runtime_crash() {
    let mut session = Session::new();
    let prior = session
        .eval_string("prior:{1/0};prior[]")
        .expect_err("prior runtime evaluation should fail");
    assert!(session.postmortem_available(&prior));

    let result = session.eval_script_with(
        SourceUnit::named("plain-directive.wq", "\\load missing\n"),
        |_, _| Err::<Option<Value>, _>("plain directive failure"),
    );

    assert!(matches!(
        result,
        Err(ScriptRunError::Directive("plain directive failure"))
    ));
    assert!(!session.postmortem_available(&prior));
    assert!(session.debugger().backtrace().is_empty());
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
        error
            .error
            .source_ctx
            .expect("directive source context")
            .path,
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
