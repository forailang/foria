# Trap Failures

Not all tests verify the happy path. Sometimes the most important behavior to test is that a callable correctly rejects bad input, detects invalid state, or propagates an error. The `trap` expression is forai's tool for testing failure paths.

## Syntax

```fa
err = trap FuncCall(args)
```

`trap` wraps a callable invocation and inverts the expectation: it expects the call to fail (to `fail` with an error), not to succeed. If the call fails, `trap` captures the error value in the named variable (`err` in the example above, but any name works). If the call succeeds — if it emits a value instead of failing — the test fails immediately.

## Basic Example

```fa
docs LoginRejectsEmptyPassword
    Confirm that an empty password causes login to fail.
done

test LoginRejectsEmptyPassword
    req = { email: "alice@example.com", password: "" }
    err = trap Login(req)
    must err == "password cannot be empty"
done
```

In this test:
1. `Login(req)` is called with an empty password.
2. `trap` expects `Login` to invoke `fail`. If it does, the error message is bound to `err`.
3. `must err == "password cannot be empty"` asserts the exact error text.

If `Login` emits a token instead of failing, the test fails at the `trap` line with a message like:

```
FAIL  LoginRejectsEmptyPassword
  Docs: Confirm that an empty password causes login to fail.
  trap expected a failure but call succeeded
    Call: Login(req)
```

## The Error Value

The variable bound by `trap` holds the error message — the value passed to `fail` by the called func. You can assert on it with `must`:

```fa
err = trap Register(duplicate_req)
must err == "email already registered"
```

Or check that an error occurred at all without caring about the exact message:

```fa
err = trap Validate(bad_input)
must err != ""
```

Or check that the error message contains a key phrase:

```fa
err = trap Parse(malformed)
must str.contains(err, "unexpected character")
```

## Using a Descriptive Error Variable Name

The variable name after `=` can be anything. Choose a name that makes the test readable:

```fa
# Short conventional name
err = trap Login(bad_creds)

# More descriptive
loginError = trap Login(bad_creds)
parseError = trap Parse(malformed_json)
validationError = trap Validate(empty_input)
```

The name has no semantic effect — it is just the binding for the captured error string.

## Combining Trap with Must

A common pattern is to capture the error and make multiple assertions about it:

```fa
docs RegisterRejectsTooShortPassword
    Verify that passwords shorter than 8 characters are rejected with
    a descriptive error message.
done

test RegisterRejectsTooShortPassword
    req = { email: "bob@example.com", password: "abc" }
    err = trap Register(req)
    must err != ""
    must str.contains(err, "password")
    must str.contains(err, "8")
done
```

First confirm that an error occurred, then inspect its content.

## Trap with Mocks

`trap` works alongside `mock`. Mock declarations must still appear at the top of the test body, before any statements including `trap`:

```fa
docs OrderFailsWhenPaymentDeclines
    Confirm that a declined payment causes order placement to fail.
done

test OrderFailsWhenPaymentDeclines
    mock payment.Charge => { status: "declined", code: "insufficient_funds" }
    req = { item: "widget", qty: 1, card: "4242..." }
    err = trap PlaceOrder(req)
    must str.contains(err, "payment declined")
done
```

The mock makes `payment.Charge` return a declined response, which triggers the failure path in `PlaceOrder`. `trap` captures the resulting error.

## Testing Multiple Failure Paths

Use separate test blocks for each failure path you want to cover:

```fa
docs LoginFailsOnBadEmail
    Verify that a malformed email address is rejected.
done

test LoginFailsOnBadEmail
    req = { email: "not-an-email", password: "hunter2" }
    err = trap Login(req)
    must str.contains(err, "invalid email")
done

docs LoginFailsOnBlankPassword
    Verify that a blank password is rejected.
done

test LoginFailsOnBlankPassword
    req = { email: "alice@example.com", password: "" }
    err = trap Login(req)
    must err == "password cannot be empty"
done

docs LoginFailsOnUnknownEmail
    Verify that an email address not in the system is rejected.
done

test LoginFailsOnUnknownEmail
    mock db.FindUser => { found: false }
    req = { email: "nobody@example.com", password: "hunter2" }
    err = trap Login(req)
    must str.contains(err, "not found")
done
```

Each test has a single clear responsibility. When one fails in CI, you know exactly which failure case is broken.

## Trap Does Not Catch Panics

`trap` only captures errors explicitly emitted via `fail` in the called func. It does not catch unexpected runtime panics or internal errors. If a callable crashes unexpectedly, `trap` does not help — that is a bug, not an expected failure path. Tests verify the behavior you designed; unexpected crashes are surfaced as test errors, not as captured values.
