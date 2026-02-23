# Branch

The `branch` statement in a flow body creates a conditional or unconditional sub-pipeline. It is the flow-level equivalent of `if/else` — but instead of executing code, it routes events into sub-pipelines that run independently.

## Conditional Branch

A conditional branch executes its body only when the given expression is truthy:

```fa
branch when condition
  # flow statements — steps, emits, nested branches
done
```

```fa
flow HandleRequest
  on req from server
    step ParseBody(req to :raw) then
      next :result to body
      next :error  to parse_error
    done

    branch when parse_error
      step SendError(parse_error to :msg) done
      emit parse_error to :fail
    done

    step ProcessBody(body to :data) then
      next :result to response
    done
    emit response to :out
  done
done
```

The `branch when parse_error` block only runs if `parse_error` is truthy (non-empty string, non-zero). When the branch is skipped, execution continues with the next statement after `done`.

## Unconditional Branch

A bare `branch` (without `when`) always executes:

```fa
branch
  step AuditLog(req to :event) done
  step UpdateMetrics(req to :data) done
done
```

Unconditional branches are useful for spawning parallel side pipelines that always run regardless of conditions. Both the branch and the statements after it execute.

## Independent Sub-Pipelines

Branches do not merge their outputs back into the main pipeline. Each branch is an independent sub-pipeline with its own event routing. This is different from `if/else` in funcs — branches do not "join" at the `done`:

```fa
flow RouteByRole
  on req from server
    step GetRole(req to :request) then
      next :role to role
    done

    branch when role == "admin"
      step AdminHandler(req to :request) then
        next :response to admin_response
      done
      emit admin_response to :out
    done

    branch when role == "user"
      step UserHandler(req to :request) then
        next :response to user_response
      done
      emit user_response to :out
    done

    branch when role == "guest"
      step GuestHandler(req to :request) then
        next :response to guest_response
      done
      emit guest_response to :out
    done
  done
done
```

Each branch independently emits to `:out` when its condition matches. Multiple branches can match simultaneously if their conditions overlap (though typically they are mutually exclusive).

## Nested Branches

Branches can be nested inside other branches:

```fa
branch when authenticated
  branch when is_admin
    step AdminPanel(req to :request) done
  done
  branch when !is_admin
    step UserDashboard(req to :request) done
  done
done
```

Nesting creates conditional sub-sub-pipelines. The outer condition gates the inner branches.

## Branch vs If/Else

The key difference:

- `if/else` in a **func body** computes a value and continues in one code path
- `branch when` in a **flow body** routes events into independent sub-pipelines that do not merge

In a func:

```fa
# Func body — if/else selects one path and continues
if is_admin
  role = "admin"
else
  role = "user"
done
emit role
```

In a flow:

```fa
# Flow body — branch creates independent sub-pipelines
branch when is_admin
  step AdminFlow(req to :r) then
    next :result to admin_result
  done
  emit admin_result to :out
done

branch when !is_admin
  step UserFlow(req to :r) then
    next :result to user_result
  done
  emit user_result to :out
done
```

## Branch with Error Handling

A common pattern is to use a branch to handle errors from a previous step:

```fa
step Parse(raw to :input) then
  next :result to parsed
  next :error  to parse_err
done

branch when parse_err
  step LogError(parse_err to :msg) done
  emit parse_err to :fail
done

# Only runs if parse_err is falsy (no error)
step Process(parsed to :data) then
  next :result to output
done
emit output to :out
```

This pattern — step, then branch-when-error — is the standard flow-level error guard.

## Branch as a Parallel Fork

Unconditional branches can create parallel paths that both execute for every event:

```fa
flow FanOut
  on event from source
    branch
      step PathA(event to :input) done
    done
    branch
      step PathB(event to :input) done
    done
    branch
      step PathC(event to :input) done
    done
  done
done
```

All three branches execute for every event. This is a one-to-many fan-out pattern.

## Practical Example

```fa
flow ProcessOrder
  on order from order_queue
    step Validate(order to :data) then
      next :valid to validated
      next :error to val_error
    done

    branch when val_error
      step NotifyFailure(val_error to :msg, order.customer to :recipient) done
      emit val_error to :fail
    done

    step Fulfill(validated to :order) then
      next :shipment to shipment
      next :error    to fulfill_error
    done

    branch when fulfill_error
      step RetryQueue(validated to :order) done
      emit fulfill_error to :fail
    done

    branch
      send nowait NotifyCustomer(shipment to :data)
    done

    emit shipment to :out
  done
done
```
