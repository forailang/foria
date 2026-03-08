# Plan: Universal UI System for Forai

Build a built-in, declarative UI templating system for `forai` that works across Terminal, Web, and Native platforms using a unified stack-based abstraction.

## 1. Objective
Enable developers to build visually rich, interactive applications using a single set of UI primitives that are "flow-aware." UI components act as both **Sinks** (rendering state) and **Sources** (emitting events).

## 2. The "Core 10" Primitives (`ui.*`)
Inspired by SwiftUI and React Native, these primitives provide a "simple but composable" foundation.

| Primitive | Type | Description |
|-----------|------|-------------|
| `vstack` | Layout | Vertical stack of children |
| `hstack` | Layout | Horizontal stack of children |
| `zstack` | Layout | Depth-based stack (overlays, modals) |
| `text` | Content | Styled labels and paragraphs |
| `image` | Content | Static or remote images |
| `shape` | Content | Rects, circles, paths for custom UI |
| `button` | Interact | Clickable container with label/children |
| `input` | Interact | Text entry field |
| `toggle` | Interact | Binary switch (checkbox/toggle) |
| `list` | Collection | High-performance scrolling list of items |

## 3. Architecture

### A. Compiler: Block-Aware Functions
Update the `forai` parser to support **UI Blocks**. 
Syntactically, these are function calls that take an optional trailing `do...done` block.
- **Syntax:** `ui.vstack(10 to :spacing) do ... done`
- **Internal Representation:** The block is captured as a list of `ui_node` expressions passed to a reserved `:children` port of the function.

### B. Runtime: The UI-IR
The runtime executes UI functions to produce a **UI-IR (JSON Tree)**.
- Each node contains `type`, `props`, `children`, and `events`.
- This tree is what travels across the "wire" in the debugger.
- **Reactivity:** When a flow's state changes, the UI-IR is recalculated.

### C. The Sink Strategy (Platform Adapters)
Platforms are implemented as specialized `sinks` that translate the UI-IR to local primitives.

1. **Terminal Sink (`term.render`):** Maps stacks to grid coordinates using `crossterm`.
2. **Web Sink (`html.render`):**
   - **SSR:** Generates static HTML/CSS.
   - **WASM:** Uses a Virtual DOM approach to patch the browser DOM.
3. **Native Sinks (Mac/Linux/iOS/Android):** Maps to platform-native toolkits (AppKit, GTK, SwiftUI/Jetpack Compose).

## 4. Event Model: UI as Source
Interaction primitives (like `button` or `on :click`) use the standard `emit` keyword.
- Clicks/Toggles emit to named ports.
- The `flow` wiring handles these as standard events.
- **Example:** `step ui.Button(...) then next :click to clicked_wire done`

## 5. Implementation Phases

### Phase 1: IR & Parser (Core) ✅

#### 1a. AST: Add block children to `Expr::Call`

Currently `Expr::Call` has `{ func: String, args: Vec<Expr> }`. Extend it with an optional children field:

```rust
Expr::Call {
    func: String,
    args: Vec<Expr>,
    children: Option<Vec<Statement>>,  // ← NEW: do...done block body
}
```

This keeps the change minimal — non-UI calls simply have `children: None`. Every existing `Expr::Call` construction site gets `children: None` added.

Affected patterns:
- `parse_pratt_expr` where `Call` is constructed (primary expression parser)
- `parse_bare_call` where `Call` is constructed for bare op calls
- `parse_assign_stmt` where `Call` is destructured for the simple-args fast path
- `eval_expr` in `sync_runtime.rs` where `Expr::Call` is matched

#### 1b. Parser: Detect `do...done` blocks after function calls

In `parser.rs`, after parsing `op(args)` in both `parse_bare_call()` and the call branch of `parse_pratt_expr()`:

1. Peek for `Ident("do")` token after the closing `)`
2. If found, consume `do`, then call `parse_block()` with `Done` as the stop token to collect the inner statements
3. Store the resulting `Vec<Statement>` in `Expr::Call { children: Some(stmts) }`
4. If not found, proceed as today with `children: None`

Edge cases to handle:
- Nested `do...done` blocks (recursive — each inner UI call parses its own block)
- Mixed content: blocks containing both UI calls and regular statements (e.g., `emit` inside a `ui.button do...done`)
- `do` must appear on the same line as `)` (not after a newline, to avoid ambiguity with bare `do` keywords elsewhere)

#### 1c. Types: Register `UiNode` as a built-in struct

