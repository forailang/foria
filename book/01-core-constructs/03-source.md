# source

A `source` is an event producer. It knows how to obtain events from the external world — by polling a device, accepting network connections, reading lines from a terminal, iterating over a list — and emits each event into the pipeline one at a time. The source owns the "how do I get the next item" logic. Downstream stages never see the waiting or the loop.

## Basic Structure

```fa
docs MySource
    One-sentence description of what events this source produces.
done

source MySource
    emit item as text
    fail error as text
body
    # event loop goes here
done

test MySource
    # typically a stub test for sources that require I/O
    ok = true
    must ok == true
done
```

A source has the same header structure as a func: a `docs` block, port declarations, and a `body...done` block. The difference is in the body: sources use event loop constructs (`on`, `loop`) to produce multiple events over time, rather than computing a single result.

## Port Declarations

Sources declare `emit` and `fail` ports, just like funcs. They do not declare a `take` port (sources receive no input from the pipeline — they are the origin).

```fa
source Events
    emit event as Event
    fail error as text
body
    ...
done
```

## The `on` Block: Event-Driven Sources

The `on :tag from op(args) to var` construct is the primary building block for event-driven sources. It repeatedly calls an async operation and processes each result.

```fa
source Commands
    emit cmd as text
    fail error as text
body
    on :input from term.prompt("docs> ") to raw
        trimmed = str.trim(raw)
        emit trimmed
        case trimmed
            when "quit"
                break
            when "exit"
                break
        done
    done
done
```

Breaking down the `on` block:

- `:input` — the event tag (cosmetic in v1; stored but not used for routing by the runtime)
- `from term.prompt("docs> ")` — the blocking async operation to call on each iteration
- `to raw` — the variable name to bind the result to inside the block
- The block body runs once per event. Call `emit` to send the event downstream. Call `break` to stop the source.

The `on` block loops automatically. You do not write `while true` or manage the loop counter. As soon as one iteration finishes, the runtime calls `term.prompt` again for the next event.

### Multiple `on` Blocks

A source can contain multiple `on` blocks, each handling a different type of incoming event:

```fa
source HttpAndWs
    emit request as Request
    fail error as text
body
    on :http from http.server.accept(srv) to conn
        req = http.extract_path(conn)
        emit req
    done
    on :ws from ws.recv(ws_conn) to msg
        parsed = json.decode(msg)
        emit parsed
    done
done
```

In v1, multiple `on` blocks run sequentially (one finishes before the next starts). v2 will support concurrent event handlers.

## Loop-Based Sources: Polling

For polling patterns, use `loop` inside the body:

```fa
source RandomNum
    emit num as real
    fail error as text
body
    iters = list.range(0, 5)
    loop iters as idx
        n_raw = random.float()
        n = n_raw * 100.0
        emit n
    done
done
```

This source emits five random numbers between 0 and 100, then stops. The `loop` iterates over the list; `emit` sends each value downstream.

Polling with a fixed interval:

```fa
source Heartbeat
    emit tick as long
    fail error as text
body
    count = 0
    loop list.range(0, 100) as _
        count = count + 1
        emit count
    done
done
```

## Emit and Break

Inside a source body:

- `emit value` — sends an event downstream and continues the loop (the source does not stop)
- `break` — exits the current loop or `on` block, stopping the source

This is different from `emit` in a func. In a func, `emit` exits the body immediately. In a source, `emit` continues execution — the loop runs again and may emit more events.

```fa
body
    on :line from term.prompt("> ") to input
        trimmed = str.trim(input)
        if str.len(trimmed) == 0
            # skip empty lines, continue loop
        else if trimmed == "quit"
            emit trimmed
            break      # stop after emitting "quit"
        else
            emit trimmed   # emit and loop again
        done
    done
done
```

## State Declarations

For sources that need persistent state across events (such as a connection handle that must be opened once and reused), use `state` declarations:

```fa
source HttpRequests
    emit req as Request
    fail error as text
body
    state srv = http.server.listen(8080)
    on :request from http.server.accept(srv) to conn
        path = http.extract_path(conn)
        req = build_request(conn, path)
        emit req
    done
done
```

The `state name = op(args)` line runs once when the source starts, before any events are processed. The resulting handle (or value) is available for the rest of the body. This is the correct place to open database connections, start HTTP servers, or establish WebSocket connections that the source will use repeatedly.

## A Complete Annotated Example

```fa
docs Commands
    Prompts the user for commands and emits them as text events.
    Emits "quit" and "exit" as regular commands, then stops the source.
done

source Commands
    emit cmd as text       # each prompt result is emitted as this type
    fail error as text     # failure track for I/O errors
body
    on :input from term.prompt("docs> ") to raw
        # trim whitespace from the raw input
        trimmed = str.trim(raw)

        # always emit the trimmed command, including quit/exit
        emit trimmed

        # check if the user wants to stop
        case trimmed
            when "quit"
                break
            when "exit"
                break
        done
    done
done

test Commands
    # Sources that read from the terminal cannot be fully unit-tested.
    # Integration tests cover this path.
    ok = true
    must ok == true
done
```

## Sources in Flows

When a flow calls a source, it uses the same `step...then` syntax as for funcs:

```fa
flow main
body
    step sources.Commands() then
        next :cmd to cmd
    done
    step router.Classify(cmd to :cmd) then
        next :result to kind
    done
    # ...
done
```

The flow calls `sources.Commands()` once. The source runs its event loop internally, emitting events one at a time. For each emitted event, the flow continues from the `next :cmd to cmd` line, running the downstream steps (`router.Classify`, etc.) for that event. When the source stops (via `break` or exhausting its loop), the flow branch ends.

This is the assembly-line model: the source is one station, and the pipeline below it processes each item the station produces.

## Rules and Gotchas

- Sources do not have a `take` port. They produce events, they do not receive them.
- `emit` inside a source body does not exit the loop. It sends the event downstream and continues.
- `break` exits the innermost `on` or `loop` block. Use it to stop the source.
- `state` declarations run once before the event loop starts. Use them for one-time setup (opening connections).
- Sources are tested with stub tests by convention, because full I/O event loops require the runtime. Integration testing covers the real behavior.
- The event tag (`:input`, `:request`, `:data`) is cosmetic in v1. It is stored in the IR but not used for routing.
