# forai Dataflow Language Spec v1

Status: Complete (Phases 0–5 implemented, plus v2 extensions)
Scope: Compiler/runtime contract for `.fa` files in `/Users/bal/labs/forai/dataflow`

## 1. Design Rules
- Dataflow-first.
- Async by default.
- Quad-char keyword set.
- Strict interfaces (`take/emit/fail`) on every func and sink. Flows may omit ports.
- Docs are compiler-visible language constructs.

## 2. Source Model
- File extension: `.fa`
- Module = folder namespace.
- One callable (func, flow, or sink) per file. The callable name must match the filename stem (e.g., `func Foo` in `Foo.fa`).
- Symbol visibility:
  - Funcs, flows, and sinks are always public.
  - Types and enums are private by default; `open` makes them public.
- Imports use `use Name from "path"` syntax:
  - `use auth from "./auth"` (directory) → call as `auth.Login(...)`
  - `use Round from "./round.fa"` (file) → call as `Round(...)`
- Paths are relative to the importing `.fa` file's own directory, not the project root.

## 3. Keywords

### 3.1 Declarations
- `func`, `flow`, `sink`, `type`, `data`, `enum`, `use`, `docs`, `test`

### 3.2 Interface ports
- `take`, `emit`, `fail`, `return`, `as`

### 3.3 Body and blocks
- `body`, `done`, `open`

### 3.4 Control flow (func body)
- `case`, `when`, `else`, `loop`, `break`, `sync`

### 3.5 Async dispatch
- `send`, `nowait`

### 3.6 Flow body
- `step`, `state`, `on`, `from`, `next`, `then`, `to`

### 3.7 Testing
- `must`, `trap`, `mock`

## 4. Lexical Rules

### 4.1 Tokens
| Token | Description |
|-------|-------------|
| `Ident` | `[A-Za-z_][A-Za-z0-9_]*` |
| `Number` | Integer or float literal (digits with optional `.`) |
| `StringLit` | `"..."` with escape sequences |
| `StringInterp` | `"...#{expr}..."` — string with interpolated expressions |
| `RegexLit` | `/.../` — regex literal |
| `Symbol` | Single-char punctuation: `: , ( ) [ ] . + - * / % = ! < > & \| { } ?` |
| `FatArrow` | `=>` |
| `EqEq` | `==` |
| `BangEq` | `!=` |
| `GtEq` | `>=` |
| `LtEq` | `<=` |
| `AmpAmp` | `&&` |
| `PipePipe` | `\|\|` |
| `StarStar` | `**` |
| `Newline` | Line terminator |

### 4.2 Comments
- Comments start with `#` and extend to end of line.
- Inside strings, `#` is literal unless followed by `{` (which starts interpolation). Use `\#` to escape.

### 4.3 Strings
- Delimited by double quotes: `"hello"`.
- Escape sequences: `\n` (newline), `\t` (tab), `\\` (backslash), `\"` (quote), `\#` (literal hash).
- Bare `{` and `}` are literal characters (safe for regex quantifiers like `{4}`).

### 4.4 String interpolation
- `"Hello #{name}!"` — expressions inside `#{}` are evaluated at runtime.
- Interpolation expressions can be any expression: variables, function calls, arithmetic, dotted paths.
- Nested braces are tracked, so `"#{obj.get(x, "key")}"` works correctly.
- Type coercion: strings pass through, numbers use their string form, booleans become `"true"`/`"false"`, null becomes `"null"`, arrays/objects use JSON serialization.

### 4.5 Other literals
- Identifiers `true` and `false` are boolean literals.
- Type names use identifier syntax (recommended PascalCase for user types).
- Primitive type names are lowercase (see section 9).
- Constraint options in type definitions use `:key => value`.
- Regex literals are `/.../` (used by validation options like `:matches`).