In `types.rs`, add `UiNode` to the built-in struct types (alongside `HttpRequest`, `HttpResponse`, etc.):

```rust
// UiNode — the universal UI tree node
TypeDef::Struct {
    open: true,
    fields: vec![
        ResolvedField { name: "type".into(),     type_name: "text".into(), required: true,  constraints: vec![] },
        ResolvedField { name: "props".into(),    type_name: "dict".into(), required: true,  constraints: vec![] },
        ResolvedField { name: "children".into(), type_name: "list".into(), required: false, constraints: vec![] },
        ResolvedField { name: "events".into(),   type_name: "dict".into(), required: false, constraints: vec![] },
    ],
}
```

Also add `"UiNode"` to `op_types.rs` struct-returning ops registry so that `ui.*` ops are recognized as returning `UiNode`.

#### 1d. Runtime: Evaluate block children into UiNode trees

In `sync_runtime.rs`, update the `Expr::Call` evaluation branch:

```rust
Expr::Call { func, args, children } => {
    let mut evaluated_args = /* ... existing arg eval ... */;

    // If this call has a do...done block, evaluate children into a list
    if let Some(child_stmts) = children {
        let mut child_nodes = Vec::new();
        for stmt in child_stmts {
            let result = execute_statement(stmt, vars, ...)?;
            // Collect non-void results (each UI call returns a UiNode)
            if !result.is_null() {
                child_nodes.push(result);
            }
            // Emit statements inside blocks are forwarded, not collected
        }
        evaluated_args.push(Value::Array(child_nodes));  // children as last arg
    }

    dispatch_op(func, &evaluated_args, ...)
}
```

Key detail: statements like `emit` inside a block (e.g., inside `ui.button do...done`) must still propagate to the enclosing func's emit ports, not be collected as children.

#### 1e. Pure Ops: Implement `ui.vstack`, `ui.hstack`, `ui.text`, `ui.screen`, `ui.button`, `ui.input`, `ui.toggle`

Add to `pure_ops.rs`:

```rust
// ui.vstack(spacing, children_list) → UiNode
"ui.vstack" => build_ui_node("vstack", args),
"ui.hstack" => build_ui_node("hstack", args),
"ui.zstack" => build_ui_node("zstack", args),
"ui.text"   => build_ui_node("text", args),
"ui.screen" => build_ui_node("screen", args),
"ui.button" => build_ui_node("button", args),
"ui.input"  => build_ui_node("input", args),
"ui.toggle" => build_ui_node("toggle", args),
```

Where `build_ui_node` is a helper:
- Named args (`:spacing`, `:value`, `:size`) go into `props` dict
- The last arg, if it's a list (from block eval), becomes `children`
- Returns `{ "type": "vstack", "props": { "spacing": 20 }, "children": [...] }`

#### 1f. Tests: TDD round-trip for UI blocks

Write tests in this order (red-green-refactor):

1. **Lexer/Parser test**: `.fa` source with `ui.vstack(10 to :spacing) do ... done` parses into `Expr::Call` with `children: Some([...])`
2. **Nested parse test**: Three levels of nesting (`screen > vstack > text`) parse correctly
3. **Runtime test**: Evaluating a UI block returns the expected `UiNode` JSON tree
4. **Integration test**: The counter example's `CounterView` func compiles and produces the correct UI-IR tree with `.children[0].children[0].props.value == "Current Count: 42"`
5. **Emit-in-block test**: `emit` inside a `ui.button do...done` block correctly propagates to the func's output port, not into the children list

### Phase 2: Terminal Renderer ✅

Render UiNode trees to the terminal using `crossterm` (already a dependency). This is the first visual proof that the UI-IR works.

#### 2a. Layout Algorithm

Implement a minimal box-layout engine that converts a UiNode tree into positioned character cells:

- [x] `Rect` struct: `{ x, y, width, height }` in character units
- [x] `layout_node(node, available_rect) → Vec<PositionedNode>` recursive layout pass
- [x] **vstack**: divides available height equally among children (or by content size), full width
- [x] **hstack**: divides available width equally among children (or by content size), full height
- [x] **text**: measures string length, wraps at available width, returns measured height
- [x] **button**: renders as `[ label ]` with 1-char padding, fixed height of 1 line
- [x] **screen**: root container, sized to terminal dimensions via `crossterm::terminal::size()`

Keep it simple: no fractional sizing, no flex weights, no scroll. Equal division with content-size fallback.

#### 2b. Render Pass

