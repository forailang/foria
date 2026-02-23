# Port Naming

Ports are the named connection points on funcs, flows, and sinks. They define the interface that the flow's wiring connects to. Understanding port naming is essential for writing correct step declarations and `emit`/`fail` routing.

## What Is a Port

A port is a declared name on a `take`, `emit`, or `fail` declaration. Ports identify where data enters and leaves a callable:

```fa
func Transform
  take input  as text    # port name: "input"
  emit result as text    # port name: "result"
  fail error  as text    # port name: "error"
body
  ...
done
```

In this function, three ports exist: `:input` (take), `:result` (emit), and `:error` (fail).

## Port Syntax: The Colon Prefix

When used in step wiring, `emit`, `next`, and `branch` statements, port names are prefixed with `:`:

```fa
step Transform(data to :input) then   # :input is the take port
  next :result to output              # :result is the emit port
  next :error  to err_msg             # :error is the fail port
done
```

The `:` prefix distinguishes port names from variable names. Variables are bare identifiers; ports always start with `:`.

## Take Ports in Step Calls

In a step call, arguments are bound to take ports using `varname to :portname`:

```fa
step Multiply(a to :left, b to :right) then
  next :product to result
done
```

This maps variable `a` to the `:left` take port, and `b` to the `:right` take port. The callee declared:

```fa
func Multiply
  take left    as real
  take right   as real
  emit product as real
body
  return left * right
done
```

Port names in the step call must exactly match the `take` names in the callee.

## Emit and Fail Ports in Next Statements

After a step call, `next :portname to varname` extracts the output from a named port:

```fa
step Parse(raw to :input) then
  next :tokens to tokens    # binds :tokens emit port to variable 'tokens'
  next :error  to lex_err   # binds :error fail port to variable 'lex_err'
done
```

Each `next` statement creates a new variable in the flow's current scope.

## Flow Emit and Fail Ports

Flows declare their own `emit` and `fail` ports, which are used to send data out of the flow:

```fa
flow Pipeline
  take raw    as text
  emit result as text   # flow's :result port
  fail error  as text   # flow's :error port
body
  step Process(raw to :input) then
    next :output to processed
    next :error  to proc_err
  done

  emit processed to :result   # send to flow's own :result port
  emit proc_err  to :error    # send to flow's own :error port
done
```

`emit varname to :portname` sends a variable value through the flow's declared output port.

## V1 Named Ports vs V2 Unnamed Ports

**V1 named ports** use `as portname` in declarations:

```fa
func Convert
  take raw     as text
  emit value   as long
  fail message as text
body
  emit to.long(raw) as value
  fail "Cannot convert" as message
done
```

In v1, the port name appears after `as` in both the declaration and the emit/fail statement.

**V2 unnamed ports** use `return` and `fail` without a name:

```fa
func Convert
  take raw as text
  return long
  fail text
body
  return to.long(raw)
  fail "Cannot convert"
done
```

In v2, there is one implicit output port (`:result` by convention) and one implicit fail port (`:error`). V2 is simpler for single-output functions.

## Wiring V2 Functions in Steps

When calling a v2 function in a step, use `:result` for the output and `:error` for the fail:

```fa
step Convert(raw to :raw) then
  next :result to value
  next :error  to conv_err
done
```

## Port Names in Branch Emit

Inside a `branch` block, `emit` sends to the flow's own ports:

```fa
branch when is_error
  emit error_msg to :error   # sends to the flow's :error port
done

emit result to :out           # sends to the flow's :out port
```

## Default Port Names

When a function has a single take, emit, or fail, the port name defaults to the parameter name from the `take`/`emit`/`fail` declaration. For v2 functions, the defaults are `:result` (for `return`) and `:error` (for `fail`).

## Port Naming Conventions

By convention, forai ports use descriptive lowercase names:

| Purpose | Common Port Names |
|---------|------------------|
| Single input | `:input`, `:data`, `:raw`, `:request` |
| Single output | `:result`, `:output`, `:response` |
| Error | `:error`, `:reason`, `:message` |
| Named inputs | `:user`, `:config`, `:db`, `:token` |
| Named outputs | `:user`, `:token`, `:body`, `:count` |

Consistent naming makes step wiring readable:

```fa
step Authenticate(request to :input, conn to :db) then
  next :user  to authenticated_user
  next :error to auth_error
done
```

## Practical Example

```fa
func SplitName
  take full_name as text
  emit first     as text   # port :first
  emit last      as text   # port :last
  fail error     as text   # port :error
body
  parts = str.split(str.trim(full_name), " ")
  if list.len(parts) < 2
    fail "Name must have at least two parts"
  else
    emit parts[0] as first
    emit parts[1] as last
  done
done

flow ProcessName
  take raw    as text
  emit result as text
  fail error  as text
body
  step SplitName(raw to :full_name) then
    next :first to first_name
    next :last  to last_name
    next :error to split_error
    emit split_error to :error
  done

  step FormatName(first_name to :first, last_name to :last) then
    next :result to formatted
  done

  emit formatted to :result
done
```
