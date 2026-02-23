# Mental Model

Understanding forai deeply requires internalizing two ideas: the four-construct decomposition, and the assembly-line concurrency model. This chapter explains both.

## The Four Constructs

forai programs are made of exactly four kinds of thing. Every `.fa` file contains one of them.

### Source

A source is an event producer. It knows how to obtain events — by polling a device, accepting a network connection, reading from a queue, prompting a user — and emits each event downstream as it arrives. The source owns the "how do I get the next event?" loop. The rest of the pipeline never sees that loop; it only sees the events.

```fa
source TemperatureSensor
    emit reading as real
    fail error as text
body
    loop list.range(0, 1000) as _
        raw = read_sensor_value()
        emit raw
    done
done
```

Sources are where the world enters the program. They are the only construct that contains polling or blocking I/O at the top level.

### Func

A func is a pure computation unit. It takes a value, does work, and emits a result (or fails). Funcs may call built-in ops, do arithmetic, branch on conditions, loop over lists, and call other funcs. They may not do terminal I/O directly (that is a sink's job) and they do not produce event streams (that is a source's job).

```fa
func Validate
    take email as text
    emit result as bool
    fail error as text
body
    is_valid = str.contains(email, "@")
    if is_valid
        emit true
    else
        fail "invalid email address"
    done
done
```

Funcs are the building blocks of computation. Most of a complex forai application lives in funcs.

### Sink

A sink is syntactically identical to a func. The `sink` keyword signals that this callable is a terminal — it performs a side effect (printing, responding to an HTTP request, writing to a database) and does not produce a value for further pipeline stages. The distinction is semantic and documentary; the compiler enforces that sinks appear at the end of pipelines.

```fa
sink WriteRecord
    take record as Record
    emit done as bool
    fail error as text
body
    _ = db.exec(conn, "INSERT INTO records VALUES (?)", record.id)
    emit true
done
```

Think of sinks as the shipping dock. When data reaches a sink, it is leaving the program's internal processing and going out into the world.

### Flow

A flow is a declarative wiring diagram. It names which stages connect to which, in what order. It contains no computation — no arithmetic, no branching on values (only on routing), no I/O. A flow calls sources, funcs, and sinks by name, and declares how data flows between them.

```fa
flow ProcessOrder
    take order as Order
    emit result as Confirmation
    fail error as text
body
    step ValidateOrder(order to :order) then
        next :result to valid_order
    done
    step ComputeTotal(valid_order to :order) then
        next :result to total
    done
    step sinks.Confirm(total to :total) then
        next :result to confirmation
    done
    emit confirmation
done
```

Flows can have ports (take/emit/fail) when they are sub-pipelines called from other flows. The top-level `main` flow typically has no ports.

## The Assembly-Line Concurrency Model

A pipeline is not a sequential program that processes one item at a time. It is more like an assembly line with multiple stations, where a new item enters the line as soon as the first station is free.

Consider a pipeline with three stages:

```
Source → Transform → Sink
```

When the source emits event 1, Transform begins processing it. While Transform is working on event 1, the Source is free to emit event 2 into the pipeline. When Transform finishes event 1 and passes it to Sink, Transform picks up event 2. Meanwhile the Source emits event 3.

At peak throughput, all three stages are busy simultaneously: Source is producing event N+2, Transform is processing event N+1, and Sink is consuming event N. The pipeline is never idle waiting for one stage to finish before the next stage starts.

This is structural concurrency — it emerges from the pipeline shape, not from explicit thread management. You do not write `async/await` or spawn threads. The runtime handles it.

### Branch Concurrency

When a flow contains branches, all branches whose conditions are true fire independently. If two branches both match, their downstream stages run concurrently. Each branch is its own sub-pipeline.

```fa
flow Route
body
    step sources.Events() then
        next :event to ev
    done
    branch when ev.kind == "alert"
        step sinks.Notify(ev to :event) done
    done
    branch when ev.kind == "log"
        step sinks.Archive(ev to :event) done
    done
done
```

If an event has `kind = "alert"`, only the Notify branch fires. If an event matches neither, neither fires. Branches are always independent; they never merge back into a single path.

### Sync Blocks

Inside a func body, `sync` runs a set of operations concurrently and waits for all of them to finish:

```fa
func FetchAll
    take query as text
    emit result as Summary
    fail error as text
body
    [users, products, orders] = sync
        users = db.query(conn, "SELECT * FROM users")
        products = db.query(conn, "SELECT * FROM products")
        orders = db.query(conn, "SELECT * FROM orders")
    done [users, products, orders]
    summary = build_summary(users, products, orders)
    emit summary
done
```

The three database queries run at the same time. The func waits for all three before proceeding. This is the explicit concurrency primitive inside func bodies.

## What Does Not Exist in forai

forai deliberately omits several constructs that appear in most languages:

- **No global mutable state.** All data flows through ports. There are no module-level variables.
- **No inheritance or method dispatch.** Types are structs and enums; behavior lives in funcs.
- **No unchecked exceptions.** Failures travel through the `fail` port. A caller that does not handle the failure port propagates it automatically.
- **No implicit async.** Every construct is async; there is nothing to mark.
- **No unnamed lambdas.** Computation lives in named funcs in named files. Anonymous functions do not exist.

These omissions are features. They keep the language small, the programs readable, and the pipelines explicit.