## 5. Grammar (EBNF)
```ebnf
module           = { top_decl } ;

top_decl         = uses_decl
                 | docs_decl
                 | func_decl
                 | sink_decl
                 | flow_decl
                 | type_decl
                 | enum_decl
                 | test_decl ;

(* --- Imports --- *)
uses_decl        = "use" IDENT "from" STRING_LIT ;

(* --- Documentation --- *)
docs_decl        = "docs" IDENT NEWLINE docs_body "done" ;
docs_body        = { docs_text | field_docs_decl } ;
docs_text        = ANY_LINE_EXCEPT_DONE ;
field_docs_decl  = INDENT "docs" IDENT NEWLINE
                   { INDENT ANY_LINE_EXCEPT_DONE }
                   INDENT "done" ;

(* --- Func: v1 named ports --- *)
func_decl        = "func" IDENT NEWLINE
                   { take_decl }
                   ( v1_ports | v2_ports )
                   "body" NEWLINE
                   stmt_block
                   "done" ;

v1_ports         = { emit_decl } { fail_decl } ;
v2_ports         = return_decl [ v2_fail_decl ] ;

take_decl        = "take" IDENT "as" type_ref NEWLINE ;
emit_decl        = "emit" IDENT "as" type_ref NEWLINE ;
fail_decl        = "fail" IDENT "as" type_ref NEWLINE ;
return_decl      = "return" type_ref NEWLINE ;
v2_fail_decl     = "fail" type_ref NEWLINE ;        (* no name, no "as" *)

(* --- Sink: identical to func --- *)
sink_decl        = "sink" IDENT NEWLINE
                   { take_decl }
                   { emit_decl }
                   { fail_decl }
                   "body" NEWLINE
                   stmt_block
                   "done" ;

(* --- Flow: step-based wiring --- *)
flow_decl        = "flow" IDENT NEWLINE
                   { take_decl }
                   { emit_decl }
                   { fail_decl }
                   "body" NEWLINE
                   flow_stmt_block
                   "done" ;

(* --- Types --- *)
type_decl        = [ "open" ] ( scalar_type_decl | struct_type_decl ) ;

scalar_type_decl = "type" IDENT "as" type_ref [ type_opts ] NEWLINE ;

struct_type_decl = ( "type" | "data" ) IDENT NEWLINE
                   { field_decl }
                   "done" ;

field_decl       = IDENT type_ref [ type_opts ] NEWLINE ;

type_opts        = type_opt { "," type_opt } ;
type_opt         = ":" IDENT [ "=>" constraint_value ] ;
constraint_value = BOOL | NUMBER | REGEX | STRING | ":" IDENT ;

(* --- Enums --- *)
enum_decl        = [ "open" ] "enum" IDENT NEWLINE
                   { IDENT NEWLINE }
                   "done" ;

(* --- Tests --- *)
test_decl        = "test" IDENT NEWLINE
                   test_body
                   "done" ;

test_body        = { mock_decl } { test_stmt } ;
mock_decl        = "mock" dotted_ident "=>" expr NEWLINE ;
test_stmt        = must_stmt | trap_stmt | assign_stmt | comment ;
must_stmt        = "must" expr NEWLINE ;
trap_stmt        = IDENT "=" "trap" dotted_ident "(" expr_list ")" NEWLINE ;

(* --- Expressions --- *)
expr             = ternary_expr ;
ternary_expr     = or_expr [ "?" or_expr ":" or_expr ] ;
or_expr          = and_expr { "||" and_expr } ;
and_expr         = eq_expr { "&&" eq_expr } ;
eq_expr          = cmp_expr { ( "==" | "!=" ) cmp_expr } ;
cmp_expr         = add_expr { ( "<" | ">" | "<=" | ">=" ) add_expr } ;
add_expr         = mul_expr { ( "+" | "-" ) mul_expr } ;
mul_expr         = pow_expr { ( "*" | "/" | "%" ) pow_expr } ;
pow_expr         = unary_expr { "**" unary_expr } ;  (* right-associative *)
unary_expr       = ( "-" | "!" ) unary_expr | postfix_expr ;
postfix_expr     = atom { "[" expr "]" } [ "(" expr_list ")" ] ;
atom             = NUMBER | STRING | STRING_INTERP | BOOL
                 | list_lit | dict_lit
                 | "(" expr ")"
                 | dotted_ident ;

list_lit         = "[" [ expr { "," expr } ] "]" ;
dict_lit         = "{" [ dict_pair { "," dict_pair } ] "}" ;
dict_pair        = IDENT ":" expr ;

dotted_ident     = IDENT { "." IDENT } ;
expr_list        = [ expr { "," expr } ] ;

(* --- Func body statements --- *)
stmt_block       = { stmt } ;

stmt             = assign_stmt
                 | emit_stmt
                 | return_stmt
                 | fail_stmt
                 | case_stmt
                 | loop_stmt
                 | bare_loop_stmt
                 | break_stmt
                 | sync_stmt
                 | send_stmt
                 | nowait_stmt ;

assign_stmt      = IDENT "=" expr NEWLINE ;
emit_stmt        = "emit" IDENT NEWLINE ;
return_stmt      = "return" IDENT NEWLINE ;
fail_stmt        = "fail" IDENT NEWLINE ;
break_stmt       = "break" NEWLINE ;

case_stmt        = "case" expr NEWLINE
                   { "when" pattern NEWLINE stmt_block }
                   [ "else" NEWLINE stmt_block ]
                   "done" ;

loop_stmt        = "loop" expr "as" IDENT NEWLINE
                   stmt_block
                   "done" ;

bare_loop_stmt   = "loop" NEWLINE
                   stmt_block
                   "done" ;

sync_stmt        = "[" ident_list "]" "=" "sync" [ sync_opts ] NEWLINE
                   stmt_block
                   "done" "[" ident_list "]" NEWLINE ;

send_stmt        = "send" "nowait" dotted_ident "(" expr_list ")" NEWLINE ;
nowait_stmt      = "nowait" dotted_ident "(" expr_list ")" NEWLINE ;

sync_opts        = sync_opt { "," sync_opt } ;
sync_opt         = ":timeout" "=>" duration
                 | ":retry"   "=>" INT
                 | ":safe"    "=>" BOOL ;

ident_list       = IDENT { "," IDENT } ;
pattern          = IDENT | STRING | INT | FLOAT | BOOL ;
type_ref         = IDENT ;

(* --- Flow body statements --- *)
flow_stmt_block  = { flow_stmt } ;

flow_stmt        = step_block
                 | state_decl
                 | on_block
                 | branch_stmt
                 | flow_send_stmt
                 | flow_emit_stmt
                 | flow_fail_stmt ;

(* Step: v1 block form *)
step_block       = "step" NEWLINE
                   callee_call NEWLINE
                   { then_item }
                   "done"
                 (* Step: v2 inline form *)
                 | "step" callee_call "then" NEWLINE
                   { then_item }
                   "done"
                 (* Step: fire-and-forget *)
                 | "step" callee_call "done" ;

callee_call      = dotted_ident "(" port_mappings ")" ;

then_item        = "next" ":" IDENT "to" IDENT NEWLINE
                 | flow_emit_stmt
                 | flow_fail_stmt
                 | ":" IDENT "to" callee_call NEWLINE ;

port_mappings    = [ port_mapping { "," port_mapping } ] ;
port_mapping     = ( IDENT | STRING | NUMBER | BOOL ) "to" ":" IDENT ;

(* State: resource initialization *)
state_decl       = "state" IDENT "=" dotted_ident "(" arg_list ")" NEWLINE ;
arg_list         = [ arg { "," arg } ] ;
arg              = IDENT | STRING | NUMBER | BOOL ;

(* On: event loop *)
on_block         = "on" IDENT "from" IDENT NEWLINE
                   flow_stmt_block
                   "done" ;

(* Flow-level routing *)
flow_emit_stmt   = "emit" IDENT "to" ":" IDENT NEWLINE ;
flow_fail_stmt   = "fail" IDENT "to" ":" IDENT NEWLINE ;
flow_send_stmt   = "send" "nowait" dotted_ident "(" ident_list ")" NEWLINE ;

(* Branch: conditional sub-pipeline *)
branch_stmt      = "branch" [ "when" expr ] NEWLINE
                   flow_stmt_block
                   "done" ;
```

## 6. Resolved Decisions

### 6.1 `must` scope
- `must` is valid only inside `test` blocks.
- Using `must` outside `test` is a compile error.

### 6.2 `sync` shape
- Canonical syntax is:
```fa
[out1, out2] = sync :timeout => 5s, :retry => 2, :safe => true
  a = TaskA(...)
  b = TaskB(...)
  done [a, b]
```
- Left-hand list and `done [vars]` are both required.
- `done [vars]` must match left-hand count and order by position.

### 6.3 `type` vs `data`
- Top-level `type` and `data` both define named struct-like composite types.
- `type` is preferred spelling for named declarations.
- `data` is retained as a supported alias for compatibility with spec intent.
- Expression-level anonymous `data ... done` is reserved (not in parser).

