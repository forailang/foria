// Shared types for the playground Worker protocol.

export interface CompileError {
  file: string;
  line: number;
  col: number;
  message: string;
}

export interface LogEntry {
  level: string;
  message: string;
}

export interface StepSnapshot {
  step: number;
  op: string;
  bind: string;
  bindings: Record<string, unknown>;
}

// Messages sent from main thread to Worker
export type WorkerRequest =
  | { type: "compile"; id: number; files: Record<string, string>; entryPoint: string }
  | { type: "format"; id: number; source: string }
  | { type: "execute"; id: number; files: Record<string, string>; entryPoint: string }
  | { type: "debug"; id: number; files: Record<string, string>; entryPoint: string };

// Messages sent from Worker to main thread
export type WorkerResponse =
  | { type: "ready" }
  | { type: "error"; message: string }
  | { type: "compile-result"; id: number; error?: string; result?: CompileResult; elapsed?: number }
  | { type: "format-result"; id: number; error?: string; formatted?: string }
  | { type: "execute-result"; id: number; error?: string; result?: ExecuteResult; elapsed?: number }
  | { type: "debug-result"; id: number; error?: string; result?: DebugResult; elapsed?: number };

export interface CompileResult {
  ok?: {
    entry_ir: IrData;
    entry_flow: unknown;
    type_registry: unknown;
    flow_registry: unknown;
  };
  errors?: CompileError[];
}

export interface ExecuteResult {
  ok?: {
    prints: string[];
    logs: LogEntry[];
    outputs: unknown;
  };
  error?: string;
  prints?: string[];
  logs?: LogEntry[];
  compile_errors?: CompileError[];
}

export interface IrData {
  nodes?: { id: string; op: string }[];
  edges?: unknown[];
  [key: string]: unknown;
}

export interface DebugResult {
  ok?: {
    snapshots: StepSnapshot[];
    prints: string[];
    logs: LogEntry[];
    outputs: unknown;
  };
  error?: string;
  prints?: string[];
  logs?: LogEntry[];
  compile_errors?: CompileError[];
}

// WASM module interface
export interface WasmModule {
  default(): Promise<void>;
  compile(files_json: string, entry_point: string): string;
  execute(files_json: string, entry_point: string): string;
  execute_stepping(files_json: string, entry_point: string): string;
  format_source(source: string): string;
}
