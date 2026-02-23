# Chapter 15.4: Error Messages

forai compiler errors are precise: every error includes a file path, line number, column number, and a message describing what went wrong. This chapter documents the most common errors, what causes them, and how to fix them.

## Error Format

```
path/to/file.fa:line:col: error message
```

Examples:
```
app/handler/Process.fa:5:1: missing docs block for func `ProcessItem`
app/handler/Route.fa:23:9: unknown op `db.queery`
app/main.fa:1:1: circular dependency: main → auth → main
```

---

## Compiler Errors

### Missing docs block

```
app/handler/Process.fa:5:1: missing docs block for func `ProcessItem`
```

**Cause:** Every `func`, `flow`, `sink`, `source`, `type`, `enum`, and `test` must have a `docs Name` block immediately before it (or before the type declarations that precede it). This one is missing.

**Fix:** Add a `docs` block:

```fa
docs ProcessItem
    Processes a single queue item and returns its result.
done

func ProcessItem
    ...
```

---

### Name-filename mismatch

```
app/handler/Process.fa:3:6: name mismatch: file is `Process.fa` but func declares `ProcessItem`
```

**Cause:** The callable name in the file (`func ProcessItem`) does not match the filename stem (`Process`). forai enforces one callable per file with a matching name.

**Fix:** Either rename the file to `ProcessItem.fa` or rename the func to `Process`.

---

### Circular dependency

```
app/main.fa:1:1: circular dependency: main → auth → util → main
```

**Cause:** Module `main` imports `auth`, which imports `util`, which imports `main`. This cycle cannot be resolved.

**Fix:** Break the cycle. Usually the fix is to extract the shared code into a third module that neither of the cyclic modules imports. In the example, move the shared logic from `main` into a new `shared` module.

---

### Unknown op

```
app/handler/Process.fa:18:14: unknown op `db.queery`
```

**Cause:** A built-in op name is misspelled or does not exist. `db.queery` is not a real op — `db.query` is.

**Fix:** Check the op name spelling. Refer to the Built-in Ops table in Chapter 10 for the full list of available ops in each namespace.

Common misspellings:
- `db.queery` → `db.query`
- `str.lenth` → `str.len`
- `list.lenght` → `list.len`
- `http.server.accpet` → `http.server.accept`
- `obj.merge` → `obj.merge` (this one exists — check args)

---

### Handle type mismatch

```
app/handler/Process.fa:22:9: handle type mismatch: expected db_conn, got http_server
```

**Cause:** A variable holding an `http_server` handle was passed to `db.query`, which expects a `db_conn`. Handle types are checked at compile time.

**Fix:** Ensure the correct handle is passed to each op. If you have both `conn` (db_conn) and `srv` (http_server), do not mix them up:

```fa
# Wrong:
rows = db.query(srv, "SELECT * FROM users")

# Correct:
rows = db.query(conn, "SELECT * FROM users")
```

---

### Orphan docs block

```
app/handler/Process.fa:8:1: orphan docs block: `OldFunc` has no corresponding declaration
```

**Cause:** A `docs OldFunc` block exists but there is no `func OldFunc`, `flow OldFunc`, `type OldFunc`, etc. following it. This usually happens when a func is renamed or deleted but its docs block is left behind.

**Fix:** Remove the orphan docs block, or rename it to match the actual declaration.

---

### func has no emit output

```
app/handler/Process.fa:5:1: func `ProcessItem` has no emit output
```

**Cause:** A `func` body was parsed but contains no `emit` statement. Every `func` and `sink` must emit on at least one output path.

**Fix:** Add an `emit` statement. Every code path through the func body must reach an `emit` or `fail`:

```fa
func ProcessItem
    take item as dict
    emit result as dict
    fail error as text
body
    # Must emit or fail on every path
    ok = obj.get(item, "ok")
    case ok
        when true
            result = obj.set(obj.new(), "status", "done")
            emit result to :result   # <- required
        else
            emit "Item not ok" to :error   # <- required
    done
done
```

