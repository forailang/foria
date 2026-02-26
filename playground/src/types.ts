// Shared types for the playground Worker protocol.

export interface CompileError {
  file: string;
  line: number;
  col: number;
  message: string;
}

// Messages sent from main thread to Worker
export type WorkerRequest =
  | { type: "compile"; id: number; files: Record<string, string>; entryPoint: string }
  | { type: "format"; id: number; source: string };

// Messages sent from Worker to main thread
export type WorkerResponse =
  | { type: "ready" }
  | { type: "error"; message: string }
  | { type: "compile-result"; id: number; error?: string; result?: CompileResult; elapsed?: number }
  | { type: "format-result"; id: number; error?: string; formatted?: string };

export interface CompileResult {
  ok?: {
    entry_ir: IrData;
    entry_flow: unknown;
    type_registry: unknown;
    flow_registry: unknown;
  };
  errors?: CompileError[];
}

export interface IrData {
  nodes?: { id: string; op: string }[];
  edges?: unknown[];
  [key: string]: unknown;
}

// WASM module interface
export interface WasmModule {
  default(): Promise<void>;
  compile(files_json: string, entry_point: string): string;
  format_source(source: string): string;
}
