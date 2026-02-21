# Dataflow: A Flowchart Programming Language

Dataflow is a language where you program by drawing flowcharts. Instead of writing procedures that run top-to-bottom, you define **boxes** (sources, funcs, sinks) and **wires** (flows) that connect them. Data enters through sources, gets processed by funcs, and exits through sinks. Flows are the flowchart itself — they say which box connects to which.

## The Mental Model

Think of a factory floor:

```
[Source: Orders]  →  [Func: Validate]  →  [Func: Price]  →  [Sink: Send Invoice]
```

- **Source** — the loading dock. Events arrive here (HTTP requests, user input, database polls)
- **Func** — a workstation. Takes input, does computation, produces output
- **Sink** — the shipping bay. Side effects leave here (print to screen, send response, write file)
- **Flow** — the conveyor belt. Connects sources to funcs to sinks. The flow never does work itself — it just says what connects to what

Every program starts with `flow main`. That's your top-level flowchart.

## Quick Start: Hello World

```
docs main
  Prints hello.
done

flow main
body
  step display.Print("Hello, world!" to :text) done
done
```

Funcs and sinks declare their **interface** — what they take in, what they emit out, and what they fail with. Flows optionally declare `take`/`emit`/`fail` ports — but a flow that just wires sources to funcs to sinks needs none. Then the `body` says what happens.

## The Four Building Blocks

### 1. `source` — Where Data Enters

A source is a box that produces events. It blocks until the next event is ready, then yields it. The flowchart never sees the waiting.

```
source Commands
  emit cmd as text
  fail error as text
from term.prompt("docs> ") as raw
  trimmed = str.trim(raw)
  emit trimmed
  case trimmed
    when "quit"
      break
    when "exit"
      break
  done
done
```

A source has two forms:

**Poll form** — `from expr as var`: repeatedly calls the expression, runs the body for each result, `emit` sends it downstream, `break` stops the source.

**Init + poll form** — set up a resource once, then poll it:

```
source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
init
  srv = http.server.listen(port)
from http.server.accept(srv)
done
```

**Body form** — full imperative loop with explicit `emit`:

```
source QueuedJobs
  take conn as db_conn
  emit job as dict
  fail error as text
body
  loop
    rows = db.query(conn, "SELECT * FROM jobs WHERE status = 'Queued' LIMIT 1")
    count = list.len(rows)
    case count
      when 0
        _ = time.sleep(3)
      else
        job = list.get(rows, 0)
        result = {type: "queued", job: job}
        emit result
    done
  done
done
```

### 2. `func` — Where Computation Happens

A func is a box that takes input, does work, and produces output. All imperative logic lives here: math, string ops, conditionals, loops, database queries.

```
func Classify
  take cmd as text
  emit result as text
  fail error as text
body
  if cmd == "help"
    r = "help"
    emit r
  else if str.contains(cmd, ".")
    r = "op"
    emit r
  else
    r = "unknown"
    emit r
  done
done
```

A func with `return`/`fail` (single-output shorthand):

```
func CreateJob
  take conn as db_conn
  take title as text
  take spec as text
  return text
  fail text
body
  id = random.uuid()
  params = [id, title, spec]
  ok = db.exec(conn, "INSERT INTO jobs (id, title, spec) VALUES (?1, ?2, ?3)", params)
  return id
done
```

### 3. `sink` — Where Results Leave

A sink is like a func but marks a terminal side effect — printing, responding, writing.

```
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
```

### 4. `flow` — The Flowchart Itself

A flow wires the other three together. No computation, no loops, no I/O — just **steps** that name the boxes and **wires** that connect them.

```
flow Start
body
  step display.Welcome() done
  step sources.Commands() then
    next :cmd to cmd
  done
  step router.Classify(cmd to :cmd) then
    next :result to kind
  done
  branch when kind == "help"
    step data.HelpText() then
      next :result to output
    done
    step display.Print(output to :text) done
  done
  branch when kind == "ls"
    step data.Namespaces() then
      next :result to output
    done
    step display.Print(output to :text) done
  done
  branch when kind == "quit"
    step display.Print("Goodbye!" to :text) done
  done
  branch when kind == "unknown"
    step display.PrintError(cmd to :cmd) done
  done
done
```

**Flow vocabulary:**
- `step Func(arg to :port) done` — call a func/flow/sink, mapping arguments to its input ports
- `step Func(...) then ... done` — call and handle outputs
- `next :output_port to wire_name` — bind a callee's output to a local wire
- `emit wire to :flow_output` — send a wire's value to the flow's output port
- `fail wire to :flow_output` — send a wire's value to the flow's failure port
- `state x = op(...)` — initialize a shared resource (runs once)
- `send nowait Module.Func()` — fire-and-forget a background task
- `branch when <expr> ... done` — conditional sub-pipeline (runs body only if true)
- `branch ... done` — unguarded sub-pipeline (always runs)

## Language Syntax

### Every Statement is an Assignment or Action

There are no bare expressions. Every line is one of:
- `var = expression` — assignment
- `emit var` / `return var` — success output
- `fail var` — failure output
- `if`/`case`/`loop`/`sync`/`break` — control flow
- `_ = something()` — call whose result you discard

