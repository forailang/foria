import test from "node:test";
import assert from "node:assert/strict";

import {
  SAB_SIZE,
  writeRequest,
  readRequest,
  writeResponse,
  readResponse,
} from "../dist-test/protocol.js";

test("protocol request round-trip", () => {
  const sab = new SharedArrayBuffer(SAB_SIZE);
  writeRequest(sab, "ui.events", '[{"x":1}]');
  const { op, argsJson } = readRequest(sab);
  assert.equal(op, "ui.events");
  assert.equal(argsJson, '[{"x":1}]');
});

test("protocol response round-trip ok + error", () => {
  const sab = new SharedArrayBuffer(SAB_SIZE);

  writeResponse(sab, JSON.stringify({ ok: true }), false);
  const ok = readResponse(sab);
  assert.equal(ok.ok, true);
  assert.equal(ok.data, JSON.stringify({ ok: true }));

  writeResponse(sab, "boom", true);
  const err = readResponse(sab);
  assert.equal(err.ok, false);
  assert.equal(err.data, "boom");
});
