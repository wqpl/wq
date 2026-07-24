import rawInit, {
  doc_index,
  get_doc_markdown,
  get_wq_ver,
  initSync as rawInitSync,
  WasmFrontend,
  WasmWqSessionCore,
} from "./pkg/wq_wasm.js";

export { doc_index, get_doc_markdown, get_wq_ver, WasmFrontend };

export function initSync(module) {
  rawInitSync(module);
}

export default async function init(moduleOrPath) {
  await rawInit(moduleOrPath);
}

const DEFAULT_TIME_SLICE_MS = 8;
const INITIAL_WORK_BUDGET = 10_000;
const MIN_WORK_BUDGET = 100;
const MAX_WORK_BUDGET = 1_000_000;

function now() {
  return globalThis.performance?.now() ?? Date.now();
}

function abortError(reason) {
  const message =
    typeof reason === "string"
      ? reason
      : reason instanceof Error
        ? reason.message
        : "Evaluation aborted";
  const error = new Error(message);
  error.name = "AbortError";
  return error;
}

async function yieldToHost() {
  if (globalThis.scheduler?.yield) {
    await globalThis.scheduler.yield();
    return;
  }
  await new Promise((resolve) => setTimeout(resolve, 0));
}

function disposedError() {
  return new Error("WasmWqSession has been disposed");
}

async function awaitWithCancellation(value, signal, disposalSignal) {
  if (signal?.aborted) {
    throw abortError(signal.reason);
  }
  if (disposalSignal.aborted) {
    throw disposedError();
  }

  return await new Promise((resolve, reject) => {
    const cleanup = () => {
      signal?.removeEventListener("abort", onAbort);
      disposalSignal.removeEventListener("abort", onDispose);
    };
    const resolveOnce = (result) => {
      cleanup();
      resolve(result);
    };
    const rejectOnce = (error) => {
      cleanup();
      reject(error);
    };
    const onAbort = () => rejectOnce(abortError(signal.reason));
    const onDispose = () => rejectOnce(disposedError());
    signal?.addEventListener("abort", onAbort, { once: true });
    disposalSignal.addEventListener("abort", onDispose, { once: true });
    Promise.resolve(value).then(resolveOnce, rejectOnce);
  });
}

function inputCallbackMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

const DEBUGGER_RESUME_ACTIONS = new Set([
  "continue",
  "step_in",
  "step_over",
  "step_out",
]);

function debuggerResumeAction(value) {
  if (DEBUGGER_RESUME_ACTIONS.has(value)) return value;
  throw new TypeError(
    "onDebuggerPause must return 'continue', 'step_in', 'step_over', or 'step_out'",
  );
}

function debuggerSourceLines(lines) {
  if (lines == null || typeof lines[Symbol.iterator] !== "function") {
    throw new TypeError("debugger breakpoint lines must be iterable");
  }
  const values = Array.from(lines, Number);
  for (const line of values) {
    if (!Number.isSafeInteger(line) || line < 1 || line > 0xffff_ffff) {
      throw new TypeError(
        "debugger breakpoint lines must be positive 32-bit integers",
      );
    }
  }
  return new Uint32Array(values);
}

/**
 * Public browser session facade.
 *
 * wasm-bindgen's generated `free()` clears its JavaScript pointer before Rust
 * can reject disposal of a borrowed receiver. This facade defers disposal
 * requested by a synchronous runtime callback until the active method returns.
 */
export class WasmWqSession {
  #session;
  #activeCalls = 0;
  #activeEvaluation = false;
  #activePauseId = null;
  #disposeRequested = false;
  #disposalController = new AbortController();
  #stdinCallback = null;

  constructor() {
    this.#session = new WasmWqSessionCore();
  }

