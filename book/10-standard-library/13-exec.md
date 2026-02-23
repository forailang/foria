# 10.13 — exec (Process Execution)

The `exec.*` namespace runs external processes and captures their output. It is the forai equivalent of `subprocess` or `child_process` — used for shelling out to system tools, scripts, or other binaries.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `exec.run` | command, args_list | ProcessOutput | Run an external process synchronously |

### ProcessOutput (built-in type)

`exec.run` returns a `ProcessOutput`:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `code` | long | yes | Exit code (0 = success on Unix) |
| `stdout` | text | yes | Standard output captured as text |
| `stderr` | text | yes | Standard error captured as text |
| `ok` | bool | yes | True if exit code is 0 |

This is a built-in type — use `ProcessOutput` directly in `take`/`emit` declarations without defining it. The compiler tracks this return type: passing a `ProcessOutput` to an op that expects a different type (e.g., `db_conn`) is a compile error.

## Important: Command and Args Must Be Separate

The `command` argument is the executable name or path. The `args_list` is a forai list of argument strings. Never pass a shell command string with arguments in `command` — this would fail or behave unexpectedly.

**Correct:**
```fa
result = exec.run("git", list.append(list.append(list.new(), "log"), "--oneline"))
```

**Wrong:**
```fa
result = exec.run("git log --oneline", list.new())  # WRONG — will fail
```

## Examples

### Running a simple command

```fa
func GetGitHash
  emit hash text
  fail err dict
body
  args = list.append(list.new(), "rev-parse")
  args = list.append(args, "HEAD")
  result = exec.run("git", args)
  if obj.get(result, "ok") == false
    fail error.new("GIT_FAILED", obj.get(result, "stderr"))
  done
  emit str.trim(obj.get(result, "stdout"))
done
```

### Listing files with ls

```fa
func ListDir
  take path text
  emit entries list
  fail err dict
body
  args = list.append(list.append(list.new(), "-1"), path)
  result = exec.run("ls", args)
  if obj.get(result, "ok") == false
    fail error.new("LS_FAILED", obj.get(result, "stderr"))
  done
  stdout = obj.get(result, "stdout")
  entries = str.split(str.trim(stdout), "\n")
  emit entries
done
```

### Running a Python script

```fa
func RunScript
  take script_path text
  take arg text
  emit output text
  fail err dict
body
  args = list.append(list.append(list.new(), script_path), arg)
  result = exec.run("python3", args)
  if obj.get(result, "ok") == false
    err_msg = obj.get(result, "stderr")
    fail error.new("SCRIPT_FAILED", "Exit #{to.text(obj.get(result, "code"))}: #{err_msg}")
  done
  emit str.trim(obj.get(result, "stdout"))
done
```

### Checking if a tool is installed

```fa
func HasTool
  take tool text
  emit available bool
body
  args = list.append(list.new(), tool)
  result = exec.run("which", args)
  available = obj.get(result, "ok")
  emit available
done
```

### Running with multiple arguments

```fa
func CompressFile
  take input_path text
  take output_path text
  emit ok bool
  fail err dict
body
  args = list.new()
  args = list.append(args, "-z")
  args = list.append(args, "-c")
  args = list.append(args, input_path)
  result = exec.run("gzip", args)
  if obj.get(result, "ok") == false
    fail error.new("GZIP_FAILED", obj.get(result, "stderr"))
  done
  file.write(output_path, obj.get(result, "stdout"))
  emit true
done
```

### Capturing both stdout and stderr

```fa
func RunAndLog
  take command text
  take args list
  emit result dict
body
  result = exec.run(command, args)
  log.info("Exit code: #{to.text(obj.get(result, "code"))}")
  stdout = obj.get(result, "stdout")
  stderr = obj.get(result, "stderr")
  if str.len(stdout) > 0
    log.debug("stdout: #{stdout}")
  done
  if str.len(stderr) > 0
    log.warn("stderr: #{stderr}")
  done
  emit result
done
```

### Running curl (example of building arg lists)

```fa
func CurlGet
  take url text
  emit body text
  fail err dict
body
  args = list.new()
  args = list.append(args, "-s")         # silent
  args = list.append(args, "-L")         # follow redirects
  args = list.append(args, "--fail")     # fail on HTTP error
  args = list.append(args, url)
  result = exec.run("curl", args)
  if obj.get(result, "ok") == false
    fail error.new("CURL_FAILED", obj.get(result, "stderr"))
  done
  emit obj.get(result, "stdout")
done
```

### Parsing command output as JSON

```fa
func DockerPs
  emit containers list
  fail err dict
body
  args = list.append(list.append(list.new(), "ps"), "--format")
  args = list.append(args, "json")
  result = exec.run("docker", args)
  if obj.get(result, "ok") == false
    fail error.new("DOCKER_FAILED", obj.get(result, "stderr"))
  done
  containers = json.decode(obj.get(result, "stdout"))
  emit containers
done
```

## Common Patterns

### Check exit code explicitly

```fa
result = exec.run("make", list.append(list.new(), "build"))
code = obj.get(result, "code")
case code
  when 0
    log.info("Build succeeded")
  when 1
    log.error("Build failed: #{obj.get(result, "stderr")}")
  else
    log.warn("Unexpected exit code: #{to.text(code)}")
done
```

### Build arg list from a forai list of strings

```fa
func ArgsFromList
  take strs list
  emit args list
body
  args = list.new()
  loop strs as s
    args = list.append(args, s)
  done
  emit args
done
```

### Run and ignore output

```fa
exec.run("notify-send", list.append(list.new(), "Task complete"))
```

## Gotchas

- `exec.run` is **synchronous** and blocking — it waits for the child process to exit before returning. For long-running processes, use `nowait exec.run(...)` to fire and forget.
- The `command` argument is **not a shell command**. It is passed directly to `execve` — no shell expansion, no pipes, no redirects. To use shell features, invoke the shell explicitly: `exec.run("bash", list.append(list.append(list.new(), "-c"), "ls | grep foo"))`.
- Standard input is not available through `exec.run`. If the subprocess needs stdin input, invoke a wrapper script.
- The `stderr` field captures stderr output even when `ok` is true — some tools write warnings or informational messages to stderr even on success.
- Environment variables are inherited from the forai process. Use `env.set` before `exec.run` if the subprocess needs specific env vars.
- On Windows, use `"cmd"` and `"/C"` to run shell commands: `exec.run("cmd", list.append(list.append(list.new(), "/C"), "dir"))`.
- Large stdout/stderr output is captured fully in memory. For processes that produce gigabytes of output, prefer streaming via a pipe (not available in the current `exec.run` interface — use file redirection in a shell wrapper instead).
