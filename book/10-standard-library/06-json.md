# 10.6 — json and codec

The `json.*` namespace provides JSON serialization and deserialization. The `codec.*` namespace is a generalized version that dispatches by format name — currently JSON is the only built-in codec, but `codec.*` is the extension point for future formats.

## json.*

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `json.decode` | text | value | Parse a JSON string into a forai value |
| `json.encode` | val | text | Serialize a forai value to compact JSON |
| `json.encode_pretty` | val | text | Serialize with indentation and newlines |

### Examples

#### Decoding an API response body

```fa
func ParseResponse
  take body text
  emit data dict
  fail err dict
body
  data = json.decode(body)
  emit data
done
```

#### Building and serializing a payload

```fa
func BuildPayload
  take user_id text
  take action text
  emit payload text
body
  obj = obj.set(obj.set(obj.new(), "user_id", user_id), "action", action)
  payload = json.encode(obj)
  emit payload
done
```

#### Pretty-printing for debug output

```fa
func DebugValue
  take val value
  emit formatted text
body
  formatted = json.encode_pretty(val)
  emit formatted
done
```

#### Round-trip (decode then re-encode)

```fa
func NormalizeJson
  take input text
  emit normalized text
body
  parsed = json.decode(input)
  normalized = json.encode(parsed)
  emit normalized
done
```

#### Serializing a list

```fa
func ToJsonArray
  take items list
  emit json_text text
body
  json_text = json.encode(items)
  emit json_text
done
```

### Common Patterns

#### Parse and extract a field

```fa
parsed = json.decode(body)
user_id = obj.get(parsed, "user_id")
```

#### Embed JSON in a template

```fa
data_json = json.encode(data)
html = tmpl.render("<script>window.__data__ = {{{data_json}}};</script>", obj.set(obj.new(), "data_json", data_json))
```

#### Return JSON from an HTTP handler

```fa
body = json.encode(result)
response = http.response(200, body)
```

### Gotchas

- `json.decode` will raise a runtime error on invalid JSON. Wrap with error handling if the input is untrusted.
- JSON integers are decoded as `long`, JSON floats as `real`, JSON strings as `text`, JSON booleans as `bool`, JSON arrays as `list`, and JSON objects as `dict`. JSON `null` becomes `void`.
- `json.encode` on a forai `void` value produces `"null"`.
- `json.encode_pretty` uses 2-space indentation. The exact format (spaces vs. tabs, indentation amount) is implementation-defined — do not parse the whitespace.
- Handle types (`db_conn`, `ws_conn`, etc.) cannot be serialized with `json.encode`. Attempting to do so raises a runtime error.

---

## codec.*

The `codec.*` namespace generalizes encoding/decoding to be format-parameterized. The format is specified as the first argument.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `codec.decode` | format, text | value | Decode `text` using the named format |
| `codec.encode` | format, val | text | Encode `val` using the named format |
| `codec.encode_pretty` | format, val | text | Pretty-encode `val` using the named format |

Currently supported formats:

| Format string | Description |
|--------------|-------------|
| `"json"` | JSON — same as `json.*` ops |

### Examples

#### Using codec for format-agnostic processing

```fa
func DecodeAny
  take format text
  take raw text
  emit parsed value
body
  parsed = codec.decode(format, raw)
  emit parsed
done
```

#### Dynamic encoding

```fa
func EncodeOutput
  take format text
  take data value
  emit output text
body
  output = codec.encode(format, data)
  emit output
done
```

### Gotchas

- `codec.decode` with an unknown format name raises a runtime error. Check `format` before calling if it comes from user input.
- `codec.*` and `json.*` are functionally equivalent for JSON — use whichever reads more clearly.
