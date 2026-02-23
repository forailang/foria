# Chapter 11.2: Sync — Concurrent Joins

The `sync` block lets you run multiple independent operations concurrently inside a `func` body and collect their results once all complete. It is forai's equivalent of `Promise.all` or `join_all` — a structured way to fan out work and fan it back in.

## Syntax

```fa
[var_a, var_b, var_c] = sync :timeout => 5s, :retry => 2, :safe => true
    var_a = OpA(...)
    var_b = OpB(...)
    var_c = OpC(...)
done [var_a, var_b, var_c]
```

The `sync` block has three parts:

1. **Left-hand binding** `[var_a, var_b, var_c]` — variables to receive results from the block.
2. **Options** — zero or more key-value options prefixed with `:`. All are optional.
3. **Body** — a sequence of statements. Each statement runs concurrently with the others.
4. **Export list** `done [var_a, var_b, var_c]` — which variables from inside the block are merged back into the outer scope.

## Options

| Option | Type | Description |
|--------|------|-------------|
| `:timeout => 5s` | duration | Maximum time to wait for all statements. Supports `ms`, `s`, `m` suffixes (e.g. `500ms`, `5s`, `2m`). If exceeded, fails with a timeout error. |
| `:retry => 2` | integer | Retry count. If any statement fails, the whole sync block is retried up to this many times before propagating the error. |
| `:safe => true` | bool | If `true`, any statement failure is converted to `null` instead of propagating. The block always succeeds; failed results are `null`. |

Options can be combined:

```fa
[a, b] = sync :timeout => 10s, :retry => 1, :safe => true
    a = http.get("https://api.example.com/users")
    b = http.get("https://api.example.com/items")
done [a, b]
```

## How Statements Execute

Each statement in a `sync` body runs concurrently via `join_all`. The runtime launches all of them at once, then waits for every one to complete before continuing past the `done` line.

Each statement gets its **own isolated copy** of the current scope. This means:

- Reads from outer variables work — the copy has the same values.
- Writes inside one statement are **not visible** to other statements inside the same sync block.
- This is enforced by design: sync statements must be independent.

After all statements complete, only the variables listed in the `done [...]` export list are merged back into the outer scope. Variables created inside the sync body that are not exported are discarded.

## Basic Example: Fan-Out HTTP Calls

```fa
docs FetchDashboard
    Fetches user profile, notifications, and feed in parallel.
done

func FetchDashboard
    take user_id as text
    emit result as dict
    fail error as text
body
    url_profile = "https://api.example.com/profile/" + user_id
    url_notifs = "https://api.example.com/notifications/" + user_id
    url_feed = "https://api.example.com/feed/" + user_id

    [profile, notifs, feed] = sync :timeout => 8s
        profile = http.get(url_profile)
        notifs = http.get(url_notifs)
        feed = http.get(url_feed)
    done [profile, notifs, feed]

    result = obj.new()
    result = obj.set(result, "profile", profile)
    result = obj.set(result, "notifications", notifs)
    result = obj.set(result, "feed", feed)
    emit result to :result
done
```

All three HTTP requests are issued simultaneously. The total wall-clock time is approximately `max(t_profile, t_notifs, t_feed)`, not the sequential sum.

## Scope Isolation Rules

Because each statement gets its own scope copy, the following is a compiler error — `b` tries to read `a`, but they are in isolated scopes:

```fa
# WRONG — cross-reference inside sync body
[a, b] = sync
    a = db.query(conn, "SELECT * FROM users")
    b = db.query(conn, "SELECT * FROM #{a}")  # a is not available here
done [a, b]
```

Correct approach: compute `a` before the sync block, then use its value inside:

```fa
# Correct — use outer variable (read-only copy)
first_table = "users"
[rows_a, rows_b] = sync
    rows_a = db.query(conn, "SELECT * FROM " + first_table)
    rows_b = db.query(conn, "SELECT count(*) FROM " + first_table)
done [rows_a, rows_b]
```

## Using :safe for Partial Results

When `:safe => true`, a failed statement sets its variable to `null` rather than failing the whole block. This is useful when you want best-effort parallel fetches:

```fa
docs FetchOptional
    Fetches two external APIs; either may be unavailable.
done

func FetchOptional
    take id as text
    emit result as dict
    fail error as text
body
    [primary, fallback] = sync :timeout => 3s, :safe => true
        primary = http.get("https://primary.example.com/" + id)
        fallback = http.get("https://fallback.example.com/" + id)
    done [primary, fallback]

    result = obj.new()
    result = obj.set(result, "primary", primary)
    result = obj.set(result, "fallback", fallback)
    emit result to :result
done
```

If `primary` fails or times out, `primary` will be `null`. The caller can inspect the result dict and decide how to handle absent values.

## Database + HTTP in Parallel

A common pattern is to kick off a DB query and an external HTTP call at the same time:

```fa
docs EnrichUser
    Fetches user from DB and their public profile from an external service simultaneously.
done

func EnrichUser
    take conn as db_conn
    take user_id as text
    emit result as dict
    fail error as text
body
    profile_url = "https://profiles.example.com/" + user_id
    params = list.new()
    params = list.append(params, user_id)

    [db_row, ext_profile] = sync :timeout => 5s
        db_row = db.query(conn, "SELECT * FROM users WHERE id = ?1", params)
        ext_profile = http.get(profile_url)
    done [db_row, ext_profile]

    result = obj.new()
    result = obj.set(result, "local", db_row)
    result = obj.set(result, "external", ext_profile)
    emit result to :result
done
```

## Timeout Suffixes

| Suffix | Meaning | Example |
|--------|---------|---------|
| `ms` | Milliseconds | `:timeout => 500ms` |
| `s` | Seconds | `:timeout => 5s` |
| `m` | Minutes | `:timeout => 2m` |

Durations must be positive integers followed immediately by the suffix with no space.

## What sync Is Not

`sync` is not a general-purpose thread spawner. It is a structured join:

- All statements start at the same moment.
- The block does not complete until all statements finish (or timeout/failure triggers).
- There is no way to get partial results before all statements complete. Use `:safe => true` if you want to tolerate failures without blocking.

For fire-and-forget work that should not block the caller at all, see `send nowait` in the next chapter.

## Nesting

`sync` blocks can be nested inside `loop` or `case` arms, and can appear anywhere in a `func` body. Each `sync` is an independent join scope.

```fa
docs ProcessBatch
    Processes a batch of items, fetching each from two sources in parallel.
done

func ProcessBatch
    take items as list
    emit result as list
    fail error as text
body
    result = list.new()
    loop items as item
        item_id = obj.get(item, "id")
        url_a = "https://source-a.example.com/" + item_id
        url_b = "https://source-b.example.com/" + item_id

        [data_a, data_b] = sync :timeout => 3s, :safe => true
            data_a = http.get(url_a)
            data_b = http.get(url_b)
        done [data_a, data_b]

        merged = obj.new()
        merged = obj.set(merged, "a", data_a)
        merged = obj.set(merged, "b", data_b)
        result = list.append(result, merged)
    done
    emit result to :result
done
```
