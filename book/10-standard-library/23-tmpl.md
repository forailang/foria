# 10.23 — tmpl (Template Rendering)

The `tmpl.*` namespace provides Mustache-style template rendering. Templates are text strings with `{{variable}}` placeholders that are replaced with values from a data dict. This is useful for generating HTML, email bodies, config files, and any text output where the structure is fixed but the values vary.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `tmpl.render` | template, data | text | Render a Mustache-style template with the given data dict |

## Template Syntax

| Syntax | Description |
|--------|-------------|
| `{{var}}` | HTML-escaped variable substitution |
| `{{{var}}}` | Raw (unescaped) variable substitution |
| `{{#section}}...{{/section}}` | Block section: renders if value is truthy; iterates if value is a list |
| `{{^inverted}}...{{/inverted}}` | Inverted section: renders if value is falsy or missing |
| `{{var.field}}` | Dot-path access into nested dicts |
| `{{.}}` | Current element (inside a list iteration) |

## Examples

### Basic variable substitution

```fa
func Greet
  take name text
  take greeting text
  emit output text
body
  tmpl_str = "{{greeting}}, {{name}}! Welcome to forai."
  data = obj.new()
  data = obj.set(data, "greeting", greeting)
  data = obj.set(data, "name", name)
  output = tmpl.render(tmpl_str, data)
  emit output
done
```

Output: `"Hello, Alice! Welcome to forai."`

### HTML page with auto-escaping

```fa
func RenderProfile
  take user dict
  emit html text
body
  tmpl_str = "<!DOCTYPE html>
<html>
<head><title>{{name}}'s Profile</title></head>
<body>
  <h1>{{name}}</h1>
  <p>Email: {{email}}</p>
  <p>Bio: {{bio}}</p>
</body>
</html>"
  html = tmpl.render(tmpl_str, user)
  emit html
done
```

`{{name}}` auto-escapes HTML special characters — `<script>` in the name becomes `&lt;script&gt;`.

### Raw HTML output (unescaped)

```fa
func RenderWithRaw
  take content text
  take safe_badge_html text
  emit html text
body
  tmpl_str = "<div>{{content}}{{{safe_badge_html}}}</div>"
  data = obj.set(obj.set(obj.new(), "content", content), "safe_badge_html", safe_badge_html)
  html = tmpl.render(tmpl_str, data)
  emit html
done
```

Use `{{{triple braces}}}` only for content you have already verified is safe HTML.

### Conditional sections

```fa
func RenderUserCard
  take user dict
  emit html text
body
  tmpl_str = "<div class=\"user-card\">
  <h2>{{name}}</h2>
  {{#is_admin}}
  <span class=\"badge\">Admin</span>
  {{/is_admin}}
  {{^is_admin}}
  <span class=\"badge\">Member</span>
  {{/is_admin}}
</div>"
  html = tmpl.render(tmpl_str, user)
  emit html
done
```

`{{#is_admin}}...{{/is_admin}}` renders when `is_admin` is truthy.
`{{^is_admin}}...{{/is_admin}}` renders when `is_admin` is falsy or missing.

### List iteration

```fa
func RenderItemList
  take items list
  emit html text
body
  tmpl_str = "<ul>
{{#items}}
  <li>{{name}} — ${{price}}</li>
{{/items}}
</ul>"
  data = obj.set(obj.new(), "items", items)
  html = tmpl.render(tmpl_str, data)
  emit html
done
```

When `items` is a list, `{{#items}}...{{/items}}` iterates over each element. Inside the block, `{{name}}` refers to a field of the current list element.

### Iterating a list of scalars

```fa
func RenderTagList
  take tags list
  emit html text
body
  tmpl_str = "<ul>{{#tags}}<li>{{.}}</li>{{/tags}}</ul>"
  data = obj.set(obj.new(), "tags", tags)
  html = tmpl.render(tmpl_str, data)
  emit html
done
```

`{{.}}` refers to the current list element when iterating a list of scalars (text, numbers).

### Dot-path access into nested dicts

```fa
func RenderAddress
  take person dict
  emit html text
body
  tmpl_str = "<p>{{name}} lives at {{address.street}}, {{address.city}}, {{address.country}}</p>"
  html = tmpl.render(tmpl_str, person)
  emit html
done
```

`person` must have an `address` field that is itself a dict with `street`, `city`, and `country` fields.

