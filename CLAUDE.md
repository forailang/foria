# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Core Principles

### The Pipeline Model

The system is a tree of pipelines rooted at `main`. The main flow branches into sub-flows, which branch further — a large application may have hundreds of sinks spread across the leaves and many sources scattered throughout the tree. Sources are the onramps (where events enter), funcs are the processing stations, sinks are the exits (where results leave), and flows define the highway connecting them.

```
source → flow → func → sink
(input)   (wire)  (compute)  (output)
```

Each branch of the tree is a pipeline. A pipeline is not a loop. If a branch has 5 steps, there can be 5 events in flight simultaneously — one at each stage. When the source emits event 1 into step 2, the source is already free to emit event 2 into step 1. Like an assembly line: the line doesn't stop while one car is being painted — the next car is already getting welded. Flow declarations describe the *shape* of the tree. The runtime fills it with concurrent events flowing through every branch.

### Design Rules

These are non-negotiable:

- **`main` is always a `flow`** — the entry point is declarative, never imperative
- **Sources are where data comes from** — event producers like listeners, pollers, and streams. Sources own the imperative "how to get events" (polling, sleeping, accepting connections). A source blocks until the next event, then yields it. The flow never sees the waiting
- **Flows are the glue** — they wire sources, funcs, and sinks together declaratively. Flows define *what connects to what*, not *how* things are computed. No loops, no computation, no I/O — just steps that name the pipeline stages. A step calling a source means "events enter here"; the runtime handles the repetition
- **Funcs are the building blocks** — imperative code lives here: ops, `case/when`, `loop`, `sync`, string interpolation. Funcs do the actual work
- **Sinks are where data goes** — terminal side effects like `term.print`, `term.prompt`, file writes, HTTP responses. Think of sinks as the UI/output layer of the stack
- **All source/func/flow/sink are async and awaited by default** — each pipeline stage awaits its input, does its work, passes the result downstream. Backpressure is natural — if a stage is slow, upstream waits

## What This Is

`forai` is a text-first forai DSL compiler and runtime written in Rust. It compiles `.fa` source files into a graph IR (JSON), executes them with trace capture, and serves an interactive debug web UI. The binary is called `forai`.

## Build & Test

```bash
cargo build                                              # build
cargo test                                               # Rust unit tests + example round-trip test
cargo run -- compile examples/read-docs/main.fa              # compile .fa → IR JSON (stdout)
cargo run -- compile examples/read-docs/main.fa -o out.json  # compile .fa → file (--compact for minified)
cargo run -- run examples/read-docs/main.fa                  # execute flow (interactive docs browser)
cargo run -- test examples/read-docs/                        # run .fa test blocks in a dir/file
cargo run -- dev examples/read-docs/main.fa                  # launch interactive debug server (WebSocket UI)
cargo run -- doc examples/read-docs/main.fa                  # generate structured docs artifact (JSON)
```

Rust edition is 2024. The runtime is fully async using `tokio` (current_thread). Dependencies: `regex`, `serde`, `serde_json`, `tokio` (async runtime), `reqwest` (async HTTP client), `tokio-tungstenite` (async WebSocket), `tungstenite` (debugger WebSocket), `futures`, `crossterm` (terminal I/O), `rusqlite` (SQLite).

## Architecture

The compiler pipeline is a linear chain through these modules:

```
.fa source → lexer → parser → AST → types → sema → typecheck → IR → runtime
```

