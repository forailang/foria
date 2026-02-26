---
status: pending
priority: p2
issue_id: "005"
tags: [code-review, performance]
dependencies: ["001"]
---

# Cache known_ops() as Static

## Problem Statement

`known_ops()` constructs a ~200-entry HashSet, creates a CodecRegistry with HashMap + Box<dyn Codec>, generates a Vec<String> of codec ops — all rebuilt on every `compile_project()` call. This is pure waste since the result is identical every time.

## Proposed Solution

Use `LazyLock` to compute once (same fix as issue 001).

## Acceptance Criteria

- [ ] `known_ops()` returns a reference to a static, computed-once set
- [ ] No per-compile allocation for op set construction
