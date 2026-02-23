# Installation and CLI

## Installing forai

forai is written in Rust. You need the Rust toolchain installed before proceeding. If you do not have it, install it from [rustup.rs](https://rustup.rs/).

Once Rust is available, clone the repository and build the binary:

```sh
git clone https://github.com/your-org/forai
cd forai
cargo build --release
```

The compiled binary is at `target/release/forai`. Add it to your `PATH`:

```sh
export PATH="$PATH:$(pwd)/target/release"
```

To verify the installation:

```sh
forai --version
```

## CLI Commands

The `forai` binary has five subcommands, each corresponding to a phase of the development workflow.

### `forai compile`

Compiles a `.fa` source file (or the entry-point of a multi-module project) to the JSON IR format. By default, the IR is printed to stdout. Use `-o` to write to a file, and `--compact` to minify the output.

```sh
forai compile main.fa                     # pretty-printed IR to stdout
forai compile main.fa -o out.json         # write to file
forai compile main.fa -o out.json --compact   # minified JSON
```

The compile step runs all checks: lexing, parsing, type checking, semantic validation (docs enforcement, port completeness), and op validation. Any error stops compilation and prints a message to stderr.

### `forai run`

Compiles and executes a `.fa` file or project. This is the main command for running programs during development.

```sh
forai run main.fa
forai run examples/read-docs/main.fa
```

The runtime uses Tokio's current-thread async executor. All I/O operations (HTTP, file, terminal, process execution) are non-blocking. The program runs until the main flow completes or a fatal error occurs.

### `forai test`

Runs the `test` blocks found in `.fa` files within a directory. Each test block has its own isolated environment. Mocks defined inside a test block substitute real sub-func calls without affecting other tests.

```sh
forai test app/              # run all tests in the app/ directory
forai test app/router/       # run tests in a subdirectory
forai test app/router/Classify.fa   # run tests in a single file
```

Note that `forai test` does not recurse automatically into subdirectories. To test an entire project tree, run it separately for each subdirectory you care about.

Test output shows each test name, a pass/fail result, and the assertion that failed (if any). Exit code is non-zero if any test fails.

### `forai doc`

Generates structured documentation from a project's `docs` blocks and emits a JSON artifact. This is useful for building documentation sites, generating API references, or feeding docs into other tooling.

```sh
forai doc main.fa             # JSON docs to stdout
forai doc main.fa -o docs.json
```

Because docs blocks are a compiler-visible language construct (not comments), the documentation is always in sync with the source. A module with missing docs will not compile, so the doc artifact is always complete.

### `forai dev`

Launches an interactive debug server with a WebSocket-based UI. This command compiles the project, runs it, and opens a debug session where you can step through the pipeline, inspect variable values at each stage, set breakpoints, and restart execution.

```sh
forai dev main.fa
```

The debug server listens on a local port and serves an embedded HTML interface. Open the URL printed to the terminal in your browser. The step/continue/restart protocol runs over WebSocket.

## Workspace Layout

A forai project is a directory of `.fa` files. There is no package manifest required for simple projects. For larger projects, a `forai.json` file at the root can configure project metadata:

```json
{
  "name": "my-project",
  "version": "0.1.0"
}
```

The conventional layout for a multi-module project looks like this:

```
my-project/
├── forai.json
├── main.fa           # entry flow (must be a flow named main)
└── app/
    ├── Start.fa      # a flow
    ├── sources/
    │   └── Events.fa # a source
    ├── router/
    │   └── Classify.fa  # a func
    └── display/
        ├── Print.fa     # a sink
        └── Welcome.fa   # a sink
```

Each directory is a module. The entry point (`main.fa`) imports modules with `use`:

```fa
use app from "./app"

flow main
body
    step app.Start() done
done
```

When a `use` points to a directory (e.g., `./app`), the module name becomes the directory name, and callables inside are accessed as `app.Start(...)`, `app.Classify(...)`, and so on.

When a `use` points to a specific `.fa` file (e.g., `./round.fa`), the callable is accessed directly as `Round(...)`.

## Path Resolution

Import paths in `use` declarations are always relative to the importing file's directory, not the project root:

```fa
# In app/Start.fa
use sources from "./sources"    # resolves to app/sources/
use router from "./router"      # resolves to app/router/
```

This means you can move a subtree of the project without updating paths in unrelated modules.