### 6.4 Test requirement strictness
- Docs requirement is always strict (hard error).
- "Every func/flow/type should have at least one test" is:
  - warning by default
  - hard error with `--require-tests`

### 6.5 Variable reassignment
- Variables may be reassigned in func bodies. Single-assignment is not enforced.

### 6.6 v1 vs v2 func syntax
- v1 uses named `emit`/`fail` ports: `emit result as text`, `fail error as text`.
- v2 uses unnamed return/fail types: `return text`, `fail text` (no name, no `as`).
- v2 maps to internal ports `_return` and `_fail`.
- v2 body uses `return var` instead of `emit var`.
- Cannot mix v1 named ports and v2 return/fail in the same func.
- If v2 `return` is present, `fail` type must also be present, and vice versa.

## 7. Three Constructs: func, flow, sink

### 7.1 `func` — Leaf computation
- Body contains imperative statements: assignment, emit/return, fail, case, loop, sync, break, send nowait.
- Calls built-in ops and other funcs/flows.
- All funcs are async. Statements execute sequentially — each is awaited before the next runs.
- I/O ops (HTTP, file, WebSocket, process, DB) are non-blocking under the hood.
- `emit var` / `return var` ends func on success track.
- `fail var` ends func on failure track.

### 7.2 `flow` — Composition wiring
- Body contains only declarative statements: `step`, `state`, `on`, `send nowait`, `emit ... to`, `fail ... to`.
- Each step invokes a func or flow with explicit port mappings.
- No arbitrary expressions in flow bodies.
- Steps execute sequentially unless inside `on` event loops.

### 7.3 `sink` — Side-effect endpoint
- Same syntax and body as `func`.
- Semantically marks a terminal side-effect (terminal I/O, writes, responses).
- Declaration keyword is `sink` instead of `func`.

### 7.4 `main` must be a `flow`
- The entry-point callable named `main` must be a `flow`, not a `func` or `sink`.

### 7.5 Interface contract
- **Funcs and sinks** must declare:
  - Zero or more `take` inputs.
  - One or more `emit` output ports (or `return` type for v2 funcs).
  - One or more `fail` output ports (or `fail` type for v2 funcs).
- **Flows** may declare:
  - Zero or more `take` inputs.
  - Zero or more `emit` output ports (flows that are pure wiring need none).
  - Zero or more `fail` output ports.
- `take/emit/fail/return` declarations must appear before `body`.

### 7.6 Call behavior
- Untrapped failing calls propagate failure immediately.
- `trap FlowCall(...)` captures failure payload as a value and prevents propagation.

### 7.7 Module system
- `use Name from "path"` imports a module by file or directory path.
- Directory imports: calls use qualified names `Name.FuncName(...)`.
- File imports: calls use the bound name directly `Name(...)`.
- Module directories contain `.fa` files; each file defines one `func`, `flow`, or `sink`.
- Paths resolve relative to the importing file's directory, not the project root.
- Circular dependencies are detected and rejected.

### 7.8 Handle sharing across calls
- Stateful resource handles (database connections, HTTP servers, WebSocket connections)
  are shared from parent funcs/flows to child funcs/flows automatically.
- When a func opens a resource (e.g. `conn = db.open(":memory:")`), the handle value
  (an opaque `db_conn` handle like `"db_0"`) can be passed as an argument to a child func or flow.
  The child declares the handle type in its `take` (e.g. `take conn as db_conn`) and can use
  it directly because it shares the parent's handle registry.
- **Ownership rule**: the func/flow that opened a handle owns it. Children may use it
  but should not close it — the parent is responsible for cleanup.
- **`send nowait` isolation**: fire-and-forget tasks get their own isolated handle
  registry. Handle IDs from the parent are not resolvable inside a `send nowait` target.
  If a spawned task needs a resource, it must open its own.
- **`sync` sharing**: statements inside a `sync` block share the parent's handle
  registry. Multiple concurrent branches can use the same handles, but opening new
  handles from concurrent branches risks ID collisions — prefer opening handles before
  the sync block.

Example — passing a DB connection to a child func:
```fa
func Setup
  emit conn as db_conn
body
  conn = db.open(":memory:")
  created = db.exec(conn, "CREATE TABLE items (name TEXT)")
  emit conn
done

func InsertItem
  take conn as db_conn
  take name as text
  emit ok as bool
body
  params = list.new()
  p = list.append(params, name)
  result = db.exec(conn, "INSERT INTO items VALUES (?1)", p)
  ok = true
  emit ok
done

func Demo
  emit count as long
body
  conn = Setup()
  ok1 = InsertItem(conn, "apple")
  ok2 = InsertItem(conn, "banana")
  rows = db.query(conn, "SELECT * FROM items")
  count = list.len(rows)
  closed = db.close(conn)
  emit count
done
```

## 8. Expressions

### 8.1 Operator precedence (lowest to highest)

| Precedence | Operator | Description | Associativity |
|------------|----------|-------------|---------------|
| 0 | `? :` | Ternary conditional | — |
| 1 | `\|\|` | Logical OR | Left |
| 2 | `&&` | Logical AND | Left |
| 3 | `==` `!=` | Equality | Left |
| 4 | `<` `>` `<=` `>=` | Comparison | Left |
| 5 | `+` `-` | Addition, subtraction, string concatenation | Left |
| 6 | `*` `/` `%` | Multiplication, division, modulo | Left |
| 7 | `**` | Exponentiation | Right |
| 8 | `-` `!` (prefix) | Unary negation, logical not | — |

### 8.2 Ternary conditional
```fa
result = condition ? "yes" : "no"
```
- Lowest precedence; only valid at top-level expression position (RHS of assignment).
- Short-circuit: only the chosen branch is evaluated.
- Truthiness rules: `false`, `null`, empty string `""`, and `0` are falsy. Everything else (including empty arrays and objects) is truthy.

### 8.3 Arithmetic
- `+` `-` `*` `/` `%` `**` on numbers.
- `+` on strings performs concatenation.
- `/` errors on division by zero. `%` errors on modulo by zero.
- **Integer preservation**: when both operands are integers, `+`, `-`, `*`, and `%` produce integer results. `/` and `**` always produce floats.

### 8.4 Comparison
- `==` `!=` use deep JSON structural equality. Works for all types.
- `<` `>` `<=` `>=` require numeric operands.

### 8.5 Logical
- `&&` `||` require boolean operands. Non-short-circuit (both sides evaluated).
- `!` (prefix) requires boolean operand.

