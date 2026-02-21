# forai — VSCode Extension

Language support for [forai](https://forai.dev) `.fa` files.

## Features

- Syntax highlighting for `.fa` files
- Language server integration (diagnostics, completion, hover, go-to-definition, formatting)
- Code snippets for `func`, `flow`, `type`, `test`, `case`, `loop`, `sync`, and more
- Smart indentation and bracket matching

## Requirements

The `forai` CLI must be installed and available on your `PATH`:

```bash
cargo install forai
```

Or configure a custom path in settings:

```json
"forai.lsp.path": "/path/to/forai"
```

## Extension Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `forai.lsp.path` | `"forai"` | Path to the forai binary used as the language server |
