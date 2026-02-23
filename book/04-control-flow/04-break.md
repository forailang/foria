# Break

The `break` statement exits the nearest enclosing `loop` immediately. Execution continues with the first statement after the loop's `done`.

## Basic Usage

```fa
loop
  line = term.prompt("> ")
  if line == "quit"
    break
  else
    process(line)
  done
done
# execution resumes here after break
```

`break` is only valid inside a `loop` body. Using it outside a loop is a compile-time error.

## Break in Collection Loops

`break` works in collection loops the same way as in bare loops — it exits immediately, even if there are remaining elements:

```fa
names  = ["alice", "bob", "carol", "dave"]
target = "carol"
found  = false

loop names as name
  if name == target
    found = true
    break
  else
    found = found
  done
done

# found is true; loop stopped at "carol"
```

After a `break`, the loop variable is no longer in scope. Any variables mutated by the loop body retain their last-assigned values.

## Break in Nested Loops

`break` exits only the **nearest enclosing** loop. An outer loop continues its iteration:

```fa
matrix = [[1, 2, 3], [4, 5, 6], [7, 8, 9]]
targets = [5, 8]

loop matrix as row
  loop row as cell
    if cell == 5
      break   # exits inner loop only
    else
      process(cell)
    done
  done
  # outer loop continues with next row
done
```

To break an outer loop, use a flag variable:

```fa
stop = false
loop matrix as row
  if stop
    break
  else
    loop row as cell
      if cell == 99
        stop = true
        break
      else
        process(cell)
      done
    done
  done
done
```

## Break and Case

`break` inside a `case/when` arm inside a loop propagates outward to the enclosing loop:

```fa
loop events as evt
  case evt.kind
  when "stop"
    break       # exits the loop
  when "pause"
    wait(1000)
  else
    handle(evt)
  done
done
```

The `break` is not "swallowed" by the `case` — it exits the loop, as expected. This makes it safe to use `break` inside case arms to short-circuit a processing loop.

## Break and Sync

`break` inside a `sync` block is **swallowed** — it does not propagate to the enclosing loop. A `sync` block runs its statements concurrently as a group; there is no concept of "stopping the sync early." Each statement in a sync block is independent and runs to completion.

```fa
loop items as item
  [result] = sync
    result = process(item)  # break here would be swallowed
  done [result]
  # to exit the loop, check the result after sync:
  if result == "stop"
    break
  else
    continue_processing(result)
  done
done
```

The correct pattern is to use a value from the sync block's output as a signal, then check it after the sync and issue a `break` at the loop body level.

## Break with Return

In a func body, you can also use `return` (or `emit` for named ports) to exit both the loop and the function at once:

```fa
func FindValue
  take haystack as list
  take needle as long
  emit index as long
  fail msg as text
body
  i = 0
  loop haystack as item
    if item == needle
      emit i   # exits loop AND function
    else
      i = i + 1
    done
  done
  fail "Not found"
done
```

Using `emit` or `fail` inside a loop exits the entire function — not just the loop. This is often cleaner than using a flag variable and a separate `break`.

## Practical Patterns

### Early exit on error

```fa
lines  = file.read("/etc/hosts")
parsed = []
error  = ""

loop lines as line
  result = parse_line(line)
  if result.ok
    parsed = list.append(parsed, result.value)
  else
    error = result.message
    break
  done
done
```

### Retry loop

```fa
max_attempts = 3
attempt      = 0
success      = false

loop
  attempt = attempt + 1
  response = try_connect(host)
  if response.ok
    success = true
    break
  else if attempt >= max_attempts
    break
  else
    time.sleep(1000)
  done
done
```

### Drain a queue

```fa
loop
  item = queue.pop(q)
  if item == ""
    break
  else
    process(item)
  done
done
```
