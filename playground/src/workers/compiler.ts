// Compiler Web Worker — loads forai-playground-wasm and compiles on demand.

import type { WasmModule, WorkerRequest, WorkerResponse } from "../types";

let wasmModule: WasmModule | null = null;

function post(msg: WorkerResponse) {
  self.postMessage(msg);
}

async function initWasm() {
  try {
    const wasm = await import("./forai_playground_wasm.js") as WasmModule;
    await wasm.default();
    wasmModule = wasm;
    post({ type: "ready" });
  } catch (e: unknown) {
    post({ type: "error", message: `Failed to load WASM: ${e instanceof Error ? e.message : String(e)}` });
  }
}

self.onmessage = async (e: MessageEvent<WorkerRequest>) => {
  const msg = e.data;

  if (msg.type === "compile") {
    if (!wasmModule) {
      post({ type: "compile-result", id: msg.id, error: "WASM not loaded" });
      return;
    }
    const start = performance.now();
    try {
      const result = wasmModule.compile(JSON.stringify(msg.files), msg.entryPoint);
      const elapsed = performance.now() - start;
      const parsed = JSON.parse(result);
      post({ type: "compile-result", id: msg.id, result: parsed, elapsed });
    } catch (e: unknown) {
      post({ type: "compile-result", id: msg.id, error: e instanceof Error ? e.message : String(e) });
    }
  }

  if (msg.type === "format") {
    if (!wasmModule) {
      post({ type: "format-result", id: msg.id, error: "WASM not loaded" });
      return;
    }
    try {
      const formatted = wasmModule.format_source(msg.source);
      post({ type: "format-result", id: msg.id, formatted });
    } catch (e: unknown) {
      post({ type: "format-result", id: msg.id, error: e instanceof Error ? e.message : String(e) });
    }
  }
};

// Auto-init on load
initWasm();
