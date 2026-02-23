# 10.8 — http.server

The `http.server.*` namespace runs an HTTP server inside your forai program. The server is event-driven: `http.server.listen` creates a handle, and `http.server.accept` blocks until the next request arrives. This pattern is used inside `source` blocks — the source loops accepting connections and emits them as events into the flow.

## Handle Types

| Handle | Created by | Used by |
|--------|-----------|---------|
| `http_server` | `http.server.listen` | `http.server.accept`, `http.server.close` |
| `http_conn` | field inside `http.server.accept` result | `http.server.respond`, `http.respond.html/json/text` |

Handle values are opaque — they cannot be serialized or inspected.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `http.server.listen` | port | http_server | Bind TCP port, return server handle |
| `http.server.accept` | http_server | HttpRequest | Block until next request; returns HttpRequest |
| `http.server.respond` | http_conn, status, headers, body | bool | Write HTTP response on the connection |
| `http.server.close` | http_server | bool | Close the server and stop accepting |

### Convenience Response Ops (`http.respond.*`)

These ops combine header construction and response writing into a single call:

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `http.respond.html` | http_conn, status, body | bool | Respond with `content-type: text/html; charset=utf-8` |
| `http.respond.json` | http_conn, status, body | bool | Respond with `content-type: application/json` |
| `http.respond.text` | http_conn, status, body | bool | Respond with `content-type: text/plain; charset=utf-8` |

### HttpRequest (built-in type)

`http.server.accept` returns an `HttpRequest` with:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `method` | text | yes | `"GET"`, `"POST"`, etc. |
| `path` | text | yes | URL path (e.g. `"/users/42"`) |
| `query` | text | | Raw query string |
| `headers` | dict | | Request headers (lowercased keys) |
| `body` | text | | Request body |
| `conn_id` | text | yes | Connection identifier for `http.server.respond` |

This is a built-in type — use `HttpRequest` directly in `take`/`emit` declarations without defining it.

## Examples

### Minimal HTTP server (source + flow pattern)

```fa
# In a source file: app/HttpSource.fa
source HttpSource
  emit request dict
body
  server = http.server.listen(8080)
  loop list.range(1, 999999) as _
    request = http.server.accept(server)
    emit request
  done
done
```

```fa
# In a func: app/HandleRequest.fa
func HandleRequest
  take request dict
  emit response dict
body
  path = http.extract_path(request)
  method = obj.get(request, "method")

  case path
    when "/health"
      response = http.response(200, json.encode(obj.set(obj.new(), "ok", true)))
    when "/ping"
      response = http.response(200, "pong")
    else
      response = http.error_response(404, "NOT_FOUND", "Path #{path} not found")
  done

  emit response
done
```

### Responding to a request

`http.server.respond` takes the original request dict (which carries the connection handle internally):

```fa
func ServeAndRespond
  take request dict
  emit done bool
body
  path = obj.get(request, "path")
  hdrs = headers.new()
  hdrs = headers.set(hdrs, "content-type", "application/json")

  if path == "/hello"
    body = json.encode(obj.set(obj.new(), "message", "hello world"))
    http.server.respond(request, 200, hdrs, body)
  else
    body = json.encode(obj.set(obj.new(), "error", "not found"))
    http.server.respond(request, 404, hdrs, body)
  done

  emit true
done
```

### Routing by method and path

```fa
func Router
  take request dict
  emit response dict
  fail err dict
body
  method = obj.get(request, "method")
  path = obj.get(request, "path")

  response = obj.new()

  if method == "GET" && path == "/users"
    users = list.new()
    # ... fetch from db ...
    response = http.response(200, json.encode(users))
  done

  if method == "POST" && path == "/users"
    body_text = obj.get(request, "body")
    user_data = json.decode(body_text)
    # ... create user ...
    response = http.response(201, json.encode(user_data))
  done

  if method == "GET" && str.starts_with(path, "/users/")
    user_id = str.slice(path, 7, str.len(path))
    # ... fetch user by id ...
    response = http.response(200, json.encode(obj.set(obj.new(), "id", user_id)))
  done

  emit response
done
```

### Reading query parameters

```fa
func SearchHandler
  take request dict
  emit response dict
body
  query = obj.get(request, "query")
  q = ""
  if obj.has(query, "q")
    q = obj.get(query, "q")
  done
  limit = 10
  if obj.has(query, "limit")
    limit = to.long(obj.get(query, "limit"))
  done
  # ... perform search ...
  response = http.response(200, json.encode(obj.set(obj.set(obj.new(), "query", q), "limit", limit)))
  emit response
done
```

### Reading request body (POST/PUT)

```fa
func CreateUser
  take request dict
  emit response dict
  fail err dict
body
  raw_body = obj.get(request, "body")
  if str.len(raw_body) == 0
    fail error.new("EMPTY_BODY", "Request body is required")
  done
  data = json.decode(raw_body)
  name = obj.get(data, "name")
  email = obj.get(data, "email")
  # ... insert into db ...
  response = http.response(201, json.encode(data))
  emit response
done
```

### Reading request headers

```fa
func AuthMiddleware
  take request dict
  emit user_id text
  fail err dict
body
  hdrs = obj.get(request, "headers")
  auth = headers.get(hdrs, "authorization")
  if str.len(auth) == 0
    fail error.new("UNAUTHORIZED", "Missing Authorization header")
  done
  # strip "Bearer " prefix
  token = str.slice(auth, 7, str.len(auth))
  result = crypto.verify_token(token, env.get("JWT_SECRET"))
  if obj.get(result, "valid") == false
    fail error.new("UNAUTHORIZED", "Invalid token")
  done
  payload = obj.get(result, "payload")
  user_id = obj.get(payload, "sub")
  emit user_id
done
```

### Closing the server

```fa
server = http.server.listen(8080)
# ... handle requests ...
http.server.close(server)
```

## Common Patterns

### JSON API response helper

Use the convenience ops to skip manual header construction:

```fa
http.respond.json(conn_id, 200, json.encode(data))
```

Or with the full form when you need custom headers:

```fa
hdrs = headers.set(headers.new(), "content-type", "application/json")
http.server.respond(conn_id, 200, hdrs, json.encode(data))
```

### CORS headers

```fa
cors_hdrs = headers.new()
cors_hdrs = headers.set(cors_hdrs, "access-control-allow-origin", "*")
cors_hdrs = headers.set(cors_hdrs, "access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS")
cors_hdrs = headers.set(cors_hdrs, "access-control-allow-headers", "Content-Type, Authorization")
```

### Path-based routing with route.*

For complex routing with parameters, combine `route.match` (returns bool) and `route.params` (returns dict):

```fa
if route.match("/users/:id", path)
  params = route.params("/users/:id", path)
  user_id = obj.get(params, "id")
done
```

## Gotchas

- `http.server.accept` **blocks** until the next request arrives. This is intentional — it is meant to be called inside a `source` body's `loop` or `on` block, not directly inside a `func`.
- The request dict returned by `http.server.accept` contains an opaque connection handle. Pass the entire request dict to `http.server.respond` — do not reconstruct it.
- `http.server.respond` must be called exactly once per accepted request. Failing to respond leaves the client connection hanging. Calling it twice raises a runtime error.
- `http.server.respond` does not automatically add headers like `Content-Type`. Use `http.respond.html`, `http.respond.json`, or `http.respond.text` to skip manual header construction.
- `http.server.close` stops the listener but does not terminate in-flight request handlers. Pending `http.server.respond` calls on already-accepted connections will still succeed.
- Ports below 1024 require elevated privileges on most operating systems.
