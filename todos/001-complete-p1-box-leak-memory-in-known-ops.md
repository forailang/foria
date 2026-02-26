---
status: pending
priority: p1
issue_id: "001"
tags: [code-review, performance, security, memory-leak]
---

# Box::leak Memory Leak in known_ops()

## Problem Statement

`known_ops()` in `crates/forai-core/src/compile.rs:91` uses `Box::leak` to promote codec op strings to `&'static str`. Every call to `compile_project()` leaks ~150 bytes permanently. In the WASM playground, this is called on every keystroke (debounced 300ms), leading to unbounded memory growth that can never be reclaimed (WASM linear memory only grows).

Projected: ~1.6 MB leaked per hour of active typing.

## Findings

- Found by: Security Sentinel, Performance Oracle, Rust Reviewer
- Location: `crates/forai-core/src/compile.rs` lines 88-92
- Additionally, `CodecRegistry::default_registry()` is built twice per compile (once inside `known_ops()`, once at line 154)

## Proposed Solutions

### Option A: LazyLock/OnceLock (Recommended)
Compute the HashSet once with `std::sync::LazyLock`:
```rust
static KNOWN_OPS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| { /* build once */ });
```
- Pros: Zero allocation after first call, zero leaks
- Cons: Minimal refactoring
- Effort: Small

### Option B: Return HashSet<String>
Change return type to owned strings.
- Pros: No leaks, no static lifetime hacks
- Cons: Allocates on every call (though much cheaper than current)
- Effort: Small

## Acceptance Criteria

- [ ] `known_ops()` is computed at most once per WASM instance lifetime
- [ ] No `Box::leak` calls remain in compile.rs
- [ ] Redundant `CodecRegistry::default_registry()` on line 154 removed