| Module | File | Role |
|--------|------|------|
| **lexer** | `src/lexer.rs` | Tokenizes `.fa` source into `Token` stream (`Ident`, `Number`, `StringLit`, `StringInterp`, `Symbol`, `FatArrow`, `Newline`) |
| **parser** | `src/parser.rs` | Token-stream parser producing `ModuleAst` (top-level decls). Second pass converts `FuncDecl` body text into executable `Flow` via `parse_runtime_func_decl_v1`. Flow (step-based) bodies are parsed by `parse_flow_graph_decl_v1` |
| **ast** | `src/ast.rs` | All AST types: `ModuleAst`, `TopDecl` (Uses/Docs/Func/Sink/Flow/Type/Data/Enum/Test), `Statement` (Node/ExprAssign/Emit/Case/Loop/Sync), `Flow` (runtime form), `FlowGraph` (step-based form) |
| **types** | `src/types.rs` | `TypeRegistry` built from module AST. Defines `TypeDef` (Primitive/Scalar/Struct/Enum), `PrimitiveType` (Text/Bool/Long/Real/Uuid/Time/List/Dict/Void/DbConn/HttpServer/HttpConn/WsConn), constraint resolution and validation. Built-in struct types: `HttpRequest`, `HttpResponse`, `Date`, `Stamp`, `TimeRange`, `ProcessOutput`, `WebSocketMessage`, `ErrorObject`, `URLParts` |
| **op_types** | `src/op_types.rs` | Static op signature registry for handle-producing/consuming ops and struct-returning ops. Maps ops to their argument and return types including named struct types (`ProcessOutput`, `Date`, `HttpResponse`, etc.) |
| **typecheck** | `src/typecheck.rs` | Forward type-inference pass over func bodies. Validates that handle-consuming ops receive the correct handle type at compile time |
| **sema** | `src/sema.rs` | Semantic validation: enforces `emit`/`fail` presence on funcs/sinks (flows may omit them), docs coverage for funcs and tests, duplicate docs detection |
| **ir** | `src/ir.rs` | Lowers `Flow` → `Ir` (normalized nodes, edges, emits with guard conditions) |
| **codec** | `src/codec.rs` | `Codec` trait and `CodecRegistry` for pluggable serialization formats. Built-in `JsonCodec`; auto-generates `<format>.decode`/`encode`/`encode_pretty` ops |
| **runtime** | `src/runtime.rs` | Async execution engine (`tokio` current_thread). Executes IR against inputs, produces `RunReport` with trace events. All I/O ops (HTTP, file, WebSocket, process) are non-blocking. `sync` blocks run statements concurrently via `join_all`. Built-in ops across 16+ namespaces (see below). Sub-flow dispatch via `FlowRegistry` with value mock support |
| **deps** | `src/deps/` | External dependency system: `semver.rs` (version parsing/ranges), `source.rs` (dep source types: GitHub/File/Git), `fetch.rs` (git-based fetching/cache), `lockfile.rs` (forai.lock management), `resolve.rs` (dependency resolution orchestrator) |
| **loader** | `src/loader.rs` | Module loader: resolves `uses` declarations and `@`-prefixed package imports, compiles funcs into `FlowProgram` entries, builds `FlowRegistry` for cross-module dispatch |
| **tester** | `src/tester.rs` | Parses and runs `test` blocks from `.fa` files. Supports `must` assertions, `trap` for failure-path testing, `mock` for substituting sub-func calls, variable bindings |
| **debugger** | `src/debugger.rs` | WebSocket-based interactive debugger for the `dev` command. Step/continue/breakpoint/restart protocol with embedded HTML UI |
| **doc** | `src/doc.rs` | Extracts structured documentation from modules for the `doc` command |
| **cli** | `src/cli.rs` | Argument parsing for `compile`, `run`, `test`, `doc`, `dev` subcommands |

Key design note: the parser has two layers. First it parses the full module structure (`parse_module_v1` → `ModuleAst`), then it re-parses func body text into executable `Statement` nodes (`parse_runtime_func_decl_v1`). This two-pass approach means the body is stored as raw text in `FuncDecl.body_text` before being parsed into statements.

## Built-in Runtime Ops

