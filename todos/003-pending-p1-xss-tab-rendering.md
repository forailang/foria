---
status: pending
priority: p1
issue_id: "003"
tags: [code-review, security, xss]
---

# XSS via Unsanitized File Name in Tab Rendering

## Problem Statement

`playground/src/main.ts:236` inserts file names directly into HTML via `innerHTML` without escaping:
```typescript
tab.innerHTML = `<span>${f.name}</span>`;
```

Currently file names are hardcoded, but the `add-tab` element exists and dynamic file creation is planned. A file named `<img src=x onerror=alert(1)>` would execute arbitrary JavaScript.

## Findings

- Found by: Security Sentinel, TypeScript Reviewer
- `escapeHtml()` utility already exists in the codebase at line 380

## Proposed Solutions

### Option A: Use textContent (Recommended)
```typescript
const span = document.createElement("span");
span.textContent = f.name;
tab.appendChild(span);
```
- Effort: Small (one-line fix)

### Option B: Use existing escapeHtml
```typescript
tab.innerHTML = `<span>${escapeHtml(f.name)}</span>`;
```
- Effort: Small (one-line fix)

## Acceptance Criteria

- [ ] File names in tab bar are properly escaped or use textContent
- [ ] No `innerHTML` with unescaped user-controllable data
