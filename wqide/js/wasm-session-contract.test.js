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

  const { initSync, WasmWqSession } = await import(wasmModuleUrl);
  assert.equal(initSync({ module: bytes }), undefined);

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