- [x] `render_tree(positioned_nodes) → Vec<DrawCommand>` converts layout output to crossterm commands
- [x] `DrawCommand` enum: `MoveTo(x,y)`, `Print(text)`, `SetColor(fg,bg)`, `SetBgColor`, `SetAttribute`, `ResetColor`, `Clear`
- [x] Add `ui.render` as an I/O op in `host_native.rs` — takes a UiNode tree, runs layout + render, writes to stdout
- [x] Enter alternate screen + raw mode on first `ui.render`, restore on flow exit (via `Drop`)

#### 2c. Input Event Loop

- [x] `ui.events` source op — wraps `crossterm::event::read()` to emit keyboard/mouse events
- [x] Emit events as dicts: `{ "type": "key", "key": "enter" }`, `{ "type": "resize", "width": 80, "height": 24 }`
- [x] Terminal resize events trigger re-layout and re-render with new dimensions

#### 2d. Tests

- [x] Layout test: vstack with 3 text children in an 80x24 rect produces correct y-offsets
- [x] Layout test: hstack divides width among children
- [x] Render test: `ui.render` on a simple tree produces expected crossterm command sequence (mock stdout)
- [x] Integration test: counter example renders to alternate screen, captures initial frame

### Phase 3: Reactivity

Introduces `state`, `local`, and `via` to enable live interactive loops. This bridges Phase 1 (static trees) and Phase 2 (terminal rendering) into reactive applications.

#### 3a. `state` keyword — reactive flow-level variables

`state` declares a variable at the top of a flow body, before any steps. State variables use **snapshot semantics**: reads within a cycle return the value from cycle start, writes are queued and take effect on the next cycle.

```fa
flow CounterFlow
body
  state counter = 0           # declared before steps

  step CounterView(counter to :count) then
    next :inc_delta via IncrementButton(counter to :count) then
      next :result to counter  # queues counter=1, still 0 this cycle
    done
  done
  step DoSomeLogging() done    # counter is still 0 here
done
```

Rules:
- [x] `state` declarations must appear before any `step` in a flow body (compiler enforces)
- [x] Parser: add `state` as a new statement type — `state <name> = <expr>`
- [x] AST: add `Statement::State { name, init_expr }` variant
- [x] Runtime: at cycle start, snapshot all state values into an immutable read map
- [x] Runtime: writes to state variables go to a pending-writes map, not the snapshot
- [x] Runtime: after all steps complete, if pending-writes differ from snapshot, start new cycle with merged values

#### 3b. `local` keyword — mutable cycle-scoped variables

`local` declares a variable that updates immediately within a cycle and does not trigger re-runs. Useful for accumulators, intermediate computations, and values built up across steps.

```fa
flow ProcessBatch
body
  state items = get_items()
  local total = 0

  loop items as item
    total = total + item["amount"]   # immediately updated
  done
  step ShowSummary(total to :total) done  # sees accumulated value
done
```

Rules:
- [x] `local` declarations must appear before any `step` (alongside `state`)
- [x] Parser: add `local` as a new statement type — `local <name> = <expr>`
- [x] AST: add `Statement::Local { name, init_expr }` variant
- [x] Runtime: `local` variables live in the normal mutable scope, reset to init value at cycle start

#### 3c. `via` keyword — async event handlers

`via` triggers a flow/func in response to an async event from a step. It works just like `step` — you wire its outputs with `then`/`done`. The only difference is initiation: `step` runs as part of the pipeline, `via` runs when an event fires.

```fa
step CounterView(counter to :count) then
  # shorthand: single output goes directly to target
  next :inc_delta via IncrementButton(counter to :count) to counter

  # full form: wire multiple outputs
  next :submit via ValidateForm(data to :input) then
    next :valid to form_data
    next :error to error_message
  done
done
```

Rules:
- [x] Parser: inside `next` declarations, detect `via` keyword after port name
- [x] Parser: after `via`, parse a func/flow call (same as `step` call syntax)
- [x] Parser: after the call, accept `to <var>` shorthand (multi-output `then`/`done` deferred — conflicts with `collect_body_text` depth tracking)
- [x] AST: extend `NextWire` with optional `via_callee`, `via_inputs`, `via_outputs` fields
- [x] Runtime: when the parent step emits on the port, dispatch the `via` handler as a new async task
- [x] Runtime: when the handler completes, write its outputs to the target variables
- [x] Runtime: if a target is a `state` variable, queue the write (triggers new cycle)

#### 3d. Cycle execution model

The flow runtime executes in cycles. A cycle is one complete forward pass through all steps.

