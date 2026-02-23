# Chapter 15.1: Keyword Reference

forai has a fixed set of reserved keywords. All are lowercase. An identifier that matches any keyword is a syntax error in identifier position.

## Declaration Keywords

| Keyword | Description |
|---------|-------------|
| `func` | Declares a function — a named, typed computation unit with an imperative body. One per `.fa` file; filename must match the func name. |
| `flow` | Declares a flow — a named wiring declaration that connects sources, funcs, and sinks in a pipeline. The entry point `main` must be a `flow`. |
| `sink` | Declares a sink — a terminal side-effect unit (printing, file writing, HTTP response). Like `func` but has no return value in the pipeline sense. |
| `source` | Declares a source — an event producer. Its body contains `on ... from ... to` blocks or loops that emit events one at a time. |
| `type` | Declares a named struct type with typed fields and optional constraints. Use `open type` to make it extensible. |
| `data` | Declares a type with validation constraints at the value level (`:required`, `:matches`, `:min`, `:max`). |
| `enum` | Declares a discriminated union type. Variants are listed inside the enum body. |
| `extern` | Modifier on `func`: declares a body-less function whose implementation is provided by the host runtime. |
| `use` | Imports a module: `use auth from "./auth"` makes `auth.Login(...)` callable. |
| `docs` | Declares a documentation block for the following construct. Required for every `func`, `flow`, `sink`, `source`, `test`, `type`, and `enum`. |
| `test` | Declares a test block for a func, flow, sink, or source. Contains `must`, `trap`, and `mock` assertions. |

## Port Keywords

| Keyword | Description |
|---------|-------------|
| `take` | Declares an input port on a `func`, `flow`, `sink`, or `source`. Syntax: `take name as Type`. |
| `emit` | Declares an output port. In `func`/`sink` headers: `emit name as Type`. In bodies: `emit value to :portname` sends the value to that output port. |
| `fail` | Declares a failure output port. In headers: `fail name as Type`. In bodies: `emit value to :error` uses the fail port. |
| `return` | Alternative to `emit` for single-output funcs. Declares the return type in the header; `return value` in the body sends to the implicit output. |
| `as` | Used after `take`, `emit`, `fail`, `return` to specify the type: `take name as Type`. Also used in `loop list as item`. |

## Body and Block Keywords

| Keyword | Description |
|---------|-------------|
| `body` | Marks the start of a func, sink, or source body. Followed by statements until `done`. |
| `done` | Closes a block: `body...done`, `case...done`, `loop...done`, `sync...done [...]`, `on...done`, `branch...done`, `docs...done`. |
| `open` | Modifier on `type` or `data`: makes the type extensible (public). `open type Foo`. |

## Control Flow Keywords

| Keyword | Description |
|---------|-------------|
| `case` | Starts a pattern-match block. `case expr` followed by `when` arms and optional `else`. Closed by `done`. |
| `when` | An arm in a `case` block. `when literal` or `when ident` matches the case expression. |
| `else` | The fallback arm in a `case` block. Runs when no `when` arm matches. |
| `if` | Syntactic sugar for `case`. `if expr` ... `else if expr` ... `else` ... `done`. Desugars to `case` at parse time. |
| `loop` | Starts an iteration block. `loop list as item` iterates over a list. Closed by `done`. |
| `break` | Exits the current `loop` or `on` block immediately. |
| `sync` | Starts a concurrent join block. `[a, b] = sync [:opts]` ... `done [a, b]` runs body statements concurrently via `join_all`. |

## Async Dispatch Keywords

| Keyword | Description |
|---------|-------------|
| `send` | Used with `nowait` to fire a func/flow as a background task: `send nowait FuncName(args)`. |
| `nowait` | Used after `send` for user funcs, or alone for built-in ops: `nowait op(args)`. Caller continues immediately; task runs in background. |

## Flow Body Keywords

These keywords appear inside `flow` bodies (step-based wiring declarations):

| Keyword | Description |
|---------|-------------|
| `step` | Declares a pipeline stage in a flow body. `step FuncName(inputs) then ... done` or `step FuncName(inputs) done`. |
| `state` | Declares a persistent state variable in a flow body. `state conn = db.open("app.db")` — opened once, shared across all events. |
| `on` | Event handler block in a `source` body. `on :tag from op(args) to var` runs the body once per emitted event. |
| `from` | Used in `on` blocks to specify the event source expression. |
| `next` | Inside a `step ... then` block, routes a named output port: `next :portname to var`. |
| `then` | Opens the output routing block of a `step`. `step Func(...) then` ... `done`. |
| `to` | In `step` args: passes a value to a named port: `value to :portname`. In `emit`: `emit val to :result`. In `next`: `next :port to var`. |
| `branch` | In flow bodies: `branch when expr` creates a conditional sub-pipeline. `branch` alone (unguarded) always runs. |

## Testing Keywords

| Keyword | Description |
|---------|-------------|
| `must` | In a `test` block: asserts a boolean expression. `must expr == value`. Failure reports the expression and resolved values with file:line:col. |
| `trap` | In a `test` block: asserts that a call fails. `trap FuncName(args)` succeeds if `FuncName` emits to its `fail` port; fails if `FuncName` succeeds. |
| `mock` | In a `test` block: substitutes a sub-func's return value. `mock module.FuncName => value`. Active for the duration of the test block. |

## Summary: Keyword Count

forai has 38 reserved keywords in v1:

```
func  flow  sink  source  type  data  enum  use  docs  test
take  emit  fail  return  as
body  done  open
case  when  else  if  loop  break  sync
send  nowait
step  state  on  from  next  then  to  branch
must  trap  mock
extern
```

All keywords are lowercase. `True`, `False`, `Null` are not keywords — literal values are `true`, `false`, and `null` (parsed as `Ident` tokens with special meaning in `when` arms and expressions).
