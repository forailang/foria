// Compiler Web Worker — loads forai-playground-wasm and compiles on demand.

let wasmModule: any = null;

async function initWasm() {
  try {
    const wasm = await import("./forai_playground_wasm.js");
    await wasm.default();
    wasmModule = wasm;
    self.postMessage({ type: "ready" });
  } catch (e: any) {
    self.postMessage({ type: "error", message: `Failed to load WASM: ${e.message}` });
  }
}

self.onmessage = async (e: MessageEvent) => {
  const { type, id, files, entryPoint, source } = e.data;

  if (type === "compile") {
    if (!wasmModule) {
      self.postMessage({ type: "compile-result", id, error: "WASM not loaded" });
      return;
    }
    const start = performance.now();
    try {
      const result = wasmModule.compile(JSON.stringify(files), entryPoint);
      const elapsed = performance.now() - start;
      const parsed = JSON.parse(result);
      self.postMessage({ type: "compile-result", id, result: parsed, elapsed });
    } catch (e: any) {
      self.postMessage({ type: "compile-result", id, error: e.message });
    }
  }

  if (type === "format") {
    if (!wasmModule) {
      self.postMessage({ type: "format-result", id, error: "WASM not loaded" });
      return;
    }
    try {
      const formatted = wasmModule.format_source(source);
      self.postMessage({ type: "format-result", id, formatted });
    } catch (e: any) {
      self.postMessage({ type: "format-result", id, error: e.message });
    }
  }

  if (type === "tokenize") {
    if (!wasmModule) {
      self.postMessage({ type: "tokenize-result", id, error: "WASM not loaded" });
      return;
    }
    try {
      const result = wasmModule.tokenize(source);
      self.postMessage({ type: "tokenize-result", id, result: JSON.parse(result) });
    } catch (e: any) {
      self.postMessage({ type: "tokenize-result", id, error: e.message });
    }
  }
};

// Auto-init on load
initWasm();
