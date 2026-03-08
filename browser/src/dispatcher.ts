/**
 * Main thread async dispatcher.
 *
 * Watches the SharedArrayBuffer for requests from the worker,
 * dispatches to the appropriate async handler (fetch, setTimeout, etc.),
 * and writes the response back for the worker to read.
 */

import {
  FLAG_IDLE, FLAG_REQUEST,
  readRequest, writeResponse,
} from "./protocol.js";
import { handleHttpOp } from "./ops/http.js";
import { handleTimeOp } from "./ops/time.js";
import { handleWsOp } from "./ops/ws.js";
import { handleTermOp, type TermCallbacks } from "./ops/term.js";

export interface DispatcherOptions extends TermCallbacks {
  onUiEvent?: () => unknown | Promise<unknown>;
}

export function createDispatcher(sab: SharedArrayBuffer, opts: DispatcherOptions = {}) {
  let running = false;

  // Inject options into term handler for prompt support
  (handleTermOp as unknown as { _opts: TermCallbacks })._opts = opts;

  async function dispatchLoop(): Promise<void> {
    const flagArray = new Int32Array(sab, 0, 1);
    running = true;

    while (running) {
      // Wait for the worker to set flag to REQUEST
      const currentFlag = Atomics.load(flagArray, 0);
      if (currentFlag === FLAG_IDLE) {
        // Use Atomics.waitAsync to avoid blocking the main thread
        const result = Atomics.waitAsync(flagArray, 0, FLAG_IDLE);
        if (result.async) {
          const waitResult = await result.value;
          if (waitResult === "timed-out") continue;
        }
        // Re-check after waking
        if (!running) break;
        if (Atomics.load(flagArray, 0) !== FLAG_REQUEST) continue;
      } else if (currentFlag !== FLAG_REQUEST) {
        // Unexpected state — spin briefly and retry
        await new Promise(r => setTimeout(r, 1));
        continue;
      }

      // Read the request
      const { op, argsJson } = readRequest(sab);
      let args: unknown[];
      try {
        args = JSON.parse(argsJson);
      } catch {
        writeResponse(sab, `failed to parse args for ${op}`, true);
        Atomics.notify(flagArray, 0);
        continue;
      }

      // Dispatch to the appropriate handler
      try {
        const result = await dispatchOp(op, args, opts);
        const json = JSON.stringify(result);
        writeResponse(sab, json, false);
      } catch (e) {
        const msg = e instanceof Error ? e.message : String(e);
        writeResponse(sab, msg, true);
      }

      // Notify worker that response is ready
      Atomics.notify(flagArray, 0);
    }
  }

  return {
    start(): Promise<void> {
      return dispatchLoop();
    },
    stop(): void {
      running = false;
      // Wake the dispatcher if it's waiting
      const flagArray = new Int32Array(sab, 0, 1);
      Atomics.notify(flagArray, 0);
    },
  };
}

async function dispatchOp(op: string, args: unknown[], opts: DispatcherOptions): Promise<unknown> {
  if (op.startsWith("http.") && !op.startsWith("http.server.")) {
    return handleHttpOp(op, args);
  }
  if (op.startsWith("time.")) {
    return handleTimeOp(op, args);
  }
  if (op.startsWith("ws.")) {
    return handleWsOp(op, args);
  }
  if (op === "term.prompt") {
    return handleTermOp(op, args);
  }
  if (op === "ui.events") {
    if (!opts.onUiEvent) {
      throw new Error("ui.events: no browser UI event source configured");
    }
    return opts.onUiEvent();
  }
  if (op === "ui.current_path") {
    if (typeof window === "undefined") return "/";
    return window.location.pathname || "/";
  }
  const browserUnsupported: Record<string, string> = {
    "ui.render": "use ui.mount/ui.update in browser apps",
    "term.read_key": "use ui.events() for browser input",
    "term.move_to": "terminal cursor control is not available in browser mode",
    "term.color": "terminal color control is not available in browser mode",
  };
  if (browserUnsupported[op]) {
    throw new Error(`browser mode: \`${op}\` is not supported (${browserUnsupported[op]})`);
  }
  if (
    op.startsWith("file.") ||
    op.startsWith("exec.") ||
    op.startsWith("db.") ||
    op.startsWith("http.server.") ||
    op.startsWith("env.") ||
    op.startsWith("ffi.")
  ) {
    throw new Error(`browser mode: \`${op}\` is not supported in this target`);
  }
  throw new Error(`browser mode: unsupported op \`${op}\``);
}
