# 10.24 — error and log

This chapter covers two namespaces for observability and error handling:

- `error.*` — constructing and inspecting structured error dicts
- `log.*` — level-based logging to stderr

---

## error.*

The `error.*` namespace creates and inspects structured error values. In forai, errors are `ErrorObject` values with a `code` and a `message` field. The `error.*` ops provide a consistent shape and convenient construction helpers.

### ErrorObject (built-in type)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `code` | text | yes | Machine-readable error code (e.g., `"NOT_FOUND"`, `"INVALID_INPUT"`) |
| `message` | text | yes | Human-readable description |
| `details` | dict | | Additional context (optional, set via third arg to `error.new`) |

This is a built-in type — use `ErrorObject` directly in `take`/`emit`/`fail` declarations without defining it.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `error.new` | code, message [, details] | ErrorObject | Create an error object |
| `error.wrap` | error, context | ErrorObject | Prepend `context` to the error message |
| `error.code` | error | text | Extract the `code` field |
| `error.message` | error | text | Extract the `message` field |

### Examples

#### Creating a basic error

```fa
func ValidateAge
  take age long
  emit ok bool
  fail err dict
body
  if age < 0
    fail error.new("INVALID_AGE", "Age cannot be negative")
  done
  if age > 150
    fail error.new("INVALID_AGE", "Age cannot exceed 150")
  done
  emit true
done
```

#### Creating an error with details

```fa
func ValidateUser
  take user dict
  emit ok bool
  fail err dict
body
  if obj.has(user, "email") == false
    details = obj.set(obj.new(), "field", "email")
    fail error.new("MISSING_FIELD", "email is required", details)
  done
  emit true
done
```

Note: the `details` argument is stored alongside `code` and `message`. Access it with `obj.get(err, "details")`.

#### Wrapping errors with context

```fa
func LoadUserConfig
  take user_id text
  emit config dict
  fail err dict
body
  path = "configs/#{user_id}.json"
  if file.exists(path) == false
    base_err = error.new("NOT_FOUND", "Config file missing: #{path}")
    fail error.wrap(base_err, "LoadUserConfig")
  done
  raw = file.read(path)
  config = json.decode(raw)
  emit config
done
```

`error.wrap` prepends the context string to the message: `"LoadUserConfig: Config file missing: configs/u42.json"`.

#### Extracting error fields

```fa
func HandleError
  take err dict
  emit formatted text
body
  code = error.code(err)
  message = error.message(err)
  formatted = "[#{code}] #{message}"
  emit formatted
done
```

#### Error inspection in a flow

```fa
func TryFetch
  take url text
  emit data dict
  fail err dict
body
  resp = http.get(url)
  if resp.status == 404
    fail error.new("NOT_FOUND", "Resource not found at: #{url}")
  done
  if resp.status == 401
    fail error.new("UNAUTHORIZED", "Access denied to: #{url}")
  done
  if resp.status != 200
    fail error.new("HTTP_ERROR", "Unexpected status #{to.text(resp.status)} from #{url}")
  done
  data = json.decode(resp.body)
  emit data
done
```

#### Converting errors to HTTP responses

```fa
func ErrorToResponse
  take err dict
  emit response dict
body
  code = error.code(err)
  message = error.message(err)
  status = 500
  if code == "NOT_FOUND"
    status = 404
  done
  if code == "UNAUTHORIZED"
    status = 401
  done
  if code == "INVALID_INPUT" || code == "MISSING_FIELD"
    status = 400
  done
  response = http.error_response(status, code, message)
  emit response
done
```

#### Chaining error context across layers

```fa
func DatabaseLayer
  take id text
  emit row dict
  fail err dict
body
  rows = db.query(conn, "SELECT * FROM items WHERE id = ?", list.append(list.new(), id))
  if list.len(rows) == 0
    fail error.new("NOT_FOUND", "Item #{id} not found in database")
  done
  emit rows[0]
done
```

```fa
func ServiceLayer
  take id text
  emit item dict
  fail err dict
body
  # In a real flow, errors from DatabaseLayer propagate automatically.
  # Use error.wrap in the fail branch if you need to add context.
  item = DatabaseLayer(id)
  emit item
done
```

### Common Patterns

#### Error code constants (by convention)

Use uppercase with underscores for error codes. Common conventions:

| Code | Meaning |
|------|---------|
| `"NOT_FOUND"` | Resource does not exist |
| `"UNAUTHORIZED"` | Not authenticated |
| `"FORBIDDEN"` | Authenticated but not allowed |
| `"INVALID_INPUT"` | Bad request data |
| `"MISSING_FIELD"` | Required field absent |
| `"CONFLICT"` | Resource already exists |
| `"INTERNAL"` | Unexpected internal error |
| `"TIMEOUT"` | Operation timed out |
| `"RATE_LIMITED"` | Too many requests |

#### Structured error logging

```fa
log.error("Request failed", obj.set(obj.set(obj.new(), "code", error.code(err)), "msg", error.message(err)))
```

### Gotchas

- `error.code` and `error.message` are convenience accessors equivalent to `obj.get(err, "code")` and `obj.get(err, "message")`. They fail if the dict does not have those keys.
- `error.new` always creates a dict — it does not throw or raise. You must use `fail` to propagate it as an error to the caller.
- `error.wrap` only modifies the `message` field. The `code` is preserved from the original error. If you need to change the code, use `obj.set(err, "code", new_code)`.
- There is no stack trace in forai errors — errors are plain data. Use `log.*` at the point of failure to record context.
- Comparing error codes: `error.code(err) == "NOT_FOUND"` — use `==` on text values.

