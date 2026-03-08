import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

function run(cmd, args, cwd) {
  return spawnSync(cmd, args, { cwd, encoding: "utf8" });
}

test("wasm/browser integration: wasm-test builds wasm output", { timeout: 120000 }, () => {
  const root = resolve(process.cwd(), "..");
  const out = run("cargo", ["run", "-q", "-p", "forai", "--", "build", "examples/wasm-test"], root);
  assert.equal(out.status, 0, out.stderr || out.stdout);
  assert.equal(existsSync(resolve(root, "examples/wasm-test/dist/wasm-test.wasm")), true);
});

test("wasm/browser integration: web-simple-wasm tests + browser artifacts", { timeout: 120000 }, () => {
  const root = resolve(process.cwd(), "..");
  const t = run("cargo", ["run", "-q", "-p", "forai", "--", "test", "examples/web-simple-wasm"], root);
  assert.equal(t.status, 0, t.stderr || t.stdout);

  const b = run("cargo", ["run", "-q", "-p", "forai", "--", "build", "examples/web-simple-wasm"], root);
  assert.equal(b.status, 0, b.stderr || b.stdout);
  assert.equal(existsSync(resolve(root, "examples/web-simple-wasm/dist/browser/index.html")), true);
  assert.equal(existsSync(resolve(root, "examples/web-simple-wasm/dist/browser/forai-browser.js")), true);
  assert.equal(existsSync(resolve(root, "examples/web-simple-wasm/dist/browser/web-simple-wasm.wasm")), true);
});
