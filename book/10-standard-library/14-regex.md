# 10.14 — regex (Regular Expressions)

The `regex.*` namespace provides regular expression matching, searching, replacement, and splitting. Patterns follow Rust's `regex` crate syntax, which is largely compatible with PCRE but excludes backtracking features (no lookahead/lookbehind, no backreferences in match — use `replace` patterns for backreferences).

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `regex.match` | pattern, text | bool | True if `pattern` matches anywhere in `text` |
| `regex.find` | pattern, text | dict | First match details: `{matched, text, groups}` |
| `regex.find_all` | pattern, text | list | All match strings (list of text) |
| `regex.replace` | pattern, text, repl | text | Replace first match |
| `regex.replace_all` | pattern, text, repl | text | Replace all matches |
| `regex.split` | pattern, text | list | Split text by pattern |

### Find Result Dict

`regex.find` returns:

| Field | Type | Description |
|-------|------|-------------|
| `matched` | bool | Whether the pattern matched |
| `text` | text | The matched substring |
| `groups` | list | Capture group strings (index 0 = full match, 1+ = groups) |

## Examples

### Testing if a string matches

```fa
func IsEmail
  take input text
  emit valid bool
body
  pattern = "^[a-zA-Z0-9._%+\\-]+@[a-zA-Z0-9.\\-]+\\.[a-zA-Z]{2,}$"
  valid = regex.match(pattern, input)
  emit valid
done
```

### Finding the first match

```fa
func ExtractVersion
  take text text
  emit version text
  fail err dict
body
  result = regex.find("v(\\d+\\.\\d+\\.\\d+)", text)
  if obj.get(result, "matched") == false
    fail error.new("NO_VERSION", "No version found in: #{text}")
  done
  groups = obj.get(result, "groups")
  # groups[0] = full match ("v1.2.3"), groups[1] = captured group ("1.2.3")
  emit groups[1]
done
```

### Finding all matches

```fa
func FindAllUrls
  take text text
  emit urls list
body
  pattern = "https?://[^\\s]+"
  urls = regex.find_all(pattern, text)
  emit urls
done
```

### Replacing text

```fa
func RedactEmail
  take text text
  emit redacted text
body
  pattern = "[a-zA-Z0-9._%+\\-]+@[a-zA-Z0-9.\\-]+\\.[a-zA-Z]{2,}"
  redacted = regex.replace_all(pattern, text, "[REDACTED]")
  emit redacted
done
```

### Replacing with capture group backreferences

```fa
func FormatDate
  take date_text text
  emit formatted text
body
  # Transform "2026-02-22" → "22/02/2026"
  pattern = "(\\d{4})-(\\d{2})-(\\d{2})"
  formatted = regex.replace(pattern, date_text, "$3/$2/$1")
  emit formatted
done
```

### Splitting by pattern

```fa
func SplitWhitespace
  take text text
  emit words list
body
  words = regex.split("\\s+", str.trim(text))
  emit words
done
```

### Splitting by multiple delimiters

```fa
func ParseCsv
  take line text
  emit fields list
body
  # Split on comma, semicolon, or tab
  fields = regex.split("[,;\\t]", line)
  emit fields
done
```

### Extracting all numbers from text

```fa
func ExtractNumbers
  take text text
  emit numbers list
body
  raw_matches = regex.find_all("-?\\d+\\.?\\d*", text)
  numbers = list.new()
  loop raw_matches as m
    numbers = list.append(numbers, to.real(m))
  done
  emit numbers
done
```

### Validating formats

```fa
func ValidatePhone
  take phone text
  emit ok bool
body
  # Match common US phone formats: 555-1234, (555) 123-4567, etc.
  ok = regex.match("^\\(?\\d{3}\\)?[\\s\\-]?\\d{3}[\\s\\-]?\\d{4}$", phone)
  emit ok
done
```

```fa
func ValidateUuid
  take id text
  emit ok bool
body
  pattern = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
  ok = regex.match(pattern, id)
  emit ok
done
```

### Stripping tags from HTML

```fa
func StripHtml
  take html text
  emit plain text
body
  plain = regex.replace_all("<[^>]+>", html, "")
  emit plain
done
```

## Common Patterns

### Named capture groups (Rust regex syntax)

```fa
result = regex.find("(?P<year>\\d{4})-(?P<month>\\d{2})-(?P<day>\\d{2})", text)
groups = obj.get(result, "groups")
# groups[1] = year, groups[2] = month, groups[3] = day (by position)
```

### Case-insensitive matching

Prefix the pattern with `(?i)`:

```fa
ok = regex.match("(?i)error", log_line)
```

### Multiline matching

Use `(?m)` for multiline mode (^ and $ match line boundaries):

```fa
lines = regex.find_all("(?m)^error:.*$", log_text)
```

### Count occurrences

```fa
matches = regex.find_all(pattern, text)
count = list.len(matches)
```

### Check then extract (avoid double matching)

```fa
result = regex.find(pattern, text)
if obj.get(result, "matched")
  value = obj.get(result, "groups")[1]
  # use value
done
```

## Gotchas

- Patterns are matched **anywhere** in the string by `regex.match`, `regex.find`, and `regex.find_all`. To match the whole string, use `^` and `$` anchors: `"^pattern$"`.
- In forai string literals, `\\` is a literal backslash. To write the regex `\d`, use `"\\d"`. To write `\n` in a regex (newline), use `"\\n"`.
- `regex.find_all` returns a list of matched strings — not a list of match dicts. If you need capture groups from multiple matches, call `regex.find` in a loop after progressive `str.slice`.
- `regex.split` on a pattern that matches at the start or end of the string may produce empty string elements at the beginning or end of the result list.
- Backreferences in replacement strings use `$1`, `$2` syntax (not `\1`). `$0` refers to the full match.
- The `regex` crate does not support lookaheads (`(?=...)`) or lookbehinds (`(?<=...)`). If you need these, restructure your logic or use `str.split` + post-processing.
- Very large inputs with complex patterns can be slow. Cache compiled patterns if you are calling the same pattern in a tight loop (the runtime may or may not cache internally — assume it does not).
