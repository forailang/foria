# The forai test Command

The `forai test` command runs the test blocks in one `.fa` file or all `.fa` files in a single directory. It reports results per test, shows timing, and exits with a non-zero code if any test fails.

## Basic Usage

Run tests in a single file:

```
forai test app/auth/Login.fa
```

Run tests in all files in a directory (non-recursive):

```
forai test app/auth/
```

If no path is given, forai tests the current directory:

```
forai test .
```

## No Automatic Recursion

This is the most important operational detail about `forai test`: it does **not** recurse into subdirectories. Running `forai test app/` tests only the `.fa` files directly in `app/` — it does not descend into `app/auth/`, `app/orders/`, or any other subdirectory.

To test an entire project tree, you must run `forai test` at each directory level separately:

```
forai test app/
forai test app/auth/
forai test app/orders/
forai test app/round/
forai test app/round/check/
```

This design keeps each invocation fast and focused. It also means you can test one module in isolation without triggering the tests of all its dependencies.

## Running Tests in CI

A full test run across a project tree is typically scripted:

```bash
forai test app/
forai test app/auth/
forai test app/orders/
forai test app/storage/
```

Or with a shell loop:

```bash
find app -type d | while read dir; do
    forai test "$dir/"
done
```

Each invocation exits with code 1 if any test in that directory fails. A CI pipeline treats any non-zero exit as a failure.

## Output Format

The test runner prints one line per test, preceded by the file it came from:

```
app/auth/Login.fa
  PASS  LoginSucceedsWithValidCredentials    (3ms)
  PASS  LoginRejectsEmptyPassword            (1ms)
  PASS  LoginRejectsUnknownEmail             (2ms)

app/auth/Register.fa
  PASS  RegisterCreatesNewUser               (4ms)
  FAIL  RegisterRejectsDuplicateEmail        (2ms)

app/auth/Logout.fa
  PASS  LogoutInvalidatesSession             (1ms)

---
5 passed, 1 failed (13ms total)
```

The final summary line shows the total counts and elapsed time. Each test line shows `PASS` or `FAIL`, the test name, and its individual duration in milliseconds.

## Failure Details

When a test fails, the runner prints the failure details immediately below the failing test line:

```
  FAIL  RegisterRejectsDuplicateEmail        (2ms)
    Docs: Verify that registering with an already-used email returns a conflict error.
    Assertion failed: err == "email already registered"
      left:  "internal error"
      right: "email already registered"
```

The output includes:
- The test's docs text (the "what this verifies" description)
- The exact `must` expression that failed
- The resolved left and right values

For `trap` failures (where a call was expected to fail but succeeded):

```
  FAIL  LoginFailsOnBlankPassword            (1ms)
    Docs: Verify that a blank password is rejected.
    trap expected a failure but call succeeded
      Call: Login(req)
```

## Exit Codes

| Exit code | Meaning |
|-----------|---------|
| `0` | All tests passed |
| `1` | One or more tests failed |
| `2` | A compilation error prevented tests from running |

Exit code `2` occurs when the source file has a compile error (missing docs, type mismatch, unknown op) that prevents the test runner from loading it. Fix the compile error first, then rerun.

## Requiring Tests

By default, a callable with no tests produces a warning but does not cause `forai test` to exit with a failure:

```
app/orders/Cancel.fa
  WARNING: no tests found (use --require-tests to make this an error)
```

To enforce that every callable has at least one test:

```
forai test app/ --require-tests
```

With `--require-tests`, a file with no test blocks exits with code 1. This is the recommended setting for CI once a project is mature enough to have full test coverage.

## Filtering by Test Name

To run only tests whose names contain a specific substring:

```
forai test app/auth/ --filter "RejectsEmpty"
```

This is useful when iterating on a specific behavior — run only the relevant tests rather than the full directory.

## Verbose Output

By default, passing tests print one summary line each. For more detail (including the docs text of passing tests):

```
forai test app/ --verbose
```

Verbose mode prints the docs text for every test, passing or failing, making it easier to read the test suite as a specification.

## A Complete Test Workflow

A typical development workflow:

1. Write the callable and its docs block.
2. Write one or more test blocks with docs.
3. Run `forai test path/to/file.fa` to execute only the new tests.
4. Iterate on implementation and tests until all pass.
5. Before pushing, run `forai test` at each relevant directory level.

Because tests live in the same file as the callable, there is no separate step to create or link test files. The test suite grows naturally alongside the implementation.
