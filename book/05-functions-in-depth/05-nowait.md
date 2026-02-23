# Nowait

`nowait` and `send nowait` start a background task without waiting for it to complete. The calling function continues immediately after the dispatch. No result is captured, and errors from the background task are logged to stderr rather than propagated.

## Two Forms

There are two forms of nowait dispatch:

**`nowait op(args)`** — for built-in runtime ops:

```fa
nowait log.info("Background log message")
nowait file.write("/tmp/audit.log", entry)
nowait http.post(webhook_url, { event: "user_created", id: user_id })
```

**`send nowait Func(args)`** — for user-defined funcs and flows:

```fa
send nowait NotifyUser(user_id, "Your order has shipped")
send nowait IndexDocument(doc_id, content)
send nowait CleanupSession(session_id)
```

The caller does not wait for either form. Execution continues to the next statement immediately.

## Behavior

When `nowait` or `send nowait` is executed:

1. The target operation is dispatched as a background async task
2. The caller continues immediately — no blocking
3. No result is captured (the return value is discarded)
4. If the background task fails, the error is logged to stderr but not propagated to the caller
5. The background task runs in an isolated scope — it cannot modify the caller's variables

```fa
func ProcessRequest
  take req   as dict
  emit resp  as dict
  fail error as text
body
  # Process the request immediately
  result = compute_response(req)

  # Fire background analytics — we don't wait for this
  send nowait LogAnalytics(req.user_id, req.path, stamp.now())

  # Return immediately
  emit result
done
```

The caller receives `result` without waiting for `LogAnalytics` to complete.

## Isolated Scope

The background task receives a **copy** of the arguments passed to it, not references to the caller's variables. Once dispatched, the background task is fully independent:

```fa
user_id = "abc123"
send nowait CleanupUser(user_id)
# Changing user_id here does NOT affect CleanupUser
user_id = "xyz789"
```

The `CleanupUser` task has its own copy of `"abc123"` and is not affected by subsequent changes to `user_id`.

## No Error Propagation

Errors from background tasks are silently logged. The calling function cannot detect or handle them:

```fa
# If SendEmail fails, the error goes to stderr
# — it does NOT fail ProcessOrder
send nowait SendEmail(user_email, "Order confirmation", body)
emit order_id
```

This makes `send nowait` appropriate for optional side effects (logging, analytics, notifications) where a failure should not abort the main operation. For operations where errors matter, use a regular (awaited) call.

## Nowait in Flow Bodies

In a flow body, `send nowait` dispatches a background func without blocking the pipeline:

```fa
flow HandleOrder
  on req from order_server
    step ValidateOrder(req to :data)
    next :result to validated_order

    send nowait AuditLog(validated_order to :entry)

    step FulfillOrder(validated_order to :order)
    next :result to fulfillment
    emit fulfillment to :out
  done
done
```

The `AuditLog` call runs in the background. The pipeline continues to `FulfillOrder` immediately.

## Differences from Sync

| Feature | `sync` | `send nowait` |
|---------|--------|---------------|
| Waits for completion | Yes | No |
| Captures result | Yes | No |
| Propagates errors | Yes | No (logs only) |
| Scope isolation | Copy per statement | Full copy |
| Use case | Parallel fan-out with results | Fire-and-forget side effects |

## When to Use Nowait

Use `nowait` or `send nowait` for:

- **Logging and auditing**: Log events after responding, where log failures should not affect the response
- **Notifications**: Send emails, push notifications, webhooks without blocking the response
- **Analytics**: Track usage events asynchronously
- **Cache warming**: Pre-populate a cache without waiting
- **Cleanup**: Delete temporary files, expire sessions, trigger garbage collection

Do not use `send nowait` for:

- Operations whose output is needed by the current function
- Operations where failures must be handled
- Long-running jobs that need progress tracking (use a proper queue instead)

## Practical Examples

```fa
func CreateUser
  take data  as dict
  emit user  as dict
  fail error as text
body
  user = db.exec(conn, "INSERT INTO users VALUES (?)", [json.encode(data)])

  # These run in the background — CreateUser returns immediately after db.exec
  send nowait WelcomeEmail(data.email, data.name)
  send nowait InitUserStats(user.id)
  nowait log.info("User created: #{user.id}")

  emit user
done

sink AuditEvent
  take event as dict
  fail error as text
body
  entry = json.encode(event)
  nowait file.append("/var/log/audit.log", entry + "\n")
done
```
