# Structs

Structs are named composite types with named fields. They are the primary way to group related data in forai. Use structs to give meaningful names to the inputs and outputs of funcs, flows, and sinks.

## Declaring a Struct

Use the `type` keyword (or its alias `data`) followed by the type name, field declarations, and `done`:

```fa
docs User
    A registered user account.

    docs id
        The unique user identifier.
    done

    docs name
        The user's display name.
    done

    docs email
        The user's email address.
    done

    docs active
        Whether the account is currently active.
    done
done

type User
    id    uuid
    name  text
    email text
    active bool
done
```

Every field is a name followed by a type. Types can be any primitive type, a scalar wrapper type, another struct, or an enum.

## `type` vs `data`

Both keywords produce identical struct types. `type` is the preferred spelling. `data` is retained as a supported alias for compatibility. You will see both in existing code — treat them as synonymous.

```fa
# Both of these are identical
type Point
    x real
    y real
done

data Point
    x real
    y real
done
```

## Field Types

Fields can reference any known type:

```fa
type Order
    id         uuid
    customer   User          # nested struct
    items      list          # list of items
    status     OrderStatus   # enum
    total      real
    created_at time
done
```

Nested struct fields trigger recursive validation: when an `Order` value is validated, its `customer` field is also validated as a `User`.

## Visibility: `open`

Types are private to their module by default. Add `open` before `type` or `data` to make the type accessible from other modules:

```fa
open type User
    id   uuid
    name text
done
```

Without `open`, the type can only be used within the same `.fa` file where it is declared. Other modules that import this module cannot reference the type by name.

## Required Docs

Every struct type must have a `docs` block. Every field in the struct must have a matching `docs field_name ... done` sub-block. Missing field docs is a compile error. An orphan field doc (documenting a field that does not exist in the type) is also a compile error.

```fa
docs EmailResult
    Contains the result of an email quality check.

    docs email
        The cleaned email address.
    done

    docs valid
        Whether the email passed validation.
    done
done

type EmailResult
    email text
    valid bool
done
```

## Constructing Struct Values

Struct values are constructed using `obj.*` ops, which work with the underlying `dict` representation:

```fa
func BuildUser
    take name as text
    emit result as User
    fail error as text
body
    id = random.uuid()
    user = obj.new()
    user = obj.set(user, "id", id)
    user = obj.set(user, "name", name)
    user = obj.set(user, "email", "")
    user = obj.set(user, "active", true)
    emit user
done
```

Because structs are represented as JSON objects at runtime, `obj.set` and `obj.get` are the standard construction and access operations.

## Accessing Fields

Field access uses dot notation:

```fa
func Greet
    take user as User
    emit result as text
    fail error as text
body
    name = user.name
    greeting = "Hello, #{name}!"
    emit greeting
done
```

Nested field access:

```fa
func OrderSummary
    take order as Order
    emit result as text
    fail error as text
body
    customer_name = order.customer.name
    total = order.total
    summary = "Order for #{customer_name}: $#{total}"
    emit summary
done
```

## Passing Structs Between Funcs

When a func emits a struct and another func takes that struct type, the types must match. The type name in `take` and `emit` declarations is checked at compile time:

```fa
# Validate.fa
func Validate
    take raw as dict
    emit result as User      # emits a User struct
    fail error as text
body
    # ... build and validate the user dict
    emit validated_user
done

# Save.fa
func Save
    take user as User        # takes a User struct
    emit result as bool
    fail error as text
body
    # ... save user to database
    emit true
done
```

In the flow:

```fa
flow SignUp
body
    step Validate(raw to :raw) then
        next :result to user
    done
    step Save(user to :user) then    # user is a User, Save takes User
        next :result to ok
    done
done
```

## Structs and JSON

Since structs are JSON objects at runtime, you can decode external JSON directly into a struct type using `json.decode`:

```fa
func ParseUser
    take body as text
    emit result as User
    fail error as text
body
    user = json.decode(body)
    # user is now a dict with the User fields
    # type validation happens at the emit boundary
    emit user
done
```

Encoding a struct to JSON:

```fa
func SerializeUser
    take user as User
    emit result as text
    fail error as text
body
    json_text = json.encode(user)
    emit json_text
done
```

## Structs in Test Blocks

Test blocks can construct struct values using `obj(...)` for simple cases:

```fa
docs SaveTest
    Verifies that Save succeeds for a valid user.
done

test SaveTest
    u = obj("id", "abc", "name", "Alice", "email", "alice@example.com", "active", true)
    r = Save(u)
    must r == true
done
```

## Built-in Struct Types

The standard library provides several struct types that are available without declaration. These are returned by stdlib ops:

| Type | Returned by | Fields |
|------|-------------|--------|
| `HttpRequest` | `http.server.accept` | `method`, `path`, `query`, `headers`, `body`, `conn_id` |
| `HttpResponse` | `http.get/post/put/delete` | `status`, `headers`, `body` |
| `Date` | `date.now`, `date.from_iso`, etc. | `unix_ms`, `tz_offset_min` |
| `Stamp` | `stamp.now`, `stamp.from_ns` | `ns` |
| `TimeRange` | `trange.new` | `start` (Date), `end` (Date) |
| `ProcessOutput` | `exec.run` | `code`, `stdout`, `stderr`, `ok` |
| `WebSocketMessage` | `ws.recv` | `type`, `data` |
| `ErrorObject` | `error.new` | `code`, `message`, `details` |
| `URLParts` | `url.parse` | `path`, `query`, `fragment` |

The compiler tracks these struct return types — passing a `Date` where a `TimeRange` is expected, or a `ProcessOutput` where a `db_conn` is expected, is a compile error.

Use them directly in `take`/`emit` declarations:

```fa
source Requests
    take port as long
    emit req as HttpRequest
    fail error as text
body
    srv = http.server.listen(port)
    on :request from http.server.accept(srv) to req
        emit req
    done
done
```

Defining a user type with the same name as a built-in type is a compile error.

## Rules and Gotchas

- Every user-defined struct type requires a `docs` block with sub-blocks for every field. This is a compile error, not a warning. Built-in struct types do not need docs.
- `type` and `data` are interchangeable. Prefer `type`.
- Types are private by default. Use `open type` to export across module boundaries.
- Struct values are `dict` at runtime. `obj.*` ops work on them directly.
- Field access with dot notation (`user.name`) is valid in func bodies and test assertions.
- Nested structs are validated recursively at the `take` and `emit` boundaries.
- There is no struct literal syntax. Build structs with `obj.new()` and `obj.set()`, or decode them from JSON.
