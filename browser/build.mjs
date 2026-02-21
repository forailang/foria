import { build } from "esbuild";
import { readFileSync, writeFileSync, mkdirSync } from "fs";

mkdirSync("dist", { recursive: true });

// 1. Build worker as self-contained IIFE (no imports — everything bundled in)
await build({
  entryPoints: ["src/worker.ts"],
  bundle: true,
  format: "iife",
  outfile: "dist/dataflow-worker.js",
  platform: "browser",
  target: "es2020",
  sourcemap: true,
  minify: false,
});

console.log("built dist/dataflow-worker.js");

// 2. Read worker bundle and generate inline module for embedding
const workerCode = readFileSync("dist/dataflow-worker.js", "utf8");
writeFileSync(
  "dist/worker-inline.js",
  `export const workerCode = ${JSON.stringify(workerCode)};\n`,
);

console.log("generated dist/worker-inline.js");

// 3. Build main bundle (imports the generated worker-inline.js)
await build({
  entryPoints: ["src/index.ts"],
  bundle: true,
  format: "esm",
  outfile: "dist/forai-browser.js",
  platform: "browser",
  target: "es2020",
  sourcemap: true,
  minify: false,
});

console.log("built dist/forai-browser.js");
