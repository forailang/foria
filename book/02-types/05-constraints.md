# Constraints

Constraints are validation rules attached to type declarations. They define what values are considered valid for a type. forai evaluates constraints at `take` and `emit` boundaries — before the body runs (for `take`) and after the body runs (for `emit`).

## Where Constraints Appear

Constraints appear in two places:

1. **Scalar type declarations** — on the type itself:

```fa
type Email as text, :matches => /@/
type Score as real, :min => 0.0, :max => 100.0
```

2. **Struct field declarations** — on individual fields:

```fa
type User
    id    uuid :required => true
    name  text :min => 1, :max => 100
    email text :matches => /@/
    age   long :min => 0, :max => 150
done
```

## Constraint Syntax

Each constraint is a colon-prefixed key, optionally followed by `=> value`:

```fa
:required           # boolean flag, implies :required => true
:required => true   # explicit boolean
:required => false  # explicitly not required
:min => 0           # number value
:max => 100         # number value
:matches => /@/     # regex literal value
:map => :lowercase  # symbol value
```

Multiple constraints on one declaration are comma-separated:

```fa
type Password as text, :min => 8, :max => 128
type Email as text, :matches => /@/, :map => :lowercase

type User
    username text :min => 3, :max => 32
    email    text :matches => /@/
done
```

## The Constraints

### `:matches` — Pattern Validation

Validates that a `text` value matches a regex pattern.

```fa
type Email as text, :matches => /@/
type PhoneUS as text, :matches => /^[0-9]{3}-[0-9]{3}-[0-9]{4}$/
type HexColor as text, :matches => /^#[0-9a-fA-F]{6}$/
type IsoDate as text, :matches => /^\d{4}-\d{2}-\d{2}$/
```

The regex uses standard regex syntax. The `/` delimiters are part of the regex literal syntax — do not include them in the pattern itself. For full-string matching, use `^` and `$` anchors. Without anchors, the pattern matches if found anywhere in the string:

```fa
type ContainsAt as text, :matches => /@/       # passes if "@" appears anywhere
type StartsWithHttp as text, :matches => /^https?:\/\//   # must start with http:// or https://
```

Note: `{` and `}` inside regex patterns are literal — they do not trigger forai's string interpolation (which uses `#{}`).

### `:min` — Minimum Value or Length

For `long` and `real`: validates that the numeric value is greater than or equal to the minimum.

```fa
type Age as long, :min => 0         # no negative ages
type Temperature as real, :min => -273.15   # above absolute zero
type Percentage as real, :min => 0.0, :max => 100.0
```

For `text`: validates that the character count is at least the minimum.

```fa
type Username as text, :min => 3    # at least 3 characters
type Password as text, :min => 8    # at least 8 characters
```

### `:max` — Maximum Value or Length

For `long` and `real`: validates that the numeric value is less than or equal to the maximum.

```fa
type Score as long, :max => 100
type Latitude as real, :min => -90.0, :max => 90.0
```

For `text`: validates that the character count is at most the maximum.

```fa
type ShortText as text, :max => 280    # tweet-length limit
type Name as text, :min => 1, :max => 100
```

### `:required` — Field Presence

`:required` applies to struct fields (not scalar types). It validates that the field is present in the struct value. Without `:required`, a field may be absent (null) in a struct value without causing a validation error.

```fa
type Config
    host text :required => true      # must be present
    port long :required => true      # must be present
    debug bool                       # optional — may be absent
done
```

`:required` used as a bare flag (without `=> true`) defaults to true:

```fa
type Config
    host text :required    # same as :required => true
    port long :required
done
```

### `:map` — Value Transform

`:map` applies a transform to the value after validation. Currently the only supported transform is `:lowercase`:

```fa
type Email as text, :matches => /@/, :map => :lowercase
```

With `:map => :lowercase`, the value is lowercased after the regex match passes. A caller who passes `"Alice@EXAMPLE.COM"` will see `"alice@example.com"` inside the func body — the transform happens at the `take` boundary.

This is useful for normalizing text values at the boundary rather than writing `str.lower(email)` in every func body that handles emails.

## Validation Boundaries

Constraints are checked at two points:

1. **`take` validation** — when a value enters a func or sink body. If the value fails any constraint, the call fails on the `fail` track before any body statement runs. The caller receives the failure.

2. **`emit` validation** — when a value exits a func or sink body. If the value fails any constraint, it is a hard error (a bug in the func, not bad input). Output validation failure indicates that the func produced a value that violates its own contract.

The practical consequence: you can trust the value inside a func body. If the func declared `take email as Email` and the body is running, then `email` is already a valid email string. No additional checks needed.

## Combining Constraints

Constraints compose freely. A field can have multiple constraints:

```fa
type Product
    sku      text :required, :min => 3, :max => 20, :matches => /^[A-Z0-9-]+$/
    name     text :required, :min => 1, :max => 200
    price    real :required, :min => 0.01
    quantity long :required, :min => 0
done
```

All constraints must pass for validation to succeed. If any constraint fails, validation fails.

## A Complete Validated Type Example

```fa
open type Email as text, :matches => /@/, :map => :lowercase
open type Password as text, :min => 8, :max => 256

docs RegistrationRequest
    A request to register a new user account.

    docs email
        The desired email address. Must contain "@". Normalized to lowercase.
    done

    docs password
        The account password. Must be at least 8 characters.
    done

    docs display_name
        The user's chosen display name.
    done
done

type RegistrationRequest
    email        Email    :required
    password     Password :required
    display_name text     :required, :min => 2, :max => 50
done

docs Register
    Registers a new user account.
done

func Register
    take req as RegistrationRequest
    emit result as User
    fail error as text
body
    # At this point:
    # - req.email is validated as Email (contains @, lowercased)
    # - req.password is at least 8 chars
    # - req.display_name is 2-50 chars
    # No defensive checks needed

    existing = db.query_user_by_email(conn, req.email)
    if obj.has(existing, "id")
        fail "email already registered"
    done

    id = random.uuid()
    hash = crypto.hash_password(req.password)
    user = obj.new()
    user = obj.set(user, "id", id)
    user = obj.set(user, "name", req.display_name)
    user = obj.set(user, "email", req.email)
    user = obj.set(user, "password_hash", hash)
    emit user
done
```

## Rules and Gotchas

- `:matches` is for `text` only. Applying it to `long` or `real` is a type error.
- `:min` and `:max` on `text` count Unicode characters (code points), not bytes.
- `:required` is for struct fields only, not scalar type declarations.
- `:map` transforms happen after validation. If the value fails `:matches`, the transform never runs.
- Output validation (at `emit`) failure is a hard program error. Input validation (at `take`) failure is propagated as a normal failure on the `fail` track.
- Constraints are purely additive. A type can have as many constraints as needed — all must pass.
