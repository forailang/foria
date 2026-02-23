# 10.19 — time and fmt

This chapter covers two utility namespaces:

- `time.*` — sleeping and splitting decimal hours into hours/minutes/seconds
- `fmt.*` — formatting HMS dicts and wrapping named fields

---

## time.*

The `time.*` namespace provides time-related utilities. For full calendar date operations, see [Chapter 18 — date](18-date.md).

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `time.sleep` | seconds | bool | Async sleep; `seconds` is a real (fractional OK) |
| `time.split_hms` | decimal_hours | dict | Split decimal hours into `{h, m, s}` |

### time.sleep

`time.sleep` pauses execution asynchronously for the given number of seconds. The runtime yields the coroutine — other concurrent tasks continue running during the sleep. Returns `true` when the sleep completes.

The `seconds` argument is a `real`, so fractional values work:
- `time.sleep(0.5)` — 500ms
- `time.sleep(0.1)` — 100ms
- `time.sleep(60)` — 1 minute

### time.split_hms

`time.split_hms` takes a decimal hours value (like `2.5` for 2 hours 30 minutes) and decomposes it into a dict:

| Field | Type | Description |
|-------|------|-------------|
| `h` | long | Whole hours |
| `m` | long | Whole minutes (0–59) |
| `s` | long | Whole seconds (0–59) |

### Examples

#### Simple sleep

```fa
func WaitThenProceed
  take delay_seconds real
  emit ok bool
body
  log.info("Waiting #{to.text(delay_seconds)} seconds...")
  time.sleep(delay_seconds)
  log.info("Done waiting")
  emit true
done
```

#### Retry with delay

```fa
func RetryWithDelay
  take max_attempts long
  take delay_seconds real
  emit attempts long
  fail err dict
body
  attempts = 0
  success = false
  loop list.range(1, max_attempts) as attempt
    if success == false
      attempts = attempt
      resp = http.get("https://api.example.com/health")
      if obj.get(resp, "status") == 200
        success = true
      else
        if attempt < max_attempts
          time.sleep(delay_seconds)
        done
      done
    done
  done
  if success == false
    fail error.new("MAX_RETRIES", "Failed after #{to.text(max_attempts)} attempts")
  done
  emit attempts
done
```

#### Polling a queue

```fa
source PollSource
  emit message dict
body
  loop list.range(1, 999999) as _
    messages = db.query(conn, "SELECT * FROM queue WHERE processed = 0 LIMIT 1")
    if list.len(messages) > 0
      emit messages[0]
    else
      time.sleep(0.5)
    done
  done
done
```

#### Rate limiting

```fa
func RateLimitedFetch
  take urls list
  take delay_ms long
  emit results list
body
  results = list.new()
  delay_s = delay_ms / 1000
  loop urls as url
    resp = http.get(url)
    results = list.append(results, resp)
    time.sleep(delay_s)
  done
  emit results
done
```

#### Splitting decimal hours

```fa
func DecimalToHms
  take decimal_hours real
  emit hms dict
body
  hms = time.split_hms(decimal_hours)
  emit hms
done
```

For example, `time.split_hms(2.75)` returns `{h: 2, m: 45, s: 0}`.

#### Duration display

```fa
func FormatDuration
  take elapsed_ms long
  emit display text
body
  decimal_hours = elapsed_ms / 3600000
  hms = time.split_hms(decimal_hours)
  h = to.long(obj.get(hms, "h"))
  m = to.long(obj.get(hms, "m"))
  s = to.long(obj.get(hms, "s"))
  if h > 0
    emit "#{to.text(h)}h #{to.text(m)}m #{to.text(s)}s"
  else
    if m > 0
      emit "#{to.text(m)}m #{to.text(s)}s"
    else
      emit "#{to.text(s)}s"
    done
  done
done
```

### Gotchas

- `time.sleep` is async — it does not block other concurrent tasks in the same runtime (e.g., other `sync` branches). It does block the current execution path.
- `time.sleep(0)` is valid and yields to the scheduler without waiting — useful for cooperative multitasking.
- `time.split_hms` truncates — it does not round. `time.split_hms(1.9999)` gives `{h: 1, m: 59, s: 59}`, not `{h: 2, m: 0, s: 0}`.
- `time.split_hms` only handles positive values. Negative decimal hours produce undefined behavior.

---

## fmt.*

The `fmt.*` namespace provides small formatting utilities. These are helpers commonly needed when building displays with time values or wrapping data.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `fmt.pad_hms` | hms | text | Format `{h, m, s}` dict as `"HH:MM:SS"` with zero-padding |
| `fmt.wrap_field` | name, val | dict | Return `{name: val}` — a single-key dict |

### Examples

#### Formatting HMS as a clock string

```fa
func ClockDisplay
  take decimal_hours real
  emit clock text
body
  hms = time.split_hms(decimal_hours)
  clock = fmt.pad_hms(hms)
  emit clock
done
```

For `decimal_hours = 9.5`, this returns `"09:30:00"`.

#### Full elapsed time display

```fa
func ElapsedDisplay
  take start_ms long
  take end_ms long
  emit display text
body
  elapsed_ms = end_ms - start_ms
  decimal_hours = elapsed_ms / 3600000
  hms = time.split_hms(decimal_hours)
  display = fmt.pad_hms(hms)
  emit display
done
```

#### Wrapping a result with a field name

```fa
func WrapResult
  take field_name text
  take value value
  emit wrapped dict
body
  wrapped = fmt.wrap_field(field_name, value)
  emit wrapped
done
```

For example, `fmt.wrap_field("status", "ok")` returns `{"status": "ok"}`.

#### Building response envelopes

```fa
func SuccessEnvelope
  take data value
  emit response dict
body
  status = fmt.wrap_field("status", "success")
  payload = fmt.wrap_field("data", data)
  response = obj.merge(status, payload)
  emit response
done
```

#### Timestamped log entry

```fa
func LogEntry
  take level text
  take message text
  emit entry dict
body
  ts_ms = date.to_unix_ms(date.now())
  elapsed_h = ts_ms / 3600000
  hms = time.split_hms(elapsed_h)
  ts_str = fmt.pad_hms(hms)
  entry = obj.new()
  entry = obj.set(entry, "time", ts_str)
  entry = obj.set(entry, "level", level)
  entry = obj.set(entry, "message", message)
  emit entry
done
```

### Gotchas

- `fmt.pad_hms` expects a dict with keys `h`, `m`, and `s` as long values (as returned by `time.split_hms`). Passing a dict with wrong keys or non-integer values raises a runtime error.
- `fmt.pad_hms` always produces a two-digit hour component. If hours exceed 99, the output will be wider than `"HH:MM:SS"` — this is by design for durations longer than 99 hours.
- `fmt.wrap_field(name, val)` is a convenience for creating a single-key dict. It is exactly equivalent to `obj.set(obj.new(), name, val)`.
- There is no `fmt.pad_left` or `fmt.pad_right` general-purpose padding op. Use `str.repeat("0", n)` + `str.slice` for custom zero-padding.
