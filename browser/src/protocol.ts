/**
 * SharedArrayBuffer protocol for Worker ↔ Main thread communication.
 *
 * Layout (4MB total):
 *   Offset 0:  Int32 flag (0=idle, 1=request-ready, 2=response-ok, 3=response-error)
 *   Offset 4:  Int32 dataLen (total data bytes)
 *   Offset 8:  Int32 opLen (op string length, request only)
 *   Offset 12+: data region (op + \0 + JSON args for request, JSON result for response)
 */

export const FLAG_IDLE = 0;
export const FLAG_REQUEST = 1;
export const FLAG_RESPONSE_OK = 2;
export const FLAG_RESPONSE_ERR = 3;

export const HEADER_SIZE = 12;
export const SAB_SIZE = 4 * 1024 * 1024; // 4MB

const encoder = new TextEncoder();
const decoder = new TextDecoder();

function decodeSharedBytes(data: Uint8Array, start: number, len: number): string {
  if (len <= 0) return "";
  // TextDecoder rejects views backed by SharedArrayBuffer in some browsers.
  // Copy into a regular Uint8Array before decoding.
  const copy = new Uint8Array(len);
  copy.set(data.subarray(start, start + len));
  return decoder.decode(copy);
}

/**
 * Write a request into the SAB (worker side).
 * Format: op string + \0 + JSON args in the data region.
 */
export function writeRequest(sab: SharedArrayBuffer, op: string, argsJson: string): void {
  const view = new DataView(sab);
  const data = new Uint8Array(sab);

  const opBytes = encoder.encode(op);
  const argsBytes = encoder.encode(argsJson);
  const totalLen = opBytes.length + 1 + argsBytes.length; // op + \0 + args

  view.setInt32(8, opBytes.length, true); // opLen
  data.set(opBytes, HEADER_SIZE);
  data[HEADER_SIZE + opBytes.length] = 0; // null separator
  data.set(argsBytes, HEADER_SIZE + opBytes.length + 1);
  view.setInt32(4, totalLen, true); // dataLen
}

/**
 * Read a request from the SAB (main thread side).
 */
export function readRequest(sab: SharedArrayBuffer): { op: string; argsJson: string } {
  const view = new DataView(sab);
  const data = new Uint8Array(sab);

  const opLen = view.getInt32(8, true);
  const dataLen = view.getInt32(4, true);

  const op = decodeSharedBytes(data, HEADER_SIZE, opLen);
  const argsStart = HEADER_SIZE + opLen + 1; // skip \0
  const argsLen = dataLen - opLen - 1;
  const argsJson = decodeSharedBytes(data, argsStart, argsLen);

  return { op, argsJson };
}

/**
 * Write a response into the SAB (main thread side).
 */
export function writeResponse(sab: SharedArrayBuffer, json: string, isError: boolean): void {
  const view = new DataView(sab);
  const data = new Uint8Array(sab);

  const bytes = encoder.encode(json);
  data.set(bytes, HEADER_SIZE);
  view.setInt32(4, bytes.length, true); // dataLen
  view.setInt32(0, isError ? FLAG_RESPONSE_ERR : FLAG_RESPONSE_OK, true); // flag
}

/**
 * Read a response from the SAB (worker side).
 */
export function readResponse(sab: SharedArrayBuffer): { ok: boolean; data: string } {
  const view = new DataView(sab);
  const sabData = new Uint8Array(sab);

  const flag = view.getInt32(0, true);
  const dataLen = view.getInt32(4, true);
  const text = decodeSharedBytes(sabData, HEADER_SIZE, dataLen);

  return { ok: flag === FLAG_RESPONSE_OK, data: text };
}
