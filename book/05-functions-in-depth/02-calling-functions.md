# Calling Functions

forai uses a consistent call syntax across all function types — funcs, flows, sinks, and built-in ops. Arguments are passed positionally in parentheses.

## Basic Call Syntax

A function call is an expression that can appear on the right side of an assignment, in a condition, or inside another expression:

```fa
result = MyFunc(arg1, arg2)
count  = list.len(items)
upper  = str.upper(name)
```

The return value (the emitted output) is bound to the variable on the left. If the function can also fail, the failure is propagated unless explicitly trapped.

## Calling Your Own Functions

Functions defined in the same module are called by name:

```fa
# Calling a func in the same file (not typical — one func per file)
# Calling a func imported from another module
result = Transform(input)
```

In practice, since each file has one callable and modules map to directories, calling another function means calling a module-prefixed name or a directly imported name.

## Module Calls

When you use a module directory with `uses module`, call its functions with dot notation:

```fa
uses auth from "./auth"

# In the body:
token  = auth.CreateToken(user_id)
valid  = auth.Verify(token, secret)
```

The module name is always lowercase; the function name starts with a capital letter (by convention).

## File Imports

When you import a specific file with `use Name from "./path.fa"`, call it directly:

```fa
use Round from "./round.fa"

result = Round(input_value)
```

The imported name `Round` is called like a top-level function.

## Built-in Op Calls

Built-in ops use namespace-prefixed lowercase names:

```fa
text    = to.text(42)
length  = str.len(message)
encoded = json.encode(data)
upper   = str.upper(name)
trimmed = str.trim(input)
joined  = str.join(parts, ", ")
```

The namespace is always lowercase and separated from the op name by a dot. Multiple levels of nesting are possible: `http.server.listen(port)`.

## Chaining Results

The result of one call can be passed directly into another:

```fa
result = str.upper(str.trim(raw_input))
count  = list.len(str.split(text, "\n"))
```

This is idiomatic for short chains. For longer chains, use intermediate variables for readability:

```fa
trimmed  = str.trim(raw_input)
words    = str.split(trimmed, " ")
filtered = list.slice(words, 0, 10)
```

## Passing Expressions as Arguments

Any expression is valid as an argument — literals, variables, arithmetic, interpolated strings, other calls:

```fa
padded  = fmt.pad_hms(hours, minutes, seconds)
encoded = base64.encode(key + ":" + secret)
result  = math.floor(total / list.len(items))
msg     = format_msg("error", "#{code}: #{description}")
```

## Call Results and Types

The type of a call expression is the type declared in the callee's `emit` (or `return`) port. The compiler tracks these types for handle-consuming ops (db, http, ws connections) to enforce correct usage.

For functions that emit structured types, field access works on the result:

```fa
user   = auth.GetUser(token)
name   = user.name
email  = user.email
```

## Error Propagation from Calls

By default, if a called function fails, the failure propagates up through the caller. You do not need to check for errors explicitly — a failed call exits the current function's body and routes to the caller's fail port:

```fa
func ProcessUser
  take user_id as text
  emit profile as text
  fail error   as text
body
  # If GetUser fails, ProcessUser also fails immediately
  user    = auth.GetUser(user_id)
  profile = json.encode(user)
  emit profile
done
```

To capture a failure instead of propagating it, use `trap`:

```fa
user = trap auth.GetUser(user_id)
if user.ok
  profile = json.encode(user.value)
  emit profile
else
  fail "User not found: #{user_id}"
done
```

See [Error Handling](03-error-handling.md) for full details on `trap`.

## Calling Functions in Conditions

Function calls can appear directly in `if` conditions, ternary expressions, and `case` subjects:

```fa
if str.contains(email, "@")
  valid = true
else
  valid = false
done

result = list.len(items) > 0 ? "non-empty" : "empty"
```

## Function Calls in String Interpolation

Calls inside `#{}` interpolation blocks work as expected:

```fa
msg = "Found #{list.len(results)} results in #{str.upper(category)}."
log = "Time: #{stamp.to_ms(stamp.now())}ms"
```

## Practical Example

```fa
func BuildReport
  take user_id as text
  take format  as text
  emit report  as text
  fail error   as text
body
  user    = db.query_user_by_email(conn, user_id)
  name    = str.trim(user.name)
  email   = str.lower(user.email)
  entries = db.query(conn, "SELECT * FROM logs WHERE user = ?", [user_id])

  rows    = []
  loop entries as entry
    line = "#{entry.ts}: #{entry.action}"
    rows = list.append(rows, line)
  done

  body   = str.join(rows, "\n")
  header = "Report for #{name} <#{email}>\n---"

  case format
  when "plain"
    report = header + "\n" + body
  else
    report = json.encode({ user: name, email: email, entries: entries })
  done

  emit report
done
```
