# Enums

An enum is a type with a fixed set of named string variants. Use enums when a value can only be one of a known set of named possibilities — order status, user role, command type, error code.

## Declaring an Enum

Use the `enum` keyword followed by the enum name, one variant per line, and `done`:

```fa
enum Role
    Admin
    Editor
    Viewer
    Guest
done
```

At runtime, enum values are strings. `Role.Admin` is the string `"Admin"`. There is no special enum boxing — the value traveling through the pipeline is just a text value that happens to be validated against the enum's variant list.

## Visibility: `open`

Like struct types, enums are private to their module by default. Use `open` to make them accessible from other modules:

```fa
open enum OrderStatus
    Pending
    Processing
    Shipped
    Delivered
    Cancelled
done
```

## Using Enums in Type Declarations

Enum types can be used as field types in structs and as port types in funcs and flows:

```fa
docs Order
    A customer order with its current status.

    docs id
        The unique order identifier.
    done

    docs status
        The current processing status of the order.
    done

    docs total
        The order total in dollars.
    done
done

type Order
    id     uuid
    status OrderStatus
    total  real
done
```

And in func ports:

```fa
func TransitionOrder
    take order as Order
    emit result as OrderStatus
    fail error as text
body
    case order.status
        when "Pending"
            emit "Processing"
        when "Processing"
            emit "Shipped"
        else
            fail "cannot transition from #{order.status}"
    done
done
```

## Pattern Matching with `case/when`

The `case/when/else/done` construct matches against enum variant strings. Because enum values are strings at runtime, `when` arms use string literals:

```fa
func DescribeRole
    take role as Role
    emit result as text
    fail error as text
body
    label = ""
    case role
        when "Admin"
            label = "Full system access"
        when "Editor"
            label = "Can create and edit content"
        when "Viewer"
            label = "Read-only access"
        when "Guest"
            label = "Limited public access"
        else
            label = "Unknown role"
    done
    emit label
done
```

**Scoping rule:** Variables assigned inside a `case` arm are not visible after the `case` block. Pre-initialize them before the block if you need them afterward (as shown above with `label = ""`).

## The `open` Modifier and Extensible Enums

A regular enum is closed: the compiler validates that only declared variants are used. An `open` enum allows additional variants beyond those declared — useful when the enum is shared across modules and may be extended in the future, or when values come from an external system.

```fa
open enum Permission
    Read
    Write
    Delete
    Admin
done
```

With an `open` enum, a value like `"SuperAdmin"` (not in the declaration) is accepted without a compile error. Use open enums when you cannot guarantee that all variants are known at compile time.

## Enums vs Text

You may wonder: if enum values are just strings, why not use `text` everywhere? Two reasons:

1. **Documentation.** An enum name in a port declaration tells the reader exactly which values are valid. `take role as Role` is more expressive than `take role as text`.
2. **Validation.** The type system validates enum values at `take` and `emit` boundaries. If a func tries to emit `"Superuser"` as a `Role` (which has no `Superuser` variant), the runtime catches it.

For truly open-ended string values, use `text`. For a well-defined set of named values, use an enum.

## A Complete Example

```fa
docs CommandKind
    The recognized command categories for the documentation browser.
done

enum CommandKind
    Help
    List
    Namespace
    Operation
    Quit
    Unknown
done

docs Classify
    Categorizes a user command into a recognized command kind.
done

func Classify
    take cmd as text
    emit result as CommandKind
    fail error as text
body
    if cmd == "help"
        emit "Help"
    else if cmd == "ls"
        emit "List"
    else if cmd == "quit" || cmd == "exit"
        emit "Quit"
    else if str.contains(cmd, ".")
        emit "Operation"
    else
        known = ["obj", "list", "str", "math", "json", "db", "term"]
        is_ns = list.contains(known, cmd)
        if is_ns
            emit "Namespace"
        else
            emit "Unknown"
        done
    done
done

test Classify
    must Classify("help") == "Help"
    must Classify("quit") == "Quit"
    must Classify("str.upper") == "Operation"
    must Classify("obj") == "Namespace"
    must Classify("xyz") == "Unknown"
done
```

## Enums in Flow Branches

Enum-matched branching in flows uses string comparison against the variant name:

```fa
flow RouteByKind
body
    step sources.Commands() then
        next :cmd to raw_cmd
    done
    step Classify(raw_cmd to :cmd) then
        next :result to kind
    done
    branch when kind == "Help"
        step display.PrintHelp() done
    done
    branch when kind == "List"
        step display.PrintNamespaces() done
    done
    branch when kind == "Quit"
        step display.PrintGoodbye() done
    done
    branch when kind == "Unknown"
        step display.PrintError(raw_cmd to :cmd) done
    done
done
```

## Rules and Gotchas

- Enum values are strings at runtime. `when "Admin"` compares against the string `"Admin"`, not a special enum value.
- Enums are private by default. Use `open enum` to share across modules. (Note: `open` here controls visibility, not variant extensibility — for extensible variants, also add `open`; the same keyword serves both purposes.)
- There is no exhaustiveness check in `case/when`. Always provide an `else` arm if there is any possibility of an unexpected value.
- Enum declarations are exempt from the `docs` requirement. You do not need a `docs` block for an `enum` declaration (though it is good practice).
- `open` enums accept any string value, including undefined variants. Use them only when necessary.
