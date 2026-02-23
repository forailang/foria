# sink

A `sink` is the terminal stage of a pipeline branch. It receives data, performs a side effect — printing to a terminal, writing to a database, sending an HTTP response — and signals completion. Sinks are the "shipping dock" of the assembly line: where processed goods leave the factory.

## Syntax

Sinks are syntactically identical to funcs. The only difference is the keyword:

```fa
docs Print
    Terminal output sink for the documentation browser.
    Receives formatted text and prints it to the terminal.
done

sink Print
    take text as text
    emit result as bool
    fail error as text
body
    _ = term.print("")
    _ = term.print(text)
    ok = true
    emit ok
done

test Print
    r = Print("hello test")
    must r == true
done
```

Replace `func` with `sink` and everything else is the same: `docs` block, port declarations, `body...done`, `test` block.

## Why a Separate Keyword?

The `sink` keyword is a semantic signal, not a syntactic constraint. It communicates several things:

1. **Intent.** A reader scanning the codebase knows immediately that a `sink` is a terminal output stage. It is not intermediate computation.
2. **Architecture enforcement.** Sinks should appear at the ends of pipeline branches. The keyword makes accidental misuse visible in code review.
3. **Documentation.** The `doc` command generates separate sections for sources, funcs, flows, and sinks, making the architecture of a system visible in its documentation.

The compiler does not structurally prevent a sink from being called mid-pipeline, but convention — enforced through code review and the doc tool — keeps sinks at the leaves.

## Port Declarations

Sinks have the same port declarations as funcs:

- `take` — the input. What the sink receives.
- `emit` — the success signal. Typically `emit done as bool` or `emit result as bool`.
- `fail` — the failure output. Required.

The emit value of a sink is usually just a boolean confirming that the side effect succeeded. The caller (the flow) typically does not use this value for further computation.

```fa
sink WriteRecord
    take record as Record
    emit done as bool
    fail error as text
body
    query = "INSERT INTO records (id, name) VALUES (?, ?)"
    params = list.new()
    params = list.append(params, record.id)
    params = list.append(params, record.name)
    _ = db.exec(conn, query, params)
    emit true
done
```

## Common Sink Patterns

### Terminal Output

The most common sink: print something to the terminal.

```fa
docs Print
    Prints a line of text to the terminal.
done

sink Print
    take line as text
    emit done as bool
    fail error as text
body
    _ = term.print(line)
    ok = true
    emit ok
done

test Print
    r = Print("hello")
    must r == true
done
```

### HTTP Response

In a web server, a sink sends the HTTP response back to the client.

```fa
docs SendResponse
    Sends a JSON response to the HTTP client.
done

sink SendResponse
    take payload as Response
    emit done as bool
    fail error as text
body
    body_json = json.encode(payload)
    _ = http.server.respond(conn, 200, body_json)
    emit true
done

test SendResponse
    mock http.server.respond => true
    r = SendResponse(sample_payload)
    must r == true
done
```

### Database Write

```fa
docs StoreUser
    Persists a new user record to the database.
done

sink StoreUser
    take user as User
    emit done as bool
    fail error as text
body
    sql = "INSERT INTO users (id, name, email) VALUES (?, ?, ?)"
    params = [user.id, user.name, user.email]
    _ = db.exec(conn, sql, params)
    emit true
done

test StoreUser
    mock db.exec => true
    u = sample_user()
    r = StoreUser(u)
    must r == true
done
```

## Using Sinks in Flows

Sinks are called in a flow `step` the same way as funcs. Because sinks are typically at the end of a branch, the step usually ends with just `done` (no `then` block):

```fa
flow ShowResult
body
    step sources.Commands() then
        next :cmd to cmd
    done
    step Process(cmd to :cmd) then
        next :result to output
    done
    step sinks.Print(output to :line) done     # sink at end of branch
done
```

If you need the sink's boolean result for some reason, use the full `then...next...done` form:

```fa
step sinks.WriteRecord(record to :record) then
    next :done to ok
done
```

## Sinks vs Funcs: When to Choose

Use a `sink` when:
- The callable's purpose is a terminal side effect (output, persistence, response)
- The result is not fed into further computation
- You want readers to immediately understand this is an exit point

Use a `func` when:
- The callable transforms data for use by another stage
- The result matters to the caller
- The operation is a building block, not a final output

In practice, the distinction is architectural and documentary. A team may choose to use `func` everywhere and reserve `sink` for the most obviously terminal operations. The language does not prevent either approach.

## Testing Sinks

Sinks that perform real I/O (terminal, file, network) should be tested with `mock` for the underlying ops in unit tests, and with integration tests for the full I/O path:

```fa
docs PrintTest
    Verifies that Print succeeds on valid input.
done

test PrintTest
    r = Print("test line")
    must r == true
done
```

Because `term.print` in the test environment is a stub that always succeeds, the test passes without actually printing to a terminal. The test verifies the logic of the sink (variable assignment, emit path) without requiring real terminal access.

For sinks that call external services or databases, use `mock`:

```fa
docs StoreUserTest
    Verifies the happy path of user storage.
done

test StoreUserTest
    mock db.exec => true
    u = sample_user()
    r = StoreUser(u)
    must r == true
done
```

## Rules and Gotchas

- Sinks are syntactically identical to funcs. The keyword is the only difference.
- Every sink must have both `emit` and `fail` port declarations — same as funcs.
- Every sink must have a `docs` block and a `test` block.
- By convention, sinks appear at the ends of pipeline branches, never mid-pipeline.
- The `emit` value of a sink is usually `bool` — just confirming success. The flow typically ignores it.
- Sinks may call built-in ops freely. The most common are `term.print`, `db.exec`, `http.server.respond`, `file.write`.
