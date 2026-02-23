# Steps and Wiring

A flow body is a declarative pipeline — it describes how data moves from one computation stage to the next. The primary building block is the `step`, which names a function to call and wires its inputs and outputs.

## The Step Statement

A `step` calls a func or flow, binding named ports to data variables. There are three forms: the v2 inline form, the v1 block form, and the fire-and-forget form.

## V2 Inline Step (Preferred)

The v2 inline form puts the call and all its wiring on a `step ... then ... done` block:

```fa
step Func(wire to :port) then
  next :port to wire
  emit wire to :port
done
```

Breaking this down:

- `step Func(wire to :port)` — calls `Func`, passing variable `wire` as the `:port` input
- `then` — begins the output routing section
- `next :port to wire` — takes the `:port` output of `Func` and binds it to variable `wire`
- `emit wire to :port` — sends the variable on the flow's own output port
- `done` — closes the step block

A complete example:

```fa
flow ProcessText
  take raw    as text
  emit result as text
  fail error  as text
body
  step Normalize(raw to :input) then
    next :result to normalized
    next :error  to norm_err
    emit norm_err to :error
  done

  step Analyze(normalized to :text) then
    next :result to analyzed
    emit analyzed to :result
  done
done
```

Data flows from `raw` into `Normalize`, whose `:result` output becomes `normalized`, which then flows into `Analyze`.

## V1 Block Step

The v1 block form separates the call and the routing into distinct sub-blocks:

```fa
step
  Func(wire to :port)
  next :port to wire
done
```

This form is equivalent to v2 but more verbose. V1 is useful when you need to document each stage or when tooling generates step blocks in this format.

```fa
step
  Parse(raw to :input)
  next :result to parsed
  next :error  to parse_err
done
```

## Fire-and-Forget Step

A step that does not need to capture any output uses the fire-and-forget form:

```fa
step Func(args) done
```

This calls `Func` and discards all outputs. It is equivalent to `send nowait` but is expressed as a step in the flow's declarative wiring:

```fa
flow LogAndProcess
  on req from server
    step AuditLog(req to :request) done
    step Process(req to :input) then
      next :result to response
      emit response to :out
    done
  done
done
```

## Wiring Multiple Inputs

Functions with multiple `take` declarations receive multiple arguments in the step call:

```fa
step Merge(left to :a, right to :b) then
  next :result to merged
done
```

The `:port` labels must match the `take` port names in the callee's declaration.

## Wiring Multiple Outputs

Functions with multiple `emit` declarations produce multiple outputs, each routed with its own `next`:

```fa
step SplitStream(data to :input) then
  next :headers to headers
  next :rows    to rows
  next :error   to parse_error
done
```

Each `:port` in the `next` must match an `emit` or `fail` port name in the callee.

## Implicit Port Names

When a function has a single `take`, `emit`, or `fail`, you can often omit the port label in simple pipelines. The v2 style with `return`/`fail` for single-port functions simplifies wiring:

```fa
# Function with 'return text' (v2 style — single unnamed output)
step Transform(input to :raw) then
  next :result to transformed
done
```

The `:result` port name is the default for v2-style `return` outputs.

## Chaining Steps

The output of one step becomes the input to the next:

```fa
flow Pipeline
  take raw    as text
  emit output as text
  fail error  as text
body
  step Tokenize(raw to :input) then
    next :tokens to tokens
    next :error  to lex_error
    emit lex_error to :error
  done

  step Parse(tokens to :input) then
    next :ast   to ast
    next :error to parse_error
    emit parse_error to :error
  done

  step Evaluate(ast to :input) then
    next :result to output
    next :error  to eval_error
    emit eval_error to :error
  done

  emit output to :output
done
```

Each stage consumes the previous stage's output. Errors are routed to the flow's own `:error` port at each stage.

## Module Steps

Steps can call functions from imported modules:

```fa
uses auth from "./auth"
uses db   from "./db"

flow Login
  on req from server
    step auth.Validate(req to :request) then
      next :user  to user
      next :error to auth_error
    done
    step db.SaveSession(user to :user) then
      next :token to session_token
    done
    emit session_token to :out
  done
done
```

## Steps vs Functions

Flows contain steps; funcs contain imperative code. The distinction is intentional:

- **Steps** declare *what* connects to *what* — the shape of the pipeline
- **Func bodies** declare *how* to compute a value — the logic

Keep computation in funcs and wiring in flows. A step that does heavy computation is a hint that the logic should be extracted to a func.
