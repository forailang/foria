import test from "node:test";
import assert from "node:assert/strict";

import { createUiRuntime } from "../dist-test/ui_runtime.js";
import { installFakeDom } from "./helpers/fake_dom.mjs";

test("ui runtime patch updates text, removes and appends children", () => {
  const env = installFakeDom();
  const runtime = createUiRuntime(() => {});

  runtime.mount({
    type: "vstack",
    props: { spacing: 4 },
    children: [
      { type: "text", props: { value: "A" }, children: [] },
      { type: "button", props: { label: "Old" }, children: [] },
    ],
  }, "#app");

  runtime.update({
    type: "vstack",
    props: { spacing: 10 },
    children: [
      { type: "text", props: { value: "B" }, children: [] },
      { type: "input", props: { placeholder: "new", value: "x" }, children: [] },
      { type: "button", props: { label: "Add" }, children: [] },
    ],
  });

  const root = env.app.childNodes[0];
  assert.equal(root.style.gap, "10px");
  assert.equal(root.childNodes.length, 3);
  assert.equal(root.childNodes[0].textContent, "B");
  assert.equal(root.childNodes[1].tagName, "INPUT");
  assert.equal(root.childNodes[2].textContent, "Add");

  runtime.unmount();
  env.teardown();
});

test("ui runtime patch handles child replacement and reordering", () => {
  const env = installFakeDom();
  const runtime = createUiRuntime(() => {});

  runtime.mount({
    type: "hstack",
    props: { spacing: 6 },
    children: [
      { type: "text", props: { value: "First" }, children: [] },
      { type: "button", props: { label: "SwapMe" }, children: [] },
    ],
  }, "#app");

  runtime.update({
    type: "hstack",
    props: { spacing: 6 },
    children: [
      { type: "input", props: { placeholder: "Replaced", value: "ok" }, children: [] },
      { type: "text", props: { value: "First" }, children: [] },
    ],
  });

  const root = env.app.childNodes[0];
  assert.equal(root.childNodes[0].tagName, "INPUT");
  assert.equal(root.childNodes[0].placeholder, "Replaced");
  assert.equal(root.childNodes[1].textContent, "First");

  runtime.unmount();
  env.teardown();
});
