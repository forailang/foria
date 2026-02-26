---
status: pending
priority: p1
issue_id: "002"
tags: [code-review, architecture, correctness, duplication]
---

# compile.rs Duplicates loader.rs with Semantic Bugs

## Problem Statement

`crates/forai-core/src/compile.rs` (781 LOC) is a near-verbatim reimplementation of `crates/forai/src/loader.rs`. The duplication has already introduced semantic divergences:

1. **`transform_source_steps` is broken**: Virtual version creates `SourceLoop` with empty body; native version wraps all subsequent statements into the loop body. This produces incorrect IR for any flow with statements after a source call.
2. **`collect_ops` missing `Statement::On`**: Ops inside `on` blocks are not validated in the playground.
3. **`collect_expr_ops` missing `Expr::Interp`**: Ops inside string interpolation not validated.
4. **`sub_registry` not forwarded to parent**: Nested imports won't be in final FlowRegistry.

## Findings

- Found by: Code Simplicity Reviewer, Rust Reviewer, Architecture Strategist
- Three independent `collect_ops` implementations exist: `main.rs`, `loader.rs`, `compile.rs`
- Two `transform_source_steps` implementations with different semantics
- `collect_arg_ops` is a dead no-op function

## Proposed Solutions

### Option A: SourceLoader Trait (Recommended)
Extract shared compilation logic behind a `trait SourceLoader { fn read_file(&self, path: &str) -> Option<String>; fn list_directory(&self, path: &str) -> Vec<String>; }`. Both native and virtual paths call the same `compile_from_loader()`.
- Pros: Eliminates all duplication, single source of truth
- Cons: Larger refactor touching loader.rs
- Effort: Medium-Large

### Option B: Fix Divergences in compile.rs
Manually fix `transform_source_steps`, add `On`/`Interp` handlers, forward sub_registry.
- Pros: Smaller diff, quick fix
- Cons: Duplication remains; drift will recur
- Effort: Small

## Acceptance Criteria

- [ ] `transform_source_steps` produces nested bodies (matching loader.rs semantics)
- [ ] `collect_ops` handles `Statement::On` and `Expr::Interp`
- [ ] `sub_registry` contents forwarded to parent FlowRegistry
- [ ] Dead `collect_arg_ops` removed
- [ ] Same .fa source produces same IR via both native and virtual paths
