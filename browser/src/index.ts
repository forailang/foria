export { ProcessExit } from "./wasi.js";
export { SAB_SIZE } from "./protocol.js";

import { SAB_SIZE } from "./protocol.js";
import { createDispatcher, type DispatcherOptions } from "./dispatcher.js";
import { ProcessExit } from "./wasi.js";
// @ts-ignore — generated at build time by build.mjs
import { workerCode } from "../dist/worker-inline.js";

export interface RunOptions extends DispatcherOptions {
  wasmUrl?: string;
  wasmBytes?: ArrayBuffer;
  onPrint?: (text: string) => void;
  onStdout?: (text: string) => void;
  onStderr?: (text: string) => void;
}

/**
 * Extract a named custom section from a WASM binary.
 * Custom sections have id=0, followed by a LEB128 name length, name, then data.
 */
export function extractCustomSection(
  wasm: Uint8Array,
  sectionName: string,
): Uint8Array | null {
  let pos = 8; // skip magic + version

  while (pos < wasm.length) {
    const sectionId = wasm[pos];
    pos += 1;

    const [sectionLen, bytesRead] = readLeb128(wasm, pos);
    pos += bytesRead;
    const sectionEnd = pos + sectionLen;

    if (sectionId === 0) {
      const [nameLen, nameBytes] = readLeb128(wasm, pos);
      pos += nameBytes;

      if (pos + nameLen <= wasm.length) {
        const name = new TextDecoder().decode(wasm.subarray(pos, pos + nameLen));
        if (name === sectionName) {
          const dataStart = pos + nameLen;
          return wasm.subarray(dataStart, sectionEnd);
        }
      }
      pos = sectionEnd;
    } else {
      pos = sectionEnd;
    }
  }

  return null;
}

function readLeb128(data: Uint8Array, offset: number): [number, number] {
  let result = 0;
  let shift = 0;
  let bytesRead = 0;
  while (offset + bytesRead < data.length) {
    const byte = data[offset + bytesRead];
    bytesRead += 1;
    result |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) break;
    shift += 7;
  }
  return [result, bytesRead];
}

/**
 * Load and run a forai WASM application in the browser.
 *
 * Uses a Web Worker + SharedArrayBuffer architecture:
 * - WASM runs in a Worker thread (no Asyncify transform needed)
 * - Async ops block the Worker via Atomics.wait
 * - Main thread dispatches async work (fetch, setTimeout, etc.)
 * - Results are written back to the SharedArrayBuffer
 *
 * Requires cross-origin isolation (COOP + COEP headers).
 */
export async function run(opts: RunOptions): Promise<void> {
  // Check for cross-origin isolation (required for SharedArrayBuffer)
  if (typeof crossOriginIsolated !== "undefined" && !crossOriginIsolated) {
    throw new Error(
      "SharedArrayBuffer requires cross-origin isolation. " +
      "Serve with headers: Cross-Origin-Opener-Policy: same-origin, " +
      "Cross-Origin-Embedder-Policy: require-corp"
    );
  }

  // Fetch WASM bytes
  let wasmBytes: ArrayBuffer;
  if (opts.wasmBytes) {
    wasmBytes = opts.wasmBytes;
  } else if (opts.wasmUrl) {
    const resp = await fetch(opts.wasmUrl);
    if (!resp.ok) {
      throw new Error(`failed to fetch WASM: ${resp.status} ${resp.statusText}`);
    }
    wasmBytes = await resp.arrayBuffer();
  } else {
    throw new Error("either wasmUrl or wasmBytes must be provided");
  }

  // Extract the embedded program bundle from the custom section
  const wasmData = new Uint8Array(wasmBytes);
  const bundleData = extractCustomSection(wasmData, "forai_program");
  if (!bundleData) {
    throw new Error("WASM module has no embedded forai_program section");
  }

  // The program bundle is passed to the WASM module via stdin
  const stdinData = new ArrayBuffer(bundleData.byteLength);
  new Uint8Array(stdinData).set(bundleData);

  // Create SharedArrayBuffer for Worker ↔ Main thread communication
  const sab = new SharedArrayBuffer(SAB_SIZE);

  // Create Worker from inline code (embedded at build time)
  const blob = new Blob([workerCode], { type: "application/javascript" });
  const workerUrl = URL.createObjectURL(blob);
  const worker = new Worker(workerUrl);

  // Set up the main thread dispatcher for async ops
  const dispatcher = createDispatcher(sab, {
    onPrint: opts.onPrint,
    onPrompt: opts.onPrompt,
  });

  // Start the dispatch loop (runs in background)
  const dispatchPromise = dispatcher.start();

  return new Promise<void>((resolve, reject) => {
    const onPrint = opts.onPrint ?? console.log;
    const onStdout = opts.onStdout ?? ((text: string) => console.log(text));
    const onStderr = opts.onStderr ?? ((text: string) => console.error(text));

    worker.onmessage = (event: MessageEvent) => {
      const msg = event.data;
      switch (msg.type) {
        case "stdout":
          onStdout(msg.text);
          break;
        case "stderr":
          onStderr(msg.text);
          break;
        case "print":
          onPrint(msg.text);
          break;
        case "clear":
          if (typeof console !== "undefined" && console.clear) console.clear();
          break;
        case "log": {
          const level = msg.level as string;
          const args = msg.context !== undefined ? [msg.message, msg.context] : [msg.message];
          switch (level) {
            case "debug": console.debug(...args); break;
            case "info": console.log(...args); break;
            case "warn": console.warn(...args); break;
            case "error": console.error(...args); break;
            case "trace": console.trace(...args); break;
            default: console.log(...args); break;
          }
          break;
        }
        case "done":
          cleanup();
          if (msg.success) {
            resolve();
          } else {
            reject(new Error(msg.error ?? "WASM execution failed"));
          }
          break;
        case "exit":
          cleanup();
          if (msg.code === 0) {
            resolve();
          } else {
            reject(new ProcessExit(msg.code));
          }
          break;
      }
    };

    worker.onerror = (event: ErrorEvent) => {
      cleanup();
      reject(new Error(`Worker error: ${event.message}`));
    };

    function cleanup() {
      dispatcher.stop();
      worker.terminate();
      URL.revokeObjectURL(workerUrl);
    }

    // Send init message to worker with WASM bytes and SAB
    worker.postMessage(
      { type: "init", wasmBytes, sab, stdinData },
      [wasmBytes, stdinData], // transfer ownership
    );
  });
}

/**
 * Create a reusable app handle (load once, run multiple times).
 * Note: each run() creates a fresh Worker, so state is not shared.
 */
export async function createApp(opts: RunOptions) {
  return {
    run: () => run(opts),
  };
}
