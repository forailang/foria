# Loop

The `loop` statement provides iteration in func and sink bodies. forai has two forms: **collection loops** that iterate over a list, and **bare loops** that repeat indefinitely until a `break`.

## Collection Loop

A collection loop iterates over each element of a list, binding it to a named variable:

```fa
loop items as item
  # item is the current element
done
```

The collection must be a **variable** — you cannot pass an inline expression or function call directly:

```fa
# Correct — iterate over a variable
words = ["alpha", "beta", "gamma"]
loop words as word
  log.info(word)
done

# Incorrect — inline call not allowed
loop str.split(line, " ") as word   # parser error
  log.info(word)
done

# Fix: assign to a variable first
parts = str.split(line, " ")
loop parts as part
  log.info(part)
done
```

The loop variable (`item`, `word`, `part`) is a fresh binding for each iteration. It can be used anywhere in the loop body but does not persist after `done`.

## Mutating Outer Variables in a Loop

The loop body can read and reassign variables declared outside the loop. This is the primary way to accumulate results:

```fa
total = 0
loop prices as price
  total = total + price
done
# total now holds the sum of all prices

result = []
loop items as item
  processed = transform(item)
  result = list.append(result, processed)
done
```

Note that `result` is declared before the loop — the loop body reassigns it on each iteration.

## Bare Loop

A bare `loop` without a collection runs indefinitely. It requires a `break` statement to exit:

```fa
loop
  line = term.prompt("> ")
  if line == "quit"
    break
  else
    process(line)
  done
done
```

This is useful for REPL-style interactions, retry loops, and polling patterns inside funcs.

## Loop with Index

forai does not have a built-in indexed `loop`. When you need both the element and its index, use `list.indices` to get a list of indices and look up elements by index:

```fa
items  = ["a", "b", "c"]
idxs   = list.indices(items)
loop idxs as i
  item = items[i]
  log.info("#{i}: #{item}")
done
```

Alternatively, maintain a counter variable:

```fa
items = ["a", "b", "c"]
i     = 0
loop items as item
  log.info("#{i}: #{item}")
  i = i + 1
done
```

## Nested Loops

Loops can be nested freely:

```fa
rows = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
loop rows as row
  loop row as cell
    process_cell(cell)
  done
done
```

Each `break` only exits the **nearest enclosing loop**. See [Break](04-break.md) for details.

## Loop and Case

`case/when` inside a loop body works normally. Variables declared in a `case` arm are scoped to that arm:

```fa
events = get_events()
loop events as evt
  kind = evt.kind
  case kind
  when "click"
    handle_click(evt)
  when "key"
    handle_key(evt)
  else
    log.warn("Unknown event: #{kind}")
  done
done
```

## Loop and Sync

`sync` blocks inside a loop are supported but behave with scope isolation. Each sync block gets its own copy of the current scope; results are merged back after the concurrent statements complete. The loop itself still runs sequentially — sync makes the statements *within one iteration* concurrent, not the iterations themselves.

## Loop vs Flow

In a **flow body**, there is no `loop` statement. Flows are declarative wiring, not imperative control flow. Repetition in flows comes from the source — events continue to arrive and flow through the pipeline indefinitely. A step connected to a source processes one event at a time, but the runtime keeps calling the step for each new event.

For polling or background repetition, use a `source` with a bare `loop` in its body, or a func that uses a loop internally.

## Practical Examples

```fa
func SumList
  take numbers as list
  emit total as long
body
  acc = 0
  loop numbers as n
    acc = acc + n
  done
  emit acc
done

func FindFirst
  take haystack as list
  take needle as text
  emit index as long
  fail msg as text
body
  found = -1
  i     = 0
  loop haystack as item
    if item == needle && found == -1
      found = i
    else
      found = found
    done
    i = i + 1
  done
  if found >= 0
    emit found
  else
    fail "Not found: #{needle}"
  done
done

func CollectLong
  take items as list
  emit result as list
body
  out = []
  loop items as item
    if str.len(item) > 5
      out = list.append(out, item)
    else
      out = out
    done
  done
  emit out
done
```
