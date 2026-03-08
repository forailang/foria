import test from "node:test";
import assert from "node:assert/strict";

import { createUiRuntime } from "../dist-test/ui_runtime.js";
import { installFakeDom } from "./helpers/fake_dom.mjs";

test("ui runtime maps styles and normalizes button/input/toggle events", () => {
  const env = installFakeDom();
  const events = [];
  const runtime = createUiRuntime((event) => events.push(event));

  const tree = {
    type: "vstack",
    props: { spacing: 8, padding: 12, align: "center", "letter-spacing": "1px" },
    children: [
      { type: "button", props: { label: "About" }, events: { on_about: true }, children: [] },
      { type: "input", props: { placeholder: "Name", value: "" }, events: { on_name: true }, children: [] },
      { type: "toggle", props: { value: false }, events: { on_enabled: true }, children: [] },
    ],
  };

  runtime.mount(tree, "#app");

  const root = env.app.childNodes[0];
  assert.equal(root.style.display, "flex");
  assert.equal(root.style.flexDirection, "column");
  assert.equal(root.style.gap, "8px");
  assert.equal(root.style.padding, "12px");
  assert.equal(root.style.alignItems, "center");
  assert.equal(root.style["letter-spacing"], "1px");

  const btn = root.childNodes[0];
  btn.click();

  const input = root.childNodes[1];
  input.value = "ada";
  input.dispatchEvent({ type: "input" });

  const toggle = root.childNodes[2];
  toggle.checked = true;
  toggle.dispatchEvent({ type: "change" });

  assert.deepEqual(events[0], { type: "action", action: "on_about", value: true });
  assert.deepEqual(events[1], { type: "input", action: "on_name", value: "ada" });
  assert.deepEqual(events[2], { type: "toggle", action: "on_enabled", value: true });

  runtime.unmount();
  env.teardown();
});
