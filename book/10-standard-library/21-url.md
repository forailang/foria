# 10.21 — url

The `url.*` namespace provides URL parsing, query string parsing, and percent-encoding utilities. These ops are used when building and consuming URLs programmatically — extracting path components, reading query parameters, or constructing URL-safe strings.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `url.parse` | url | URLParts | Parse a URL into path, query, and fragment |
| `url.query_parse` | query | dict | Parse a query string into a key→value dict; percent-decodes values |
| `url.encode` | text | text | Percent-encode a string (spaces → `%20`) |
| `url.decode` | text | text | Percent-decode a string; `+` is treated as a space |

### URLParts (built-in type)

`url.parse` returns a `URLParts`:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `path` | text | yes | URL path component (e.g., `"/users/42"`) |
| `query` | text | yes | Raw query string without leading `?` (e.g., `"q=foo&page=2"`) |
| `fragment` | text | yes | Fragment identifier without `#` (e.g., `"section-1"`) |

This is a built-in type — use `URLParts` directly in `take`/`emit` declarations without defining it.

Note: `url.parse` returns the **raw query string** — use `url.query_parse` to decode it into a dict.

## Examples

### Parsing a full URL

```fa
func ParseUrl
  take raw_url text
  emit parts dict
body
  parts = url.parse(raw_url)
  emit parts
done
```

For `"https://example.com/search?q=hello+world&page=2#results"`:
- `path` = `"/search"`
- `query` = `"q=hello+world&page=2"`
- `fragment` = `"results"`

### Parsing query parameters

```fa
func ExtractQuery
  take raw_url text
  emit params dict
body
  parts = url.parse(raw_url)
  query_str = obj.get(parts, "query")
  params = url.query_parse(query_str)
  emit params
done
```

For query `"q=hello+world&page=2"`, returns `{"q": "hello world", "page": "2"}`.

### Building a URL with encoded parameters

```fa
func BuildSearchUrl
  take base text
  take query text
  take page long
  emit url text
body
  encoded_query = url.encode(query)
  url = "#{base}?q=#{encoded_query}&page=#{to.text(page)}"
  emit url
done
```

### Reading a specific query parameter

```fa
func GetQueryParam
  take request dict
  take param_name text
  take default text
  emit value text
body
  query = obj.get(request, "query")
  if obj.has(query, param_name)
    value = obj.get(query, param_name)
  else
    value = default
  done
  emit value
done
```

### Constructing a redirect URL

```fa
func BuildRedirect
  take base_url text
  take return_to text
  emit redirect_url text
body
  encoded = url.encode(return_to)
  redirect_url = "#{base_url}?return_to=#{encoded}"
  emit redirect_url
done
```

### Decoding a callback parameter

```fa
func ParseCallback
  take raw_callback text
  emit decoded text
body
  decoded = url.decode(raw_callback)
  emit decoded
done
```

### Full request routing example

```fa
func HandleSearch
  take request dict
  emit response dict
body
  # HTTP server puts parsed query in request.query as a dict
  query_dict = obj.get(request, "query")
  search_term = ""
  if obj.has(query_dict, "q")
    search_term = obj.get(query_dict, "q")
  done
  page = 1
  if obj.has(query_dict, "page")
    page = to.long(obj.get(query_dict, "page"))
  done
  # ... perform search ...
  result = obj.new()
  result = obj.set(result, "term", search_term)
  result = obj.set(result, "page", page)
  response = http.response(200, json.encode(result))
  emit response
done
```

### Encoding a path segment

```fa
func BuildProfileUrl
  take base text
  take username text
  emit profile_url text
body
  # Encode username in case it contains special characters
  encoded_name = url.encode(username)
  profile_url = "#{base}/users/#{encoded_name}"
  emit profile_url
done
```

### Parsing multiple values for the same parameter

Some query strings use repeated keys: `?tag=a&tag=b&tag=c`. `url.query_parse` returns a dict, so only the last value for a repeated key is preserved. Handle this manually:

```fa
func ParseMultiValues
  take query_str text
  take key text
  emit values list
body
  values = list.new()
  pairs = str.split(query_str, "&")
  prefix = "#{key}="
  loop pairs as pair
    if str.starts_with(pair, prefix)
      raw_val = str.slice(pair, str.len(prefix), str.len(pair))
      values = list.append(values, url.decode(raw_val))
    done
  done
  emit values
done
```

## Common Patterns

### Always decode user-provided URL parameters

```fa
raw = obj.get(query, "name")
decoded = url.decode(raw)
safe_name = str.trim(decoded)
```

### Build a query string from a dict

```fa
func BuildQueryString
  take params dict
  emit query text
body
  parts = list.new()
  keys = obj.keys(params)
  loop keys as k
    v = to.text(obj.get(params, k))
    parts = list.append(parts, "#{url.encode(k)}=#{url.encode(v)}")
  done
  query = str.join(parts, "&")
  emit query
done
```

## Gotchas

- `url.parse` returns the **raw** query string (not decoded). Call `url.query_parse` on the `query` field to get a decoded dict.
- `url.decode` treats `+` as a space (HTML form encoding convention). `url.encode` encodes spaces as `%20` (not `+`). This asymmetry is intentional: HTML forms send `+`, but proper URL encoding uses `%20`. Use `str.replace(encoded, " ", "+")` if you need form-style encoding.
- `url.query_parse` percent-decodes both keys and values. Keys in the resulting dict are decoded strings.
- `url.parse` does not validate the URL structure. It extracts the path, query, and fragment by splitting on `?` and `#`. Malformed URLs may produce unexpected results.
- `url.encode` encodes all special characters except letters, digits, `-`, `_`, `.`, and `~` (RFC 3986 unreserved characters). Path separators `/` are encoded — do not use `url.encode` on a full path; encode each segment separately.
