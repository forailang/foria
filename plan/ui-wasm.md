# Plan: UI WASM Runtime and Browser Client Apps

Enable `ui.*` apps to run fully in the browser with client-side rendering, client-side events, and reactive updates, reusing the same DSL and UI tree model used by terminal and SSR HTML.

## 1. Goal

Build a browser runtime path so Forai UI apps can run as:

1. Terminal apps (`ui.render` + `ui.events`) — existing
2. SSR apps (`ui.to_html`) — existing
3. WASM/browser apps (`ui.mount` + browser events + client updates) — new

The first target example is `examples/web-simple-wasm`, functionally similar to `examples/web-simple` but running in-browser with client-side UI updates.

## 2. Success Criteria

- A Forai flow can mount a UI tree into a browser DOM container and update it reactively without full page reload.
- UI events (`button` click, `input` change, `toggle`) flow back into Forai as normal event dicts.
- The same view functions used by SSR (`ui.*` trees) are reused in WASM with minimal/no branching.
- `examples/web-simple-wasm` runs in browser and supports navigation + interaction.

## 3. High-Level Design

### 3a. Keep UI Tree as Source of Truth

- Continue using the same JSON UiNode shape: `{type, props, children, events}`.
- WASM runtime receives new tree snapshots and applies DOM patches.

### 3b. Add Browser UI Ops

- `ui.mount(tree, selector)` — mount initial tree in DOM container.
- `ui.update(tree)` — patch existing mounted DOM from new tree.
- `ui.events()` — read next browser event from queue as dict.
- `ui.navigate(path)` — push route/state to browser history.

### 3c. Event Transport

- Browser bindings attach listeners from node event metadata.
- Listener payloads enqueue normalized events for `ui.events()`.
- Event dict format aligns with terminal model where possible:
  - `{type: "action", action: "on_inc", value: true}`
  - `{type: "input", action: "on_change", value: "abc"}`
  - `{type: "nav", path: "/about"}`

### 3d. DOM Patcher

- Start with keyed-or-indexed diff by node position.
- Keep patcher simple in v1:
  - replace node when `type` changes
  - update changed props/styles/events
  - reconcile children list length

## 4. Phases

### Phase 0: Scope + Runtime Contracts

- [x] Document browser UI op contracts in stdlib docs (`ui.mount`, `ui.update`, `ui.events`, `ui.navigate`)
- [x] Define normalized browser event payload schema
- [x] Decide selector/container contract (`#app` default + explicit selector override)

### Phase 1: Core WASM UI Runtime Wiring

- [x] Add/extend WASM host runtime to execute browser UI ops
- [x] Implement `ui.mount(tree, selector)` in wasm host
- [x] Implement `ui.update(tree)` in wasm host
- [x] Implement `ui.events()` event dequeue in wasm host
- [x] Implement `ui.navigate(path)` history push + popstate support
- [x] Ensure non-UI ops needed by example work in browser runtime (or provide stubs with clear failures)

### Phase 2: Browser Renderer

- [x] Add DOM renderer module (`create_element`, `apply_props`, `apply_styles`, `wire_events`)
- [x] Map existing style props to CSS exactly as `ui_html` does for parity
- [x] Support custom style passthrough from `.style("key", value)`
- [x] Implement patch algorithm for `ui.update` (type/props/children diff)
- [x] Add cleanup for detached listeners to avoid leaks

### Phase 3: Routing + App Loop

- [x] Implement browser source flow for initial route + navigation changes
- [x] Add route dispatch flow pattern for client-side path handling
- [x] Support link/button-driven navigation events (`on_nav` -> `ui.navigate`)
- [x] Ensure back/forward updates route state and rerenders correctly

### Phase 4: New Example App (`web-simple-wasm`)

- [x] Create `examples/web-simple-wasm/` project scaffold
- [x] Reuse/port page view funcs from `examples/web-simple` (home/about/blog/not-found)
- [x] Replace server request source with browser route/events source
- [x] Add a browser entry HTML + JS/WASM bootstrap script
- [x] Include run instructions in example docs/README block
- [x] Keep visual structure aligned with `web-simple` so parity is obvious

### Phase 5: Tooling + DX

- [x] Add CLI support for wasm app dev run (or documented script) with live rebuild
- [x] Add compile target docs for wasm/browser
- [x] Provide friendly runtime errors for unsupported ops in browser mode
- [x] Add minimal debugger visibility for browser UI events/tree snapshots

### Phase 6: Tests

- [x] Unit tests for DOM renderer prop/style/event mapping
- [x] Unit tests for patcher correctness (replace/update/remove/reorder cases)
- [x] Unit tests for browser event normalization
- [x] Integration test for basic counter-style reactive UI in wasm
- [x] Integration test for `web-simple-wasm` navigation flow
- [x] Parity test: same UiNode snapshot renders equivalent structure in SSR and browser runtime

## 5. Deliverables

1. Browser UI ops in runtime with clear contracts.
2. DOM renderer + patcher with style/event support.
3. New `examples/web-simple-wasm` example app.
4. Test coverage for renderer, events, and navigation.
5. Updated docs for running browser UI apps.

## 6. Out of Scope (This Plan)

- Full VDOM optimization and keyed reconciliation heuristics beyond simple diffing.
- SSR hydration compatibility with pre-rendered HTML.
- Native mobile/desktop UI targets.
- Advanced accessibility features beyond basic semantic element mapping.

## 7. Recommended Execution Order

1. Phase 0 + 1 first (contracts and runtime hooks)
2. Phase 2 next (renderer + patcher)
3. Phase 4 early skeleton in parallel to validate assumptions quickly
4. Phase 3 routing polish
5. Phase 5 + 6 before marking complete
