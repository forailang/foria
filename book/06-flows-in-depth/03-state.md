# State

The `state` declaration in a flow body creates a shared resource that is initialized once when the flow starts and reused across all events. It is the mechanism for database connections, HTTP server handles, and other long-lived resources.

## Syntax

```fa
state name = op(args)
```

`state` always calls an operation that returns a handle — a connection, server, or other resource object. The operation runs exactly once when the flow is first loaded. All subsequent event handling in the flow body can use the handle by name.

```fa
flow DataService
  take request as dict
  emit response as dict
  fail error    as text
body
  state conn = db.open(":memory:")

  # 'conn' is available for every request handled by this flow
  step QueryData(request to :req, conn to :db) then
    next :result to data
  done
  emit data to :response
done
```

## Initialization Timing

`state` runs at **startup** — before the flow processes its first event. If the initialization operation fails, the flow fails to start entirely. This is intentional: if a required database connection cannot be opened, the flow should not run at all.

```fa
flow ApiServer
body
  state server = http.server.listen(8080)
  state conn   = db.open("postgresql://localhost/mydb")

  # server and conn are ready before any request is processed
  on req from server
    step HandleRequest(req to :request, conn to :db) then
      next :response to resp
    done
    step http.server.respond(server to :srv, req to :conn, resp to :body) done
  done
done
```

Both `server` (the HTTP listener) and `conn` (the database connection) are initialized before the `on` loop begins.

## Multiple State Declarations

A flow can have multiple `state` declarations:

```fa
flow FullService
body
  state db      = db.open("app.sqlite")
  state server  = http.server.listen(8080)
  state cache   = redis.connect("localhost:6379")

  on req from server
    step Route(req to :request, db to :conn, cache to :store) then
      next :response to resp
    done
    step http.server.respond(server to :srv, req to :conn, resp to :body) done
  done
done
```

State handles are shared across all event handling — every request that comes through `server` uses the same `db` and `cache` handles.

## State and Concurrency

State handles are designed for concurrent use. Database handles (`db_conn`), HTTP server handles (`http_server`), and WebSocket handles are safe to share across concurrent event handlers because the underlying runtime operations are async and non-blocking.

For the SQLite db handle specifically: forai's runtime uses a single-threaded async executor, so concurrent queries are interleaved rather than truly parallel. This is safe and correct.

## What State Is Not

State is not general mutable memory. You cannot write:

```fa
state counter = 0         # invalid — 0 is not a handle
state config  = {}        # invalid — {} is not a handle
```

`state` is specifically for resource handles returned by operations like `db.open`, `http.server.listen`, `ws.connect`, and similar. These are opaque handle types recognized by the type system.

For shared mutable values (like a counter or a cache dictionary), use a database or a dedicated service func instead. forai's flow model does not provide shared mutable memory — state handles are the exception, and they are read-only references to external resources.

## State in Sub-Flows

State declarations live in the flow that declares them. If a sub-flow needs access to a database connection, it receives the handle as a `take` parameter:

```fa
flow Main
body
  state conn = db.open("app.sqlite")

  on req from server
    step SubFlow(req to :request, conn to :db) then
      next :response to resp
    done
  done
done

flow SubFlow
  take request as dict
  take db      as db_conn
  emit response as dict
  fail error    as text
body
  step Query(request to :req, db to :conn) then
    next :result to data
  done
  emit data to :response
done
```

The handle is passed down the call chain like any other value.

## Common State Patterns

### Database connection

```fa
state conn = db.open("./data.sqlite")
```

### HTTP server

```fa
state server = http.server.listen(8080)
```

### WebSocket connection

```fa
state ws = ws.connect("wss://api.example.com/stream")
```

### Multiple databases

```fa
state primary = db.open("postgresql://primary/app")
state replica = db.open("postgresql://replica/app")
```

## Practical Example

```fa
flow TodoApi
body
  state conn   = db.open("todos.sqlite")
  state server = http.server.listen(3000)

  # Initialize schema on startup
  step InitSchema(conn to :db) done

  on req from server
    step ParseRoute(req to :request) then
      next :method to method
      next :path   to path
      next :body   to body
    done

    step Dispatch(method to :verb, path to :route, body to :data, conn to :db) then
      next :response to response
      next :status   to status
      next :error    to dispatch_error
    done

    branch when dispatch_error
      step http.server.respond(server to :srv, req to :conn, "500" to :status, dispatch_error to :body) done
    done

    step http.server.respond(server to :srv, req to :conn, status to :status, response to :body) done
  done
done
```
