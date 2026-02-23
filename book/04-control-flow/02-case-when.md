# Case / When

The `case/when` construct is forai's primary pattern-matching statement. It compares a value against a sequence of patterns and executes the first matching arm. Unlike `if/else` (which desugars to `case`), you write `case/when` directly when matching against known literals or enum variants.

## Basic Syntax

```fa
case expr
when value1
  # statements
when value2
  # statements
else
  # statements (required — the default arm)
done
```

The `else` arm is required. Every `case` block must have an explicit default.

```fa
case command
when "start"
  status = "running"
when "stop"
  status = "stopped"
when "restart"
  status = "restarting"
else
  status = "unknown"
done
```

## Matching Literals

`when` arms can match string literals, integer literals, boolean literals, and float literals:

```fa
case code
when 200
  label = "OK"
when 404
  label = "Not Found"
when 500
  label = "Server Error"
else
  label = "Other"
done

case is_active
when true
  msg = "Active"
when false
  msg = "Inactive"
else
  msg = "Unknown"
done
```

## Matching Enum Variants with Binding

When the case subject is an enum value, `when` arms can name the variant and bind its payload to a variable:

```fa
case result
when ok(value)
  output = "Got: #{value}"
when err(e)
  output = "Error: #{e}"
else
  output = "Unexpected"
done
```

The binding variable (`value`, `e`) is available only within that arm's statement block. The match arms use lowercase (`ok`, `err`, `some`) for standard Result/Option patterns.

Custom enum variants are matched by name with their payload:

```fa
case shape
when Circle(radius)
  area = 3.14159 * radius * radius
when Rectangle(w, h)
  area = w * h
when Triangle(base, height)
  area = 0.5 * base * height
else
  area = 0
done
```

## Arm Variable Scope

Variables assigned inside a `when` arm are **scoped to that arm only**. The IR discards each arm's scope after execution. This means you cannot use a variable first assigned in a `when` arm after the `case` block closes.

```fa
# This does NOT work — 'description' is out of scope after done
case kind
when "admin"
  description = "Full access"
else
  description = "Limited access"
done
emit description   # error: description not in scope
```

The correct pattern is to initialize the variable before the `case` block:

```fa
description = ""
case kind
when "admin"
  description = "Full access"
else
  description = "Limited access"
done
emit description   # correct
```

This is consistent with how `if/else` works (since `if/else` desugars to `case`). Binding variables from enum variant arms (`when ok(v)`) follow the same rule — they only exist inside their arm.

## Multiple Statements per Arm

Each arm can contain any number of statements:

```fa
case event_type
when "login"
  user_key = "user:" + user_id
  attempts = 0
  log.info("Login attempt for #{user_id}")
when "logout"
  session_key = "session:" + session_id
  log.info("Logout for #{user_id}")
else
  log.warn("Unknown event: #{event_type}")
done
```

## Case Without a Payload (Enum Flags)

When an enum variant carries no payload, match it by name alone:

```fa
case status
when Active
  label = "online"
when Inactive
  label = "offline"
when Suspended
  label = "suspended"
else
  label = "unknown"
done
```

## Nested Case

Arms can contain nested `case` blocks:

```fa
case method
when "GET"
  case path
  when "/health"
    response = "ok"
  when "/status"
    response = current_status
  else
    response = "not found"
  done
when "POST"
  response = handle_post(body)
else
  response = "method not allowed"
done
```

## Case vs If/Else

Use `if/else` for conditions involving comparisons and boolean logic. Use `case/when` for matching a single value against a known set of literals or enum variants:

```fa
# Prefer if/else for conditions:
if score > 90 && bonus_applied
  tier = "platinum"
else
  tier = "standard"
done

# Prefer case/when for dispatch:
case tier
when "platinum"
  discount = 0.30
when "gold"
  discount = 0.20
when "silver"
  discount = 0.10
else
  discount = 0.0
done
```

## Practical Example

```fa
func RouteRequest
  take method as text
  take path as text
  emit response as text
  fail reason as text
body
  response = ""
  case method
  when "GET"
    case path
    when "/ping"
      response = "pong"
    when "/version"
      response = "1.0.0"
    else
      fail "Not found: #{path}"
    done
  when "POST"
    response = "Created"
  else
    fail "Method not allowed: #{method}"
  done
  emit response
done
```
