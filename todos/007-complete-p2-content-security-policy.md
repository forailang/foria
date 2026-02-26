---
status: pending
priority: p2
issue_id: "007"
tags: [code-review, security]
---

# Missing Content Security Policy

## Problem Statement

`playground/public/index.html` has no CSP meta tag. Without CSP, any XSS vector has no defense-in-depth mitigation.

## Proposed Solution

Add to `<head>`:
```html
<meta http-equiv="Content-Security-Policy"
  content="default-src 'self'; script-src 'self' blob:; worker-src 'self' blob:; style-src 'self' 'unsafe-inline'; wasm-src 'self'">
```

## Acceptance Criteria

- [ ] CSP meta tag added to index.html
- [ ] Playground still loads and works with CSP active
