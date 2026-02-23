# Primitive Types

forai has eight primitive types. They map directly to JSON representations, because the IR (intermediate representation) that the compiler produces is JSON, and values flow through the runtime as JSON-compatible data.

## The Eight Primitives

| Type | Description | JSON representation | Example literal |
|------|-------------|---------------------|-----------------|
| `text` | Unicode string | `String` | `"hello"` |
| `bool` | Boolean | `Boolean` | `true`, `false` |
| `long` | 64-bit signed integer | `Integer` | `42`, `-7`, `1000000` |
| `real` | 64-bit floating-point | `Number` | `3.14`, `0.0`, `-2.5` |
| `uuid` | UUID string | `String` (semantically a UUID) | `"550e8400-e29b-41d4-a716-446655440000"` |
| `time` | Timestamp string | `String` (semantically a timestamp) | `"2024-01-15T10:30:00Z"` |
| `void` | Absence of value | `Null` | (no literal; produced by ops that return nothing) |
| `list` | Ordered collection | `Array` | `["a", "b", "c"]` |
| `dict` | Key-value map | `Object` | `{name: "Alice", age: 30}` |

## text

`text` is a Unicode string. String literals use double quotes. The `str.*` namespace provides all string operations.

```fa
func Greet
    take name as text
    emit result as text
    fail error as text
body
    greeting = "Hello, #{name}!"
    upper = str.upper(name)
    length = str.len(name)
    emit greeting
done
```

String interpolation embeds expressions with `#{}`:

```fa
msg = "User #{user.name} logged in at #{stamp.now()}"
path = "/users/#{user_id}/orders/#{order_id}"
```

Escape sequences inside strings: `\n` (newline), `\t` (tab), `\\` (backslash), `\"` (double quote), `\#` (literal `#` to avoid interpolation). Bare `{` and `}` are literal characters — safe for regex quantifiers.

## bool

`bool` has two values: `true` and `false`. Boolean expressions use standard operators: `&&` (and), `||` (or), `!` (not), `==` (equal), `!=` (not equal), `<`, `>`, `<=`, `>=`.

```fa
func CheckAccess
    take user as User
    emit result as bool
    fail error as text
body
    is_admin = user.role == "admin"
    is_active = user.active == true
    has_access = is_admin && is_active
    emit has_access
done
```

## long

`long` is a 64-bit signed integer. Use it for counts, indices, sizes, and any value that must be a whole number. Arithmetic operators and `math.floor`/`math.round` work with both `long` and `real`.

```fa
func CountItems
    take items as list
    emit result as long
    fail error as text
body
    count = list.len(items)
    doubled = count * 2
    emit doubled
done
```

Integer literals are undecorated: `42`, `0`, `-100`, `1_000_000` (underscores are not supported — write `1000000`).

## real

`real` is a 64-bit IEEE 754 float. Use it for measurements, rates, probabilities, and any value that may have a fractional part.

```fa
func CircleArea
    take radius as real
    emit result as real
    fail error as text
body
    pi = 3.14159265
    area = pi * radius * radius
    emit area
done
```

Float literals require a decimal point: `3.14`, `0.0`, `-2.5`. A bare `42` is a `long`; write `42.0` for a `real`.

## uuid

`uuid` is semantically a UUID but stored as a string. The `random.uuid()` op generates a new UUID. Use it for identifiers that need global uniqueness.

```fa
func CreateId
    take _ as void
    emit result as uuid
    fail error as text
body
    id = random.uuid()
    emit id
done
```

There is no UUID literal syntax. UUIDs are always generated or received from external systems.

## time

`time` is semantically a timestamp but stored as an ISO 8601 string. The `stamp.*` namespace provides timestamp operations. The `date.*` namespace provides calendar-aware date operations.

```fa
func Now
    take _ as void
    emit result as time
    fail error as text
body
    ts = stamp.now()
    emit ts
done
```

## void

`void` represents the absence of a meaningful value. It appears as `null` in JSON. Some ops return `void` when they have no useful return value. You can use `_` to discard a `void` result:

```fa
body
    _ = term.print("hello")    # term.print returns void
    _ = db.exec(conn, sql)     # db.exec returns void
done
```

`void` can appear as a `take` type for funcs that need no input:

```fa
func GetConfig
    take _ as void
    emit result as Config
    fail error as text
body
    # ... load config
done
```

## Type Conversion

The `to.*` namespace converts between scalar types:

```fa
body
    n = 42
    s = to.text(n)        # "42"
    f = to.real(n)        # 42.0
    b = to.bool(n)        # true (non-zero)

    s2 = "3.14"
    r = to.real(s2)       # 3.14
    i = to.long(s2)       # 3 (truncated)
done
```

## Equality and Comparison

All primitive types support `==` and `!=`. These operators use deep JSON structural equality, which means two values are equal if their JSON representations are identical. `long` and `real` comparisons: `42 == 42.0` is true in JSON (both are numbers), but be careful — `to.text(42)` produces `"42"`, which is not equal to `42`.

Ordering operators (`<`, `>`, `<=`, `>=`) work on `long`, `real`, and `text` (lexicographic order).

## JSON Representation Reference

When debugging with `forai dev` or inspecting compiled IR, values appear in their JSON form:

| forai value | JSON |
|-------------|------|
| `"hello"` | `"hello"` |
| `true` | `true` |
| `42` | `42` |
| `3.14` | `3.14` |
| `"550e8400-..."` | `"550e8400-..."` |
| `"2024-01-15T..."` | `"2024-01-15T..."` |
| (void) | `null` |
| `["a", "b"]` | `["a","b"]` |
| `{name: "Alice"}` | `{"name":"Alice"}` |

The JSON representation is not just a runtime detail — it is the contract. When a func emits a `text` value, the flow receives a JSON string. When a sink takes a `dict`, it receives a JSON object. This uniformity makes serialization trivial and makes the debugger's trace output immediately readable.
