# Send Nowait in Flows

`send nowait` in a flow body dispatches a function call as a background task without blocking the pipeline. The flow continues to the next statement immediately, and the dispatched task runs independently.

## Syntax

```fa
send nowait FuncName(arg to :port, ...)
```

The arguments follow the same port-binding syntax as step calls. Unlike a step, there is no `then ... done` block — no outputs are captured.

```fa
flow HandleOrder
  on req from order_server
    step Fulfill(req to :order) then
      next :receipt to receipt
    done

    send nowait NotifyCustomer(receipt to :data)
    send nowait UpdateInventory(receipt to :receipt)

    emit receipt to :out
  done
done
```

Both `NotifyCustomer` and `UpdateInventory` are dispatched in the background. The flow emits the receipt immediately without waiting for either to complete.

## Behavior

When `send nowait` executes:

1. The target function is dispatched as an independent async task
2. The current flow statement continues immediately — no blocking
3. The background task runs to completion (or failure) independently
4. If the background task fails, the error is logged to stderr but not propagated to the flow
5. The background task has no access to the flow's state after dispatch — it receives copies of the arguments

## Comparison with Step

| Feature | `step` | `send nowait` |
|---------|--------|---------------|
| Awaits completion | Yes | No |
| Captures outputs | Yes | No |
| Propagates errors | Yes | No (logs only) |
| Blocks pipeline | Yes | No |
| Use case | Required processing | Fire-and-forget side effects |

Use `step` when the output is needed by a subsequent stage. Use `send nowait` when the operation is optional or supplementary to the main pipeline.

## Send Nowait vs Unconditional Branch

Both `send nowait` and an unconditional `branch` can run side operations in parallel with the main pipeline. The difference is scope and intent:

- `branch` is a full sub-pipeline with steps and emits — used for structured side flows
- `send nowait` is a single function call — used for simple fire-and-forget invocations

```fa
# Using send nowait — simpler for a single call
send nowait LogEvent(req to :event)

# Using unconditional branch — for a multi-step side pipeline
branch
  step TransformEvent(req to :raw) then
    next :event to event
  done
  step LogEvent(event to :event) done
done
```

## Error Isolation

Errors from `send nowait` are fully isolated from the main pipeline:

```fa
flow ProcessPayment
  on req from payment_server
    step ChargeCard(req to :payment) then
      next :receipt to receipt
      next :error   to charge_error
    done

    branch when charge_error
      emit charge_error to :fail
    done

    # These background tasks cannot fail the pipeline
    send nowait SendReceipt(receipt to :data)
    send nowait UpdateLedger(receipt to :entry)
    send nowait AuditLog(receipt to :record)

    emit receipt to :out
  done
done
```

If `SendReceipt` fails (email delivery error, for example), the pipeline continues normally. The failure is logged but the payment is still recorded and returned.

## Send Nowait with Module Functions

Module functions can be dispatched with `send nowait`:

```fa
uses notifications from "./notifications"
uses analytics     from "./analytics"

flow TrackEvent
  on event from event_source
    step ProcessEvent(event to :input) then
      next :result to processed
    done

    send nowait notifications.Push(processed to :event)
    send nowait analytics.Record(processed to :data)

    emit processed to :out
  done
done
```

## When to Use Send Nowait

Use `send nowait` in flows for:

- **Logging and auditing**: Record events without delaying the response
- **Notifications**: Send emails, push notifications, SMS after processing
- **Analytics**: Track metrics without impacting latency
- **Cache updates**: Warm or invalidate caches as a side effect
- **Webhooks**: Notify external systems about events without waiting for acknowledgment

Avoid `send nowait` when:

- The operation's result is needed by a later step
- Failures must be handled and reported to the caller
- The operation must complete before the next event is processed

## Nowait in Func Bodies vs Flow Bodies

`send nowait` appears in both func bodies and flow bodies, with identical semantics:

```fa
# In a func body:
func CreateUser
  take data as dict
  emit user as dict
body
  user = db.exec(conn, "INSERT ...", [data])
  send nowait WelcomeEmail(data.email to :address)
  emit user
done

# In a flow body:
flow OnboardUser
  on req from server
    step CreateUser(req to :data) then
      next :user to new_user
    done
    send nowait NotifyAdmin(new_user to :data)
    emit new_user to :out
  done
done
```

The distinction is the calling context — flow bodies use declarative step-based wiring; func bodies use imperative statements. `send nowait` works naturally in both.
