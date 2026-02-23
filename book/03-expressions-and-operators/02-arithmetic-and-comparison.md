# Arithmetic and Comparison

forai supports a full set of infix operators for arithmetic, comparison, and logic. All operators work on expressions — variables, literals, function call results, field accesses, and nested operator expressions.

## Operator Table

| Operator | Name | Example | Notes |
|----------|------|---------|-------|
| `+`  | Add / Concatenate | `a + b` | Adds numbers; concatenates strings |
| `-`  | Subtract | `a - b` | |
| `*`  | Multiply | `a * b` | |
| `/`  | Divide | `a / b` | Always produces a float |
| `%`  | Modulo | `a % b` | Integer remainder |
| `**` | Exponentiate | `a ** b` | Always produces a float |
| `==` | Equal | `a == b` | Deep equality (JSON-level) |
| `!=` | Not Equal | `a != b` | |
| `<`  | Less Than | `a < b` | |
| `>`  | Greater Than | `a > b` | |
| `<=` | Less Than or Equal | `a <= b` | |
| `>=` | Greater Than or Equal | `a >= b` | |
| `&&` | Logical And | `a && b` | Short-circuits |
| `\|\|` | Logical Or | `a \|\| b` | Short-circuits |
| `!`  | Logical Not (unary) | `!a` | |
| `-`  | Negation (unary) | `-a` | |
| `?:` | Ternary | `c ? a : b` | Lowest precedence |

## Operator Precedence

Precedence runs from lowest (evaluated last) to highest (evaluated first):

| Level | Operators | Associativity |
|-------|-----------|---------------|
| 1 (lowest) | `?:` | Right |
| 2 | `\|\|` | Left |
| 3 | `&&` | Left |
| 4 | `==` `!=` | Left |
| 5 | `<` `>` `<=` `>=` | Left |
| 6 | `+` `-` | Left |
| 7 | `*` `/` `%` | Left |
| 8 | `**` | Right |
| 9 (highest) | Unary `-` `!` | Right |

When in doubt, use parentheses. Parenthesized expressions are always evaluated first regardless of precedence:

```fa
result = (a + b) * (c - d)
check  = (x > 0) && (y > 0)
```

## Integer Preservation Rule

forai tracks numeric types. The operators `+`, `-`, `*`, and `%` are **integer-preserving**: when both operands are integers (`long`), the result is an integer.

```fa
a = 10
b = 3

sum  = a + b   # 13   (long)
diff = a - b   # 7    (long)
prod = a * b   # 30   (long)
rem  = a % b   # 1    (long)
```

The operators `/` and `**` always produce a float (`real`), even when both operands are integers:

```fa
quot = a / b     # 3.3333...  (real)
cube = a ** 3    # 1000.0     (real)
```

This means `10 / 2` is `5.0`, not `5`. If you need integer division, use `math.floor` on the result:

```fa
whole = math.floor(10 / 2)   # 5 (long)
```

Mixed arithmetic (long op real) produces real:

```fa
x = 5 + 1.0   # 6.0 (real)
```

## String Concatenation

The `+` operator concatenates strings when applied to string values:

```fa
first = "Hello"
last  = "World"
full  = first + ", " + last + "!"   # "Hello, World!"
```

Concatenating a non-string with `+` is a type error — convert first:

```fa
count = 42
label = "Item #" + to.text(count)   # "Item #42"
```

For richer formatting, prefer [string interpolation](03-string-interpolation.md).

## Equality and Deep Comparison

`==` and `!=` use **deep JSON equality**. Two values are equal if their JSON representations are equal. This means:

- Primitive values compare by value: `42 == 42`, `"foo" == "foo"`, `true == true`
- Lists compare element-by-element: `[1, 2] == [1, 2]` is `true`
- Dicts compare key-value pairs: `{a: 1} == {a: 1}` is `true`
- Order matters for lists: `[1, 2] == [2, 1]` is `false`
- Key order does not matter for dicts: `{a: 1, b: 2} == {b: 2, a: 1}` is `true`

```fa
list_a = [1, 2, 3]
list_b = [1, 2, 3]
same   = list_a == list_b   # true

dict_a = {x: 1, y: 2}
dict_b = {y: 2, x: 1}
same2  = dict_a == dict_b   # true
```

Comparison operators `<`, `>`, `<=`, `>=` apply to numbers and strings (lexicographic for strings).

## Logical Operators and Short-Circuit Evaluation

`&&` and `||` use short-circuit evaluation:

- `a && b`: evaluates `b` only if `a` is truthy
- `a || b`: evaluates `b` only if `a` is falsy

This means the right-hand side may not be evaluated. This is important if the right side has side effects (which is generally avoided in forai func bodies, but can occur with op calls).

```fa
valid = str.len(name) > 0 && str.len(email) > 0
found = cache_hit || expensive_lookup(key)
```

`!` negates the boolean interpretation of its operand:

```fa
empty  = !str.len(s)          # true if s is ""
absent = !obj.has(map, "key") # true if key not present
```

## Comparison Examples

```fa
# Arithmetic
area   = width * height
bmi    = weight / (height ** 2)
change = (index + 1) % list.len(items)

# Comparisons
in_range = x >= min && x <= max
positive = n > 0

# String comparison
before = name_a < name_b   # alphabetical order

# Equality
matched = input == expected
differs = actual != reference
```
