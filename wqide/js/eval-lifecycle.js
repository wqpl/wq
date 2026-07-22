export function abortError(reason = "evaluation interrupted") {
  const message =
    reason instanceof Error ? reason.message : String(reason ?? "evaluation interrupted");
  if (typeof DOMException === "function") {
    return new DOMException(message, "AbortError");
  }
  const error = new Error(message);
  error.name = "AbortError";
  return error;
}

export function isAbortError(error) {
  return error?.name === "AbortError";
}

let evalQueue = Promise.resolve();

export function queueEval(taskFn, { signal } = {}) {
  if (signal?.aborted) {
    return Promise.reject(abortError(signal.reason));
  }
  let started = false;
  let rejectQueuedAbort;
  const queuedAbort = new Promise((_, reject) => {
    rejectQueuedAbort = reject;
  });
  const onAbort = () => {
    if (!started) rejectQueuedAbort(abortError(signal.reason));
  };
  signal?.addEventListener("abort", onAbort, { once: true });

  const evaluation = evalQueue.then(() => {
    started = true;
    if (signal?.aborted) throw abortError(signal.reason);
    return taskFn();
  });
  evalQueue = evaluation.catch(() => {});
  if (!signal) return evaluation;
  return Promise.race([evaluation, queuedAbort]).finally(() => {
    signal.removeEventListener("abort", onAbort);
  });
}

export function createEvaluationController(onState = () => {}) {
  let run = null;

  function setState(targetRun, state) {
    if (run !== targetRun) return;
    targetRun.state = state;
    onState(state);
  }

  return {
    get active() {
      return run !== null;
    },
    get state() {
      return run?.state ?? "idle";
    },
    async run(task) {
      if (run) {
        throw new Error("evaluation is already active");
      }
      const controller = new AbortController();
      const currentRun = { controller, state: "queued" };
      run = currentRun;
      onState("queued");
      try {
        return await task({
          signal: controller.signal,
          setState(state) {
            setState(currentRun, state);
          },
        });
      } finally {
        run = null;
        onState("idle");
      }
    },
    stop(reason = "evaluation interrupted") {
      if (!run || run.controller.signal.aborted) return false;
      setState(run, "stopping");
      run.controller.abort(reason);
      return true;
    },
  };
}