### 8.6 Literals
- **Integer**: `42`, `-1` — parsed as `i64`.
- **Float**: `3.14` — parsed as `f64`.
- **Boolean**: `true`, `false`.
- **String**: `"hello"`, `"hello #{name}"` (with interpolation).
- **List**: `[1, 2, 3]` or `[]` — elements are any expressions.
- **Dict**: `{name: "Alice", age: 30}` or `{}` — keys are identifiers (not strings), values are any expressions.
- **Null**: not a literal in source; produced by runtime operations.

### 8.7 Function calls
```fa
result = math.floor(x)
result = str.split(text, ",")
result = MyFunc(arg1, arg2)
```
- Arguments are full expressions.
- Dispatch priority: (1) value mocks (in tests), (2) user-defined funcs/flows, (3) built-in ops.

### 8.8 Variable paths
- `name` — simple variable lookup.
- `input.field.subfield` — dot-path traversal into nested objects.

### 8.9 Bracket indexing
```fa
first = items[0]
last = items[-1]
name = row["name"]
cell = matrix[0][1]
```
- Postfix `[expr]` indexes into lists or dicts.
- List indexing: index must be an integer. Supports negative indices (`-1` = last element, `-2` = second-to-last).
- Dict indexing: index must be a string key.
- Chained indexing (`a[0][1]`) is supported.
- Out-of-bounds or missing key is a runtime error.

### 8.10 Parenthesized expressions
- `(expr)` overrides precedence.

## 9. Control Flow

### 9.1 `case`
```fa
case expr
  when "active"
    # statements for active
  when "inactive"
    # statements for inactive
  else
    # default statements
done
```
- Pattern matching on the subject expression.
- Patterns: string literals, integer literals, float literals, boolean literals, identifiers (matched as strings).
- First matching arm executes. `else` is the default fallback.
- At runtime, variable assignments inside case arms persist in the caller's scope.
- In the IR lowerer, case arm scopes are discarded — emit from each arm instead of setting a variable.

### 9.2 `loop` — Collection iteration
```fa
iters = list.range(0, 10)
loop iters as i
  # statements using i
done
```
- Iterates over a list expression.
- The loop variable (`i`) is scoped to the loop body.
- **Important**: the collection expression must be a variable or simple atom, not an inline function call. Assign the collection first.

### 9.3 `loop` — Bare loop (infinite)
```fa
loop
  # statements
  break
done
```
- Infinite loop; must contain `break` to exit.
- Detected by `loop` followed immediately by a newline (no `as` clause).

### 9.4 `break`
- Exits the nearest enclosing loop (bare or collection).
- Propagates through `case` blocks: a `break` inside a case arm inside a loop exits the loop.
- Inside `sync` blocks, `break` is swallowed (treated as continue).

### 9.5 `sync` — Concurrent execution
```fa
[out1, out2] = sync :timeout => 5s, :retry => 2, :safe => true
  a = TaskA(...)
  b = TaskB(...)
done [a, b]
```
- Statements inside a sync block run concurrently via `join_all`.
- Each statement gets its own copy of the current scope — statements must be independent (no cross-references).
- Results are merged after all statements complete; only exported variables are visible outside.
- Options:
  - `:timeout => <duration>` — duration uses `ms`, `s`, or `m` suffixes.
  - `:retry => <int>` — retry count on failure (default 0).
  - `:safe => <bool>` — if `true`, converts branch failure to `null` exports instead of propagating.

### 9.6 `nowait` and `send nowait`
- Fire-and-forget async call. The target starts immediately but the caller continues without waiting.
- `nowait op(args)` — fire-and-forget for built-in ops.
- `send nowait FuncName(args)` — fire-and-forget for user funcs/flows.
- The spawned task gets its own isolated scope and handle registry.
- Errors in spawned tasks are logged to stderr, not propagated to the caller.

## 10. Flow Body Semantics

### 10.1 `step` — Wiring a callee
```fa
# v2 inline form (preferred):
step calc.Mortgage(amount to :loan_amount, rate to :apr) then
  next :result to payment
  emit payment to :result
done

# Fire-and-forget (no result capture):
step logger.Log(msg to :message) done

# v1 block form:
step
  calc.Mortgage(amount to :loan_amount, rate to :apr)
  next :result to payment
  emit payment to :result
done
```
- Port mappings bind caller variables/literals to callee input ports.
- Port mapping values can be: variables, string literals, numeric literals, boolean literals.
- `next :port to wire` binds callee output ports to new wire labels.
- `emit wire to :port` sends a wire value to a declared flow emit port.
- `fail wire to :port` sends a wire value to a declared flow fail port.
- Conditional routing uses `branch when <expr>` at the flow level, not inside step blocks.

### 10.2 `state` — Resource initialization
```fa
state conn = db.open(":memory:")
state srv = http.server.listen(8080)
```
- Creates a shared resource handle available to subsequent steps.
- Arguments can be identifiers, string literals, numeric literals, or booleans.

### 10.3 `on` — Event handler

In a source body, `on` declares an event handler that loops over a blocking async call:

```fa
on :request from http.server.accept(srv) to req
  emit req
done
```

- `:tag` — event type name (cosmetic in v1, stored but not routed by runtime)
- `from op(args)` — blocking async call that returns one event per invocation
- `to var` — binds the result of each call
- Body runs per event; `emit` sends downstream, `break` stops the source
- Single `on` per source for v1

In a flow body, `on` can also wire a source handle to downstream steps:

```fa
on req from srv
  step router.Dispatch(req to :request) then
    next :result to resp
    emit resp to :response
  done
done
```
- Declares an event-driven loop: repeatedly accepts events from a source handle.
- The bind variable (`req`) is populated with each accepted event.
- Contains flow statements that process each event.

### 10.4 Flow-level `send nowait`
```fa
send nowait workflow.RunJobLoop(conn)
```
- Fire-and-forget dispatch from within a flow body.
- Arguments are identifiers only (no expressions).

### 10.5 Wire scoping
- Wire labels are scoped: only `take` names, `state` binds, and prior `next` labels are in scope.

### 10.6 Branch
```
branch when raw > 50.0
    step lib.Square(raw to :num) then
        next :result to processed
    done
    step sinks.Print(processed to :line) done
done

branch
    step lib.Double(raw to :num) then
        next :result to doubled
    done
done
```
- `branch when <expr>` runs its body only when the expression evaluates to `true`.
- `branch` (unguarded) always runs its body.
- Multiple branches are independent — all whose conditions are true fire.
- Branches don't merge back — they are one-way sub-pipelines.
- The body may contain any flow statement: `step`, `emit`, `fail`, nested `branch`, etc.
- Guarded branches lower to `case` at compile time; unguarded branches inline their body directly.

