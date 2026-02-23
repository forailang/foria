# Chapter 11.1: The Pipeline Model

forai's concurrency model is not thread-based and not callback-based. It is pipeline-based: a chain of stages through which events flow, with multiple events in-flight simultaneously at any given moment. Understanding this model is the key to writing efficient forai programs.

## The Assembly Line Metaphor

Think of an automobile assembly line. A car moves through a series of stations: welding, painting, wiring, final inspection. Each station does one job and passes the result to the next. Critically, the line does not pause while a car is being painted. As soon as station 2 receives a car from station 1, station 1 is free to receive the next car. Multiple cars are always in progress simultaneously — one at each stage.

forai pipelines work exactly this way. A `source` is the start of the line — it produces events (HTTP requests, user input, queue items) one at a time, then immediately returns to watch for the next event. Each `func` is a station — it receives an event, does its work (computation, I/O, DB queries), and passes a result downstream. A `sink` is the end of the line — it consumes the final result (prints it, writes it, responds to a client). While one event is being processed at a `func`, the `source` is already feeding the next event into the first stage.

This is not theoretical. If your pipeline has five stages and each stage takes 100ms, you can sustain a throughput of one event per 100ms (limited by the slowest stage), not one event per 500ms (the sequential total). Five events are always in-flight simultaneously.

## The Four Constructs

Every forai pipeline is composed of exactly four kinds of constructs:

```
source → flow → func → sink
```

| Construct | Role | Has I/O? | Has body? |
|-----------|------|----------|-----------|
| `source` | Event producer | Yes — blocking I/O to get events | Yes — imperative |
| `func` | Processing station | Yes — I/O, computation, DB | Yes — imperative |
| `flow` | Wiring declarator | No — pure structure | Yes — declarative steps |
| `sink` | Terminal output | Yes — side effects | Yes — imperative |

**Sources** are where data enters the pipeline. A `source` blocks until the next event arrives, then emits it and returns to waiting. The flow never sees the waiting — it only sees a stream of events. A source for an HTTP server accepts one connection at a time; a source for a terminal reads one line at a time.

**Flows** describe the shape of the pipeline. They wire sources to funcs, funcs to other funcs, and funcs to sinks. A flow has no loops, no computation, no I/O — only `step` declarations that name pipeline stages. The flow declaration is the blueprint; the runtime fills it with live events.

**Funcs** are where computation happens. Imperative code — `case/when`, `loop`, `sync`, `send nowait`, DB queries, HTTP calls — lives inside `func` bodies. A func receives a typed set of inputs and produces a typed output (or fails with a typed error).

**Sinks** are terminal side effects: printing to the terminal, writing a file, sending an HTTP response. A sink has no return value. Like funcs, sinks have `take`/`emit`/`fail` ports.

## A Complete Pipeline

Here is a minimal but complete pipeline that reads lines from the terminal, converts them to uppercase, and prints the result:

```fa
# sources/Lines.fa
docs Lines
    Reads lines from the terminal and emits them as text events.
done
source Lines
    emit line as text
    fail error as text
body
    on :input from term.prompt("> ") to raw
        trimmed = str.trim(raw)
        emit trimmed
    done
done
```

```fa
# Uppercase.fa
docs Uppercase
    Converts a string to uppercase.
done
func Uppercase
    take input as text
    emit result as text
    fail error as text
body
    up = str.upper(input)
    emit up to :result
done
```

```fa
# Print.fa
docs Print
    Prints text to the terminal.
done
sink Print
    take text as text
    emit done as bool
    fail error as text
body
    term.print(text)
    emit true to :done
done
```

```fa
# main.fa
use sources from "./sources"
docs main
    Echo pipeline: read lines, uppercase, print.
done
flow main
body
    step sources.Lines() then
        next :line to line
    done
    step Uppercase(line to :input) then
        next :result to upper
    done
    step Print(upper to :text) done
done
```

When `main` runs: `sources.Lines` emits the first line. Before `Uppercase` finishes processing it, `sources.Lines` is already waiting for line 2. Before `Print` finishes, `Uppercase` may be processing line 2. All three stages proceed concurrently, each on its own event.

## All Stages Are Async

Every stage is async and awaited by default. This means:

- A `source` that calls `http.server.accept(srv)` does not block the runtime thread while waiting for a connection. It suspends, yields control, and resumes when a connection arrives.
- A `func` that calls `db.query(conn, sql)` does not block while the database responds. It suspends, and other work proceeds.
- A `sync` block inside a `func` runs its statements concurrently using `join_all`.

All built-in I/O ops (`http.*`, `db.*`, `file.*`, `term.*`, `ws.*`, `exec.*`) are non-blocking under the hood. You write them as if they were synchronous (sequential assignment), but the runtime runs them asynchronously.

## Back-Pressure

If a stage is slow, upstream waits. This is called back-pressure, and it is automatic in forai.

If your `func` takes 500ms per event but your source emits 1000 events per second, the source will not race ahead and queue up millions of unprocessed events. Each `step` in the flow awaits the previous one before passing the next event. The pipeline rate is naturally governed by the slowest stage.

This means you do not need explicit rate limiting, queue size caps, or semaphores in most cases. The pipeline shape itself enforces a natural bound. If you need higher throughput at a bottleneck stage, you restructure the pipeline (add more parallel branches) — not add locking primitives.

## Pipeline vs. Request/Response

The pipeline model is different from a traditional request/response model. In a typical web server, each request is isolated: receive a request, do work, return a response, done. In forai, multiple requests move through the same pipeline simultaneously. A `flow` is not called once per request — it is the ongoing structure through which all requests flow.

This is why flows use `step ... then ... done` rather than function calls with return values. The flow describes the wire, not the invocation. When you write:

```fa
step handler.ProcessRequest(req to :req) then
    next :result to response
done
```

You are saying: "events flow through `ProcessRequest`; when one comes out, bind its `:result` port to `response` and continue." The runtime handles scheduling, concurrency, and event delivery.

The pipeline model makes concurrent I/O-heavy workloads efficient by default, without requiring the programmer to manage threads, callbacks, or async primitives explicitly.
