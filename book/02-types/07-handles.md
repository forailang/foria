# Handles

Handles are opaque types representing open connections to external systems — databases, HTTP servers, HTTP connections, and WebSocket connections. They are produced by specific ops, used by other ops, and validated by the compiler to ensure you never pass the wrong handle to the wrong op.

## The Four Handle Types

| Type | Produced by | Consumed by | Description |
|------|------------|-------------|-------------|
| `db_conn` | `db.open(...)` | `db.exec`, `db.query`, `db.close` | SQLite database connection |
| `http_server` | `http.server.listen(port)` | `http.server.accept`, `http.server.close` | HTTP server listener |
| `http_conn` | `http.server.accept(server)` | `http.server.respond`, `http.extract_path`, `http.extract_params` | An individual HTTP client connection |
| `ws_conn` | `ws.connect(url)` | `ws.send`, `ws.recv`, `ws.close` | WebSocket connection |

## Handles Are Opaque

At runtime, a handle is a string like `"db_0"`, `"srv_1"`, or `"ws_3"`. You cannot construct a handle from a string literal. You cannot compare two handles with `==`. You cannot inspect the string value of a handle directly. Handles are opaque by design — you obtain them through the appropriate `open`/`listen`/`connect` op and pass them to the ops that consume them.

```fa
body
    # Correct: obtain handle from the right op
    conn = db.open(":memory:")
    rows = db.query(conn, "SELECT * FROM users", list.new())

    # Wrong: cannot construct a handle literal
    # conn = "db_0"    # this would be a plain text value, not a db_conn handle
done
```

## Handle Validation

The compiler validates handle types at compile time. If you pass an `http_server` handle to `db.query`, the compiler produces an error — even though both handles are strings at runtime. This prevents a class of bugs where the wrong resource is passed to the wrong operation.

```fa
func ServeHttp
    take _ as void
    emit done as bool
    fail error as text
body
    conn = db.open(":memory:")       # conn: db_conn
    srv = http.server.listen(8080)   # srv: http_server

    # Compiler error: db.query expects db_conn, received http_server
    # rows = db.query(srv, "SELECT 1", list.new())

    # Correct:
    rows = db.query(conn, "SELECT 1", list.new())
done
```

## Passing Handles Between Funcs

A func that opens a handle can pass it to child funcs and flows. The child declares the handle type in its `take` port:

```fa
# Parent func: opens the handle
func OpenAndQuery
    take _ as void
    emit result as list
    fail error as text
body
    conn = db.open("app.db")
    result = QueryUsers(conn to :conn)
    _ = db.close(conn)
    emit result
done

# Child func: receives the handle
func QueryUsers
    take conn as db_conn    # takes a db_conn handle
    emit result as list
    fail error as text
body
    params = list.new()
    rows = db.query(conn, "SELECT id, name FROM users", params)
    emit rows
done
```

The child can use the handle because it shares the parent's handle registry. The handle string `"db_0"` is valid in the child's runtime context because the parent created it.

## Ownership Rule

The func that opens a handle owns it. Children may use it, but only the owner is responsible for closing it.

```fa
func ProcessRequests
    take _ as void
    emit done as bool
    fail error as text
body
    conn = db.open("app.db")            # owns conn

    step_a = ProcessOrder(conn to :conn)   # child uses conn
    step_b = UpdateStatus(conn to :conn)   # another child uses conn

    _ = db.close(conn)                  # owner closes conn
    emit true
done
```

Children should not close handles they did not open. Closing a handle in a child and then using it in the parent or another child causes a runtime error.

## State Declarations for Shared Handles

In flows, use `state` to open handles once and share them across all steps:

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

`state` declarations run once when the flow starts. The handles are available to every step in the flow. This is the correct pattern for a long-running server that handles many requests against the same database.

## `send nowait` Isolation

Fire-and-forget tasks (launched with `send nowait`) run in an isolated context. They do not share the parent's handle registry. A spawned task that needs a database connection must open its own:

```fa
func SpawnWorker
    take job_id as text
    emit done as bool
    fail error as text
body
    # Correct: pass data (not a handle) to the background task
    send nowait BackgroundProcess(job_id to :job_id)

    # Wrong: if BackgroundProcess takes db_conn, the handle won't work
    # in the isolated context of the spawned task
    emit true
done

func BackgroundProcess
    take job_id as text
    emit done as bool
    fail error as text
body
    # Open its own connection in the isolated context
    conn = db.open("app.db")
    _ = db.exec(conn, "UPDATE jobs SET status = 'done' WHERE id = ?", [job_id])
    _ = db.close(conn)
    emit true
done
```

## `sync` Sharing

Unlike `send nowait`, statements inside a `sync` block share the parent's handle registry. Multiple concurrent statements can use the same handles safely:

```fa
func FetchParallel
    take user_id as text
    emit result as Summary
    fail error as text
body
    conn = db.open("app.db")

    [orders, profile] = sync
        orders = db.query(conn, "SELECT * FROM orders WHERE user_id = ?", [user_id])
        profile = db.query(conn, "SELECT * FROM profiles WHERE user_id = ?", [user_id])
    done [orders, profile]

    _ = db.close(conn)
    summary = build_summary(orders, profile)
    emit summary
done
```

Both `db.query` calls inside the `sync` block use the same `conn` handle and run concurrently. The handle registry is shared, not copied.

## Common Handle Patterns

### Database CRUD

```fa
docs QueryUsers
    Retrieves all active users from the database.
done

func QueryUsers
    take conn as db_conn
    emit result as list
    fail error as text
body
    params = list.new()
    rows = db.query(conn, "SELECT id, name, email FROM users WHERE active = 1", params)
    emit rows
done
```

### HTTP Server Loop

```fa
docs AcceptRequests
    Listens for incoming HTTP connections and emits each as a request event.
done

source AcceptRequests
    emit req as dict
    fail error as text
body
    state srv = http.server.listen(8080)
    on :request from http.server.accept(srv) to conn
        path = http.extract_path(conn)
        params = http.extract_params(conn)
        req = obj.new()
        req = obj.set(req, "conn", conn)
        req = obj.set(req, "path", path)
        req = obj.set(req, "params", params)
        emit req
    done
done
```

### WebSocket Client

```fa
docs ListenForUpdates
    Connects to a WebSocket server and emits received messages.
done

source ListenForUpdates
    emit msg as dict
    fail error as text
body
    ws = ws.connect("ws://localhost:9000/updates")
    loop list.range(0, 100) as _
        raw = ws.recv(ws)
        parsed = json.decode(raw)
        emit parsed
    done
    _ = ws.close(ws)
done
```

## Rules and Gotchas

- Handles cannot be constructed from string literals. They come only from `db.open`, `http.server.listen`, `http.server.accept`, or `ws.connect`.
- The compiler validates handle types at compile time. Passing the wrong handle type to an op is a compile error.
- At runtime, handles are string identifiers like `"db_0"`. Inspecting or comparing them as strings is not meaningful.
- Ownership: the func that opens a handle is responsible for closing it. Do not close a handle you did not open.
- `send nowait` (fire-and-forget) tasks are isolated. They cannot use handles from the parent. Pass primitive data to background tasks and open fresh handles inside them.
- `sync` blocks share the parent's handle registry. It is safe to use the same handle from multiple concurrent branches inside a `sync` block.
- Prefer opening handles in `state` declarations (for flows) or at the top of a func body. Avoid opening handles inside loops — the handle registry can accumulate stale entries.
