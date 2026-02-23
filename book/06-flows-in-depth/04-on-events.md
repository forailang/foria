# On Events

The `on` statement in a flow body creates an event-driven loop — it continuously accepts events from a source and processes them through a sub-pipeline. `on` is how flows connect to sources.

## Syntax

```fa
on varname from source_expr
  # flow statements
done
```

- `varname` is the name bound to each incoming event
- `source_expr` is either a `state` handle (like an HTTP server or WebSocket) or a source callable
- The body contains flow statements: steps, branches, emits, sends

```fa
flow Server
body
  state server = http.server.listen(8080)

  on req from server
    step Handle(req to :request) then
      next :response to resp
    done
    step http.server.respond(server to :srv, req to :conn, resp to :body) done
  done
done
```

For every HTTP request that arrives on port 8080, the `on` block binds the request to `req` and runs the body.

## The Event Loop

`on` implements an implicit infinite loop. The runtime:

1. Awaits the next event from the source
2. Binds it to the variable name
3. Executes the body statements
4. Returns to step 1

The flow never manually loops — the `on` construct handles the repetition. This is the dataflow model: sources push events; the flow processes each one.

## Multiple On Blocks

A flow can have multiple `on` blocks, each consuming from a different source:

```fa
flow MultiSource
body
  state http_srv = http.server.listen(8080)
  state ws_srv   = ws.connect("wss://events.example.com")

  on http_req from http_srv
    step HandleHttp(http_req to :request) then
      next :response to resp
    done
    step http.server.respond(http_srv to :srv, http_req to :conn, resp to :body) done
  done

  on ws_msg from ws_srv
    step HandleWs(ws_msg to :message) then
      next :reply to reply
    done
    step ws.send(ws_srv to :conn, reply to :msg) done
  done
done
```

Each `on` block runs as an independent event loop. Both loops can be active simultaneously — the runtime handles the concurrency.

## On Inside a Source

Inside a `source` body (not a flow), `on` works slightly differently: it handles events from an expression that produces events (like an OS listener). Source bodies use `on :tag from expr to var`:

```fa
source HttpSource
  emit request as dict
body
  server = http.server.listen(8080)
  on :request from http.server.accept(server) to req
    emit req
  done
done
```

The `:tag` is a semantic label for the event type. In v1 it is stored but cosmetic — the tag does not affect routing.

## On in Flow vs Source

| Context | Syntax | Purpose |
|---------|--------|---------|
| `flow` body | `on var from handle` | Event loop over a handle |
| `source` body | `on :tag from expr to var` | Event handler producing emits |

In a flow body, `on` is the primary event-processing construct. In a source body, `on` is one way to declare the event-pumping loop.

## Event Handling with State

The `on` block has access to all `state` handles declared in the flow body:

```fa
flow ClickTracker
body
  state conn   = db.open("clicks.sqlite")
  state server = http.server.listen(9090)

  on req from server
    # 'conn' is accessible here — same handle for every request
    step RecordClick(req to :request, conn to :db) then
      next :count to total_clicks
    done
    step Respond(total_clicks to :n) then
      next :body to resp_body
    done
    step http.server.respond(server to :srv, req to :conn, resp_body to :body) done
  done
done
```

The database connection `conn` is shared across all requests — each `on` invocation uses the same handle.

## Error Handling in On Blocks

Errors from steps inside an `on` block can be routed with branches:

```fa
on req from server
  step Parse(req to :raw) then
    next :result to parsed
    next :error  to parse_err
  done

  branch when parse_err
    step ErrorResponse(parse_err to :msg) then
      next :body to err_body
    done
    step http.server.respond(server to :srv, req to :conn, err_body to :body) done
  done

  step Process(parsed to :data) then
    next :result to result
  done
  step http.server.respond(server to :srv, req to :conn, result to :body) done
done
```

The branch catches the error case and sends an appropriate response before the main path continues.

## On and Backpressure

Each invocation of the `on` body is awaited before the next event is processed from that source. If a step takes 100ms, the next event from that source waits 100ms. This is natural backpressure — the pipeline does not accept new work faster than it can process it.

For high-throughput scenarios, use `send nowait` inside the `on` block to dispatch processing in the background and accept the next event immediately.

## Practical Example

```fa
flow ChatServer
body
  state conn   = db.open("chat.sqlite")
  state server = http.server.listen(4000)

  on req from server
    step AuthRequest(req to :request) then
      next :user  to user
      next :error to auth_err
    done

    branch when auth_err
      step http.server.respond(server to :srv, req to :conn, "401" to :status, auth_err to :body) done
    done

    step RouteMessage(req to :request, user to :sender) then
      next :message to msg
      next :room    to room_id
    done

    step StoreMessage(msg to :content, room_id to :room, conn to :db) then
      next :stored to stored_msg
    done

    send nowait BroadcastMessage(stored_msg to :message, room_id to :room)

    step http.server.respond(server to :srv, req to :conn, stored_msg to :body) done
  done
done
```
