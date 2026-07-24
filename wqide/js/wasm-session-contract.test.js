import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const wasmModuleUrl = new URL("../../wq-wasm/browser.js", import.meta.url);
const wasmBinaryUrl = new URL(
  "../../wq-wasm/pkg/wq_wasm_bg.wasm",
  import.meta.url,
);

test("session callback boundaries return structured diagnostics", async (t) => {
  let bytes;
  try {
    bytes = await readFile(wasmBinaryUrl);
  } catch (error) {
    if (error?.code === "ENOENT") {
      t.skip("build wq-wasm first to run the browser API contract test");
      return;
    }
    throw error;
  }

  const { initSync, WasmFrontend, WasmWqSession } = await import(wasmModuleUrl);
  assert.equal(initSync({ module: bytes }), undefined);

  const frontend = new WasmFrontend();
  try {
    assert.equal(frontend.is_complete_input("f:{[x]"), false);
    assert.equal(frontend.is_complete_input(")"), true);
    assert.deepEqual(frontend.diagnostics('value:"'), [
      {
        span: [6, 7],
        kind: "eof",
        message: "string is not properly terminated",
      },
    ]);
    assert.ok(
      frontend
        .highlight_spans("f:{[x] x+1}")
        .some((span) => span.kind === "variable-parameter"),
    );
    assert.equal(frontend.cursor_context_at('"abc"', 2), "string");
    assert.equal(frontend.format_wq("(1;2)|has?@1[2]"), "(1;2)|has?@1 2");
  } finally {
    frontend.free();
  }

  const session = new WasmWqSession();
  try {
    let callbackError = null;
    let setterError = null;
    const originalBoxFlags = session.get_box_flags();
    session.set_stdout_callback(() => {
      try {
        session.get_debug_flags();
      } catch (error) {
        callbackError = error;
      }
      try {
        session.set_box_flags("0");
      } catch (error) {
        setterError = error;
      }
    });

    const result = session.eval_wq("echo 1");
    assert.equal(result.display, "()");
    assert.deepEqual(callbackError, {
      version: 2,
      kind: "reentrant-session-access",
      message:
        "session methods cannot be called reentrantly from an active session callback",
      rendered:
        "session methods cannot be called reentrantly from an active session callback",
      source: null,
      span: null,
      path: null,
      notes: [],
      data: {},
      stack: [],
      cause: null,
    });
    assert.deepEqual(setterError, callbackError);
    assert.equal(session.get_box_flags(), originalBoxFlags);

    let streamedOutput = "";
    session.set_stdout_callback((chunk) => {
      streamedOutput += chunk;
    });
    session.set_box_flags("0");
    session.eval_wq(
      'asciiplot[(1;2;3);(3;2;1);`size:(12;5);`color:("red";"blue")]',
    );
    assert.match(streamedOutput, /\x1b\[/);

    session.set_ansi_styles_enabled(false);
    streamedOutput = "";
    session.eval_wq(
      'asciiplot[(1;2;3);(3;2;1);`size:(12;5);`color:("red";"blue")]',
    );
    assert.doesNotMatch(streamedOutput, /\x1b\[/);
    session.set_ansi_styles_enabled(true);

    assert.throws(
      () => session.eval_wq("f:{assert_eq[1;2]};f[]"),
      (error) => {
        assert.equal(error.version, 2);
        assert.equal(error.kind, "assert");
        assert.deepEqual(error.data.actual, {
          display: "1",
          category: "int",
        });
        assert.deepEqual(error.data.expected, {
          display: "2",
          category: "int",
        });
        assert.ok(error.stack.some((frame) => frame.function === "f"));
        assert.equal(error.cause, null);
        return true;
      },
    );

    let timerRan = false;
    const timer = new Promise((resolve) => {
      setTimeout(() => {
        timerRan = true;
        resolve();
      }, 0);
    });
    const asyncResult = await session.eval_wq_async(
      "i:0;inc:{[x]x+1};W[i<50000;i:inc[i]];i",
      { timeSliceMs: 1 },
    );
    assert.equal(asyncResult.display, "50000");
    assert.equal(timerRan, true);
    await timer;

    for (const interpreter of ["sample", "profiler"]) {
      session.set_interpreter_by_name(interpreter);
      const alternateResult = await session.eval_wq_async(
        "i:0;W[i<100;i+:1];i",
        { timeSliceMs: 1 },
      );
      assert.equal(alternateResult.display, "100");
    }
    session.set_interpreter_by_name("vanilla");

    const controller = new AbortController();
    const abortedEvaluation = session.eval_wq_async(
      "before:41;i:0;W[i<10000000;i+:1];after:42",
      { signal: controller.signal, timeSliceMs: 1 },
    );
    assert.throws(() => session.globals(), /WasmWqSession is evaluating/);
    controller.abort("stop requested");
    await assert.rejects(abortedEvaluation, (error) => {
      assert.equal(error.name, "AbortError");
      assert.match(error.message, /stop requested/);
      return true;
    });
    assert.equal(session.eval_wq("2+3").display, "5");

    let receivedPrompt = null;
    session.set_stdin_callback(async (prompt) => {
      receivedPrompt = prompt;
      await new Promise((resolve) => setTimeout(resolve, 0));
      return "Ada";
    });
    const inputResult = await session.eval_wq_async('input["name> "]');
    assert.equal(receivedPrompt, "name> ");
    assert.equal(inputResult.display, '"Ada"');

    const prompts = [];
    const responses = ["Ada", "Grace"];
    session.set_stdin_callback(async (prompt) => {
      prompts.push(prompt);
      await new Promise((resolve) => setTimeout(resolve, 0));
      return responses.shift();
    });
    const mappedInput = await session.eval_wq_async(
      'map[("first> ";"second> ");input]',
      { timeSliceMs: 1 },
    );
    assert.deepEqual(prompts, ["first> ", "second> "]);
    assert.equal(mappedInput.display, '("Ada";"Grace")');

    const inputController = new AbortController();
    session.set_stdin_callback(() => new Promise(() => {}));
    const waitingForInput = session.eval_wq_async("input[]", {
      signal: inputController.signal,
    });
    inputController.abort("input cancelled");
    await assert.rejects(waitingForInput, (error) => {
      assert.equal(error.name, "AbortError");
      assert.match(error.message, /input cancelled/);
      return true;
    });
    assert.equal(session.eval_wq("3+4").display, "7");

    session.set_stdin_callback(async () => {
      throw new Error("reader failed");
    });
    await assert.rejects(session.eval_wq_async("input[]"), (error) => {
      assert.equal(error.version, 2);
      assert.equal(error.kind, "io");
      assert.match(error.notes.join("\n"), /reader failed/);
      return true;
    });
    assert.equal(session.eval_wq("4+5").display, "9");

    session.set_stdin_callback(() => 42);
    assert.throws(
      () => session.eval_wq("input[]"),
      (error) => {
        assert.equal(error.version, 2);
        assert.equal(error.kind, "io");
        assert.match(
          error.notes.join("\n"),
          /stdin callback must return a string, null, or undefined/,
        );
        assert.ok(error.stack.some((frame) => frame.path === "<wasm>"));
        assert.equal(error.cause, null);
        return true;
      },
    );

    session.set_stdin_callback(null);
    session.set_wqdb_mode(true);
    const debuggerNotifications = [];
    const pauseReasons = [];
    let staleStop = null;
    const debugResult = await session.eval_wq_async(
      'answer:40+2\n@p answer',
      {
        sourcePath: "<contract:debug>",
        onDebuggerNotification(notification) {
          debuggerNotifications.push(notification);
        },
        onDebuggerPause(stop) {
          staleStop = stop;
          pauseReasons.push(stop.pause.reason);
          assert.equal(stop.pause.location.path, "<contract:debug>");
          assert.ok(stop.pause.location.line >= 1);
          assert.ok(Array.isArray(stop.pause.location.span));
          assert.ok(stop.instruction());
          assert.ok(stop.stack().length >= 1);
          assert.equal(stop.granularity(), "expr");
          if (stop.pause.reason === "entry") {
            const tracked = stop.trackGlobal("answer");
            assert.equal(tracked.added, true);
          } else {
            assert.equal(stop.pause.reason, "explicit_pause");
            assert.equal(stop.pause.breakpoint_id, null);
            assert.ok(stop.pause.explicit_pause_id > 0);
            assert.ok(
              stop.globals().some((binding) => binding.name === "answer"),
            );
          }
          return "continue";
        },
      },
    );
    assert.equal(debugResult.display, "42");
    assert.deepEqual(pauseReasons, ["entry", "explicit_pause"]);
    assert.equal(debuggerNotifications.length, 1);
    assert.equal(debuggerNotifications[0].target.name, "answer");
    assert.equal(debuggerNotifications[0].new_value.display, "42");
    assert.throws(
      () => staleStop.stack(),
      /Debugger pause is no longer active/,
    );

    session.arm_wqdb_next();
    const breakpointReasons = [];
    const breakpointResult = await session.eval_wq_async(
      "first:1\nsecond:2\nfirst+second",
      {
        sourcePath: "<contract:breakpoint>",
        onDebuggerPause(stop) {
          breakpointReasons.push(stop.pause.reason);
          if (stop.pause.reason === "entry") {
            const [breakpoint] = stop.setSourceBreakpoints([2]);
            assert.equal(breakpoint.source_path, "<contract:breakpoint>");
            assert.equal(breakpoint.requested_line, 2);
            assert.equal(breakpoint.chunk, null);
          }
          return "continue";
        },
      },
    );
    assert.equal(breakpointResult.display, "3");
    assert.deepEqual(breakpointReasons, ["entry", "breakpoint"]);

    session.arm_wqdb_next();
    const stepReasons = [];
    const stepResult = await session.eval_wq_async(
      "left:1;right:2;left+right",
      {
        sourcePath: "<contract:step>",
        onDebuggerPause(stop) {
          stepReasons.push(stop.pause.reason);
          return stepReasons.length === 1 ? "step_over" : "continue";
        },
      },
    );
    assert.equal(stepResult.display, "3");
    assert.deepEqual(stepReasons, ["entry", "step"]);

    session.arm_wqdb_next();
    await assert.rejects(
      session.eval_wq_async("1", {
        sourcePath: "<contract:missing-handler>",
      }),
      /onDebuggerPause is not configured/,
    );

    session.arm_wqdb_next();
    const debugController = new AbortController();
    let signalPauseReached;
    const pauseReached = new Promise((resolve) => {
      signalPauseReached = resolve;
    });
    let abortedStop = null;
    const pausedEvaluation = session.eval_wq_async("1", {
      signal: debugController.signal,
      sourcePath: "<contract:abort>",
      onDebuggerPause(stop) {
        abortedStop = stop;
        signalPauseReached();
        return new Promise(() => {});
      },
    });
    await pauseReached;
    debugController.abort("debugger cancelled");
    await assert.rejects(pausedEvaluation, (error) => {
      assert.equal(error.name, "AbortError");
      assert.match(error.message, /debugger cancelled/);
      return true;
    });
    assert.throws(
      () => abortedStop.globals(),
      /Debugger pause is no longer active/,
    );
    session.set_wqdb_mode(false);
    assert.equal(session.eval_wq("6+7").display, "13");
  } finally {
    session.free();
  }

  const disposalSession = new WasmWqSession();
  let disposalCallbackRan = false;
  disposalSession.set_stdout_callback(() => {
    disposalCallbackRan = true;
    disposalSession.free();
  });

  const disposalResult = disposalSession.eval_wq("echo 2");
  assert.equal(disposalResult.display, "()");
  assert.equal(disposalCallbackRan, true);
  assert.throws(
    () => disposalSession.get_debug_flags(),
    /WasmWqSession has been disposed/,
  );
  disposalSession.free();

  const asyncDisposalSession = new WasmWqSession();
  const disposedEvaluation = asyncDisposalSession.eval_wq_async(
    "i:0;W[i<10000000;i+:1];i",
    { timeSliceMs: 1 },
  );
  asyncDisposalSession.free();
  await assert.rejects(disposedEvaluation, /WasmWqSession has been disposed/);
  assert.throws(
    () => asyncDisposalSession.get_debug_flags(),
    /WasmWqSession has been disposed/,
  );
});
