# flow

A `flow` is a declarative wiring diagram. It names the stages of a pipeline and declares how data moves between them. Flows contain no computation — no arithmetic, no string operations, no I/O. They only name what connects to what.

Every forai program has at least one flow. The entry point of every program is a flow named `main`.

## Basic Structure

```fa
docs ProcessOrder
    Validates and stores an incoming order.
done

flow ProcessOrder
    take order as Order
    emit result as Confirmation
    fail error as text
body
    step ValidateOrder(order to :order) then
        next :result to validated
    done
    step StoreOrder(validated to :order) then
        next :result to confirmation
    done
    emit confirmation
done
```

A flow has:

1. A `docs` block with the same name
2. The `flow` keyword and name
3. Optional port declarations (`take`, `emit`, `fail`)
4. A `body...done` block containing steps

## Port Declarations

Flows may have ports or they may have none at all. The top-level `main` flow typically has no ports:

```fa
flow main
body
    step app.Start() done
done
```

A sub-flow called from another flow may have ports to receive and return data:

```fa
flow Authenticate
    take credentials as Credentials
    emit result as Session
    fail error as AuthError
body
    step ValidateCredentials(credentials to :credentials) then
        next :result to user
    done
    step CreateSession(user to :user) then
        next :result to session
    done
    emit session
done
```

## Steps

A `step` is a call to a source, func, sink, or another flow. It is the fundamental unit of a flow body.

### Step with Output

```fa
step FuncName(arg to :portName) then
    next :emitPortName to localVar
done
```

- `FuncName(arg to :portName)` — calls `FuncName` with `arg` mapped to its `take` port named `portName`
- `then` — opens the result-extraction block
- `next :emitPortName to localVar` — extracts the named `emit` port result into `localVar`
- `done` — closes the step block

### Step with No Output

When the step's result is not needed downstream (for example, calling a sink at the end of a branch):

```fa
step sinks.Print(greeting to :line) done
```

The `done` closes the step immediately without a `then` block.

### Step Calling a Source

Sources are called like funcs in a flow step:

```fa
step sources.Events() then
    next :event to ev
done
```

The source runs its internal event loop. Each time it emits, the downstream steps run once for that event.

## Module Calls

Callables from imported modules are called with the module prefix:

```fa
use sources from "./sources"
use router from "./router"
use sinks from "./sinks"

flow main
body
    step sources.Commands() then
        next :cmd to cmd
    done
    step router.Classify(cmd to :cmd) then
        next :result to kind
    done
    step sinks.Print(kind to :line) done
done
```

Callables in the same directory (same module) can be called without a prefix.

## Branches

`branch when condition` creates a conditional sub-pipeline. The block body runs only if the condition is true. Multiple branches with different conditions can coexist — all whose conditions are true fire.

```fa
flow RouteCommand
body
    step sources.Commands() then
        next :cmd to cmd
    done
    step router.Classify(cmd to :cmd) then
        next :result to kind
    done
    branch when kind == "help"
        step data.HelpText() then
            next :result to text
        done
        step display.Print(text to :text) done
    done
    branch when kind == "quit"
        step display.Print("Goodbye!" to :text) done
    done
    branch when kind == "unknown"
        step display.PrintError(cmd to :cmd) done
    done
done
```

### Branch Rules

- Branches are one-way sub-pipelines. They do not merge back.
- Multiple branches can match the same event — all matching branches fire.
- A branch with no condition (`branch`) always fires. Use it to run unconditional steps.
- Branches can be nested.

### Unguarded Branch

```fa
flow LogAndProcess
body
    step sources.Events() then
        next :event to ev
    done
    branch
        step sinks.Log(ev to :event) done
    done
    branch when ev.kind == "order"
        step ProcessOrder(ev to :event) done
    done
done
```

Here the logging branch always fires, and the order-processing branch fires only for order events.

## Emit and Fail in Flows

Flows that have output ports can emit or fail directly:

```fa
flow Authenticate
    take credentials as Credentials
    emit result as Session
    fail error as AuthError
body
    step ValidateCredentials(credentials to :credentials) then
        next :result to user
    done
    step CreateSession(user to :user) then
        next :result to session
    done
    emit session
done
```

`emit session` sends the `session` variable out through the `result` port. `fail "message"` sends through the `fail` port.

## State in Flows

For flows that manage long-lived resources shared across all steps, use `state` declarations at the top of the body:

```fa
flow Server
body
    state conn = db.open("app.db")
    state srv = http.server.listen(8080)
    on :request from http.server.accept(srv) to http_conn
        step handlers.Route(http_conn to :conn, conn to :db) done
    done
done
```

`state name = op(args)` runs once when the flow starts. The result is available to all subsequent steps.

## A Complete Annotated Example

```fa
# File: main.fa

use sources from "./sources"
use lib from "./lib"
use sinks from "./sinks"

docs main
    Pipeline demo with branching and error handling.

    Generates random numbers between 0 and 100, classifies each as
    "big" (> 50) or "small" (<= 50), and routes through different paths.

    Big numbers are squared.
    Small numbers are doubled and used as circle radii.
    Both paths format and print the result.
done

flow main
body
    # Step 1: generate a random number from the source
    step sources.RandomNum() then
        next :num to raw
    done

    # Step 2a: big-number branch
    branch when raw > 50.0
        step lib.Square(raw to :num) then
            next :result to processed
        done
        step lib.NumToText(processed to :num) then
            next :result to label
        done
        step sinks.Print(label to :line) done
    done

    # Step 2b: small-number branch
    branch when raw <= 50.0
        step lib.Double(raw to :num) then
            next :result to doubled
        done
        step lib.CircleArea(doubled to :radius) then
            next :result to area
        done
        step lib.NumToText(area to :num) then
            next :result to label
        done
        step sinks.Print(label to :line) done
    done
done

test main
    mock sources.RandomNum => 75.0
    mock lib.Square => 5625.0
    mock lib.NumToText => "5625"
    mock sinks.Print => true
    _ = main()
done
```

## Flows vs Funcs: The No-Computation Rule

A flow is forbidden from containing computation. If you find yourself wanting to put arithmetic, string manipulation, or conditional logic on a value inside a flow, that logic belongs in a func.

Wrong:

```fa
# This is invalid — flows cannot contain computation
flow ProcessAge
    take age as long
body
    doubled = age * 2        # ERROR: computation in a flow
    step sinks.Print(doubled to :line) done
done
```

Right:

```fa
# Computation lives in a func
func DoubleAge
    take age as long
    emit result as long
    fail error as text
body
    result = age * 2
    emit result
done

# Flow only wires stages
flow ProcessAge
    take age as long
body
    step DoubleAge(age to :age) then
        next :result to doubled
    done
    step sinks.Print(doubled to :line) done
done
```

## Rules and Gotchas

- `main` must always be a `flow`. A `func main` or `sink main` is a compile error.
- Flows may have zero ports (no `take`, `emit`, `fail`) — this is common for top-level and sub-pipeline flows.
- If a flow has `emit` or `fail` ports, you must call `emit` or `fail` somewhere in the body to produce output.
- Branches are independent and do not merge. If you need to collect results from multiple branches, use a func with a `sync` block instead.
- `step` is the only statement type in a flow body (plus `state`, `branch`, `emit`, `fail`, and `on`). There are no variable assignments in flows outside of the `then...next` extraction syntax.
