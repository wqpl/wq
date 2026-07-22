import test from "node:test";
import assert from "node:assert/strict";

import {
  abortError,
  createEvaluationController,
  isAbortError,
  queueEval,
} from "./eval-lifecycle.js";

function deferred() {
  let resolve;
  const promise = new Promise((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

test("evaluation controller aborts active work and returns to idle", async () => {
  const states = [];
  const controller = createEvaluationController((state) => states.push(state));
  const evaluation = controller.run(async ({ signal, setState }) => {
    setState("running");
    await new Promise((_, reject) => {
      signal.addEventListener(
        "abort",
        () => reject(abortError(signal.reason)),
        { once: true },
      );
    });
  });

  assert.equal(controller.stop("user stopped"), true);
  await assert.rejects(evaluation, (error) => {
    assert.equal(isAbortError(error), true);
    assert.match(error.message, /user stopped/);
    return true;
  });
  assert.equal(controller.active, false);
  assert.deepEqual(states, ["queued", "running", "stopping", "idle"]);
});

test("aborted queued work never starts", async () => {
  const blocker = deferred();
  const first = queueEval(() => blocker.promise);
  const controller = createEvaluationController();
  let started = false;
  const queued = controller.run(({ signal, setState }) =>
    queueEval(
      () => {
        setState("running");
        started = true;
      },
      { signal },
    ),
  );

  controller.stop("cancel queued run");
  await assert.rejects(queued, (error) => isAbortError(error));
  assert.equal(started, false);

  blocker.resolve();
  await first;
  await Promise.resolve();
  assert.equal(started, false);
});

test("already-aborted work does not enter the evaluation queue", async () => {
  const controller = new AbortController();
  controller.abort("already stopped");
  let started = false;

  await assert.rejects(
    queueEval(
      () => {
        started = true;
      },
      { signal: controller.signal },
    ),
    (error) => isAbortError(error),
  );
  assert.equal(started, false);
});

test("state updates from a completed run cannot affect a later run", async () => {
  const controller = createEvaluationController();
  let staleSetState;
  await controller.run(({ setState }) => {
    staleSetState = setState;
    setState("running");
  });

  const blocker = deferred();
  const laterRun = controller.run(async ({ setState }) => {
    setState("running");
    await blocker.promise;
  });

  staleSetState("awaiting-input");
  assert.equal(controller.state, "running");
  blocker.resolve();
  await laterRun;
});
