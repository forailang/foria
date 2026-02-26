---
status: pending
priority: p3
issue_id: "010"
tags: [code-review, quality, polish]
---

# Minor Improvements (P3 Batch)

## Items

1. **Resize handler fragility** — `document.onmousemove` global handler in main.ts. Consider `addEventListener`/`removeEventListener` pattern.
2. **IR JSON pretty-printed eagerly** — `JSON.stringify(result.ok.entry_ir, null, 2)` runs on every compile even when IR tab not viewed. Defer to tab switch.
3. **WASM binary not size-optimized** — Add `[profile.release]` with `lto = true`, `opt-level = "z"`, `codegen-units = 1` to Cargo.toml. Run `wasm-opt`. Could save 20-40%.
4. **Non-ASCII input may panic WASM lexer** — Byte-level slicing on multi-byte UTF-8 chars. Use `source.get(t.start..t.end).unwrap_or("")` as guard.
5. **`unwrap_or_default` silently swallows serialization failures** — WASM crate lib.rs lines 26-29. Consider propagating as errors.
6. **forai-core carries unused crypto deps for WASM** — bcrypt/sha2/hmac in pure_ops. Consider cargo feature flag `runtime-ops`.
7. **Hardcoded "Cmd+Enter"** — Should detect OS for Ctrl vs Cmd display.
8. **Run button misleading** — Shows "Run" but only compiles. Label as "Compile" until runtime execution is implemented.

## Acceptance Criteria

- [ ] Each item addressed or explicitly deferred with rationale
