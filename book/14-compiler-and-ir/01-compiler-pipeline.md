# Chapter 14.1: The Compiler Pipeline

The forai compiler transforms `.fa` source files into an executable IR (intermediate representation) through a linear sequence of stages. Each stage takes the output of the previous and produces a more refined representation. Understanding the pipeline helps you interpret error messages, contribute to the compiler, and debug unusual behavior.

## Pipeline Overview

```
.fa source text
     ↓
  [Lexer]          → Token stream
     ↓
  [Parser]         → ModuleAst (top-level declarations)
     ↓   (second pass)
  [Parser]         → Flow (executable func bodies parsed from body_text)
     ↓
  [Types]          → TypeRegistry (struct/enum/scalar definitions)
     ↓
  [Sema]           → Semantic validation (docs, ports, coverage)
     ↓
  [Typecheck]      → Handle type inference and validation
     ↓
  [IR Lowerer]     → Ir (normalized nodes, edges, emits)
     ↓
  [Runtime]        → Execution + RunReport
```

## Stage 1: Lexer (`src/lexer.rs`)

The lexer tokenizes the raw `.fa` source text into a flat stream of tokens. It recognizes:

- **Ident** — identifiers: `func`, `flow`, `name`, `my_var`
- **Number** — integer and float literals: `42`, `3.14`
- **StringLit** — plain string literals: `"hello"`
- **StringInterp** — strings with `#{}` interpolation: `"Hello #{name}!"`
- **RegexLit** — regex literals: `/[a-z]+/`
- **Symbol** — single-character punctuation: `(`, `)`, `[`, `]`, `:`, `,`, `.`, `+`, `-`, `*`, `/`, `=`, `!`, `<`, `>`, `&`, `|`, `{`, `}`, `?`
- **FatArrow** — `=>`
- **EqEq** — `==`
- **BangEq** — `!=`
- **GtEq** — `>=`
- **LtEq** — `<=`
- **Newline** — significant newlines (forai is newline-sensitive)

Keywords are recognized as `Ident` tokens at the lexer stage; the parser distinguishes them by value.

Comments start with `#` and run to end of line. The lexer strips them before producing the token stream.

## Stage 2: Parser (`src/parser.rs`) — Two Passes

The parser is unique: it makes two passes over the source.

**First pass** (`parse_module_v1`): reads the full `.fa` file and produces a `ModuleAst` — a list of top-level declarations: `Uses`, `Docs`, `Func`, `Sink`, `Flow`, `Type`, `Data`, `Enum`, `Test`. During this pass, func bodies are stored as **raw text** in `FuncDecl.body_text` rather than being fully parsed. This allows the parser to handle forward references and resolve imports before fully parsing bodies.

**Second pass** (`parse_runtime_func_decl_v1`): after all modules are loaded, each func's `body_text` is re-parsed into an executable `Flow` (list of `Statement` nodes). This second pass has full access to the module's type and import context.

Flow (step-based) bodies use `parse_flow_graph_decl_v1`, which parses `step`, `branch`, `emit`, `fail`, and `state` declarations into a `FlowGraph`.

This two-pass approach means that:
- A func can call another func defined later in the same module.
- The body is validated with full context (imports resolved, types known).
- Parse errors in func bodies are attributed to the correct line numbers within the body text.

## Stage 3: Types (`src/types.rs`)

The `TypeRegistry` is built from the `ModuleAst`. It collects:

- **Primitive types** — `text`, `bool`, `long`, `real`, `uuid`, `time`, `list`, `dict`, `void`, `db_conn`, `http_server`, `http_conn`, `ws_conn`
- **Scalar types** — named aliases for primitives (e.g. `type Email text`)
- **Struct types** — named records with fields (e.g. `type User / name text / email text`)
- **Enum types** — named discriminated unions

Type validation is applied at struct construction: required fields, pattern constraints (`:matches`), and range constraints (`:min`, `:max`) are checked when values are assigned.

The registry is used by the typecheck stage to validate handle types and by the sema stage to enforce port contracts.

## Stage 4: Sema (`src/sema.rs`)

The semantic analysis stage enforces high-level language rules that cannot be checked at parse time:

- Every `func` and `sink` must have at least one `emit` and one `fail` in its body.
- Every `func`, `flow`, `sink`, and `test` must have a corresponding `docs` block.
- Every `docs` block must correspond to an actual declaration (no orphan docs).
- Extern funcs are exempt from body requirements and test requirements.
- Flow bodies may omit `emit`/`fail` — they are pure wiring.

Sema errors include file, line, and column information.

## Stage 5: Typecheck (`src/typecheck.rs`)

The typecheck stage performs a forward type-inference pass over func bodies. Its primary job is validating **handle types**:

- When `db.open(path)` is called, the result is typed as `db_conn`.
- When `db.query(conn, sql)` is called, `conn` must be of type `db_conn`. If it is an `http_server`, the compiler reports a handle type mismatch error.
- The typecheck stage also validates that funcs receiving handle-typed arguments (`take conn as db_conn`) are called with the correct handle type.

Handle types are tracked in a `local_types` map from variable name to type. The `infer_expr_type` helper resolves variable types, field accesses on struct-typed variables, and unwrapped `Result`/`Optional` types.

## Stage 6: IR Lowerer (`src/ir.rs`)

The IR lowerer converts the parsed AST into a normalized graph representation (the `Ir` struct). This is the format written to JSON by `forai compile`.

Key transformations:
- Each statement becomes a node with an `op`, optional `bind` (output variable), `args`, and `when` guard condition.
- `case/when` branches become nodes with `when` conditions.
- `loop` becomes a loop node with edges back to the start.
- `emit` and `fail` become `emits` entries in the IR.
- Variables assigned inside `case` arms are **scoped to the arm** — they are not visible outside the case block. This is a key IR lowerer rule.
- Variables can be reassigned inside `loop` bodies (the `in_loop` flag permits this).

## Stage 7: Runtime (`src/runtime.rs`)

The runtime executes the IR. It is async, built on `tokio` (current-thread executor). All I/O ops are non-blocking under the hood.

Execution produces a `RunReport` containing:
- The emitted output value (for `emit`).
- The failed error value (for `fail`).
- A trace of events with timestamps, op names, variable bindings, and timing.

The trace is used by the `--report` flag and the `dev` debugger.

## Module Loading (`src/loader.rs`)

Before compilation, the loader resolves `uses` declarations:

1. Each `uses module` is resolved relative to the importing file's directory.
2. Modules are loaded recursively: if `A.fa` uses `b` and `b/` contains `C.fa` which uses `d`, all of `A`, `C`, and `d` are loaded.
3. The loader detects circular dependencies and reports an error.
4. All loaded funcs are compiled into a `FlowRegistry` for cross-module dispatch.

## Error Attribution

Every compiler error includes `file:line:column`. The lexer tracks position as it tokenizes, and each token carries its source position. The parser propagates positions into AST nodes. The sema, typecheck, and IR stages report positions from the AST nodes they operate on.

Parse errors in func bodies are attributed to positions within `body_text`, which the second-pass parser converts to absolute file positions.