| Namespace | Ops | Description |
|-----------|-----|-------------|
| `obj.*` | `new`, `set`, `get`, `has`, `delete`, `keys`, `merge` | Immutable dict operations |
| `list.*` | `new`, `range`, `append`, `len`, `contains`, `slice`, `indices` | Immutable list operations; access via `items[0]` bracket indexing |
| `str.*` | `len`, `upper`, `lower`, `trim`, `trim_start`, `trim_end`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `slice`, `index_of`, `repeat` | String operations |
| `math.*` | `floor`, `round` | Rounding (arithmetic uses infix operators: `+` `-` `*` `/` `%` `**`) |
| `type.*` | `of` | Type introspection — returns `"text"`, `"bool"`, `"long"`, `"real"`, `"list"`, `"dict"`, `"void"` |
| `to.*` | `text`, `long`, `real`, `bool` | Type conversion between scalars |
| `json.*` | `decode`, `encode`, `encode_pretty` | JSON codec (via codec registry) |
| `codec.*` | `decode`, `encode`, `encode_pretty` | Generic codec dispatch by format name |
| `http.*` | `extract_path`, `extract_params`, `response`, `error_response`, `get`, `post`, `put`, `patch`, `delete`, `request` | HTTP client and response helpers |
| `http.server.*` | `listen`, `accept`, `respond`, `close` | HTTP server (uses `http_server`/`http_conn` handle types) |
| `http.respond.*` | `html`, `json`, `text` | HTTP response convenience ops (auto content-type, uses `http_conn`) |
| `ws.*` | `connect`, `send`, `recv`, `close` | WebSocket client (uses `ws_conn` handle type) |
| `headers.*` | `new`, `set`, `get`, `delete` | HTTP header utilities |
| `auth.*` | `extract_email`, `extract_password`, `validate_email`, `validate_password`, `verify_password`, `sample_checks`, `pass_through` | Auth simulation |
| `db.*` | `open`, `exec`, `query`, `close`, `query_user_by_email`, `query_credentials` | SQLite database ops (uses `db_conn` handle type; plus legacy simulation) |
| `date.*` | `now`, `from_parts`, `from_iso`, `to_parts`, `to_iso`, `add`, `diff`, `weekday`, etc. | Calendar date operations |
| `stamp.*` | `now`, `from_ns`, `to_ms`, `to_date`, `add`, `diff`, etc. | Monotonic timestamp operations |
| `trange.*` | `new`, `start`, `end`, `duration_ms`, `contains`, `overlaps`, `shift` | Time range operations |
| `file.*` | `read`, `write`, `append`, `delete`, `exists`, `list`, `mkdir`, `copy`, `move`, `size`, `is_dir` | File I/O |
| `term.*` | `print`, `prompt`, `clear`, `size`, `cursor`, `move_to`, `color`, `read_key` | Terminal I/O |
| `time.*` | `split_hms` | Time utilities |
| `fmt.*` | `pad_hms`, `wrap_field` | Formatting helpers |
| `env.*` | `get`, `set`, `has`, `list`, `remove` | Environment variables |
| `exec.*` | `run` | Process/command execution |
| `regex.*` | `match`, `find`, `find_all`, `replace`, `replace_all`, `split` | Regular expressions |
| `random.*` | `int`, `float`, `uuid`, `choice`, `shuffle` | Random & UUID generation |
| `hash.*` | `sha256`, `sha512`, `hmac` | Cryptographic hash digests |
| `base64.*` | `encode`, `decode`, `encode_url`, `decode_url` | Base64 encoding/decoding |
| `crypto.*` | `hash_password`, `verify_password`, `sign_token`, `verify_token`, `random_bytes` | Bcrypt, JWT, secure random |
| `log.*` | `debug`, `info`, `warn`, `error`, `trace` | Level-based logging to stderr with timestamps |
| `error.*` | `new`, `wrap`, `code`, `message` | Structured error construction and inspection |
| `cookie.*` | `parse`, `get`, `set`, `delete` | HTTP cookie parsing, construction, and deletion |
| `url.*` | `parse`, `query_parse`, `encode`, `decode` | URL parsing, query strings, percent-encoding |
| `route.*` | `match` | URL path pattern matching (`:param`, `*wildcard`) |
| `html.*` | `escape`, `unescape` | HTML entity escaping and unescaping |
| `tmpl.*` | `render` | Mustache-style template rendering |

Expressions in func bodies also support infix operators: `+` `-` `*` `/` `%` `**` `==` `!=` `<` `>` `<=` `>=` `&&` `||` and unary `!` `-`. String concatenation uses `+`. Bracket indexing: `items[0]`, `items[-1]`, `row["key"]`.

## The `.fa` Language (v1 Syntax)

Source files use `.fa` extension. Comments start with `#`. Key constructs:

