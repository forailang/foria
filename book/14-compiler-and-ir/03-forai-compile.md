# Chapter 14.3: forai compile

`forai compile` is the command that runs the full compiler pipeline on a `.fa` file and produces the IR JSON. It is the primary tool for inspecting what the compiler produces, debugging compilation errors, and feeding IR to external tooling.

## Basic Usage

```bash
forai compile <file.fa>
```

Compiles `file.fa` and writes the IR JSON to stdout. The compiler also loads all modules referenced by `uses` declarations in the file and its transitive dependencies.

```bash
forai compile examples/read-docs/main.fa
```

Output: JSON IR on stdout, formatted with 2-space indentation.

## Options

| Flag | Description |
|------|-------------|
| `-o <out.json>` | Write IR to a file instead of stdout. |
| `--compact` | Write minified JSON (no whitespace). Useful for smaller output files or piping to tools. |

### Writing to a File

```bash
forai compile examples/factory/main.fa -o /tmp/factory-ir.json
```

Creates `/tmp/factory-ir.json` with the formatted IR. If the file exists, it is overwritten.

### Minified Output

```bash
forai compile examples/factory/main.fa --compact -o /tmp/factory-ir.min.json
```

The `--compact` flag produces a single-line JSON without whitespace. The schema is identical to formatted output; only the whitespace differs.

### Combining Flags

```bash
forai compile examples/read-docs/main.fa --compact -o out.json
```

Both flags can be used together.

## What Compilation Does

When you run `forai compile`:

1. **Lexes and parses** the entry `.fa` file and all transitively imported modules.
2. **Builds the type registry** from `type`, `data`, and `enum` declarations.
3. **Runs sema** — enforces docs coverage, port contracts, and other semantic rules.
4. **Runs typecheck** — infers and validates handle types across all funcs.
5. **Lowers to IR** — converts func bodies and flow steps to the normalized graph representation.
6. **Serializes to JSON** — writes the IR to stdout or a file.
7. **Generates `docs/`** — writes a structured documentation artifact to a `docs/` folder beside the output.

If any stage produces an error, compilation stops and the error is written to stderr with file, line, and column information. No IR is produced.

## The docs/ Side Effect

Every `forai compile` run generates a `docs/` folder. If you compile to `-o out.json`, the docs folder is written alongside `out.json`. If you compile to stdout, the docs folder is written to the current directory.

The docs folder contains one JSON file per module, structured like:

```json
{
  "module": "main",
  "funcs": [
    {
      "name": "main",
      "description": "Entry point for the application.",
      "ports": {
        "take": [],
        "emit": [{ "name": "result", "type": "ServerResult" }],
        "fail": [{ "name": "error", "type": "ServerError" }]
      }
    }
  ],
  "types": [ ... ]
}
```

This is used by the `forai doc` command and IDE tooling.

## Inspecting Compiled Output

Use `jq` to query specific parts of the IR:

```bash
# Count nodes
forai compile examples/factory/main.fa | jq '.nodes | length'

# List all ops used
forai compile examples/factory/main.fa | jq '[.nodes[].op] | unique'

# Find all DB operations
forai compile examples/factory/main.fa | jq '.nodes[] | select(.op | startswith("db."))'

# Find guarded edges
forai compile examples/factory/main.fa | jq '.edges[] | select(.when != null)'

# List all output ports
forai compile examples/factory/main.fa | jq '.outputs[].name'
```

## Compiling a Module Directory

`forai compile` takes a single `.fa` file as its entry point, but it automatically loads all transitively imported modules. To compile an entire application, compile `main.fa`:

```bash
forai compile app/main.fa -o dist/app.ir.json
```

The compiler follows `uses` declarations and loads every referenced module. The output IR represents the full transitive closure of the program.

## Compilation Errors

Errors are written to stderr in the format `file:line:col: error message`. Examples:

```
examples/main.fa:5:1: missing docs block for func `ProcessItem`
examples/main.fa:12:5: func `GetUser` has no emit output
examples/main.fa:18:9: handle type mismatch: expected db_conn, got http_server
examples/auth/Login.fa:3:1: name mismatch: file is `Login.fa` but func declares `Signin`
```

The exit code is non-zero on any compilation error. Scripts can check `$?` to detect failures:

```bash
forai compile main.fa -o out.json
if [ $? -ne 0 ]; then
    echo "Compilation failed"
    exit 1
fi
```

## Integration with CI

In a CI pipeline, compile to verify source is correct:

```bash
# Verify compilation succeeds, discard output
forai compile main.fa > /dev/null
```

Or compile and store the IR as a build artifact:

```bash
forai compile main.fa --compact -o dist/app.ir.json
```

## forai compile vs forai run

`forai compile` only produces the IR — it does not execute the program. Use `forai run` to execute. The two commands are intentionally separate so you can inspect the IR before running, store IR artifacts, and run the same IR against different inputs.

```bash
# Compile once
forai compile main.fa -o app.ir.json

# Run (forai run re-compiles internally; -o is for external tooling use)
forai run main.fa --input input.json
```

Note: `forai run` does not take a pre-compiled IR file as input — it takes the `.fa` source and compiles internally before running. The `-o` flag on `forai compile` is for external tooling that wants to inspect or cache the IR.