---

### main must be a flow

```
app/main.fa:3:1: `main` must be a `flow`, but it is declared as `func`
```

**Cause:** The entry point `main` is declared as `func main` instead of `flow main`. The language rule is: `main` is always a `flow`.

**Fix:** Change `func main` to `flow main` and restructure the body to use `step`-based wiring instead of an imperative body.

---

### Missing fail port

```
app/handler/Process.fa:5:1: func `ProcessItem` declares `fail error as text` but no `emit ... to :error` is reachable
```

**Cause:** A `fail` port is declared in the header but the body never actually emits to it. This can cause unreachable error handling in callers.

**Fix:** Either remove the `fail` declaration if the func cannot fail, or add an `emit ... to :error` in the appropriate error paths.

---

### Case arm variable scope

```
app/handler/Process.fa:34:5: variable `result` used before assignment
```

**Cause:** A variable is assigned inside a `case` arm and then used after the `case` block closes. The IR lowerer does not merge variables from case arm scopes into the outer scope. This is a scoping rule enforcement.

**Fix:** Initialize the variable before the `case` block with a default value:

```fa
# WRONG:
case status
    when "ok"
        result = "success"
    else
        result = "failure"
done
term.print(result)  # error: result not in scope

# Correct:
result = "failure"   # default before case
case status
    when "ok"
        result = "success"
    else
done
term.print(result)  # ok — result was initialized before case
```

---

### Duplicate docs block

```
app/handler/Process.fa:12:1: duplicate docs block for `ProcessItem` — already defined at line 4
```

**Cause:** Two `docs ProcessItem` blocks exist in the same file (or across files in a module). Each construct may have only one docs block.

**Fix:** Remove the duplicate. If the two docs blocks describe different things, one of the constructs may need to be renamed.

---

### Uses not found

```
app/main.fa:1:5: module not found: `./handler` — no such directory or file
```

**Cause:** A `use handler from "./handler"` declaration references a path that does not exist relative to the importing file's directory.

**Fix:** Check the path. Paths are relative to the `.fa` file, not the project root:
- `use handler from "./handler"` looks for `./handler/` directory relative to the importing file.
- `use Handler from "./Handler.fa"` looks for `Handler.fa` as a sibling file.

---

### must assertion failure

At runtime during `forai test`:

```
app/handler/Process.fa:45:5: must result.status == "done" — got result.status = "pending", result = {status: "pending", id: "x"}
```

**Cause:** A `must` assertion in a test block evaluated to `false`. The error shows:
- The file, line, and column of the `must` statement.
- The full expression that was asserted.
- The resolved values of variables referenced in the expression.

**Fix:** Investigate why the value is wrong. The resolved values give you the actual state at the time of the assertion. Common causes:
- A `mock` directive returned a wrong value.
- The func under test has a bug in its logic.
- The test setup (e.g. DB migration) was incomplete.

---

## Runtime Errors

### Division by zero

```
runtime error at app/Compute.fa:18: division by zero: 0 / 0
```

**Cause:** A `/` or `%` operation was evaluated with a zero denominator.

**Fix:** Add a guard before the division:

```fa
result = 0.0
case denominator
    when 0
    else
        result = to.real(numerator) / to.real(denominator)
done
```

### Handle not found

```
runtime error at app/Query.fa:22: handle not found: conn
```

**Cause:** A handle variable was used after it was closed, or a `send nowait` task tried to use a handle from the parent scope.

**Fix:** Do not close handles before all uses are complete. In `send nowait` tasks, open new handles rather than using parent handles.

### File not found

```
runtime error at app/Load.fa:8: file not found: data/config.json
```

**Cause:** `file.read(path)` was called with a path that does not exist.

**Fix:** Check that the file exists before reading, or catch the failure with a `fail` path:

```fa
exists = file.exists("data/config.json")
case exists
    when true
        content = file.read("data/config.json")
    else
        emit "Config file not found" to :error
done
```