- **func**: `func Name` with `take`/`emit`/`fail` header, `body`...`done` — leaf computation with imperative body
- **source**: `source Name` with `emit`/`fail` header, `body`...`done` — event producer. Body uses `on :tag from op(args) to var`...`done` blocks for event-driven patterns, or `loop` for polling patterns
- **on**: `on :eventType from sourceExpr(args) to varName`...`done` — event handler block inside source bodies. Runs body once per event from the source expression. Event tag (`:request`, `:input`) is stored but cosmetic in v1
- **sink**: `sink Name` — same syntax as `func`, declared as a side-effect-only endpoint
- **flow**: `flow Name` with optional `take`/`emit`/`fail` header, `body`...`done` — step-based wiring of funcs/flows; body contains `step`, `branch`, `emit`/`fail` blocks. `branch when <expr>` is a conditional sub-pipeline; `branch` (unguarded) always runs. Flows may have zero ports (no take/emit/fail) — they are pure wiring
- **uses**: `uses module` — imports a module directory; call as `module.FuncName(...)`. External package imports use `use Name from "@user/repo"` where the path matches a key in `forai.json` `dependencies`
- **docs**: `docs Identifier`...`done` — required for every func, flow, sink, and test
- **test**: `test Name`...`done` with `must` assertions, `trap` for failure paths, `mock` for substituting sub-func calls
- **if/else if/else/done**: boolean conditional branching; condition is a full expression. Desugars to `case` at parse time
- **case/when/else/done**: pattern matching; `when` accepts literals (`"text"`, `42`, `true`) and idents
- **loop expr as item**...`done`: iteration over lists
- **sync**: `[vars] = sync [:opts]`...`done [exports]` — runs body statements concurrently (`join_all`); each statement gets its own scope, so statements inside a sync block must be independent (no cross-references). Exports merge results back. Options: `:timeout`, `:retry`, `:safe`
- **nowait / send nowait**: `nowait op(args)` for built-in ops, `send nowait Step(args)` for triggering a step (func/flow) — fire-and-forget async call. Starts the target as a background task and continues immediately. No result is captured. Errors are logged to stderr, not propagated
- **type/data/enum**: type declarations with validation options (`:matches`, `:min`, `:required`); `open` modifier for extensible types
- **mock**: `mock module.Func => expr` inside test blocks — substitutes sub-func calls with fixed values
- **string interpolation**: `"Hello #{name}!"` — expressions in `#{}` are evaluated; `\n` `\t` `\\` `\"` `\#` escapes supported. Bare `{` `}` are literal (safe for regex quantifiers like `{4}`)

### IR lowerer scoping rules

The IR lowerer (`ir.rs`) tracks variable scope with these constraints:
- Variables assigned inside `case` arms are **not visible** outside the case block (the arm's scope is discarded)
- Variables assigned inside `loop` bodies can reassign outer variables (the `in_loop` flag permits this)
- **Pattern**: initialize variables before a `case` block if they need to be used after it (e.g., `val = default` before the case, then reassign in each arm)

Full grammar is in `spec-v1.md`. The `desired.md` file contains the broader language vision document.

## Examples

All examples live under `examples/` as project directories:

| Example | Description |
|---------|-------------|
| `read-docs/` | Interactive stdlib documentation browser — source/func/flow/sink with `uses`, `branch when`, `case`, `loop`, nested modules, `term.prompt` |
| `web-simple/` | Web server with route.match-based routing, parameterized URLs (`/blog/:slug`), blog pages, HTML templating |

## Migration Status

The project is following `language-migration-plan.md` through 10 phases. Phases 0–5 are complete:
- Phases 0–2: Parser foundation, core syntax (`func`, `flow`, `docs`, `test`, `case`, `loop`, `sync`, `type`, `data`, `enum`, `uses`)
- Phase 3: Type system v1 (scalar/struct/enum types, constraints, input validation)
- Phase 4: Semantic rules (docs enforcement, output type validation)
- Phase 5: Module system (`uses` resolution, cross-module calls), func/flow split, test mocking

Additionally implemented: interactive debugger (`dev` command), doc generation (`doc` command), codec registry, type conversion ops, async runtime migration (all I/O non-blocking, sync blocks concurrent), external dependency system (semver ranges, GitHub/file/git URL sources, caching, lockfile, transitive deps).

Remaining: flow (step-based) runtime execution, generated docs artifacts.

## IR Shape

The compiled IR JSON contains:
- `forai_dataflow`: version string (e.g. `"0.1"`)
- `flow`: the flow name
- `inputs`: list of `{name, type}` — input port declarations
- `outputs`: list of `{name, type}` — output port declarations
- `nodes`: list of `{id, op, bind, args, when}` — each node is a computation step
- `edges`: list of `{from: {kind, id, port?}, to: {kind, id, port?}, when}` — graph wiring with guard conditions
- `emits`: list of `{output, value_var, when}` — output routing decisions with guards
