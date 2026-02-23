# func

A `func` is the basic unit of computation in forai. It takes a value in through a `take` port, does work in its `body`, and exits through either an `emit` port (success) or a `fail` port (failure). Funcs are the workstations on the assembly line.

## Basic Structure

```fa
docs MyFunc
    One-sentence description of what this func does.
done

func MyFunc
    take input as text
    emit result as text
    fail error as text
body
    # computation goes here
    emit input
done
```

Every func has four required parts:

1. A `docs` block with the same name as the func
2. The `func` keyword and name
3. Port declarations (`take`, `emit`, `fail`)
4. A `body...done` block with the computation

## Port Declarations

### `take` — Input Port

`take` declares the input. Every func has exactly one `take` port. The name after `take` is the variable name that the input value is bound to inside the body.

```fa
take user_id as text
take count as long
take config as Config
```

The type after `as` can be any primitive type, a user-defined struct type, or an enum type.

### `emit` — Success Output Port

`emit` declares the success output. Every func must have at least one `emit` declaration. The name is used when extracting the result in a flow `step...then...next` block.

```fa
emit result as text
emit count as long
emit record as User
```

Inside the body, `emit value` sends a value out through the emit port and exits the func immediately (no further statements execute).

### `fail` — Failure Output Port

`fail` declares the error output. Every func must have at least one `fail` declaration. The name is used when the caller handles the failure path.

```fa
fail error as text
fail err as ErrorCode
```

Inside the body, `fail "message"` sends a value out through the fail port and exits the func immediately.

## Body

The body is an imperative block containing statements that run in sequence. It ends with `done`.

### Variable Assignment

```fa
body
    x = 42
    name = "Alice"
    greeting = "Hello, #{name}!"
    result_list = list.new()
done
```

Variables are assigned with `=`. They are local to the body. There are no type annotations on local variables — types are inferred from the right-hand side.

### Calling Built-in Ops

Built-in ops are called as `namespace.op(args)`. The result is assigned to a variable. If the result is not needed, use `_`.

```fa
body
    trimmed = str.trim(input)
    upper = str.upper(trimmed)
    _ = term.print(upper)      # discard result
done
```

### Calling Other Funcs

Funcs from imported modules are called the same way:

```fa
use validator from "./validator"

func Process
    take raw as text
    emit result as Record
    fail error as text
body
    validated = validator.Validate(raw to :raw)
    emit validated
done
```

The `to :portName` syntax maps a local variable to the named `take` port of the callee. If the callee takes a single port, the mapping is direct.

### Conditionals

forai supports `if/else if/else/done` conditionals. The condition is a full boolean expression. There is no colon — the block ends with `done`.

```fa
body
    if str.len(input) == 0
        fail "input is empty"
    else if str.len(input) > 100
        fail "input too long"
    else
        result = str.trim(input)
        emit result
    done
done
```

The `case/when/else/done` form does pattern matching on a value:

```fa
body
    case status
        when "active"
            label = "Active User"
        when "inactive"
            label = "Inactive User"
        else
            label = "Unknown"
    done
    emit label
done
```

**Important scoping rule:** Variables assigned inside a `case` arm are not visible outside the `case` block. Initialize variables before the `case` if you need them after it:

```fa
body
    label = ""           # initialize before case
    case status
        when "active"
            label = "Active"
        when "inactive"
            label = "Inactive"
        else
            label = "Unknown"
    done
    emit label           # label is visible here because it was pre-initialized
done
```

### Loops

`loop expr as item` iterates over a list:

```fa
body
    items = ["apple", "banana", "cherry"]
    results = list.new()
    loop items as fruit
        upper = str.upper(fruit)
        results = list.append(results, upper)
    done
    emit results
done
```

Use `break` to exit a loop early.

### Sync Blocks

`sync` runs multiple statements concurrently and waits for all to complete:

```fa
body
    [a, b] = sync
        a = fetch_data("source_a")
        b = fetch_data("source_b")
    done [a, b]
    combined = obj.merge(a, b)
    emit combined
done
```

The variables in the left-hand list and the `done [...]` list must match. Each statement inside the sync block runs independently — they cannot reference each other's results.

### String Interpolation

Use `#{}` to interpolate expressions into strings:

```fa
body
    n = 42
    msg = "The answer is #{n}."
    full = "User #{user.name} has #{count} items."
    emit msg
done
```

## A Complete Annotated Example

```fa
docs Classify
    Categorizes a user command into a routing label.
    Returns "help", "ls", "quit", "ns", "op", or "unknown".
done

func Classify
    take cmd as text              # single input port
    emit result as text           # success output
    fail error as text            # failure output
body
    if cmd == "help"
        r = "help"
        emit r                    # exits immediately on success
    else if cmd == "ls"
        r = "ls"
        emit r
    else if cmd == "quit" || cmd == "exit"
        r = "quit"
        emit r
    else if str.contains(cmd, ".")
        r = "op"
        emit r
    else
        # Check if it's a known namespace
        known = ["obj", "list", "str", "math", "json", "db", "term", "http"]
        is_ns = list.contains(known, cmd)
        if is_ns
            emit "ns"
        else
            emit "unknown"
        done
    done
done

test Classify
    must Classify("help") == "help"
    must Classify("quit") == "quit"
    must Classify("obj") == "ns"
    must Classify("str.upper") == "op"
    must Classify("xyz") == "unknown"
done
```

## Testing Funcs

Every func must have a `test` block (and a `docs` block for the test). Tests call the func directly and assert on its result with `must`:

```fa
docs FormatTest
    Verifies greeting format.
done

test FormatTest
    must Format("Alice") == "Hello, Alice!"
    must Format("Bob") == "Hello, Bob!"
done
```

Use `trap` to test the failure path:

```fa
docs ValidateEmptyTest
    Verifies that empty input produces a failure.
done

test ValidateEmptyTest
    err = trap Validate("")
    must err == "input is empty"
done
```

## Rules and Gotchas

- Every `func` must have both `emit` and `fail` port declarations. A func with only `emit` does not compile.
- `emit` and `fail` exit the body immediately. No code after them runs.
- Variables assigned inside `case` arms are scoped to the arm. Pre-initialize before the `case` if you need the value after.
- The `take` port name is the variable name inside the body. `take user as User` means the body variable is named `user`.
- Funcs may not contain `step`, `branch`, `state`, or `on` — those are flow and source constructs.
