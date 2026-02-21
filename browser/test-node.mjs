// Node.js smoke test for the browser runtime.
// This exercises the same code paths as the browser, minus Asyncify
// (which requires wasm-opt instrumented WASM + browser WebAssembly API).
//
// What we test here:
// 1. Custom section extraction from the WASM binary
// 2. WASI stubs (fd_write, fd_read, clock_time_get, etc.)
// 3. host_call dispatch (log.*, term.print)
// 4. Full WASM instantiation and execution

import { readFileSync } from "fs";
import { extractCustomSection } from "./dist/dataflow-browser.js";

const wasmPath = "../examples/browser-demo/dist/browser-demo.wasm";
const wasmBytes = readFileSync(wasmPath);

// Test 1: Extract custom section
console.log("test 1: extract custom section...");
const section = extractCustomSection(new Uint8Array(wasmBytes), "dataflow_program");
if (!section) {
  console.error("FAIL: no dataflow_program section found");
  process.exit(1);
}
const bundleJson = new TextDecoder().decode(section);
const bundle = JSON.parse(bundleJson);
console.log(`  found program bundle: entry_flow="${bundle.entry_flow.name}", ` +
  `flows=${Object.keys(bundle.flow_registry).length + 1}`);
console.log("  PASS");

// Test 2: Instantiate the non-asyncified WASM with our WASI stubs + host_call
// Use the original (non-asyncified) WASM since Node doesn't have Asyncify support
// but we can test that host_call gets invoked correctly
console.log("\ntest 2: WASI stubs + host_call integration...");

const prints = [];
const logs = [];

// Build minimal WASI imports
function readU32(mem, ptr) {
  return new DataView(mem.buffer).getUint32(ptr, true);
}
function writeU32(mem, ptr, val) {
  new DataView(mem.buffer).setUint32(ptr, val, true);
}

const stdinBuf = new Uint8Array(section);
let stdinPos = 0;

const wasi = {
  fd_write(fd, iovsPtr, iovsLen, nwrittenPtr) {
    const mem = instance.exports.memory;
    const data = new Uint8Array(mem.buffer);
    let total = 0;
    for (let i = 0; i < iovsLen; i++) {
      const off = iovsPtr + i * 8;
      const bufPtr = readU32(mem, off);
      const bufLen = readU32(mem, off + 4);
      const text = new TextDecoder().decode(data.subarray(bufPtr, bufPtr + bufLen));
      if (fd === 1) prints.push(text);
      total += bufLen;
    }
    writeU32(mem, nwrittenPtr, total);
    return 0;
  },
  fd_read(fd, iovsPtr, iovsLen, nreadPtr) {
    if (fd !== 0) return 8;
    const mem = instance.exports.memory;
    const data = new Uint8Array(mem.buffer);
    let totalRead = 0;
    for (let i = 0; i < iovsLen; i++) {
      const off = iovsPtr + i * 8;
      const bufPtr = readU32(mem, off);
      const bufLen = readU32(mem, off + 4);
      const remaining = stdinBuf.length - stdinPos;
      const toCopy = Math.min(bufLen, remaining);
      if (toCopy > 0) {
        data.set(stdinBuf.subarray(stdinPos, stdinPos + toCopy), bufPtr);
        stdinPos += toCopy;
        totalRead += toCopy;
      }
    }
    writeU32(mem, nreadPtr, totalRead);
    return 0;
  },
  fd_close() { return 8; },
  fd_prestat_get() { return 8; },
  fd_prestat_dir_name() { return 8; },
  fd_seek() { return 8; },
  environ_sizes_get(countPtr, sizePtr) {
    const mem = instance.exports.memory;
    writeU32(mem, countPtr, 0);
    writeU32(mem, sizePtr, 0);
    return 0;
  },
  environ_get() { return 0; },
  args_sizes_get(argcPtr, bufSizePtr) {
    const mem = instance.exports.memory;
    writeU32(mem, argcPtr, 1);
    writeU32(mem, bufSizePtr, 9); // "dataflow\0"
    return 0;
  },
  args_get(argvPtr, bufPtr) {
    const mem = instance.exports.memory;
    writeU32(mem, argvPtr, bufPtr);
    const data = new Uint8Array(mem.buffer);
    const arg = new TextEncoder().encode("dataflow");
    data.set(arg, bufPtr);
    data[bufPtr + arg.length] = 0;
    return 0;
  },
  clock_time_get(_clockId, _precision, timePtr) {
    const mem = instance.exports.memory;
    const ns = BigInt(Date.now()) * 1000000n;
    new DataView(mem.buffer).setBigUint64(timePtr, ns, true);
    return 0;
  },
  random_get(bufPtr, bufLen) {
    const mem = instance.exports.memory;
    const data = new Uint8Array(mem.buffer, bufPtr, bufLen);
    for (let i = 0; i < bufLen; i++) data[i] = Math.floor(Math.random() * 256);
    return 0;
  },
  proc_exit(code) {
    throw new Error(`proc_exit(${code})`);
  },
};

