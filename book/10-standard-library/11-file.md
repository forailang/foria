# 10.11 — file

The `file.*` namespace provides file system operations: reading and writing files, directory management, and file metadata. All paths are strings — relative paths are resolved against the process working directory.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `file.read` | path | text | Read file contents as UTF-8 text |
| `file.write` | path, content | bool | Write (overwrite) file; creates if missing |
| `file.append` | path, content | bool | Append to file; creates if missing |
| `file.delete` | path | bool | Delete file or empty directory |
| `file.exists` | path | bool | True if path exists (file or directory) |
| `file.list` | path | list | List directory entries (filenames only, not full paths) |
| `file.mkdir` | path | bool | Create directory recursively |
| `file.copy` | src, dst | bool | Copy file from `src` to `dst` |
| `file.move` | src, dst | bool | Move/rename file |
| `file.size` | path | long | File size in bytes |
| `file.is_dir` | path | bool | True if path is a directory |

## Examples

### Reading a file

```fa
func ReadConfig
  take path text
  emit config dict
  fail err dict
body
  if file.exists(path) == false
    fail error.new("CONFIG_NOT_FOUND", "File not found: #{path}")
  done
  contents = file.read(path)
  config = json.decode(contents)
  emit config
done
```

### Writing a file

```fa
func SaveOutput
  take path text
  take data dict
  emit ok bool
body
  content = json.encode_pretty(data)
  ok = file.write(path, content)
  emit ok
done
```

### Appending to a log file

```fa
func AppendLog
  take log_path text
  take message text
  emit ok bool
body
  ts = to.text(date.to_unix_ms(date.now()))
  line = "#{ts} #{message}\n"
  ok = file.append(log_path, line)
  emit ok
done
```

### Listing directory contents

```fa
func ListJsonFiles
  take dir_path text
  emit json_files list
body
  all_entries = file.list(dir_path)
  json_files = list.new()
  loop all_entries as entry
    if str.ends_with(entry, ".json")
      full_path = "#{dir_path}/#{entry}"
      json_files = list.append(json_files, full_path)
    done
  done
  emit json_files
done
```

### Creating directories

```fa
func EnsureDir
  take path text
  emit ok bool
body
  if file.exists(path) == false
    ok = file.mkdir(path)
  else
    ok = true
  done
  emit ok
done
```

### Copy and move

```fa
func BackupFile
  take source text
  take backup_dir text
  emit backup_path text
  fail err dict
body
  if file.exists(source) == false
    fail error.new("SOURCE_NOT_FOUND", "File does not exist: #{source}")
  done
  filename = str.split(source, "/")[-1]
  backup_path = "#{backup_dir}/#{filename}.bak"
  file.mkdir(backup_dir)
  file.copy(source, backup_path)
  emit backup_path
done
```

### File size check

```fa
func CheckFileSize
  take path text
  take max_bytes long
  emit ok bool
  fail err dict
body
  if file.exists(path) == false
    fail error.new("NOT_FOUND", "#{path} does not exist")
  done
  size = file.size(path)
  if size > max_bytes
    fail error.new("TOO_LARGE", "File is #{to.text(size)} bytes, max is #{to.text(max_bytes)}")
  done
  emit true
done
```

### Checking if a path is a directory

```fa
func ProcessPath
  take path text
  emit result text
body
  if file.is_dir(path)
    entries = file.list(path)
    result = "Directory with #{to.text(list.len(entries))} entries"
  else
    content = file.read(path)
    result = "File with #{to.text(str.len(content))} chars"
  done
  emit result
done
```

### Processing all files in a directory

```fa
func ProcessDirectory
  take dir text
  emit results list
body
  entries = file.list(dir)
  results = list.new()
  loop entries as name
    path = "#{dir}/#{name}"
    if file.is_dir(path) == false
      content = file.read(path)
      result = obj.set(obj.set(obj.new(), "name", name), "size", file.size(path))
      results = list.append(results, result)
    done
  done
  emit results
done
```

### Atomic write (write then move)

```fa
func AtomicWrite
  take target_path text
  take content text
  emit ok bool
body
  tmp_path = "#{target_path}.tmp"
  file.write(tmp_path, content)
  file.move(tmp_path, target_path)
  emit true
done
```

## Common Patterns

### Config file with defaults

```fa
config_path = "config.json"
if file.exists(config_path)
  raw = file.read(config_path)
  config = json.decode(raw)
else
  config = obj.set(obj.set(obj.new(), "port", 8080), "debug", false)
done
```

### Recursive directory walk (manual)

```fa
# forai does not have built-in recursive directory listing.
# For shallow listings, use file.list + filter by file.is_dir.
entries = file.list(dir)
files = list.new()
subdirs = list.new()
loop entries as e
  full = "#{dir}/#{e}"
  if file.is_dir(full)
    subdirs = list.append(subdirs, full)
  else
    files = list.append(files, full)
  done
done
```

### Ensure output directory exists before writing

```fa
output_dir = "output"
file.mkdir(output_dir)
file.write("#{output_dir}/result.json", json.encode(data))
```

## Gotchas

- `file.read` reads the entire file into memory as a text string. Do not use it on very large binary files — it may fail or corrupt data for non-UTF-8 content.
- `file.write` **overwrites** the file if it exists. Use `file.append` to add to an existing file, or `file.exists` + a branch to decide.
- `file.delete` raises a runtime error if the path does not exist or if it is a non-empty directory. Check `file.exists` and `file.is_dir` first.
- `file.list` returns filenames only (not full paths). Construct full paths with `"#{dir}/#{entry}"`.
- `file.mkdir` creates all intermediate directories (like `mkdir -p`). It does not fail if the directory already exists.
- `file.move` across filesystems may fail on some operating systems. If the source and destination are on different volumes, use `file.copy` followed by `file.delete`.
- Relative paths are resolved from the process working directory, which is typically where `forai` was invoked — not the location of the `.fa` file.
