# Collections

forai has two collection types: `list` (ordered arrays) and `dict` (key-value maps). Both are immutable — operations return new collections rather than modifying existing ones. Both map directly to JSON arrays and objects.

## list

A `list` is an ordered, indexed sequence of values. Items can be of any type (lists are untyped at the element level).

### List Literals

```fa
body
    fruits = ["apple", "banana", "cherry"]
    numbers = [1, 2, 3, 4, 5]
    mixed = ["text", 42, true]
    empty = []
done
```

List literals use square brackets with comma-separated values.

### Creating Lists Programmatically

Use `list.new()` for an empty list, then `list.append()` to add items:

```fa
body
    items = list.new()
    items = list.append(items, "first")
    items = list.append(items, "second")
    items = list.append(items, "third")
    # items is now ["first", "second", "third"]
done
```

Because `list.append` returns a new list (rather than mutating), assign the result back to the variable.

### Accessing List Items

```fa
body
    items = ["alpha", "beta", "gamma"]
    first = items[0]     # "alpha"
    last = items[-1]     # "gamma" (negative indices count from end)
    second = items[1]    # "beta"
done
```

Bracket indexing (`items[index]`) retrieves the item at the given index. Negative indices count from the end: `-1` is the last item, `-2` is second to last, and so on.

### Common List Operations

```fa
body
    items = ["apple", "banana", "cherry"]

    n = list.len(items)                    # 3 — number of items
    has_apple = list.contains(items, "apple")  # true

    slice = list.slice(items, 0, 2)        # ["apple", "banana"] — indices [0, 2)
    indices = list.indices(items)          # [0, 1, 2] — useful for indexed loops

    nums = [3, 1, 4, 1, 5]
    joined = str.join(nums, ", ")          # "3, 1, 4, 1, 5"
done
```

| Op | Description |
|----|-------------|
| `list.new()` | Empty list |
| `list.range(start, end)` | `[start, start+1, ..., end]` inclusive |
| `list.append(list, item)` | New list with item appended |
| `items[index]` | Item at index (bracket indexing; negative indices supported) |
| `list.len(list)` | Number of items |
| `list.contains(list, value)` | `true` if value is in the list |
| `list.slice(list, start, end)` | Sub-list `[start, end)` clamped to bounds |
| `list.indices(list)` | `[0, 1, 2, ...]` for each position |

### Iterating Over Lists

Use `loop list as item` to iterate:

```fa
func SumAll
    take numbers as list
    emit result as real
    fail error as text
body
    total = 0.0
    loop numbers as n
        total = total + n
    done
    emit total
done
```

For indexed iteration, use `list.indices`:

```fa
func IndexedProcess
    take items as list
    emit result as list
    fail error as text
body
    results = list.new()
    indices = list.indices(items)
    loop indices as i
        item = items[i]
        processed = "#{i}: #{item}"
        results = list.append(results, processed)
    done
    emit results
done
```

For a range of numbers, use `list.range`:

```fa
body
    iters = list.range(0, 9)    # [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]
    loop iters as i
        # i goes 0, 1, 2, ..., 9
    done
done
```

### Passing Lists to Ops

Many built-in ops accept a `list` as an argument:

```fa
body
    params = list.new()
    params = list.append(params, user_id)
    params = list.append(params, "active")
    rows = db.query(conn, "SELECT * FROM users WHERE id = ? AND status = ?", params)
done
```

For `db.exec` and `db.query`, the third argument is a list of parameter values that replace the `?` placeholders in the SQL string.

## dict

A `dict` is an unordered key-value map where keys are strings and values can be any type. Dicts map directly to JSON objects. All struct values are `dict` at runtime.

### Dict Literals

```fa
body
    config = {host: "localhost", port: 8080, debug: true}
    empty_dict = {}
    nested = {user: {name: "Alice", age: 30}, active: true}
done
```

Dict literal keys are bare identifiers (no quotes needed). Values can be any expression.

### Creating Dicts Programmatically

Use `obj.new()` and `obj.set()`:

```fa
body
    user = obj.new()
    user = obj.set(user, "id", random.uuid())
    user = obj.set(user, "name", "Alice")
    user = obj.set(user, "email", "alice@example.com")
    user = obj.set(user, "active", true)
done
```

`obj.set(dict, key, value)` returns a new dict with the key set. Assign the result back to update the variable.

### Accessing Dict Values

```fa
body
    user = {name: "Alice", age: 30, active: true}

    name = user.name           # dot notation — field access
    age = obj.get(user, "age") # op form — useful when key is dynamic
    has_role = obj.has(user, "role")  # false — key not present
done
```

Dot notation (`user.name`) is syntactic sugar for `obj.get(user, "name")`. Both work. Use dot notation for static field access, `obj.get` when the key is a variable.

### Common Dict Operations

| Op | Description |
|----|-------------|
| `obj.new()` | Empty dict |
| `obj.set(dict, key, value)` | New dict with key set (returns new dict) |
| `obj.get(dict, key)` | Value at key (error if missing) |
| `obj.has(dict, key)` | `true` if key exists |
| `obj.delete(dict, key)` | New dict with key removed |
| `obj.keys(dict)` | List of key strings |
| `obj.merge(dict1, dict2)` | New dict merging both (right overwrites left) |

### Merging Dicts

```fa
body
    defaults = {debug: false, timeout: 30, retries: 3}
    overrides = {timeout: 60, verbose: true}
    config = obj.merge(defaults, overrides)
    # config = {debug: false, timeout: 60, retries: 3, verbose: true}
done
```

### Iterating Over Dict Keys

`obj.keys()` returns a list of key strings that you can loop over:

```fa
func PrintAll
    take data as dict
    emit result as bool
    fail error as text
body
    keys = obj.keys(data)
    loop keys as key
        value = obj.get(data, key)
        line = "#{key}: #{value}"
        _ = term.print(line)
    done
    emit true
done
```

## Collections and JSON

Since `list` maps to JSON array and `dict` maps to JSON object, JSON serialization and deserialization work directly on collections:

```fa
body
    # Parse JSON into a dict
    raw_json = '{"name": "Alice", "scores": [95, 87, 92]}'
    data = json.decode(raw_json)
    name = data.name        # "Alice"
    scores = data.scores    # [95, 87, 92]

    # Serialize back to JSON
    updated = obj.set(data, "verified", true)
    json_out = json.encode(updated)
done
```

## Nested Access

Access nested structures with chained dot notation:

```fa
body
    response = json.decode(http_body)
    user_name = response.user.name
    first_score = response.scores[0]
    city = response.address.city
done
```

## Rules and Gotchas

- Lists and dicts are immutable. `list.append` and `obj.set` return new values; reassign the variable to "update" it.
- `list.range(start, end)` is inclusive on both ends: `list.range(0, 4)` gives `[0, 1, 2, 3, 4]`.
- `list.slice(list, start, end)` is exclusive on the right: `list.slice(items, 0, 2)` gives the first two items (indices 0 and 1).
- Bracket indexing with an out-of-bounds index causes a runtime error. Use `list.len` to guard if the length is uncertain.
- Dict key lookup with `obj.get` errors if the key is missing. Use `obj.has` to check before accessing uncertain keys.
- Dot notation (`dict.field`) and `obj.get(dict, "field")` are equivalent. The compiler desugars dot notation to `obj.get` calls.
- There are no typed lists (e.g., `list<text>`). Lists are untyped at the element level. Type discipline at the element level is the programmer's responsibility.
