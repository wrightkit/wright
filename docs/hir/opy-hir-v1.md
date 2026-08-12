# Opy HIR v1 — Wright frontend protocol

Status: accepted baseline for v0.1
Scope: the interchange format between the temporary OverPy frontend adapter
and the Wright Rust core

This document is the normative specification for `wright/opy-hir` protocol
version `1.0.0`. It defines the JSON payload that the compatibility adapter
(`adapter/`) emits and that the Rust core (`crates/wright-core/src/hir/`)
validates and consumes. The adapter is the only component allowed to know how
an OverPy AST maps onto this schema; the Rust core sees only this protocol.

The protocol is a Wright-owned contract. It is not `JSON.stringify()` of an
OverPy AST, and no node in it is named after an OverPy-internal class. Node
kinds, operator spellings, and structural choices are Wright's.

## 1. Goals

The protocol must:

1. describe the parsed program semantics the v0.1 compatibility corpus needs:
   declarations, rules, events, conditions, statements, and expressions;
2. preserve file, line, and column provenance so later stages can report
   diagnostics against source;
3. be deterministic: the same source, frontend version, and adapter version
   produce byte-identical JSON;
4. be versioned so a producer and consumer can agree on compatibility without
   inspecting each other's implementation;
5. fail loudly on constructs the adapter cannot map, and be rejected or
   reported by the consumer rather than silently ignored.

## 2. Protocol envelope

Every payload is a JSON object with the following top-level fields.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `protocol` | object | yes | Protocol identity and version. |
| `generator` | object | yes | Producer identity for provenance. |
| `files` | array | yes | Source-file registry referenced by spans. |
| `defines` | array | no | Preprocessor constant/function macros seen by the frontend. |
| `declarations` | array | yes | Symbols declared at program scope, grouped by kind, each group in declaration order. |
| `rules` | array | yes | Rule and subroutine-definition bodies, in source order. |

### 2.1 `protocol`

```jsonc
{
  "name": "wright/opy-hir",
  "version": "1.0.0"
}
```

* `name` must be exactly `wright/opy-hir`.
* `version` is a semantic version (`major.minor.patch`). The major component
  is the compatibility boundary described in §7. The `1` in `v1` refers to
  this major version.

### 2.2 `generator`

```jsonc
{
  "name": "wright-overpy-adapter",
  "version": "0.1.0",
  "frontend": "overpy@9.7.10"
}
```

* `name` identifies the producer.
* `version` is the producer's own version.
* `frontend` records the exact external frontend identity (package and
  version) the producer translated from, so compatibility evidence can name
  the reference.

### 2.3 `files`

```jsonc
[
  { "id": 0, "path": "source.opy" },
  { "id": 1, "path": "shared.opy" }
]
```

* `id` is a non-negative integer, unique within the payload.
* `path` is the file name as the frontend reported it, unique within the
  payload. Paths are recorded for diagnostics; they are not canonicalized by
  the protocol.

### 2.4 `defines`

Preprocessing definitions (`#!define` constants and function macros) that the
frontend expanded before parsing. They are recorded for provenance so a
diagnostic can explain where a value came from; they carry no semantic
payload because expansion already happened.

```jsonc
{ "name": "CAKE_SIDE_LENGTH", "isFunction": false, "span": { "file": 0, "start": { "line": 10, "col": 1 }, "end": { "line": 10, "col": 24 } } }
```

## 3. Source provenance

Every node that originates from source carries a `span`. A span is a
half-open interval in a file:

```jsonc
{ "file": 0, "start": { "line": 6, "col": 5 }, "end": { "line": 6, "col": 21 } }
```

* `file` indexes into `files`.
* `line` and `col` are 1-based. `end` is exclusive: it is the position just
  past the last character of the node.
* A synthetic node (for example a compiler-generated initializer) carries the
  span of the source text that caused it to exist, or is omitted when no
  source text exists.

Spans are for diagnostics and identity, not for byte-accurate reconstruction.
The adapter is responsible for producing them; the consumer validates them
(§8). A span whose end would precede its start (for example a node expanded
from a preprocessor macro that mixes call-site and definition-site positions)
must be normalized to a degenerate interval anchored at the start, so every
emitted span is structurally valid.

## 4. Declarations

Declarations appear in `declarations` in source order. Each is an object
discriminated by `kind`. All kinds carry `name` and `span` unless noted.

### 4.1 `globalVariable` / `playerVariable`

```jsonc
{
  "kind": "globalVariable",
  "name": "score",
  "index": null,
  "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 17 } },
  "initializer": null
}
```

* `index` is the explicit index the source requested (`globalvar x 5`), or
  `null` when the frontend assigns it later.
* `initializer` is an expression or `null`. It is present only when the source
  provided a non-trivial initializer; the frontend's implicit defaults are
  not emitted.

### 4.2 `subroutine`

A subroutine declaration (`subroutine name`), independent of any `def`.

```jsonc
{ "kind": "subroutine", "name": "showStatus", "index": null, "span": { ... } }
```

### 4.3 `subroutineDef`