```
# WRONG - bare expression:
0

# RIGHT - assign it:
_ = 0
```

### emit/fail/return Require Variables

```
# WRONG:
emit true

# RIGHT:
ok = true
emit ok
```

### Expressions

**Operators** (standard precedence): `+` `-` `*` `/` `%` `**` `==` `!=` `<` `>` `<=` `>=` `&&` `||` `!`

**Ternary**: `result = condition ? "yes" : "no"`

**String interpolation**: `msg = "Hello #{name}, you have #{count} items"`

**List literals**: `items = [1, 2, 3]` or `empty = []`

**Dict literals**: `config = {host: "localhost", port: 8080}` or `empty = {}`

**Function calls**: `result = str.upper(name)` or `result = MyFunc(arg1, arg2)`

String `+` does concatenation. `#{}` inside strings evaluates expressions. Escapes: `\n` `\t` `\\` `\"` `\#`.

### Control Flow (Inside func/sink/source Bodies)

**if/else if/else:**
```
if cmd == "help"
  r = "help"
  emit r
else if cmd == "quit"
  r = "quit"
  emit r
else
  r = "unknown"
  emit r
done
```

**case/when** (pattern matching):
```
case method
  when "GET"
    result = handle_get(path)
    emit result
  when "POST"
    result = handle_post(path, body)
    emit result
  else
    err = "Method not allowed"
    fail err
done
```

**loop** (collection):
```
items = list.range(0, 10)
loop items as i
  _ = term.print("Item #{i}")
done
```

The collection must be a variable — `loop list.range(0,10) as i` is invalid. Assign it first.

**loop** (bare/infinite):
```
loop
  req = http.server.accept(srv)
  _ = process(req)
  if should_stop
    break
  done
done
```

**sync** (concurrent execution):
```
[a, b] = sync
  a = fetch_users()
  b = fetch_orders()
done [a, b]
```

Statements inside sync run in parallel. Each gets its own scope — they can't reference each other.

### Modules

Organize code into folders. Import with `uses`:

```
uses auth
uses db

# Call as namespace.Function:
token = auth.Login(email, password)
user = db.GetUser(conn, user_id)
```

Rules:
- One func/flow/sink per file. Name must match filename (`func Foo` in `Foo.fa`)
- `uses` resolves relative to the importing file's directory
- `server/Start.fa` with `uses db` looks for `server/db/`, not top-level `db/`

### Types

**Scalar** (named wrapper with constraints):
```
type Email as text, :matches => /@/
```

**Struct:**
```
type User
  id    uuid :required => true
  email Email
  age   long :min => 0
done
```

**Enum:**
```
enum Role
  Admin
  User
  Guest
done
```

Primitive types: `text`, `bool`, `long` (i64), `real` (f64), `uuid`, `time`, `list`, `dict`, `void`, `db_conn`, `http_server`, `http_conn`, `ws_conn`.

Handle types (`db_conn`, `http_server`, `http_conn`, `ws_conn`) are opaque — they cannot be constructed from string literals. They are produced by specific ops (`db.open`, `http.server.listen`, etc.) and the compiler validates correct usage at compile time.

### Documentation (Required)

Every func, flow, sink, and type must have a `docs` block. The compiler enforces this.

```
docs CreateJob
  Inserts a new job into the database and returns its generated ID.
done

func CreateJob
  ...
done
```

For struct types, each field needs documentation too:

```
docs User
  A system user.

  docs id
    Unique identifier.
  done

  docs email
    Login email address.
  done
done

type User
  id uuid
  email text
done
```

### Tests

```
docs TestClassify
  Verifies command classification.
done

test TestClassify
  must Classify("help") == "help"
  must Classify("quit") == "quit"
done
```

**Failure testing** with `trap`:
```
test BadInput
  err = trap Validate(bad_data)
  must err == "invalid"
done
```

**Mocking** sub-calls:
```
test Mocked
  mock api.Fetch => {status: 200}
  result = Process(input)
  must result.ok == true
done
```

## Built-in Operations

The runtime provides 160+ ops across namespaces. All called as `namespace.op(args)`.

