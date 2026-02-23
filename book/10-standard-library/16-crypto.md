# 10.16 — crypto, hash, and base64

This chapter covers three security-related namespaces:

- `crypto.*` — bcrypt password hashing, JWT token operations, and secure random bytes
- `hash.*` — cryptographic hash digests (SHA-256, SHA-512, HMAC)
- `base64.*` — base64 standard and URL-safe encoding/decoding

---

## crypto.*

The `crypto.*` namespace provides high-level cryptographic primitives for common web application security needs: password storage, authentication tokens, and cryptographic randomness.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `crypto.hash_password` | password | text | Bcrypt hash with cost factor 12 |
| `crypto.verify_password` | password, hash | bool | Verify a password against a bcrypt hash |
| `crypto.sign_token` | payload, secret | text | Sign a JWT using HS256 |
| `crypto.verify_token` | token, secret | dict | Verify a JWT; returns `{valid, payload}` or `{valid: false, error}` |
| `crypto.random_bytes` | count | text | `count` random bytes as a hex string (1–1024 bytes) |

### Examples

#### Hashing a password on registration

```fa
func RegisterUser
  take email text
  take password text
  emit user dict
  fail err dict
body
  if str.len(password) < 8
    fail error.new("WEAK_PASSWORD", "Password must be at least 8 characters")
  done
  hashed = crypto.hash_password(password)
  user = obj.new()
  user = obj.set(user, "id", random.uuid())
  user = obj.set(user, "email", email)
  user = obj.set(user, "password_hash", hashed)
  emit user
done
```

#### Verifying a password on login

```fa
func Login
  take email text
  take password text
  take stored_hash text
  emit token text
  fail err dict
body
  ok = crypto.verify_password(password, stored_hash)
  if ok == false
    fail error.new("INVALID_CREDENTIALS", "Email or password is incorrect")
  done
  payload = obj.set(obj.set(obj.new(), "sub", email), "iat", date.to_unix_ms(date.now()))
  secret = env.get("JWT_SECRET")
  token = crypto.sign_token(payload, secret)
  emit token
done
```

#### Verifying a JWT token

```fa
func AuthMiddleware
  take token text
  emit claims dict
  fail err dict
body
  secret = env.get("JWT_SECRET")
  result = crypto.verify_token(token, secret)
  if obj.get(result, "valid") == false
    err_msg = obj.get(result, "error")
    fail error.new("UNAUTHORIZED", "Invalid token: #{err_msg}")
  done
  claims = obj.get(result, "payload")
  emit claims
done
```

#### Generating a secure random token

```fa
func NewApiKey
  emit key text
body
  # 32 bytes = 64 hex chars
  raw = crypto.random_bytes(32)
  key = "ak_#{raw}"
  emit key
done
```

#### Generating a session ID

```fa
func NewSessionId
  emit session_id text
body
  session_id = crypto.random_bytes(16)
  emit session_id
done
```

### Common Patterns

#### JWT with expiry claim

```fa
now_ms = date.to_unix_ms(date.now())
# 24 hours in ms = 86400000
exp_ms = now_ms + 86400000
payload = obj.set(obj.set(obj.set(obj.new(), "sub", user_id), "iat", now_ms), "exp", exp_ms)
token = crypto.sign_token(payload, secret)
```

Check expiry after verifying:

```fa
result = crypto.verify_token(token, secret)
if obj.get(result, "valid")
  claims = obj.get(result, "payload")
  exp = to.long(obj.get(claims, "exp"))
  now = date.to_unix_ms(date.now())
  if now > exp
    fail error.new("TOKEN_EXPIRED", "Token has expired")
  done
done
```

### Gotchas

- `crypto.hash_password` is intentionally slow (bcrypt cost 12). Do not call it in a hot path or tight loop — it is designed for one-time use at registration/password-change time.
- `crypto.verify_password` takes the **original plaintext password** and the **stored bcrypt hash** as arguments — not two hashes. Never store or log the plaintext password.
- `crypto.sign_token` produces a HS256 JWT. The payload dict must be JSON-serializable (no handle types). The secret should be a long random string stored in an environment variable.
- `crypto.verify_token` returns a dict in both success and failure cases. Always check `obj.get(result, "valid")` before accessing `payload`.
- `crypto.random_bytes(count)` accepts 1–1024 bytes. The returned hex string is `count * 2` characters long. For UUIDs, use `random.uuid()` instead.

---

## hash.*

The `hash.*` namespace provides low-level cryptographic hash digests. Unlike `crypto.*`, these are fast one-way hashes suitable for data integrity, content addressing, and HMAC-based message authentication.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `hash.sha256` | text | text | SHA-256 hex digest (64 chars) |
| `hash.sha512` | text | text | SHA-512 hex digest (128 chars) |
| `hash.hmac` | key, data [, algo] | text | HMAC hex digest; `algo` defaults to `"sha256"` |

