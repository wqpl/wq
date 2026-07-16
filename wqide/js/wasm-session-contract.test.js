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
    let toggleError = null;
    const originalBoxMode = session.get_box_mode();
    session.set_stdout_callback(() => {
      try {
        session.get_debug_flags();
      } catch (error) {
        callbackError = error;
      }
      try {
        session.toggle_box_mode();
      } catch (error) {
        toggleError = error;
      }
    });

    const result = session.eval_wq("echo 1");
    assert.equal(result.display, "()");
    assert.deepEqual(callbackError, {
      version: 1,
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
    assert.deepEqual(toggleError, callbackError);
    assert.equal(session.get_box_mode(), originalBoxMode);

    assert.throws(
      () => session.eval_wq("f:{assert_eq[1;2]};f[]"),
      (error) => {
        assert.equal(error.version, 1);
        assert.equal(error.kind, "assert");
        assert.deepEqual(error.data.actual, {
          display: "1",
          type_name: "int",
        });
        assert.deepEqual(error.data.expected, {
          display: "2",
          type_name: "int",
        });
        assert.ok(error.stack.some((frame) => frame.function === "f"));
        assert.equal(error.cause, null);
        return true;
      },
    );

    session.set_stdin_callback(() => 42);
    assert.throws(
      () => session.eval_wq("input[]"),
      (error) => {
        assert.equal(error.version, 1);
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
});
