# forai

A text-first DSL compiler and runtime for building concurrent pipeline systems. Source is textual and deterministic — visualization and runtime behavior are derived from a canonical graph IR.

## Why text-first

Visual-first graph systems are hard to diff, merge, and review at scale. `forai` keeps source code textual, then derives everything else from it.

## The pipeline model

The system is a tree of pipelines rooted at `main`. Sources are where events enter, funcs do the computation, sinks are the exits, and flows wire them together.

```
source → flow → func → sink
(input)  (wire)  (compute) (output)
```

Each branch of the tree is a concurrent pipeline — like an assembly line where multiple events can be in flight simultaneously across stages.

## Build & run

Requires Rust (edition 2024).

```bash
cargo build                                              # build
cargo test                                               # Rust unit tests

cargo run -- compile examples/read-docs/main.fa              # compile .fa → IR JSON (stdout)
cargo run -- compile examples/read-docs/main.fa -o out.json  # compile .fa → file
cargo run -- run examples/read-docs/main.fa                  # execute flow
cargo run -- test examples/read-docs/                        # run .fa test blocks
cargo run -- dev examples/read-docs/main.fa                  # interactive debug server
cargo run -- doc examples/read-docs/main.fa                  # generate docs artifact (JSON)
```

## Language syntax (`.fa`)

```
# A func does computation
func Greet
  take name as Text
  emit greeting as Text
body
  greeting = str.join("Hello ", name)
  emit greeting
done

# A flow wires things together
flow Main
body
  step GetName
  step Greet
done
```

Key constructs:

| Construct | Description |
|-----------|-------------|
| `func` | Leaf computation — imperative body with `take`/`emit`/`fail` ports |
| `flow` | Declarative wiring — `step`, `branch when <expr>`, `emit`/`fail` |
| `source` | Event producer — blocks until next event, yields it to the flow |
| `sink` | Terminal side effect — like `func` but output-only |
| `case/when/else/done` | Pattern matching |
| `if/else if/else/done` | Boolean branching (desugars to `case`) |
| `loop expr as item` | List iteration |
| `sync [...] = sync` | Concurrent statement block (`join_all`) |
| `nowait op(...)` | Fire-and-forget async call |
| `type`/`data`/`enum` | Type declarations with validation constraints |
| `test` | Test blocks with `must`, `trap`, `mock` |
| `uses module` | Import a module directory |
| `docs` | Required documentation blocks |

String interpolation: `"Hello #{name}!"`. Infix operators: `+ - * / % ** == != < > <= >= && ||` and unary `! -`.

See [`spec-v1.md`](spec-v1.md) for the full grammar and [`LANGUAGE.md`](LANGUAGE.md) for the language vision.

## Project structure

```
crates/
  forai/           # CLI binary (compile, run, test, doc, dev)
  forai-core/      # Compiler + runtime library
  forai-wasm/      # WASM build target
browser/           # Browser runtime (TypeScript/WASM)
editors/
  vscode/          # VS Code syntax extension
examples/
  read-docs/       # Interactive stdlib docs browser
  factory/         # Factory manager web app (HTTP, SQLite, multi-module)
  pipeline/        # Pipeline demo
  browser-demo/    # Browser runtime demo
  wasm-test/       # WASM target test
```

## Compiler pipeline

```
.fa source → lexer → parser → AST → types → sema → typecheck → IR → runtime
```

## IR shape

The compiled IR JSON contains:

- `forai_dataflow`: version string
- `flow`: flow name
- `inputs` / `outputs`: port declarations
- `nodes`: computation steps — `{id, op, bind, args, when}`
- `edges`: graph wiring — `{from, to, when}`
- `emits`: output routing — `{output, value_var, when}`

## Runtime

Fully async using `tokio` (current_thread). All I/O (HTTP, file, WebSocket, process) is non-blocking. `sync` blocks run concurrently via `join_all`. Built-in ops cover: `obj`, `list`, `str`, `math`, `json`, `http`, `db`, `file`, `term`, `crypto`, `date`, `regex`, `log`, `env`, and more.
