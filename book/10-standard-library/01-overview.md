# Chapter 10: Standard Library

The forai standard library is a set of built-in namespaces available in every `.fa` file without any `uses` declaration. All ops are called using the `namespace.op(args)` syntax inside `func` bodies. No imports are needed — these ops are resolved by the runtime directly.

## Calling Convention

Standard library ops are ordinary function calls in a `func` body:

```fa
func FormatUser
  take name text
  take age long
  emit result text
body
  upper = str.upper(name)
  trimmed = str.trim(upper)
  emit "#{trimmed} (age: #{to.text(age)})"
done
```

Ops return values that can be assigned to locals, passed as arguments to other ops, or used in expressions. There is no special import syntax — the namespace prefix is enough.

## All Namespaces

| Namespace | Description |
|-----------|-------------|
| `str.*` | String manipulation: case, trimming, splitting, searching, slicing |
| `list.*` | Immutable list construction and querying |
| `obj.*` | Immutable dict (object) construction and querying |
| `math.*` | Arithmetic: add, subtract, multiply, divide, mod, power, round, floor |
| `type.*` | Runtime type introspection — returns the type name of any value |
| `to.*` | Type conversion between text, long, real, and bool |
| `json.*` | JSON encode and decode |
| `codec.*` | Generic codec dispatch by format name |
| `http.*` | HTTP client (GET/POST/PUT/PATCH/DELETE) and response construction |
| `http.server.*` | HTTP server: listen, accept connections, send responses |
| `http.respond.*` | HTTP response shortcuts: `html`, `json`, `text` (auto content-type) |
| `ws.*` | WebSocket client: connect, send, receive, close |
| `headers.*` | HTTP header dict construction and access |
| `cookie.*` | HTTP cookie parsing, construction, and deletion |
| `db.*` | SQLite database: open, exec, query, close |
| `file.*` | File I/O: read, write, append, copy, move, list, stat |
| `term.*` | Terminal I/O: print, prompt, color, cursor control |
| `time.*` | Sleep and time splitting utilities |
| `fmt.*` | Formatting helpers: pad HMS, wrap fields |
| `env.*` | Environment variable access and mutation |
| `exec.*` | Run external processes, capture output |
| `regex.*` | Regular expression matching, search, replace, split |
| `random.*` | Random integers, floats, UUIDs, list shuffling |
| `hash.*` | Cryptographic hash digests (SHA-256, SHA-512, HMAC) |
| `base64.*` | Base64 standard and URL-safe encode/decode |
| `crypto.*` | Bcrypt password hashing, JWT sign/verify, secure random bytes |
| `log.*` | Structured leveled logging to stderr |
| `error.*` | Structured error dict construction and inspection |
| `date.*` | Calendar date arithmetic, parsing, and formatting |
| `stamp.*` | Monotonic nanosecond timestamps |
| `trange.*` | Time range construction and querying |
| `url.*` | URL parsing, query string parsing, percent-encoding |
| `route.*` | URL path pattern matching with `:param` and `*wildcard` |
| `html.*` | HTML entity escaping and unescaping |
| `tmpl.*` | Mustache-style template rendering |

## Handle Types

Some ops return opaque *handle* values that must be threaded through subsequent calls:

| Handle type | Created by | Consumed by |
|-------------|-----------|-------------|
| `db_conn` | `db.open` | `db.exec`, `db.query`, `db.close` |
| `http_server` | `http.server.listen` | `http.server.accept`, `http.server.close` |
| `http_conn` | returned inside `http.server.accept` result | `http.server.respond` |
| `ws_conn` | `ws.connect` | `ws.send`, `ws.recv`, `ws.close` |

Handle values cannot be inspected or serialized — they are only valid as arguments to the matching ops.

## Built-in Struct Types

The standard library also provides built-in struct types that are returned by ops and available without declaration. Use them directly in `take`/`emit`/`fail` declarations:

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

User-defined types with the same name as a built-in type produce a "duplicate type name" compile error. The type checker tracks struct return types from stdlib ops — passing a `Date` where a `TimeRange` is expected is a compile error, just like passing `text` where `db_conn` is expected.

## Error Behavior

Most ops fail loudly: if they receive invalid input (e.g. division by zero with `/`, `obj.get` with a missing key), the runtime raises an error that propagates up to the nearest `fail` handler. Use `error.*` to construct structured error values to pass to `fail`.

## Chapters in This Section

| Chapter | Namespace(s) |
|---------|-------------|
| [02 — str](02-str.md) | `str.*` |
| [03 — list](03-list.md) | `list.*` |
| [04 — obj](04-obj.md) | `obj.*` |
| [05 — math](05-math.md) | `math.*`, `type.*`, `to.*` |
| [06 — json](06-json.md) | `json.*`, `codec.*` |
| [07 — http-client](07-http-client.md) | `http.*`, `headers.*`, `cookie.*` |
| [08 — http-server](08-http-server.md) | `http.server.*` |
| [09 — websocket](09-websocket.md) | `ws.*` |
| [10 — db](10-db.md) | `db.*` |
| [11 — file](11-file.md) | `file.*` |
| [12 — term](12-term.md) | `term.*` |
| [13 — exec](13-exec.md) | `exec.*` |
| [14 — regex](14-regex.md) | `regex.*` |
| [15 — random](15-random.md) | `random.*` |
| [16 — crypto](16-crypto.md) | `crypto.*`, `hash.*`, `base64.*` |
| [17 — hash](17-hash.md) | `hash.*` (reference — see chapter 16) |
| [18 — date](18-date.md) | `date.*`, `stamp.*`, `trange.*` |
| [19 — time](19-time.md) | `time.*`, `fmt.*` |
| [20 — env](20-env.md) | `env.*` |
| [21 — url](21-url.md) | `url.*` |
| [22 — route](22-route.md) | `route.*`, `url.*`, `html.*` |
| [23 — tmpl](23-tmpl.md) | `tmpl.*` |
| [24 — error](24-error.md) | `error.*`, `log.*` |
