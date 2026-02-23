# If / Else

The `if/else` construct provides conditional branching in func and sink bodies. It evaluates a boolean expression and executes one of two (or more) branches depending on the result.

## Basic Syntax

```fa
if condition
  # statements when condition is truthy
else
  # statements when condition is falsy
done
```

Both `else` and the closing `done` are required. An if block without an else is not valid in forai — every conditional path must be explicit.

```fa
if score >= 60
  grade = "pass"
else
  grade = "fail"
done
```

## If / Else If / Else

Chain multiple conditions with `else if`:

```fa
if score >= 90
  grade = "A"
else if score >= 80
  grade = "B"
else if score >= 70
  grade = "C"
else if score >= 60
  grade = "D"
else
  grade = "F"
done
```

Each condition is tested in order. The first branch whose condition is truthy is executed; the rest are skipped.

## Desugaring to Case

`if/else` is **syntactic sugar** that the parser desugars into a `case` expression at parse time. The transformation is:

```fa
# This if/else:
if x > 0
  label = "positive"
else
  label = "non-positive"
done

# Becomes this case internally:
case x > 0
when true
  label = "positive"
else
  label = "non-positive"
done
```

This means `if/else` and `case` share exactly the same runtime behavior and the same scoping rules. In particular: **variables assigned inside an if branch are not visible outside the if block**. Initialize variables before the `if` if you need them afterward:

```fa
# Wrong — status is undefined after if/else
if connected
  status = "online"
else
  status = "offline"
done
emit status   # error: status not in scope

# Correct — initialize before
status = "offline"
if connected
  status = "online"
done   # if with no else — this is NOT valid syntax; always provide else
```

Wait — forai requires `else`. So the correct pattern is:

```fa
status = "offline"
if connected
  status = "online"
else
  status = "offline"
done
emit status   # correct
```

Or use the ternary operator for simple value selection:

```fa
status = connected ? "online" : "offline"
emit status
```

## Conditions

Any expression that evaluates to a truthy or falsy value is a valid condition. forai's truthiness rules:

- Falsy: `false`, `""`, `0`, `0.0`
- Truthy: everything else

```fa
# Numeric check
if count > 0
  msg = "Found #{count} items."
else
  msg = "Nothing found."
done

# String check (empty string is falsy)
if username
  welcome = "Hello, #{username}!"
else
  welcome = "Hello, guest!"
done

# Boolean flag
if is_admin && is_active
  role = "admin"
else
  role = "user"
done
```

## Nested If/Else

Branches can contain nested `if/else` blocks:

```fa
if logged_in
  if is_admin
    page = "admin_panel"
  else
    page = "dashboard"
  done
else
  page = "login"
done
```

Deeply nested conditions are often better expressed as flat `case/when` chains or as separate funcs. See [Case/When](02-case-when.md) for pattern matching over enumerated values.

## If in Flow Bodies

`if/else` is a func-body construct. Flows use `branch when` for conditional sub-pipelines:

```fa
# In a func body:
if error
  fail error
else
  emit result
done

# In a flow body:
branch when has_error
  step HandleError(error to :msg)
done
```

The `branch when` construct is for wiring decisions, not value computation. Compute values in funcs; route events in flows.

## Practical Examples

```fa
func Classify
  take score as long
  emit label as text
  fail msg as text
body
  validated = score >= 0 && score <= 100
  if !validated
    fail "Score must be between 0 and 100"
  else
    label = ""
    if score >= 90
      label = "excellent"
    else if score >= 70
      label = "good"
    else if score >= 50
      label = "average"
    else
      label = "below average"
    done
    emit label
  done
done
```
