# 10.20 — env (Environment Variables)

The `env.*` namespace provides access to the process environment. Environment variables are the standard way to pass configuration into a forai program without hardcoding values — API keys, database paths, feature flags, and runtime settings all live in the environment.

## Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `env.get` | name [, default] | text | Get env var by name; if missing, returns `default` or empty string |
| `env.set` | name, val | bool | Set env var for the current process |
| `env.has` | name | bool | True if env var exists (even if empty) |
| `env.list` | | dict | All environment variables as a dict |
| `env.remove` | name | bool | Remove env var from the current process |

## Examples

### Reading required config

```fa
func LoadConfig
  emit config dict
  fail err dict
body
  if env.has("DATABASE_URL") == false
    fail error.new("MISSING_ENV", "DATABASE_URL is not set")
  done
  db_url = env.get("DATABASE_URL")
  port_str = env.get("PORT", "8080")
  port = to.long(port_str)
  debug_str = env.get("DEBUG", "false")
  debug = to.bool(debug_str)
  config = obj.new()
  config = obj.set(config, "db_url", db_url)
  config = obj.set(config, "port", port)
  config = obj.set(config, "debug", debug)
  emit config
done
```

### Reading with defaults

```fa
func ServerPort
  emit port long
body
  port_str = env.get("PORT", "8080")
  port = to.long(port_str)
  emit port
done
```

### Checking for optional feature flags

```fa
func IsFeatureEnabled
  take feature text
  emit enabled bool
body
  key = "FEATURE_#{str.upper(feature)}"
  enabled = to.bool(env.get(key, "false"))
  emit enabled
done
```

### Setting env vars for subprocesses

```fa
func RunWithEnv
  take command text
  take args list
  take extra_env dict
  emit result dict
body
  keys = obj.keys(extra_env)
  loop keys as k
    v = obj.get(extra_env, k)
    env.set(k, to.text(v))
  done
  result = exec.run(command, args)
  emit result
done
```

### Listing all env vars

```fa
func PrintEnv
  emit ok bool
body
  all_vars = env.list()
  keys = obj.keys(all_vars)
  loop keys as k
    val = obj.get(all_vars, k)
    term.print("#{k}=#{val}")
  done
  emit true
done
```

### Removing a sensitive env var after use

```fa
func ConsumeSecret
  emit secret text
  fail err dict
body
  if env.has("BOOTSTRAP_SECRET") == false
    fail error.new("NO_SECRET", "BOOTSTRAP_SECRET not set")
  done
  secret = env.get("BOOTSTRAP_SECRET")
  env.remove("BOOTSTRAP_SECRET")   # clear from env after reading
  emit secret
done
```

### Loading a .env file manually

```fa
func LoadDotEnv
  take path text
  emit ok bool
body
  if file.exists(path) == false
    emit false
  else
    content = file.read(path)
    lines = str.split(content, "\n")
    loop lines as line
      trimmed = str.trim(line)
      # Skip comments and blank lines
      if str.len(trimmed) > 0 && str.starts_with(trimmed, "#") == false
        parts = str.split(trimmed, "=")
        if list.len(parts) >= 2
          key = str.trim(parts[0])
          val = str.trim(str.join(list.slice(parts, 1, list.len(parts)), "="))
          env.set(key, val)
        done
      done
    done
    emit true
  done
done
```

## Common Patterns

### Mandatory env var helper

Pattern for asserting required vars at startup:

```fa
jwt_secret = env.get("JWT_SECRET")
if str.len(jwt_secret) == 0
  fail error.new("MISSING_ENV", "JWT_SECRET must be set")
done
```

### Environment-based config selection

```fa
env_name = env.get("FORAI_ENV", "development")
case env_name
  when "production"
    log_level = "warn"
    db_path = "/var/data/prod.db"
  when "staging"
    log_level = "info"
    db_path = "/var/data/staging.db"
  else
    log_level = "debug"
    db_path = ":memory:"
done
```

### Reading a numeric env var

```fa
max_conn_str = env.get("MAX_CONNECTIONS", "10")
max_conn = to.long(max_conn_str)
```

## Gotchas

- `env.get` with no default returns an empty string `""` if the variable is not set — it does not raise an error. Use `env.has` if you need to distinguish "not set" from "set to empty string".
- `env.set` and `env.remove` affect only the current process and any child processes spawned after the change. They do not modify the parent shell or other running processes.
- `env.list` returns all environment variables, which may include sensitive values (secrets, tokens). Never log or emit the full env dict in production code.
- Environment variable names are case-sensitive on Unix/macOS. `PATH` and `path` are different variables. On Windows, they are case-insensitive — but use uppercase consistently for portability.
- `env.get` always returns `text`. Use `to.long`, `to.real`, or `to.bool` to convert to other types.
- Setting an env var to an empty string (`env.set("KEY", "")`) is different from removing it (`env.remove("KEY")`). `env.has("KEY")` returns `true` for an empty string value.
