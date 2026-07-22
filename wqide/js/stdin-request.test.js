import test from "node:test";
import assert from "node:assert/strict";

import { isAbortError } from "./eval-lifecycle.js";
import { createStdinRequester } from "./stdin-request.js";

function fakeRenderer() {
  let controls = null;
  const completions = [];
  return {
    render(request) {
      controls = request;
      return {
        complete(completion) {
          completions.push(completion);
        },
      };
    },
    get controls() {
      return controls;
    },
    completions,
  };
}

test("stdin requester distinguishes an empty line from EOF", async () => {
  const renderer = fakeRenderer();
  const requester = createStdinRequester({ render: renderer.render });

  const emptyLine = requester.request("value> ");
  renderer.controls.submit("");
  assert.equal(await emptyLine, "");
  assert.deepEqual(renderer.completions, [{ kind: "line", value: "" }]);

  const eof = requester.request("next> ");
  renderer.controls.eof();
  assert.equal(await eof, null);
  assert.deepEqual(renderer.completions.at(-1), { kind: "eof" });
});

test("stdin requester aborts once and ignores stale submissions", async () => {
  const renderer = fakeRenderer();
  const requester = createStdinRequester({ render: renderer.render });
  const controller = new AbortController();
  const pending = requester.request("name> ", { signal: controller.signal });
  const staleControls = renderer.controls;

  controller.abort("run stopped");
  await assert.rejects(pending, (error) => isAbortError(error));
  staleControls.submit("too late");
  staleControls.eof();

  assert.equal(requester.pending, false);
  assert.deepEqual(renderer.completions, [{ kind: "aborted" }]);
});

test("stdin requester can cancel a request without a host signal", async () => {
  const renderer = fakeRenderer();
  const requester = createStdinRequester({ render: renderer.render });
  const pending = requester.request("value> ");

  assert.equal(requester.cancel("view closed"), true);
  await assert.rejects(pending, (error) => {
    assert.equal(isAbortError(error), true);
    assert.match(error.message, /view closed/);
    return true;
  });
  assert.equal(requester.cancel(), false);
});
