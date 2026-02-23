# Chapter 11.3: send nowait — Background Tasks

`send nowait` is forai's fire-and-forget mechanism. It starts a func or flow as a background task and returns to the caller immediately, without waiting for the task to finish. The caller does not receive a return value. The spawned task runs independently.

## Syntax

```fa
send nowait FuncName(arg1 to :port1, arg2 to :port2)
```

For built-in ops, use `nowait` directly (without `send`):

```fa
nowait op(args)
```

The `send` keyword is for user-defined funcs and flows. `nowait` alone is for runtime built-in ops.

## What Happens When You Write send nowait

When execution reaches a `send nowait` line:

1. The runtime spawns a new async task for the target func/flow.
2. The current func **immediately continues** to the next line.
3. No result is captured. There is no binding on the left-hand side.
4. If the spawned task fails, the error is **logged to stderr** — it is not propagated to the caller.

```fa
func HandleRequest
    take req as dict
    emit result as dict
    fail error as text
body
    # Fire background analytics logging — don't wait for it
    send nowait LogRequest(req to :req)

    # Continue processing the request immediately
    result = obj.new()
    result = obj.set(result, "status", "ok")
    emit result to :result
done
```

`LogRequest` starts running in the background. `HandleRequest` produces its result without waiting for the log to complete.

## Isolated Handle Registry

This is the most important constraint on `send nowait`: the spawned task receives an **isolated handle registry**. It does not share handles (database connections, HTTP server handles, WebSocket connections) with the parent.

This means:

- If the parent opened a `db_conn` called `conn`, the spawned task **cannot use `conn`**.
- If the parent has an `http_server` handle, the spawned task **cannot use it**.
- The spawned task must open its own handles if it needs them.

```fa
# WRONG — conn is from the parent; not available in the background task
func Start
    take nothing as bool
    emit result as bool
    fail error as text
body
    conn = db.open("app.db")
    send nowait ProcessJobs(conn to :conn)  # conn is unavailable inside ProcessJobs
    emit true to :result
done
```

```fa
# Correct — ProcessJobs opens its own connection
docs ProcessJobs
    Background job processor. Opens its own DB connection.
done
func ProcessJobs
    take nothing as bool
    emit result as bool
    fail error as text
body
    conn = db.open("app.db")
    # use conn here freely...
    emit true to :result
done
```

The factory example (`server/Start.fa`) uses this pattern:

```fa
send nowait workflow.RunJobLoop()
```

`RunJobLoop` opens its own database connection:

```fa
flow RunJobLoop
    emit result as RunJobLoopResult
    fail error as RunJobLoopError
body
    state conn = db.open("factory.db")   # its own connection
    step sources.QueuedJobs(conn to :conn) then
        next :job to poll
    done
    # ... process jobs
done
```

## Common Use Cases

### Background Logging

```fa
func ProcessOrder
    take order as dict
    emit result as dict
    fail error as text
body
    order_id = obj.get(order, "id")
    # Process the order ...
    result = obj.set(obj.new(), "status", "processed")

    # Log asynchronously — don't slow down the response
    send nowait AuditLog(order_id to :order_id, result to :result)

    emit result to :result
done
```

### Background Metrics

```fa
func ServeRequest
    take req as dict
    emit result as dict
    fail error as text
body
    start_ms = time.now_ms()
    result = obj.set(obj.new(), "body", "Hello")
    end_ms = time.now_ms()
    elapsed = end_ms - start_ms

    send nowait RecordMetric("request_duration_ms" to :name, elapsed to :value)

    emit result to :result
done
```

### Spawning a Long-Running Worker

```fa
flow ServerMain
    emit result as dict
    fail error as dict
body
    # Spawn background worker, continue to serve HTTP
    send nowait workflow.RunJobLoop()

    step sources.HTTPRequests(8080 to :port) then
        next :req to req
    done
    step handler.HandleRequest(req to :req) done
done
```

### Event Notifications

```fa
func CreateUser
    take conn as db_conn
    take email as text
    emit result as dict
    fail error as text
body
    id = random.uuid()
    params = list.new()
    params = list.append(params, id)
    params = list.append(params, email)
    ok = db.exec(conn, "INSERT INTO users (id, email) VALUES (?1, ?2)", params)

    # Send welcome email in background — do not block user creation
    send nowait email.SendWelcome(email to :email, id to :user_id)

    result = obj.set(obj.new(), "id", id)
    emit result to :result
done
```

## Error Behavior

Errors inside a `send nowait` target do not propagate to the caller. They are written to stderr with a timestamp and a description. This is intentional: since the caller has already moved on, there is nowhere to propagate to.

If you need to capture errors from background work, you should:

- Write errors to a database table or log file inside the target func.
- Use structured `log.error(...)` calls to emit machine-readable error records.
- Poll or tail the log from a monitoring flow.

## nowait for Built-in Ops

For built-in ops, use `nowait` without `send`:

```fa
nowait log.info("Background operation started")
nowait file.write("audit.log", entry)
```

This is fire-and-forget for a single op. The caller does not wait for the op to complete.

## What send nowait Is Not

- It is not a thread pool. The runtime manages scheduling internally (tokio async tasks).
- It is not a message queue. There is no delivery guarantee, retry, or dead-letter handling — if the target fails, the error is logged and discarded.
- It is not suitable for work that the caller needs results from. If you need a result, use a regular call or a `sync` block.

For durable background work with retry and failure handling, use a database-backed job queue: insert a job record, have a worker loop poll for it. The `send nowait workflow.RunJobLoop()` pattern in the factory example is the recommended approach for long-running workers.
