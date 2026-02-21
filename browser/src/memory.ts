/** Read a UTF-8 string from WASM linear memory. */
export function readString(
  memory: WebAssembly.Memory,
  ptr: number,
  len: number,
): string {
  const bytes = new Uint8Array(memory.buffer, ptr, len);
  return new TextDecoder().decode(bytes);
}

/** Write a UTF-8 string into WASM linear memory at `ptr`. Returns byte length written. */
export function writeString(
  memory: WebAssembly.Memory,
  ptr: number,
  str: string,
): number {
  const bytes = new TextEncoder().encode(str);
  const target = new Uint8Array(memory.buffer, ptr, bytes.length);
  target.set(bytes);
  return bytes.length;
}

/**
 * Write a JSON result into the WASM result buffer.
 * Returns positive byte length on success, or negative byte length on error
 * (matching the host_call ABI).
 */
export function writeResult(
  memory: WebAssembly.Memory,
  ptr: number,
  cap: number,
  json: string,
): number {
  const bytes = new TextEncoder().encode(json);
  if (bytes.length > cap) {
    return writeError(memory, ptr, cap, "result too large for buffer");
  }
  const target = new Uint8Array(memory.buffer, ptr, bytes.length);
  target.set(bytes);
  return bytes.length;
}

/**
 * Write an error message into the WASM result buffer.
 * Returns negative byte length (the host_call error convention).
 */
export function writeError(
  memory: WebAssembly.Memory,
  ptr: number,
  cap: number,
  message: string,
): number {
  const bytes = new TextEncoder().encode(message);
  const len = Math.min(bytes.length, cap);
  const target = new Uint8Array(memory.buffer, ptr, len);
  target.set(bytes.subarray(0, len));
  return -len;
}

/** Read a u32 (little-endian) from WASM memory. */
export function readU32(memory: WebAssembly.Memory, ptr: number): number {
  const view = new DataView(memory.buffer);
  return view.getUint32(ptr, true);
}

/** Write a u32 (little-endian) into WASM memory. */
export function writeU32(
  memory: WebAssembly.Memory,
  ptr: number,
  value: number,
): void {
  const view = new DataView(memory.buffer);
  view.setUint32(ptr, value, true);
}

/** Write a u64 (little-endian) into WASM memory as two u32s. */
export function writeU64(
  memory: WebAssembly.Memory,
  ptr: number,
  value: bigint,
): void {
  const view = new DataView(memory.buffer);
  view.setBigUint64(ptr, value, true);
}
