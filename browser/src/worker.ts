/**
 * Web Worker entry point for running forai WASM.
 *
 * The WASM module runs synchronously in this worker thread.
 * When it hits an async op (http.get, time.sleep, etc.), the worker
 * blocks via Atomics.wait while the main thread performs the async work.
 * Sync ops (log.*, term.print) are handled locally via postMessage.
 */

import { createWasiImports, ProcessExit } from "./wasi.js";
import { readString, writeResult, writeError } from "./memory.js";
import {
  FLAG_IDLE, FLAG_REQUEST, FLAG_RESPONSE_OK,
  HEADER_SIZE, writeRequest, readResponse,
} from "./protocol.js";

declare const self: DedicatedWorkerGlobalScope;

interface InitMessage {
  type: "init";
  wasmBytes: ArrayBuffer;
  sab: SharedArrayBuffer;
  stdinData: ArrayBuffer;
}

// Worker-local ops: dispatch inline, no SAB round-trip needed
const WORKER_LOCAL_OPS = new Set([
  "log.debug", "log.info", "log.warn", "log.error", "log.trace",
  "term.print", "term.clear", "term.size", "term.cursor",
]);

// Ops unavailable in the browser — return error immediately
const UNAVAILABLE_PREFIXES = [
  "file.", "exec.", "http.server.", "db.", "env.",
];

function isUnavailable(op: string): string | null {
  const messages: Record<string, string> = {
    "file.": "file I/O is not available in the browser",
    "exec.": "process execution is not available in the browser",
    "http.server.": "HTTP server is not available in the browser",
    "db.": "SQLite database is not available in the browser",
    "env.": "environment variables are not available in the browser",
  };
  for (const prefix of UNAVAILABLE_PREFIXES) {
    if (op.startsWith(prefix)) {
      return `${op}: ${messages[prefix]}`;
    }
  }
  return null;
}

function handleWorkerLocalOp(op: string, args: unknown[]): unknown {
  // Log ops → postMessage to main thread for console output
  if (op.startsWith("log.")) {
    const message = args[0] ?? "";
    const context = args[1];
    self.postMessage({ type: "log", level: op.slice(4), message, context });
    return true;
  }

  switch (op) {
    case "term.print": {
      const text = String(args[0] ?? "");
      self.postMessage({ type: "print", text });
      return true;
    }
    case "term.clear":
      self.postMessage({ type: "clear" });
      return true;
    case "term.size":
      return { cols: 80, rows: 24 };
    case "term.cursor":
      return { col: 0, row: 0 };
    default:
      throw new Error(`unhandled worker-local op: ${op}`);
  }
}

/**
 * Perform a SAB round-trip: write request, notify main thread, block until response.
 */
function sabRoundTrip(sab: SharedArrayBuffer, op: string, argsJson: string): { ok: boolean; data: string } {
  const flagArray = new Int32Array(sab, 0, 1);

  // Write request into SAB
  writeRequest(sab, op, argsJson);

  // Set flag to REQUEST and notify main thread
  Atomics.store(flagArray, 0, FLAG_REQUEST);
  Atomics.notify(flagArray, 0);

  // Block until main thread writes response (flag changes from REQUEST)
  Atomics.wait(flagArray, 0, FLAG_REQUEST);

  // Read response
  const response = readResponse(sab);

  // Reset flag to IDLE for next round-trip
  Atomics.store(flagArray, 0, FLAG_IDLE);

  return response;
}

self.onmessage = (event: MessageEvent) => {
  const msg = event.data as InitMessage;
  if (msg.type !== "init") return;

  const { wasmBytes, sab, stdinData } = msg;

  let memory: WebAssembly.Memory;

  // Create WASI imports with postMessage-based I/O
  const wasiImports = createWasiImports(() => memory, {
    stdinData,
    onStdout: (text: string) => self.postMessage({ type: "stdout", text }),
    onStderr: (text: string) => self.postMessage({ type: "stderr", text }),
  });

  // Create host_call that routes between worker-local and SAB ops
  function host_call(
    opPtr: number, opLen: number,
    argsPtr: number, argsLen: number,
    resultPtr: number, resultCap: number,
  ): number {
    let op: string;
    let args: unknown[];
    try {
      op = readString(memory, opPtr, opLen);
      const argsStr = readString(memory, argsPtr, argsLen);
      args = JSON.parse(argsStr);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      return writeError(memory, resultPtr, resultCap, `host_call parse error: ${msg}`);
    }

    // Check for unavailable ops first
    const unavailMsg = isUnavailable(op);
    if (unavailMsg) {
      return writeError(memory, resultPtr, resultCap, unavailMsg);
    }

    // Worker-local ops: handle inline
    if (WORKER_LOCAL_OPS.has(op)) {
      try {
        const result = handleWorkerLocalOp(op, args);
        const json = JSON.stringify(result);
        return writeResult(memory, resultPtr, resultCap, json);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        return writeError(memory, resultPtr, resultCap, msg);
      }
    }

    // SAB round-trip for async ops (http.*, time.*, ws.*, term.prompt)
    const argsJson = JSON.stringify(args);
    const response = sabRoundTrip(sab, op, argsJson);

    if (response.ok) {
      return writeResult(memory, resultPtr, resultCap, response.data);
    } else {
      return writeError(memory, resultPtr, resultCap, response.data);
    }
  }

  // Instantiate WASM synchronously (we're in a worker, sync is fine)
  const importObject: WebAssembly.Imports = {
    wasi_snapshot_preview1: wasiImports,
    env: { host_call },
  };

  try {
    const module = new WebAssembly.Module(wasmBytes);
    const instance = new WebAssembly.Instance(module, importObject);
    memory = instance.exports.memory as WebAssembly.Memory;

    if (!memory) {
      self.postMessage({ type: "done", success: false, error: "WASM module does not export memory" });
      return;
    }

    // Run _start — this is a plain synchronous call, no Asyncify needed.
    // The worker blocks on Atomics.wait when async ops are needed.
    const start = instance.exports._start as () => void;
    start();

    self.postMessage({ type: "done", success: true });
  } catch (e) {
    if (e instanceof ProcessExit) {
      self.postMessage({ type: "exit", code: e.code });
    } else {
      const error = e instanceof Error ? e.message : String(e);
      self.postMessage({ type: "done", success: false, error });
    }
  }
};
