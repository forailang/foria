# Chapter 13.5: CLI Tools

forai is well suited for command-line tools: reading input from the terminal or arguments, executing subprocesses, reading and writing files, and dispatching on commands. This chapter covers the common patterns for building CLI tools in forai.

## Reading Command-Line Arguments

Access environment variables (including arguments passed as env vars) via `env.get`:

```fa
func GetConfig
    take nothing as bool
    emit result as dict
    fail error as text
body
    api_url = env.get("API_URL")
    api_key = env.get("API_KEY")
    output_dir = env.get("OUTPUT_DIR")

    config = obj.new()
    config = obj.set(config, "api_url", api_url)
    config = obj.set(config, "api_key", api_key)
    config = obj.set(config, "output_dir", output_dir)
    emit config to :result
done
```

Check whether an env var is set:

```fa
has_debug = env.has("DEBUG")
case has_debug
    when true
        log.debug("Debug mode enabled")
    else
done
```

## Interactive Terminal Loop

For an interactive CLI, use `term.prompt` in a `source`:

```fa
# sources/Prompt.fa
docs Prompt
    Reads user input from the terminal in a REPL loop.
    Stops when the user types "quit" or "exit".
done

source Prompt
    take prompt_text as text
    emit input as text
    fail error as text
body
    on :input from term.prompt(prompt_text) to raw
        trimmed = str.trim(raw)
        emit trimmed
        case trimmed
            when "quit"
                break
            when "exit"
                break
            when ""
        done
    done
done
```

```fa
# main.fa
use sources from "./sources"

docs main
    Interactive CLI tool: reads commands and dispatches them.
done

flow main
body
    step sources.Prompt("cli> " to :prompt_text) then
        next :input to cmd
    done
    step Dispatch(cmd to :cmd) done
done
```

## Command Dispatch with case/when

Parse the command string and dispatch using `case/when`:

```fa
docs Dispatch
    Parses and executes a CLI command.
done

func Dispatch
    take cmd as text
    emit result as bool
    fail error as text
body
    parts = str.split(cmd, " ")
    verb = parts[0]

    result = true
    case verb
        when "help"
            term.print("Commands:")
            term.print("  help             Show this help")
            term.print("  ls [dir]         List files")
            term.print("  cat <file>       Print file contents")
            term.print("  run <cmd>        Run a shell command")
            term.print("  env <var>        Get an environment variable")
            term.print("  quit             Exit")
        when "ls"
            dir = "."
            parts_len = list.len(parts)
            case parts_len
                when 2
                    dir = parts[1]
                else
            done
            files = file.list(dir)
            loop files as f
                term.print(f)
            done
        when "cat"
            filename = parts[1]
            content = file.read(filename)
            term.print(content)
        when "run"
            subcmd = parts[1]
            run_args = list.slice(parts, 2, list.len(parts))
            run_result = exec.run(subcmd, run_args)
            stdout = obj.get(run_result, "stdout")
            term.print(stdout)
        when "env"
            var_name = parts[1]
            val = env.get(var_name)
            term.print(var_name + "=" + val)
        when "quit"
        when ""
        else
            term.print("Unknown command: " + cmd + " (type 'help' for usage)")
    done
    emit result to :result
done
```

## Running Subprocesses

`exec.run(command, args_list)` runs an external program and returns a dict with `stdout`, `stderr`, and `exit_code`:

```fa
docs RunGit
    Runs a git command and returns the output.
done

func RunGit
    take git_args as list
    emit result as dict
    fail error as text
body
    run_result = exec.run("git", git_args)
    exit_code = obj.get(run_result, "exit_code")
    stdout = obj.get(run_result, "stdout")
    stderr = obj.get(run_result, "stderr")

    case exit_code
        when 0
            emit run_result to :result
        else
            emit "git failed: " + stderr to :error
    done
done
```

Calling it:

```fa
args = list.new()
args = list.append(args, "log")
args = list.append(args, "--oneline")
args = list.append(args, "-10")
git_result = RunGit(args to :git_args)
stdout = obj.get(git_result, "stdout")
term.print(stdout)
```

## File I/O

