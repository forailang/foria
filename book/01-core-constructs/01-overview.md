# Core Constructs Overview

forai has exactly four constructs. Every `.fa` file contains one of them. This chapter explains the role of each and provides guidance on which to use when.

## The Four Constructs

| Construct | Keyword | Role | Contains I/O? | Contains computation? |
|-----------|---------|------|--------------|----------------------|
| Source    | `source` | Event producer | Yes (event loop) | Minimal |
| Func      | `func`   | Computation unit | No | Yes |
| Flow      | `flow`   | Wiring diagram | No | No |
| Sink      | `sink`   | Terminal output | Yes (side effects) | Minimal |

### Source → Func → Sink: The Data Path

Data always flows in one direction: from sources through funcs to sinks. A source emits events. Those events travel through one or more funcs that transform them. Eventually the transformed data reaches a sink that consumes it — printing it, writing it, sending it over a network.

```
source.Events → func.Validate → func.Enrich → sink.Store
```

### Flow: The Wiring Layer

Flows sit above the data path. They declare how sources, funcs, and sinks connect. A flow does not touch data; it describes the shape of the assembly line. The main entry point of every forai program is a flow.

```
flow main
  step source.Events → func.Validate → func.Enrich → sink.Store
```

(The above is conceptual; see Chapter 1.4 for actual flow syntax.)

## When to Use Each Construct

### Use a `source` when:
- You need to produce a stream of events from an external system (terminal input, network socket, timer, file watcher)
- The event loop logic (polling, accepting connections, reading from a queue) belongs in one place
- The downstream pipeline should not be aware of how events are obtained

### Use a `func` when:
- You have computation to perform: string manipulation, arithmetic, conditional logic, data transformation
- You need to call built-in ops or other funcs and combine their results
- The operation may succeed or fail, and you want to express that with explicit ports

### Use a `flow` when:
- You need to connect multiple stages into a pipeline
- You need to route events to different sub-pipelines based on a condition (`branch when`)
- You are writing the entry point (`main`) or a named sub-pipeline

### Use a `sink` when:
- The callable performs terminal side effects: printing, writing to a database, sending an HTTP response
- The result is not meant to be consumed by further pipeline stages
- You want to document that this is the "exit point" of a pipeline branch

## The Relationship Between Constructs

The constructs form a hierarchy:

1. **Flows** are at the top. They wire everything together.
2. **Sources** feed data into flows. They are called in flow `step` declarations.
3. **Funcs** transform data. They are called in flow `step` declarations and from other funcs.
4. **Sinks** consume data. They are called in flow `step` declarations, always at the end of a branch.

A flow can call other flows. This is how large applications are structured: a `main` flow calls sub-flows, which call further sub-flows, with sources, funcs, and sinks at the leaves.

```
flow main
  → flow app.Start
      → source sources.Commands
      → func router.Classify
      → flow branches...
          → sink display.Print
```

## The One-Callable Rule

Each `.fa` file contains exactly one callable (one func, flow, sink, or source). A file may also contain any number of:

- `type` or `data` declarations (struct types)
- `enum` declarations
- `docs` blocks
- `test` blocks
- `use` declarations (imports)

But only one callable. This keeps files small and focused. When you want to know where `router.Classify` is implemented, you look in `router/Classify.fa`. There is no ambiguity.

## Naming Conventions

- Callable names use `PascalCase`: `Validate`, `ProcessOrder`, `HttpServer`
- Type names use `PascalCase`: `User`, `OrderStatus`, `EmailResult`
- Variable names inside bodies use `snake_case`: `user_id`, `raw_input`, `is_valid`
- Module directories use `lowercase`: `router/`, `sources/`, `display/`
- Port names in `take`/`emit`/`fail` use `snake_case`: `take cmd as text`, `emit result as bool`

The compiler does not enforce these conventions, but they are consistent with all standard library examples and existing projects.

## Compile-Time Guarantees

The compiler enforces correctness properties across all four constructs:

- **Docs required.** Every callable and every test must have a `docs` block. Missing docs is a compile error.
- **Port completeness.** Every `func` and `sink` must declare both `emit` and `fail` ports. A func with an `emit` but no `fail` does not compile.
- **Op validation.** Every op called in a `func` or `sink` body is checked against the built-in op registry at compile time. Unknown ops produce an error.
- **Name match.** The callable name must match the filename stem.
- **main is a flow.** A `func main` or `sink main` is a compile error.

These guarantees mean that if `forai compile` succeeds, the program has a complete documentation contract, all outputs are explicitly typed, and all ops are valid.
