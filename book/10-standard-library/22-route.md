# 10.22 — route, url, and html

This chapter covers three namespaces for web application URL and HTML handling:

- `route.*` — URL path pattern matching with named parameters and wildcards
- `url.*` — URL parsing, query string decoding, and percent-encoding (full docs in [Chapter 21](21-url.md))
- `html.*` — HTML entity escaping and unescaping

---

## route.*

The `route.*` namespace matches URL paths against patterns and extracts named parameters. It is the building block for HTTP routing in web applications.

### Pattern Syntax

| Pattern element | Matches | Captured as |
|----------------|---------|-------------|
| `/literal` | Exact segment | (nothing) |
| `/:param` | Any single path segment | `param` key in params dict |
| `/*wildcard` | Any number of remaining segments | `wildcard` key in params dict |

Examples:
- `"/users/:id"` matches `"/users/42"` → `{id: "42"}`
- `"/files/*path"` matches `"/files/a/b/c"` → `{path: "a/b/c"}`
- `"/api/:version/users/:id"` matches `"/api/v2/users/99"` → `{version: "v2", id: "99"}`

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `route.match` | pattern, path | dict | Match path against pattern; returns `{matched, params}` |

### Match Result Dict

| Field | Type | Description |
|-------|------|-------------|
| `matched` | bool | True if the pattern matched |
| `params` | dict | Extracted named parameters (empty dict if no params or no match) |

### Examples

#### Basic route matching

```fa
func MatchUserId
  take path text
  emit user_id text
  fail err dict
body
  result = route.match("/users/:id", path)
  if obj.get(result, "matched") == false
    fail error.new("NO_MATCH", "Path does not match /users/:id")
  done
  params = obj.get(result, "params")
  user_id = obj.get(params, "id")
  emit user_id
done
```

#### Router dispatch

```fa
func Route
  take request dict
  emit response dict
body
  path = obj.get(request, "path")
  method = obj.get(request, "method")

  # Static routes
  if path == "/"
    response = http.response(200, "Welcome")
  done
  if path == "/health"
    response = http.response(200, json.encode(obj.set(obj.new(), "ok", true)))
  done

  # Dynamic routes
  users_match = route.match("/users/:id", path)
  if obj.get(users_match, "matched")
    params = obj.get(users_match, "params")
    user_id = obj.get(params, "id")
    # fetch user and respond
    response = http.response(200, json.encode(obj.set(obj.new(), "id", user_id)))
  done

  posts_match = route.match("/posts/:post_id/comments/:comment_id", path)
  if obj.get(posts_match, "matched")
    params = obj.get(posts_match, "params")
    post_id = obj.get(params, "post_id")
    comment_id = obj.get(params, "comment_id")
    response = http.response(200, json.encode(params))
  done

  # Wildcard route (catch-all for static files)
  files_match = route.match("/static/*filepath", path)
  if obj.get(files_match, "matched")
    params = obj.get(files_match, "params")
    filepath = obj.get(params, "filepath")
    content = file.read("static/#{filepath}")
    response = http.response(200, content)
  done

  emit response
done
```

#### Method + route dispatching

```fa
func ApiRouter
  take request dict
  emit response dict
  fail err dict
body
  path = obj.get(request, "path")
  method = obj.get(request, "method")
  response = http.error_response(404, "NOT_FOUND", "Route not found: #{method} #{path}")

  if method == "GET"
    list_match = route.match("/api/items", path)
    if obj.get(list_match, "matched")
      response = http.response(200, json.encode(list.new()))
    done
    get_match = route.match("/api/items/:id", path)
    if obj.get(get_match, "matched")
      item_id = obj.get(obj.get(get_match, "params"), "id")
      response = http.response(200, json.encode(obj.set(obj.new(), "id", item_id)))
    done
  done

  if method == "POST"
    create_match = route.match("/api/items", path)
    if obj.get(create_match, "matched")
      body = json.decode(obj.get(request, "body"))
      response = http.response(201, json.encode(body))
    done
  done

  if method == "DELETE"
    delete_match = route.match("/api/items/:id", path)
    if obj.get(delete_match, "matched")
      item_id = obj.get(obj.get(delete_match, "params"), "id")
      response = http.response(200, json.encode(obj.set(obj.new(), "deleted", item_id)))
    done
  done

  emit response
done
```

#### Nested resource routing

```fa
func ResourceRoute
  take path text
  emit matched dict
  fail err dict
body
  # Try most specific patterns first
  deep = route.match("/api/:version/users/:user_id/posts/:post_id", path)
  if obj.get(deep, "matched")
    emit obj.get(deep, "params")
  done

  mid = route.match("/api/:version/users/:user_id", path)
  if obj.get(mid, "matched")
    emit obj.get(mid, "params")
  done

  shallow = route.match("/api/:version/users", path)
  if obj.get(shallow, "matched")
    emit obj.get(shallow, "params")
  done

  fail error.new("NO_ROUTE", "No matching route for #{path}")
done
```

#### Wildcard for file serving

```fa
func ServeStatic
  take request dict
  emit response dict
  fail err dict
body
  path = obj.get(request, "path")
  result = route.match("/assets/*filepath", path)
  if obj.get(result, "matched") == false
    fail error.new("NO_MATCH", "Not a static file path")
  done
  filepath = obj.get(obj.get(result, "params"), "filepath")
  full_path = "public/assets/#{filepath}"
  if file.exists(full_path) == false
    emit http.error_response(404, "NOT_FOUND", "File not found")
  else
    content = file.read(full_path)
    emit http.response(200, content)
  done
done
```

### Common Patterns

#### Route table (list of patterns to try in order)

