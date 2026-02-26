---
status: pending
priority: p2
issue_id: "004"
tags: [code-review, correctness, race-condition]
---

# Stale Compilation Results Race Condition

## Problem Statement

`playground/src/main.ts:88-105` — the `compile-result` handler never checks `msg.id` against the current `compileId`. If compilation N takes 100ms and compilation N+1 takes 50ms, N+1's correct results will be overwritten by N's stale results.

## Proposed Solution

Add one line at top of handler:
```typescript
if (msg.id !== compileId) return;
```

## Acceptance Criteria

- [ ] `compile-result` handler validates `msg.id` matches current `compileId`
- [ ] `format-result` handler also validates message ID
