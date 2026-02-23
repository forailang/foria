# The forai doc Command

The `forai doc` command extracts structured documentation from `.fa` source files and produces a machine-readable JSON artifact. Because the compiler enforces docs coverage, the output is always complete — there are no gaps where a callable was not documented.

## Basic Usage

Run `forai doc` on a file or directory:

```
forai doc app/Start.fa
forai doc app/
```

Without the `-o` flag, the docs artifact is written to stdout as JSON. With `-o`, it is written to a file:

```
forai doc app/ -o docs/api.json
forai doc app/auth/Login.fa -o docs/auth-login.json
```

The output file will be created if it does not exist, or overwritten if it does.

## DocsArtifact JSON Format

The JSON output is a `DocsArtifact` object. Its top-level structure:

```json
{
  "version": "0.1",
  "generated_at": "2026-02-22T14:30:00Z",
  "modules": [
    {
      "path": "app/auth/Login.fa",
      "callable": {
        "kind": "func",
        "name": "Login",
        "docs": "Authenticate a user with email and password.\nReturns a signed session token on success.",
        "ports": {
          "take": [{ "name": "req", "type": "LoginRequest" }],
          "emit": [{ "name": "token", "type": "text" }],
          "fail": [{ "name": "error", "type": "text" }]
        }
      },
      "types": [
        {
          "name": "LoginRequest",
          "open": false,
          "docs": "Credentials submitted by a user attempting to log in.",
          "fields": [
            { "name": "email", "type": "text", "docs": "The user's email address." },
            { "name": "password", "type": "text", "docs": "Plaintext password before hashing." }
          ]
        }
      ],
      "tests": [
        {
          "name": "LoginRejectsWrongPassword",
          "docs": "Confirm that a wrong password is rejected with an error."
        }
      ]
    }
  ]
}
```

Each entry in `modules` corresponds to one `.fa` file. The `callable` object contains the kind, name, documentation text, and port signatures. The `types` array lists all types declared in the file along with their field-level docs. The `tests` array lists all test blocks and their docs.

## Generated docs/ Folder Tree

When run on a directory, `forai doc` can also generate a `docs/` folder containing one Markdown file per callable, mirroring the source tree structure:

```
forai doc app/ -o docs/
```

This produces:

```
docs/
  auth/
    Login.md
    Logout.md
    Register.md
  orders/
    Create.md
    List.md
  Start.md
```

Each `.md` file contains the callable's name, kind, port signatures, documentation text, and type information formatted for human reading. This output is suitable for committing to a repository alongside source code and rendering in a documentation site.

An individual file's generated Markdown looks like:

```markdown
# func Login

Authenticate a user with email and password.
Returns a signed session token on success.

## Ports

**take** `req` — `LoginRequest`
**emit** `token` — `text`
**fail** `error` — `text`

## Types

### LoginRequest

Credentials submitted by a user attempting to log in.

| Field | Type | Description |
|-------|------|-------------|
| email | text | The user's email address. |
| password | text | Plaintext password before hashing. |
```

## Tooling Integration

The JSON format is designed for downstream consumers. Common uses:

- **Documentation sites**: parse `DocsArtifact` JSON and render HTML pages, one per callable
- **API explorers**: build interactive UI that lets users browse callable signatures and descriptions
- **Change detection**: diff two `DocsArtifact` files to find added, removed, or changed APIs between releases
- **Code generation**: use the port signatures to generate client SDKs or stub implementations in other languages
- **IDE language servers**: index the JSON to provide hover documentation and autocomplete

Because the output is deterministic (the same source always produces the same JSON), it is safe to commit the generated artifact alongside source code and use it as the canonical docs source.

## Running Doc Generation in CI

A typical CI pipeline for a forai project runs doc generation after a successful build:

```
forai build app/Start.fa
forai doc app/ -o docs/api.json
```

If the build passes (which means all docs requirements are met), the doc command is guaranteed to succeed and produce complete output. There is no case where `forai doc` produces partial results — the compiler's enforcement ensures the source is always fully documented before it compiles.

## Missing Docs Are Build Errors, Not Warnings

It bears repeating: if a callable or test is missing its `docs` block, the project does not compile. This means `forai doc` will never be called on undocumented code in a working CI pipeline. The docs artifact always reflects 100% coverage.

This guarantee is what makes the tooling integration reliable. You do not need to check whether the artifact is complete — it always is, by construction.
