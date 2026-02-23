# Must Assertions

The `must` statement is the primary assertion mechanism in forai tests. It evaluates a boolean expression and, if the result is false, immediately fails the test and reports the failure. All subsequent statements in the test body are skipped.

## Syntax

```fa
must expr
```

`expr` must be a boolean expression — any expression that evaluates to `true` or `false`. The most common form is a comparison:

```fa
must result == "expected"
must count > 0
must valid == true
must error != ""
```

## What You Can Assert

Any expression that produces a boolean value is valid. This includes:

**Equality and inequality:**

```fa
must name == "Alice"
must code != 404
must response != ""
```

**Comparison operators:**

```fa
must count >= 1
must score <= 100
must elapsed < 5000
must index > -1
```

**Boolean values directly:**

```fa
must is_valid
must !is_empty
must found == true
must str.contains(response, "success")
```

**Compound conditions with `&&` and `||`:**

```fa
must status == "ok" && body != ""
must code == 200 || code == 201
must str.len(token) > 0 && str.starts_with(token, "tok_")
```

**Negation:**

```fa
must !(result == "error")
```

## First Failure Stops the Test

When a `must` assertion fails, the test stops immediately. Statements after the failing `must` do not run:

```fa
test ProcessReturnsValidRecord
    record = Process("raw input")
    must record.id != ""       # if this fails, the next two lines do not run
    must record.name != ""
    must record.active == true
done
```

This is intentional. Once an assertion fails, later assertions may depend on state that is already wrong — running them would produce misleading additional failures. Fix the first failure, then run again.

## Failure Reporting

When a `must` fails, the test runner reports:

- The test name
- The docs text for the test
- The exact expression that failed, as written in source
- The resolved values of both sides (where they can be determined)

For example, if `must result == "expected"` fails when `result` is `"actual"`, the report shows:

```
FAIL  ProcessReturnsExpected
  Docs: Confirm that processing raw input returns the expected record name.
  Assertion failed: result == "expected"
    left:  "actual"
    right: "expected"
```

The expression text comes from the source; the resolved values come from the test runtime. This combination makes failures self-diagnosing — you rarely need to add extra logging to understand what went wrong.

## Asserting on Struct Fields

If the callable returns a struct, you can assert on individual fields:

```fa
test LoginReturnsToken
    mock session.Issue => "tok_abc123"
    req = { email: "alice@example.com", password: "hunter2" }
    result = Login(req)
    must result.token == "tok_abc123"
    must result.userId != ""
done
```

Access nested fields with chained dot notation:

```fa
must response.body.status == "ok"
must response.headers.content_type == "application/json"
```

## Asserting on List and String Properties

Use the standard library ops inside `must` expressions:

```fa
test ResponseBodyIsNonEmpty
    resp = Respond(data)
    must str.len(resp.body) > 0
    must str.contains(resp.body, "success")
    must str.starts_with(resp.content_type, "application/")
done
```

```fa
test ResultContainsAllItems
    items = ListOrders(user_id)
    must list.len(items) == 3
    must list.contains(items, expected_order)
done
```

## Multiple Assertions in One Test

A single test block may contain many `must` statements. Group related assertions in one test when they all verify the same behavior:

```fa
docs ProcessedRecordIsComplete
    Verify that all required fields are populated in the processed record.
done

test ProcessedRecordIsComplete
    record = Process("alice|30|admin")
    must record.name == "alice"
    must record.age == 30
    must record.role == "admin"
done
```

When assertions belong to different behaviors, split them into separate test blocks with separate names. This makes failures precise — a failing test named `ProcessedRecordHasCorrectRole` tells you immediately which behavior is broken.

## Must Is Not a Conditional

`must` is not `if`. It does not branch or continue. A failed `must` ends the test. Do not use `must` when you want to conditionally skip assertions — use multiple test blocks instead.

```fa
# Incorrect: trying to use must as a guard
test SomethingComplex
    result = DoThing()
    must result != ""
    # If the above passes, continue — but you cannot "if" on must
    must result.field == "expected"   # this runs only if the previous must passed
done
```

This is actually fine for sequential guards — each `must` acts as a precondition for the next. But if you want to express "if result is non-empty, then check field", split that into a separate test or use a single `must` that combines both conditions.