| Namespace | What | Key Ops |
|-----------|------|---------|
| `obj.*` | Dicts (immutable) | `new`, `set`, `get`, `has`, `delete`, `keys`, `merge` |
| `list.*` | Lists (immutable) | `new`, `range`, `append`, `get`, `len`, `contains`, `slice` |
| `str.*` | Strings | `len`, `upper`, `lower`, `trim`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `slice`, `index_of` |
| `math.*` | Arithmetic | `add`, `subtract`, `multiply`, `divide`, `floor`, `mod`, `power`, `round` |
| `to.*` | Type conversion | `text`, `long`, `real`, `bool` |
| `type.*` | Introspection | `of` (returns `"text"`, `"long"`, etc.) |
| `json.*` | JSON | `decode`, `encode`, `encode_pretty` |
| `http.*` | HTTP client | `get`, `post`, `put`, `delete`, `response`, `error_response` |
| `http.server.*` | HTTP server | `listen`, `accept`, `respond`, `close` |
| `ws.*` | WebSocket | `connect`, `send`, `recv`, `close` |
| `db.*` | SQLite (`db_conn` handles) | `open`, `exec`, `query`, `close` |
| `file.*` | File I/O | `read`, `write`, `append`, `delete`, `exists`, `list`, `mkdir` |
| `term.*` | Terminal | `print`, `prompt`, `clear`, `read_key` |
| `exec.*` | Processes | `run` (command, args_list) |
| `regex.*` | Regex | `match`, `find`, `find_all`, `replace`, `split` |
| `random.*` | Random | `int`, `float`, `uuid`, `choice`, `shuffle` |
| `date.*` | Calendar dates | `now`, `from_iso`, `to_iso`, `add`, `diff`, `weekday` |
| `time.*` | Utilities | `sleep` (seconds) |
| `env.*` | Environment | `get`, `set`, `has` |
| `log.*` | Logging | `debug`, `info`, `warn`, `error` |
| `crypto.*` | Security | `hash_password`, `verify_password`, `sign_token`, `verify_token` |
| `hash.*` | Hashing | `sha256`, `sha512`, `hmac` |
| `base64.*` | Encoding | `encode`, `decode` |
| `html.*` | HTML | `escape`, `unescape` |
| `tmpl.*` | Templates | `render` (Mustache-style) |
| `route.*` | URL routing | `match` (`:param`, `*wildcard` patterns) |
| `url.*` | URL parsing | `parse`, `query_parse`, `encode`, `decode` |
| `cookie.*` | Cookies | `parse`, `get`, `set`, `delete` |
| `error.*` | Errors | `new`, `wrap`, `code`, `message` |

## Patterns

### HTTP Server

```
flow Start
body
  state conn = db.open("app.db")
  step db.Migrate(conn to :conn) done
  step sources.HTTPRequests(8080 to :port) then
    next :req to req
  done
  step handler.HandleRequest(conn to :conn, req to :req) done
done
```

With the source:

```
source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
init
  srv = http.server.listen(port)
from http.server.accept(srv)
done
```

The flow opens the database once (`state`), starts listening for HTTP requests from the source, and pipes each request to a handler. The source handles the accept loop. The flow is the flowchart — it never loops itself.

### Background Tasks

```
send nowait workflow.RunJobLoop()
```

Fire-and-forget. The background task runs independently with its own scope.

### Request Routing

```
flow HandleRequest
  take conn as db_conn
  take req as dict
body
  step router.Route(req to :req) then
    next :result to handler
  done
  branch when handler == "home"
    step pages.Home(conn to :conn, req to :req) done
  done
  branch when handler == "about"
    step pages.About(req to :req) done
  done
  branch when handler == "not_found"
    step pages.NotFound(req to :req) done
  done
done
```

A router func classifies the request, then `branch when` blocks route to the right handler — like a flowchart diamond.

### Database Operations

```
func CreateJob
  take conn as db_conn
  take title as text
  return text
  fail text
body
  id = random.uuid()
  params = [id, title]
  ok = db.exec(conn, "INSERT INTO jobs (id, title) VALUES (?1, ?2)", params)
  return id
done
```

Use `db.open` for connections (returns `db_conn`), `db.exec` for writes, `db.query` for reads. Query returns a list of dicts. Funcs that receive a connection declare `take conn as db_conn`.

## Gotchas

1. **No bare expressions** — every line must be `var = ...`, `emit`, `fail`, `return`, or control flow
2. **`emit`/`fail`/`return` take variables only** — `emit true` fails; use `ok = true` then `emit ok`
3. **Loop collection must be a variable** — `loop list.range(0,5) as i` fails; assign the range first
4. **`exec.run` needs separate command and args** — `exec.run("ls -la")` fails; use `exec.run("ls", ["-la"])`
5. **`uses` is relative to the file** — not the project root
6. **One callable per file** — name must match filename
7. **Docs are mandatory** — compiler rejects missing `docs` blocks
8. **Flows don't compute** — no `+`, no `str.upper`, no function calls except `step` invocations
9. **`_` discards a return value** — use `_ = op(...)` when you don't need the result
10. **All data structures are immutable** — `obj.set` and `list.append` return new copies

## File Structure

A typical project:

```
my-app/
  main.fa              # flow main — entry point (the top-level flowchart)
  server/
    Start.fa           # flow Start — wires source + handler
    sources/
      HTTPRequests.fa  # source — accepts connections
    handler/
      HandleRequest.fa # flow — routes to the right handler
      router/
        Route.fa       # func — classifies requests
      routes/
        Home.fa        # func — builds home page
        About.fa       # func — builds about page
        db/
          Migrate.fa   # func — database setup
          CreateJob.fa # func — inserts a job
        pages/
          Layout.fa    # func — HTML layout wrapper
```

## CLI

```bash
forai compile main.fa              # Compile to IR JSON
forai run main.fa                  # Execute the program
forai test src/                    # Run test blocks
forai doc main.fa                  # Generate documentation
forai dev main.fa                  # Interactive debugger
```
