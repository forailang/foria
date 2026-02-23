# 10.17 — hash (reference)

The `hash.*` and `base64.*` namespaces are documented in full in [Chapter 16 — crypto](16-crypto.md), which covers all three security-related namespaces together:

- `crypto.*` — bcrypt, JWT, secure random bytes
- `hash.*` — SHA-256, SHA-512, HMAC
- `base64.*` — standard and URL-safe base64

## Quick Reference

### hash.*

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `hash.sha256` | text | text | SHA-256 hex digest (64 hex chars) |
| `hash.sha512` | text | text | SHA-512 hex digest (128 hex chars) |
| `hash.hmac` | key, data [, algo] | text | HMAC hex digest; `algo` defaults to `"sha256"`, also accepts `"sha512"` |

### base64.*

| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `base64.encode` | text | text | Standard base64 with padding |
| `base64.decode` | text | text | Standard base64 decode |
| `base64.encode_url` | text | text | URL-safe, no padding |
| `base64.decode_url` | text | text | URL-safe decode |

## When to Use hash.* vs crypto.*

| Use case | Recommended op |
|----------|---------------|
| Password storage | `crypto.hash_password` (bcrypt) |
| Password verification | `crypto.verify_password` |
| JWT creation | `crypto.sign_token` |
| JWT verification | `crypto.verify_token` |
| Webhook signature | `hash.hmac` |
| Content checksum | `hash.sha256` |
| API token generation | `crypto.random_bytes` |
| HTTP Basic Auth encoding | `base64.encode` |
| URL-safe tokens | `base64.encode_url` |

See [Chapter 16](16-crypto.md) for full documentation and examples.
