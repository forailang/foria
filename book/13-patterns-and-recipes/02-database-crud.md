# Chapter 13.2: Database CRUD

forai's `db.*` ops provide a straightforward SQLite interface. All operations work through a `db_conn` handle: open a connection, execute statements, query rows, close when done. This chapter covers every operation with annotated examples.

## Opening and Closing Connections

```fa
conn = db.open("app.db")       # file-based SQLite database
conn = db.open(":memory:")     # in-memory database (useful in tests)
db.close(conn)                 # close the connection when done
```

`db.open` returns a `db_conn` handle. The handle type is enforced at compile time — you cannot pass a `db_conn` to `http.server.accept` or vice versa.

Connections opened with `db.open(":memory:")` are ephemeral: data is lost when `db.close` is called or the program exits. Use in-memory databases in tests for fast, isolated state.

## Executing Statements (INSERT, UPDATE, DELETE)

`db.exec` runs a SQL statement that modifies the database. It returns a dict with the field `rows_affected`:

```fa
result = db.exec(conn, "INSERT INTO users (id, name) VALUES ('u1', 'Alice')")
affected = obj.get(result, "rows_affected")
```

### Parameterized Queries

Always use parameterized queries to prevent SQL injection. Parameters are passed as a `list` and referenced in the SQL as `?1`, `?2`, `?3`, etc.:

```fa
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
```

The parameter list is 1-indexed: `?1` is the first element, `?2` the second, and so on. The list must be passed as the third argument to `db.exec` (after the connection and SQL string).

## Querying Rows (SELECT)

`db.query` runs a SELECT statement and returns a list of dicts. Each dict maps column names to values:

```fa
rows = db.query(conn, "SELECT id, name, email FROM users")
# rows is a list of dicts: [{id: "u1", name: "Alice", email: "alice@ex.com"}, ...]
```

Parameterized queries work the same way:

```fa
func GetUserByEmail
    take conn as db_conn
    take email as text
    emit result as dict
    fail error as text
body
    params = list.new()
    params = list.append(params, email)
    rows = db.query(conn, "SELECT id, name, email FROM users WHERE email = ?1", params)
    count = list.len(rows)
    case count
        when 0
            emit "User not found" to :error
        else
            user = rows[0]
            emit user to :result
    done
done
```

## Full CRUD Example

Here is a complete set of CRUD funcs for a `tasks` table:

### Schema Migration

```fa
docs MigrateTasks
    Creates the tasks table if it does not already exist.
done

func MigrateTasks
    take conn as db_conn
    emit result as bool
    fail error as text
body
    ok = db.exec(conn, "CREATE TABLE IF NOT EXISTS tasks (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        description TEXT,
        status TEXT DEFAULT 'todo',
        created_at TEXT DEFAULT CURRENT_TIMESTAMP,
        updated_at TEXT DEFAULT CURRENT_TIMESTAMP
    )")
    emit true to :result
done
```

### Create

```fa
docs CreateTask
    Inserts a new task and returns its ID.
done

func CreateTask
    take conn as db_conn
    take title as text
    take description as text
    emit result as text
    fail error as text
body
    id = random.uuid()
    params = list.new()
    params = list.append(params, id)
    params = list.append(params, title)
    params = list.append(params, description)
    ok = db.exec(conn, "INSERT INTO tasks (id, title, description) VALUES (?1, ?2, ?3)", params)
    emit id to :result
done
```

### Read (Single)

```fa
docs GetTask
    Retrieves a single task by ID.
done

func GetTask
    take conn as db_conn
    take task_id as text
    emit result as dict
    fail error as text
body
    params = list.new()
    params = list.append(params, task_id)
    rows = db.query(conn, "SELECT id, title, description, status, created_at FROM tasks WHERE id = ?1", params)
    count = list.len(rows)
    case count
        when 0
            emit "Task not found" to :error
        else
            task = rows[0]
            emit task to :result
    done
done
```

### Read (List)

