# 10.3 — list

The `list.*` namespace provides immutable list operations. Every op that "modifies" a list returns a new list — the original is never mutated. Lists are ordered and zero-indexed. Elements can be any forai value: text, long, real, bool, dict, or nested lists.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `list.new` | | `[]` | Create an empty list |
| `list.range` | start, end | list | Inclusive integer range `[start, start+1, ..., end]` |
| `list.append` | list, item | list | Return new list with `item` appended at the end |
| `list.len` | list | long | Number of elements |
| `list.contains` | list, val | bool | True if `val` is in the list (equality comparison) |
| `list.slice` | list, start, end | list | Sublist `[start, end)`, clamped to bounds |
| `list.indices` | list | list | Returns `[0, 1, 2, ..., len-1]` for a list |

## Examples

### Building a list incrementally

```fa
func CollectNames
  take items list
  emit names list
body
  names = list.new()
  loop items as item
    name = obj.get(item, "name")
    names = list.append(names, name)
  done
  emit names
done
```

### Using range for iteration

```fa
func SumRange
  take start long
  take end long
  emit total long
body
  nums = list.range(start, end)
  total = 0
  loop nums as n
    total = total + n
  done
  emit total
done
```

### Accessing elements by index (bracket indexing)

Use bracket indexing to access elements by position. Negative indices count from the end:

```fa
func FirstAndLast
  take items list
  emit first value
  emit last value
body
  first = items[0]
  last = items[-1]   # negative index: last element
  emit first
  emit last
done
```

### Checking membership

```fa
func IsAllowed
  take role text
  take allowed_roles list
  emit ok bool
body
  ok = list.contains(allowed_roles, role)
  emit ok
done
```

### Slicing a page of results

```fa
func Paginate
  take items list
  take page long
  take page_size long
  emit page_items list
body
  start = page * page_size
  end = start + page_size
  page_items = list.slice(items, start, end)
  emit page_items
done
```

### Iterating with index

```fa
func IndexedItems
  take items list
  emit result list
body
  idxs = list.indices(items)
  result = list.new()
  loop idxs as i
    item = items[i]
    entry = obj.set(obj.set(obj.new(), "index", i), "value", item)
    result = list.append(result, entry)
  done
  emit result
done
```

### Building a list of ranges

```fa
func ChunkIndices
  take total long
  take chunk_size long
  emit chunks list
body
  chunks = list.new()
  starts = list.range(0, math.floor(total / chunk_size))
  loop starts as i
    start = i * chunk_size
    end = start + chunk_size - 1
    chunk = obj.set(obj.set(obj.new(), "start", start), "end", end)
    chunks = list.append(chunks, chunk)
  done
  emit chunks
done
```

## Common Patterns

### Accumulator pattern

The most common list pattern is building up a result with `list.new()` + `list.append` inside a `loop`:

```fa
out = list.new()
loop items as item
  processed = SomeFunc(item)
  out = list.append(out, processed)
done
```

### Safe last element

Use `items[-1]` for the last element. Always check `list.len` first if the list may be empty:

```fa
n = list.len(items)
if n > 0
  last = items[-1]
done
```

### Convert range to list for arithmetic

`list.range` is useful when you need N repetitions of something:

```fa
times = list.range(1, count)
loop times as _
  # runs `count` times
done
```

## Gotchas

- `list.range(start, end)` is **inclusive** on both ends. `list.range(0, 3)` produces `[0, 1, 2, 3]` (four elements), not three. This differs from most languages' exclusive-end conventions.
- Bracket indexing (`items[i]`) with a negative index counts from the end: `-1` is the last element, `-2` is second-to-last. Out-of-bounds access (positive or negative) raises a runtime error.
- `list.slice(list, start, end)` uses **exclusive** end (half-open interval), unlike `list.range` which is inclusive. The bounds are clamped, so `list.slice(items, 0, 999)` on a 3-element list returns all 3 elements without error.
- Lists are immutable. `list.append` returns a new list; the original is unchanged. Always reassign: `items = list.append(items, newItem)`.
- `list.contains` uses value equality. For dicts and lists nested inside a list, equality is deep structural equality.
- `list.indices` is a convenience for index-based loops. It returns a new list `[0, 1, ..., n-1]`, not a lazy iterator — avoid it on very large lists where you only need a subset of indices.
