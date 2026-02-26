import * as esbuild from "esbuild";
import { execSync } from "child_process";
import { existsSync, mkdirSync, copyFileSync } from "fs";
import { join, dirname } from "path";
import { fileURLToPath } from "url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const watch = process.argv.includes("--watch");

// Ensure dist directory exists
mkdirSync(join(__dirname, "dist"), { recursive: true });

// Copy WASM pkg files to dist
const wasmPkg = join(__dirname, "../crates/forai-playground-wasm/pkg");
if (existsSync(wasmPkg)) {
  for (const f of ["forai_playground_wasm_bg.wasm", "forai_playground_wasm.js"]) {
    const src = join(wasmPkg, f);
    if (existsSync(src)) {
      copyFileSync(src, join(__dirname, "dist", f));
    }
  }
  console.log("[build] Copied WASM files to dist/");
} else {
  console.warn("[build] WASM pkg not found — run `npm run wasm` first");
}

// Copy index.html to dist
copyFileSync(join(__dirname, "public/index.html"), join(__dirname, "dist/index.html"));

// Build main bundle
const ctx = await esbuild.context({
  entryPoints: [join(__dirname, "src/main.ts")],
  bundle: true,
  outfile: join(__dirname, "dist/playground.js"),
  format: "esm",
  sourcemap: true,
  minify: !watch,
  target: "es2022",
  logLevel: "info",
});

// Build compiler worker
const workerCtx = await esbuild.context({
  entryPoints: [join(__dirname, "src/workers/compiler.ts")],
  bundle: true,
  outfile: join(__dirname, "dist/compiler-worker.js"),
  format: "esm",
  sourcemap: true,
  minify: !watch,
  target: "es2022",
  logLevel: "info",
});

if (watch) {
  await ctx.watch();
  await workerCtx.watch();
  console.log("[build] Watching for changes...");

  // Serve dist directory
  const { host, port } = await ctx.serve({
    servedir: join(__dirname, "dist"),
    port: 5173,
  });
  console.log(`[build] Serving at http://localhost:${port}`);
} else {
  await ctx.rebuild();
  await workerCtx.rebuild();
  ctx.dispose();
  workerCtx.dispose();
  console.log("[build] Done.");
}