// host_call implementation for Node test
function host_call(opPtr, opLen, argsPtr, argsLen, resultPtr, resultCap) {
  const mem = instance.exports.memory;
  const data = new Uint8Array(mem.buffer);

  const op = new TextDecoder().decode(data.subarray(opPtr, opPtr + opLen));
  const argsStr = new TextDecoder().decode(data.subarray(argsPtr, argsPtr + argsLen));
  const args = JSON.parse(argsStr);

  let result;
  if (op === "log.info" || op === "log.debug" || op === "log.warn" || op === "log.error") {
    logs.push({ op, message: args[0] });
    result = true;
  } else if (op === "term.print") {
    prints.push(String(args[0]) + "\n");
    result = true;
  } else {
    // Unknown op — return error
    const errMsg = `unsupported op in test: ${op}`;
    const errBytes = new TextEncoder().encode(errMsg);
    const len = Math.min(errBytes.length, resultCap);
    data.set(errBytes.subarray(0, len), resultPtr);
    return -len;
  }

  const json = JSON.stringify(result);
  const bytes = new TextEncoder().encode(json);
  if (bytes.length > resultCap) {
    const errMsg = "result too large";
    const errBytes = new TextEncoder().encode(errMsg);
    data.set(errBytes.subarray(0, Math.min(errBytes.length, resultCap)), resultPtr);
    return -Math.min(errBytes.length, resultCap);
  }
  data.set(bytes, resultPtr);
  return bytes.length;
}

// Use the NON-asyncified WASM (the original one) for Node testing
const origWasmPath = "../examples/browser-demo/dist/browser-demo.wasm";
const origWasmBytes = readFileSync(origWasmPath);

let instance;
try {
  const mod = await WebAssembly.compile(origWasmBytes);
  const inst = await WebAssembly.instantiate(mod, {
    wasi_snapshot_preview1: wasi,
    env: { host_call },
  });
  instance = inst;

  // Run _start
  inst.exports._start();
  console.log("  process returned normally");
} catch (e) {
  if (e.message === "proc_exit(0)") {
    console.log("  process exited cleanly (code 0)");
  } else {
    console.error(`  FAIL: ${e.message}`);
    process.exit(1);
  }
}

// Check captured output
const allOutput = prints.join("");
console.log(`  stdout output: ${JSON.stringify(allOutput.trim())}`);
console.log(`  log calls: ${logs.map(l => `${l.op}("${l.message}")`).join(", ")}`);

if (!allOutput.includes("Hello from dataflow in the browser!")) {
  console.error("  FAIL: missing greeting in output");
  process.exit(1);
}
if (!allOutput.includes("The answer is: 42")) {
  console.error("  FAIL: missing arithmetic result in output");
  process.exit(1);
}
if (logs.length < 2) {
  console.error("  FAIL: expected at least 2 log calls");
  process.exit(1);
}

console.log("  PASS");
console.log("\nall tests passed!");
