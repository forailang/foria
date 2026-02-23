# Mock

When a func calls another func from an imported module, tests need a way to control what that dependency returns — without actually executing it. `mock` replaces a specific callable with a fixed return value for the duration of one test block.

## Syntax

```fa
mock module.FuncName => value
```

`module.FuncName` identifies the callable to replace. `value` is the value it will return when called during this test. The `=>` separates the target from the replacement value.

Mock declarations must appear at the top of the test body, before any other statements. You cannot mock in the middle of a test.

## Basic Example

```fa
use session from "./session"

func Login
    take req as LoginRequest
    emit token as text
    fail error as text
body
    token = session.Issue(req.email)
    emit token
done

docs LoginReturnsToken
    Verify that a successful login emits the session token.
done

test LoginReturnsToken
    mock session.Issue => "tok_abc123"
    req = { email: "alice@example.com", password: "hunter2" }
    result = Login(req)
    must result == "tok_abc123"
done
```

When `Login` calls `session.Issue(req.email)`, the mock intercepts the call and returns `"tok_abc123"` instead of running the real `Issue` func. The test is isolated from the session module's implementation.

## What Values Can Mock Return

Mock value expressions are evaluated in an empty environment — no variables, no function calls. Only literal values are permitted:

**String literals:**

```fa
mock auth.GetRole => "admin"
mock storage.Read => "file contents here"
```

**Number literals:**

```fa
mock counter.Next => 42
mock price.Lookup => 9.99
```

**Boolean literals:**

```fa
mock feature.IsEnabled => true
mock cache.Has => false
```

**Dict literals (using literal keys and values):**

```fa
mock db.FindUser => { id: "u001", email: "alice@example.com", role: "admin" }
mock config.Load => { host: "localhost", port: "8080", debug: true }
```

**List literals:**

```fa
mock orders.List => ["order-1", "order-2", "order-3"]
```

**Nested dict and list literals:**

```fa
mock api.Fetch => {
    status: "ok",
    data: { count: 3, items: ["a", "b", "c"] }
}
```

## What Mock Values Cannot Contain

Mock values cannot contain:

- Variable references (`mock thing.Fn => myVariable` is an error)
- Function calls (`mock thing.Fn => str.upper("hello")` is an error)
- Interpolated strings (`mock thing.Fn => "hello #{name}"` is an error)
- Any expression that requires runtime evaluation

This restriction is intentional. Mock values are static substitutions, not computed ones. If you need a test where the dependency returns a complex computed value, compute it outside the test and embed the literal result in the mock.

## Multiple Mocks Per Test

A test block may contain multiple mock declarations, one per imported callable:

```fa
docs OrderPlacementSucceeds
    Verify that an order is placed when inventory and payment both succeed.
done

test OrderPlacementSucceeds
    mock inventory.Check => { available: true, qty: 10 }
    mock payment.Charge => { status: "approved", txId: "tx_9876" }
    mock notify.Send => true
    req = { item: "widget", qty: 1, card: "4242..." }
    result = PlaceOrder(req)
    must result.status == "confirmed"
    must result.txId == "tx_9876"
done
```

All mocks must appear before any non-mock statements. Placing a mock after an assignment or `must` is a compile error.

## Mocks Are Test-Scoped

A mock only applies within the test block that declares it. Other test blocks in the same file are not affected. The real callable is used everywhere except in tests that explicitly mock it.

```fa
test WithMock
    mock session.Issue => "tok_test"
    result = Login(valid_req)
    must result == "tok_test"   # uses the mock
done

test WithoutMock
    # session.Issue is not mocked here — runs the real implementation
    result = Login(valid_req)
    must str.len(result) > 0
done
```

## Mocking Directory Module Functions

For directory module imports (e.g., `use auth from "./auth"`), mock the specific callable using `module.FuncName`:

```fa
use auth from "./auth"

test CreateAccountWithValidData
    mock auth.Validate => true
    mock auth.Hash => "hashed_secret"
    req = { email: "bob@example.com", password: "letmein" }
    result = CreateAccount(req)
    must result.id != ""
done
```

The import alias (`auth`) is the module part of the mock target. The callable name (`Validate`, `Hash`) is the right side.

## When to Mock

Use mocks whenever a test would otherwise:

- Make a real network call
- Write to a real database
- Read from the filesystem
- Depend on the current time or a random value
- Call code whose behavior you want to control precisely

A test that uses mocks runs fast, deterministically, and in isolation. A test that calls real implementations depends on infrastructure being available and produces results that can vary.

Mock liberally. The goal is for every test in the suite to pass reliably on any machine, at any time, with no external dependencies required.
