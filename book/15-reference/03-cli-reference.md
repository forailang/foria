# Chapter 15.3: CLI Reference

The `forai` binary provides five subcommands. This chapter documents each command's flags, behavior, and exit codes.

## forai compile

Compiles a `.fa` file to IR JSON.

```
forai compile <file.fa> [-o <out.json>] [--compact]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<file.fa>` | Yes | Path to the entry-point `.fa` file. All `uses` dependencies are loaded automatically. |

### Flags

| Flag | Description |
|------|-------------|
| `-o <out.json>` | Write IR to a file instead of stdout. |
| `--compact` | Write minified JSON (no whitespace). |

### Behavior

1. Lexes, parses, and compiles the entry file and all transitively imported modules.
2. Runs sema and typecheck.
3. Lowers to IR and serializes to JSON.
4. Writes a `docs/` folder alongside the output (or in the current directory if writing to stdout).

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Compilation succeeded. |
| `1` | Compilation failed (error written to stderr). |

### Examples

```bash
forai compile main.fa
forai compile main.fa -o dist/app.ir.json
forai compile main.fa --compact -o dist/app.ir.min.json
forai compile examples/factory/main.fa | jq '.nodes | length'
```

---

## forai run

Compiles and executes a `.fa` file.

```
forai run <file.fa> [args...] [--input <input.json>] [--report <file>]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<file.fa>` | Yes | Path to the entry-point `.fa` file. |
| `[args...]` | No | Additional string arguments passed to the program (accessible via `env.get`). |

### Flags

| Flag | Description |
|------|-------------|
| `--input <input.json>` | JSON file providing input values for the flow's `take` ports. If not provided, the flow starts with no inputs. |
| `--report <file>` | Write the execution trace (`RunReport`) to a JSON file after completion. The report includes all steps, variable bindings, and timing. |

### Behavior

1. Compiles the file (same as `forai compile` internally).
2. Executes the compiled IR with the given input.
3. All I/O ops run against the live environment (real HTTP, real DB, real filesystem).
4. Prints the final output (emitted value) to stdout on success.
5. Prints the failure value to stderr on fail, then exits with code 1.

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | Program completed (emitted successfully). |
| `1` | Program failed (emitted to fail port, or compilation failed). |

### Examples

```bash
forai run main.fa
forai run main.fa --input input.json
forai run main.fa --report run-report.json
forai run server.fa --input server.input.json --report report.json
```

### Input File Format

The input JSON file is a flat object mapping port names to values:

```json
{
  "user_id": "u-123",
  "limit": 10,
  "debug": false
}
```

---

## forai test

Runs test blocks from `.fa` files in a directory or file.

```
forai test [path]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `[path]` | No | Path to a directory or a single `.fa` file. Defaults to the current directory if omitted. |

### Behavior

1. Scans the given path for `.fa` files (non-recursive — does **not** descend into subdirectories).
2. For each `.fa` file, parses and runs all `test` blocks.
3. Each test block's `must` assertions are evaluated. Failures report `file:line:col: expression + resolved values`.
4. `mock` directives in test blocks substitute sub-func calls for the duration of that test.
5. `trap` assertions verify that a call emits to its `fail` port.
6. Uses `register_test_stubs()` for host functions (extern funcs), not the live implementations.

### Important: Non-Recursive

`forai test` does **not** recurse into subdirectories. To test a multi-directory project, run `forai test` for each directory separately:

```bash
forai test app/
forai test app/handler/
forai test app/sources/
forai test app/workflow/
```

### Exit Codes

| Code | Meaning |
|------|---------|
| `0` | All tests passed. |
| `1` | One or more tests failed (failures written to stderr). |

### Examples

```bash
forai test                         # test files in current directory
forai test app/                    # test files in app/
forai test app/handler/Process.fa  # test a single file
```

### Test Output Format

```
running tests in app/handler/Process.fa
  test ProcessItem: ok
  test ProcessItem/edge_case: FAILED
    app/handler/Process.fa:42:5: must result.status == "done" — got result.status = "pending"
running tests in app/handler/Route.fa
  test Route: ok
2 passed, 1 failed
```

---

## forai doc

Generates structured documentation for a module.

```
forai doc <path> [-o <out.json>]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<path>` | Yes | Path to a `.fa` file or directory to document. |

### Flags

| Flag | Description |
|------|-------------|
| `-o <out.json>` | Write documentation JSON to a file. If omitted, writes to stdout. |

### Behavior

Extracts all `docs` blocks and their associated declarations (funcs, types, enums, fields) and produces a structured JSON artifact. Does not run the full compiler pipeline — only parses and extracts docs.

### Output Format

```json
{
  "module": "handler",
  "path": "app/handler/",
  "funcs": [
    {
      "name": "ProcessItem",
      "description": "Processes a single item.",
      "ports": {
        "take": [{"name": "item", "type": "dict"}],
        "emit": [{"name": "result", "type": "ItemResult"}],
        "fail": [{"name": "error", "type": "text"}]
      }
    }
  ],
  "types": [
    {
      "name": "ItemResult",
      "description": "The result of processing an item.",
      "fields": [
        {"name": "id", "type": "text", "description": "The item ID."}
      ]
    }
  ]
}
```

### Examples

```bash
forai doc app/
forai doc app/ -o docs/api.json
forai doc app/handler/Process.fa
```

---

## forai dev

Starts the interactive WebSocket debugger.

```
forai dev <file.fa> [--input <input.json>] [--port <N>]
```

### Arguments

| Argument | Required | Description |
|----------|----------|-------------|
| `<file.fa>` | Yes | Path to the entry-point `.fa` file. |

### Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--input <input.json>` | auto | JSON file providing input values. If omitted, looks for `<stem>.input.json` in the same directory. |
| `--port <N>` | `481` | Port for the WebSocket debug server. The browser UI is served at `http://localhost:<N>`. |

### Behavior

1. Compiles the file.
2. Starts an async WebSocket server on the specified port.
3. Serves the browser debug UI at `http://localhost:<port>`.
4. Waits for a browser connection, then begins execution in paused state.
5. The browser controls execution via WebSocket messages (step, continue, breakpoints, restart).

### Exit

Press `Ctrl+C` to stop the dev server.

### Examples

```bash
forai dev main.fa
forai dev main.fa --input main.input.json
forai dev main.fa --port 9090
forai dev server/Start.fa --input start.input.json --port 481
```

---

## Global Behavior

### Error Format

All compiler and runtime errors use the format:

```
file.fa:line:col: error message
```

Errors are written to stderr. The program exits with code 1 on any error.

### Version

```bash
forai --version
```

Prints the forai compiler version string.

### Help

```bash
forai --help
forai compile --help
forai run --help
forai test --help
forai doc --help
forai dev --help
```