  #requireSession() {
    if (!this.#session || this.#disposeRequested) {
      throw new Error("WasmWqSession has been disposed");
    }
    return this.#session;
  }

  #call(method, ...args) {
    const session = this.#requireSession();
    if (this.#activeEvaluation) {
      throw new Error("WasmWqSession is evaluating");
    }
    this.#activeCalls += 1;
    try {
      return session[method](...args);
    } finally {
      this.#activeCalls -= 1;
      this.#flushDisposal();
    }
  }

  #evaluationCall(method, ...args) {
    const session = this.#session;
    if (!session) {
      throw new Error("WasmWqSession has been disposed");
    }
    this.#activeCalls += 1;
    try {
      return session[method](...args);
    } finally {
      this.#activeCalls -= 1;
      this.#flushDisposal();
    }
  }

  #debuggerStop(pause, sourcePath) {
    const pauseId = pause.id;
    const requireCurrentPause = () => {
      if (this.#activePauseId !== pauseId) {
        throw new Error("Debugger pause is no longer active");
      }
    };
    const call = (method, ...args) => {
      requireCurrentPause();
      return this.#evaluationCall(method, ...args);
    };
    return Object.freeze({
      pause,
      sourcePath,
      stack: () => Array.from(call("debugger_stack")),
      globals: () => Array.from(call("debugger_globals")),
      locals: (frameIndex) =>
        Array.from(call("debugger_locals", frameIndex)),
      instruction: (pc = pause.pc) =>
        call("debugger_instruction", pc),
      granularity: () => call("get_debugger_step_granularity"),
      setGranularity: (name) =>
        call("set_debugger_step_granularity", name),
      setSourceBreakpoints: (lines) =>
        Array.from(
          call(
            "set_debugger_source_breakpoints",
            sourcePath,
            debuggerSourceLines(lines),
          ),
        ),
      trackSymbol: (name) => call("track_debugger_symbol", name),
      trackGlobal: (name) => call("track_debugger_global", name),
      trackLocal: (name) => call("track_debugger_local", name),
      trackCapture: (slot) => call("track_debugger_capture", slot),
      trackers: () =>
        Array.from(call("debugger_symbol_trackers")),
      removeTracker: (id) =>
        call("remove_debugger_symbol_tracker", id),
      clearTrackers: () => call("clear_debugger_symbol_trackers"),
      takeNotifications: () =>
        Array.from(call("take_debugger_notifications")),
    });
  }

  async #deliverDebuggerNotifications(
    callback,
    signal,
  ) {
    if (!callback) return;
    const notifications = Array.from(
      this.#evaluationCall("take_debugger_notifications"),
    );
    for (const notification of notifications) {
      await awaitWithCancellation(
        callback(notification),
        signal,
        this.#disposalController.signal,
      );
    }
  }

  #flushDisposal() {
    if (
      !this.#disposeRequested ||
      this.#activeCalls !== 0 ||
      this.#activeEvaluation ||
      !this.#session
    ) {
      return;
    }
    const session = this.#session;
    this.#session = null;
    session.free();
  }

  free() {
    if (!this.#session) return;
    this.#disposeRequested = true;
    this.#disposalController.abort();
    this.#flushDisposal();
  }

  apply_box_flags(spec) {
    return this.#call("apply_box_flags", spec);
  }

  apply_debug_flags(spec) {
    return this.#call("apply_debug_flags", spec);
  }

  backtrace_enabled() {
    return this.#call("backtrace_enabled");
  }

  builtin_preset_names() {
    return this.#call("builtin_preset_names");
  }

  clear_bindings() {
    return this.#call("clear_bindings");
  }

  eval_wq(src) {
    return this.#call("eval_wq", src);
  }

  async eval_wq_async(src, options = {}) {
    this.#requireSession();
    if (this.#activeEvaluation) {
      throw new Error("WasmWqSession is evaluating");
    }

    const { signal } = options;
    const sourcePath = options.sourcePath ?? "<wasm>";
    const onDebuggerPause = options.onDebuggerPause;
    const onDebuggerNotification = options.onDebuggerNotification;
    if (typeof sourcePath !== "string" || sourcePath.length === 0) {
      throw new TypeError("sourcePath must be a non-empty string");
    }
    if (
      onDebuggerPause !== undefined &&
      typeof onDebuggerPause !== "function"
    ) {
      throw new TypeError("onDebuggerPause must be a function");
    }
    if (
      onDebuggerNotification !== undefined &&
      typeof onDebuggerNotification !== "function"
    ) {
      throw new TypeError("onDebuggerNotification must be a function");
    }
    const timeSliceMs = options.timeSliceMs ?? DEFAULT_TIME_SLICE_MS;
    if (!Number.isFinite(timeSliceMs) || timeSliceMs <= 0) {
      throw new TypeError("timeSliceMs must be a positive finite number");
    }

    this.#activeEvaluation = true;
    let started = false;
    let finished = false;
    let workBudget = INITIAL_WORK_BUDGET;
    try {
      if (signal?.aborted) {
        throw abortError(signal.reason);
      }
      this.#evaluationCall("start_eval_wq_named", sourcePath, src);
      started = true;

      while (true) {
        if (this.#disposeRequested) {
          throw new Error("WasmWqSession has been disposed");
        }
        if (signal?.aborted) {
          throw abortError(signal.reason);
        }

        const sliceStarted = now();
        const result = this.#evaluationCall(
          "run_eval_wq_slice",
          workBudget,
        );
        const elapsed = Math.max(now() - sliceStarted, 0.01);
        const adjustment = Math.min(
          4,
          Math.max(0.25, timeSliceMs / elapsed),
        );
        workBudget = Math.min(
          MAX_WORK_BUDGET,
          Math.max(
            MIN_WORK_BUDGET,
            Math.round(workBudget * adjustment),
          ),
        );

        if (result.status === "ready") {
          await this.#deliverDebuggerNotifications(
            onDebuggerNotification,
            signal,
          );
          finished = true;
          return result.value;
        }
        if (result.status === "awaiting_input") {
          const callback = this.#stdinCallback;
          if (!callback) {
            this.#evaluationCall(
              "resume_eval_wq_input",
              result.request_id,
              "error",
              "stdin is not configured",
            );
            continue;
          }

          let input;
          try {
            input = await awaitWithCancellation(
              callback(result.prompt),
              signal,
              this.#disposalController.signal,
            );
          } catch (error) {
            if (this.#disposeRequested) {
              throw disposedError();
            }
            if (signal?.aborted) {
              throw abortError(signal.reason);
            }
            this.#evaluationCall(
              "resume_eval_wq_input",
              result.request_id,
              "error",
              inputCallbackMessage(error),
            );
            continue;
          }

          if (input === null || input === undefined) {
            this.#evaluationCall(
              "resume_eval_wq_input",
              result.request_id,
              "eof",
            );
          } else if (typeof input === "string") {
            this.#evaluationCall(
              "resume_eval_wq_input",
              result.request_id,
              "line",
              input,
            );
          } else {
            this.#evaluationCall(
              "resume_eval_wq_input",
              result.request_id,
              "error",
              "stdin callback must return a string, null, or undefined",
            );
          }
          continue;
        }
        if (result.status === "paused") {
          await this.#deliverDebuggerNotifications(
            onDebuggerNotification,
            signal,
          );
          if (!onDebuggerPause) {
            this.#evaluationCall(
              "resume_eval_wq_debugger",
              result.pause.id,
              "continue",
            );
            continue;
          }
          this.#activePauseId = result.pause.id;
          try {
            const action = debuggerResumeAction(
              await awaitWithCancellation(
                onDebuggerPause(
                  this.#debuggerStop(result.pause, sourcePath),
                ),
                signal,
                this.#disposalController.signal,
              ),
            );
            this.#evaluationCall(
              "resume_eval_wq_debugger",
              result.pause.id,
              action,
            );
          } finally {
            this.#activePauseId = null;
          }
          continue;
        }
        if (result.status !== "yielded") {
          throw new Error(`Unknown evaluation status '${result.status}'`);
        }
        await this.#deliverDebuggerNotifications(
          onDebuggerNotification,
          signal,
        );
        await yieldToHost();
      }
    } finally {
      if (started && !finished && this.#session) {
        try {
          this.#evaluationCall("cancel_eval_wq");
        } catch {
          // Preserve the original evaluation, abort, or disposal error.
        }
      }
      this.#activePauseId = null;
      this.#activeEvaluation = false;
      this.#flushDisposal();
    }
  }

  get_box_flags() {
    return this.#call("get_box_flags");
  }

  get_box_summary() {
    return this.#call("get_box_summary");
  }

  get_builtins_preset() {
    return this.#call("get_builtins_preset");
  }

  get_debug_flags() {
    return this.#call("get_debug_flags");
  }

  get_dry_mode() {
    return this.#call("get_dry_mode");
  }

  get_interpreter_name() {
    return this.#call("get_interpreter_name");
  }

  get_wqdb_mode() {
    return this.#call("get_wqdb_mode");
  }

  arm_wqdb_next() {
    return this.#call("arm_wqdb_next");
  }

  debugger_stack() {
    return Array.from(this.#call("debugger_stack"));
  }

  debugger_globals() {
    return Array.from(this.#call("debugger_globals"));
  }

  debugger_locals(frameIndex) {
    return Array.from(this.#call("debugger_locals", frameIndex));
  }

  debugger_instruction(pc) {
    return this.#call("debugger_instruction", pc);
  }

  get_debugger_step_granularity() {
    return this.#call("get_debugger_step_granularity");
  }

  set_debugger_step_granularity(name) {
    return this.#call("set_debugger_step_granularity", name);
  }

  set_debugger_source_breakpoints(sourcePath, lines) {
    return Array.from(
      this.#call(
        "set_debugger_source_breakpoints",
        sourcePath,
        debuggerSourceLines(lines),
      ),
    );
  }

  track_debugger_symbol(name) {
    return this.#call("track_debugger_symbol", name);
  }

  track_debugger_global(name) {
    return this.#call("track_debugger_global", name);
  }

  track_debugger_local(name) {
    return this.#call("track_debugger_local", name);
  }

  track_debugger_capture(slot) {
    return this.#call("track_debugger_capture", slot);
  }

  debugger_symbol_trackers() {
    return Array.from(this.#call("debugger_symbol_trackers"));
  }

  remove_debugger_symbol_tracker(id) {
    return this.#call("remove_debugger_symbol_tracker", id);
  }

  clear_debugger_symbol_trackers() {
    return this.#call("clear_debugger_symbol_trackers");
  }

  take_debugger_notifications() {
    return Array.from(this.#call("take_debugger_notifications"));
  }

  globals() {
    return this.#call("globals");
  }

  interpreter_names() {
    return this.#call("interpreter_names");
  }

  reset_execution_state() {
    return this.#call("reset_execution_state");
  }

  reset_workspace() {
    return this.#call("reset_workspace");
  }

  set_ansi_styles_enabled(on) {
    return this.#call("set_ansi_styles_enabled", on);
  }

  set_backtrace_enabled(on) {
    return this.#call("set_backtrace_enabled", on);
  }

  set_box_flags(spec) {
    return this.#call("set_box_flags", spec);
  }

  set_builtins_preset(name) {
    return this.#call("set_builtins_preset", name);
  }

  set_debug_flags(spec) {
    return this.#call("set_debug_flags", spec);
  }

  set_dry_mode(on) {
    return this.#call("set_dry_mode", on);
  }

  set_interpreter_by_name(name) {
    return this.#call("set_interpreter_by_name", name);
  }

  set_stderr_callback(callback) {
    return this.#call("set_stderr_callback", callback);
  }

  set_stdin_callback(callback) {
    const result = this.#call("set_stdin_callback", callback);
    this.#stdinCallback = callback ?? null;
    return result;
  }

  set_stdout_callback(callback) {
    return this.#call("set_stdout_callback", callback);
  }

  set_wqdb_mode(on) {
    return this.#call("set_wqdb_mode", on);
  }
}

if (Symbol.dispose) {
  WasmWqSession.prototype[Symbol.dispose] = WasmWqSession.prototype.free;
}
