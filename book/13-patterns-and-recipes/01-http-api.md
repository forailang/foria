# Chapter 13.1: HTTP API Server

Building an HTTP API server in forai follows a consistent pattern: a `source` accepts connections, a `flow` routes requests to handler `func`s, and `sink`s or funcs send responses. This chapter walks through a complete annotated example.

## The Full Pattern

```
http.server.listen(port) → source accepts connections → flow routes requests → handler funcs → http.server.respond
```

The key built-in ops:

| Op | Description |
|----|-------------|
| `http.server.listen(port)` | Opens an HTTP server on the given port. Returns an `http_server` handle. |
| `http.server.accept(server)` | Waits for the next HTTP request. Returns a dict with `conn_id`, `method`, `path`, `body`, `headers`. |
| `http.server.respond(conn_id, status, body, headers)` | Sends an HTTP response for the given connection. |
| `route.match(pattern, path)` | Tests whether a URL path matches a pattern (e.g. `"/users/:id"`). Returns a dict of extracted params, or `null` if no match. |
| `http.response(status, body)` | Constructs a response dict. |
| `http.error_response(status, message)` | Constructs an error response dict. |

## Source: Accepting HTTP Requests

The `source` construct wraps the accept loop. Its body uses `on :request from http.server.accept(srv) to req` to receive one request at a time:

```fa
# sources/HTTPRequests.fa
docs HTTPRequests
    Accepts HTTP connections on a given port and emits parsed request dicts.
    Each emitted value is a dict with conn_id, method, path, body, and headers.
done

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

test HTTPRequests
    ok = true
    must ok == true
done
```

The `on :request from ... to req` block runs once per connection. The source does not close the server — it runs forever, emitting one event per request.

## Routing

Routing is typically done in a `func` that uses `route.match` to check which pattern the request path matches:

```fa
# handler/Route.fa
docs Route
    Routes an incoming request to a handler name.
done

func Route
    take req as dict
    emit result as text
    fail error as text
body
    path = obj.get(req, "path")
    method = obj.get(req, "method")

    match_users_list = route.match("/api/users", path)
    match_user_detail = route.match("/api/users/:id", path)
    match_health = route.match("/health", path)

    route_name = "not_found"
    case match_health
        when null
        else
            route_name = "health"
    done
    case match_users_list
        when null
        else
            route_name = "users_list"
    done
    case match_user_detail
        when null
        else
            route_name = "user_detail"
    done

    emit route_name to :result
done
```

## Handler Funcs

Each route has a handler func that receives the request, does its work, and sends the response via `http.server.respond`:

```fa
# handler/routes/HealthCheck.fa
docs HealthCheck
    Returns a 200 OK health check response.
done

func HealthCheck
    take req as dict
    emit result as bool
    fail error as text
body
    conn_id = obj.get(req, "conn_id")
    headers = headers.new()
    headers = headers.set(headers, "Content-Type", "application/json")
    body_text = "{\"status\": \"ok\"}"
    http.server.respond(conn_id, 200, body_text, headers)
    emit true to :result
done
```

```fa
# handler/routes/ListUsers.fa
docs ListUsers
    Queries all users from the database and returns them as JSON.
done

func ListUsers
    take conn as db_conn
    take req as dict
    emit result as bool
    fail error as text
body
    conn_id = obj.get(req, "conn_id")
    rows = db.query(conn, "SELECT id, name, email FROM users")
    body_text = json.encode(rows)
    headers = headers.new()
    headers = headers.set(headers, "Content-Type", "application/json")
    http.server.respond(conn_id, 200, body_text, headers)
    emit true to :result
done
```