```fa
docs ListTasks
    Returns all tasks, optionally filtered by status.
done

func ListTasks
    take conn as db_conn
    take status_filter as text
    emit result as list
    fail error as text
body
    rows_all = list.new()
    case status_filter
        when "all"
            rows_all = db.query(conn, "SELECT id, title, status, created_at FROM tasks ORDER BY created_at DESC")
        else
            params = list.new()
            params = list.append(params, status_filter)
            rows_all = db.query(conn, "SELECT id, title, status, created_at FROM tasks WHERE status = ?1 ORDER BY created_at DESC", params)
    done
    emit rows_all to :result
done
```

### Update

```fa
docs UpdateTask
    Updates the title, description, and status of a task.
done

func UpdateTask
    take conn as db_conn
    take task_id as text
    take title as text
    take description as text
    take status as text
    emit result as bool
    fail error as text
body
    params = list.new()
    params = list.append(params, title)
    params = list.append(params, description)
    params = list.append(params, status)
    params = list.append(params, task_id)
    ok = db.exec(conn, "UPDATE tasks SET title = ?1, description = ?2, status = ?3, updated_at = CURRENT_TIMESTAMP WHERE id = ?4", params)
    affected = obj.get(ok, "rows_affected")
    case affected
        when 0
            emit "Task not found" to :error
        else
            emit true to :result
    done
done
```

### Delete

```fa
docs DeleteTask
    Deletes a task by ID.
done

func DeleteTask
    take conn as db_conn
    take task_id as text
    emit result as bool
    fail error as text
body
    params = list.new()
    params = list.append(params, task_id)
    ok = db.exec(conn, "DELETE FROM tasks WHERE id = ?1", params)
    affected = obj.get(ok, "rows_affected")
    case affected
        when 0
            emit "Task not found" to :error
        else
            emit true to :result
    done
done
```

## Testing with In-Memory Databases

In-memory databases are ideal for tests because they start empty, require no cleanup, and are fast:

```fa
test CreateTask
    conn = db.open(":memory:")
    ok = db.exec(conn, "CREATE TABLE IF NOT EXISTS tasks (id TEXT PRIMARY KEY, title TEXT NOT NULL, description TEXT, status TEXT DEFAULT 'todo', created_at TEXT DEFAULT CURRENT_TIMESTAMP, updated_at TEXT DEFAULT CURRENT_TIMESTAMP)")
    id = CreateTask(conn to :conn, "Write docs" to :title, "Write chapter 13" to :description)
    must id != ""
    rows = db.query(conn, "SELECT * FROM tasks WHERE id = '" + id + "'")
    must list.len(rows) == 1
    task = rows[0]
    must obj.get(task, "title") == "Write docs"
    db.close(conn)
done
```

## Transactions

SQLite transactions can be managed manually via `db.exec`:

```fa
func TransferPoints
    take conn as db_conn
    take from_id as text
    take to_id as text
    take points as long
    emit result as bool
    fail error as text
body
    ok = db.exec(conn, "BEGIN TRANSACTION")
    params_debit = list.new()
    params_debit = list.append(params_debit, points)
    params_debit = list.append(params_debit, from_id)
    debit = db.exec(conn, "UPDATE accounts SET balance = balance - ?1 WHERE id = ?2", params_debit)

    params_credit = list.new()
    params_credit = list.append(params_credit, points)
    params_credit = list.append(params_credit, to_id)
    credit = db.exec(conn, "UPDATE accounts SET balance = balance + ?1 WHERE id = ?2", params_credit)

    ok = db.exec(conn, "COMMIT")
    emit true to :result
done
```

For error handling with transactions, use `fail` and `ROLLBACK` in the error path.

## Tips

- Always use parameterized queries (`?1`, `?2`, ...) — never interpolate user input directly into SQL strings.
- Open the connection once at the start of your server or worker and keep it open for the lifetime of the process. Reconnecting on every request is slow.
- In `sync` blocks, the same `conn` can be shared across concurrent statements (they share the parent's scope). SQLite handles concurrent reads well; concurrent writes serialize automatically.
- Use `db.open(":memory:")` in all `test` blocks for fast, isolated tests.
