# Scalar Types

A scalar type is a named alias of a primitive type with optional validation constraints. Use scalar types to give semantic meaning to primitive values and to enforce domain rules at the type boundary.

## Declaring a Scalar Type

Use `type Name as PrimitiveType` followed by optional constraints:

```fa
type Email as text, :matches => /@/
type Username as text, :min => 3, :max => 32
type Score as real, :min => 0.0, :max => 100.0
type PositiveInt as long, :min => 1
```

The primitive type after `as` is the base type. Constraints appear after the base type, separated by commas. Each constraint is a `:key => value` pair.

## Purpose of Scalar Types

Without scalar types, a func that takes an email address looks like this:

```fa
func SendEmail
    take address as text    # what kind of text? any string is accepted
    ...
done
```

With a scalar type, the contract is explicit and enforced:

```fa
type Email as text, :matches => /@/

func SendEmail
    take address as Email   # must be a valid email address
    ...
done
```

When `SendEmail` is called with a value for `address`, the runtime validates it against the `Email` constraints before the body runs. If the value does not contain `@`, the call fails on the failure track.

This validation happens at every `take` and `emit` boundary where the scalar type appears. Define the constraint once; enforce it everywhere.

## Constraints

Scalar types support these constraints:

| Constraint | Applies to | Value type | Description |
|------------|-----------|------------|-------------|
| `:matches` | `text` | Regex | Value must match the regex pattern |
| `:min` | `long`, `real` | Number | Minimum numeric value (or minimum character count for `text`) |
| `:max` | `long`, `real` | Number | Maximum numeric value (or maximum character count for `text`) |
| `:required` | struct fields | Boolean | Field must be present (used in struct declarations, not scalars) |
| `:map` | `text` | Symbol | Reserved for future transforms (e.g., `:map => :lowercase`) |

### `:matches` — Regex Validation

```fa
type Email as text, :matches => /@/
type PhoneNumber as text, :matches => /^\+?[0-9]{10,15}$/
type HexColor as text, :matches => /^#[0-9a-fA-F]{6}$/
type Slug as text, :matches => /^[a-z0-9-]+$/
```

The regex is a standard regex literal (`/.../`). The value must match the pattern. For full-string matching, anchor with `^` and `$`.

### `:min` and `:max` — Range Validation

For numeric types:

```fa
type Percentage as real, :min => 0.0, :max => 100.0
type Age as long, :min => 0, :max => 150
type Temperature as real, :min => -273.15
```

For `text`, `:min` and `:max` constrain the character count:

```fa
type Password as text, :min => 8, :max => 128
type Name as text, :min => 1, :max => 100
```

### `:map` — Value Transforms

`:map` is reserved for automatic value transforms applied after validation. The primary use case is `:map => :lowercase`, which normalizes text to lowercase:

```fa
type Email as text, :matches => /@/, :map => :lowercase
```

With this declaration, any `Email` value is automatically lowercased after validation. `"Alice@Example.COM"` becomes `"alice@example.com"` at the take boundary. This eliminates a class of bugs where two representations of the same email address are treated as different values.

## Using Scalar Types in Structs

Scalar types can be used as field types in struct declarations:

```fa
type Email as text, :matches => /@/
type Username as text, :min => 3, :max => 32

docs Account
    A user account with validated credentials.

    docs username
        The account username, 3 to 32 characters.
    done

    docs email
        The account email address.
    done
done

type Account
    username Username
    email    Email
done
```

When an `Account` value is validated, each field is validated against its scalar type constraints. An `Account` with an invalid email fails validation before it enters any func body.

## Scalar Types vs Struct Types

Both scalar types and struct types are named types in forai. The difference:

- A scalar type wraps a single primitive value with constraints. `Email` is still fundamentally a `text`.
- A struct type groups multiple fields. `User` is a composite of `uuid`, `text`, `Email`, etc.

Scalar types do not add fields. They add validation rules. A func that takes `Email` receives a single string value — just validated more strictly than plain `text`.

## Visibility

Like struct types, scalar types are private by default. Use `open` to export them:

```fa
open type Email as text, :matches => /@/
```

Other modules can then use `Email` as a type in their `take` and `emit` declarations.

## A Complete Example

```fa
open type Email as text, :matches => /@/, :map => :lowercase
open type Password as text, :min => 8, :max => 128
open type Username as text, :min => 3, :max => 32

docs Credentials
    Login credentials submitted by a user.

    docs username
        The account username.
    done

    docs password
        The plaintext password (before hashing).
    done
done

type Credentials
    username Username
    password Password
done

docs ValidateCredentials
    Validates login credentials and returns the authenticated user.
done

func ValidateCredentials
    take creds as Credentials
    emit result as User
    fail error as text
body
    # creds.username is guaranteed to be 3-32 chars (validated at take boundary)
    # creds.password is guaranteed to be 8-128 chars (validated at take boundary)
    user = db.query_user_by_email(conn, creds.username)
    is_valid = crypto.verify_password(creds.password, user.password_hash)
    if is_valid
        emit user
    else
        fail "invalid credentials"
    done
done

test ValidateCredentials
    mock db.query_user_by_email => sample_user()
    mock crypto.verify_password => true
    creds = obj("username", "alice", "password", "secretpassword")
    r = ValidateCredentials(creds)
    must r.name == "Alice"
done
```

## Rules and Gotchas

- Scalar types are validated at `take` and `emit` boundaries, not at the point of construction. A value is only validated when it passes through a typed port.
- `:matches` uses regex literal syntax (`/.../`). Standard regex rules apply. For full-string matching, use anchors `^` and `$`.
- For `text`, `:min` and `:max` constrain character count, not byte count. Unicode-aware.
- `:map => :lowercase` is the only currently implemented transform. `:map => :uppercase` and others are reserved for future implementation.
- Scalar types are private by default. Use `open` to share across modules.
- There is no operator to "unwrap" a scalar type to its base primitive. The value is already the base primitive — `Email` values flow as strings.
- Constraints are documented in `docs` blocks for struct fields, but scalar type constraints are self-documenting through their declaration syntax.
