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
const INITIAL_INSTRUCTION_BUDGET = 10_000;
const MIN_INSTRUCTION_BUDGET = 100;
const MAX_INSTRUCTION_BUDGET = 1_000_000;

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
  #disposeRequested = false;

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
    const timeSliceMs = options.timeSliceMs ?? DEFAULT_TIME_SLICE_MS;
    if (!Number.isFinite(timeSliceMs) || timeSliceMs <= 0) {
      throw new TypeError("timeSliceMs must be a positive finite number");
    }

    this.#activeEvaluation = true;
    let started = false;
    let finished = false;
    let instructionBudget = INITIAL_INSTRUCTION_BUDGET;
    try {
      if (signal?.aborted) {
        throw abortError(signal.reason);
      }
      this.#evaluationCall("start_eval_wq", src);
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
          instructionBudget,
        );
        const elapsed = Math.max(now() - sliceStarted, 0.01);
        const adjustment = Math.min(
          4,
          Math.max(0.25, timeSliceMs / elapsed),
        );
        instructionBudget = Math.min(
          MAX_INSTRUCTION_BUDGET,
          Math.max(
            MIN_INSTRUCTION_BUDGET,
            Math.round(instructionBudget * adjustment),
          ),
        );

        if (result.status === "ready") {
          finished = true;
          return result.value;
        }
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
    return this.#call("set_stdin_callback", callback);
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
