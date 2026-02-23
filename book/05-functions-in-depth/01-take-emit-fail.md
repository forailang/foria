# Take, Emit, Fail

Every `func` and `sink` in forai declares its interface using three port keywords: `take` (inputs), `emit` (success outputs), and `fail` (error outputs). These declarations form the function's contract with the rest of the system.

## Take: Declaring Inputs

`take` declares one input parameter. A func can have multiple `take` declarations:

```fa
func Add
  take a as long
  take b as long
  emit sum as long
body
  emit a + b
done
```

Each `take` specifies a name and a type. At runtime, all takes must be satisfied before the function body runs. If a take receives invalid input (wrong type, failed validation), forai automatically emits a `fail` with an error message — the body does not run.

Takes can have type constraints:

```fa
func Register
  take email as text :matches r"[^@]+@[^@]+"
  take age   as long :min 0 :max 150
  emit user  as User
  fail error as text
body
  # email is guaranteed to match the regex
  # age is guaranteed to be 0..150
  emit { email: email, age: age }
done
```

## V1 Named Ports vs V2 Unnamed Ports

forai supports two port-naming conventions. **V1** uses explicit port names with `as`:

```fa
# V1 — named ports
func Divide
  take a as real
  take b as real
  emit result as real
  fail reason as text
body
  if b == 0.0
    fail "Division by zero" as reason
  else
    emit a / b as result
  done
done
```

**V2** omits the port name and uses `return`/`fail` directly:

```fa
# V2 — unnamed ports (simpler)
func Divide
  take a as real
  take b as real
  return real
  fail text
body
  if b == 0.0
    fail "Division by zero"
  else
    return a / b
  done
done
```

In v2, `return value` emits the value on the single output port. `fail message` emits on the single fail port. V2 is preferred for simple functions; v1 is useful when you need named ports for multi-port wiring in flows.

## Emit: Declaring Success Outputs

A func can have zero, one, or multiple `emit` declarations. Zero emits means the func is purely side-effectful (like a sink). Multiple emits define multiple named output ports:

```fa
func ParseCSV
  take raw as text
  emit headers as list
  emit rows    as list
  fail error   as text
body
  lines   = str.split(raw, "\n")
  headers = str.split(lines[0], ",")
  rest    = list.slice(lines, 1, list.len(lines))
  rows    = []
  loop rest as line
    row  = str.split(line, ",")
    rows = list.append(rows, row)
  done
  emit headers as headers
  emit rows    as rows
done
```

Each `emit` statement in the body sends to the named port. When a flow calls a multi-emit func, it routes each port separately in the step wiring.

## Fail: Declaring Error Outputs

`fail` declares an error output port. A func typically has one fail port, but multiple are allowed for different error categories:

```fa
func Fetch
  take url as text
  emit body   as text
  fail network as text
  fail timeout as text
body
  response = http.get(url)
  if response.status == 0
    fail "Connection refused" as network
  else if response.status == 408
    fail "Request timed out" as timeout
  else
    emit response.body as body
  done
done
```

## Fail on Invalid Take Input

When a `take` has type constraints and incoming data violates them, forai automatically routes to the fail port before the body runs. This means you get input validation "for free" — you do not need to check constraint conditions inside the body:

```fa
func SafeDivide
  take numerator   as real
  take denominator as real :min 0.0001  # cannot be near zero
  emit quotient as real
  fail error    as text
body
  # denominator is guaranteed >= 0.0001 here
  emit numerator / denominator
done
```

If the caller passes `0.0` as denominator, the function fails with a constraint violation error and the body never runs.

## Multiple Takes, Order Matters

Takes are positional in function calls. The call `MyFunc(a, b, c)` maps the first argument to the first `take`, second to the second, and so on.

In a `sync` block or step wiring, takes can be provided by port name:

```fa
step Divide(numerator to :a, denominator to :b)
```

## Sinks vs Funcs

A `sink` uses the same `take`/`emit`/`fail` syntax but conventionally has no `emit` (it is a terminal side-effect). If a sink has no `emit`, it still must have a `fail` for error cases:

```fa
sink PrintLine
  take message as text
  fail error   as text
body
  term.print(message)
done
```

Or with v2 syntax:

```fa
sink PrintLine
  take message as text
  fail text
body
  term.print(message)
done
```

## Practical Example: Full V2 Function

```fa
func Greet
  take name  as text :required
  take lang  as text
  return text
  fail text
body
  greeting = ""
  case lang
  when "es"
    greeting = "Hola, #{name}!"
  when "fr"
    greeting = "Bonjour, #{name}!"
  else
    greeting = "Hello, #{name}!"
  done
  return greeting
done
```