```fa
# handler/routes/GetUser.fa
docs GetUser
    Looks up a single user by ID and returns them as JSON.
done

func GetUser
    take conn as db_conn
    take req as dict
    emit result as bool
    fail error as text
body
    conn_id = obj.get(req, "conn_id")
    path = obj.get(req, "path")
    params = route.match("/api/users/:id", path)
    user_id = obj.get(params, "id")

    query_params = list.new()
    query_params = list.append(query_params, user_id)
    rows = db.query(conn, "SELECT id, name, email FROM users WHERE id = ?1", query_params)

    row_count = list.len(rows)
    headers = headers.new()
    headers = headers.set(headers, "Content-Type", "application/json")
    case row_count
        when 0
            error_body = "{\"error\": \"Not found\"}"
            http.server.respond(conn_id, 404, error_body, headers)
            emit true to :result
        else
            first_row = rows[0]
            body_text = json.encode(first_row)
            http.server.respond(conn_id, 200, body_text, headers)
            emit true to :result
    done
done
```

## Flow: Wiring It Together

The `flow` wires the source, router, and handlers:

```fa
# HandleRequest.fa
use sources from "./sources"
use handler from "./handler"

docs HandleRequest
    Main HTTP API flow: accept, route, dispatch, respond.
done

flow HandleRequest
    emit result as dict
    fail error as dict
body
    state conn = db.open("app.db")
    step db.Migrate(conn to :conn) done
    step sources.HTTPRequests(8080 to :port) then
        next :req to req
    done
    step handler.Route(req to :req) then
        next :result to route_name
    done
    branch when route_name == "health"
        step handler.routes.HealthCheck(req to :req) done
    done
    branch when route_name == "users_list"
        step handler.routes.ListUsers(conn to :conn, req to :req) done
    done
    branch when route_name == "user_detail"
        step handler.routes.GetUser(conn to :conn, req to :req) done
    done
    branch when route_name == "not_found"
        step handler.routes.NotFound(req to :req) done
    done
done
```

The `state conn = db.open("app.db")` line opens the database once for the entire server lifetime. The handle is passed into every request handler that needs it.

## Request Body Parsing

For `POST` and `PUT` routes, parse the request body as JSON:

```fa
func CreateUser
    take conn as db_conn
    take req as dict
    emit result as bool
    fail error as text
body
    conn_id = obj.get(req, "conn_id")
    raw_body = obj.get(req, "body")
    parsed = json.decode(raw_body)

    name = obj.get(parsed, "name")
    email = obj.get(parsed, "email")

    id = random.uuid()
    params = list.new()
    params = list.append(params, id)
    params = list.append(params, name)
    params = list.append(params, email)
    ok = db.exec(conn, "INSERT INTO users (id, name, email) VALUES (?1, ?2, ?3)", params)

    response_body = json.encode(obj.set(obj.new(), "id", id))
    headers = headers.new()
    headers = headers.set(headers, "Content-Type", "application/json")
    http.server.respond(conn_id, 201, response_body, headers)
    emit true to :result
done
```

## Authentication via Headers

Extract and validate auth tokens in a dedicated func before dispatching:

```fa
func AuthCheck
    take req as dict
    emit result as dict
    fail error as text
body
    req_headers = obj.get(req, "headers")
    auth_header = headers.get(req_headers, "Authorization")

    case auth_header
        when null
            result = obj.set(obj.new(), "authenticated", false)
            emit result to :result
        else
            # strip "Bearer " prefix
            token = str.slice(auth_header, 7, str.len(auth_header))
            verified = crypto.verify_token(token, "secret")
            result = obj.set(obj.new(), "authenticated", verified)
            result = obj.set(result, "token", token)
            emit result to :result
    done
done
```

## Test Blocks

Route and handler funcs can be tested by mocking the DB and crafting fake request dicts:

```fa
test GetUser
    mock db.query => [{id: "u1", name: "Alice", email: "alice@example.com"}]
    mock http.server.respond => true
    conn = db.open(":memory:")
    req = {conn_id: "c1", path: "/api/users/u1", method: "GET", body: "", headers: {}}
    result = GetUser(conn to :conn, req to :req)
    must result == true
done
```
