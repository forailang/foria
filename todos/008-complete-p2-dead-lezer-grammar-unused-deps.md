---
status: pending
priority: p2
issue_id: "008"
tags: [code-review, dead-code, cleanup]
---

# Dead Lezer Grammar File and Unused Dependencies

## Problem Statement

- `playground/src/lang/forai.grammar` (168 LOC) is never imported or compiled. StreamLanguage is used instead.
- `playground/src/lang/forai.ts:2` imports `tags as t` which is never referenced.
- npm deps `@lezer/lr`, `@lezer/generator`, `codemirror` are unused.
- Unused WASM exports: `tokenize`, `check_formatted` (+ worker handler for tokenize).
- Unused Examples dropdown in HTML.
- Dead `collect_arg_ops` no-op function in compile.rs.

## Proposed Solution

Delete all dead code and unused dependencies.

## Acceptance Criteria

- [ ] `forai.grammar` deleted
- [ ] Unused import `tags as t` removed
- [ ] Unused npm deps removed from package.json
- [ ] Unused WASM exports removed (add back when needed)
- [ ] Worker tokenize handler removed
- [ ] Examples dropdown removed from HTML
- [ ] `collect_arg_ops` removed from compile.rs