## 11. Type System

### 11.1 Primitive types
| Type | JSON representation |
|------|-------------------|
| `text` | String |
| `bool` | Boolean |
| `long` | Integer (i64) |
| `real` | Number (f64 or i64) |
| `uuid` | String (semantically a UUID) |
| `time` | String (semantically a timestamp) |
| `list` | Array |
| `dict` | Object |
| `void` | Null |
| `db_conn` | String (opaque database connection handle) |
| `http_server` | String (opaque HTTP server handle) |
| `http_conn` | String (opaque HTTP connection handle) |
| `ws_conn` | String (opaque WebSocket connection handle) |

Handle types are opaque — they cannot be constructed from string literals. They are produced only by specific ops (`db.open`, `http.server.listen`, etc.) and the compiler validates that handle-consuming ops receive the correct handle type. At runtime, handles are string values (e.g. `"db_0"`, `"srv_1"`).

### 11.1b Built-in struct types

The following struct types are returned by stdlib ops and available without declaration:

| Type | Returned by | Fields |
|------|-------------|--------|
| `HttpRequest` | `http.server.accept` | `method` text, `path` text, `query` text, `headers` dict, `body` text, `conn_id` text |
| `HttpResponse` | `http.get/post/put/delete` | `status` long, `headers` dict, `body` text |
| `Date` | `date.now`, `date.from_iso`, etc. | `unix_ms` long, `tz_offset_min` long |
| `Stamp` | `stamp.now`, `stamp.from_ns` | `ns` long |
| `TimeRange` | `trange.new` | `start` Date, `end` Date |
| `ProcessOutput` | `exec.run` | `code` long, `stdout` text, `stderr` text, `ok` bool |
| `WebSocketMessage` | `ws.recv` | `type` text, `data` text |
| `ErrorObject` | `error.new` | `code` text, `message` text, `details` dict (optional) |
| `URLParts` | `url.parse` | `path` text, `query` text, `fragment` text |

The type checker tracks built-in struct return types at compile time — passing a `Date` where a `TimeRange` is expected, or a `ProcessOutput` where a `db_conn` is expected, is a compile error.

User-defined types with the same name as a built-in type produce a "duplicate type name" compile error.

### 11.2 Scalar wrappers
```fa
type Email as text, :matches => /@/, :map => :lowercase
```
- Named alias of a primitive type with validation constraints.

### 11.3 Structs
```fa
type User
  id    uuid :required => true
  email Email
  age   long :min => 0
done
```
- Both `type` and `data` keywords produce struct types.
- Fields reference known types (primitives, scalars, other structs, enums).
- Nested struct fields trigger recursive validation.

### 11.4 Enums
```fa
enum Role
  Admin
  User
  Guest
done
```
- String enumeration with a fixed set of variant names.
- `open` modifier for extensible enums.

### 11.5 Constraints
| Key | Applies to | Value | Description |
|-----|-----------|-------|-------------|
| `:matches` | text | Regex | Value must match the regex pattern |
| `:min` | long, real | Number | Minimum value (or minimum length for text) |
| `:max` | long, real | Number | Maximum value (or maximum length for text) |
| `:required` | struct field | Boolean | Field must be present |
| `:map` | text | Symbol | Reserved for future transforms |

Constraint syntax: `:key` (boolean flag, defaults to true) or `:key => value`.

### 11.6 Validation boundary
- `take` values are validated before entering `body`.
- Validation failure is reported on the failure track.
- `emit`/`fail` values are validated against their declared types after `body` execution.
- Output validation failure is a hard error (bug in the flow, not bad input).

## 12. Documentation Rules

### 12.1 Syntax
```fa
docs LoginFlow
  Authenticates user credentials.
done
```

For struct types, nested `docs field_name ... done` sub-blocks document each field:
```fa
docs EmailResult
  Contains the result of an email quality check.

  docs email
    The cleaned email address.
  done

  docs valid
    Whether the email passed validation.
  done
done

type EmailResult
  email text
  valid bool
done
```

