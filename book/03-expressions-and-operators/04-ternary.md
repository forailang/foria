# Ternary Operator

The ternary operator `?:` is forai's inline conditional expression. It selects between two values based on a boolean condition, all within a single expression.

## Syntax

```fa
result = condition ? value_if_true : value_if_false
```

The condition is any expression. If it is truthy, the whole expression evaluates to the left branch; if falsy, to the right branch.

```fa
label  = score >= 60 ? "pass" : "fail"
capped = n > 100 ? 100 : n
sign   = x > 0 ? 1 : (x < 0 ? -1 : 0)
```

## Precedence

The ternary operator has the **lowest precedence** of all operators — lower than `||`, `&&`, comparisons, and arithmetic. This means the condition, true branch, and false branch are each parsed as complete sub-expressions:

```fa
# This is parsed as:
# result = (a + b > c * d) ? (x || y) : (z && w)
result = a + b > c * d ? x || y : z && w
```

You do not need parentheses around complex conditions or branches. However, parentheses improve readability when ternaries are nested or the branches are long:

```fa
tier = revenue > 100000 ? "platinum"
     : revenue > 10000  ? "gold"
     : revenue > 1000   ? "silver"
     : "bronze"
```

forai does not have multi-line expression syntax — the above is a conceptual representation. In practice, nested ternaries are written on one line or broken up with intermediate variables:

```fa
mid_tier = revenue > 10000 ? "gold" : "silver"
tier     = revenue > 100000 ? "platinum" : (revenue > 1000 ? mid_tier : "bronze")
```

## Short-Circuit Evaluation

The ternary operator short-circuits: only the branch that is selected is evaluated. The other branch is not executed.

```fa
# safe_divisor is only evaluated if denominator != 0
# (in practice, if safe_divisor calls an op, it only runs when denominator != 0)
result = denominator != 0 ? numerator / denominator : 0
```

This matters when branches have side effects or when a branch would produce a runtime error if evaluated with certain inputs.

## Truthiness in the Condition

The condition follows forai's standard truthiness rules:

| Value | Truthy? |
|-------|---------|
| `true` | Yes |
| `false` | No |
| `0` | No |
| `0.0` | No |
| `""` | No |
| Any non-zero number | Yes |
| Any non-empty string | Yes |
| Lists and dicts | Yes (always) |

```fa
# Treat empty string as "none"
display = name ? name : "(anonymous)"

# Use 0 as "unset"
count_str = count ? to.text(count) : "none"
```

## Ternary Inside Interpolation

Ternary expressions work inside `#{}` interpolation blocks:

```fa
status = active ? "enabled" : "disabled"
msg    = "Service is #{active ? "running" : "stopped"}."
```

When the branches are strings, this avoids declaring an intermediate variable.

## Ternary vs If/Else

The ternary operator is an **expression** — it produces a value and can appear anywhere an expression is expected: on the right side of an assignment, inside an interpolation, as a function argument, or nested inside another expression.

`if/else` is a **statement** — it executes code and can assign to variables, but the branches are statement blocks, not expressions. Use `if/else` when each branch needs multiple statements; use `?:` for single-value selection.

```fa
# Ternary: inline, single value
clipped = value > max ? max : value

# If/else: multiple statements per branch
if value > max
  clipped = max
  log.warn("Value #{value} exceeded max #{max}")
else
  clipped = value
done
```

## Practical Examples

```fa
# Default values
host    = config_host ? config_host : "localhost"
timeout = user_timeout ? user_timeout : 30

# Conditional formatting
suffix  = list.len(items) == 1 ? "item" : "items"
summary = "Found #{list.len(items)} #{suffix}."

# Boolean to text
flag_str = is_admin ? "admin" : "user"

# Range clamping
safe_pct = pct < 0 ? 0 : (pct > 100 ? 100 : pct)

# Conditional URL fragment
endpoint = debug ? base + "/debug/data" : base + "/data"
```
