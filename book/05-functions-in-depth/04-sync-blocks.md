# Sync Blocks

A `sync` block runs multiple statements concurrently and waits for all of them to complete before continuing. It is forai's mechanism for parallel fan-out within a single function — the equivalent of `join_all` in async Rust or `Promise.all` in JavaScript.

## Basic Syntax

```fa
[out1, out2] = sync
  out1 = OperationOne(arg)
  out2 = OperationTwo(arg)
done [out1, out2]
```

The variables in the brackets on the left are the exports that will be merged back into the outer scope. The `done [exports]` list names which variables from inside the sync block to export.

## How Sync Works

When execution reaches a `sync` block:

1. Each statement in the block is dispatched concurrently (via `join_all`)
2. Each statement receives an isolated copy of the current scope
3. All statements run in parallel, completing in whatever order they finish
4. When all statements have completed, their exported variables are merged back into the outer scope
5. Execution continues with the statement after `done [exports]`

```fa
func FetchUserData
  take user_id as text
  emit profile as dict
  fail error   as text
body
  [name, email, prefs] = sync
    name  = lookup_name(user_id)
    email = lookup_email(user_id)
    prefs = lookup_prefs(user_id)
  done [name, email, prefs]

  profile = { name: name, email: email, prefs: prefs }
  emit profile
done
```

All three lookups run simultaneously. If any of them fails, the sync block fails and the function's fail port fires.

## Scope Isolation

Each statement inside a sync block receives an **independent copy** of the outer scope at the moment the sync starts. Statements cannot share variables with each other — they are fully isolated:

```fa
x = 10

[a, b] = sync
  a = service_a(x)    # sees x = 10
  b = service_b(x)    # also sees x = 10, NOT 'a'
done [a, b]
```

Trying to reference a variable from another sync statement is a logic error — that variable is not in scope:

```fa
# Wrong: b cannot see 'a' because they run concurrently
[a, b] = sync
  a = fetch_base()
  b = transform(a)    # 'a' is not available here; use sequential code instead
done [a, b]

# Correct: run sequentially for dependent operations
a = fetch_base()
b = transform(a)
```

## Sync Options

The `sync` keyword accepts options using the fat-arrow syntax:

```fa
[result] = sync :timeout => 5000, :retry => 3, :safe => true
  result = slow_external_call(data)
done [result]
```

| Option | Type | Description |
|--------|------|-------------|
| `:timeout` | long (ms) | Maximum milliseconds to wait before timing out |
| `:retry` | long | Number of times to retry failed statements |
| `:safe` | bool | If `true`, a failing statement does not fail the whole sync |

With `:safe => true`, failed statements produce null/zero values for their exports rather than propagating the failure. This lets you handle partial results:

```fa
[primary, fallback] = sync :safe => true
  primary  = fetch_primary(id)
  fallback = fetch_fallback(id)
done [primary, fallback]

result = primary ? primary : fallback
```

## Break Is Swallowed in Sync

A `break` statement inside a sync block is swallowed — it does not propagate to the enclosing loop. Each sync statement is an independent task; there is no concept of "breaking out" of a concurrent group. If you need to conditionally exit a loop based on sync results, check the exported values after the sync:

```fa
loop items as item
  [result] = sync
    result = process(item)
  done [result]

  # Check here, outside the sync
  if result == "stop"
    break
  else
    continue_with(result)
  done
done
```

## Sync in a Loop

Sync blocks inside loops work correctly. Each loop iteration gets a fresh sync — the concurrent operations within each iteration run in parallel, but the iterations themselves are sequential:

```fa
summaries = []
loop batches as batch
  [processed] = sync :timeout => 10000
    processed = ProcessBatch(batch)
  done [processed]
  summaries = list.append(summaries, processed)
done
```

This processes each batch with a 10-second timeout before moving to the next.

## Exported Variables

Only variables listed in `done [exports]` are available in the outer scope after the sync. Any intermediate variables computed inside sync statements are discarded:

```fa
[name, email] = sync
  name  = user.name    # exported
  email = user.email   # exported
  temp  = "ignored"    # NOT exported — discarded after sync
done [name, email]

# name and email are in scope; temp is not
```

## Practical Examples

### Parallel HTTP requests

```fa
func FetchAll
  take ids as list
  emit data as list
  fail error as text
body
  id1 = ids[0]
  id2 = ids[1]
  id3 = ids[2]

  [r1, r2, r3] = sync :timeout => 5000
    r1 = http.get("https://api.example.com/item/" + id1)
    r2 = http.get("https://api.example.com/item/" + id2)
    r3 = http.get("https://api.example.com/item/" + id3)
  done [r1, r2, r3]

  emit [r1.body, r2.body, r3.body]
done
```

### Parallel database queries

```fa
[users, orders, stats] = sync
  users  = db.query(conn, "SELECT * FROM users LIMIT 100", [])
  orders = db.query(conn, "SELECT * FROM orders WHERE date > ?", [cutoff])
  stats  = db.query(conn, "SELECT COUNT(*), AVG(amount) FROM orders", [])
done [users, orders, stats]
```

### Fan-out with safe mode

```fa
[cache_hit, db_result] = sync :safe => true
  cache_hit = redis.get(cache_key)
  db_result = db.query(conn, "SELECT * FROM data WHERE id = ?", [id])
done [cache_hit, db_result]

final = cache_hit ? cache_hit : db_result
```
