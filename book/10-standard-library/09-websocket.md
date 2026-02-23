# 10.9 — ws (WebSocket client)

The `ws.*` namespace provides a WebSocket client. It connects to a remote WebSocket server, sends and receives messages, and closes the connection. The connection is represented by a `ws_conn` handle that is threaded through all subsequent ops.

## Handle Type

| Handle | Created by | Used by |
|--------|-----------|---------|
| `ws_conn` | `ws.connect` | `ws.send`, `ws.recv`, `ws.close` |

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `ws.connect` | url | ws_conn | Perform TCP connect + WebSocket handshake; returns connection handle |
| `ws.send` | ws_conn, msg | bool | Send a text message frame |
| `ws.recv` | ws_conn | WebSocketMessage | Block until next message |
| `ws.close` | ws_conn | bool | Send close frame and shut down connection |

### WebSocketMessage (built-in type)

`ws.recv` returns a `WebSocketMessage`:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | text | yes | `"text"`, `"binary"`, `"ping"`, `"pong"`, `"close"` |
| `data` | text | yes | Message payload (text frames) or empty for control frames |

This is a built-in type — use `WebSocketMessage` directly in `take`/`emit` declarations without defining it.

## Examples

### Basic connect, send, receive, close

```fa
func PingServer
  take url text
  emit response text
  fail err dict
body
  conn = ws.connect(url)
  ws.send(conn, "ping")
  msg = ws.recv(conn)
  ws.close(conn)
  if obj.get(msg, "type") != "text"
    fail error.new("UNEXPECTED_MSG_TYPE", "Expected text frame")
  done
  emit obj.get(msg, "data")
done
```

### Subscribe to a stream (inside a source)

```fa
source MarketDataSource
  emit tick dict
body
  conn = ws.connect("wss://stream.example.com/market")
  ws.send(conn, json.encode(obj.set(obj.new(), "action", "subscribe")))
  loop list.range(1, 999999) as _
    msg = ws.recv(conn)
    kind = obj.get(msg, "type")
    if kind == "close"
      break
    done
    if kind == "text"
      data = json.decode(obj.get(msg, "data"))
      emit data
    done
  done
  ws.close(conn)
done
```

### JSON message protocol

Many WebSocket APIs exchange JSON messages:

```fa
func WsRpc
  take url text
  take method text
  take params dict
  emit result value
  fail err dict
body
  conn = ws.connect(url)
  request = obj.new()
  request = obj.set(request, "method", method)
  request = obj.set(request, "params", params)
  request = obj.set(request, "id", random.uuid())
  ws.send(conn, json.encode(request))
  msg = ws.recv(conn)
  ws.close(conn)
  payload = json.decode(obj.get(msg, "data"))
  if obj.has(payload, "error")
    fail error.new("RPC_ERROR", obj.get(obj.get(payload, "error"), "message"))
  done
  result = obj.get(payload, "result")
  emit result
done
```

### Handling different message types

```fa
func ProcessMessages
  take ws_url text
  emit count long
body
  conn = ws.connect(ws_url)
  count = 0
  done_flag = false
  loop list.range(1, 10000) as _
    if done_flag == false
      msg = ws.recv(conn)
      kind = obj.get(msg, "type")
      case kind
        when "text"
          data = obj.get(msg, "data")
          log.info("Received: #{data}")
          count = count + 1
        when "close"
          done_flag = true
        else
          log.debug("Ignoring frame type: #{kind}")
      done
    done
  done
  ws.close(conn)
  emit count
done
```

### Sending multiple messages

```fa
func BatchSend
  take url text
  take messages list
  emit sent long
  fail err dict
body
  conn = ws.connect(url)
  sent = 0
  loop messages as msg
    ok = ws.send(conn, msg)
    if ok
      sent = sent + 1
    done
  done
  ws.close(conn)
  emit sent
done
```

## Common Patterns

### Connection with authentication

Many WebSocket servers require auth on the first message:

```fa
conn = ws.connect(url)
auth_msg = obj.set(obj.set(obj.new(), "type", "auth"), "token", token)
ws.send(conn, json.encode(auth_msg))
ack = ws.recv(conn)
# check ack before proceeding
```

### Heartbeat / keepalive

If the server sends ping frames, reply with pong:

```fa
msg = ws.recv(conn)
if obj.get(msg, "type") == "ping"
  ws.send(conn, "")   # pong response
done
```

### Reconnection pattern

```fa
connected = false
conn = obj.new()  # placeholder
loop list.range(1, 5) as attempt
  if connected == false
    conn = ws.connect(url)
    connected = true
  done
done
```

## Gotchas

- `ws.recv` **blocks** until a message arrives. There is no timeout. Design sources to loop on `ws.recv` — do not call it from a regular func that should return quickly.
- `ws.send` returns `true` on success. Network errors raise a runtime error.
- After receiving a `"close"` type message, calling `ws.recv` again will raise an error. Use a flag to break out of the loop when you receive a close frame.
- Binary frames are received with `type: "binary"` but the `data` field contains the bytes decoded as a UTF-8 string — this may be lossy for truly binary data. forai WebSocket is designed for text-protocol APIs.
- `ws.connect` performs both TCP connection and WebSocket upgrade handshake. Network errors (DNS failure, refused connection) raise a runtime error immediately.
- There is no built-in WebSocket *server* — forai only provides a WebSocket client. For bidirectional server-side WebSocket, use `http.server.*` with upgrade support (if available in the runtime version).
