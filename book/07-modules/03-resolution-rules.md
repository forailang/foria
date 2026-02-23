# Module Resolution Rules

When the forai compiler encounters a `use` declaration, it must locate the file or directory the path refers to. The resolution process follows a small, deterministic set of rules. Understanding them prevents the most common import-related confusion.

## Paths Are Always Relative to the Importing File

This is the rule that trips up most newcomers. The path in a `use` declaration is **relative to the directory of the file that contains the import** — not the project root, not the current working directory of the shell, not the location of the main entry point.

Given this project structure:

```
project/
  main.fa          # entry point
  app/
    Start.fa
    round/
      Play.fa
      Score.fa
```

If `Start.fa` imports `Play.fa`:

```fa
# In app/Start.fa
use round from "./round"
```

The path `"./round"` is resolved relative to `app/`, so it looks for `app/round/`. This is correct.

If `main.fa` were to import the same module:

```fa
# In main.fa (project root)
use round from "./round"   # looks for project/round/ — does not exist!
use round from "./app/round"  # correct for main.fa's location
```

The path must always be written from the perspective of the file containing the `use`.

## Directory Import Resolution

When the path in a `use` declaration refers to a directory (no `.fa` extension), the compiler:

1. Resolves the path relative to the importing file's directory
2. Verifies that a directory exists at that location
3. Makes all `.fa` files in that directory accessible under the alias

```fa
# app/Start.fa
use auth from "./auth"      # → app/auth/ directory
use db from "../shared/db"  # → shared/db/ directory (one level up, then into shared/)
```

Calls through a directory alias use dot notation:

```fa
result = auth.Login(creds)
saved = db.Save(record)
```

Each `auth.Something` call resolves to `app/auth/Something.fa`. The file must exist — calling `auth.Nonexistent` is a compile-time error.

## File Import Resolution

When the path ends with `.fa`, the compiler:

1. Resolves the path relative to the importing file's directory
2. Verifies that a file exists at exactly that path
3. Checks that the callable inside matches the file's stem

```fa
# app/Start.fa
use Round from "./round.fa"    # → app/round.fa, callable must be named Round
use Config from "../Config.fa" # → project/Config.fa, callable must be named Config
```

File imports are called directly using the alias:

```fa
cfg = Config()
outcome = Round(state)
```

Note: `use Round from "./round.fa"` and `use round from "./round"` look similar but are different things. The first imports a single callable named `Round` from `round.fa`. The second imports all callables in the `round/` directory under the `round` namespace.

## Practical Module Tree Examples

### Example 1: Flat Application

```
app/
  Main.fa
  Validate.fa
  Process.fa
  Respond.fa
```

```fa
# app/Main.fa
use Validate from "./Validate.fa"
use Process from "./Process.fa"
use Respond from "./Respond.fa"

flow Main
body
    step Validate
    step Process
    step Respond
done
```

All files are peers. Each is imported by filename. This works well for small applications where there is no natural grouping.

### Example 2: Feature Module

```
app/
  Start.fa
  auth/
    Login.fa
    Register.fa
    Refresh.fa
  orders/
    Create.fa
    List.fa
    Cancel.fa
```

```fa
# app/Start.fa
use auth from "./auth"
use orders from "./orders"

flow Start
body
    step auth.Login
    step orders.Create
done
```

Each feature lives in its own directory. The top-level flow wires them together.

### Example 3: Nested Modules

```
app/
  Start.fa
  round/
    Play.fa
    check/
      IsValid.fa
      IsComplete.fa
```

```fa
# app/Start.fa
use round from "./round"

flow Start
body
    step round.Play
done
```

```fa
# app/round/Play.fa
use check from "./check"

func Play
    take state as GameState
    emit result as GameState
    fail error as text
body
    valid = check.IsValid(state)
    done_val = check.IsComplete(state)
    emit state
done
```

`Start.fa` imports `round/`. `Play.fa` imports `check/` — relative to `Play.fa`'s own directory, which is `app/round/`. Each file manages its own immediate dependencies.

### Example 4: Shared Utilities

```
project/
  app/
    Start.fa
    auth/
      Login.fa
  shared/
    types/
      User.fa
    util/
      Hash.fa
```

```fa
# app/auth/Login.fa
use Hash from "../../shared/util/Hash.fa"

func Login
    take creds as Credentials
    emit result as User
    fail error as text
body
    hashed = Hash(creds.password)
    emit result
done
```

The `../../` path navigates up two directories (from `app/auth/` to `project/`) and then down into `shared/util/`. Relative paths can traverse upward freely. There is no requirement that imports stay within the same "feature" directory.

## What the Compiler Checks

At compile time, for every `use` declaration:

- The path must resolve to a real file or directory
- If a directory: every callable referenced through the alias must have a matching `.fa` file
- If a file: the callable inside must have a name matching the file stem

If any of these checks fail, the compiler produces a clear error naming the unresolvable path or mismatched name.

## No Dynamic Imports

forai has no runtime module loading. All `use` declarations are resolved at compile time. The full dependency graph is known before any code executes. This means there are no "module not found" errors at runtime — only compile-time errors.
