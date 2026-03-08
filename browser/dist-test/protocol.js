const FLAG_IDLE = 0;
const FLAG_REQUEST = 1;
const FLAG_RESPONSE_OK = 2;
const FLAG_RESPONSE_ERR = 3;
const HEADER_SIZE = 12;
const SAB_SIZE = 4 * 1024 * 1024;
const encoder = new TextEncoder();
const decoder = new TextDecoder();
function decodeSharedBytes(data, start, len) {
  if (len <= 0)
    return "";
  const copy = new Uint8Array(len);
  copy.set(data.subarray(start, start + len));
  return decoder.decode(copy);
}
function writeRequest(sab, op, argsJson) {
  const view = new DataView(sab);
  const data = new Uint8Array(sab);
  const opBytes = encoder.encode(op);
  const argsBytes = encoder.encode(argsJson);
  const totalLen = opBytes.length + 1 + argsBytes.length;
  view.setInt32(8, opBytes.length, true);
  data.set(opBytes, HEADER_SIZE);
  data[HEADER_SIZE + opBytes.length] = 0;
  data.set(argsBytes, HEADER_SIZE + opBytes.length + 1);
  view.setInt32(4, totalLen, true);
}
function readRequest(sab) {
  const view = new DataView(sab);
  const data = new Uint8Array(sab);
  const opLen = view.getInt32(8, true);
  const dataLen = view.getInt32(4, true);
  const op = decodeSharedBytes(data, HEADER_SIZE, opLen);
  const argsStart = HEADER_SIZE + opLen + 1;
  const argsLen = dataLen - opLen - 1;
  const argsJson = decodeSharedBytes(data, argsStart, argsLen);
  return { op, argsJson };
}
function writeResponse(sab, json, isError) {
  const view = new DataView(sab);
  const data = new Uint8Array(sab);
  const bytes = encoder.encode(json);
  data.set(bytes, HEADER_SIZE);
  view.setInt32(4, bytes.length, true);
  view.setInt32(0, isError ? FLAG_RESPONSE_ERR : FLAG_RESPONSE_OK, true);
}
function readResponse(sab) {
  const view = new DataView(sab);
  const sabData = new Uint8Array(sab);
  const flag = view.getInt32(0, true);
  const dataLen = view.getInt32(4, true);
  const text = decodeSharedBytes(sabData, HEADER_SIZE, dataLen);
  return { ok: flag === FLAG_RESPONSE_OK, data: text };
}
export {
  FLAG_IDLE,
  FLAG_REQUEST,
  FLAG_RESPONSE_ERR,
  FLAG_RESPONSE_OK,
  HEADER_SIZE,
  SAB_SIZE,
  readRequest,
  readResponse,
  writeRequest,
  writeResponse
};
