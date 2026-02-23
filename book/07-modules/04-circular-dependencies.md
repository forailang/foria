# Circular Dependencies

A circular dependency occurs when module A imports module B, and module B (directly or transitively) imports module A. forai detects this at compile time and rejects it as an error. The compiler traces the full import chain and reports the cycle so you can break it.

## Why Circles Are Rejected

forai compiles modules in dependency order: if A depends on B, B must be compiled before A. A circular dependency makes this ordering impossible — there is no valid compilation sequence. Beyond the technical problem, circular dependencies are usually a symptom of a design issue: two modules that depend on each other are probably doing too much, or sharing something that should be extracted into a third module.

## What an Error Looks Like

If `auth/Login.fa` imports from `session/`, and `session/Issue.fa` imports from `auth/`, the compiler will report something like:

```
error: circular dependency detected
  app/auth/Login.fa
    → app/session/Issue.fa
      → app/auth/Login.fa (cycle)
```

The full chain is shown so you can see exactly where the loop closes.

## Example: A Circular Pair

Here is a concrete example that triggers the error:

```fa
# app/auth/Login.fa
use session from "../session"

func Login
    take creds as Credentials
    emit token as text
    fail error as text
body
    token = session.Issue(creds.userId)
    emit token
done
```

```fa
# app/session/Issue.fa
use auth from "../auth"   # error: this creates a cycle

func Issue
    take userId as text
    emit token as text
    fail error as text
body
    valid = auth.Validate(userId)   # circular!
    emit token
done
```

`Login` depends on `session.Issue`, and `session.Issue` depends on `auth.Validate` (which is in the same `auth/` directory as `Login`). This closes the cycle.

## Strategy 1: Extract a Shared Module

The most common fix is to extract the shared concern into a third module that both sides import. Neither side imports the other.

```
app/
  auth/
    Login.fa      # imports session and shared/user
    Validate.fa   # imports shared/user only
  session/
    Issue.fa      # imports shared/user only
  shared/
    user/
      Find.fa     # no imports from auth or session
      Verify.fa
```

```fa
# app/session/Issue.fa — after extracting shared logic
use userStore from "../shared/user"

func Issue
    take userId as text
    emit token as text
    fail error as text
body
    user = userStore.Find(userId)
    emit token
done
```

```fa
# app/auth/Login.fa — after extracting shared logic
use session from "../session"
use userStore from "../shared/user"

func Login
    take creds as Credentials
    emit token as text
    fail error as text
body
    user = userStore.Find(creds.email)
    token = session.Issue(user.id)
    emit token
done
```

Now `auth/Login.fa` and `session/Issue.fa` both import from `shared/user/` — no cycle.

## Strategy 2: Invert the Dependency

Sometimes the cycle exists because one module is calling back into the thing that started it. This often signals an inversion-of-control opportunity. Instead of A calling into B which calls back into A, restructure so that A calls B, and B returns enough data that A can continue without a callback.

Before (circular):

```fa
# order/Place.fa imports notify, notify/Send.fa imports order to fetch details
```

After (no cycle):

```fa
# order/Place.fa fetches what it needs, passes details to notify as arguments
# notify/Send.fa takes full order details as input, has no need to import order/
```

This is the dataflow-native approach: pass data through function arguments rather than letting called functions reach back into the caller's module.

## Strategy 3: Merge Small Modules

If two files are genuinely inseparable — each does half of one logical thing — consider whether they should be one file. If merging them violates the one-callable rule (because each has its own func), that is a signal to redesign the interface so one func can subsume the other's role, or to call them in a linear chain instead of in a mutual-call pattern.

## Transitive Cycles Are Also Detected

forai detects not just direct A→B→A cycles, but transitive ones of any length:

```
A imports B
B imports C
C imports D
D imports A   ← cycle detected here
```

The compiler walks the entire import graph depth-first and reports the chain whenever a node is visited that is already on the current path.

## No Workarounds at Runtime

There is no way to break a cycle with lazy loading, runtime requires, or deferred evaluation. All imports are compile-time. If the compiler reports a cycle, the code structure must change. This is intentional — forai does not allow designs that make the dependency graph ambiguous or unresolvable.

## Checking Your Dependency Graph

If you are working on a large module tree and want to verify there are no cycles before they cause a build failure, run:

```
forai build app/Start.fa
```

The compiler walks all transitive imports from the entry point and reports any cycles it finds, along with the full chain for each one.
