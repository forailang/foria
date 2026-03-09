import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { resolve } from "node:path";

function run(cmd, args, cwd) {
  return spawnSync(cmd, args, { cwd, encoding: "utf8" });
}

test("linux-ui integration: counter-linux tests + linux artifact", { timeout: 120000 }, () => {
  const root = resolve(process.cwd(), "..");
  const t = run("cargo", ["run", "-q", "-p", "forai", "--", "test", "examples/counter-linux"], root);
  assert.equal(t.status, 0, t.stderr || t.stdout);

  const b = run("cargo", ["run", "-q", "-p", "forai", "--", "build", "examples/counter-linux"], root);
  assert.equal(b.status, 0, b.stderr || b.stdout);
  assert.equal(existsSync(resolve(root, "examples/counter-linux/dist/linux-ui/counter-linux")), true);
  assert.equal(existsSync(resolve(root, "examples/counter-linux/dist/linux-ui/run-linux-ui.sh")), true);
});

test("linux-ui integration: web-simple-linux tests + linux artifact", { timeout: 120000 }, () => {
  const root = resolve(process.cwd(), "..");
  const t = run("cargo", ["run", "-q", "-p", "forai", "--", "test", "examples/web-simple-linux"], root);
  assert.equal(t.status, 0, t.stderr || t.stdout);

  const b = run("cargo", ["run", "-q", "-p", "forai", "--", "build", "examples/web-simple-linux"], root);
  assert.equal(b.status, 0, b.stderr || b.stdout);
  assert.equal(existsSync(resolve(root, "examples/web-simple-linux/dist/linux-ui/web-simple-linux")), true);
  assert.equal(existsSync(resolve(root, "examples/web-simple-linux/dist/linux-ui/run-linux-ui.sh")), true);
});
