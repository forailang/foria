# 10.7 — http client, headers, and cookie

This chapter covers the HTTP client-side namespaces:

- `http.*` — making outbound HTTP requests and constructing responses
- `headers.*` — building and reading HTTP header dicts
- `cookie.*` — parsing and constructing HTTP cookie strings

For the HTTP *server* (accepting inbound connections), see [Chapter 8](08-http-server.md).

---

## http.* (client)

The HTTP client ops send outbound requests and return an `HttpResponse`. All calls are async and non-blocking — the runtime awaits the response before continuing.

### HttpResponse (built-in type)

Every HTTP client op returns an `HttpResponse` with:

| Field | Type | Description |
|-------|------|-------------|
| `status` | long | HTTP status code |
| `headers` | dict | Response headers (lowercased keys) |
| `body` | text | Response body as text |

This is a built-in type — use `HttpResponse` directly in `take`/`emit` declarations without defining it.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `http.get` | url | HttpResponse | HTTP GET |
| `http.post` | url, body | HttpResponse | HTTP POST with body |
| `http.put` | url, body | HttpResponse | HTTP PUT with body |
| `http.patch` | url, body | HttpResponse | HTTP PATCH with body |
| `http.delete` | url | HttpResponse | HTTP DELETE |
| `http.request` | method, url, options | HttpResponse | Generic request with full options |
| `http.response` | status, body | dict | Construct a `{status, body}` response dict |
| `http.error_response` | status, code, msg | dict | Construct a JSON error response |
| `http.extract_path` | req | text | Extract `.path` from a request dict |
| `http.extract_params` | req | dict | Extract `.params` from a request dict |

### Examples

#### Simple GET

```fa
func FetchUser
  take user_id text
  emit user dict
  fail err dict
body
  url = "https://api.example.com/users/#{user_id}"
  resp = http.get(url)
  if resp.status != 200
    fail error.new("FETCH_FAILED", "Got status #{to.text(resp.status)}")
  done
  user = json.decode(resp.body)
  emit user
done
```

#### POST with JSON body

```fa
func CreateItem
  take name text
  take price real
  emit item dict
  fail err dict
body
  payload = obj.set(obj.set(obj.new(), "name", name), "price", price)
  body = json.encode(payload)
  resp = http.post("https://api.example.com/items", body)
  if resp.status != 201
    fail error.new("CREATE_FAILED", resp.body)
  done
  item = json.decode(resp.body)
  emit item
done
```

#### Generic request with custom headers

```fa
func AuthenticatedGet
  take url text
  take token text
  emit data dict
  fail err dict
body
  opts = obj.new()
  hdrs = headers.new()
  hdrs = headers.set(hdrs, "authorization", "Bearer #{token}")
  hdrs = headers.set(hdrs, "accept", "application/json")
  opts = obj.set(opts, "headers", hdrs)
  resp = http.request("GET", url, opts)
  if resp.status != 200
    fail error.new("HTTP_ERROR", "Status: #{to.text(resp.status)}")
  done
  data = json.decode(resp.body)
  emit data
done
```

#### Constructing response dicts (for server handlers)

```fa
func OkResponse
  take data dict
  emit resp dict
body
  body = json.encode(data)
  resp = http.response(200, body)
  emit resp
done
```

#### Constructing error responses

```fa
func NotFoundResponse
  take resource text
  emit resp dict
body
  resp = http.error_response(404, "NOT_FOUND", "#{resource} not found")
  emit resp
done
```

`http.error_response` encodes the body as: `{"code": "NOT_FOUND", "message": "... not found"}`.

#### Extracting path and params

These ops are useful inside flow bodies that dispatch on the incoming request:

```fa
path = http.extract_path(request)
params = http.extract_params(request)
```

### Common Patterns

#### Check status and decode

```fa
resp = http.get(url)
if resp.status == 200
  data = json.decode(resp.body)
else
  fail error.new("HTTP_#{to.text(resp.status)}", resp.body)
done
```

#### Retry with backoff (manual)

```fa
attempt = 0
success = false
result = obj.new()
loop list.range(1, 3) as _
  if success == false
    resp = http.get(url)
    if resp.status == 200
      result = json.decode(resp.body)
      success = true
    else
      time.sleep(1)
    done
  done
done
```

### Gotchas

- All `http.*` client ops are async — the func blocks until the response arrives. There is no timeout option in the basic ops; use `http.request` with an `options` dict for advanced control.
- `http.post`, `http.put`, and `http.patch` send the `body` argument as-is. Set `Content-Type` explicitly in headers if the server requires it.
- The response `body` field is always `text`. Parse with `json.decode` or `codec.decode` as needed.
- Network errors (DNS failure, connection refused) raise a runtime error. Wrap in a `trap` test or check status codes.

