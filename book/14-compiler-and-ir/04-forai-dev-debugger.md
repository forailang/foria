# Chapter 14.4: forai dev — Interactive Debugger

`forai dev` launches an interactive debugging session for a forai program. It compiles the program, starts execution, and serves a WebSocket-based debug UI in the browser. You can step through execution node by node, inspect variable bindings at each step, set breakpoints, and restart.

## Starting the Debugger

```bash
forai dev <file.fa> [--input input.json] [--port N]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--input <file>` | auto-detected | JSON file providing input values for the flow's `take` ports. |
| `--port <N>` | `481` | Port for the WebSocket debug server. |

Examples:

```bash
# Debug with auto-detected input file
forai dev examples/read-docs/main.fa

# Debug with explicit input
forai dev main.fa --input test.input.json

# Use a non-default port
forai dev main.fa --port 9090
```

After starting, the terminal prints:

```
forai dev: listening on ws://localhost:481
open http://localhost:481 in your browser
```

Open the URL in any browser to access the debug UI.

## Auto-Detecting Input

If `--input` is not provided, `forai dev` looks for a file named `<stem>.input.json` in the same directory as the `.fa` file. For `main.fa`, it looks for `main.input.json`. If found, it is used as the input; otherwise the flow is started with no inputs.

```bash
# main.fa and main.input.json exist in the same directory
forai dev main.fa    # automatically uses main.input.json
```

The input JSON format is a flat dict mapping port names to values:

```json
{
  "user_id": "u-123",
  "limit": 10
}
```

## The Debug UI

The browser UI is served from the same port as the WebSocket server. It shows:

### Source Code Panel

The source code of the compiled file, with the current execution position highlighted. As you step through, the highlighted line moves to the next statement being executed. Breakpoints appear as colored markers in the gutter.

### Graph Panel

An SVG visualization of the IR graph. Nodes are rendered as boxes labeled with their `op` name and `bind` variable. Edges are drawn as arrows. The currently executing node is highlighted. This panel helps you understand the dataflow structure at a glance.

### Variables Panel

The current variable bindings at the execution point. For each variable in scope, the panel shows its name, current value (formatted as JSON), and type. This panel updates after each step.

### Execution Trace

A scrollable log of all steps executed so far, in order:

```
→ n_0  db.open("app.db")                 conn = <db_conn:0>
→ n_1  db.exec(conn, "CREATE TABLE...")  ok = {rows_affected: 0}
→ n_2  route.match("/api/users", path)   match = null
→ n_3  [branch: when handler == "health"]
```

Each trace entry shows: the node ID, op name with args, and the bound variable name and value.

## Debug Protocol

The debug UI communicates with the runtime over WebSocket using a JSON message protocol:

| Message | Direction | Description |
|---------|-----------|-------------|
| `{"cmd": "step"}` | UI → runtime | Execute one node and pause. |
| `{"cmd": "continue"}` | UI → runtime | Run until next breakpoint or end. |
| `{"cmd": "run_to_breakpoint"}` | UI → runtime | Run until a specific node ID. |
| `{"cmd": "set_breakpoints", "ids": ["n_3", "n_7"]}` | UI → runtime | Set breakpoints at node IDs. |
| `{"cmd": "restart"}` | UI → runtime | Restart execution from the beginning with the same input. |
| `{"event": "paused", "node": "n_3", "bindings": {...}}` | runtime → UI | Execution paused; includes current bindings. |
| `{"event": "done", "output": {...}}` | runtime → UI | Execution completed; includes final output. |
| `{"event": "failed", "error": {...}}` | runtime → UI | Execution failed; includes error value. |

The UI handles these messages to update the highlighted node, variable panel, and trace log.

## Setting Breakpoints

In the source code panel, click the line number gutter to toggle a breakpoint. Breakpoints are associated with IR node IDs. When execution reaches a breakpointed node, it pauses and the UI updates.

You can also set breakpoints in the graph panel by clicking a node box.

## Stepping Through Execution

- **Step** (keyboard: `s` or the Step button): execute one IR node and pause. The source code panel advances to the next line.
- **Continue** (keyboard: `c` or the Continue button): run until the next breakpoint or end of program.
- **Restart** (keyboard: `r` or the Restart button): reset execution to the beginning with the same input.

## Inspecting Variables

The variables panel shows all locals in scope at the pause point. Values are shown formatted:

```
conn      <db_conn:0>
rows      [{"id": "u1", "name": "Alice"}, ...]
count     2
route     "users_list"
```

Complex values (lists, dicts) are expandable. Click to expand/collapse nested structures.

## Practical Workflow

A typical debugging session:

1. Start `forai dev main.fa`.
2. Open the browser at `http://localhost:481`.
3. Click **Step** a few times to advance through initialization.
4. When you reach a node of interest, inspect the Variables panel to check intermediate values.
5. Set a breakpoint at the suspect node, then click **Continue** to run to it quickly.
6. Inspect the variables at that point. If a value is wrong, note the upstream node that produced it.
7. Click **Restart** to reset and step through more carefully.

## Example: Debugging a Route Mismatch

Suppose requests are hitting the "not_found" branch when they should match "users_list". Start the debugger:

```bash
forai dev server/Start.fa --input start.input.json
```

Step through until the `route.match` node. Inspect `path` in the Variables panel — if it shows `/api/Users` (capital U) instead of `/api/users`, the route pattern doesn't match because `route.match` is case-sensitive.

The graph panel shows the `route.match` node, its edge to the guard node, and the guard's `when` condition. The Variables panel shows the match result as `null`, confirming the mismatch.

## Limitations

- The debugger runs a single execution from a fixed input. It does not simulate concurrent events or sources emitting multiple events.
- WebSocket connections from external clients are not live during a debug session (the port is used by the debugger).
- `send nowait` tasks run in the background during debugging but their execution is not stepped through — they appear in the trace log when they complete.
- The dev server does not hot-reload source changes. To reload, restart `forai dev`.
