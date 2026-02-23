# Literals

Literals are the simplest expressions in forai — fixed values written directly in source code. forai has six categories of literals: integers, floats, booleans, strings, lists, and dicts.

## Numeric Literals

forai distinguishes between integer values (`long`) and floating-point values (`real`) based on how the literal is written.

An integer literal is a sequence of digits with no decimal point:

```fa
x = 42
y = 0
z = 1000000
```

A float literal includes a decimal point:

```fa
pi = 3.14159
rate = 0.5
whole = 1.0
```

The distinction matters because forai applies an **integer preservation rule**: arithmetic operators `+`, `-`, `*`, and `%` applied to two integer operands produce an integer result. Division `/` and exponentiation `**` always produce a float. See the [arithmetic chapter](02-arithmetic-and-comparison.md) for details.

Negative numbers are written with unary `-`:

```fa
temperature = -10
delta = -0.001
```

## Boolean Literals

The two boolean literals are lowercase `true` and `false`:

```fa
active = true
done = false
```

Booleans participate in logical operators `&&` (and), `||` (or), and unary `!` (not). They also appear in conditions: `if`, `case`, `branch when`, and the ternary `?:`.

## String Literals

Strings are enclosed in double quotes:

```fa
greeting = "Hello, world!"
empty = ""
```

Strings support a small set of escape sequences:

| Escape | Meaning |
|--------|---------|
| `\n`   | Newline |
| `\t`   | Tab |
| `\\`   | Literal backslash |
| `\"`   | Literal double-quote |
| `\#`   | Literal `#` (prevents interpolation) |

Bare `{` and `}` characters are always literal — they do not need escaping:

```fa
regex_hint = "match {4} digits"
json_like  = "{ key: value }"
```

Only `#{` starts an interpolation sequence. For full string interpolation syntax, see [String Interpolation](03-string-interpolation.md).

## Multi-line Strings

A string literal may span multiple lines. Line breaks inside the double quotes become `\n` characters in the value:

```fa
message = "Line one
Line two
Line three"
```

This is equivalent to writing `"Line one\nLine two\nLine three"`. For structured text with indentation control, use explicit `\n` sequences or the `str.join` op on a list of lines.

## List Literals

A list literal is a comma-separated sequence of values enclosed in square brackets:

```fa
numbers = [1, 2, 3, 4, 5]
words   = ["apple", "banana", "cherry"]
mixed   = [1, "two", true]
empty   = []
```

Lists are ordered and zero-indexed. Access individual elements with bracket indexing: `lst[0]`, `lst[-1]`. Lists are immutable — operations like `list.append` return a new list.

Nesting is allowed:

```fa
matrix = [[1, 2], [3, 4], [5, 6]]
```

## Dict Literals

A dict literal is a comma-separated sequence of `key: value` pairs enclosed in curly braces:

```fa
person = { name: "Alice", age: 30, active: true }
config = { host: "localhost", port: 8080 }
empty  = {}
```

Keys are bare identifiers (not strings). Values can be any expression. Dicts are immutable — use `obj.set` and `obj.merge` to produce updated copies.

Nested dicts are written naturally:

```fa
server = {
  host: "0.0.0.0",
  port: 8080,
  tls: { cert: "/etc/cert.pem", key: "/etc/key.pem" }
}
```

## What Is Not a Literal

`null` is not a source literal in forai. Absent values are represented through the type system: functions that may fail use `fail` to propagate errors, optional results use `trap` to capture them. If you need a sentinel "no value" in a data structure, use a boolean flag or an empty string, depending on context.

There are no character literals — a single character is just a one-character string: `"a"`.

There are no raw string literals or heredoc strings — use `\n` and `\t` escapes inside regular string literals.

## Literal Truthiness

All values in forai have a boolean interpretation when used in conditions. The falsy values are:

- `false`
- `""` (empty string)
- `0` (integer zero)
- `0.0` (float zero)

Everything else is truthy, including non-empty lists and dicts, non-zero numbers, and non-empty strings. This matters for `if`, `branch when`, and the ternary operator.
