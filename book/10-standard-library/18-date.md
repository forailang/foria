# 10.18 — date, stamp, and trange

This chapter covers three time-related namespaces:

- `date.*` — calendar date operations (year/month/day/hour/minute/second)
- `stamp.*` — monotonic nanosecond timestamps (for performance measurement and ordering)
- `trange.*` — time range construction and querying

---

## date.*

The `date.*` namespace works with calendar dates stored as `Date` values. All dates include a Unix millisecond timestamp and a timezone offset in minutes. Dates are immutable — all arithmetic ops return new `Date` values.

### Date (built-in type)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `unix_ms` | long | yes | Unix timestamp in milliseconds |
| `tz_offset_min` | long | yes | Timezone offset in minutes from UTC (e.g., -300 for UTC-5) |

This is a built-in type — use `Date` directly in `take`/`emit` declarations without defining it. Use `date.to_parts` to decompose into human-readable fields.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `date.now` | | Date | Current UTC date |
| `date.now_tz` | offset | Date | Current date with timezone offset (minutes) |
| `date.from_unix_ms` | ms | Date | Construct from Unix milliseconds |
| `date.from_parts` | y, mo, d, h, mi, s, ms | Date | Construct from components (UTC) |
| `date.from_parts_tz` | y, mo, d, h, mi, s, ms, tz | Date | Construct from components with tz offset |
| `date.from_iso` | text | Date | Parse ISO 8601 string |
| `date.from_epoch` | date, offset_ms | Date | Add millisecond offset to a date |
| `date.to_unix_ms` | date | long | Extract Unix milliseconds |
| `date.to_parts` | date | dict | Decompose: `{year, month, day, hour, min, sec, ms, tz_offset_min}` |
| `date.to_iso` | date | text | Format as ISO 8601 string |
| `date.to_epoch` | date1, date2 | long | Difference in ms between two dates |
| `date.weekday` | date | long | ISO weekday: 1=Monday, 7=Sunday |
| `date.with_tz` | date, offset | Date | Change timezone offset, preserving the instant |
| `date.add` | date, ms | Date | Add milliseconds to a date |
| `date.add_days` | date, days | Date | Add N calendar days |
| `date.diff` | date1, date2 | long | Difference in ms (`date1 - date2`) |
| `date.compare` | date1, date2 | long | Returns -1, 0, or 1 |

### Examples

#### Getting the current date

```fa
func CurrentTimestamp
  emit unix_ms long
body
  now = date.now()
  unix_ms = date.to_unix_ms(now)
  emit unix_ms
done
```

#### Formatting a date

```fa
func FormatDate
  take date_obj dict
  emit formatted text
body
  parts = date.to_parts(date_obj)
  y = to.text(obj.get(parts, "year"))
  mo = to.text(obj.get(parts, "month"))
  d = to.text(obj.get(parts, "day"))
  # Zero-pad month and day
  if str.len(mo) == 1
    mo = "0#{mo}"
  done
  if str.len(d) == 1
    d = "0#{d}"
  done
  emit "#{y}-#{mo}-#{d}"
done
```

#### Parsing an ISO date string

```fa
func ParseIso
  take iso text
  emit date_obj dict
  fail err dict
body
  date_obj = date.from_iso(iso)
  emit date_obj
done
```

Example ISO formats: `"2026-02-22"`, `"2026-02-22T14:30:00Z"`, `"2026-02-22T14:30:00+05:00"`.

#### Date arithmetic

```fa
func AddDays
  take start_iso text
  take days long
  emit result_iso text
body
  start = date.from_iso(start_iso)
  result = date.add_days(start, days)
  result_iso = date.to_iso(result)
  emit result_iso
done
```

#### Calculating age in years

```fa
func AgeInYears
  take birth_iso text
  emit age long
body
  birth = date.from_iso(birth_iso)
  now = date.now()
  diff_ms = date.diff(now, birth)
  ms_per_year = 31536000000
  age = math.floor(diff_ms / ms_per_year)
  emit age
done
```

#### Days between two dates

```fa
func DaysBetween
  take start_iso text
  take end_iso text
  emit days long
body
  start = date.from_iso(start_iso)
  end_date = date.from_iso(end_iso)
  diff_ms = date.diff(end_date, start)
  days = math.floor(diff_ms / 86400000)
  emit days
done
```

#### Comparing dates

