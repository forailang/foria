# Test Blocks

forai has a built-in test system. Tests live in the same `.fa` file as the callable they test, written as `test` blocks alongside the implementation. Running `forai test` executes them directly against the compiled code — no test framework, no test runner configuration, no separate test files.

## Syntax

A test block begins with `test Name` and ends with `done`:

```fa
test ValidateAcceptsShortInput
    result = Validate("hello")
    must result == true
done
```

The name must be a valid identifier (no spaces). Use underscores or camelCase to make names readable:

```fa
test LoginRejectsEmptyPassword
test ParseHandlesMalformedInput
test RoundAdvancesScore
```

## Placement in a File

Test blocks appear at the top level of a `.fa` file, alongside the callable and type declarations. They are not nested inside funcs or flows. A file may contain any number of test blocks:

```fa
use session from "./session"

type LoginRequest
    email as text
    password as text
done

docs LoginRequest
    Credentials for login.
    docs email
        The user's email.
    done
    docs password
        The user's password.
    done
done

docs Login
    Authenticate a user and return a session token.
done

func Login
    take req as LoginRequest
    emit token as text
    fail error as text
body
    token = session.Issue(req.email)
    emit token
done

docs LoginSucceeds
    Verify that valid credentials produce a token.
done

test LoginSucceeds
    mock session.Issue => "tok_abc123"
    req = { email: "alice@example.com", password: "hunter2" }
    result = Login(req)
    must result == "tok_abc123"
done

docs LoginRejectsEmpty
    Verify that empty email is rejected.
done

test LoginRejectsEmpty
    req = { email: "", password: "hunter2" }
    err = trap Login(req)
    must err != ""
done
```

## Every Test Requires a Docs Block

Test blocks are subject to the same documentation requirement as funcs and flows. Each `test Name` must have a corresponding `docs Name` block. Omitting it is a compile error:

```fa
# Missing docs — compile error
test ValidateLength
    must true == true
done
```

```
error: missing docs for test 'ValidateLength'
```

Write the docs block before or after the test:

```fa
docs ValidateLength
    Confirm that strings longer than 500 characters are rejected.
done

test ValidateLength
    long_str = str.repeat("x", 501)
    result = Validate(long_str)
    must result == false
done
```

## What Can Appear in a Test Body

A test body is a sequence of statements. The available statement types are:

- **`mock` declarations** — substitute an imported callable with a fixed value. Must appear at the top of the test body, before any other statements. See [Mock](04-mock.md).
- **Variable assignments** — `name = expr` binds a value to a local name for use in later statements.
- **`must` assertions** — `must expr` evaluates a boolean expression and fails the test if it is false. See [Must Assertions](02-must-assertions.md).
- **`trap` expressions** — `err = trap FuncCall(args)` expects a call to fail and captures the error. See [Trap Failures](03-trap-failures.md).
- **Callable invocations** — call the func or flow under test, or helper callables, to produce values to assert on.

Tests do not use `emit`, `fail`, `loop`, `case`, `sync`, or any of the runtime body constructs. The test body is a flat sequence of assignments and assertions.

## Test Naming Conventions

Test names should describe the behavior being verified, not the mechanics of the test. Prefer names that read as complete statements:

```fa
# Good — describes behavior
test LoginSucceedsWithValidCredentials
test LoginFailsWhenPasswordIsWrong
test RegisterRejectsDuplicateEmail

# Less good — describes what the test does, not what it verifies
test TestLoginHappyPath
test TestLoginError
test TestEmailValidation
```

Good test names serve as executable specifications. Reading the list of test names for a module should give you a clear picture of what the callable guarantees.

## Test-Only Requirement

By default, missing test blocks produce a warning rather than a hard error. This allows teams to ship a callable before writing its tests, while the tooling reminds them of the gap.

To treat missing tests as a hard error (recommended for CI):

```
forai test app/ --require-tests
```

With `--require-tests`, a callable that has no test block fails the test run even if all existing tests pass.

## Tests Are Isolated

Each test block runs in isolation. Variables declared in one test are not visible in another. Mocks declared in one test have no effect on others. If a test modifies state (for example, by calling a func that writes to a file), that state is not cleaned up automatically — tests that require isolation from side effects should use mocks to avoid real I/O.

See [Mock](04-mock.md) for the standard approach to keeping tests free of side effects.