```
Cycle 1: state snapshot {counter: 0}
  → CounterView renders with 0
  → user clicks increment
  → via IncrementButton runs → queues counter=1
  → DoSomeLogging runs
  → OtherSideEffect runs
  → cycle ends, counter changed → start cycle 2

Cycle 2: state snapshot {counter: 1}
  → CounterView renders with 1
  → no user interaction (or no state-changing events)
  → DoSomeLogging runs
  → OtherSideEffect runs
  → cycle ends, no state changed → done, wait for next event
```

Rules:
- [x] Runtime: implement cycle loop — snapshot state, run all steps, check for pending writes, repeat or wait
- [x] Runtime: max cycle limit (default 100) as safety net against infinite loops; emit warning at limit
- [x] Runtime: between cycles, only re-execute if at least one `state` variable changed
- [x] Runtime: first cycle is special — not needed; sources handle blocking for events, cycle model is uniform across all cycles

#### 3e. Interaction with UI rendering

The cycle model determines when rendering happens:
- UI steps (`CounterView`) re-run every cycle, producing a new UiNode tree
- `ui.render` (from Phase 2) diffs the new tree against the previous one and patches the terminal
- On the first cycle, there's no previous tree — full render
- Between cycles (waiting for events), the rendered UI stays on screen

This means rendering is not special-cased — it's just a step that happens to have a visible side effect. The cycle model handles the "when to re-render" question automatically.

#### 3f. Tests

- [x] State snapshot test: writing to `state` inside a step doesn't change the read value within the same cycle
- [x] State cycle test: writing to `state` triggers a new cycle where the updated value is visible
- [x] Local mutation test: writing to `local` is immediately visible to later steps in the same cycle
- [x] Via dispatch test: event on a port triggers the `via` handler, result writes to target variable
- [x] Via-to-state test: `via` handler result writing to a `state` variable triggers a new cycle
- [x] Max cycle test: flow that always mutates state stops at cycle limit with warning
- [x] Counter integration test: existing counter-ui example tests pass (8/8), reactive cycle tests cover state/local semantics

### Phase 4: Web SSR

Generate static HTML/CSS from the UiNode tree. Since the UI-IR is platform-agnostic, this is a second renderer alongside terminal.

#### 4a. HTML Generation

- [x] `render_html(node: &UiNode) → String` recursive tree walker
- [x] **vstack** → `<div style="display:flex; flex-direction:column; gap:{spacing}px">`
- [x] **hstack** → `<div style="display:flex; flex-direction:row; gap:{spacing}px">`
- [x] **text** → `<span style="font-size:{size}px">` with escaped content
- [x] **button** → `<button>` with label
- [x] **input** → `<input type="text">` with placeholder/value props
- [x] **toggle** → `<input type="checkbox">`
- [x] **screen** → `<div class="forai-screen">` wrapper with basic reset CSS

#### 4b. Integration with `http.respond`

- [x] Add `ui.to_html` pure op — takes a UiNode tree, returns HTML string
- [x] Compose with existing `http.respond.html(conn, html)` — no new sink needed
- [x] Example: `html = ui.to_html(view)` then `http.respond.html(conn, html)`

#### 4c. Tests

- [x] Unit test: vstack with two text children produces correct nested div/span HTML
- [x] Unit test: HTML entities in text content are escaped
- [x] Integration test: counter view renders to HTML with correct structure

### Phase 5: Styling System

Add visual styling props to UI primitives. Minimal but sufficient for real apps.

#### 5a. Style Props

- [x] **Modifier syntax**: block context binding + fluent modifiers, e.g. `ui.vstack(8) do stack ... stack.padding(12) ... done`
- [x] **Runtime prop capture**: `stack.<modifier>(...)` writes to node props (`padding`, `margin`, `align`, `width`, `height`, `color`, `bg`, `backgroundColor`, `bold`, `italic`, `size`, `border`) plus arbitrary keys via `stack.style("key", value)`
- [x] **Layout**: `padding`, `margin`, `align` (start/center/end), `width`, `height` (via modifier methods)
- [x] **Color**: `color` (text foreground), `bg`/`backgroundColor` (via modifier methods)
- [x] **Text**: `size`, `bold`, `italic` (via modifier methods)
- [x] **Border**: `border` (bool or style string)
- [x] **Escape hatch**: generic inline style API (e.g. `stack.style("key", "value")`)

#### 5b. Renderer Updates

- [x] Terminal: map colors to crossterm `Color` enum, map `:bold`/`:italic` to `Attribute`
- [x] Web: map style props to inline CSS (`padding`, `margin`, `align`, `width`, `height`, `color`, `bg`, `size`, `bold`, `italic`, `border`)