| Op | Description |
|----|-------------|
| `file.read(path)` | Reads file contents as text |
| `file.write(path, content)` | Writes text to a file (overwrites) |
| `file.append(path, content)` | Appends text to a file |
| `file.exists(path)` | Returns `true` if path exists |
| `file.list(dir)` | Returns list of filenames in directory |
| `file.mkdir(path)` | Creates a directory |
| `file.delete(path)` | Deletes a file |
| `file.copy(src, dst)` | Copies a file |
| `file.move(src, dst)` | Moves/renames a file |
| `file.size(path)` | Returns file size in bytes |
| `file.is_dir(path)` | Returns `true` if path is a directory |

A typical file processing tool:

```fa
docs ProcessCSV
    Reads a CSV file, processes each line, writes output.
done

func ProcessCSV
    take input_path as text
    take output_path as text
    emit result as bool
    fail error as text
body
    exists = file.exists(input_path)
    case exists
        when false
            emit "File not found: " + input_path to :error
        else
    done

    content = file.read(input_path)
    lines = str.split(content, "\n")
    output_lines = list.new()

    loop lines as line
        trimmed = str.trim(line)
        case trimmed
            when ""
            else
                cols = str.split(trimmed, ",")
                first = cols[0]
                upper_first = str.upper(first)
                rest = list.slice(cols, 1, list.len(cols))
                joined_rest = str.join(rest, ",")
                out_line = upper_first + "," + joined_rest
                output_lines = list.append(output_lines, out_line)
        done
    done

    output_content = str.join(output_lines, "\n")
    file.write(output_path, output_content)
    emit true to :result
done
```

## Configuration from Environment

A common pattern for CLI tools: read configuration from environment variables with fallback defaults:

```fa
docs LoadConfig
    Reads configuration from environment variables with defaults.
done

func LoadConfig
    take nothing as bool
    emit result as dict
    fail error as text
body
    has_port = env.has("PORT")
    port_str = "8080"
    case has_port
        when true
            port_str = env.get("PORT")
        else
    done

    has_db = env.has("DATABASE_URL")
    db_url = ":memory:"
    case has_db
        when true
            db_url = env.get("DATABASE_URL")
        else
    done

    has_debug = env.has("DEBUG")
    debug_mode = false
    case has_debug
        when true
            debug_mode = true
        else
    done

    config = obj.new()
    config = obj.set(config, "port", to.long(port_str))
    config = obj.set(config, "db_url", db_url)
    config = obj.set(config, "debug", debug_mode)
    emit config to :result
done
```

## Color Output

Use `term.color` for colored terminal output:

```fa
func PrintStatus
    take status as text
    take message as text
    emit result as bool
    fail error as text
body
    case status
        when "ok"
            term.color("green")
            term.print("[OK] " + message)
            term.color("reset")
        when "warn"
            term.color("yellow")
            term.print("[WARN] " + message)
            term.color("reset")
        when "error"
            term.color("red")
            term.print("[ERROR] " + message)
            term.color("reset")
        else
            term.print("[?] " + message)
    done
    emit true to :result
done
```

## Progress Reporting

Use `term.print` with carriage returns for progress bars:

```fa
func ProcessFiles
    take files as list
    emit result as bool
    fail error as text
body
    total = list.len(files)
    count = 0
    loop files as f
        count = count + 1
        progress = to.text(count) + "/" + to.text(total) + " " + f
        term.print("Processing: " + progress)
        content = file.read(f)
        # ... do work ...
    done
    term.print("Done: " + to.text(total) + " files processed")
    emit true to :result
done
```

## Full CLI Tool: File Search

```fa
# main.fa
use sources from "./sources"

docs main
    CLI tool for searching text in files.
    Usage: DEBUG=1 forai run main.fa
done

flow main
body
    step sources.Prompt("search> " to :prompt_text) then
        next :input to query
    done
    step SearchFiles(query to :query) then
        next :result to matches
    done
    step PrintMatches(matches to :matches) done
done
```

```fa
docs SearchFiles
    Searches all files in the current directory for a text pattern.
done

func SearchFiles
    take query as text
    emit result as list
    fail error as text
body
    files = file.list(".")
    matches = list.new()
    loop files as filename
        is_dir = file.is_dir(filename)
        case is_dir
            when true
            else
                content = file.read(filename)
                found = str.contains(content, query)
                case found
                    when true
                        match = obj.new()
                        match = obj.set(match, "file", filename)
                        matches = list.append(matches, match)
                    else
                done
        done
    done
    emit matches to :result
done
```
