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

Sources always use a `body` block. The idiomatic way to handle events is the `on` block:

```
source Commands
  emit cmd as text
  fail error as text
body
  on :input from term.prompt("docs> ") to raw
    trimmed = str.trim(raw)
    emit trimmed
    case trimmed
      when "quit"
        break
      when "exit"
        break
    done
  done
done
```

**`on :tag from op(args) to var`** — event handler. The `:tag` names the event type (cosmetic in v1). The `from` expression is a blocking async call that returns one event per invocation. The `to` variable binds the result. The body runs per event. `emit` sends it downstream, `break` stops the source.

**Init + on pattern** — set up a resource once, then handle events from it:

```
source HTTPRequests
  take port as long
  emit req as dict
  fail error as text
body
  srv = http.server.listen(port)
  on :request from http.server.accept(srv) to req
    emit req
  done
done
```

**Loop form** — for complex polling patterns, `loop` is available as an escape hatch:

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
        time.sleep(3)
      else
        job = rows[0]
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
  term.print("")
  term.print(text)
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
- `state x = op(...)` or `state x = <literal>` — initialize a shared variable (runs once)
- `send nowait Module.Func()` — fire-and-forget a background task
- `branch when <expr> ... done` — conditional sub-pipeline (runs body only if true)
- `branch ... done` — unguarded sub-pipeline (always runs)

### `state` vs `step` in flows

`state` and `step` are both flow-level statements but serve different purposes:

