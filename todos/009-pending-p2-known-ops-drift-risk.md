---
status: pending
priority: p2
issue_id: "009"
tags: [code-review, architecture, maintenance]
dependencies: ["002"]
---

# known_ops Drift Risk Between Three Op Lists

## Problem Statement

Three separate op lists must be manually kept in sync:
- `crates/forai-core/src/pure_ops.rs` (pure ops)
- `crates/forai/src/runtime.rs` (runtime ops)
- `crates/forai-core/src/compile.rs` lines 31-84 (hardcoded superset)

When a new op is added to runtime but not compile.rs, the playground will reject valid programs. No compile-time or test-time check exists.

## Proposed Solutions

### Option A: Single canonical list
Move the I/O op list to a static in forai-core. Runtime and compile.rs both reference it.

### Option B: Sync test
Add a test that asserts `compile::known_ops()` is a superset of `runtime::known_ops()`.

## Acceptance Criteria

- [ ] A test or compile-time check prevents op list drift
- [ ] No manual duplication of op names across files