### 12.2 Enforcement
- Every `func`, `flow`, `sink`, `type`, and `test` must have a matching `docs` block.
- `enum` and `uses` declarations are exempt.
- For struct types, every field must have a matching `docs field_name` sub-block.
- Orphan field docs (documenting a field that doesn't exist in the type) is a compile error.
- Orphan docs blocks (documenting a symbol that doesn't exist in the module) is a compile error.
- Duplicate docs blocks for the same symbol is a compile error.
- Missing docs is a compile error.

### 12.3 Binding
- `docs X` must resolve to symbol `X` in the same module.

## 13. Testing Rules

### 13.1 Syntax
```fa
docs LoginHappy
  Verifies success path.
done

test LoginHappy
  res = LoginFlow(req)
  must res.status == 200
done
```

### 13.2 Failure testing
```fa
test LoginBadCreds
  err = trap LoginFlow(bad_req)
  must err.code == "INVALID_CREDENTIALS"
done
```
- `trap` expects the call to fail. Captures the failure value.
- If the call succeeds, the test fails.

### 13.3 Mocking
```fa
test NumberCrunchMocked
  mock calc.AddTwo => obj("value", 99.0)
  mock calc.MultiplyFive => obj("value", 99.0)
  inp = obj("value", 1.0)
  res = NumberCrunch(inp)
  must res.value == 99.0
done
```
- `mock` lines must appear at the top of the test block, before any statements.
- Mock values bypass the real func/flow execution entirely.
- Mock value expressions are evaluated in an empty environment (only literal values).

### 13.4 Assertions
- `must expr` accepts any boolean expression.
- Comparison operators: `==`, `!=`, `>`, `>=`, `<`, `<=`.
- Truthiness check (no operator): null, false, 0, empty string, empty array, empty object are falsy.
- First failed `must` fails the test immediately.

### 13.5 Test value expressions
- String literals: `"text"` or `'text'`.
- Numbers, booleans: `42`, `3.14`, `true`, `false`.
- Constructors: `dict()`, `obj("key1", val1, "key2", val2)`.
- Variable/path references: `var`, `var.field.subfield`.

## 14. Semantic Rules

### 14.1 Single callable per file
A `.fa` file may contain at most one `func`, `flow`, or `sink`. It may also contain types, enums, docs, tests, and `use` declarations alongside the single callable.

### 14.2 Name-filename match
When a file contains a callable, its name must match the filename stem (e.g., `func Foo` in `Foo.fa`). Exception: entry-point files like `main.fa`.

### 14.3 `main` must be a flow
If a callable is named `main`, it must be a `flow`. `func main` and `sink main` are compile errors.

### 14.4 v2 port completeness
If a func uses v2 syntax (`return Type`), it must also declare `fail Type`, and vice versa. v1 named ports and v2 return/fail cannot be mixed in the same func.

### 14.5 Op validation
Every op used in func bodies is validated at compile time against:
- The built-in op registry (~160 ops)
- The codec registry (e.g., `json.decode`, `json.encode`, `json.encode_pretty`)
- The flow registry (user-defined funcs/flows from `uses` modules)
Unknown ops produce a compile error.

## 15. Built-in Runtime Ops

### 15.1 `obj.*` — Immutable dict operations
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `obj.new` | | `{}` | Empty object |
| `obj.set` | obj, key, value | dict | Clone + insert key/value |
| `obj.get` | obj, key | value | Get by key (errors if missing) |
| `obj.has` | obj, key | bool | Check if key exists |
| `obj.delete` | obj, key | dict | Clone + remove key |
| `obj.keys` | obj | list | Array of key strings |
| `obj.merge` | obj1, obj2 | dict | Merge two dicts (right overwrites) |

### 15.2 `list.*` — Immutable list operations
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `list.new` | | `[]` | Empty list |
| `list.range` | start, end | list | Inclusive range `[start..=end]` |
| `list.append` | list, item | list | Clone + push item |

| `list.len` | list | long | Array length |
| `list.contains` | list, value | bool | Check if value is in list |
| `list.slice` | list, start, end | list | Subarray `[start..end)`, clamped |
| `list.indices` | list | list | Returns `[0, 1, 2, ...]` for list length |

### 15.3 `str.*` — String operations
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `str.len` | text | long | Character count (Unicode-aware) |
| `str.upper` | text | text | Uppercase |
| `str.lower` | text | text | Lowercase |
| `str.trim` | text | text | Trim whitespace both sides |
| `str.trim_start` | text | text | Trim leading whitespace |
| `str.trim_end` | text | text | Trim trailing whitespace |
| `str.split` | text, delimiter | list | Split by delimiter |
| `str.join` | list, separator | text | Join array with separator |
| `str.replace` | text, from, to | text | Replace all occurrences |
| `str.contains` | text, substr | bool | Substring check |
| `str.starts_with` | text, prefix | bool | Prefix check |
| `str.ends_with` | text, suffix | bool | Suffix check |
| `str.slice` | text, start, end | text | Character-based slice, clamped |
| `str.index_of` | text, substr | long | First index (-1 if not found) |
| `str.repeat` | text, count | text | Repeat string N times |

### 15.4 `math.*` — Rounding
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `math.floor` | a | long | `floor(a)` |
| `math.round` | value, places | real | Round to N decimal places |

Arithmetic uses infix operators: `+` `-` `*` `/` `%` `**`.

### 15.5 `type.*` — Type introspection
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `type.of` | value | text | Returns `"text"`, `"bool"`, `"long"`, `"real"`, `"list"`, `"dict"`, or `"void"` |

### 15.6 `to.*` — Type conversion
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `to.text` | value | text | Convert any value to string |
| `to.long` | value | long | Convert to integer (extracts number from strings, rounds reals) |
| `to.real` | value | real | Convert to float |
| `to.bool` | value | bool | Convert to boolean (`""`, `"false"`, `"0"` are false) |

### 15.7 `json.*` — JSON codec
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `json.decode` | text | value | Parse JSON string |
| `json.encode` | value | text | Compact JSON string |
| `json.encode_pretty` | value | text | Pretty-printed JSON string |

### 15.8 `codec.*` — Generic codec dispatch
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `codec.decode` | format, text | value | Decode using named format |
| `codec.encode` | format, value | text | Encode using named format |
| `codec.encode_pretty` | format, value | text | Pretty-encode using named format |

### 15.9 `http.*` — HTTP client and helpers
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `http.get` | url | dict | HTTP GET request |
| `http.post` | url, body | dict | HTTP POST request |
| `http.put` | url, body | dict | HTTP PUT request |
| `http.patch` | url, body | dict | HTTP PATCH request |
| `http.delete` | url | dict | HTTP DELETE request |
| `http.request` | method, url, options | dict | Generic HTTP request |
| `http.response` | status, body | dict | Construct `{status, body}` response |
| `http.error_response` | status, code, message | dict | Construct error response with JSON body |
| `http.extract_path` | request | text | Extract `.path` from request object |
| `http.extract_params` | request | dict | Extract `.params` from request object |

### 15.10 `http.server.*` — HTTP server
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `http.server.listen` | port | http_server | Bind TCP listener, returns server handle |
| `http.server.accept` | http_server | dict | Accept connection: `{method, path, query, headers, body, conn_id}` |
| `http.server.respond` | http_conn, status, headers, body | bool | Write HTTP response |
| `http.server.close` | http_server | bool | Close server handle |

### 15.10b `http.respond.*` — Response Convenience Ops
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `http.respond.html` | http_conn, status, body | bool | Respond with `text/html; charset=utf-8` |
| `http.respond.json` | http_conn, status, body | bool | Respond with `application/json` |
| `http.respond.text` | http_conn, status, body | bool | Respond with `text/plain; charset=utf-8` |

### 15.11 `ws.*` — WebSocket client
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `ws.connect` | url | ws_conn | TCP + WebSocket handshake, returns handle |
| `ws.send` | ws_conn, message | bool | Send text message |
| `ws.recv` | ws_conn | dict | Receive message: `{type, data}` |
| `ws.close` | ws_conn | bool | Close connection |

### 15.12 `headers.*` — HTTP header utilities
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `headers.new` | | `{}` | Empty headers |
| `headers.set` | headers, key, value | dict | Set header (lowercased key) |
| `headers.get` | headers, key | text | Get header value (lowercased key) |
| `headers.delete` | headers, key | dict | Remove header (lowercased key) |

### 15.13 `db.*` — SQLite database
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `db.open` | path | db_conn | Open SQLite (`:memory:` or file path) |
| `db.exec` | db_conn, sql [, params] | dict | Execute SQL, returns `{rows_affected}` |
| `db.query` | db_conn, sql [, params] | list | Query SQL, returns array of row objects |
| `db.close` | db_conn | bool | Close database connection |

### 15.14 `date.*` — Calendar date operations
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `date.now` | | dict | Current UTC date `{unix_ms, tz_offset_min}` |
| `date.now_tz` | offset | dict | Current date with given tz offset |
| `date.from_unix_ms` | ms | dict | Construct from Unix milliseconds |
| `date.from_parts` | y, mo, d, h, mi, s, ms | dict | Construct from parts (UTC) |
| `date.from_parts_tz` | y, mo, d, h, mi, s, ms, tz | dict | Construct from parts with tz |
| `date.from_iso` | text | dict | Parse ISO 8601 string |
| `date.from_epoch` | date, offset_ms | dict | Add offset to epoch date |
| `date.to_unix_ms` | date | long | Extract Unix milliseconds |
| `date.to_parts` | date | dict | Decompose: `{year, month, day, hour, min, sec, ms, tz_offset_min}` |
| `date.to_iso` | date | text | Format as ISO 8601 |
| `date.to_epoch` | date1, date2 | long | Difference in ms |
| `date.weekday` | date | long | ISO weekday (1=Mon, 7=Sun) |
| `date.with_tz` | date, offset | dict | Change tz offset, preserving instant |
| `date.add` | date, ms | dict | Add milliseconds |
| `date.add_days` | date, days | dict | Add N days |
| `date.diff` | date1, date2 | long | Difference in ms |
| `date.compare` | date1, date2 | long | Returns -1, 0, or 1 |

### 15.15 `stamp.*` — Monotonic timestamp (nanosecond)
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `stamp.now` | | dict | Current time as `{ns}` |
| `stamp.from_ns` | ns | dict | Construct from nanoseconds |
| `stamp.from_epoch` | stamp, offset_ns | dict | Epoch + offset |
| `stamp.to_ns` | stamp | long | Extract ns |
| `stamp.to_ms` | stamp | long | Convert to ms |
| `stamp.to_date` | stamp | dict | Convert to date object |
| `stamp.to_epoch` | stamp1, stamp2 | long | Difference from epoch |
| `stamp.add` | stamp, ns | dict | Add nanoseconds |
| `stamp.diff` | stamp1, stamp2 | long | Difference in ns |
| `stamp.compare` | stamp1, stamp2 | long | Returns -1, 0, or 1 |

### 15.16 `trange.*` — Time range operations
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `trange.new` | start, end | dict | Create range (validates start <= end) |
| `trange.start` | range | dict | Extract start date |
| `trange.end` | range | dict | Extract end date |
| `trange.duration_ms` | range | long | End - start in ms |
| `trange.contains` | range, date | bool | Inclusive containment check |
| `trange.overlaps` | range1, range2 | bool | Overlap check |
| `trange.shift` | range, ms | dict | Shift both bounds by ms |

### 15.17 `file.*` — File I/O
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `file.read` | path | text | Read file contents |
| `file.write` | path, content | bool | Write file |
| `file.append` | path, content | bool | Append to file |
| `file.delete` | path | bool | Delete file |
| `file.exists` | path | bool | Check if path exists |
| `file.list` | path | list | List directory entries |
| `file.mkdir` | path | bool | Create directory (recursive) |
| `file.copy` | src, dst | bool | Copy file |
| `file.move` | src, dst | bool | Move/rename file |
| `file.size` | path | long | File size in bytes |
| `file.is_dir` | path | bool | Check if path is a directory |

### 15.18 `term.*` — Terminal I/O
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `term.print` | text | bool | Print to stdout |
| `term.prompt` | text | text | Read line from stdin |
| `term.clear` | | bool | Clear terminal |
| `term.size` | | dict | Terminal dimensions `{cols, rows}` |
| `term.cursor` | | dict | Cursor position `{col, row}` |
| `term.move_to` | col, row | bool | Move cursor |
| `term.color` | text, color | text | ANSI-colored text |
| `term.read_key` | | text | Read single keypress |

### 15.19 `time.*` — Time utilities
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `time.sleep` | seconds | bool | Async sleep (f64, supports fractional) |
| `time.split_hms` | decimal_hours | dict | Split into `{h, m, s}` |

### 15.20 `fmt.*` — Formatting helpers
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `fmt.pad_hms` | hms | text | Format `{h, m, s}` as `"HH:MM:SS"` |
| `fmt.wrap_field` | name, value | dict | Wrap value as `{name: value}` |

### 15.21 `env.*` — Environment variables
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `env.get` | name [, default] | text | Get env var (optional default) |
| `env.set` | name, value | bool | Set env var |
| `env.has` | name | bool | Check if env var exists |
| `env.list` | | dict | All env vars as dict |
| `env.remove` | name | bool | Remove env var |

### 15.22 `exec.*` — Process execution
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `exec.run` | command, args_list | dict | Run process: `{code, stdout, stderr, ok}`. Command and args must be separate. |

### 15.23 `regex.*` — Regular expressions
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `regex.match` | pattern, text | bool | Boolean match test |
| `regex.find` | pattern, text | dict | First match: `{matched, text, groups}` |
| `regex.find_all` | pattern, text | list | All matches as string array |
| `regex.replace` | pattern, text, replacement | text | Replace first match |
| `regex.replace_all` | pattern, text, replacement | text | Replace all matches |
| `regex.split` | pattern, text | list | Split by pattern |

### 15.24 `random.*` — Random and UUID
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `random.int` | min, max | long | Random integer in `[min, max]` |
| `random.float` | | real | Random float in `[0, 1)` |
| `random.uuid` | | text | UUID v4 string |
| `random.choice` | list | value | Random element from array |
| `random.shuffle` | list | list | Fisher-Yates shuffle |

### 15.25 `hash.*` — Cryptographic hashes
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `hash.sha256` | text | text | SHA-256 hex digest |
| `hash.sha512` | text | text | SHA-512 hex digest |
| `hash.hmac` | key, data [, algo] | text | HMAC (default sha256) |

### 15.26 `base64.*` — Base64 encoding
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `base64.encode` | text | text | Standard base64 encode |
| `base64.decode` | text | text | Standard base64 decode |
| `base64.encode_url` | text | text | URL-safe no-pad encode |
| `base64.decode_url` | text | text | URL-safe no-pad decode |

### 15.27 `crypto.*` — Bcrypt, JWT, secure random
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `crypto.hash_password` | password | text | Bcrypt hash (cost 12) |
| `crypto.verify_password` | password, hash | bool | Bcrypt verify |
| `crypto.sign_token` | payload, secret | text | JWT HS256 sign |
| `crypto.verify_token` | token, secret | dict | JWT verify: `{valid, payload}` or `{valid: false, error}` |
| `crypto.random_bytes` | count | text | N random bytes as hex (1-1024) |

### 15.28 `log.*` — Level-based logging
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `log.debug` | message [, context] | bool | Log to stderr with timestamp |
| `log.info` | message [, context] | bool | Log to stderr with timestamp |
| `log.warn` | message [, context] | bool | Log to stderr with timestamp |
| `log.error` | message [, context] | bool | Log to stderr with timestamp |
| `log.trace` | message [, context] | bool | Log to stderr with timestamp |

### 15.29 `error.*` — Structured error construction
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `error.new` | code, message [, details] | dict | Create `{code, message}` |
| `error.wrap` | error, context | dict | Prepend context to message |
| `error.code` | error | text | Extract `.code` |
| `error.message` | error | text | Extract `.message` |

### 15.30 `cookie.*` — HTTP cookies
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `cookie.parse` | header | dict | Parse `"k=v; k2=v2"` |
| `cookie.get` | cookies, name | text | Get cookie value |
| `cookie.set` | name, value, options | text | Build Set-Cookie header |
| `cookie.delete` | name, options | text | Build delete cookie header (Max-Age=0) |

### 15.31 `url.*` — URL parsing
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `url.parse` | url | dict | Parse: `{path, query, fragment}` |
| `url.query_parse` | query | dict | Parse query string (percent-decodes) |
| `url.encode` | text | text | Percent-encode |
| `url.decode` | text | text | Percent-decode (handles `+` as space) |

### 15.32 `route.*` — URL path matching
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `route.match` | pattern, path | dict | Match with `:param`/`*wildcard`: `{matched, params}` |

### 15.33 `html.*` — HTML escaping
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `html.escape` | text | text | Escape `& < > " '` |
| `html.unescape` | text | text | Unescape HTML entities |

### 15.34 `tmpl.*` — Template rendering
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `tmpl.render` | template, data | text | Mustache-style: `{{var}}`, `{{{raw}}}`, `{{#section}}`, `{{^inverted}}`, dot paths, list iteration |

### 15.35 `auth.*` — Auth simulation
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `auth.extract_email` | params | text | Extract `.email` from params |
| `auth.extract_password` | params | text | Extract `.password` from params |
| `auth.validate_email` | email | bool | Check email contains `@` and `.` |
| `auth.validate_password` | password | bool | Check length >= 8 |
| `auth.verify_password` | password, creds | bool | Compare against `password_hash` in creds |
| `auth.sample_checks` | | list | Returns `[true, true, true]` |
| `auth.pass_through` | value | value | Returns first arg unchanged |

### 15.36 `accept` — Generic accept dispatch
| Op | Args | Returns | Description |
|----|------|---------|-------------|
| `accept` | handle | dict | Dispatches by handle prefix: `srv_` for HTTP accept, `ws_` for WebSocket recv |

## 16. Compiler Modes

### 16.1 `compile` — Compile to IR JSON
```
forai compile <file.fa> [-o <out.json>] [--compact]
```
- Outputs IR JSON to stdout or file.
- `--compact` produces minified JSON.
- Also generates a `docs/` folder as a side effect.

### 16.2 `run` — Execute a flow
```
forai run <file.fa> [args...] [--input <input.json>] [--report <file>]
```
- Positional args are mapped to flow input ports in order.
- `--input` loads inputs from a JSON file.
- `--report` writes the full `RunReport` (trace, outputs) to a JSON file.

### 16.3 `test` — Run test blocks
```
forai test <path>
```
- Accepts a file or directory (defaults to `.` if omitted).
- Runs all `test` blocks found in `.fa` files.
- Prints pass/fail summary with timing. Exits with error code if any fail.

### 16.4 `doc` — Generate documentation
```
forai doc <path> [-o <out.json>]
```
- Produces a `DocsArtifact` JSON with modules, symbols, types, tests.
- Also generates a `docs/` folder tree with per-file, per-namespace, and stdlib docs.

### 16.5 `dev` — Interactive debugger
```
forai dev <file.fa> [--input <input.json>] [--port N]
```
- Launches a WebSocket-based debug server (default port 481).
- Opens a browser with an interactive step-through UI.
- Auto-detects `<stem>.input.json` for input values.
- Protocol: `step`, `continue`, `run_to_breakpoint`, `set_breakpoints`, `restart`.
- UI shows: source code, graph visualization (SVG), variable bindings, execution trace.

## 17. Diagnostics Contract
- Errors must include `file:line:column`.
- For failed `must`, runner reports:
  - expression text
  - resolved operand values (when available)
  - test docs summary (first line of the test's docs block)

## 18. IR Shape

The compiled IR JSON contains:
- `forai_dataflow`: version string (`"0.1"`)
- `flow`: the flow name
- `inputs`: list of `{name, type}` — input port declarations
- `outputs`: list of `{name, type}` — output port declarations
- `nodes`: list of `{id, op, bind, args, when}` — computation steps with guard conditions
- `edges`: list of `{from, to, when}` — graph wiring; endpoints have `{kind, id, port?}` where kind is `"input"`, `"node"`, `"output"`, or `"loop_item"`
- `emits`: list of `{output, value_var, when}` — output routing with guards

### 18.1 IR lowerer scoping rules
- Variables assigned inside `case` arms are not visible outside the case block (scope is discarded).
- Variables assigned inside `loop` bodies can reference and reassign outer variables.
- `sync` exports are explicitly mapped back to the outer scope.
- `break` is a no-op in the IR (runtime-only construct).

## 19. Non-Goals
- Anonymous expression-level `data ... done` values.
- Full type inference across modules.
- Optimizing scheduler for high-throughput production workloads.
- Short-circuit evaluation of `&&` and `||`.

## 20. Conformance Gate
Compiler is conformant when all are true:
- Grammar above parses and validates.
- Docs/test enforcement rules pass.
- Semantic rules (section 14) are enforced.
- `sync/case/loop/break/trap/must` semantics match this spec.
- All CLI commands in section 16 are implemented.
- Built-in ops in section 15 are implemented.
