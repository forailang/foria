---
status: pending
priority: p2
issue_id: "006"
tags: [code-review, typescript, type-safety]
---

# Untyped Worker Message Protocol

## Problem Statement

`wasmModule: any`, `compileErrors: any[]`, Worker messages have no TypeScript types. The `tokenBase(stream: any, state: any)` in forai.ts also lacks types.

## Proposed Solution

Define discriminated union types:
```typescript
interface CompileError { file: string; line: number; col: number; message: string; }
type WorkerRequest = { type: "compile"; id: number; files: Record<string,string>; entryPoint: string; }
  | { type: "format"; id: number; source: string; }
  | { type: "tokenize"; id: number; source: string; };
```

Add a `tsconfig.json` with strict mode enabled.

## Acceptance Criteria

- [ ] All `any` types replaced with proper interfaces
- [ ] Worker messages use discriminated unions
- [ ] `tsconfig.json` with `strict: true` added