A subroutine definition (`def name():`) with its statement body. A definition
is a program body, so it appears in `rules` (§5) rather than in
`declarations`; this section defines its node shape.

```jsonc
{
  "kind": "subroutineDef",
  "name": "showStatus",
  "span": { ... },
  "body": [ /* statements */ ]
}
```

### 4.4 `constant`

A source-level constant (`macro name = value`), kept so constant references
stay resolvable.

```jsonc
{
  "kind": "constant",
  "name": "PI",
  "span": { ... },
  "value": { /* expression */ }
}
```

### 4.5 `macro`

A source-level function macro (`macro name(a, b):`). Macro *calls* remain
explicit in expressions (§6.9); the definition is recorded so a later stage
can expand or lower it without re-parsing source.

```jsonc
{
  "kind": "macro",
  "name": "double",
  "args": ["value"],
  "span": { ... },
  "body": [ /* statements */ ]
}
```

## 5. Rules

A rule is an object with the fields below. Each entry in `rules` is either a
rule object or a `subroutineDef` node (§4.3). Rules appear in source order.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `name` | string | yes | The rule name as written (empty is allowed for delimiter rules). |
| `span` | span | yes | The `rule` line. |
| `disabled` | boolean | yes | `true` when the rule is disabled by annotation. |
| `event` | event | yes | The rule's event. |
| `conditions` | array | yes | `@Condition` expressions, in source order. |
| `actions` | array | yes | Statements, in source order. |

### 5.1 Event

An event is an object with `name`, `args`, and `span`:

```jsonc
{ "name": "global", "args": [], "span": { ... } }
{ "name": "eachPlayer", "args": [], "span": { ... } }
{ "name": "onFlag", "args": [ { "kind": "string", "value": "FLAG", "span": { ... } } ], "span": { ... } }
```

`name` is the event keyword as written. `args` are the event's parameters as
expressions.

## 6. Statements and expressions

Statements and expressions are JSON objects discriminated by `kind`. A node
whose `kind` the consumer does not recognize is an *unsupported node* (§7.3).

### 6.1 Statement kinds

| Kind | Fields | Meaning |
| --- | --- | --- |
| `expr` | `expr`, `span` | An expression statement (typically a call with side effects). |
| `assign` | `target`, `value`, `span` | Assignment. Compound assignments are desugared by the frontend. |
| `if` | `branches`, `else`, `span` | Conditional. `branches` is an array of `{ "condition", "body" }`; `else` is an array of statements or `null`. |
| `for` | `variable`, `iterable`, `body`, `span` | Iteration. `variable` is an expression naming the loop variable (a `globalVar` reference). |
| `while` | `condition`, `body`, `span` | Loop. |
| `callSubroutine` | `name`, `span` | Call a subroutine by name. |
| `pass` | `span` | A no-op emitted by the frontend. |

Example `for` with `if`:

```jsonc
{
  "kind": "for",
  "variable": { "kind": "globalVar", "name": "index", "span": { ... } },
  "iterable": { "kind": "call", "name": "range", "args": [ { "kind": "number", "value": 3, "text": "3", "span": { ... } } ], "span": { ... } },
  "body": [
    {
      "kind": "if",
      "branches": [
        { "condition": { "kind": "binary", "op": "==", "left": { ... }, "right": { ... }, "span": { ... } }, "body": [ { "kind": "expr", "expr": { "kind": "call", "name": "debug", "args": [ { ... } ], "span": { ... } }, "span": { ... } } ] }
      ],
      "else": null,
      "span": { ... }
    }
  ],
  "span": { ... }
}
```

### 6.2 Expression kinds — literals

| Kind | Fields | Meaning |
| --- | --- | --- |
| `number` | `value`, `text`, `span` | Numeric literal. `value` is the JSON number; `text` is the source spelling. |
| `string` | `value`, `span` | String literal without format placeholders. |
| `bool` | `value`, `span` | `true` or `false`. |
| `null` | `span` | The null literal. |
| `array` | `elements`, `span` | Array literal, possibly empty. |
| `vector` | `x`, `y`, `z`, `span` | Vector literal (`vect(x, y, z)`). |
| `enum` | `type`, `value`, `span` | A built-in enumerated value, e.g. `Team.ALL`, `Color.WHITE`, `Beam.GRAPPLE`. `type` is the value domain, `value` the member name. |

### 6.3 Expression kinds — references

| Kind | Fields | Meaning |
| --- | --- | --- |
| `globalVar` | `name`, `span` | Reference to a global variable. |
| `playerVar` | `player`, `name`, `span` | Reference to a player variable on `player` (an expression). |
| `eventPlayer` | `span` | The `eventPlayer` pseudo-symbol. |
| `constant` | `name`, `span` | Reference to a source-level constant. |

### 6.4 Expression kinds — operations

