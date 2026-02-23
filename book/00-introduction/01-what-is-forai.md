# What Is forai?

forai is a text-first dataflow language designed for building concurrent, event-driven applications. It compiles `.fa` source files into a JSON IR (intermediate representation) and executes them on an async runtime. The language is purpose-built for pipelines: programs where data moves through a series of stages, each stage doing one thing well.

## The Factory-Floor Metaphor

The clearest mental model for forai is an assembly line. Imagine a factory floor:

- **Sources** are the loading docks where raw materials arrive. They do not know what happens downstream. They just receive events — user input, HTTP requests, database rows — and pass them along.
- **Funcs** are the workstations. Each func takes one input, does one transformation, and passes its result to the next station. Workstations do not talk to each other directly; the assembly line connects them.
- **Sinks** are the shipping dock. Finished goods leave the factory here — printed to a terminal, sent as an HTTP response, written to a file.
- **Flows** are the factory floor layout itself. They declare which machines connect to which, in what order. The layout does not do any work; it just describes the shape of the line.

This separation is not arbitrary. When computation is isolated in funcs, it is easy to test, mock, and reason about in isolation. When the wiring lives in flows, the overall shape of the program is readable at a glance — you do not need to trace through nested imperative code to understand how data moves.

## Text-First and Git-Friendly

forai source files are plain text with no required tooling beyond the `forai` binary. There is no IDE plugin required, no code generation step, and no binary format in the repository. Because every `.fa` file contains exactly one callable (one func, flow, sink, or source), diffs are clean: adding a new function means adding a new file, not modifying a shared file that contains many functions. Code review is per-feature, not per-file.

The compiler enforces a strict one-callable-per-file rule. The name of the callable must match the filename stem:

```fa
# File: Validate.fa
func Validate
    take input as text
    emit result as bool
    fail error as text
body
    ok = str.len(input) > 0
    emit ok
done
```

If the file is named `Validate.fa`, the callable inside must be named `Validate`. This rule makes navigation trivial: to find `router.Classify`, look in `router/Classify.fa`.

## Async by Default

Every func, flow, sink, and source in forai is asynchronous. There is no `async`/`await` keyword pair to remember. Every call is awaited. The runtime is built on Tokio and uses a cooperative current-thread executor, which means a single-threaded event loop handles all concurrency through yielding — the same model Node.js uses, but compiled to native code via WebAssembly.

This has a practical consequence: a slow stage does not block the entire program. If step 3 in a pipeline takes 200ms, step 1 and step 2 are already processing the next event. The assembly line keeps moving.

## JSON IR Target

When you compile a `.fa` file, the output is a JSON document describing the program's graph structure. This IR contains:

- **nodes**: the computation steps (op calls, variable bindings)
- **edges**: the wiring between steps
- **emits**: the output routing decisions

The JSON IR is human-readable and stable across compiler versions. It can be stored, diffed, and transmitted. The runtime loads it and executes it on the async engine. This separation between compile and run is intentional: compile-time errors (type mismatches, missing docs, unknown ops) are caught before any code runs.

## Design Philosophy

forai enforces several constraints that are unusual in mainstream languages:

- **Docs are required.** Every func, flow, sink, source, and test must have a `docs` block. Missing documentation is a compile error, not a lint warning.
- **Flows are declarative.** A flow may not contain computation. No arithmetic, no string operations, no conditionals beyond routing. Flows wire stages together; funcs do the work.
- **main is always a flow.** The entry point of every forai program is a declarative flow, not an imperative main function.
- **Strict interfaces.** Every func and sink declares its input (`take`), success output (`emit`), and failure output (`fail`) explicitly. There are no implicit returns and no unchecked exceptions.

These constraints pay off in correctness and readability. The language trades a small amount of expressiveness per-construct for a large gain in system-level clarity.
