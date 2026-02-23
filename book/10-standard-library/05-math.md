# 10.5 — math, type, and to

This chapter covers three related namespaces:

- `math.*` — rounding operations (arithmetic uses infix operators)
- `type.*` — runtime type introspection
- `to.*` — explicit type conversion between scalars

## math.*

The `math.*` namespace provides rounding operations that have no infix equivalent. All arithmetic uses infix operators: `+` `-` `*` `/` `%` `**`.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `math.floor` | a | long | Integer floor of `a` |
| `math.round` | val, places | real | Round `val` to `places` decimal places |

### Examples

#### Safe division

```fa
func SafeDivide
  take numerator real
  take denominator real
  emit result real
  fail err dict
body
  if denominator == 0
    fail error.new("DIVIDE_BY_ZERO", "Denominator cannot be zero")
  done
  result = numerator / denominator
  emit result
done
```

#### Percentage calculation

```fa
func Percentage
  take part real
  take total real
  emit pct real
body
  raw = (part / total) * 100
  pct = math.round(raw, 2)
  emit pct
done
```

#### Integer arithmetic

```fa
func PageCount
  take item_count long
  take page_size long
  emit pages long
body
  pages = math.floor(item_count / page_size)
  # add 1 if there's a remainder
  rem = item_count % page_size
  if rem > 0
    pages = pages + 1
  done
  emit pages
done
```

#### Power and rounding

```fa
func HypotenuseCm
  take a real
  take b real
  emit c real
body
  a2 = a ** 2
  b2 = b ** 2
  c = math.round((a2 + b2) ** 0.5, 4)
  emit c
done
```

### Common Patterns

#### Clamp a value between min and max

```fa
clamped = value
if value < min
  clamped = min
done
if value > max
  clamped = max
done
```

#### Round to nearest integer

```fa
rounded = math.floor(value + 0.5)
```

### Gotchas

- `/` and `%` both raise runtime errors on zero denominator. Guard with an `if` check before calling.
- `math.floor` returns `long`. Use `to.long` on other results if you need an integer.
- `math.round(val, 0)` rounds to the nearest whole number but still returns `real`. Use `to.long(math.round(val, 0))` to get a `long`.

---

## type.*

The `type.*` namespace has a single op: `type.of`. It returns the runtime type of any value as a text string. This is useful for dynamic dispatch, input validation, and debugging.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `type.of` | val | text | Returns `"text"`, `"bool"`, `"long"`, `"real"`, `"list"`, `"dict"`, or `"void"` |

### Examples

```fa
func Describe
  take val value
  emit description text
body
  kind = type.of(val)
  case kind
    when "text"
      emit "a string of length #{to.text(str.len(val))}"
    when "long"
      emit "integer: #{to.text(val)}"
    when "real"
      emit "float: #{to.text(val)}"
    when "list"
      emit "list with #{to.text(list.len(val))} items"
    when "dict"
      emit "object with keys: #{str.join(obj.keys(val), ", ")}"
    else
      emit "other type: #{kind}"
  done
done
```

### Gotchas

- Handle types (`db_conn`, `http_server`, `ws_conn`) are not exposed by `type.of`. They return an implementation-defined string.
- There is no `"null"` or `"nil"` type in forai — the `void` type is used for missing/empty values.

---

## to.*

The `to.*` namespace converts values between forai scalar types. Conversion is explicit — forai does not coerce types automatically.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `to.text` | val | text | Convert any value to its string representation |
| `to.long` | val | long | Parse/convert to integer; rounds reals, extracts numbers from strings |
| `to.real` | val | real | Parse/convert to float |
| `to.bool` | val | bool | Convert to bool; `""`, `"false"`, `"0"` are `false` |

### Examples

#### Building display strings

```fa
func FormatCount
  take n long
  emit msg text
body
  emit "Found #{to.text(n)} items"
done
```

#### Parsing user input

```fa
func ParseAge
  take input text
  emit age long
  fail err dict
body
  trimmed = str.trim(input)
  age = to.long(trimmed)
  if age < 0 || age > 150
    fail error.new("INVALID_AGE", "Age must be between 0 and 150")
  done
  emit age
done
```

#### Truthy check from config

```fa
func IsEnabled
  take raw text
  emit enabled bool
body
  # "true", "1", "yes" are truthy; "", "false", "0" are not
  enabled = to.bool(raw)
  emit enabled
done
```

### Gotchas

- `to.long` on a real truncates toward zero (like `math.floor` for positives, `math.ceil` for negatives). Use `math.round` before `to.long` if you want rounding.
- `to.bool` considers `""`, `"false"`, and `"0"` as `false`. All other non-empty strings are `true`, including `"no"` and `"off"`.
- `to.text` on a dict or list produces a JSON-like representation — do not rely on its exact format for serialization. Use `json.encode` for reliable JSON output.
- `to.real` will fail on strings that are not valid numeric representations (e.g., `"hello"`).