| Kind | Fields | Meaning |
| --- | --- | --- |
| `call` | `name`, `args`, `span` | Function call. `name` is the function name; member calls use `receiver` (below). |
| `receiverCall` | `receiver`, `name`, `args`, `span` | Member/extension call: `receiver.name(args)`. |
| `macroCall` | `name`, `args`, `span` | A source-level macro invocation kept explicit. |
| `macroParam` | `name`, `span` | A reference to a macro parameter inside a macro definition body. |
| `binary` | `op`, `left`, `right`, `span` | Binary operation. `op` is one of `+ - * / % ** == != < <= > >= and or`. |
| `unary` | `op`, `operand`, `span` | Unary operation. `op` is `-` or `not`. |
| `index` | `array`, `index`, `span` | Indexing `array[index]`. |
| `format` | `text`, `args`, `span` | A string with `{0}`, `{1}` style placeholders and their argument expressions. |

### 6.5 Operator semantics

Operators are Wright spellings for the semantics the frontend parsed:

* arithmetic: `+ - * / % **`;
* comparison: `== != < <= > >=` (non-strict, Workshop semantics);
* logical: `and or`, with `not` as a unary operator.

The adapter maps frontend nodes (`__add__`, `__equals__`, `__lessThan__`,
... ) onto these spellings. The consumer treats `op` as an opaque string and
validates it only structurally (§8).

## 7. Versioning and compatibility rules

### 7.1 Version meaning

The protocol version is semver. Within the same major version:

* **Additive change** — a producer may add new optional fields to existing
  nodes and new `kind` variants for constructs the consumer can treat as
  opaque *only if* the consumer is updated to understand them. A consumer
  must reject a `kind` it does not recognize (§7.3), so an additive change
  ships with a matching consumer update and does not require a major bump.
* **Breaking change** — removing or renaming a node, changing the meaning of
  a field, or changing required-ness is a major-version change. Consumers of
  an older major version must reject the payload before inspecting its
  contents.

Minor and patch versions describe producer-visible evolution inside a major
version (documentation, new optional producer metadata) and do not change the
node grammar.

### 7.2 Major-version handling

A consumer must check `protocol.name` and `protocol.version` before any other
validation. If `name` is not `wright/opy-hir`, or the major version is not
supported, the consumer returns a structured *incompatible protocol* error
that names the expected and received identity. It must not attempt to parse
the program body.

### 7.3 Unsupported nodes

A node with an unknown `kind` (or an unknown statement/expression variant) is
an *unsupported node*. The consumer reports a structured error that names the
node kind and its span, so a regression report is explicit. Unsupported is
never a silent pass: the adapter refuses to emit nodes it cannot map, and the
consumer refuses to consume nodes it cannot understand.

## 8. Validation requirements

A consumer must validate, in order:

1. **Envelope**: `protocol` identity and major version (§7.2).
2. **Shape**: the payload is a JSON object with the required top-level fields;
   `files`, `declarations`, `rules` are arrays.
3. **Provenance**: every span's `file` indexes an entry in `files`; line and
   column values are ≥ 1; `end` is not before `start`.
4. **Identifiers**: `declarations` names are non-empty strings; within a
   declaration kind, names are unique; rule names are strings (may be empty);
   `defines` names are unique.
5. **References**: `globalVar`, `playerVar`, and `constant` references resolve
   to a matching declaration; `callSubroutine` references resolve to a
   `subroutine` declaration or a `subroutineDef` in `rules`; loop variables in
   `for` resolve to a global variable.
6. **Unsupported nodes**: unknown node kinds produce the §7.3 error.

Validation failures are structured: they carry a stable code, a message, and
the offending span or path when available. Human-readable wording is not part
of the stable contract; the code and structured fields are.

## 9. Determinism and debug output

For the same input, frontend version, and adapter version, the producer must
emit byte-identical JSON: object keys are emitted in a fixed order and
collections (files, declarations, rules, branches, args) preserve source
order. The consumer's debug dump (§10) must be stable for the same validated
payload so tests and issue reports can compare dumps byte-for-byte.

## 10. Debug dump

The consumer provides a deterministic, human-readable rendering of a
validated payload, intended for tests and issue reports. It is an
implementation-defined presentation, not part of the wire contract. It must:

* be reproducible byte-for-byte for the same validated payload;
* show protocol identity, files, declarations, rules, events, conditions,
  statements, and expressions with their spans; and
* print in a stable order matching the payload order.

## 11. Out of scope for v1

The following are intentionally not modeled in v1 and are rejected by the
adapter as unsupported when encountered:

* rule labels and relative gotos (`__skip__` / `__distanceTo__` forms);
* decompilation-only constructs;
* custom game settings blocks (`settings { ... }`);
* semantic analysis beyond structural validation (type checking, dead code,
  optimization).

These are not promises; they are the v0.1 boundary. A construct that appears
in the corpus and is not listed here is a bug in this specification, not a
reason to extend the schema silently.

## 12. Ownership

* The protocol contract is owned by Wright and lives in this document.
* The adapter (`adapter/`) owns all knowledge of how OverPy ASTs map to this
  schema. It is an optional, external-frontend component: the Rust core never
  imports it and never depends on OverPy types.
* Changes to the node grammar require a review of this document, the adapter,
  the Rust consumer, and the corpus fixtures together (see
  [`ARCHITECTURE.md`](../../ARCHITECTURE.md) and
  [`LICENSE-BOUNDARY.md`](../../LICENSE-BOUNDARY.md)).
