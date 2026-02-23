# Chapter 11.4: Handle Sharing

Handles are opaque references to stateful external resources: database connections, HTTP servers, WebSocket connections, and HTTP client connections. They are created by specific ops and consumed by other ops in the same namespace. Understanding how handles are owned and shared across concurrent contexts is essential for writing correct forai programs.

## Handle Types

| Type | Created by | Used by |
|------|-----------|---------|
| `db_conn` | `db.open(path)` | `db.exec`, `db.query`, `db.close` |
| `http_server` | `http.server.listen(port)` | `http.server.accept`, `http.server.close` |
| `http_conn` | `http.server.accept(server)` | `http.server.respond` |
| `ws_conn` | `ws.connect(url)` | `ws.send`, `ws.recv`, `ws.close` |

## Ownership Rule

The func or flow that opens a handle owns it. Child funcs can use the handle, but they do not own it and should not close it. Only the opener closes.

```fa
func OpenAndQuery
    take query_text as text
    emit result as list
    fail error as text
body
    conn = db.open("app.db")             # opens — owns the handle
    rows = db.query(conn, query_text)    # uses the handle
    db.close(conn)                       # closes — owner is responsible
    emit rows to :result
done
```

## Passing Handles to Child Funcs

Handles are passed as arguments using the standard `take conn as db_conn` port declaration. A child func declares the handle type it needs and uses it without opening or closing it:

```fa
docs ListUsers
    Queries all users from the database.
done

func ListUsers
    take conn as db_conn        # receives handle from caller
    emit result as list
    fail error as text
body
    rows = db.query(conn, "SELECT id, email FROM users")
    emit rows to :result
done
```

```fa
docs App
    Opens the database, runs queries, closes.
done

func App
    take nothing as bool
    emit result as dict
    fail error as text
body
    conn = db.open("app.db")
    users = ListUsers(conn to :conn)     # passes handle
    db.close(conn)                        # owner closes
    emit users to :result
done
```

The handle type is checked at compile time. If `ListUsers` takes `db_conn` but is called with an `http_conn`, the compiler reports a type mismatch.

## sync Blocks Share the Parent's Registry

A `sync` block runs inside the same func context as its parent. Its statements share the parent's handle registry — all handles opened before the sync block are accessible inside every sync statement.

```fa
func ParallelQueries
    take conn as db_conn
    emit result as dict
    fail error as text
body
    # conn is in the parent scope — accessible inside sync
    [users, jobs, items] = sync :timeout => 5s
        users = db.query(conn, "SELECT * FROM users")
        jobs = db.query(conn, "SELECT * FROM jobs")
        items = db.query(conn, "SELECT * FROM items")
    done [users, jobs, items]

    result = obj.new()
    result = obj.set(result, "users", users)
    result = obj.set(result, "jobs", jobs)
    result = obj.set(result, "items", items)
    emit result to :result
done
```

All three queries use the same `conn`. This works because sync statements share the parent's scope — they each get a **copy** of the scope at the point the sync block starts, and `conn` is in that scope.

### Open Handles Before sync to Avoid ID Collisions

If you open handles inside a `sync` body, each statement gets its own scope copy. If two statements both call `db.open(...)`, each will open its own separate connection — that is fine, but they will have different handle IDs. If you want multiple statements to share one connection, open it before the sync block:

```fa
func SharedHandle
    take nothing as bool
    emit result as bool
    fail error as text
body
    # Open once before the sync block
    conn = db.open("shared.db")

    [a, b] = sync
        a = db.query(conn, "SELECT count(*) FROM users")
        b = db.query(conn, "SELECT count(*) FROM jobs")
    done [a, b]

    db.close(conn)
    emit true to :result
done
```

If instead each statement opened its own connection, you would get two separate connections, which is valid but wastes resources.

## send nowait Isolation

A `send nowait` task receives an **isolated** handle registry. It cannot access handles from the parent at all. This is by design: the parent may close its handles before the spawned task even starts running, which would produce dangling references.

```fa
# Incorrect — trying to share conn with a background task
func StartServer
    take nothing as bool
    emit result as bool
    fail error as text
body
    conn = db.open("app.db")
    send nowait WorkerLoop(conn to :conn)  # conn not accessible inside WorkerLoop
    emit true to :result
done
```

The correct pattern for background tasks that need database access is to open a new connection inside the target func:

```fa
docs WorkerLoop
    Background worker. Opens its own DB connection.
done

func WorkerLoop
    take nothing as bool
    emit result as bool
    fail error as text
body
    conn = db.open("app.db")   # independent connection
    loop_active = true
    loop_count = 0
    loop list.range(0, 1000) as _
        rows = db.query(conn, "SELECT * FROM jobs WHERE status = 'queued' LIMIT 10")
        # process rows...
        loop_count = loop_count + 1
    done
    db.close(conn)
    emit true to :result
done
```

And the caller simply fires it:

```fa
send nowait WorkerLoop(true to :nothing)
```

## Full Example: Setup + Insert + Query Pattern

The following example demonstrates the full ownership and sharing model: a parent func opens a connection, passes it to setup and insert funcs, then queries the results.

```fa
docs SetupSchema
    Creates the users table if it does not exist.
done

func SetupSchema
    take conn as db_conn
    emit result as bool
    fail error as text
body
    ok = db.exec(conn, "CREATE TABLE IF NOT EXISTS users (id TEXT PRIMARY KEY, name TEXT, email TEXT)")
    emit true to :result
done

docs InsertUser
    Inserts a single user record.
done

func InsertUser
    take conn as db_conn
    take name as text
    take email as text
    emit result as text
    fail error as text
body
    id = random.uuid()
    params = list.new()
    params = list.append(params, id)
    params = list.append(params, name)
    params = list.append(params, email)
    ok = db.exec(conn, "INSERT INTO users (id, name, email) VALUES (?1, ?2, ?3)", params)
    emit id to :result
done

docs Demo
    Opens DB, sets up schema, inserts users in parallel, queries all.
done

func Demo
    take nothing as bool
    emit result as list
    fail error as text
body
    conn = db.open(":memory:")

    # Setup schema first — must complete before inserts
    ok = SetupSchema(conn to :conn)

    # Insert two users concurrently
    [id_a, id_b] = sync :timeout => 5s
        id_a = InsertUser(conn to :conn, "Alice" to :name, "alice@example.com" to :email)
        id_b = InsertUser(conn to :conn, "Bob" to :name, "bob@example.com" to :email)
    done [id_a, id_b]

    # Query all users
    rows = db.query(conn, "SELECT id, name, email FROM users")

    db.close(conn)
    emit rows to :result
done
```

Key observations:
- `conn` is opened by `Demo`, the owner.
- `SetupSchema` and `InsertUser` take `conn` as a parameter — they use but do not own it.
- The `sync` block shares `conn` from the parent scope.
- `Demo` closes `conn` at the end.
- If a `send nowait` were used here, it could not share `conn`.
