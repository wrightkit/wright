# Wright Agent Contract

- Status: stable, versioned session contract
- Contract: `wright-agent/v1`
- Schema: [`schemas/wright-agent-v1.schema.json`](../schemas/wright-agent-v1.schema.json)

`wright-agent/v1` is the transport-neutral request and response contract of
`wright_driver::service::ToolService`. Its Rust `ToolRequest` and `ToolResponse`
types are the executable definition. `capabilities` reports the contract
identity and supported operations before a client uses them.

The separate `wright-result/v1` contract describes command result envelopes.
`Capabilities.contract` continues to identify that result envelope;
`Capabilities.agent_contract` identifies this service contract. Results from
`compile`, `check`, `analyze`, and `inspect` carry a `wright-result/v1`
envelope inside the agent response.

## Session transports

Start a session with `wright serve [INPUT]`. `INPUT` is a source file or
project directory; omission uses the current directory. Standard input is
reserved for requests, so `-` is not a valid source input. The server binds
the configured project and processes requests until standard input closes.
Starting does not require the project to load successfully (#512): a project
that is missing, unreadable, or malformed is retried by the next
program-reading request, while `capabilities`, `targetMetadata`, `lookup`,
and the `provider*` operations answer regardless. The default transport is
one JSON
request per line on stdio:

```json
{"op":"capabilities"}
{"op":"findings"}
{"op":"check"}
```

Stdio returns one `ToolResponse` per request. A successful response has a
`result` member; an application-level refusal has an `error` member containing
`code` and `message`. The schema defines supported request fields; current
deserialization ignores unknown fields for forward compatibility. Clients
should send only fields defined by the negotiated schema.

Use `wright serve --transport jsonrpc [INPUT]` for JSON-RPC 2.0. The canonical
agent method is `request`, with a `ToolRequest` in `params`:

```json
{"jsonrpc":"2.0","id":1,"method":"request","params":{"op":"findings"}}
```

Its JSON-RPC `result` contains the same successful service result as the stdio
`ToolResponse.result`. A service refusal is returned in that `result` as
`{"error":{"code":"...","message":"..."}}`. JSON-RPC framing errors use
the top-level `error` member and standard JSON-RPC error codes. The direct
JSON-RPC methods `compile`, `check`, `analyze`, and `inspect` remain aliases for
the corresponding operations and return the same `wright-result/v1` envelope
they returned before this contract was introduced.

One-shot CLI workflows such as `wright check --format json` continue to return
their `wright-result/v1` envelope. They use the same `CompilerSession`
workflows as the session service. The semantic query commands `wright inspect
symbols`, `wright inspect refs`, `wright inspect cfg`, `wright inspect
callgraph`, and `wright inspect cost` likewise run through `ToolService`
operations (`symbols`, `references` + `usage`, `cfg`, `callGraph`,
`costEstimate`) and report the same result payloads (#429); the CLI adds no
divergent semantics. The in-process embedding API can call
`ToolService::handle` directly.

Use `wright serve --transport mcp [INPUT]` for MCP over stdio (#473,
ADR-0020). The adapter speaks newline-delimited JSON-RPC 2.0 and implements
`initialize` (with protocol-version negotiation), `ping`, `tools/list`, and
`tools/call`. Each operation in the initial set below is one tool named
`wright_` plus the operation in snake case:

| Operation | Tool |
| --- | --- |
| `project` | `wright_project` |
| `symbols` | `wright_symbols` |
| `references` | `wright_references` |
| `usage` | `wright_usage` |
| `callGraph` | `wright_call_graph` |
| `check` | `wright_check` |
| `analyze` | `wright_analyze` |
| `inspect` | `wright_inspect` |
| `lint` | `wright_lint` |
| `costEstimate` | `wright_cost_estimate` |
| `semanticRename` | `wright_semantic_rename` |
| `validateEditTransaction` | `wright_validate_edit_transaction` |
| `providerSemanticRename` | `wright_provider_semantic_rename` |
| `providerValidateEdit` | `wright_provider_validate_edit` |
| `lookup` | `wright_lookup` |

`tools/list` contains a tool only when its operation is in this set and
advertised by `capabilities.operations`. `initialize`, `tools/list`, and the
tool set itself are independent of whether the configured project currently
loads — a broken project never hides tools (#512). Each tool's `inputSchema`
is derived from the operation's request schema with `op` removed (the tool
name carries it); the Workshop edit tools also omit `sources`, which then
defaults to the on-disk text (#472). The provider tools keep `documents` and
`sources` required — the caller owns the document set. `tools/call`
arguments are the request fields.

A successful service `result` is returned unchanged as the tool result's JSON
text content. A service refusal is a tool result with `isError: true` whose
content carries the same `{code, message}`. Edit operations report their
outcome inside the result payload (`ok` plus `diagnostics`), which passes
through as ordinary content — the adapter does not reinterpret it. MCP
protocol errors (unknown tool or method, malformed arguments) are JSON-RPC
`error` responses and never become tool results.

The released `wright` binary includes `serve`, so each supported installation
channel can use the session contract without a separate runtime.

### Client tool definitions for code-executing agents (#535)

`wright agent tools` emits every operation `capabilities` advertises as
client tool definitions for code-executing agents. The output is versioned
with `wright-agent/v1` and deterministic: identical input produces identical
bytes.

`--format messages` (the default) emits
`{"contract": "wright-agent/v1", "tools": [...]}` in the Anthropic Messages
API client tool shape: each tool carries `name` (`wright_` plus the
snake-case operation), a `description` that also describes the result in
text, an `input_schema` derived from the operation's request schema with
`op` removed, and `allowed_callers: ["code_execution_20260120"]` so the
tools are callable from the code execution tool. Emitted schemas keep every
caller-meaningful field — including `sources`, which the MCP surface drops —
and ship their `$ref` targets under a local `$defs` with no recursive
reference, which the Messages API rejects.

`--format json-schema` emits
`{"contract": "wright-agent/v1", "schemas": {...}}`: operation name to the
same standalone request schema with its description inside, for harnesses
that consume plain JSON Schema.

The emitted set is generated from one catalog shared with `capabilities`
and the MCP adapter, so the definitions cannot drift from the advertised
operation set; a catalog change that drops a description, a request schema,
or introduces a recursive `$ref` fails the tests.

## Operations

All requests are JSON objects with a required `op` string. Request fields and
the common response/error shapes are defined by the committed JSON Schema. The
table lists every operation advertised by `capabilities.operations` and names
the successful `result` payload.

| Operation | Request fields | Successful `result` |
| --- | --- | --- |
| `capabilities` | none | Service name/version, `wright-agent/v1`, result contract, operation names, `result_schemas` (operation → committed-schema definition), languages, and profiles |
| `compile` | none | `wright-result/v1` compile envelope |
| `check` | none | `wright-result/v1` check envelope |
| `analyze` | optional `brief` | `wright-result/v1` analysis envelope; the brief form when `brief` is true |
| `inspect` | optional `brief` | `wright-result/v1` inspection envelope; the brief form when `brief` is true |
| `project` | none | Loaded program origin, files, counts, and findings summary |
| `rules` | optional selection | Canonical Workshop rules; `{"rules": [...], "selection": {...}}` when a selection is applied |
| `symbols` | optional selection | Symbols; `{"symbols": [...], "selection": {...}}` when `file`/`max` is applied (a `kind`-only request keeps the bare array) |
| `references` | required `symbol` (id or name), optional selection | References for the symbol; `{"references": [...], "selection": {...}}` when a selection is applied |
| `usage` | required `symbol` (id or name) | Usage counts for the symbol, plus its resolved `id` and `kind` |
| `cfg` | required `rule` (index or name), optional selection | Control-flow graph for the rule, plus `selection` when a selection is applied |
| `findings` | optional selection | Wright static-analysis findings; `{"findings": [...], "selection": {...}}` when a selection is applied |
| `persistentObjects` | none | Persistent Workshop object facts |
| `lint` | optional selection, optional `brief` | Lint findings, per-rule id/effective severity, effective configuration, and `selection` when applied; the brief form when `brief` is true |
| `lintRules` | none | Registered lint rules with full metadata and effective configuration |
| `callGraph` | optional selection | Subroutine call graph; `{"edges": [...], "selection": {...}}` when a selection is applied |
| `costEstimate` | optional selection | Exact generated-resource counts, findings, and `selection` when applied |
| `targetMetadata` | none | Canonical target/catalog metadata |
| `validateEditTransaction` | `transaction`, optional `sources` | Atomic validation status, diagnostics, and previews when valid |
| `semanticRename` | `target`, optional `sources` | Validated rename transaction or structured refusal |
| `providerSemanticRename` | `language_id`, `documents`, `position_document_uri`, `position`, `new_name`, optional `project_root`, `sources` | Provider-resolved rename transaction or structured refusal |
| `providerValidateEdit` | `language_id`, `documents`, `transaction`, `sources`, optional `project_root` | Provider-validated transaction or structured refusal |
| `lookup` | required `language`; optional `query`, `kind`, `within`, `locale`, `limit` | Owner vocabulary entries with accepted spellings and rendered signatures, or an explicit `unavailable` payload naming the owner |

### Freshness and id validity (#471, #512)

The service may exist without a program snapshot: construction over a
project that cannot load succeeds, and `capabilities`, `targetMetadata`
(the static catalog), `lookup` (the language vocabulary), and `provider*`
operations (caller-supplied documents) answer without consulting the
project at all — they neither require nor trigger its load.

A request that consults the program needs a valid current snapshot at
request time. When none exists, the request performs the deferred initial
load; when one exists, the service fingerprints the input's observable file
set — a file input's own content, a directory input's resolved members, or
every file under a source-language project's entry directory — and reloads
when it changes, so an edit, an added file, or a removed file is reflected
in the next `symbols`, `references`, `check`, `lint`, or other
program-reading request without restarting the session. An unchanged input
is never reloaded.

A load or reload that fails — the entry is missing or unreadable, or the
source does not parse — refuses the request with the loader's structured
diagnostic code (`input-io`, `parse-error`, `input-kind-ambiguous`, a
provider diagnostic). The service keeps refusing until the project loads
again, retrying on every program-reading request; the previously loaded
program is never served silently, and the service never substitutes an
empty or placeholder program. The failed attempt invalidates the served
snapshot: even restoring the input to byte-identical content loads a fresh
program rather than resurrecting the dropped one. Repairing the input
recovers the same running session — no restart and no explicit reload
request.

Numeric symbol ids and rule indexes are valid only for the program that
issued them. After a content-changing reload, a request carrying a numeric
`symbol`, `rule`, or `semanticRename` target id is refused `stale-id` until
the client observes the new space: a successful `symbols` response
re-establishes symbol ids, a successful `rules` response re-establishes
rule indexes, and an `ambiguous-symbol`/`ambiguous-rule` refusal
re-establishes its own space because it already names the current
candidates. A reload that succeeds after earlier attempts failed is no
exception: ids issued by the last served program do not silently validate
against the repaired one. Name addressing resolves against the current
program in both states and is never stale.

The responses that issue numeric ids or indexes into the loaded program are
`symbols` (symbol ids), `rules` (rule indexes), `usage` (the resolved `id`),
`references`, `findings`, `lint`, `persistentObjects`, `inspect`, `analyze`,
and the `ambiguous-*` refusal candidates — plus `skipped[].rule` in `lint`
and `lintRules`. `cfg` block ids are local to that one response and are not
program addresses; `project`, `callGraph`, `costEstimate`, `targetMetadata`,
`capabilities`, the `compile`/`check` envelopes, and the edit/provider
payloads issue no program ids. Positional fields such as `rule`, `action`,
`value`, and `span.file` inside findings and references are coordinates into
the current program rather than reusable addresses, but they are always
computed from the program that was live at request time.

### Name addressing (#429)

`references` and `usage` accept `symbol` as either a numeric symbol id or the
declared symbol name; `cfg` accepts `rule` as either the rule index or the
declared rule name. Names resolve against the loaded program's semantic index
in the driver, so a request never has to learn the program's numbering —
symbol ids and rule indexes are different spaces (a rule's symbol id is not
its rule index). Numeric ids keep their established meaning, and both
addressings return the same payload for the same target.

An unmatched name returns a structured `unknown-symbol` or `unknown-rule`
error; a name shared by more than one symbol — or more than one rule, for
`cfg` — returns `ambiguous-symbol`/`ambiguous-rule` listing the candidate
numeric ids. Resolution never guesses or returns an empty success. `usage`
additionally echoes the resolved `id` and `kind` so a name-addressed caller
can correlate the result with `symbols`.

### Finding selection (#430)

`findings`, `lint`, and `costEstimate` accept optional selection fields:

* `severity`: a threshold — `error` reports errors only, `warning` errors and
  warnings, `info` everything.
* `rule`: one lint rule id (the finding `code`). An unknown id is a
  structured `invalid-selection` error, never a silent empty result.
* `file`: one source file. The reported `span.path` spelling differs per
  surface, so the argument resolves to the same canonical file — the path as
  passed, root-relative, or absolute spellings all select it. `costEstimate`
  findings carry no span, so `file` selects nothing there.
* `max`: a bound on the reported count, applied after filtering.

The CLI options `--severity`, `--rule-id`, `--file`, and `--max` on `lint`,
`check`, and `analyze` drive the same `wright-driver` selection, so both
surfaces return the same selected set for the same input and selection.

When a request applies any selection field, the result reports
`selection: {"total": <set before selection>, "withheld": <dropped by max>}`:
`findings` becomes `{"findings": [...], "selection": {...}}`, while `lint`
and `costEstimate` add a `selection` member to their existing result objects.
Requests without selection fields receive the previous shapes unchanged.

### Semantic query selection (#531)

`rules`, `symbols`, `references`, `cfg`, and `callGraph` accept optional
selection fields with the same semantics as finding selection: filters narrow
first, `max` bounds after, and a `selection` member reports `{"total":
<set before selection>, "withheld": <dropped by max>}`. An unknown filter
value — a kind outside the listed domain, or a `name`/`rule`/`caller`/`callee`
that matches nothing in the loaded program — is a structured
`invalid-selection` error, never a silent empty result.

* `rules`: `name` (a declared rule name), `file`, `max`.
* `symbols`: `file`, `max`, plus the pre-existing `kind` (`globalVariable`,
  `playerVariable`, `subroutine`, `rule`). `kind` predates this selection
  contract: a request carrying only `kind` keeps the previous filtered bare
  array; `file`/`max` wrap the result, and a `kind` sent alongside them
  counts inside the reported selection.
* `references`: `kind` (`declaration`, `definition`, `read`, `write`,
  `call`), `rule` (only references inside this rule, addressed by index or
  declared name), `file`, `max`.
* `cfg`: `kind` (`entry`, `exit`, `block`, `if`, `while`, `for`), `max`.
  Selection filters `blocks`, which keep their original `id`s, so
  `successors` — and the `entry`/`exit` fields — still address the full
  graph's block numbering.
* `callGraph`: `caller` (a declared rule name), `callee` (a declared
  subroutine name), `max`.

The `file` field resolves like finding selection: any spelling that resolves
to the same source file selects it. Requests without selection fields receive
the previous shapes unchanged (bare arrays for `rules`, `symbols`,
`references`, `callGraph`; the unextended object for `cfg`) — including
`symbols` requests carrying only the pre-existing `kind` field. The CLI options
`--only`, `--rule`, `--file`, `--caller`, `--callee`, and `--max` on `inspect`
subcommands drive the same `wright-driver` selection.

### Brief results (#532)

`analyze`, `inspect`, and `lint` accept an optional `brief` field (the CLI's
`--brief`; defaults preserve the full result on every surface). A brief
result is the small object `{"brief": true, "program"?, "counts", "items",
"expand", "selection"?}`: `program` repeats the operation's program summary
when one exists, `counts` totals the collections the full result reports
(findings/risks by severity, rules, symbols, references, elements, skipped),
`items` carries at most five highest-priority entries — highest-severity
findings for `lint`, the costliest rules for `analyze`, the leading rules for
`inspect` — and `expand` names the way back to the full result. `lint`
composes `brief` with finding selection: selection narrows the finding set
first, the counts and items describe the selected set, and the `selection`
member keeps the pre-selection total.

CLI (`--brief`), `serve` (`"brief": true`), and the MCP tools
(`wright_analyze`, `wright_inspect`, `wright_lint` with `brief` in their
generated input schemas) return the same brief payload for the same input and
form. `capabilities.result_schemas` names the `$defs` definition describing
each advertised operation's result, including the brief form for these three.

Edit transactions use source identities and half-open, 1-based line/column
ranges. Provider positions use 0-based line/character coordinates. Wright
proposes and validates edits; a caller remains responsible for applying them.
Stale, overlapping, unsupported, or semantically invalid edits return an
explicit refusal without a partial edit set.

### Validated edits and semantic rename for raw Workshop (#434)

On raw Workshop input both operations are supported directly — `workshop-rs`
owns the source semantics and Wright orchestrates the transaction:

* `validateEditTransaction` applies the caller's transaction to the current
  `sources` and reparses/revalidates the edited project through the
  session's own `workshop-rs` path. `sources` is optional (#472): when
  omitted or null, the service reads the current text of the files the
  transaction names (each `edits[].source`) from disk; when supplied, it
  remains the precondition text, so an embedder holding unsaved buffers
  keeps that ability. A transaction that produces malformed or
  invalid Workshop refuses with the real parse/validation diagnostics and no
  partial preview; a valid one returns `ok: true` with a `preview` of each
  edited source (edited text plus the post-edit identity).
* `semanticRename` resolves `target` either by `symbol` — a numeric id or a
  declared name, exactly the addressing `references`/`usage` use — or by a
  `source`/`line`/`col` position inside one identifier occurrence. Global
  variables, player variables, and subroutines rename through the exact
  identifier spans `workshop-rs` records: the declaration, the `Subroutine`
  event binding, `Call Subroutine` callees, `Start Rule` subroutine
  arguments, `Set`/`Modify`/`For` variable
  arguments, and `Global.name`/`Event Player.name` value references all
  rewrite, while prefixes, comments, strings, and unrelated identifiers stay
  untouched. There is no textual-search fallback — an occurrence whose
  recorded span does not cover exactly the identifier refuses with
  `rename-unmapped-span`.
  `sources` is optional (#472): when omitted or null, the service reads the
  loaded input file's current text from disk; when supplied, it must carry
  the current text of the loaded input file. A text
  that differs from the loaded program's refuses with `edit-stale-source`
  because provenance spans would no longer index it. A `to` name that
  already declares a same-kind symbol refuses with `rename-name-collision`,
  and one that does not survive reparsing refuses with the parse diagnostic;
  rule names are not rename targets (`rename-unsupported-kind`).
* Non-Workshop input stays at the provider boundary: OPY refuses with
  `edit-requires-provider` naming `providerValidateEdit` /
  `providerSemanticRename`, and kinds without a shipped provider keep their
  `source-provider-unavailable` refusal.

`wright rename <NAME> <NEW_NAME> [INPUT]` exposes the same semantic rename
on the CLI: it prints the validated diff by default and applies it
atomically with `--write`, refusing `edit-stale-source` when the file
changed since validation.

### Name and signature lookup (#529, ADR-0021)

`lookup` resolves a free-text query — a display name, a near spelling, or a
guess — against the language owner's vocabulary, so an agent can learn the
accepted spelling and signature of a Workshop or OverPy name without
external documentation, source inspection, or `wright convert`. The
request's `language` selects the owner (`workshop` or `opy`, the ids
`capabilities.languages` reports); the remaining fields narrow the answer:

* `query`: the free text to resolve.
* `kind`: `action`, `value`, `event`, `enumMember`, or `setting`.
* `within`: the identity of an enum domain, a callable, or a settings path
  prefix. The result then lists its members, parameters, or child segments
  — this is how a caller enumerates an enum's accepted member spellings or
  a callable's parameters.
* `locale`: the Workshop display-name locale (session configuration, then
  the catalog's primary locale when omitted).
* `limit`: default 3, maximum 10.

Each entry carries its owner `identity`, `kind`, accepted `spelling`, and
`displayName`, plus the owner fact that describes it (`callable`, `enum`,
`parameter`, or `setting`). Callables additionally carry `signature`, a
human-readable call shape Wright renders from the owner's parameter facts:
required parameters name their type, optional parameters show `name?` or
`name=default`, and an enum domain lists its members inline (a domain
above the member bound renders as `Domain(count members)`).

Workshop answers come from the `workshop-rs` catalog in process. OverPy
answers come from the configured `opy-rs` provider's LPP lookup
capability; when the provider does not negotiate it, the result is an
explicit `unavailable` payload naming `opy-rs` — never a Wright-side
textual guess. An unmatched scope identity refuses with
`lookup.unknownWithin`; an unknown `language` or `kind` is a usage error.
The operation reads vocabulary alone, so it works over a session whose
project cannot load.

`wright lookup [QUERY]` exposes the same request on the CLI
(`--language`, `--kind`, `--within`, `--locale`, `--limit`): text mode
prints one signature or spelling per line, and `-f json` prints this
result payload. Text `check`/`compile` output points at the command after
an `unknown-*` diagnostic; structured payloads are unchanged.

## Results, diagnostics, and errors

The service response is either `{ "result": value }` or
`{ "error": { "code": string, "message": string } }`. Error codes are the
machine-readable discriminator; error message wording is for people and is not
stable. Transport framing errors are separate from service refusals.

Workflow envelopes retain `wright-result/v1` fields, including structured
diagnostics. Provider-owned diagnostics remain distinguishable through their
provider status and origin metadata. Wright findings are returned by the
findings/lint operations rather than converted into owner diagnostics. Source
spans retain their source path when mapped; missing or invalid source mapping
is reported as unmapped. Unsupported provider operations remain explicit
diagnostics or mutation refusals, not guessed source locations or textual
fallbacks.

`capabilities` is the version negotiation step. Clients should check
`agent_contract` and the advertised operation list before sending requests.
Clients should ignore unknown response fields and must not assume an operation
exists unless it is advertised.

## Versioning

`wright-agent/v1` permits additive optional response fields and additional
operations whose requests remain valid for existing clients. Removing or
renaming an operation or field, changing a field's type or meaning, or changing
the response/error model requires a new major contract such as
`wright-agent/v2`; the v1 schema and its compatibility tests remain in place.
The CLI result envelope has its independent `wright-result/v1` version; its
evolution policy is defined in [`docs/cli/machine-contract.md`](cli/machine-contract.md).

One recorded exception applies to the same rule in both contracts, decided
before the 1.0 contract freeze (#134): the `lint` result's `rules` member
lists only each rule's `id` and `effectiveSeverity` (#431). Full rule
metadata — summary, rationale, documentation, known limits, evidence, tags —
is served once by `lintRules`.

ADR-0020 records a second pre-freeze exception (#472): `sources` is optional
on `validateEditTransaction` and `semanticRename` rather than required, so
agent callers stop resending whole files.

The optional guide installed by `wright agent install` (and distributed by
`wrightkit/skills`) teaches clients to discover and use these capabilities. It
is not required to expose, execute, or validate any semantic operation.
