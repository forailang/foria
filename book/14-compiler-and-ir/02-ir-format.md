# Chapter 14.2: IR Format

The forai compiler produces a JSON intermediate representation (IR) that describes the compiled program as a dataflow graph. This chapter documents the IR schema and how to read it.

## Generating IR

To see the IR for any `.fa` file, run:

```bash
forai compile examples/read-docs/main.fa
```

Output goes to stdout as formatted JSON. To write it to a file:

```bash
forai compile examples/read-docs/main.fa -o out.json
```

To get minified JSON (smaller, for tooling):

```bash
forai compile examples/read-docs/main.fa --compact
```

## Top-Level Structure

```json
{
  "forai_dataflow": "0.1",
  "flow": "main",
  "inputs": [],
  "outputs": [
    { "name": "result", "type": "ServerResult" }
  ],
  "nodes": [ ... ],
  "edges": [ ... ],
  "emits": [ ... ]
}
```

| Field | Type | Description |
|-------|------|-------------|
| `forai_dataflow` | string | Schema version. Currently `"0.1"`. |
| `flow` | string | The name of the compiled flow. |
| `inputs` | array | Input port declarations (`name` + `type`). |
| `outputs` | array | Output port declarations (`name` + `type`). |
| `nodes` | array | Computation nodes in the graph. |
| `edges` | array | Directed edges connecting nodes. |
| `emits` | array | Output routing decisions. |

## nodes

Each node represents one computation step:

```json
{
  "id": "n_3",
  "op": "db.query",
  "bind": "rows",
  "args": {
    "conn": "conn",
    "sql": "SELECT * FROM users"
  },
  "when": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `id` | string | Unique node identifier (`"n_0"`, `"n_1"`, ...). |
| `op` | string | The operation name (built-in op, extern func, or user func). |
| `bind` | string or null | Variable name that receives the node's output. If the node has no output, `null`. |
| `args` | object | Map of argument names to values or variable references. |
| `when` | string or null | Guard condition expression. If non-null, node only executes when this expression is truthy. |

### Argument Values

Args can be literal values or references to variables:

```json
{
  "op": "db.exec",
  "bind": "ok",
  "args": {
    "conn": { "var": "conn" },
    "sql": { "literal": "INSERT INTO users (id) VALUES (?1)" },
    "params": { "var": "params" }
  }
}
```

In practice, the IR uses a compact representation where string values are variable names and literal values are prefixed or typed.

### Case Nodes

A `case/when` block becomes a sequence of nodes with `when` guards:

```json
{
  "id": "n_5",
  "op": "case_arm",
  "bind": null,
  "args": { "value": "route_name", "match": "users_list" },
  "when": "route_name == \"users_list\""
}
```

### Loop Nodes

A `loop items as item` becomes:

```json
{
  "id": "n_8",
  "op": "loop_start",
  "bind": "item",
  "args": { "list": "items" },
  "when": null
}
```

followed by the loop body nodes, and a loop edge back to the loop start.

## edges

Edges describe the flow of data between nodes:

```json
{
  "from": { "kind": "node", "id": "n_3", "port": null },
  "to": { "kind": "node", "id": "n_4", "port": null },
  "when": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `from` | endpoint | Source of the edge. |
| `to` | endpoint | Destination of the edge. |
| `when` | string or null | Guard condition. Edge is only traversed when this is truthy. |

### Endpoint Kinds

An endpoint object has:

```json
{ "kind": "node", "id": "n_3", "port": null }
```

| Kind | Meaning |
|------|---------|
| `"input"` | The flow's input port (the entry point). |
| `"node"` | A computation node, identified by `id`. |
| `"output"` | The flow's output port (an `emit` target). |
| `"loop_item"` | The loop item variable produced by a loop node. |

The `port` field names the specific output port of a node when a node has multiple outputs (e.g. a func with multiple `emit` options). For single-output nodes it is `null`.

### Edge Guards

Edges can carry guard conditions that mirror `when` clauses in branches:

```json
{
  "from": { "kind": "node", "id": "n_7" },
  "to": { "kind": "node", "id": "n_9" },
  "when": "handler == \"users_list\""
}
```

The runtime only traverses this edge when `handler == "users_list"` evaluates to `true`.

## emits

Each `emit` statement in the source produces an entry in the `emits` array:

```json
{
  "output": "result",
  "value_var": "final_result",
  "when": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `output` | string | The output port name (from the `emit X to :result` syntax, this is `"result"`). |
| `value_var` | string | The variable whose value is emitted. |
| `when` | string or null | Guard condition. The emit only fires when this is truthy. |

Multiple `emit` entries can exist for the same output port with different `when` conditions, representing conditional emit paths:

```json
{ "output": "result", "value_var": "ok_result", "when": "success == true" },
{ "output": "error", "value_var": "err_msg", "when": "success == false" }
```

## A Complete Example

Source:

```fa
func Add
    take a as long
    take b as long
    emit result as long
    fail error as text
body
    sum = a + b
    emit sum to :result
done
```

Compiled IR:

```json
{
  "forai_dataflow": "0.1",
  "flow": "Add",
  "inputs": [
    { "name": "a", "type": "long" },
    { "name": "b", "type": "long" }
  ],
  "outputs": [
    { "name": "result", "type": "long" },
    { "name": "error", "type": "text" }
  ],
  "nodes": [
    {
      "id": "n_0",
      "op": "add",
      "bind": "sum",
      "args": { "left": "a", "right": "b" },
      "when": null
    }
  ],
  "edges": [
    {
      "from": { "kind": "input", "id": null, "port": null },
      "to": { "kind": "node", "id": "n_0", "port": null },
      "when": null
    }
  ],
  "emits": [
    {
      "output": "result",
      "value_var": "sum",
      "when": null
    }
  ]
}
```

## Reading Real IR Output

To examine a real compiled program:

```bash
# Compile the factory example
forai compile examples/factory/main.fa -o /tmp/factory.json

# Pretty-print with jq
cat /tmp/factory.json | jq '.nodes | length'
cat /tmp/factory.json | jq '.nodes[] | select(.op == "db.query")'
cat /tmp/factory.json | jq '.edges[] | select(.when != null)'
```

The IR is stable across identical source compilations (given the same compiler version). It is safe to cache and diff.

## IR and the Doc Generator

`forai compile` has a side effect: it generates a `docs/` folder alongside the compiled output. The docs folder contains a JSON representation of all `docs` blocks in the module, extracted and structured for tooling. This is separate from the `forai doc` command (which generates structured documentation without running the full compiler pipeline).

## Versioning

The `forai_dataflow` version field is `"0.1"` in the current release. Future versions may add new node kinds, edge fields, or emit conditions. Tooling should check this field before parsing IR.
