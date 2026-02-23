# Chapter 12.2: Host Functions

When an `extern func` is declared in forai source, the host program — the Rust application embedding the forai runtime — must register a matching implementation. This chapter describes how host programs register implementations, how import indices are assigned, and how to write test stubs for extern funcs.

## The Import Index System

The forai runtime assigns a numeric import index to every host function. These indices are used internally by the compiled WASM module to call host-provided operations. The index space is allocated in this order:

1. **Core runtime imports** (indices 0–12): Built-in ops like `print`, `print_i64`, `read_line`, `random`, `time_ms`, and channel operations. These are always present.

2. **Standard library imports** (indices 13 onward): Ops from the standard library modules (`db.*`, `http.*`, `file.*`, etc.). The standard library occupies a fixed block of indices. As of the current release, `STD_IMPORT_COUNT = 19`, so std imports span indices 13–31.

3. **Extern func imports** (indices 32+): Each `extern func` declaration in the compiled source is assigned the next available index after the std block. Indices are assigned in the order the compiler encounters extern func declarations during module loading.

This means: if your program has two extern funcs `CheckLicense` and `SendSMS`, and `CheckLicense` is discovered first during compilation, `CheckLicense` gets index 32 and `SendSMS` gets index 33.

## Registering Host Implementations in Rust

Inside `src/runtime.rs`, host functions are registered in `register_runtime()`. The registration ties a function name to its import index and its Rust implementation:

```rust
// Example (Rust pseudocode — not forai syntax)
fn register_runtime(store: &mut Store, linker: &mut Linker) {
    // Core imports at indices 0–12 ...

    // Extern func: CheckLicense at index 32
    linker.func_wrap("env", "check_license", |key: i32, key_len: i32| -> i32 {
        // decode key from WASM memory
        // call external licensing API
        // return 1 for valid, 0 for invalid
        1
    }).unwrap();

    // Extern func: SendSMS at index 33
    linker.func_wrap("env", "send_sms", |phone: i32, phone_len: i32, msg: i32, msg_len: i32| -> i32 {
        // decode phone and message from WASM memory
        // call SMS gateway
        // return result JSON encoded into WASM memory
        0
    }).unwrap();
}
```

The function name in `linker.func_wrap` must match the name produced by the forai compiler for that extern func. The compiler converts the forai func name (e.g. `CheckLicense`) to its canonical import name using a `std_fn_to_import()` style mapping.

## Test Stubs

For unit testing (both `forai test` and Rust unit tests), the runtime uses `register_test_stubs()` instead of `register_runtime()`. This function registers lightweight stub implementations that return safe default values without performing real I/O:

```rust
fn register_test_stubs(store: &mut Store, linker: &mut Linker) {
    // Stub for CheckLicense — always returns true
    linker.func_wrap("env", "check_license", |_key: i32, _key_len: i32| -> i32 {
        1  // always valid in tests
    }).unwrap();

    // Stub for SendSMS — always returns delivered: true
    linker.func_wrap("env", "send_sms", |_phone: i32, _phone_len: i32, _msg: i32, _msg_len: i32| -> i32 {
        // write {"delivered": true} into WASM memory
        0
    }).unwrap();
}
```

When `forai test` runs, it uses these stubs so extern funcs do not require real credentials or external services.

## How the Compiler Tracks extern func Imports

In `src/std_modules.rs` and `src/codegen.rs`, extern funcs are tracked alongside standard library imports:

- The compiler scans each `.fa` file for `extern func` declarations.
- Each extern func is added to the `runtime_builtins` map with its assigned import index.
- In the generated code, calls to extern funcs follow the same path as calls to built-in ops: the compiler emits a `Call` instruction with the import index.
- Extern funcs are excluded from `all_funcs` (the list of forai-implemented functions) — they have no forai body to compile.

The `std_fn_to_import()` mapping in `std_modules.rs` handles the name translation from forai func names to their WASM import names.

## Struct Return Types from extern func

If an `extern func` emits a named struct type, the forai codegen may need special handling to assemble the struct from individual scalar return values (the WASM calling convention only supports scalar types directly). This is handled by `compile_std_import_function()` in `src/codegen.rs`, which generates code to call multiple accessor functions that retrieve individual fields from the result.

For example, if `extern func GetToken` emits a `TokenResult` with fields `token text` and `expires_at long`, the host might register:
- `get_token` — runs the operation, caches the result
- `get_token_token` — returns the `token` field as a string
- `get_token_expires_at` — returns the `expires_at` field as an integer

The codegen assembles these into a `TokenResult` struct on the forai side. This is the same pattern used internally for std modules like `exec.run`.

## Naming Conventions

| forai extern func name | WASM import name (convention) |
|------------------------|-------------------------------|
| `CheckLicense` | `check_license` |
| `SendSMS` | `send_sms` |
| `RunDockerBuild` | `run_docker_build` |
| `FetchExternalPrice` | `fetch_external_price` |

The convention is: PascalCase forai name → snake_case WASM import name. The compiler applies this transformation automatically.

## Complete Integration Example

Here is the full picture for adding an extern func `HashPassword` that uses a native bcrypt library:

**forai source (`HashPassword.fa`):**

```fa
docs HashPasswordResult
    The result of hashing a password.

    docs hash
        Bcrypt hash string.
    done
done

type HashPasswordResult
    hash text
done

docs HashPassword
    Hashes a plaintext password using bcrypt via the host runtime.
    The cost factor is fixed at 12 by the host implementation.
done

extern func HashPassword
    take plaintext as text
    emit result as HashPasswordResult
    fail error as text
```

**Rust host registration (`runtime.rs`):**

```rust
// In register_runtime():
linker.func_wrap("env", "hash_password", |plaintext_ptr: i32, plaintext_len: i32| -> i32 {
    let plaintext = read_wasm_string(memory, plaintext_ptr, plaintext_len);
    let hash = bcrypt::hash(&plaintext, 12).unwrap();
    write_wasm_string(memory, &hash)  // returns ptr to result in WASM memory
}).unwrap();

// In register_test_stubs():
linker.func_wrap("env", "hash_password", |_ptr: i32, _len: i32| -> i32 {
    write_wasm_string(memory, "$2b$12$stubhashvalue")
}).unwrap();
```

**forai caller:**

```fa
docs RegisterUser
    Hashes the password and stores the user.
done

func RegisterUser
    take conn as db_conn
    take email as text
    take plaintext_password as text
    emit result as dict
    fail error as text
body
    hash_result = HashPassword(plaintext_password to :plaintext)
    hash = obj.get(hash_result, "hash")
    id = random.uuid()
    params = list.new()
    params = list.append(params, id)
    params = list.append(params, email)
    params = list.append(params, hash)
    ok = db.exec(conn, "INSERT INTO users (id, email, password_hash) VALUES (?1, ?2, ?3)", params)
    result = obj.set(obj.new(), "id", id)
    emit result to :result
done
```

**Test block (mocking the extern func):**

```fa
test RegisterUser
    mock HashPassword => {hash: "$2b$12$testhash"}
    conn = db.open(":memory:")
    ok = db.exec(conn, "CREATE TABLE IF NOT EXISTS users (id TEXT, email TEXT, password_hash TEXT)")
    result = RegisterUser(conn to :conn, "user@example.com" to :email, "secret" to :plaintext_password)
    must result.id != ""
    db.close(conn)
done
```
