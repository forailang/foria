# Scoping Rules

forai uses lexical scoping with a small set of deliberate constraints that affect how variables flow across `case`, `loop`, and `sync` blocks. Understanding these rules prevents the most common class of bugs in forai programs.

## Function-Level Scope

Every func, sink, or source body has a single top-level scope. Variables declared (assigned for the first time) in this scope are available from the point of assignment through the end of the body:

```fa
func Example
  take input as text
  emit output as text
body
  prefix = ">> "        # declared here
  trimmed = str.trim(input)    # declared here
  output = prefix + trimmed    # both visible
  emit output
done
```

There is no block-level scope for regular statement sequences — only `case`, `loop`, and `sync` have special scoping behavior.

## Case Arm Scope: Discarded

Variables assigned inside a `case` arm (including arms of `if/else`, which desugars to `case`) are **not visible outside the case block**. The arm's scope is discarded when the arm completes.

```fa
# Wrong — 'value' is not in scope after case/done
case kind
when "price"
  value = to.real(raw)
else
  value = 0.0
done
emit value   # compile error: value not declared
```

The fix is to initialize the variable before the `case`:

```fa
# Correct
value = 0.0
case kind
when "price"
  value = to.real(raw)
else
  value = 0.0
done
emit value   # correct — value is in the outer scope
```

This rule applies to all case patterns, including enum variant bindings:

```fa
# The binding 'n' only exists inside the 'ok(n)' arm
case result
when ok(n)
  formatted = to.text(n)   # 'n' is in scope here
else
  formatted = "error"
done
# 'n' is NOT in scope here — only 'formatted' is (if initialized before)
```

## Loop Body Scope: Reassigns Outer Variables

The loop body can **reassign** variables from the outer scope. This is the primary way to accumulate results across iterations:

```fa
total = 0         # outer scope
loop prices as price
  total = total + price   # reassigns outer 'total'
done
# total is now the sum
```

The loop variable itself (the `as name` binding) exists only for the duration of one iteration:

```fa
loop items as item
  processed = item + "!"  # 'processed' is re-created each iteration
  log.info(processed)
done
# neither 'item' nor 'processed' are in scope here
```

New variables declared inside the loop body are scoped to each iteration. They cannot be read after the loop ends. To collect them, assign them to an outer variable in each iteration:

```fa
results = []
loop items as item
  result = transform(item)   # scoped to this iteration
  results = list.append(results, result)  # accumulated into outer var
done
# 'results' is in scope; 'result' is not
```

## Sync Scope: Isolated Copy

Each statement inside a `sync` block receives an **independent copy** of the current scope at the time the sync starts. Statements inside a sync block cannot reference variables introduced by other statements in the same sync block.

```fa
x = 10
y = 20

[a, b] = sync
  a = fetch_from_service_one(x)   # sees x = 10
  b = fetch_from_service_two(y)   # sees y = 20, not 'a'
done [a, b]

# a and b are now in scope
combined = a + b
```

The sync block cannot use the output of one concurrent statement as the input of another — they are independent. If you need to sequence dependent operations, do them outside the sync or in a separate func.

The `done [exports]` list specifies which variables from inside the sync are merged back into the outer scope. Variables not listed in the exports are discarded:

```fa
[name, email] = sync
  name  = lookup_name(id)
  email = lookup_email(id)
  temp  = "throwaway"   # not exported
done [name, email]
# 'name' and 'email' are in outer scope; 'temp' is not
```

## Initialization Before Case: The Standard Pattern

The scoping rule for `case` leads to a consistent pattern throughout forai codebases: initialize before, reassign inside.

```fa
func Classify
  take raw as text
  emit category as text
body
  category = "unknown"    # initialize to default

  trimmed = str.trim(raw)
  case trimmed
  when "admin"
    category = "admin"
  when "moderator"
    category = "moderator"
  when "user"
    category = "user"
  else
    category = "unknown"  # explicit, even though default matches
  done

  emit category
done
```

Even though `category = "unknown"` is the same as the `else` arm, the pre-initialization makes the code resilient to future changes (adding arms that don't set the variable) and satisfies the compiler's scope tracking.

## Summary of Scoping Behavior

| Construct | New variables after block? | Reassign outer vars? |
|-----------|---------------------------|----------------------|
| `case` arm | No — arm scope discarded | Yes |
| `loop` body | No — iteration scope discarded | Yes |
| `sync` statement | Yes — via export list | N/A (copy) |
| `if/else` arm | No — (desugars to case) | Yes |

The key insight: **forai does not use block-level scoping the way languages like Rust or Go do**. Blocks do not create persistent new scopes. Instead, each outer variable assignment persists, and inner blocks can reassign outer variables but cannot introduce new ones that survive past the block.
