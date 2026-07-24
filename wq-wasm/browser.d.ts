export {
  doc_index,
  get_doc_markdown,
  get_wq_ver,
  WasmFrontend,
} from "./pkg/wq_wasm.js";
export type {
  DebugPause,
  DebuggerInstruction,
  DebuggerLocation,
  DebuggerSourceBreakpoint,
  DebuggerStackFrame,
  DebuggerSymbolMutation,
  DebuggerSymbolTracker,
  DebuggerTrackResult,
  DebuggerValue,
  DocTopicInfo,
  EvaluationSlice,
  FrontendDiagnostic,
  GlobalBinding,
  HighlightSpan,
  InitInput,
  RenderedValue,
  SymbolAnalysis,
  SymbolDefinition,
  SymbolError,
  SymbolOccurrence,
  SyncInitInput,
  WqDiagnostic,
  WqDiagnosticDataValue,
  WqByteSpan,
  WqCursorContext,
  WqSpan,
  WqStackFrame,
} from "./pkg/wq_wasm.js";

import type {
  DebugPause,
  DebuggerInstruction,
  DebuggerSourceBreakpoint,
  DebuggerStackFrame,
  DebuggerSymbolMutation,
  DebuggerSymbolTracker,
  DebuggerTrackResult,
  DebuggerValue,
  GlobalBinding,
  RenderedValue,
} from "./pkg/wq_wasm.js";
import type { InitInput, SyncInitInput } from "./pkg/wq_wasm.js";

export type DebuggerResumeAction =
  | "continue"
  | "step_in"
  | "step_over"
  | "step_out";

export type DebuggerStepGranularity = "line" | "expr" | "inst";

export interface DebuggerStop {
  readonly pause: DebugPause;
  readonly sourcePath: string;
  stack(): DebuggerStackFrame[];
  globals(): DebuggerValue[];
  locals(frameIndex: number): DebuggerValue[];
  instruction(pc?: number): DebuggerInstruction | null;
  granularity(): DebuggerStepGranularity;
  setGranularity(name: DebuggerStepGranularity): DebuggerStepGranularity;
  setSourceBreakpoints(
    lines: Iterable<number>,
  ): DebuggerSourceBreakpoint[];
  trackSymbol(name: string): DebuggerTrackResult;
  trackGlobal(name: string): DebuggerTrackResult;
  trackLocal(name: string): DebuggerTrackResult;
  trackCapture(slot: number): DebuggerTrackResult;
  trackers(): DebuggerSymbolTracker[];
  removeTracker(id: number): boolean;
  clearTrackers(): void;
  takeNotifications(): DebuggerSymbolMutation[];
}

export interface EvalWqAsyncOptions {
  signal?: AbortSignal;
  timeSliceMs?: number;
  sourcePath?: string;
  onDebuggerPause?: (
    stop: DebuggerStop,
  ) => DebuggerResumeAction | PromiseLike<DebuggerResumeAction>;
  onDebuggerNotification?: (
    notification: DebuggerSymbolMutation,
  ) => void | PromiseLike<void>;
}

export function initSync(module: { module: SyncInitInput } | SyncInitInput): void;

export default function init(
  moduleOrPath?:
    | { module_or_path: InitInput | Promise<InitInput> }
    | InitInput
    | Promise<InitInput>,
): Promise<void>;

export class WasmWqSession {
  constructor();
  free(): void;
  [Symbol.dispose](): void;
  apply_box_flags(spec: string): void;
  apply_debug_flags(spec: string): void;
  arm_wqdb_next(): void;
  backtrace_enabled(): boolean;
  builtin_preset_names(): string[];
  clear_bindings(): void;
  eval_wq(src: string): RenderedValue;
  eval_wq_async(
    src: string,
    options?: EvalWqAsyncOptions,
  ): Promise<RenderedValue>;
  get_box_flags(): string;
  get_box_summary(): string;
  get_builtins_preset(): string;
  get_debug_flags(): string;
  get_dry_mode(): boolean;
  get_interpreter_name(): string;
  get_debugger_step_granularity(): DebuggerStepGranularity;
  get_wqdb_mode(): boolean;
  debugger_stack(): DebuggerStackFrame[];
  debugger_globals(): DebuggerValue[];
  debugger_locals(frameIndex: number): DebuggerValue[];
  debugger_instruction(pc: number): DebuggerInstruction | null;
  debugger_symbol_trackers(): DebuggerSymbolTracker[];
  globals(): GlobalBinding[];
  interpreter_names(): string[];
  reset_execution_state(): void;
  reset_workspace(): void;
  set_ansi_styles_enabled(on: boolean): void;
  set_backtrace_enabled(on: boolean): void;
  set_box_flags(spec: string): void;
  set_builtins_preset(name: string): string;
  set_debug_flags(spec: string): void;
  set_debugger_source_breakpoints(
    sourcePath: string,
    lines: Iterable<number>,
  ): DebuggerSourceBreakpoint[];
  set_debugger_step_granularity(
    name: DebuggerStepGranularity,
  ): DebuggerStepGranularity;
  set_dry_mode(on: boolean): void;
  set_interpreter_by_name(name: string): string;
  set_stderr_callback(callback?: ((chunk: string) => void) | null): void;
  set_stdin_callback(
    callback?:
      | ((prompt: string) =>
          | string
          | null
          | undefined
          | PromiseLike<string | null | undefined>)
      | null,
  ): void;
  set_stdout_callback(callback?: ((chunk: string) => void) | null): void;
  set_wqdb_mode(on: boolean): void;
  track_debugger_symbol(name: string): DebuggerTrackResult;
  track_debugger_global(name: string): DebuggerTrackResult;
  track_debugger_local(name: string): DebuggerTrackResult;
  track_debugger_capture(slot: number): DebuggerTrackResult;
  remove_debugger_symbol_tracker(id: number): boolean;
  clear_debugger_symbol_trackers(): void;
  take_debugger_notifications(): DebuggerSymbolMutation[];
}
