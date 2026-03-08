import test from "node:test";
import assert from "node:assert/strict";

import { createUiRuntime } from "../dist-test/ui_runtime.js";
import { installFakeDom } from "./helpers/fake_dom.mjs";

function counterTree(count) {
  return {
    type: "screen",
    props: {},
    children: [{
      type: "vstack",
      props: { spacing: 10 },
      children: [
        { type: "text", props: { value: `Current Count: ${count}`, size: 18 }, children: [] },
        {
          type: "hstack",
          props: { spacing: 8 },
          children: [
            { type: "button", props: { label: "+" }, events: { on_inc: true }, children: [] },
            { type: "button", props: { label: "-" }, events: { on_dec: true }, children: [] },
          ],
        },
      ],
    }],
  };
}

test("counter-style reactive UI flow updates after normalized action events", () => {
  const env = installFakeDom();
  const queue = [];
  const runtime = createUiRuntime((event) => queue.push(event));

  let count = 0;
  runtime.mount(counterTree(count), "#app");

  const controls = env.app.childNodes[0].childNodes[0].childNodes[1];
  controls.childNodes[0].click(); // on_inc

  const event = queue.shift();
  assert.deepEqual(event, { type: "action", action: "on_inc", value: true });

  if (event.action === "on_inc") count += 1;
  runtime.update(counterTree(count));

  const title = env.app.childNodes[0].childNodes[0].childNodes[0];
  assert.equal(title.textContent, "Current Count: 1");

  runtime.unmount();
  env.teardown();
});