```fa
func IsAfter
  take date1_iso text
  take date2_iso text
  emit result bool
body
  d1 = date.from_iso(date1_iso)
  d2 = date.from_iso(date2_iso)
  cmp = date.compare(d1, d2)
  result = cmp > 0
  emit result
done
```

#### Working with timezones

```fa
func ToNewYork
  take utc_date dict
  emit ny_date dict
body
  # UTC-5 = -300 minutes (EST) or UTC-4 = -240 (EDT)
  ny_date = date.with_tz(utc_date, -300)
  emit ny_date
done
```

#### Weekday check

```fa
func IsWeekend
  take date_obj dict
  emit is_weekend bool
body
  day = date.weekday(date_obj)
  # 6=Saturday, 7=Sunday
  is_weekend = day >= 6
  emit is_weekend
done
```

#### Constructing a date from parts

```fa
func NewDate
  take year long
  take month long
  take day long
  emit date_obj dict
body
  date_obj = date.from_parts(year, month, day, 0, 0, 0, 0)
  emit date_obj
done
```

### Common Patterns

#### Store dates as Unix ms in the database

```fa
# Write
unix_ms = date.to_unix_ms(date.now())
params = list.append(list.append(list.new(), id), unix_ms)
db.exec(conn, "INSERT INTO events (id, created_at) VALUES (?, ?)", params)

# Read back
rows = db.query(conn, "SELECT created_at FROM events WHERE id = ?", list.append(list.new(), id))
ts = to.long(obj.get(rows[0], "created_at"))
date_obj = date.from_unix_ms(ts)
```

#### Check if a date is in the future

```fa
now = date.to_unix_ms(date.now())
expires = to.long(obj.get(token_payload, "exp"))
expired = now > expires
```

### Gotchas

- `date.diff(date1, date2)` returns `date1 - date2` in milliseconds. The sign matters: `diff(later, earlier)` is positive.
- `date.to_parts` returns fields named `year`, `month`, `day`, `hour`, `min`, `sec`, `ms` — note `min` (not `minute`) and `sec` (not `second`).
- `date.from_iso` accepts both date-only (`"2026-02-22"`) and datetime strings. Date-only strings are interpreted as midnight UTC.
- Timezone arithmetic via `date.with_tz` changes the display offset but not the underlying `unix_ms` — the instant in time is preserved.
- `date.add_days` adds calendar days, not 24-hour periods. This matters near DST transitions — use `date.add` with `86400000` for strict 24-hour increments.

---

## stamp.*

The `stamp.*` namespace provides monotonic nanosecond timestamps. Unlike `date.*`, stamps are not tied to wall clock time — they are useful for measuring elapsed time, ordering events, and benchmarking.

### Stamp (built-in type)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `ns` | long | yes | Monotonic nanosecond timestamp |

This is a built-in type — use `Stamp` directly in `take`/`emit` declarations without defining it.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `stamp.now` | | Stamp | Current time |
| `stamp.from_ns` | ns | Stamp | Construct from nanoseconds |
| `stamp.from_epoch` | stamp, offset_ns | Stamp | Epoch + nanosecond offset |
| `stamp.to_ns` | stamp | long | Extract ns value |
| `stamp.to_ms` | stamp | long | Convert to milliseconds |
| `stamp.to_date` | stamp | Date | Convert to a Date |
| `stamp.to_epoch` | stamp1, stamp2 | long | Difference from epoch in ns |
| `stamp.add` | stamp, ns | Stamp | Add nanoseconds |
| `stamp.diff` | stamp1, stamp2 | long | Difference in ns (`stamp1 - stamp2`) |
| `stamp.compare` | stamp1, stamp2 | long | Returns -1, 0, or 1 |

### Examples

#### Measuring function duration

```fa
func TimedOperation
  take data list
  emit duration_ms real
body
  start = stamp.now()
  # ... process data ...
  end = stamp.now()
  elapsed_ns = stamp.diff(end, start)
  duration_ms = elapsed_ns / 1000000
  emit duration_ms
done
```

#### Benchmarking

```fa
func Benchmark
  take iterations long
  emit ops_per_second real
body
  start = stamp.now()
  loop list.range(1, iterations) as _
    # ... operation to benchmark ...
    hash.sha256("test data")
  done
  end = stamp.now()
  elapsed_ns = stamp.diff(end, start)
  elapsed_s = elapsed_ns / 1000000000
  ops_per_second = iterations / elapsed_s
  emit ops_per_second
done
```

