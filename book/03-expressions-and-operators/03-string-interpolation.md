# String Interpolation

String interpolation lets you embed expression values directly inside a string literal. Instead of concatenating pieces with `+`, you write the entire string in one place and mark the dynamic parts with `#{}`.

## Basic Syntax

An interpolated string is a normal double-quoted string containing one or more `#{expr}` sequences:

```fa
name = "Alice"
msg  = "Hello, #{name}!"
# msg is "Hello, Alice!"
```

Any forai expression can appear inside `#{}`: variables, arithmetic, function calls, field accesses, and nested interpolations.

```fa
width  = 80
height = 24
desc   = "Terminal is #{width}x#{height} characters."

items  = [1, 2, 3]
report = "List has #{list.len(items)} items."

user   = { name: "Bob", age: 31 }
intro  = "#{user.name} is #{user.age} years old."
```

The expression is evaluated at runtime and converted to a string automatically. Numbers, booleans, and other types are coerced to their text representation.

```fa
count  = 7
active = true
msg    = "Count: #{count}, active: #{active}"
# "Count: 7, active: true"
```

## Arithmetic Inside Interpolation

Since any expression is valid inside `#{}`, you can compute values inline:

```fa
price = 9.99
qty   = 3
total = "Your total is $#{price * qty}."
# "Your total is $29.97."

index = 4
label = "Item #{index + 1} of 10"
# "Item 5 of 10"
```

## Function Calls Inside Interpolation

Op calls and module function calls work inside `#{}`:

```fa
path = "/home/user/file.txt"
ext  = str.split(path, ".")
msg  = "Filename: #{str.upper(path)}"

words = ["one", "two", "three"]
line  = "Words: #{str.join(words, ", ")}"
# "Words: one, two, three"
```

## Multiple Interpolations

A single string can contain any number of `#{}` sequences:

```fa
first = "Alice"
last  = "Smith"
age   = 28
bio   = "#{first} #{last}, age #{age}, joined #{year}."
```

The sequences are evaluated left to right, all in the same scope.

## Escape Sequences

Interpolated strings support the same escape sequences as plain strings:

| Escape | Meaning |
|--------|---------|
| `\n`   | Newline |
| `\t`   | Tab character |
| `\\`   | Literal backslash (`\`) |
| `\"`   | Literal double-quote (`"`) |
| `\#`   | Literal `#` — prevents interpolation |

The `\#` escape is important when you need a literal `#` followed by `{` in a string:

```fa
template = "Use \#{variable} to interpolate."
# "Use #{variable} to interpolate." — the #{} is NOT evaluated
```

Without the backslash, `#{variable}` would be treated as an interpolation.

## Bare Braces Are Literal

Bare `{` and `}` characters — those not preceded by `#` — are always literal and require no escaping:

```fa
regex_tip  = "Use {2,5} to match 2 to 5 repetitions."
json_shape = "{ \"key\": \"value\" }"
template   = "SELECT * FROM {table} WHERE id = {id}"
```

This design makes forai strings safe for embedding regex quantifiers, SQL fragments, and JSON templates without extra quoting overhead.

## Nested String Interpolation

Expressions inside `#{}` can themselves contain string literals, but those inner strings cannot be interpolated (they are plain strings):

```fa
status = true
label  = "Status: #{status ? "on" : "off"}"
# "Status: on"
```

For complex conditional text, assign the computed string to a variable first, then interpolate:

```fa
status  = true
display = status ? "enabled" : "disabled"
msg     = "Feature is #{display}."
```

## Interpolation vs Concatenation

Both approaches produce the same result. Interpolation is generally preferred for readability when embedding multiple values:

```fa
# With concatenation:
msg = "Hello, " + name + "! You have " + to.text(count) + " messages."

# With interpolation:
msg = "Hello, #{name}! You have #{count} messages."
```

Use concatenation when building strings programmatically from lists (via `str.join`) or when you need the parts as separate values first. Use interpolation for human-readable text templates.

## Practical Examples

```fa
# Log formatting
log_line = "[#{level}] #{stamp.now()}: #{message}"

# URL construction
base     = "https://api.example.com"
endpoint = "#{base}/users/#{user_id}/posts/#{post_id}"

# Padding and alignment
padded = "Row #{to.text(row_num)}: #{str.pad_start(value, 10, " ")}"

# Error messages
err_msg = "Expected #{expected}, got #{actual} at index #{idx}."
```
