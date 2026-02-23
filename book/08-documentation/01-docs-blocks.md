# Docs Blocks

forai treats documentation as a first-class language construct. Every callable and every test in a `.fa` file must have a `docs` block. This is not a linter suggestion — it is a hard compiler error. Code that compiles is code that is documented.

## Syntax

A `docs` block begins with `docs Name` and ends with `done`. The body is free-form text:

```fa
docs Login
    Authenticate a user with email and password.
    Returns a signed session token on success.
    Fails with a human-readable message if credentials are invalid.
done
```

The `Name` in `docs Name` must match the name of a callable or test in the same file. The text between `docs Name` and `done` is captured verbatim, with leading indentation trimmed. Blank lines within the block are preserved.

## What Requires Docs

Every one of the following declarations requires a `docs` block:

- `func`
- `flow`
- `sink`
- `source`
- `type`
- `test`

```fa
docs Validate
    Check that the input text is non-empty and within the allowed length.
    Returns true if valid, false otherwise.
done

func Validate
    take input as text
    emit result as bool
    fail error as text
body
    ok = str.len(input) > 0 && str.len(input) <= 500
    emit ok
done
```

## What Does Not Require Docs

`enum` declarations and `use` declarations do not require docs blocks. Enums are considered self-documenting through their variant names; use declarations are structural bookkeeping. Everything else must be documented.

```fa
enum Status
    Active
    Inactive
    Suspended
done
# No docs block required for Status
```

## Placement

Docs blocks may appear before or after the declaration they document. The compiler matches them by name, not by position. Both of these are valid:

```fa
# Docs before the func (common style)
docs Process
    Transform raw input into a normalized record.
done

func Process
    take raw as text
    emit record as Record
    fail error as text
body
    emit record
done
```

```fa
# Docs after the func (also valid)
func Process
    take raw as text
    emit record as Record
    fail error as text
body
    emit record
done

docs Process
    Transform raw input into a normalized record.
done
```

Most forai code places docs blocks before the declaration they document, but this is a convention, not a rule.

## Orphan Docs

An orphan docs block is one whose name does not match any declaration in the same file. The compiler rejects orphan docs:

```fa
docs OldFunction
    This function no longer exists.
done

# There is no func, flow, sink, source, type, or test named OldFunction
# Compiler error: orphan docs block 'OldFunction'
```

This prevents stale documentation. If you rename a callable, you must rename its docs block too.

## Duplicate Docs

A file may not contain two `docs` blocks with the same name. The compiler rejects duplicates:

```fa
docs Login
    First description.
done

docs Login
    Second description.   # error: duplicate docs block 'Login'
done
```

## Multi-Paragraph Docs

Docs blocks support multi-paragraph text. Blank lines within the block create paragraph breaks:

```fa
docs Register
    Create a new user account with the supplied credentials.

    The email address must be unique across all accounts. If an account
    with the same email already exists, the func fails with a conflict error.

    The password is hashed before storage using bcrypt with a cost factor
    of 12. The plaintext password is never persisted.
done
```

## Docs for Types

Types require their own docs block, just like funcs. If the type has fields, each field also requires a field docs block nested inside the type's docs. See [Field Docs](02-field-docs.md) for the full syntax.

A type with no fields only needs a top-level docs block:

```fa
type SessionToken
    value as text
done

docs SessionToken
    An opaque signed token issued after successful authentication.

    docs value
        The base64-encoded token string. Treat this as a secret.
    done
done
```

## Docs for Tests

Test blocks require docs just like funcs:

```fa
docs ValidateRejectsEmpty
    Confirm that empty strings are rejected by the validation func.
done

test ValidateRejectsEmpty
    result = Validate("")
    must result == false
done
```

## Why Hard Enforcement

The strict requirement exists because documentation is one of the first things consulted when reading unfamiliar code. Making it optional means it gets written only when convenient — which means it often does not get written at all. By making the absence of docs a compile error on par with a type mismatch, forai ensures that every public interface is described before it can be shipped.

The enforcement also enables the `forai doc` command (see [forai doc command](03-forai-doc-command.md)): because docs are guaranteed to exist for every callable, doc generation never produces incomplete output.
