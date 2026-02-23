# 10.4 — obj

The `obj.*` namespace provides immutable dict (object/map) operations. Dicts map text keys to any forai value. All "mutating" ops return a new dict — the original is never changed. Key lookup is case-sensitive. Keys are always strings.

Dicts appear throughout forai as the primary structured data type: HTTP requests, database rows, configuration, and structured errors are all dicts.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `obj.new` | | `{}` | Create an empty dict |
| `obj.set` | obj, key, val | dict | Return new dict with `key` set to `val` |
| `obj.get` | obj, key | value | Get value by key; **errors if key is missing** |
| `obj.has` | obj, key | bool | True if key exists |
| `obj.delete` | obj, key | dict | Return new dict with `key` removed |
| `obj.keys` | obj | list | List of all key strings |
| `obj.merge` | obj1, obj2 | dict | Merge two dicts; `obj2` keys overwrite `obj1` |

## Examples

### Constructing dicts

```fa
func MakeUser
  take name text
  take email text
  take age long
  emit user dict
body
  user = obj.new()
  user = obj.set(user, "name", name)
  user = obj.set(user, "email", email)
  user = obj.set(user, "age", age)
  emit user
done
```

### Chained set (inline style)

```fa
func MakePoint
  take x real
  take y real
  emit point dict
body
  point = obj.set(obj.set(obj.new(), "x", x), "y", y)
  emit point
done
```

### Safe key access

```fa
func GetOrDefault
  take data dict
  take key text
  take default value
  emit result value
body
  if obj.has(data, key)
    result = obj.get(data, key)
  else
    result = default
  done
  emit result
done
```

### Iterating keys

```fa
func PrintAll
  take data dict
  emit count long
body
  keys = obj.keys(data)
  loop keys as k
    val = obj.get(data, k)
    term.print("#{k}: #{to.text(val)}")
  done
  emit list.len(keys)
done
```

### Merging configs

```fa
func MergeConfig
  take base dict
  take overrides dict
  emit config dict
body
  # overrides keys win
  config = obj.merge(base, overrides)
  emit config
done
```

### Deleting a key

```fa
func StripInternal
  take payload dict
  emit clean dict
body
  clean = obj.delete(payload, "_internal")
  clean = obj.delete(clean, "_debug")
  emit clean
done
```

### Transforming dict values

```fa
func UppercaseValues
  take data dict
  emit result dict
body
  result = obj.new()
  keys = obj.keys(data)
  loop keys as k
    val = obj.get(data, k)
    upper = str.upper(val)
    result = obj.set(result, k, upper)
  done
  emit result
done
```

## Common Patterns

### Building dicts from two lists (zip)

```fa
keys = list.split("a,b,c", ",")  # or however keys come in
vals = list.split("1,2,3", ",")
out = obj.new()
idxs = list.indices(keys)
loop idxs as i
  k = keys[i]
  v = vals[i]
  out = obj.set(out, k, v)
done
```

### Patch update

To update one field without touching others, use `obj.set` on the original dict:

```fa
updated = obj.set(original, "status", "active")
```

### Deep merge (manual)

`obj.merge` is shallow — nested dicts are replaced, not merged. For deep merge, iterate and recurse:

```fa
# For a two-level merge:
base_inner = obj.get(base, "settings")
override_inner = obj.get(overrides, "settings")
merged_inner = obj.merge(base_inner, override_inner)
result = obj.set(obj.merge(base, overrides), "settings", merged_inner)
```

## Gotchas

- `obj.get` **raises a runtime error** if the key does not exist. Always call `obj.has` first when the key may be absent, or use a `case`/`if` guard.
- Key comparison is case-sensitive: `"Name"` and `"name"` are different keys.
- `obj.merge(obj1, obj2)` is a **shallow** merge. If both dicts have a key whose value is itself a dict, the entire `obj2` value replaces the `obj1` value — it does not recursively merge.
- `obj.keys` returns keys in insertion order (as stored by the runtime). Do not rely on alphabetical ordering.
- There is no `obj.values` op. To get values, iterate `obj.keys` and call `obj.get` for each key.
- Dicts are immutable. `obj.set` returns a new dict; always reassign the variable.
