# Field Docs

When you document a `type`, you must also document each of its fields. Field documentation is written as nested `docs fieldName` ... `done` blocks inside the type's own docs block. The compiler requires a docs entry for every field in every documented type.

## Syntax

Field docs are nested within the type's top-level docs block:

```fa
docs TypeName
    Description of the type itself.

    docs fieldName
        Description of this field.
    done

    docs anotherField
        Description of this field.
    done
done
```

Each `docs fieldName` block describes the field with that name. The body is free-form text, identical to any other docs body. Nested `done` closes the field docs; the outer `done` closes the type docs.

## Full Example

Here is a type declaration and its complete documentation:

```fa
type UserRecord
    id as text
    email as text
    role as text
    createdAt as text
done

docs UserRecord
    A record representing a registered user in the system.

    docs id
        A UUID assigned at account creation. Immutable after assignment.
    done

    docs email
        The user's verified email address. Must be unique across all accounts.
    done

    docs role
        The user's authorization role. One of: "admin", "editor", "viewer".
    done

    docs createdAt
        ISO 8601 timestamp of when the account was created, in UTC.
    done
done
```

The type and its docs block are independent declarations — they are matched by name, not by proximity. You can place the docs block before or after the type declaration.

## Every Field Must Be Documented

If a type has three fields, its docs block must contain three field docs entries — one per field, with matching names. Omitting a field's docs is a compile error:

```fa
type Order
    id as text
    amount as text
    status as text
done

docs Order
    A purchase order in the system.

    docs id
        Unique identifier for the order.
    done

    docs amount
        Total order value as a decimal string.
    done

    # Missing: docs status — compiler error
done
```

```
error: missing field docs for 'status' in type 'Order'
```

## Orphan Field Docs

A field docs entry whose name does not match any field in the type is an orphan, and the compiler rejects it:

```fa
type Product
    name as text
    price as text
done

docs Product
    A product available for purchase.

    docs name
        The display name of the product.
    done

    docs price
        The price in USD as a decimal string.
    done

    docs sku
        The product's stock-keeping unit code.  # error: no field 'sku' in type Product
    done
done
```

This prevents documentation from drifting out of sync with the type definition. If you add or remove a field, you must update the docs to match.

## Types Without Fields

A type with no fields (used as an opaque token or marker) only needs a top-level docs block — no field docs are required:

```fa
type Empty
done

docs Empty
    A placeholder type used where no data needs to be passed.
done
```

## Open Types

The `open` modifier does not change the documentation requirement. An open type's fields are documented the same way as a private type's fields:

```fa
open type Config
    host as text
    port as text
    debug as bool
done

docs Config
    Runtime configuration loaded from environment variables.

    docs host
        The hostname or IP address the server binds to.
    done

    docs port
        The port number as a string. Must be parseable as an integer.
    done

    docs debug
        When true, verbose request logging is enabled.
    done
done
```

## Nested Types in the Same File

If a file declares multiple types, each needs its own top-level docs block with field docs for all its fields:

```fa
type Request
    method as text
    path as text
done

type Response
    status as text
    body as text
done

docs Request
    An incoming HTTP request.

    docs method
        HTTP verb: "GET", "POST", "PUT", "DELETE", etc.
    done

    docs path
        The URL path, starting with "/".
    done
done

docs Response
    An outgoing HTTP response.

    docs status
        HTTP status code as a string, e.g. "200", "404".
    done

    docs body
        The response body content.
    done
done
```

## Why Field Docs Are Enforced

Type fields are interfaces. When another module receives a value of type `UserRecord`, the field names are its only guide to what each field contains. Without field-level docs, reading a type is like reading a struct in a language with no comments — you can see the names, but not the semantics, constraints, or expected format.

Field docs enforcement ensures that every data shape passed between modules is fully described. Combined with the `forai doc` command, this produces structured API documentation that includes per-field descriptions — without any additional tooling.