### Examples

#### Content hash for caching

```fa
func ContentHash
  take content text
  emit hash text
body
  hash = hash.sha256(content)
  emit hash
done
```

#### Verifying data integrity

```fa
func VerifyIntegrity
  take data text
  take expected_hash text
  emit ok bool
body
  actual = hash.sha256(data)
  ok = actual == expected_hash
  emit ok
done
```

#### HMAC webhook signature

```fa
func SignWebhook
  take payload text
  take secret text
  emit signature text
body
  signature = "sha256=#{hash.hmac(secret, payload)}"
  emit signature
done
```

#### Verifying a GitHub-style webhook

```fa
func VerifyWebhook
  take payload text
  take signature_header text
  take secret text
  emit ok bool
body
  expected = "sha256=#{hash.hmac(secret, payload)}"
  ok = signature_header == expected
  emit ok
done
```

#### SHA-512 for password migration (legacy systems)

```fa
func HashLegacyPassword
  take password text
  take salt text
  emit digest text
body
  combined = "#{salt}:#{password}"
  digest = hash.sha512(combined)
  emit digest
done
```

#### HMAC with SHA-512

```fa
func SignData
  take key text
  take data text
  emit sig text
body
  sig = hash.hmac(key, data, "sha512")
  emit sig
done
```

### Gotchas

- `hash.sha256` and `hash.sha512` are NOT suitable for password storage. Use `crypto.hash_password` (bcrypt) for passwords.
- HMAC requires a secret key. Do not use `hash.sha256(secret + data)` as a substitute — this is vulnerable to length-extension attacks. Always use `hash.hmac`.
- Hash digests are returned as lowercase hex strings. Comparison is case-sensitive.
- There is no streaming interface — the entire input must be provided as a single text string.

---

## base64.*

The `base64.*` namespace provides standard and URL-safe base64 encoding and decoding. Used for encoding binary data as text, JWT components, and URL-safe token strings.

### Op Table

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `base64.encode` | text | text | Standard base64 encode (with `+`, `/`, `=` padding) |
| `base64.decode` | text | text | Standard base64 decode |
| `base64.encode_url` | text | text | URL-safe base64 encode (no padding, `-` and `_` instead of `+` and `/`) |
| `base64.decode_url` | text | text | URL-safe base64 decode |

### Examples

#### Encoding for HTTP Basic Auth

```fa
func BasicAuthHeader
  take username text
  take password text
  emit header_value text
body
  credentials = "#{username}:#{password}"
  encoded = base64.encode(credentials)
  header_value = "Basic #{encoded}"
  emit header_value
done
```

#### Decoding Basic Auth header

```fa
func ParseBasicAuth
  take header_value text
  emit username text
  emit password text
  fail err dict
body
  if str.starts_with(header_value, "Basic ") == false
    fail error.new("INVALID_AUTH", "Not a Basic auth header")
  done
  b64 = str.slice(header_value, 6, str.len(header_value))
  decoded = base64.decode(b64)
  parts = str.split(decoded, ":")
  if list.len(parts) < 2
    fail error.new("INVALID_AUTH", "Malformed credentials")
  done
  emit parts[0]
  emit parts[1]
done
```

#### URL-safe token for email links

```fa
func NewEmailToken
  take user_id text
  take email text
  emit token text
body
  payload = "#{user_id}:#{email}:#{to.text(date.to_unix_ms(date.now()))}"
  token = base64.encode_url(payload)
  emit token
done
```

#### Decoding a URL-safe token

```fa
func ParseEmailToken
  take token text
  emit user_id text
  emit email text
  fail err dict
body
  decoded = base64.decode_url(token)
  parts = str.split(decoded, ":")
  if list.len(parts) < 3
    fail error.new("INVALID_TOKEN", "Malformed token")
  done
  emit parts[0]
  emit parts[1]
done
```

#### Encoding binary data (hex → base64)

```fa
func HexToBase64
  take hex text
  emit b64 text
body
  # encode the hex string itself as base64
  b64 = base64.encode(hex)
  emit b64
done
```

### Gotchas

- `base64.encode` uses standard alphabet with `=` padding. `base64.encode_url` uses the URL-safe alphabet (`-` and `_`) without padding. These are not interchangeable — do not mix encode/decode variants.
- forai base64 treats the input as a UTF-8 string. For true binary data (e.g., raw bytes from `crypto.random_bytes`), the hex string representation is encoded — not the raw bytes.
- `base64.decode` will raise a runtime error on invalid base64 input. Validate or `trap` appropriately.
- Standard base64 output contains `+`, `/`, and `=` which are not URL-safe. Always use `base64.encode_url` for tokens that appear in URLs or query strings.
