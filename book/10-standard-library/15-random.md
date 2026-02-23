# 10.15 — random

The `random.*` namespace provides cryptographically-seeded random number generation, UUID v4 generation, and list randomization. The underlying generator is seeded from the OS entropy source — values are not reproducible across runs.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `random.int` | min, max | long | Random integer in the closed interval `[min, max]` |
| `random.float` | | real | Random float in `[0, 1)` |
| `random.uuid` | | text | UUID v4 string (e.g. `"550e8400-e29b-41d4-a716-446655440000"`) |
| `random.choice` | list | value | Uniformly random element from a non-empty list |
| `random.shuffle` | list | list | New list with elements in random order (Fisher-Yates) |

## Examples

### Random integer in a range

```fa
func RollDice
  take sides long
  emit result long
body
  result = random.int(1, sides)
  emit result
done
```

### Simulating a coin flip

```fa
func CoinFlip
  emit side text
body
  n = random.int(0, 1)
  if n == 0
    emit "heads"
  else
    emit "tails"
  done
done
```

### Random float for probability checks

```fa
func ShouldSample
  take rate real
  emit sampled bool
body
  # sample 10% of events: rate = 0.1
  r = random.float()
  sampled = r < rate
  emit sampled
done
```

### Generating UUIDs

```fa
func NewItem
  take name text
  emit item dict
body
  item = obj.new()
  item = obj.set(item, "id", random.uuid())
  item = obj.set(item, "name", name)
  item = obj.set(item, "created_at", date.to_unix_ms(date.now()))
  emit item
done
```

### Picking a random element

```fa
func RandomGreeting
  emit greeting text
body
  options = list.new()
  options = list.append(options, "Hello!")
  options = list.append(options, "Hi there!")
  options = list.append(options, "Good day!")
  options = list.append(options, "Greetings!")
  greeting = random.choice(options)
  emit greeting
done
```

### Shuffling a list

```fa
func ShufflePlaylist
  take tracks list
  emit shuffled list
body
  shuffled = random.shuffle(tracks)
  emit shuffled
done
```

### Random sampling (first N of a shuffled list)

```fa
func SampleN
  take items list
  take n long
  emit sample list
body
  shuffled = random.shuffle(items)
  actual_n = n
  if list.len(items) < n
    actual_n = list.len(items)
  done
  sample = list.slice(shuffled, 0, actual_n)
  emit sample
done
```

### Generating a random token

```fa
func NewToken
  emit token text
body
  # Use UUID for a random token (remove dashes for compactness)
  raw = random.uuid()
  token = str.replace(raw, "-", "")
  emit token
done
```

### Random delay between retries

```fa
func RetryWithJitter
  take base_seconds real
  take jitter_max real
  emit ok bool
body
  jitter = random.float() * jitter_max
  delay = base_seconds + jitter
  time.sleep(delay)
  emit true
done
```

### Random weighted selection (manual)

```fa
func WeightedChoice
  take options list
  take weights list
  emit chosen value
  fail err dict
body
  total = 0
  loop weights as w
    total = total + w
  done
  r = random.float() * total
  cumulative = 0
  chosen = options[0]
  idxs = list.indices(weights)
  loop idxs as i
    w = weights[i]
    cumulative = cumulative + w
    if r <= cumulative
      chosen = options[i]
    done
  done
  emit chosen
done
```

### Generating a random numeric code

```fa
func GenOtp
  take digits long
  emit code text
body
  min_val = to.long(10 ** (digits - 1))
  max_val = to.long(10 ** digits - 1)
  n = random.int(min_val, max_val)
  emit to.text(n)
done
```

## Common Patterns

### Non-repeating sequence

```fa
# Shuffle indices to iterate in random order without repetition
idxs = list.indices(items)
shuffled_idxs = random.shuffle(idxs)
loop shuffled_idxs as i
  item = items[i]
  # process item
done
```

### Randomized test data

```fa
func RandomUser
  emit user dict
body
  names = list.append(list.append(list.new(), "Alice"), "Bob")
  user = obj.new()
  user = obj.set(user, "id", random.uuid())
  user = obj.set(user, "name", random.choice(names))
  user = obj.set(user, "age", random.int(18, 80))
  emit user
done
```

## Gotchas

- `random.int(min, max)` is **inclusive** on both ends. `random.int(1, 6)` can return 1, 2, 3, 4, 5, or 6.
- `random.float` returns values in `[0, 1)` — the result is never exactly 1.0. It can be exactly 0.0.
- `random.choice` on an empty list raises a runtime error. Check `list.len(list) > 0` before calling.
- `random.shuffle` returns a new list — the original is unchanged. Always use the returned value.
- `random.uuid` produces UUID v4 format with hyphens: `xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx`. If you need the UUID without hyphens, use `str.replace(id, "-", "")`.
- The random generator is seeded per-process from OS entropy. There is no way to set a seed for reproducible sequences. For deterministic test data, hard-code values or use a sequential counter instead.
- `random.int` and `random.float` are suitable for simulations and tests. For security-sensitive uses (tokens, OTPs), prefer `crypto.random_bytes` which guarantees cryptographic strength.
