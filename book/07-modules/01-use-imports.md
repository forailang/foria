# Use Imports

forai programs are structured as collections of `.fa` files. Each file contains exactly one callable — a func, flow, sink, or source. To use one file's callable from another, you write a `use` declaration at the top of the importing file.

## Basic Syntax

The `use` declaration has a single form:

```fa
use Name from "./path"
```

`Name` is the local alias you will use to call into the imported module. `"./path"` is a path relative to the current file's directory — not the project root.

After a `use` declaration, you access the imported callable or module's functions through the alias using dot notation:

```fa
use Round from "./round"
use auth from "./auth"
```

These two imports look similar but behave differently based on what the path points to. If the path resolves to a directory, you get a namespace. If it resolves to a `.fa` file, you get a direct reference to that file's callable.

## Directory Imports

When the path points to a directory, `use` imports the entire module namespace. You call individual callables inside that directory by name, prefixed with the alias:

```fa
use auth from "./auth"

flow Main
body
    step auth.Login
    step auth.Logout
done
```

Here, `"./auth"` is a directory. Inside `./auth/`, there are files like `Login.fa` and `Logout.fa`. The `auth` alias becomes a namespace prefix, and `auth.Login` resolves to `./auth/Login.fa`.

This pattern is the standard way to group related callables under a shared namespace. A typical module directory contains multiple files, each named after the callable it exports:

```
app/
  auth/
    Login.fa
    Logout.fa
    Register.fa
  Start.fa
```

In `Start.fa`:

```fa
use auth from "./auth"

flow Start
body
    step auth.Register
    step auth.Login
done
```

## File Imports

When the path points to a `.fa` file directly (including the `.fa` extension), the alias refers to that file's callable. You call it directly using the alias as the function name — no additional dot navigation:

```fa
use Round from "./round.fa"

flow Game
body
    step Round
    step Round
    step Round
done
```

Here `Round` is used exactly as if it were defined locally. The `"./round.fa"` path must point to a file that contains a callable named `Round` — the name must match the file stem.

## Side-by-Side Comparison

```fa
# Directory import — alias becomes a namespace
use validators from "./validators"
result = validators.CheckEmail(input)

# File import — alias is called directly
use Normalize from "./Normalize.fa"
clean = Normalize(raw)
```

Directory imports are appropriate when you have a cohesive group of callables (an auth module, a routing module, a storage module). File imports are appropriate when you need one specific callable from a peer file in the same directory level.

## Calling Imported Symbols

Calling an imported callable uses the same syntax as any other call:

```fa
use math from "./math"
use Round from "./round.fa"

func Process
    take input as text
    emit result as text
    fail error as text
body
    validated = math.Validate(input)
    rounded = Round(validated)
    emit rounded
done
```

Arguments, return values, and error handling all work identically whether the callee is defined in the same file, imported from a peer file, or imported from a directory module.

## Where Use Declarations Live

`use` declarations appear at the top of a `.fa` file, before any `type`, `enum`, `docs`, `test`, or callable declarations. A file may have multiple `use` declarations:

```fa
use http from "./http"
use db from "./db"
use auth from "./auth"
use Config from "./Config.fa"

func HandleRequest
    take req as Request
    emit resp as Response
    fail error as text
body
    cfg = Config()
    session = auth.Verify(req)
    data = db.Query(session)
    resp = http.Respond(data)
    emit resp
done
```

## Aliases and Name Conflicts

The alias you choose in the `use` declaration is entirely local. Two files can import the same module under different aliases without conflict. Aliases must not collide with each other or with local variable names in the same scope. The compiler catches alias collisions and reports them as an error.

```fa
# This is an error — two imports share the same alias
use auth from "./auth"
use auth from "./legacy_auth"  # error: duplicate import alias 'auth'
```

Choose aliases that are short, descriptive, and unambiguous within the file.