#### Ordering events by timestamp

```fa
func NewEvent
  take name text
  emit event dict
body
  event = obj.new()
  event = obj.set(event, "name", name)
  event = obj.set(event, "ts", stamp.to_ns(stamp.now()))
  emit event
done
```

#### Converting stamp to date

```fa
func StampToIso
  take stamp_ns long
  emit iso text
body
  s = stamp.from_ns(stamp_ns)
  d = stamp.to_date(s)
  iso = date.to_iso(d)
  emit iso
done
```

### Gotchas

- `stamp.diff(s1, s2)` returns `s1 - s2` in nanoseconds. Elapsed time = `stamp.diff(end, start)`.
- `stamp.to_ms` returns the millisecond conversion, truncated (not rounded).
- Stamps are monotonic: they always increase within a process. They are not synchronized with wall clock time — do not use them as user-visible timestamps.

---

## trange.*

The `trange.*` namespace represents a time range between two `Date` values. Useful for scheduling, event overlap detection, and date filtering.

### TimeRange (built-in type)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `start` | Date | yes | Start of the range |
| `end` | Date | yes | End of the range |

This is a built-in type — use `TimeRange` directly in `take`/`emit` declarations without defining it.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `trange.new` | start, end | TimeRange | Create range; validates `start <= end` |
| `trange.start` | range | Date | Extract start date |
| `trange.end` | range | Date | Extract end date |
| `trange.duration_ms` | range | long | Duration in milliseconds |
| `trange.contains` | range, date | bool | True if `start <= date <= end` (inclusive) |
| `trange.overlaps` | range1, range2 | bool | True if the ranges share any common instant |
| `trange.shift` | range, ms | TimeRange | Shift both start and end by `ms` milliseconds |

### Examples

#### Creating a time range

```fa
func TodayRange
  emit range dict
body
  now = date.now()
  parts = date.to_parts(now)
  y = to.long(obj.get(parts, "year"))
  mo = to.long(obj.get(parts, "month"))
  d = to.long(obj.get(parts, "day"))
  start = date.from_parts(y, mo, d, 0, 0, 0, 0)
  end_date = date.from_parts(y, mo, d, 23, 59, 59, 999)
  range = trange.new(start, end_date)
  emit range
done
```

#### Checking if an event falls within a range

```fa
func EventInRange
  take event_ts long
  take range dict
  emit in_range bool
body
  event_date = date.from_unix_ms(event_ts)
  in_range = trange.contains(range, event_date)
  emit in_range
done
```

#### Checking for overlapping bookings

```fa
func HasConflict
  take booking1 dict
  take booking2 dict
  emit conflict bool
body
  start1 = date.from_iso(obj.get(booking1, "start"))
  end1 = date.from_iso(obj.get(booking1, "end"))
  start2 = date.from_iso(obj.get(booking2, "start"))
  end2 = date.from_iso(obj.get(booking2, "end"))
  range1 = trange.new(start1, end1)
  range2 = trange.new(start2, end2)
  conflict = trange.overlaps(range1, range2)
  emit conflict
done
```

#### Duration in hours

```fa
func DurationHours
  take range dict
  emit hours real
body
  ms = trange.duration_ms(range)
  hours = ms / 3600000
  emit hours
done
```

#### Shifting a schedule forward

```fa
func ShiftSchedule
  take range dict
  take days long
  emit shifted dict
body
  ms = days * 86400000
  shifted = trange.shift(range, ms)
  emit shifted
done
```

#### Filtering a list of events within a range

```fa
func FilterByRange
  take events list
  take range dict
  emit filtered list
body
  filtered = list.new()
  loop events as event
    ts = to.long(obj.get(event, "timestamp"))
    event_date = date.from_unix_ms(ts)
    if trange.contains(range, event_date)
      filtered = list.append(filtered, event)
    done
  done
  emit filtered
done
```

### Gotchas

- `trange.new` raises a runtime error if `start` is after `end`. Always ensure ordering before constructing a range.
- `trange.contains` uses **inclusive** bounds on both start and end.
- `trange.overlaps` returns true even if the ranges only share a single instant (boundary touch).
- `trange.shift` shifts both bounds by the same amount — it does not change the duration.
- There is no `trange.split` or `trange.intersect` op. Compute intersections manually using `date.compare` on the start and end bounds.
