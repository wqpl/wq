export {
  doc_index,
  get_doc_markdown,
  get_wq_ver,
  WasmFrontend,
} from "./pkg/wq_wasm.js";
export type {
  DocTopicInfo,
  EvaluationSlice,
  GlobalBinding,
  InitInput,
  RenderedValue,
  SymbolAnalysis,
  SymbolDefinition,
  SymbolError,
  SymbolOccurrence,
  SyncInitInput,
  WqDiagnostic,
  WqDiagnosticDataValue,
  WqSpan,
  WqStackFrame,
} from "./pkg/wq_wasm.js";

import type { GlobalBinding, RenderedValue } from "./pkg/wq_wasm.js";
import type { InitInput, SyncInitInput } from "./pkg/wq_wasm.js";

export interface EvalWqAsyncOptions {
  signal?: AbortSignal;
  timeSliceMs?: number;
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
  get_wqdb_mode(): boolean;
  globals(): GlobalBinding[];
  interpreter_names(): string[];
  reset_execution_state(): void;
  reset_workspace(): void;
  set_ansi_styles_enabled(on: boolean): void;
  set_backtrace_enabled(on: boolean): void;
  set_box_flags(spec: string): void;
  set_builtins_preset(name: string): string;
  set_debug_flags(spec: string): void;
  set_dry_mode(on: boolean): void;
  set_interpreter_by_name(name: string): string;
  set_stderr_callback(callback?: ((chunk: string) => void) | null): void;
  set_stdin_callback(
    callback?: ((prompt: string) => string | null | undefined) | null,
  ): void;
  set_stdout_callback(callback?: ((chunk: string) => void) | null): void;
  set_wqdb_mode(on: boolean): void;
}