```fa
patterns = list.new()
patterns = list.append(patterns, "/api/users/:id/profile")
patterns = list.append(patterns, "/api/users/:id")
patterns = list.append(patterns, "/api/users")
patterns = list.append(patterns, "/*any")

matched_pattern = ""
matched_params = obj.new()
loop patterns as pat
  if str.len(matched_pattern) == 0
    result = route.match(pat, path)
    if obj.get(result, "matched")
      matched_pattern = pat
      matched_params = obj.get(result, "params")
    done
  done
done
```

### Gotchas

- `route.match` does not differentiate by HTTP method — combine method checking with path matching.
- Patterns are matched in the order you call them. Try more specific patterns before less specific ones.
- `:param` captures exactly one path segment (no `/`). `*wildcard` captures everything including `/`.
- A trailing `/` in the path matters: `"/users/"` does not match `"/users/:id"`.
- `route.match` returns `{matched: false, params: {}}` on no match — it does not raise an error.
- Parameter values in `params` are always `text`. Convert with `to.long` or `to.real` as needed.

---

## url.* (quick reference)

The `url.*` namespace is documented in full in [Chapter 21](21-url.md). Quick reference:

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `url.parse` | url | dict | Parse to `{path, query, fragment}` |
| `url.query_parse` | query | dict | Parse query string to key→value dict |
| `url.encode` | text | text | Percent-encode (spaces → `%20`) |
| `url.decode` | text | text | Percent-decode (`+` → space) |

### Common use with route.*

```fa
path = obj.get(request, "path")
query = obj.get(request, "query")   # already parsed dict from http.server.accept
route_result = route.match("/search/:category", path)
if obj.get(route_result, "matched")
  category = obj.get(obj.get(route_result, "params"), "category")
  q = obj.has(query, "q") ? obj.get(query, "q") : ""
done
```

---

## html.*

The `html.*` namespace escapes and unescapes HTML entities. Always escape user-supplied content before inserting it into HTML output to prevent XSS (cross-site scripting) attacks.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `html.escape` | text | text | Escape `&`, `<`, `>`, `"`, `'` to HTML entities |
| `html.unescape` | text | text | Unescape HTML entities back to characters |

### Entity Mappings

| Character | Escaped form |
|-----------|-------------|
| `&` | `&amp;` |
| `<` | `&lt;` |
| `>` | `&gt;` |
| `"` | `&quot;` |
| `'` | `&#x27;` |

### Examples

#### Escaping user content in HTML

```fa
func SafeHtml
  take user_name text
  take message text
  emit html text
body
  safe_name = html.escape(user_name)
  safe_message = html.escape(message)
  html = "<p><strong>#{safe_name}</strong> says: #{safe_message}</p>"
  emit html
done
```

#### Rendering a table from a list

```fa
func HtmlTable
  take headers list
  take rows list
  emit html text
body
  header_cells = list.new()
  loop headers as h
    header_cells = list.append(header_cells, "<th>#{html.escape(h)}</th>")
  done
  header_row = "<tr>#{str.join(header_cells, "")}</tr>"

  body_rows = list.new()
  loop rows as row
    cells = list.new()
    loop headers as h
      val = to.text(obj.get(row, h))
      cells = list.append(cells, "<td>#{html.escape(val)}</td>")
    done
    body_rows = list.append(body_rows, "<tr>#{str.join(cells, "")}</tr>")
  done

  html = "<table>#{header_row}#{str.join(body_rows, "")}</table>"
  emit html
done
```

#### Unescaping stored HTML entities

```fa
func UnescapeContent
  take escaped text
  emit plain text
body
  plain = html.unescape(escaped)
  emit plain
done
```

#### Generating an HTML list

```fa
func HtmlList
  take items list
  emit html text
body
  lis = list.new()
  loop items as item
    lis = list.append(lis, "<li>#{html.escape(item)}</li>")
  done
  html = "<ul>#{str.join(lis, "")}</ul>"
  emit html
done
```

#### Building a form with escaped defaults

```fa
func SearchForm
  take default_query text
  emit html text
body
  safe_q = html.escape(default_query)
  html = "<form method=\"GET\" action=\"/search\"><input name=\"q\" value=\"#{safe_q}\"></form>"
  emit html
done
```

### When to Use html.escape vs tmpl.render

Use `html.escape` when building HTML by string concatenation. Use `tmpl.render` (see [Chapter 23](23-tmpl.md)) with `{{var}}` (auto-escaping) for template-based rendering.

| Approach | Auto-escapes? | Best for |
|----------|--------------|----------|
| `html.escape` + string interpolation | Manual | Fine-grained control |
| `tmpl.render` `{{var}}` | Yes | Template-based rendering |
| `tmpl.render` `{{{var}}}` | No | Trusted HTML content |

### Common Patterns

#### Escape everything by default

```fa
# Always escape, then selectively render raw HTML with {{{triple}}} in templates
safe = html.escape(user_input)
```

#### Strip then escape

```fa
# Remove surrounding whitespace before escaping
safe = html.escape(str.trim(user_input))
```

### Gotchas

- `html.escape` escapes `'` as `&#x27;`. Some older HTML parsers expect `&apos;` — use `str.replace` if you need a different apostrophe encoding.
- `html.escape` does not escape all possible dangerous characters — it covers the five critical ones (`& < > " '`). For other contexts (CSS, JavaScript), use additional sanitization.
- Never use `html.unescape` on untrusted input and then inject the result into HTML — this would undo your escaping. `html.unescape` is for reading stored entity-encoded text, not for sanitization.
- `html.escape` is not sufficient for escaping content inside `<script>` or `<style>` tags — those contexts have different encoding rules. Use `json.encode` for JavaScript data embedding.