---

## log.*

The `log.*` namespace writes structured log messages to stderr. All ops accept an optional `context` dict for structured metadata. Logs include a timestamp and level prefix.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `log.debug` | msg [, ctx] | bool | Debug-level log to stderr |
| `log.info` | msg [, ctx] | bool | Info-level log to stderr |
| `log.warn` | msg [, ctx] | bool | Warning-level log to stderr |
| `log.error` | msg [, ctx] | bool | Error-level log to stderr |
| `log.trace` | msg [, ctx] | bool | Trace-level log (most verbose) to stderr |

### Log Level Guide

| Level | When to use |
|-------|------------|
| `trace` | Step-by-step execution details; extremely verbose |
| `debug` | Diagnostic information useful during development |
| `info` | Normal significant events (startup, shutdown, request handled) |
| `warn` | Unexpected but recoverable situations |
| `error` | Failures that need attention |

### Examples

#### Basic logging

```fa
func ProcessOrder
  take order dict
  emit ok bool
body
  order_id = obj.get(order, "id")
  log.info("Processing order", obj.set(obj.new(), "order_id", order_id))
  # ... process ...
  log.info("Order complete", obj.set(obj.new(), "order_id", order_id))
  emit true
done
```

#### Logging with rich context

```fa
func HandleRequest
  take request dict
  emit response dict
body
  method = obj.get(request, "method")
  path = obj.get(request, "path")
  ctx = obj.new()
  ctx = obj.set(ctx, "method", method)
  ctx = obj.set(ctx, "path", path)
  log.debug("Incoming request", ctx)
  # ... handle ...
  response = http.response(200, "ok")
  log.info("Request handled", obj.set(ctx, "status", 200))
  emit response
done
```

#### Error logging

```fa
func SafeFetch
  take url text
  emit data dict
  fail err dict
body
  resp = http.get(url)
  if resp.status != 200
    err_val = error.new("FETCH_FAILED", "HTTP #{to.text(resp.status)}")
    log.error("Fetch failed", obj.set(obj.set(obj.new(), "url", url), "status", resp.status))
    fail err_val
  done
  data = json.decode(resp.body)
  emit data
done
```

#### Conditional debug logging

```fa
func ProcessItems
  take items list
  take debug bool
  emit count long
body
  count = 0
  loop items as item
    count = count + 1
    if debug
      log.debug("Processing item", obj.set(obj.new(), "index", count))
    done
  done
  emit count
done
```

#### Warn on slow operations

```fa
func TimedOp
  take data list
  emit result list
body
  start = stamp.now()
  result = list.new()
  loop data as item
    result = list.append(result, item)
  done
  end = stamp.now()
  elapsed_ms = stamp.diff(end, start) / 1000000
  if elapsed_ms > 100
    log.warn("Slow operation", obj.set(obj.new(), "elapsed_ms", elapsed_ms))
  done
  emit result
done
```

#### Startup logging

```fa
func StartServer
  take port long
  emit ok bool
body
  log.info("Starting server", obj.set(obj.new(), "port", port))
  server = http.server.listen(port)
  log.info("Server listening", obj.set(obj.new(), "port", port))
  emit true
done
```

#### Trace logging for deep debugging

```fa
func ParseToken
  take token text
  emit claims dict
  fail err dict
body
  log.trace("Verifying token", obj.set(obj.new(), "token_len", str.len(token)))
  secret = env.get("JWT_SECRET")
  result = crypto.verify_token(token, secret)
  log.trace("Token verified", obj.set(obj.new(), "valid", obj.get(result, "valid")))
  if obj.get(result, "valid") == false
    fail error.new("INVALID_TOKEN", obj.get(result, "error"))
  done
  claims = obj.get(result, "payload")
  emit claims
done
```

### Common Patterns

#### Log and fail in one step

```fa
func FailWithLog
  take code text
  take message text
  take ctx dict
  fail err dict
body
  log.error(message, ctx)
  fail error.new(code, message)
done
```

#### Access log pattern

```fa
method = obj.get(request, "method")
path = obj.get(request, "path")
status = obj.get(response, "status")
log.info("#{method} #{path} #{to.text(status)}")
```

#### Structured context builder

```fa
ctx = obj.new()
ctx = obj.set(ctx, "user_id", user_id)
ctx = obj.set(ctx, "request_id", random.uuid())
ctx = obj.set(ctx, "action", "create_order")
log.info("Starting action", ctx)
```

### Gotchas

- All `log.*` ops write to **stderr**, not stdout. If you are capturing program output, logs will not appear in stdout captures.
- Log output format (timestamp, level prefix, context serialization) is implementation-defined. Do not parse log output programmatically — use structured data returned from funcs instead.
- `log.*` ops always return `true`. The return value can be ignored.
- The `context` argument is optional. `log.info("message")` is valid.
- There is no log-level filtering in the runtime — all levels are always emitted. Filter at the infrastructure level (e.g., pipe through `grep` or a log aggregator).
- Do not log sensitive data (passwords, tokens, PII) even at debug level. Use `obj.delete` or masking before passing context to log ops.
