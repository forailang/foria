# Chapter 12.1: extern func Declarations

An `extern func` is a function whose implementation is provided by the host environment rather than written in forai source code. You declare the interface — the `take`, `emit`, and `fail` ports — but omit the `body`/`done` block entirely. When the runtime executes a call to the extern func, it invokes the host-registered implementation.

## Declaration Syntax

```fa
docs CheckLicense
    Verifies a license key against the external licensing server.
    Provided by the host runtime — not implemented in forai.
done

extern func CheckLicense
    take key as text
    emit result as bool
    fail error as text
```

Compare this to a regular `func`:

```fa
func CheckLicense
    take key as text
    emit result as bool
    fail error as text
body
    # ... implementation
done
```

The `extern func` form has:
- The `extern` keyword before `func`.
- The same `take`/`emit`/`fail` header as a regular func.
- **No `body` keyword**.
- **No `done`**.

The declaration ends after the port list. The compiler registers the function name in its import table; the host runtime is responsible for providing an implementation at the correct import index.

## Rules and Constraints

**One extern func per file.** Like all forai constructs, an extern func lives in its own `.fa` file, and the filename stem must match the func name. `extern func CheckLicense` goes in `CheckLicense.fa`.

**docs block required.** The compiler enforces docs coverage for extern funcs just like regular funcs. A `docs CheckLicense` block must appear in the same file.

**No test required.** The checker exempts extern funcs from the test block requirement, because extern funcs have no forai body to test. If you want to test code that calls an extern func, mock it in the calling func's test block.

**Same call syntax.** From the caller's perspective, an extern func is indistinguishable from a regular func. You call it with the same `FuncName(arg to :port)` syntax:

```fa
func ActivateProduct
    take user_id as text
    take key as text
    emit result as dict
    fail error as text
body
    valid = CheckLicense(key to :key)
    case valid
        when true
            result = obj.set(obj.new(), "status", "activated")
            emit result to :result
        when false
            emit "Invalid license key" to :error
    done
done
```

## Full Extern Func Example

Here is a complete file for an extern func that sends an SMS:

```fa
# SendSMS.fa

docs SendSMSResult
    Result of sending an SMS.

    docs delivered
        Whether the SMS was successfully delivered to the carrier.
    done
done

type SendSMSResult
    delivered bool
done

docs SendSMSError
    An error from the SMS gateway.

    docs message
        Human-readable error description.
    done
done

type SendSMSError
    message text
done

docs SendSMS
    Sends an SMS message via the host-provided gateway.
    The host must register an implementation for this function.
    Takes a phone number in E.164 format and a message body.
done

extern func SendSMS
    take phone as text
    take message as text
    emit result as SendSMSResult
    fail error as SendSMSError
```

And calling it from another func:

```fa
docs NotifyUser
    Sends an SMS alert to the user's phone number.
done

func NotifyUser
    take phone as text
    take alert_text as text
    emit result as bool
    fail error as text
body
    result = SendSMS(phone to :phone, alert_text to :message)
    delivered = obj.get(result, "delivered")
    emit delivered to :result
done
```

## Using extern func in Flows

An extern func can be used in a `flow` step the same way as any other func:

```fa
flow ProcessAlert
    take phone as text
    take message as text
    emit result as bool
    fail error as text
body
    step SendSMS(phone to :phone, message to :message) then
        next :result to sms_result
    done
    delivered = obj.get(sms_result, "delivered")
    emit delivered to :result
done
```

## When to Use extern func

Use `extern func` when:

- The implementation requires native code (Rust, C, WASM) that cannot be expressed in forai.
- You are embedding forai into a host application that needs to expose domain-specific APIs to forai programs.
- You are wrapping a third-party SDK or hardware interface.
- You need access to host-managed state that forai handles cannot represent (e.g., an in-process cache, a native socket, a GPU context).

Do not use `extern func` just to call an HTTP API — the built-in `http.*` ops handle that. Use it when the operation genuinely cannot be expressed in terms of existing forai ops.

## Import Index

Extern funcs are assigned import indices that follow the standard library's import block. The compiler assigns indices sequentially in the order extern funcs appear across the compiled module graph. These indices are used by the WASM runtime to call host functions. See Chapter 12.2 for how host programs register implementations at these indices.

## Mocking extern func in Tests

Since extern funcs have no forai body, you cannot call them directly in a `test` block. Instead, mock them in the test blocks of funcs that call them:

```fa
test NotifyUser
    mock SendSMS => {delivered: true}
    result = NotifyUser("+15551234567" to :phone, "Alert!" to :alert_text)
    must result == true
done
```

The `mock` directive substitutes the extern func's return value for the duration of the test. This lets you test the caller's logic without a real SMS gateway.
