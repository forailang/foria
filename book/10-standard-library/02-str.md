# 10.2 — str

The `str.*` namespace provides Unicode-aware string manipulation. All operations treat text as a sequence of Unicode scalar values (characters), not bytes. Indices and lengths are character counts, not byte offsets.

All `str.*` ops are pure and return new values — strings in forai are immutable.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `str.len` | text | long | Character count (Unicode-aware) |
| `str.upper` | text | text | Convert to uppercase |
| `str.lower` | text | text | Convert to lowercase |
| `str.trim` | text | text | Remove leading and trailing whitespace |
| `str.trim_start` | text | text | Remove leading whitespace only |
| `str.trim_end` | text | text | Remove trailing whitespace only |
| `str.split` | text, delim | list | Split by delimiter string; returns list of text |
| `str.join` | list, sep | text | Join list elements with separator |
| `str.replace` | text, from, to | text | Replace all occurrences of `from` with `to` |
| `str.contains` | text, sub | bool | True if `sub` appears anywhere in `text` |
| `str.starts_with` | text, pfx | bool | True if `text` begins with `pfx` |
| `str.ends_with` | text, sfx | bool | True if `text` ends with `sfx` |
| `str.slice` | text, start, end | text | Characters from `start` (inclusive) to `end` (exclusive), clamped to bounds |
| `str.index_of` | text, sub | long | First index of `sub`, or `-1` if not found |
| `str.repeat` | text, n | text | Concatenate `text` with itself `n` times |

## Examples

### Basic manipulation

```fa
func CleanInput
  take raw text
  emit clean text
body
  trimmed = str.trim(raw)
  lower = str.lower(trimmed)
  emit lower
done
```

### Splitting and joining

```fa
func ReverseWords
  take sentence text
  emit result text
body
  words = str.split(sentence, " ")
  # words is a list; reverse by iterating
  out = list.new()
  loop words as word
    out = list.append(out, word)
  done
  emit str.join(out, " ")
done
```

### Searching

```fa
func HasDomain
  take email text
  take domain text
  emit found bool
body
  found = str.contains(email, domain)
  emit found
done
```

### Slicing

```fa
func FirstN
  take text text
  take n long
  emit result text
body
  # str.slice(text, start, end) — end is exclusive, clamped
  result = str.slice(text, 0, n)
  emit result
done
```

### Building CSV rows

```fa
func ToCsv
  take fields list
  emit row text
body
  row = str.join(fields, ",")
  emit row
done
```

### Replace and repeat

```fa
func Redact
  take input text
  take secret text
  emit output text
body
  stars = str.repeat("*", str.len(secret))
  output = str.replace(input, secret, stars)
  emit output
done
```

### Finding positions

```fa
func SplitAtFirst
  take text text
  take sep text
  emit before text
  emit after text
body
  idx = str.index_of(text, sep)
  if idx == -1
    emit before text
    emit after ""
  else
    emit before str.slice(text, 0, idx)
    emit after str.slice(text, idx + str.len(sep), str.len(text))
  done
done
```

## Common Patterns

### Normalize user input

Always trim and lowercase before comparing or storing:

```fa
normalized = str.lower(str.trim(input))
```

### Check prefix for routing

```fa
if str.starts_with(path, "/api/")
  # handle API
done
```

### Build structured keys

```fa
key = str.join(list.append(list.append(list.new(), namespace), name), ":")
# produces "namespace:name"
```

## Gotchas

- `str.len` counts Unicode characters, not bytes. A string containing emoji returns the number of emoji, not the byte count. This is intentional and correct for display-width purposes.
- `str.slice(text, start, end)` is clamped: if `end` exceeds the string length, it is treated as the string length. Negative indices are not supported for `str.slice` (unlike bracket indexing on lists).
- `str.index_of` returns `-1` when the substring is not found. Always check before using the result as an index.
- `str.split` with an empty delimiter `""` is not defined — use `str.slice` in a loop if you need character iteration.
- `str.join` expects a list of text values. Passing a list that contains non-text values (e.g. longs) will fail at runtime — convert with `to.text` first.
- `str.replace` replaces **all** occurrences. There is no single-occurrence variant; use `str.index_of` + `str.slice` if you need to replace only the first match.
