# File and Directory Modules

forai's module system is built around two complementary structures: individual `.fa` files that export a single callable, and directories that act as namespaced collections of those files. Understanding this structure — and its rules — is essential for organizing any non-trivial forai project.

## The One-Callable Rule

Every `.fa` file must contain exactly one callable declaration. A callable is any `func`, `flow`, `sink`, or `source`. You cannot declare two funcs in the same file. This is not a style preference; it is a hard compiler rule.

The callable's name must match the file's stem — the filename without the `.fa` extension, and the casing must match exactly:

| File | Required callable name |
|------|------------------------|
| `Validate.fa` | `func Validate` or `flow Validate` or `sink Validate` |
| `Login.fa` | `func Login` |
| `Start.fa` | `flow Start` |
| `httpListener.fa` | `source httpListener` |

If the name inside the file does not match the filename stem, the compiler rejects it:

```fa
# File: Login.fa — this is a compile error
func Authenticate   # error: callable name must be 'Login' to match filename
    take ...
```

This rule makes navigation mechanical. To find the implementation of `auth.Register`, you look in the `auth/` directory for `Register.fa`. No grep, no IDE index required.

## What Else Can Appear in a File

Although only one callable is allowed per file, a `.fa` file is not restricted to just that callable. The following declarations may also appear:

- **`use` declarations** — to import other modules this file depends on
- **`type` declarations** — struct types used by the callable
- **`enum` declarations** — enum types used by the callable
- **`docs` blocks** — documentation for the callable and its types
- **`test` blocks** — tests for the callable

A typical file with supporting declarations:

```fa
use db from "./db"
use session from "./session"

type LoginRequest
    email as text
    password as text
done

type LoginResult
    userId as text
    token as text
done

docs LoginRequest
    Credentials submitted by a user attempting to log in.

    docs email
        The user's email address.
    done

    docs password
        The plaintext password before hashing.
    done
done

docs LoginResult
    Successful authentication response containing the session token.

    docs userId
        The authenticated user's unique identifier.
    done

    docs token
        A signed session token valid for 24 hours.
    done
done

docs Login
    Authenticate a user and issue a session token.
done

func Login
    take req as LoginRequest
    emit result as LoginResult
    fail error as text
body
    user = db.FindUser(req.email)
    valid = session.Verify(user, req.password)
    token = session.Issue(user.id)
    emit LoginResult { userId: user.id, token: token }
done

test LoginRejectsEmptyPassword
    must false == true  # placeholder: real test shown in testing chapter
done
```

This file contains one callable (`Login`), two types, docs for all three, one test, and two imports. Everything is in service of that single callable.

## Directory Modules

A directory becomes a module when another file imports it with `use Name from "./dirname"`. The directory name becomes the namespace, and every `.fa` file in that directory is reachable under that namespace.

Consider this project layout:

```
app/
  Start.fa
  auth/
    Login.fa
    Logout.fa
    Register.fa
    Verify.fa
  storage/
    Save.fa
    Load.fa
    Delete.fa
```

In `Start.fa`:

```fa
use auth from "./auth"
use storage from "./storage"

flow Start
body
    step auth.Register
    step auth.Login
    step storage.Save
done
```

The `auth` alias gives access to `Login`, `Logout`, `Register`, and `Verify` — one per file. Similarly for `storage`. You cannot call `auth.SomeUndeclaredThing` — only files that exist in the directory are valid.

## Nesting Directories

Directories may be nested arbitrarily deep. Each level adds a component to the path:

```
app/
  Start.fa
  round/
    Play.fa
    Score.fa
    check/
      IsValid.fa
      IsFinal.fa
```

If `Play.fa` needs access to the `check/` sub-module:

```fa
# In app/round/Play.fa
use check from "./check"

func Play
    take input as text
    emit result as text
    fail error as text
body
    valid = check.IsValid(input)
    final = check.IsFinal(input)
    emit result
done
```

Each level of nesting requires its own `use` declaration in the file that needs it. There is no automatic transitive import — importing `./round` does not make `./round/check` available.

## Visibility

All funcs, flows, sinks, and sources are always public. There is no `private` callable. Any file that imports a directory module can call any callable in that directory.

Types and enums are private by default — they are only visible within the file that declares them. To make a type visible to importing files, add the `open` modifier:

```fa
open type UserRecord
    id as text
    email as text
    role as text
done
```

An `open` type can be used as a parameter or return type across module boundaries. A non-`open` type is only usable within its declaring file.

## Module Boundaries and Testing

Each `.fa` file is an independent test target. When you run `forai test ./auth/Login.fa`, only the tests in `Login.fa` run. Tests in `Logout.fa` or `Register.fa` are not affected. This means you can run the full test suite for one callable without touching the rest of the module.

When testing a callable that depends on directory imports, use `mock` to substitute those dependencies. See [Chapter 9: Testing](../09-testing/04-mock.md) for details on mock syntax.
