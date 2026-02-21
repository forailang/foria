# Asyncify → Worker + SharedArrayBuffer Migration

## Status: RESOLVED

Async ops (time.sleep, http.get, ws.*, etc.) now work in the browser WASM target using a **Web Worker + SharedArrayBuffer** architecture. The Asyncify approach has been removed entirely.

## Previous Problem

Asyncify (Binaryen's wasm-opt --asyncify) transforms a WASM module so its stack can be saved/restored across async JS calls. This worked for **unwinding** but the **rewind** path corrupted Rust heap state:

- Asyncify saves/restores the **call stack** but NOT the **heap**
- The forai WASM runtime has many heap allocations (HashMap, Vec, String, serde_json::Value)
- During rewind, restored stack pointers referenced stale/inconsistent heap data → panic

## Solution: Worker + SharedArrayBuffer

Instead of transforming the WASM binary, we run it unmodified in a Web Worker:

```
MAIN THREAD                           WORKER THREAD
-----------                           -------------
run(opts)
  create SharedArrayBuffer
  create Worker                 -->   onmessage: init
  start dispatch loop                 instantiate WASM (plain, no Asyncify)
                                      call _start()
                                        |
                                      host_call("http.get", [...])
                                        write request to SAB
                                        Atomics.notify + Atomics.wait  <-- BLOCKS
  Atomics.waitAsync wakes               |
  read request from SAB                 |
  await fetch(...)                      |
  write result to SAB                   |
  Atomics.notify                  -->   wakes, reads result
                                        returns to WASM
                                        |
  onmessage: {type:"stdout"}    <--   fd_write via postMessage
  onmessage: {type:"done"}     <--   _start() returns
```

### Op routing

- **Worker-local** (sync, no SAB): `log.*`, `term.print`, `term.clear`, `term.size`, `term.cursor`
- **SAB round-trip** (async, blocks worker): `http.*`, `time.sleep`, `ws.*`, `term.prompt`
- **Worker-local error** (unavailable): `file.*`, `exec.*`, `http.server.*`, `db.*`, `env.*`

### Requirements

- `SharedArrayBuffer` requires cross-origin isolation: serve with `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` headers
- The generated `index.html` checks `crossOriginIsolated` and shows a helpful error if headers are missing

### Benefits over Asyncify

- No `wasm-opt` dependency or Asyncify transform step
- Smaller WASM binary (no Asyncify instrumentation)
- No heap corruption — WASM runs as plain synchronous code
- Works with any Rust allocator and complex heap state
