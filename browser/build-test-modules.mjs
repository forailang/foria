import { build } from "esbuild";
import { mkdirSync } from "node:fs";

mkdirSync("dist-test", { recursive: true });

await build({
  entryPoints: ["src/ui_runtime.ts", "src/protocol.ts"],
  bundle: false,
  format: "esm",
  outdir: "dist-test",
  platform: "neutral",
  target: "es2020",
  sourcemap: false,
});

console.log("built dist-test/ui_runtime.js");
console.log("built dist-test/protocol.js");
