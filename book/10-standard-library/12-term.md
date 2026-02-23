# 10.12 — term (Terminal I/O)

The `term.*` namespace provides terminal input and output operations including printing, prompting, cursor control, color, and keypress reading. These ops directly interact with the process's stdin/stdout and are designed for building interactive command-line programs.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `term.print` | text | bool | Write text to stdout (with newline) |
| `term.prompt` | text | text | Write prompt to stdout, read a line from stdin |
| `term.clear` | | bool | Clear the terminal screen |
| `term.size` | | dict | Terminal dimensions `{cols, rows}` |
| `term.cursor` | | dict | Current cursor position `{col, row}` |
| `term.move_to` | col, row | bool | Move cursor to absolute position |
| `term.color` | text, color | text | Return ANSI-colored version of text |
| `term.read_key` | | text | Read a single keypress (raw mode) |

### Color Names

`term.color(text, color)` accepts these color name strings:

| Color | Description |
|-------|-------------|
| `"red"` | Red foreground |
| `"green"` | Green foreground |
| `"blue"` | Blue foreground |
| `"yellow"` | Yellow foreground |
| `"cyan"` | Cyan foreground |
| `"magenta"` | Magenta foreground |
| `"white"` | White foreground |
| `"black"` | Black foreground |
| `"bold"` | Bold text |
| `"dim"` | Dim text |
| `"reset"` | Reset all attributes |

## Examples

### Printing output

```fa
func PrintSummary
  take items list
  emit ok bool
body
  term.print("Summary")
  term.print(str.repeat("-", 40))
  loop items as item
    name = obj.get(item, "name")
    value = obj.get(item, "value")
    term.print("  #{name}: #{to.text(value)}")
  done
  term.print(str.repeat("-", 40))
  term.print("Total: #{to.text(list.len(items))} items")
  emit true
done
```

### Reading user input

```fa
func AskForName
  emit name text
body
  name = term.prompt("Enter your name: ")
  name = str.trim(name)
  emit name
done
```

### Confirmation prompt

```fa
func Confirm
  take question text
  emit confirmed bool
body
  answer = term.prompt("#{question} [y/N]: ")
  normalized = str.lower(str.trim(answer))
  confirmed = normalized == "y" || normalized == "yes"
  emit confirmed
done
```

### Colored output

```fa
func PrintStatus
  take message text
  take level text
  emit ok bool
body
  colored = message
  if level == "error"
    colored = term.color(message, "red")
  done
  if level == "success"
    colored = term.color(message, "green")
  done
  if level == "warning"
    colored = term.color(message, "yellow")
  done
  term.print(colored)
  emit true
done
```

### Interactive menu

```fa
func ShowMenu
  take options list
  emit choice long
body
  term.print(term.color("Select an option:", "bold"))
  idxs = list.indices(options)
  loop idxs as i
    opt = options[i]
    term.print("  #{to.text(i + 1)}. #{opt}")
  done
  raw = term.prompt("Choice: ")
  choice = to.long(str.trim(raw))
  emit choice
done
```

### Terminal dimensions

```fa
func PrintCentered
  take text text
  emit ok bool
body
  sz = term.size()
  cols = to.long(obj.get(sz, "cols"))
  text_len = str.len(text)
  padding = math.floor((cols - text_len) / 2)
  padded = "#{str.repeat(" ", padding)}#{text}"
  term.print(padded)
  emit true
done
```

### Cursor control

```fa
func DrawBox
  take col long
  take row long
  take width long
  take height long
  emit ok bool
body
  term.move_to(col, row)
  term.print(str.repeat("-", width))
  idxs = list.range(1, height - 2)
  loop idxs as r
    term.move_to(col, row + r)
    term.print("|#{str.repeat(" ", width - 2)}|")
  done
  term.move_to(col, row + height - 1)
  term.print(str.repeat("-", width))
  emit true
done
```

### Reading a single keypress

```fa
func WaitForKey
  take expected_key text
  emit ok bool
body
  term.print("Press #{expected_key} to continue...")
  loop list.range(1, 999) as _
    key = term.read_key()
    if key == expected_key
      break
    done
  done
  emit true
done
```

### Clearing the screen

```fa
func RefreshDisplay
  take data dict
  emit ok bool
body
  term.clear()
  term.move_to(0, 0)
  term.print(term.color("Dashboard", "bold"))
  term.print(json.encode_pretty(data))
  emit true
done
```

## Common Patterns

### Progress indicator

```fa
func RunWithProgress
  take total long
  emit ok bool
body
  done_count = 0
  items = list.range(1, total)
  loop items as i
    # ... do work ...
    done_count = done_count + 1
    pct = math.round(done_count / total * 100, 0)
    term.print("Progress: #{to.text(pct)}%")
  done
  emit true
done
```

### Password prompt (no echo)

`term.prompt` echoes input. For passwords, warn the user or use `term.read_key` in a loop:

```fa
term.print("Password (input visible): ")
password = term.prompt("")
```

### Multi-line output with separators

```fa
term.print(str.repeat("=", 60))
term.print(term.color("REPORT", "bold"))
term.print(str.repeat("=", 60))
```

## Gotchas

- `term.print` always appends a newline. There is no `print_no_newline` variant — use `term.move_to` for cursor positioning if you need to overwrite the same line.
- `term.prompt` blocks until the user presses Enter. It echoes input — it is not suitable for password entry.
- `term.read_key` reads one character at a time in raw mode. Special keys (arrows, function keys) may produce multi-character escape sequences like `"\x1b[A"` — handle these explicitly if you support arrow key navigation.
- `term.color` returns an ANSI escape sequence string. If stdout is redirected to a file, the escape sequences will appear literally. Check `term.size` — if it returns `{cols: 0, rows: 0}`, you are likely not in a terminal.
- `term.clear` clears the visible terminal window, not the scroll buffer. The user can still scroll up to see previous output.
- `term.move_to` uses (col, row) with zero-based coordinates starting at the top-left corner of the terminal.
