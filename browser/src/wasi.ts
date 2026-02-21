import { readU32, writeU32, writeU64 } from "./memory.js";

export class ProcessExit extends Error {
  code: number;
  constructor(code: number) {
    super(`process exited with code ${code}`);
    this.code = code;
  }
}

export interface WasiOptions {
  stdinData?: ArrayBuffer;
  onStdout?: (text: string) => void;
  onStderr?: (text: string) => void;
  args?: string[];
}

/**
 * Create WASI preview 1 import stubs for the browser.
 * Ported from crates/forai/src/wasm_runner.rs lines 242-567.
 */
export function createWasiImports(
  getMemory: () => WebAssembly.Memory,
  opts: WasiOptions = {},
): Record<string, WebAssembly.ImportValue> {
  const onStdout = opts.onStdout ?? ((text: string) => console.log(text));
  const onStderr = opts.onStderr ?? ((text: string) => console.error(text));
  const args = opts.args ?? ["forai"];
  const decoder = new TextDecoder();

  // stdin state
  const stdinBuf = opts.stdinData
    ? new Uint8Array(opts.stdinData)
    : new Uint8Array(0);
  let stdinPos = 0;

  return {
    fd_write(
      fd: number,
      iovsPtr: number,
      iovsLen: number,
      nwrittenPtr: number,
    ): number {
      const memory = getMemory();
      const data = new Uint8Array(memory.buffer);
      let total = 0;

      for (let i = 0; i < iovsLen; i++) {
        const iovOffset = iovsPtr + i * 8;
        if (iovOffset + 8 > data.length) return 21; // EINVAL
        const bufPtr = readU32(memory, iovOffset);
        const bufLen = readU32(memory, iovOffset + 4);

        const start = bufPtr;
        const end = start + bufLen;
        if (end > data.length) return 21;

        const bytes = data.subarray(start, end);
        const text = decoder.decode(bytes);

        if (fd === 1) {
          onStdout(text);
        } else if (fd === 2) {
          onStderr(text);
        }
        total += bufLen;
      }

      writeU32(memory, nwrittenPtr, total);
      return 0;
    },

    fd_read(
      fd: number,
      iovsPtr: number,
      iovsLen: number,
      nreadPtr: number,
    ): number {
      if (fd !== 0) return 8; // EBADF for non-stdin

      const memory = getMemory();
      const data = new Uint8Array(memory.buffer);
      let totalRead = 0;

      for (let i = 0; i < iovsLen; i++) {
        const iovOffset = iovsPtr + i * 8;
        if (iovOffset + 8 > data.length) return 21;
        const bufPtr = readU32(memory, iovOffset);
        const bufLen = readU32(memory, iovOffset + 4);

        const remaining = stdinBuf.length - stdinPos;
        const toCopy = Math.min(bufLen, remaining);
        if (toCopy > 0) {
          data.set(stdinBuf.subarray(stdinPos, stdinPos + toCopy), bufPtr);
          stdinPos += toCopy;
          totalRead += toCopy;
        }
      }

      writeU32(memory, nreadPtr, totalRead);
      return 0;
    },

    fd_close(_fd: number): number {
      return 8; // EBADF
    },

    fd_prestat_get(_fd: number, _prestatPtr: number): number {
      return 8; // EBADF
    },

    fd_prestat_dir_name(
      _fd: number,
      _pathPtr: number,
      _pathLen: number,
    ): number {
      return 8; // EBADF
    },

    fd_seek(
      _fd: number,
      _offset: bigint,
      _whence: number,
      _newOffsetPtr: number,
    ): number {
      return 8; // EBADF
    },

    environ_sizes_get(countPtr: number, sizePtr: number): number {
      const memory = getMemory();
      writeU32(memory, countPtr, 0);
      writeU32(memory, sizePtr, 0);
      return 0;
    },

    environ_get(_environPtr: number, _bufPtr: number): number {
      return 0;
    },

    args_sizes_get(argcPtr: number, argvBufSizePtr: number): number {
      const memory = getMemory();
      writeU32(memory, argcPtr, args.length);
      const bufSize = args.reduce((sum, a) => sum + new TextEncoder().encode(a).length + 1, 0);
      writeU32(memory, argvBufSizePtr, bufSize);
      return 0;
    },

    args_get(argvPtr: number, argvBufPtr: number): number {
      const memory = getMemory();
      const data = new Uint8Array(memory.buffer);
      let bufOffset = argvBufPtr;

      for (let i = 0; i < args.length; i++) {
        // Write pointer to this arg
        writeU32(memory, argvPtr + i * 4, bufOffset);
        // Write null-terminated arg string
        const argBytes = new TextEncoder().encode(args[i]);
        data.set(argBytes, bufOffset);
        data[bufOffset + argBytes.length] = 0;
        bufOffset += argBytes.length + 1;
      }
      return 0;
    },

    clock_time_get(
      _clockId: number,
      _precision: bigint,
      timePtr: number,
    ): number {
      const memory = getMemory();
      const nowMs = performance.now();
      const ns = BigInt(Math.floor(nowMs * 1_000_000));
      writeU64(memory, timePtr, ns);
      return 0;
    },

    random_get(bufPtr: number, bufLen: number): number {
      const memory = getMemory();
      const data = new Uint8Array(memory.buffer, bufPtr, bufLen);
      crypto.getRandomValues(data);
      return 0;
    },

    proc_exit(code: number): void {
      throw new ProcessExit(code);
    },
  };
}
