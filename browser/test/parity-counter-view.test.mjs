import test from "node:test";
import assert from "node:assert/strict";

import { createUiRuntime } from "../dist-test/ui_runtime.js";
import { installFakeDom } from "./helpers/fake_dom.mjs";

test("counter-style UiNode renders browser structure equivalent to SSR expectations", () => {
  const env = installFakeDom();
  const runtime = createUiRuntime(() => {});

  const tree = {
    type: "screen",
    props: {},
    children: [{
      type: "vstack",
      props: { spacing: 20 },
      children: [
        { type: "text", props: { value: "Current Count: 42", size: 24 }, children: [] },
        {
          type: "hstack",
          props: { spacing: 10 },
          children: [
            { type: "button", props: { label: "+" }, children: [] },
            { type: "button", props: { label: "-" }, children: [] },
          ],
        },
      ],
    }],
  };

  runtime.mount(tree, "#app");

  const screen = env.app.childNodes[0];
  assert.equal(screen.className, "forai-screen");

  const vstack = screen.childNodes[0];
  assert.equal(vstack.style.display, "flex");
  assert.equal(vstack.style.flexDirection, "column");
  assert.equal(vstack.style.gap, "20px");

  const title = vstack.childNodes[0];
  assert.equal(title.tagName, "SPAN");
  assert.equal(title.textContent, "Current Count: 42");
  assert.equal(title.style.fontSize, "24px");

  const controls = vstack.childNodes[1];
  assert.equal(controls.style.display, "flex");
  assert.equal(controls.style.gap, "10px");
  assert.equal(controls.childNodes[0].textContent, "+");
  assert.equal(controls.childNodes[1].textContent, "-");

  runtime.unmount();
  env.teardown();
});