- **`state`** — runs once at startup. Use it for values that depend only on literals or other `state` values. Think: resources, constants, derived-but-fixed paths.
- **`step`** — runs per event. Use it when the value depends on a runtime wire (something that arrived from a source or a previous step's output).

**Rule of thumb**: if all inputs to the call are literals or other `state` vars, use `state`. If any input is a runtime wire, use `step`.

```
# Literals are valid as state RHS:
state label = "jobs"
state count = 0
state users = ["alice", "bob"]
state cfg = {host: "localhost", port: 8080}

# Op calls work too:
state conn = db.open(":memory:")
state parts = list.new()
```

**Building a path from a runtime value** (e.g., a redirect URL including a job ID):

```
# The static prefix is state; the part that uses a runtime wire is step
state prefix = "/jobs/"
state parts = [prefix]
step list.append(parts to :l, job_id to :item) then   # job_id is a runtime wire
  next :result to parts2
done
step str.join(parts2 to :l, "" to :sep) then
  next :result to redir_path
done
```

This pattern appears in every HTTP route that needs a redirect or a constructed URL.

## Language Syntax

### Every Statement is an Assignment or Action

There are no bare expressions. Every line is one of:
- `var = expression` — assignment
- `var: TypeName = expression` — assignment with type annotation
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

### Compound Assignment

Compound assignment operators update a variable in-place:

```
count = 0
count += 1       # count = count + 1
count -= 1       # count = count - 1
count *= 2       # count = count * 2
count /= 3       # count = count / 3
count %= 4       # count = count % 4
```

`+=` works for string concatenation too:

```
msg = "hello"
msg += " world"  # "hello world"
```

Only simple variable names are supported — dotted paths (`obj.field += 1`) are not.

### Type Annotations on Assignments

You can optionally annotate assignments with an explicit type. The compiler checks the annotation against the inferred type and reports mismatches at compile time:

```
count: long = str.len(name)
label: text = "hello"
```

Type annotations are purely for documentation and compile-time validation — they don't change runtime behavior. If the inferred type doesn't match the annotation, the compiler reports an error.

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

All arithmetic uses infix operators — there are no `math.add`, `math.divide`, etc. ops. Use `math.floor` and `math.round` for rounding. Integer preservation: when both operands are integers and the result is exact, the result stays integer (`10 / 2` → `5`, `2 ** 3` → `8`). Inexact results become float (`7 / 2` → `3.5`).

**Ternary**: `result = condition ? "yes" : "no"`

**Null-coalescing** (`??`): returns LHS unless it's null/missing, then evaluates RHS:
```
timeout = obj.get(config, "timeout") ?? 30
name = obj.get(row, "display_name") ?? obj.get(row, "username") ?? "anonymous"
```

Only `null`/void triggers the fallback — `false`, `0`, `""` are kept. RHS is lazy (not evaluated if LHS is non-null).

**String interpolation**: `msg = "Hello #{name}, you have #{count} items"`

Interpolation works in both single-line and triple-quoted strings. Use `\#` to emit a literal `#` and prevent interpolation:

```
# Interpolates: evaluates `name` at runtime
msg = "Hello, #{name}!"

# Literal: outputs the text #{name} as-is
msg = "Hello, \#{name}!"
```

**Embedding code examples in template strings** — if you put forai source code inside a triple-quoted template string (e.g., for a docs page or tutorial), escape any `#{}` to prevent the runtime from trying to evaluate them:

```
template = """
func Greet
  take name as text
body
  msg = "Hello, \#{name}!"
  emit msg
done
"""
html = tmpl.render(template, data)
```

Without the `\#`, the runtime would try to evaluate `name` as a variable inside `HomePage`, not render it as literal text.

**Mustache templates and `#{` collision** — `tmpl.render` uses Mustache `{{var}}` syntax. If your template text contains a `#` immediately before `{{`, the forai lexer sees `#{` as the start of a string interpolation expression and fails to parse:

```
# WRONG — forai lexer reads `#{` as interpolation start, then fails on `{pr_number}`:
msg = "PR #{{pr_number}} merged"

# RIGHT — escape the hash to emit it literally:
msg = "PR \#{{pr_number}} merged"
```

The rule: any `#{` inside a `tmpl.render("""...""", data)` string must be written as `\#{`.

**List literals**: `items = [1, 2, 3]` or `empty = []`

**Dict literals**: `config = {host: "localhost", port: 8080}` or `empty = {}`

**Bracket indexing**: `first = items[0]`, `last = items[-1]`, `name = row["name"]`. Supports negative indices, chained indexing (`matrix[0][1]`), and works on both lists and dicts.

**Function calls**: `result = str.upper(name)` or `result = MyFunc(arg1, arg2)`

String `+` does concatenation. Escapes: `\n` `\t` `\\` `\"` `\#`.

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

**OR patterns** — match multiple values:
```
case answer
  when "yes" | "y" | "true"
    confirmed = true
  when "no" | "n" | "false"
    confirmed = false
done
```

**Range patterns** — `lo..hi` (inclusive lo, exclusive hi):
```
case score
  when 90..101
    grade = "A"
  when 80..90
    grade = "B"
  when 70..80
    grade = "C"
  else
    grade = "F"
done
```

**Type patterns** — match by runtime type:
```
case value
  when :text
    kind = "string"
  when :long | :real
    kind = "number"
  when :list
    kind = "collection"
done
```

**Guard clauses** — `if` after pattern adds a condition:
```
case score
  when _ if score >= 90
    grade = "A"
  when _ if score >= 70
    grade = "C"
  else
    grade = "F"
done
```

`break` is valid inside a `when` arm and exits the enclosing loop:
```
loop
  result = next_state()
  case result
    when "done"
      break
    when "continue"
      term.print("keep going")
  done
done
```

**loop** (collection):
```
items = list.range(0, 10)
loop items as i
  term.print("Item #{i}")
done
```

The collection must be a variable — `loop list.range(0,10) as i` is invalid. Assign it first.

**loop with index** — access the 0-based iteration index:
```
items = list.range(10, 13)
loop items as val with index i
  term.print("#{i}: #{val}")
done
# prints: 0: 10, 1: 11, 2: 12
```

The index variable is scoped to the loop body and does not leak outside.

**continue** — skip to next iteration:
```
loop items as item
  if item == "skip"
    continue
  done
  term.print("Processing #{item}")
done
```

Works in both collection and bare loops. Applies to the innermost loop.

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

Import files or directories with `use Name from "path"`:

```
use auth from "./auth"          # directory import → call as auth.Login(...)
use db   from "./db"            # directory import → call as db.GetUser(...)
use Round from "./round.fa"     # file import → call as Round(...)

# Call directory imports as namespace.Function:
token = auth.Login(email, password)
user = db.GetUser(conn, user_id)

# Call file imports directly:
result = Round(value)
```

Rules:
- One func/flow/sink per file. Name must match filename (`func Foo` in `Foo.fa`)
- `use ... from "..."` paths are relative to the importing file's directory
- `server/Start.fa` with `use db from "./db"` looks for `server/db/`, not top-level `db/`
- Directory imports register as `name.FuncName`; file imports register as `name` directly

### Packages and Dependencies

forai projects can depend on external libraries. Dependencies are declared in `forai.json` and imported using `use` with the dependency key as the path.

**forai.json:**
```json
{
  "name": "my-app",
  "version": "0.1.0",
  "main": "src/main.fa",
  "dependencies": {
    "@user/tools": "^1.0.0",
    "mylib": "file:../mylib/",
    "requests": "git+https://somesite.com/repo.git#^2.0.0"
  }
}
```

**Three dependency sources:**

| Format | Example | Resolves to |
|--------|---------|-------------|
| GitHub shorthand | `"@user/repo": "^1.0.0"` | `https://github.com/user/repo.git` |
| Local file path | `"mylib": "file:../mylib/"` | Filesystem path relative to project root |
| Arbitrary git URL | `"requests": "git+https://host/repo.git#^1.0.0"` | Any git remote; version after `#` |

**Version ranges** (npm-style semver):
- `^1.2.3` — compatible: `>=1.2.3, <2.0.0` (caret)
- `~1.2.3` — patch-level: `>=1.2.3, <1.3.0` (tilde)
- `>=1.0.0`, `>1.0.0`, `<=2.0.0`, `<2.0.0` — comparison
- `1.2.3` — exact version

**Importing a dependency:**
```
use tools from "@user/tools"

func Process
  take input as text
  return text
  fail text
body
  result = tools.Transform(input)
  return result
done
```

The `from` path must match a key in the `dependencies` map. The compiler resolves the dependency to its cached location and loads the library's modules.

**Library projects** declare `"type": "lib"` in their `forai.json`:
```json
{
  "name": "@user/tools",
  "version": "1.0.0",
  "type": "lib",
  "main": "src/"
}
```

Only `lib` packages can be imported as dependencies. The `main` field points to the library's entry point (a directory for module imports or a `.fa` file for single-callable libraries).

**Caching:** Dependencies are fetched to `~/.config/forai/cache/<name>/v<version>/` as shallow git clones with `.git/` removed. Subsequent builds use the cache.

**Lockfile:** After resolving dependencies, the compiler writes `forai.lock` (JSON) to the project root. This pins exact versions and git SHAs for reproducible builds. Commit `forai.lock` to version control.

**Transitive dependencies:** Libraries can declare their own dependencies. The compiler resolves the full dependency tree, detecting version conflicts and circular dependencies.

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

**Open struct** (allows extra fields beyond the declared schema):
```
open type Config
  host text
  port long
done
```

Closed structs (the default) reject any field not in the schema during validation. Open structs accept extra fields — useful for extensible data like HTTP requests or configuration objects. All built-in struct types (`HttpRequest`, `Date`, `ProcessOutput`, etc.) are open.

**Enum:**
```
enum Role
  Admin
  User
  Guest
done
```

**Open enum** (accepts any string, not just declared variants):
```
open enum LogLevel
  Debug
  Info
  Warn
  Error
done
```

Closed enums (the default) only accept declared variant strings. Open enums accept any string value — the declared variants serve as documentation of known values.

Primitive types: `text`, `bool`, `long` (i64), `real` (f64), `uuid`, `time`, `list`, `dict`, `void`, `db_conn`, `http_server`, `http_conn`, `ws_conn`.

Handle types (`db_conn`, `http_server`, `http_conn`, `ws_conn`) are opaque — they cannot be constructed from string literals. They are produced by specific ops (`db.open`, `http.server.listen`, etc.) and the compiler validates correct usage at compile time.

**Built-in struct types** — these are returned by stdlib ops and available without declaration:

| Type | Returned by | Fields |
|------|-------------|--------|
| `HttpRequest` | `http.server.accept` | `method`, `path`, `query`, `headers`, `body`, `conn_id` |
| `HttpResponse` | `http.get/post/put/delete` | `status`, `headers`, `body` |
| `Date` | `date.now`, `date.from_iso`, etc. | `unix_ms`, `tz_offset_min` |
| `Stamp` | `stamp.now`, `stamp.from_ns` | `ns` |
| `TimeRange` | `trange.new` | `start` (Date), `end` (Date) |
| `ProcessOutput` | `exec.run` | `code`, `stdout`, `stderr`, `ok` |
| `WebSocketMessage` | `ws.recv` | `type`, `data` |
| `ErrorObject` | `error.new` | `code`, `message`, `details` |
| `URLParts` | `url.parse` | `path`, `query`, `fragment` |

The compiler tracks built-in struct return types for type checking — misusing a struct type (e.g., passing a `Date` to an op that expects `TimeRange`) is a compile error.

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

Test blocks use `it` sub-cases. Shared setup (mocks, assignments) before the first `it` is cloned into each case's scope. Each `it` runs independently.

```
docs Classify
  Verifies command classification.
done

test Classify
  it "recognizes help command"
    must Classify("help") == "help"
  done

  it "recognizes quit command"
    must Classify("quit") == "quit"
  done
done
```

**Failure testing** with `trap`:
```
test Validate
  it "rejects bad input"
    err = trap Validate(bad_data)
    must err == "invalid"
  done
done
```

**Mocking** sub-calls (shared mocks apply to all `it` blocks):
```
test Process
  mock api.Fetch => {status: 200}

  it "processes input"
    result = Process(input)
    must result.ok == true
  done
done
```

Per-`it` mocks override shared mocks for that case only:
```
test FetchAll
  it "fetches profiles"
    mock http.get => {status: 200, body: "..."}
    r = FetchAll(["alice"])
    must list.len(obj.get(r, "profiles")) == 1
  done

  it "records errors"
    mock http.get => {status: 404, body: ""}
    r = FetchAll(["missing"])
    must list.len(obj.get(r, "errors")) == 1
  done
done
```

**Name collision: `docs` required on test blocks** — if a flow and an imported func share the same name (e.g., `flow CreateJob` in `routes/CreateJob.fa` and `func CreateJob` in `routes/db/CreateJob.fa`), the test block inside the flow file **must** have its own `docs` block immediately before it. Without it, the checker cannot unambiguously associate the test with the flow, silently drops it, and then reports "flow has no test block":

```
# WRONG — checker silently rejects this test and reports the flow as untested:
test CreateJob
  it "works"
    ...
  done
done

# RIGHT — explicit docs block disambiguates:
docs CreateJob
  Verifies job creation route.
done

test CreateJob
  it "works"
    ...
  done
done
```

## Built-in Operations

The runtime provides 160+ ops across namespaces. All called as `namespace.op(args)`.

| Namespace | What | Key Ops |
|-----------|------|---------|
| `obj.*` | Dicts (immutable) | `new`, `set`, `get`, `has`, `delete`, `keys`, `merge` |
| `list.*` | Lists (immutable) | `new`, `range`, `append`, `len`, `contains`, `slice`; access via `items[0]` |
| `str.*` | Strings | `len`, `upper`, `lower`, `trim`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `slice`, `index_of` |
| `math.*` | Rounding | `floor`, `round` (arithmetic uses infix: `+` `-` `*` `/` `%` `**`) |
| `to.*` | Type conversion | `text`, `long`, `real`, `bool` |
| `type.*` | Introspection | `of` (returns `"text"`, `"long"`, etc.) |
| `json.*` | JSON | `decode`, `encode`, `encode_pretty` |
| `http.*` | HTTP client | `get`, `post`, `put`, `patch`, `delete`, `request`, `response`, `error_response`, `extract_path`, `extract_params` |
| `http.server.*` | HTTP server | `listen`, `accept`, `respond`, `close` |
| `http.respond.*` | HTTP response shortcuts | `html`, `json`, `text` (auto content-type, optional headers dict) |
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
body
  srv = http.server.listen(port)
  on :request from http.server.accept(srv) to req
    emit req
  done
done
```

The flow opens the database once (`state`), starts listening for HTTP requests from the source, and pipes each request to a handler. The source handles the accept loop. The flow is the flowchart — it never loops itself.

### Background Tasks

```
send nowait workflow.RunJobLoop()
```

Fire-and-forget. The background task runs independently with its own scope.

### Request Routing

Route by HTTP method and path using `obj.get` to extract fields from the request dict and `route.match` for URL pattern matching:

```
flow HandleRequest
  take conn as db_conn
  take req as dict
body
  state method = obj.get(req, "method")
  state path = obj.get(req, "path")
  state conn_id = obj.get(req, "conn_id")

  branch when method == "GET" && route.match("/", path)
    step pages.Home(conn_id to :conn_id) done
  done
  branch when method == "GET" && route.match("/users", path)
    step api.ListUsers(conn to :conn, conn_id to :conn_id) done
  done
  branch when method == "POST" && route.match("/users", path)
    step api.CreateUser(conn to :conn, req to :req) done
  done
  branch when method == "GET" && route.match("/users/:id", path)
    state params = route.params("/users/:id", path)
    step api.GetUser(conn to :conn, conn_id to :conn_id, params to :params) done
  done
  branch
    step pages.NotFound(conn_id to :conn_id) done
  done
done
```

Each `branch when` checks method + path — like a flowchart diamond. The final unguarded `branch` is the catch-all (404).

### POST Body Handling

Funcs that handle POST requests read the body from the request dict and parse it:

```
func CreateUser
  take conn as db_conn
  take req as dict
  emit result as bool
  fail error as text
body
  conn_id = obj.get(req, "conn_id")
  raw_body = obj.get(req, "body")
  data = json.decode(raw_body)
  name = obj.get(data, "name")
  email = obj.get(data, "email")

  id = random.uuid()
  params = [id, name, email]
  ok = db.exec(conn, "INSERT INTO users (id, name, email) VALUES (?1, ?2, ?3)", params)

  response = {id: id, name: name, email: email}
  body_text = json.encode(response)
  http.respond.json(conn_id, 200, body_text)
  ok2 = true
  emit ok2
done
```

The request dict from `http.server.accept` includes a `body` field containing the raw POST body as text. Use `json.decode` to parse JSON payloads.

The `http.respond.*` ops accept an optional 4th argument — a dict of extra response headers. The `content-type` is always set by the op and cannot be overridden:

```
hdrs = {"Set-Cookie": "session=abc123; HttpOnly", "Cache-Control": "no-store"}
http.respond.json(conn_id, 200, body_text, hdrs)
```

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
11. **`str.join` parameter is `:l`, not `:list`** — `str.join(items to :l, ", " to :sep)`. Using `:list` causes a parse error because `list` is a reserved type keyword. The same applies to any op parameter whose name coincides with a primitive type (`text`, `bool`, `long`, `real`, `list`, `dict`): use the actual parameter name from the stdlib reference, never the type name.
12. **Wire names, locals, and ports cannot be forai keywords** — `done`, `step`, `body`, `emit`, `fail`, `return`, `loop`, `case`, `when`, `else`, `sync`, `branch`, `take`, `from`, and other keywords cannot be used as variable or wire names. The parser reads them as statement boundaries and produces confusing errors. Use `ok`, `result`, `finished`, or `out` instead.

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
forai test                         # Run ALL test blocks (scans entire project recursively)
forai test lib/Foo.fa              # Run tests in a single file
forai build                        # Build and run all tests end-to-end
forai doc main.fa                  # Generate documentation
forai dev main.fa                  # Interactive debugger
```

`forai test` scans the project root recursively and runs every `test` block it finds, no matter how deeply nested. Passing a subdirectory (e.g. `forai test lib`) limits the scan to that subtree — avoid this unless you deliberately want partial coverage.
