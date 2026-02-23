# Chapter 15.2: Operators

forai has a fixed set of infix and prefix operators. This chapter documents each operator, its precedence, and its behavior on different types.

## Precedence Table

Operators are listed from **lowest** to **highest** precedence. Operators on the same row have the same precedence and associate left-to-right unless noted.

| Level | Operator(s) | Description | Associativity |
|-------|-------------|-------------|---------------|
| 0 | `? :` | Ternary conditional | Right |
| 1 | `\|\|` | Logical OR | Left |
| 2 | `&&` | Logical AND | Left |
| 3 | `==` `!=` | Equality / inequality | Left |
| 4 | `<` `>` `<=` `>=` | Comparison | Left |
| 5 | `+` `-` | Addition / subtraction / string concat | Left |
| 6 | `*` `/` `%` | Multiplication / division / modulo | Left |
| 7 | `**` | Exponentiation | Right |
| 8 | `-` `!` | Unary negation / logical NOT | Prefix |

Higher precedence means the operator binds tighter. For example, `a + b * c` parses as `a + (b * c)` because `*` has higher precedence than `+`.

## Operator Details

### `? :` — Ternary Conditional (Level 0)

```fa
result = condition ? value_if_true : value_if_false
```

Evaluates `condition` (must be `bool`), then evaluates and returns either `value_if_true` or `value_if_false`. Right-associative: `a ? b : c ? d : e` parses as `a ? b : (c ? d : e)`.

```fa
status = count > 0 ? "found" : "not found"
label = is_admin ? "Admin" : "User"
```

### `||` — Logical OR (Level 1)

```fa
a || b
```

Both operands must be `bool`. Returns `true` if either is `true`. **Not short-circuit**: both operands are always evaluated. If you need short-circuit evaluation, use `case/when` instead.

```fa
can_edit = is_owner || is_admin
```

### `&&` — Logical AND (Level 2)

```fa
a && b
```

Both operands must be `bool`. Returns `true` only if both are `true`. **Not short-circuit**: both operands are always evaluated.

```fa
valid = has_name && has_email
ok = age >= 18 && age <= 65
```

### `==` and `!=` — Equality (Level 3)

```fa
a == b
a != b
```

Deep equality comparison. Works on all types:
- `text`: character-by-character equality.
- `long`, `real`, `bool`: value equality.
- `list`: element-by-element recursive equality.
- `dict`: key-by-key recursive equality (order-independent for dicts).
- `null`: `null == null` is `true`.

```fa
is_match = user.email == "alice@example.com"
changed = old_count != new_count
is_empty = rows == []
```

### `<`, `>`, `<=`, `>=` — Numeric Comparison (Level 4)

```fa
a < b
a > b
a <= b
a >= b
```

Numeric comparison. Both operands must be `long` or `real`. Comparing a `long` and a `real` produces a `real` comparison. Comparing non-numeric types is a compile error.

```fa
too_many = count > 100
in_range = score >= 0 && score <= 100
```

### `+` and `-` — Arithmetic and Concatenation (Level 5)

```fa
a + b
a - b
```

**Addition (`+`):**
- `long + long` → `long`
- `real + real` → `real`
- `long + real` or `real + long` → `real`
- `text + text` → `text` (string concatenation)

**Subtraction (`-`):**
- Numeric operands only.
- `long - long` → `long`
- `real - real` → `real`
- Mixed `long`/`real` → `real`

Integer arithmetic preserves integer type: `2 + 3` is `long` `5`, not `real` `5.0`.

```fa
total = count + 1
greeting = "Hello, " + name + "!"
diff = end_time - start_time
```

### `*`, `/`, `%` — Multiplication, Division, Modulo (Level 6)

```fa
a * b
a / b
a % b
```

**Multiplication (`*`):**
- Integer operands: `long` result, integer multiplication.
- Any `real` operand: `real` result.

**Division (`/`):**
- Always produces `real`, even for integer operands: `7 / 2` is `3.5`.
- Division by zero is a runtime error.

**Modulo (`%`):**
- Integer operands only: `long % long` → `long`.
- `a % b` is the remainder when `a` is divided by `b`.
- Modulo by zero is a runtime error.

```fa
area = width * height
ratio = count / to.real(total)
remainder = n % 10
pages = (total + page_size - 1) / page_size
```

### `**` — Exponentiation (Level 7)

```fa
a ** b
```

Raises `a` to the power `b`. Always produces `real`. Right-associative: `2 ** 3 ** 2` parses as `2 ** (3 ** 2)` = `2 ** 9` = `512.0`.

```fa
squared = x ** 2.0
cube = side ** 3.0
```

### Unary `-` — Negation (Level 8)

```fa
-a
```

Negates a numeric value. `long` negation produces `long`; `real` negation produces `real`.

```fa
opposite = -count
negative_one = -1
```

### Unary `!` — Logical NOT (Level 8)

```fa
!a
```

Inverts a `bool` value. Operand must be `bool`. Compile error if applied to non-bool.

```fa
is_empty = !has_items
not_found = !list.contains(items, id)
```

## Type Preservation Rules

| Operation | Operands | Result |
|-----------|----------|--------|
| `+` `-` `*` | `long`, `long` | `long` |
| `+` `-` `*` | `real`, `real` | `real` |
| `+` `-` `*` | `long`, `real` (either order) | `real` |
| `/` | any numeric | `real` (always float division) |
| `**` | any numeric | `real` (always float) |
| `%` | `long`, `long` | `long` |
| `+` | `text`, `text` | `text` (concatenation) |
| `&&` `\|\|` | `bool`, `bool` | `bool` |
| `!` | `bool` | `bool` |
| `==` `!=` | any, any | `bool` |
| `<` `>` `<=` `>=` | numeric, numeric | `bool` |

## Common Patterns

### Null Checking

```fa
# null == null is true
case value
    when null
        # handle missing
    else
        # handle present
done
```

### Integer Division

To get integer division (floor division), divide and then floor:

```fa
quotient = math.floor(a / to.real(b))
```

### Chained Comparisons

forai does not have chained comparisons (`1 < x < 10` is not valid). Use `&&`:

```fa
in_range = x >= 1 && x <= 10
```

### Short-Circuit Logic

Since `&&` and `||` are not short-circuit, use `case` when you need to avoid evaluating a potentially-failing expression:

```fa
# Safe: only calls expensive_check if fast_check is true
safe = false
case fast_check
    when true
        safe = expensive_check()
    else
done
```

### String Building

```fa
# Multi-part string construction
line = prefix + ": " + value + " (" + to.text(count) + " items)"

# Or use string interpolation
line = "#{prefix}: #{value} (#{count} items)"
```