---

## headers.*

The `headers.*` namespace builds HTTP header dicts with lowercase-normalized keys. All keys are automatically lowercased on set and get.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `headers.new` | | `{}` | Empty headers dict |
| `headers.set` | headers, key, val | dict | Set header; key is lowercased |
| `headers.get` | headers, key | text | Get header value; key is lowercased |
| `headers.delete` | headers, key | dict | Remove header; key is lowercased |

### Examples

#### Building request headers

```fa
func BuildHeaders
  take token text
  take content_type text
  emit hdrs dict
body
  hdrs = headers.new()
  hdrs = headers.set(hdrs, "Authorization", token)   # stored as "authorization"
  hdrs = headers.set(hdrs, "Content-Type", content_type)
  hdrs = headers.set(hdrs, "X-Request-Id", random.uuid())
  emit hdrs
done
```

#### Reading a response header

```fa
content_type = headers.get(resp.headers, "content-type")
if str.starts_with(content_type, "application/json")
  data = json.decode(resp.body)
done
```

### Gotchas

- Keys are **always lowercased**. `headers.set(h, "Authorization", val)` stores the key as `"authorization"`. Use lowercase keys when reading with `headers.get` or `obj.get`.
- `headers.*` ops behave like `obj.*` with the added lowercasing. You can use `obj.get(headers, "content-type")` if you know the key is already lowercase.

---

## cookie.*

The `cookie.*` namespace handles HTTP cookie string parsing and `Set-Cookie` header construction.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `cookie.parse` | header | dict | Parse `"k=v; k2=v2"` cookie header into a dict |
| `cookie.get` | cookies, name | text | Get a cookie value by name |
| `cookie.set` | name, val, options | text | Build a `Set-Cookie` header string |
| `cookie.delete` | name, options | text | Build a delete cookie header (`Max-Age=0`) |

The `options` dict for `cookie.set` and `cookie.delete` supports keys: `"path"`, `"domain"`, `"max_age"`, `"http_only"`, `"secure"`, `"same_site"`.

### Examples

#### Parsing cookies from a request

```fa
func GetSessionId
  take request dict
  emit session_id text
  fail err dict
body
  hdrs = obj.get(request, "headers")
  cookie_header = headers.get(hdrs, "cookie")
  cookies = cookie.parse(cookie_header)
  if obj.has(cookies, "session_id") == false
    fail error.new("NO_SESSION", "No session cookie")
  done
  session_id = cookie.get(cookies, "session_id")
  emit session_id
done
```

#### Setting a session cookie

```fa
func SetSessionCookie
  take session_id text
  emit set_cookie_header text
body
  opts = obj.new()
  opts = obj.set(opts, "path", "/")
  opts = obj.set(opts, "http_only", true)
  opts = obj.set(opts, "secure", true)
  opts = obj.set(opts, "same_site", "Strict")
  opts = obj.set(opts, "max_age", 86400)
  set_cookie_header = cookie.set("session_id", session_id, opts)
  emit set_cookie_header
done
```

Result: `session_id=<value>; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=86400`

#### Deleting a cookie (logout)

```fa
func ClearSession
  emit set_cookie_header text
body
  opts = obj.set(obj.set(obj.new(), "path", "/"), "http_only", true)
  set_cookie_header = cookie.delete("session_id", opts)
  emit set_cookie_header
done
```

Result: `session_id=; Path=/; HttpOnly; Max-Age=0`

#### Full server-side session flow

```fa
func HandleLogin
  take request dict
  emit response dict
  fail err dict
body
  body = json.decode(obj.get(request, "body"))
  session_id = random.uuid()
  # ... store session in db ...
  set_cookie = cookie.set("session_id", session_id, obj.set(obj.set(obj.new(), "path", "/"), "http_only", true))
  resp_hdrs = headers.new()
  resp_hdrs = headers.set(resp_hdrs, "set-cookie", set_cookie)
  resp = http.response(200, json.encode(obj.set(obj.new(), "ok", true)))
  resp = obj.set(resp, "headers", resp_hdrs)
  emit resp
done
```

### Gotchas

- `cookie.parse` splits on `"; "` (semicolon-space). Malformed cookie strings may produce unexpected results.
- `cookie.get` returns an empty string `""` if the cookie name is not found — it does not raise an error. Use `obj.has(cookies, name)` to distinguish missing from empty.
- The `Set-Cookie` header must be sent as a response header. `cookie.set` returns a string; you must add it to the response headers dict manually.
- Cookies are not automatically sent on subsequent requests made with `http.*` client ops. Manage cookie jars manually if needed.