#### 5c. Tests

- [x] Styled text renders with correct crossterm attributes in terminal
- [x] Styled vstack produces correct CSS in HTML output
- [x] Styled text produces correct CSS in HTML output

### Future Considerations

These are out of scope for the initial implementation but worth tracking:

- **WASM client-side runtime**: Virtual DOM diffing in the browser, event binding back to forai runtime via WebSocket. Large standalone project.
- **Native renderers**: GTK, Skia, or platform-native toolkit bindings (AppKit, SwiftUI, Jetpack Compose). Requires FFI strategy.
- **`ui.list` virtualization**: High-performance scrolling for large datasets — only render visible items.
- **`ui.image` and `ui.shape`**: Content primitives for media and custom drawing. Terminal would need sixel/kitty graphics protocol support; web maps to `<img>` and `<svg>`.

## 6. Success Metrics

- **Phase 1**: `CounterView(42 to :count)` returns a UiNode JSON tree with correct structure and props
- **Phase 2**: Counter app renders live in the terminal with working button input
- **Phase 3**: State changes trigger automatic re-render without manual wiring
- **Phase 4**: Same counter app serves as an HTML page via `http.respond.html` with **zero changes** to `CounterView.fa`
- **Phase 5**: Adding `:color "blue"` to a text node works in both terminal and web renderers
- **Overall**: The `forai dev` debugger can visualize the UI tree and highlight which component emitted an event

## 7. Example: Counter Application

This example shows how a UI component acts as both a sink for state and a source for events, using declarative branching for event handling.

### A. The UI Component (`CounterView.fa`)
```fa
docs CounterView
  A simple counter display with increment and decrement buttons.
done

func CounterView
  take count as long
  emit on_inc as bool
  emit on_dec as bool
body
  ui.screen do
    ui.vstack(20 to :spacing) do
      ui.text("Current Count: #{count}" to :value, 24 to :size)
      
      ui.hstack(10 to :spacing) do
        ui.button("+") do
          v = true
          emit v to :on_inc
        done
        
        ui.button("-") do
          v = true
          emit v to :on_dec
        done
      done
    done
  done
done
```

### B. The State Manager (`CounterState.fa`)
```fa
docs CounterState
  Maintains the count and applies changes received via the delta port.
done

source CounterState
  take delta as long
  emit count as long
body
  state = 0
  emit state
  
  on :update from delta to change
    state = state + change
    emit state
  done
done
```

### C. The App Wiring (`main.fa`)
```fa
docs main
  Wires the UI and state together using explicit branching.
done

flow main
body
  # 1. The State Manager
  # Listens for values on 'delta_wire' and emits the 'count'
  step CounterState(delta_wire to :delta) then
    next :count to c
  done

  # 2. The View
  # Displays the count and emits boolean signals for clicks
  step CounterView(c to :count) then
    next :on_inc to inc
    next :on_dec to dec
  done

  # 3. Declarative Event Routing
  # These branches represent the 'cool wiring' part of the flowchart
  branch when inc == true
    val = 1
    # Routing the value 1 to the 'delta_wire'
    next val to delta_wire
  done

  branch when dec == true
    val = -1
    # Routing the value -1 to the 'delta_wire'
    next val to delta_wire
  done
done
```

## 8. Testing the UI and State

Testing is a first-class citizen in `forai`. The UI-IR (JSON Tree) makes it easy to verify the layout and interactions without a browser.

### A. Testing the UI Tree (`CounterView.fa`)
```fa
docs CounterView
  Verifies that the UI renders the correct state and contains expected buttons.
done

test CounterView
  it "renders the current count in the label"
    view = CounterView(42 to :count)
    # Path: screen -> vstack -> text
    text_node = view.children[0].children[0]
    must text_node.type == "text"
    must text_node.props.value == "Current Count: 42"
  done

  it "contains increment and decrement buttons"
    view = CounterView(0 to :count)
    # Path: screen -> vstack -> hstack
    buttons_row = view.children[0].children[1]
    must buttons_row.type == "hstack"
    must list.len(buttons_row.children) == 2
  done
done
```

### B. Testing the State Logic (`CounterState.fa`)
```fa
docs CounterState
  Verifies that the counter correctly handles delta updates.
done

test CounterState
  it "starts at zero"
    c = trap CounterState()
    must c == 0
  done

  it "increments the count"
    mock delta => 5
    c = CounterState()
    must c == 5
  done
done
```