### Email template

```fa
func RenderWelcomeEmail
  take user dict
  take confirm_link text
  emit body text
body
  tmpl_str = "Hello {{name}},

Thank you for signing up. Please confirm your email address:

{{confirm_link}}

If you did not sign up, you can safely ignore this email.

Regards,
The Team"
  data = obj.set(user, "confirm_link", confirm_link)
  body = tmpl.render(tmpl_str, data)
  emit body
done
```

### Config file generation

```fa
func RenderNginxConfig
  take hostname text
  take port long
  take root_dir text
  emit config text
body
  tmpl_str = "server {
    listen {{port}};
    server_name {{hostname}};
    root {{root_dir}};
    index index.html;
}"
  data = obj.new()
  data = obj.set(data, "hostname", hostname)
  data = obj.set(data, "port", to.text(port))
  data = obj.set(data, "root_dir", root_dir)
  config = tmpl.render(tmpl_str, data)
  emit config
done
```

### Rendering a table

```fa
func RenderTable
  take rows list
  take columns list
  emit html text
body
  # Build header row first (not easily done in tmpl — build it separately)
  header_cells = list.new()
  loop columns as col
    header_cells = list.append(header_cells, "<th>#{html.escape(col)}</th>")
  done
  header = "<tr>#{str.join(header_cells, "")}</tr>"

  # Render data rows with tmpl
  row_tmpl = "<tr>{{#cols}}<td>{{value}}</td>{{/cols}}</tr>"
  body_rows = list.new()
  loop rows as row
    cols_data = list.new()
    loop columns as col
      val = ""
      if obj.has(row, col)
        val = to.text(obj.get(row, col))
      done
      cols_data = list.append(cols_data, obj.set(obj.new(), "value", val))
    done
    row_html = tmpl.render(row_tmpl, obj.set(obj.new(), "cols", cols_data))
    body_rows = list.append(body_rows, row_html)
  done

  html = "<table>#{header}#{str.join(body_rows, "")}</table>"
  emit html
done
```

### Multiline template from file

```fa
func RenderFromFile
  take tmpl_path text
  take data dict
  emit output text
  fail err dict
body
  if file.exists(tmpl_path) == false
    fail error.new("TMPL_NOT_FOUND", "Template not found: #{tmpl_path}")
  done
  tmpl_str = file.read(tmpl_path)
  output = tmpl.render(tmpl_str, data)
  emit output
done
```

## Common Patterns

### Partials (manual)

tmpl does not support `{{> partial}}` — compose templates manually:

```fa
header = tmpl.render(header_tmpl, data)
body = tmpl.render(body_tmpl, data)
footer = tmpl.render(footer_tmpl, data)
full_page = "#{header}#{body}#{footer}"
```

### Default value for missing field

Use `{{^field}}default{{/field}}` as an inverted section to show a fallback:

```fa
tmpl_str = "Name: {{#name}}{{name}}{{/name}}{{^name}}Anonymous{{/name}}"
```

### Conditional class

```fa
tmpl_str = "<div class=\"item{{#active}} active{{/active}}\">{{name}}</div>"
```

### Number formatting in templates

tmpl renders numbers as-is. Pre-format with `to.text` or `math.round` before passing to the template:

```fa
data = obj.set(data, "price", to.text(math.round(price, 2)))
```

## Gotchas

- `{{var}}` **HTML-escapes** the value. `<`, `>`, `&`, `"`, and `'` are converted to entities. Use `{{{var}}}` (three braces) for raw output.
- Missing keys in the data dict render as empty string `""` — they do not raise an error. Use `{{#key}}...{{/key}}` or `{{^key}}...{{/key}}` to conditionally show content.
- `{{#section}}` with a non-list truthy value renders the block once. With a list, it iterates. With a falsy value or missing key, it renders nothing.
- Dot paths like `{{user.name}}` only work when `user` is a dict in the data. Deeply nested access (more than one level) is supported: `{{a.b.c}}`.
- tmpl does not support `{{> partial}}`, `{{! comments}}`, or lambdas from the full Mustache spec. It implements the core substitution and section features.
- Template strings can contain newlines — use multi-line string literals or load from a file with `file.read`.
- There is no loop index available inside `{{#list}}...{{/list}}`. Build a list of objects with an `index` field if you need positional access.
