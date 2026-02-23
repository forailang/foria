# forai Language Reference

A comprehensive reference for the forai dataflow language.

## Building the website

```bash
# one-time setup
python3 -m venv book/.venv
book/.venv/bin/pip install markdown

# build (outputs to book/_site/)
book/.venv/bin/python3 book/build.py

# open in browser
open book/_site/index.html
```

Pass `--out <dir>` to change the output location.

---

## Table of Contents

### [00 — Introduction](./00-introduction/)
- [What is forai?](./00-introduction/01-what-is-forai.md)
- [Installation and CLI](./00-introduction/02-installation-and-cli.md)
- [Hello World](./00-introduction/03-hello-world.md)
- [Mental Model](./00-introduction/04-mental-model.md)

### [01 — Core Constructs](./01-core-constructs/)
- [Overview](./01-core-constructs/01-overview.md)
- [func](./01-core-constructs/02-func.md)
- [source](./01-core-constructs/03-source.md)
- [flow](./01-core-constructs/04-flow.md)
- [sink](./01-core-constructs/05-sink.md)

### [02 — Types](./02-types/)
- [Primitives](./02-types/01-primitives.md)
- [Structs](./02-types/02-structs.md)
- [Enums](./02-types/03-enums.md)
- [Scalar Types](./02-types/04-scalar-types.md)
- [Constraints](./02-types/05-constraints.md)
- [Collections](./02-types/06-collections.md)
- [Handles](./02-types/07-handles.md)

### [03 — Expressions and Operators](./03-expressions-and-operators/)
- [Literals](./03-expressions-and-operators/01-literals.md)
- [Arithmetic and Comparison](./03-expressions-and-operators/02-arithmetic-and-comparison.md)
- [String Interpolation](./03-expressions-and-operators/03-string-interpolation.md)
- [Ternary](./03-expressions-and-operators/04-ternary.md)

### [04 — Control Flow](./04-control-flow/)
- [if / else](./04-control-flow/01-if-else.md)
- [case / when](./04-control-flow/02-case-when.md)
- [loop](./04-control-flow/03-loop.md)
- [break](./04-control-flow/04-break.md)
- [Scoping Rules](./04-control-flow/05-scoping-rules.md)

### [05 — Functions in Depth](./05-functions-in-depth/)
- [take / emit / fail](./05-functions-in-depth/01-take-emit-fail.md)
- [Calling Functions](./05-functions-in-depth/02-calling-functions.md)
- [Error Handling](./05-functions-in-depth/03-error-handling.md)
- [sync Blocks](./05-functions-in-depth/04-sync-blocks.md)
- [nowait](./05-functions-in-depth/05-nowait.md)

### [06 — Flows in Depth](./06-flows-in-depth/)
- [Steps and Wiring](./06-flows-in-depth/01-steps-and-wiring.md)
- [branch](./06-flows-in-depth/02-branch.md)
- [state](./06-flows-in-depth/03-state.md)
- [on Events](./06-flows-in-depth/04-on-events.md)
- [Port Naming](./06-flows-in-depth/05-port-naming.md)
- [send nowait](./06-flows-in-depth/06-send-nowait.md)

### [07 — Modules](./07-modules/)
- [use Imports](./07-modules/01-use-imports.md)
- [File and Directory Modules](./07-modules/02-file-and-directory-modules.md)
- [Resolution Rules](./07-modules/03-resolution-rules.md)
- [Circular Dependencies](./07-modules/04-circular-dependencies.md)

### [08 — Documentation](./08-documentation/)
- [docs Blocks](./08-documentation/01-docs-blocks.md)
- [Field docs](./08-documentation/02-field-docs.md)
- [forai doc Command](./08-documentation/03-forai-doc-command.md)

### [09 — Testing](./09-testing/)
- [test Blocks](./09-testing/01-test-blocks.md)
- [must Assertions](./09-testing/02-must-assertions.md)
- [trap Failures](./09-testing/03-trap-failures.md)
- [mock](./09-testing/04-mock.md)
- [forai test Command](./09-testing/05-forai-test-command.md)

### [10 — Standard Library](./10-standard-library/)
- [Overview](./10-standard-library/01-overview.md)
- [str](./10-standard-library/02-str.md)
- [list](./10-standard-library/03-list.md)
- [obj](./10-standard-library/04-obj.md)
- [math / type / to](./10-standard-library/05-math.md)
- [json / codec](./10-standard-library/06-json.md)
- [http client / headers / cookie](./10-standard-library/07-http-client.md)
- [http.server](./10-standard-library/08-http-server.md)
- [ws (WebSocket)](./10-standard-library/09-websocket.md)
- [db (SQLite)](./10-standard-library/10-db.md)
- [file](./10-standard-library/11-file.md)
- [term](./10-standard-library/12-term.md)
- [exec](./10-standard-library/13-exec.md)
- [regex](./10-standard-library/14-regex.md)
- [random](./10-standard-library/15-random.md)
- [crypto / base64 / hash](./10-standard-library/16-crypto.md)
- [hash (reference)](./10-standard-library/17-hash.md)
- [date / stamp / trange](./10-standard-library/18-date.md)
- [time / fmt](./10-standard-library/19-time.md)
- [env](./10-standard-library/20-env.md)
- [url](./10-standard-library/21-url.md)
- [route / html](./10-standard-library/22-route.md)
- [tmpl](./10-standard-library/23-tmpl.md)
- [error / log](./10-standard-library/24-error.md)

### [11 — Concurrency](./11-concurrency/)
- [Pipeline Model](./11-concurrency/01-pipeline-model.md)
- [sync Concurrent Joins](./11-concurrency/02-sync-concurrent-joins.md)
- [send nowait Background Tasks](./11-concurrency/03-send-nowait-background.md)
- [Handle Sharing](./11-concurrency/04-handle-sharing.md)

### [12 — Extern Functions](./12-extern-functions/)
- [extern func](./12-extern-functions/01-extern-fn.md)
- [Host Functions](./12-extern-functions/02-host-functions.md)

### [13 — Patterns and Recipes](./13-patterns-and-recipes/)
- [HTTP API](./13-patterns-and-recipes/01-http-api.md)
- [Database CRUD](./13-patterns-and-recipes/02-database-crud.md)
- [Background Jobs](./13-patterns-and-recipes/03-background-jobs.md)
- [Event Loops](./13-patterns-and-recipes/04-event-loops.md)
- [CLI Tools](./13-patterns-and-recipes/05-cli-tools.md)

### [14 — Compiler and IR](./14-compiler-and-ir/)
- [Compiler Pipeline](./14-compiler-and-ir/01-compiler-pipeline.md)
- [IR Format](./14-compiler-and-ir/02-ir-format.md)
- [forai compile](./14-compiler-and-ir/03-forai-compile.md)
- [forai dev Debugger](./14-compiler-and-ir/04-forai-dev-debugger.md)

### [15 — Reference](./15-reference/)
- [Keywords](./15-reference/01-keywords.md)
- [Operators](./15-reference/02-operators.md)
- [CLI Reference](./15-reference/03-cli-reference.md)
- [Error Messages](./15-reference/04-error-messages.md)
