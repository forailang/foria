# Error Handling

forai's error model is built around explicit propagation through `fail` ports and opt-in trapping with `trap`. Errors are not exceptions — they are first-class values flowing through the pipeline.

## Fail: Declaring and Emitting Errors

Every func and sink declares its error port with `fail`:

```fa
func ParseInt
  take raw   as text
  emit value as long
  fail error as text
body
  # Attempt conversion
  n = to.long(raw)
  if n == 0 && raw != "0"
    fail "Cannot parse as integer: #{raw}"
  else
    emit n
  done
done
```

The `fail` statement sends a value on the fail port and exits the function immediately. No further statements execute after `fail`.

## Default Error Propagation

When you call a function that can fail, and you do not trap the failure, any failure propagates automatically to the calling function's own fail port:

```fa
func ProcessOrder
  take order_id as text
  emit receipt  as text
  fail error    as text
body
  # If FetchOrder fails, ProcessOrder also fails
  order   = FetchOrder(order_id)

  # If BuildReceipt fails, ProcessOrder also fails
  receipt = BuildReceipt(order)
  emit receipt
done
```

This "fail-fast" behavior means errors bubble up through the call stack without any boilerplate. A failure in a deep function automatically surfaces at the top level.

## Trap: Capturing Failures

Use `trap` to intercept a failure instead of propagating it:

```fa
result = trap MyFunc(arg1, arg2)
```

`trap` evaluates the call and returns a result object with two fields:

- `result.ok` — boolean, `true` if the call succeeded
- `result.value` — the emitted value (available if `ok` is `true`)
- `result.error` — the failure message (available if `ok` is `false`)

```fa
func SafeFetch
  take url   as text
  emit body  as text
  fail error as text
body
  response = trap http.get(url)
  if response.ok
    emit response.value.body
  else
    fail "Fetch failed for #{url}: #{response.error}"
  done
done
```

With `trap`, you decide what to do with the error: retry, return a default, re-fail with context, or log and continue.

## Re-Failing with Context

A common pattern is to trap a failure, add context, and re-fail:

```fa
user = trap db.query_user_by_email(conn, email)
if user.ok
  emit user.value
else
  fail "User lookup failed for #{email}: #{user.error}"
done
```

This preserves the original error message while adding information about which operation failed.

## Propagating a Captured Error

If you have trapped a failure and want to re-propagate it unchanged, assign the error to a variable and use `fail`:

```fa
result = trap RiskyOperation(input)
if result.ok
  emit result.value
else
  err = result.error
  fail err
done
```

## Fail in Case Arms

`fail` can appear inside `case` arms, `if/else` branches, and loops:

```fa
func Validate
  take age  as long
  emit age  as long
  fail error as text
body
  if age < 0
    fail "Age cannot be negative: #{age}"
  else if age > 150
    fail "Age too large: #{age}"
  else
    emit age
  done
done
```

When `fail` is reached, the function exits immediately regardless of nesting depth.

## Multiple Fail Ports

A func can declare multiple named fail ports for different error categories:

```fa
func ConnectDB
  take dsn     as text
  emit conn    as db_conn
  fail network as text
  fail auth    as text
body
  result = trap db.open(dsn)
  if !result.ok
    if str.contains(result.error, "auth")
      fail result.error as auth
    else
      fail result.error as network
    done
  else
    emit result.value
  done
done
```

In the calling flow, each fail port can be routed independently.

## Input Validation Failures

Type constraints on `take` declarations automatically generate failures before the body runs:

```fa
func Percent
  take value as real :min 0.0 :max 100.0
  emit label as text
body
  emit "#{value}%"
done
```

Passing `150.0` to `Percent` fails immediately with a constraint violation — no body code needed.

## Error Values in Flows

In a flow, the fail output of a step can be routed to an error handler:

```fa
flow ProcessRequest
  on req from server
    step ParseBody(req to :raw)
    next :result to parsed
    next :error  to err_msg

    branch when err_msg
      step SendError(err_msg to :msg)
      emit err_msg to :fail
    done

    step BuildResponse(parsed to :data)
    next :result to response
    emit response to :out
  done
done
```

This is the flow-level equivalent of `trap` — the error port routes to a recovery branch.

## Practical Patterns

### Default on failure

```fa
config = trap file.read("/etc/app/config.json")
if config.ok
  settings = json.decode(config.value)
else
  settings = { host: "localhost", port: 8080 }
done
```

### Retry on failure

```fa
attempts = 0
success  = false
result   = ""

loop
  attempts = attempts + 1
  resp = trap http.get(url)
  if resp.ok
    result  = resp.value.body
    success = true
    break
  else if attempts >= 3
    break
  else
    time.sleep(1000)
  done
done

if success
  emit result
else
  fail "Failed after #{attempts} attempts"
done
```

### Batch with partial failure

```fa
results = []
errors  = []

loop items as item
  out = trap Process(item)
  if out.ok
    results = list.append(results, out.value)
  else
    errors = list.append(errors, out.error)
  done
done

emit { results: results, errors: errors }
```
