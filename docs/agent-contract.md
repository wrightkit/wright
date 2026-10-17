# Wright agent contract

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

Startup options configure the session's input and lint policy before the
first request is served; an unknown or unreadable value is a startup usage
error (exit 2), never a silently remapped default:

* `--transport <stdio|jsonrpc|mcp>`: request transport (default `stdio`);
* `--kind <auto|opy|ostw|workshop|protocol>`: input frontend (default
  `auto`, with the workflow aliases `ws`, `hir`, `json`);
* `--profile <off|compat|aggressive>`: WIR transformation policy (default
  `off`);
* `--locale <LOCALE>`: Workshop client locale override;
* `--lint-config <PATH>`, `--rule <PATH>`, `--disable-rule <ID>`,
  `--rule-severity <ID>:<SEVERITY>`: explicit lint policy. The session applies
  the same file, local rule paths, and overrides `wright lint` accepts to every
  lint-consuming operation it serves (#594).

A configured session example: `wright serve --lint-config policy.yaml
--disable-rule repeated-value project.ws` serves `lint`/`lintRules`
results under `policy.yaml` plus the disabled rule, reporting the same
effective policy the equivalent `wright lint` invocation prints.

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
`tools/call`. Each operation in the set below is one tool named `wright_`
plus the operation in snake case:

| Operation | Tool |
| --- | --- |
| `project` | `wright_project` |
| `rules` | `wright_rules` |
| `symbols` | `wright_symbols` |
| `references` | `wright_references` |
| `usage` | `wright_usage` |
| `cfg` | `wright_cfg` |
| `findings` | `wright_findings` |
| `persistentObjects` | `wright_persistent_objects` |
| `callGraph` | `wright_call_graph` |
| `check` | `wright_check` |
| `analyze` | `wright_analyze` |
| `inspect` | `wright_inspect` |
| `lint` | `wright_lint` |
| `lintRules` | `wright_lint_rules` |
| `costEstimate` | `wright_cost_estimate` |
| `targetMetadata` | `wright_target_metadata` |
| `semanticRename` | `wright_semantic_rename` |
| `validateEditTransaction` | `wright_validate_edit_transaction` |
| `providerSemanticRename` | `wright_provider_semantic_rename` |
| `providerValidateEdit` | `wright_provider_validate_edit` |
| `lookup` | `wright_lookup` |

`tools/list` contains a tool only when its operation is in this set and
advertised by `capabilities.operations`. `initialize`, `tools/list`, and the
tool set itself are independent of whether the configured project currently
loads. A broken project never hides tools (#512). Each tool's `inputSchema`
is derived from the operation's request schema with `op` removed (the tool
name carries it); the Workshop edit tools also omit `sources`, which then
defaults to the on-disk text (#472), and the provider tools likewise omit
`documents` and `sources`, which then default to the session's loaded
project (#548). `tools/call`
arguments are the request fields.

A successful service `result` is returned unchanged as the tool result's JSON
text content. A service refusal is a tool result with `isError: true` whose
content carries the same `{code, message}`. Edit operations report their
outcome inside the result payload (`ok` plus `diagnostics`), which passes
through as ordinary content. The adapter does not reinterpret it. MCP
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
caller-meaningful field, including `sources`, which the MCP surface drops,
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
| `capabilities` | none | Service name/version, `wright-agent/v1`, result contract, operation names, `result_schemas` (operation to committed-schema definition), languages, and profiles |
| `compile` | optional `output` (file path), optional `force` | `wright-result/v1` compile envelope: verdict, diagnostics, `output.written_to` and `output.sha256`; the program text is written to the file and never returned. See [Compile output](#compile-output-adr-0025) |
| `check` | optional `all` | `wright-result/v1` check envelope |
| `analyze` | optional selection, optional `all` | `wright-result/v1` essential analysis envelope |
| `inspect` | optional `all` | `wright-result/v1` overview and rule names |
| `project` | none | Loaded program origin, files, counts, and findings summary |
| `rules` | optional selection, optional `content` | Canonical Workshop rules. Each entry carries `id`, `name`, `event` (the rule's event binding, `subroutine:<name>` for subroutine rules), and `span`; `{"rules": [...], "selection": {...}}` when a selection is applied. See [Rule content](#rule-content-adr-0024) |
| `symbols` | optional selection, optional `all` | Symbols; `{"symbols": [...], "selection": {...}}` when `file`/`max` is applied (a `kind`-only request keeps the bare array) |
| `references` | required `symbol` (id or name), optional selection and `all` | Usage header and references for the symbol; `{"references": [...], "selection": {...}}` when a selection is applied |
| `usage` | required `symbol` (id or name) | Usage counts for the symbol, plus its resolved `id` and `kind` |
| `cfg` | required `rule` (index or name), optional selection and `all` | Control-flow graph for the rule, plus `selection` when a selection is applied |
| `findings` | optional selection, optional `all` | Wright static-analysis findings; `{"findings": [...], "selection": {...}}` when a selection is applied |
| `persistentObjects` | optional `all` | Persistent Workshop object facts |
| `lint` | optional selection, optional `all` | Finding counts by severity and actionable findings; effective policy is available from `lintRules` |
| `lintRules` | none | Registered lint rules with full metadata and effective configuration |
| `callGraph` | optional selection, optional `all` | Subroutine call graph; `{"edges": [...], "selection": {...}}` when a selection is applied |
| `costEstimate` | optional selection, optional `rule`, optional `all` | Exact generated-resource counts, `elementCount` (the canonical Workshop structure counted against the client element budget; `rules` ranked by count, largest first. `index` is the canonical rule position and each entry carries a resolved `span`; the 5 largest by default, `all: true` for every rule; `null` plus `elementCountUnavailable` when the count cannot run), findings (each with a resolved `span`), and `selection` when applied. With `rule` the result is the rule drill-down (#724): `rule` (identity, `count`, `span`), `statements[]` (direct children ranked by recursive `count`, each with `name`, `count`, `span`) and `byNode[]` (each node's own `base_count + adjustment` cost summed per canonical node name with `occurrences`; the entries sum to the rule's `count`) | The default result carries no run-cache comparison — an agent requests one explicitly by passing the reference record as inline data, not a file path (ADR-0027, #723). |
| `targetMetadata` | none | Canonical target/catalog metadata |
| `validateEditTransaction` | `transaction`, optional `sources` | Atomic validation status, diagnostics, and previews when valid |
| `semanticRename` | `target`, optional `sources` | Validated rename transaction or structured refusal |
| `providerSemanticRename` | `language_id`, `position_document_uri`, `position`, `new_name`, optional `documents`, `sources`, `project_root` | Provider-resolved rename transaction or structured refusal |
| `providerValidateEdit` | `language_id`, `transaction`, optional `documents`, `sources`, `project_root` | Provider-validated transaction or structured refusal |
| `lookup` | required `language`; optional `query`, `kind`, `within`, `locale`, `limit` | Owner vocabulary entries with accepted spellings and rendered signatures, or an explicit `unavailable` payload naming the owner |

### Compile output (ADR-0025)

`compile` writes the program to a file and answers with the same content as
`wright compile --format json`: the verdict, diagnostics, `output.written_to`
and `output.sha256`, without the program text.

- `output` names the file; without it the program goes to `<entry stem>.ow`
  in the service process's working directory. Unlike the CLI, `-` is not
  special: the service never streams the program.
- An existing file is replaced only when the request sets `force: true`;
  otherwise the envelope carries `output-exists` with exit 2. The service
  never prompts, so every repeat compile to the same file needs `force: true`.
- The input source file is never overwritten, even with `force`
  (`output-is-input`, exit 2).
- A failed compile writes no file. The JSON-RPC `compile` alias takes no
  parameters and uses the defaults (no `force`); callers needing `output` or
  `force` use the `request` method.

### Freshness and id validity (#471, #512)

The service may exist without a program snapshot: construction over a
project that cannot load succeeds, and `capabilities`, `targetMetadata`
(the static catalog), `lookup` (the language vocabulary), and `provider*`
operations carrying caller-supplied documents answer without consulting the
project at all. They neither require nor trigger its load. A `provider*`
request that omits `documents` derives the set from the loaded project
(#548) and so loads like any other program-reading request.

A request that consults the program needs a valid current snapshot at
request time. When none exists, the request performs the deferred initial
load; when one exists, the service fingerprints the input's observable file
set: a file input's own content, a directory input's resolved members, or
every file under a source-language project's entry directory. It reloads
when the set changes, so an edit, an added file, or a removed file is reflected
in the next `symbols`, `references`, `check`, `lint`, or other
program-reading request without restarting the session. An unchanged input
is never reloaded.

A load or reload that fails, whether from a missing or unreadable entry or
a source that does not parse, refuses the request with the loader's structured
diagnostic code (`input-io`, `parse-error`, `input-kind-ambiguous`, a
provider diagnostic). The service keeps refusing until the project loads
again, retrying on every program-reading request; the previously loaded
program is never served silently, and the service never substitutes an
empty or placeholder program. The failed attempt invalidates the served
snapshot: even restoring the input to byte-identical content loads a fresh
program rather than resurrecting the dropped one. Repairing the input
recovers the same running session, with no restart and no explicit reload
request.

`check` and `compile` are workflows rather than snapshot queries (#591):
they run the session's own operation-aware load instead of requiring a
canonical program snapshot first. A `check` request over a source-language
provider invokes the provider's check operation and, when the provider can
compile, its compile operation too. `check` then runs the same canonical
validation `compile` surfaces, so an agent gating on `check` sees every
error `compile` reports, and a successful `check` supplies the canonical
snapshot the later `symbols`, `references`, or `findings` requests read
(#617). A provider whose compile returns `Unsupported` keeps the
check-only path: it answers `check` from its own diagnostics alone, the
one deliberate difference from `compile`. Any other compile failure,
including an LPP refusal, fails `check` the way it fails `compile`. The
freshness gate above still applies: an
observed disk change invalidates the served snapshot, and the next
workflow re-resolves the input rather than serving it. A failed load
surfaces inside the `wright-result/v1` envelope's diagnostics (the same
diagnostic a snapshot query refuses with), matching the embedding API's
`service.check()`/`service.compile()` outcomes.

Numeric symbol ids and rule indexes are valid only for the program that
issued them. After a content-changing reload, a request carrying a numeric
`symbol`, `rule`, or `semanticRename` target id is refused `stale-id` until
the client observes the new space: a successful `symbols` response
re-establishes symbol ids, a successful `rules` response re-establishes
rule indexes, and an `ambiguous-symbol`/`ambiguous-rule` refusal
re-establishes its own space because it already names the current
candidates. Every numeric address a request carries is checked, including
a `references` request's numeric `rule` filter (#592). A reload that
succeeds after earlier attempts failed is no exception: ids issued by the
last served program do not silently validate against the repaired one.
Name addressing resolves against the current program in both states and is
never stale.

`inputIdentity` labels the input, not its freshness: it is the SHA-256
digest of the input's primary entry source (the provider's
`sourceIdentity` per LPP ADR-0003). For a single-file input that covers
the whole input; for a provider directory project it hashes only the
entry file, so editing a member file leaves `inputIdentity` unchanged.
The `stale-id` refusals above, not this value, report freshness.

The responses that issue numeric ids or indexes into the loaded program are
`symbols` (symbol ids), `rules` (rule indexes), `usage` (the resolved `id`),
`references`, `findings`, `lint`, `persistentObjects`, `inspect`, `analyze`,
and the `ambiguous-*` refusal candidates, plus `skipped[].rule` in `lint`
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
in the driver, so a request never has to learn the program's numbering.
Symbol ids and rule indexes are different spaces (a rule's symbol id is not
its rule index). Numeric ids keep their established meaning, and both
addressings return the same payload for the same target.

An unmatched name returns a structured `unknown-symbol` or `unknown-rule`
error whose message names up to three nearest declared names (case-insensitive edit
distance of at most 1 for names shorter than six
characters and 2 otherwise, ties in declaration order), or the `symbols` /
`rules` operation when none is near (#567). The candidates travel in the
message only, as ADR-0021 decision 6 does for owner vocabulary. A name shared
by more than one symbol, or more than one rule for `cfg`, returns
`ambiguous-symbol`/`ambiguous-rule` listing the candidate numeric ids.
Resolution never guesses or returns an empty success; `semanticRename`
addressed by name reports the same refusals. `usage`
also echoes the resolved `id` and `kind` so a name-addressed caller
can correlate the result with `symbols`.

### Finding selection (#430)

`findings`, `lint`, and `costEstimate` accept optional selection fields:

* `severity`: a threshold. `error` reports errors only, `warning` errors and
  warnings, `info` everything.
* `rule`: one lint rule id (the finding `code`). An unknown id is a
  structured `invalid-selection` error, never a silent empty result.
* `file`: one source file. The reported `span.path` spelling differs per
  surface, so the argument resolves to the same canonical file. The path as
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

### Rule content (ADR-0024)

`rules` with `"content": true` adds a `content` member to each returned rule:
the owner's `workshop-rs/rule-content-v1` document. Every action, value, and
operator is a `{"call": id, "args": [...]}` node named by its catalog id, and
`args` lists every argument in catalog parameter order, so an argument is
identified by its position (`lookup` names the parameter at each position).
Literals, `{"enum", "member"}`, `{"variable"}`, `{"subroutine"}`, `{"string"}`,
and `{"localizedString"}` are the other node forms. `actions` is flat: index
`i` is the same action index `cfg`, `references`, and findings use. Wright adds
`span` to each condition and action when the program has one. For a source
language the content is the canonical program after lowering, so assertions are
independent of spelling and locale.

A `content` request without `max` returns at most 20 rules, always in the
selection-wrapped shape; `rule` selects exactly one. A selected rule's content
is never truncated. The CLI form is `wright inspect rules --content`
(`--rule`, `--only`, `--file`, `--max`). OverPy spellings for the canonical ids
(`names`) are not served yet: they wait on a provider capability.

### Semantic query selection (#531)

`rules`, `symbols`, `references`, `cfg`, and `callGraph` accept optional
selection fields with the same semantics as finding selection: filters narrow
first, `max` bounds after, and a `selection` member reports `{"total":
<set before selection>, "withheld": <dropped by max>}`. An unknown filter
value, such as a kind outside the listed domain or a `name`/`rule`/`caller`/`callee`
that matches nothing in the loaded program, is a structured
`invalid-selection` error, never a silent empty result.

* `rules`: `name` (a declared rule name), `rule` (exactly one rule, by index
  or declared name), `file`, `max`.
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
  `successors` and the `entry`/`exit` fields still address the full
  graph's block numbering.
* `callGraph`: `caller` (a declared rule name), `callee` (a declared
  subroutine name), `max`.

The `file` field resolves like finding selection: any spelling that resolves
to the same source file selects it. A capped list reports a `truncation` member with its total, shown and withheld counts, and an `expand` hint. Unselected arrays become `{items, truncation}` only when capped. Explicit selection wrappers retain their own totals. The CLI options
`--only`, `--rule`, `--file`, `--caller`, `--callee`, and `--max` on `inspect`
subcommands drive the same `wright-driver` selection.

### Essential default results

CLI text, CLI JSON, stdio, JSON-RPC, and MCP expose the same actionable
content. JSON is compact and deterministic. `check` reports its verdict and
diagnostics; `lint` reports severity counts and actionable findings;
`analyze` reports overview, cost, hotspots, complexity, risks, and top
variables; bare `inspect` reports the overview, rule names, and detail commands.
Lint and risk findings carry code, severity, message, evidence, and a compact
`location` (`path:line:column`) when mapped. Full spans and fix transactions
are available from `findings`; lint policy is available from `lintRules`.
Persistent-object details are queried through `persistentObjects`.

Full fact dumps are available only through the in-process `CompilerSession`
API; external callers use the narrower semantic operations.

General lists default to at most 20 items; lint also bounds its compact result to 5500 bytes before the envelope so long finding messages cannot consume the default token budget; the ranked analyze report keeps its top-five hotspots, risks, and variables and top-three condition trees. Capped lists report their total, truncation, and how to
see the rest. `all: true` lifts the default cap (CLI `--all`); filters and an
explicit `max` continue to apply. Verdicts and exit codes are computed on the
complete set before truncation. Static catalogs are not capped. `brief` and
`--brief` are removed. [ADR-0025](adr/0025-essential-default-output.md) records
this breaking pre-1.0 shape change without a contract-id bump.

Edit transactions use source identities and half-open, 1-based line/column
ranges. Provider positions use 0-based line/character coordinates. Wright
proposes and validates edits; a caller remains responsible for applying them.
Stale, overlapping, unsupported, or semantically invalid edits return an
explicit refusal without a partial edit set.

### Validated edits and semantic rename for raw Workshop (#434)

On raw Workshop input both operations are supported directly. `workshop-rs`
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
* `semanticRename` resolves `target` either by `symbol`, a numeric id or a
  declared name that is exactly the addressing `references`/`usage` use, or by a
  `source`/`line`/`col` position inside one identifier occurrence. Global
  variables, player variables, and subroutines rename through the exact
  identifier spans `workshop-rs` records: the declaration, the `Subroutine`
  event binding, `Call Subroutine` callees, `Start Rule` subroutine
  arguments, `Set`/`Modify`/`For` variable
  arguments, and `Global.name`/`Event Player.name` value references all
  rewrite, while prefixes, comments, strings, and unrelated identifiers stay
  untouched. There is no textual-search fallback. An occurrence whose
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
* Findings on `lint` and `findings` carry a `fix` member when a rule offers
  a mechanically determined correction (#556): `fix.kind` names the
  remediation, `fix.summary` describes it, and `fix.transaction` is the
  same `EditTransaction` shape `semanticRename` produces. Preview it
  through `validateEditTransaction` and apply it through the caller's own
  write path; there is no separate apply operation. Fixes exist only on raw
  Workshop input for `exact`-evidence findings whose correction is
  unambiguous. The current fixable rules are `duplicate-condition`
  (`remove-dead-branch`: delete the unreachable `Else If` branch) and
  `repeated-value` (`evaluate-once`: mark each duplicated occurrence with
  `Evaluate Once`). Shapes the correction cannot safely cover, such as a
  condition that can move between evaluations or an occurrence the engine
  re-evaluates (the loop's `While` condition, `Wait Until`/`Loop If`
  conditions, `Update Every Frame` subtrees, parameters an enabled
  reevaluation mode keeps live), withhold the `fix` entirely rather than
  offer a risky edit. `evaluate-once` is an
  acknowledgment idiom, not a cost reduction: at once-per-execution
  positions it freezes nothing, it marks the repeated evaluation as
  intentional so the finding retires.

`wright rename <NAME> <NEW_NAME> [INPUT]` exposes the same semantic rename
on the CLI: it prints the validated diff by default and applies it
atomically with `--write`, refusing `edit-stale-source` when the file
changed since validation.

### Provider-backed edits

`providerValidateEdit` and `providerSemanticRename` run the same
validated-edit pipeline through the configured source-language provider
(`opy` today): Wright checks the caller's preconditions and translates
coordinates, the provider validates each edited document, and the edited
document set is rechecked before `ok: true`. The document set is
caller-supplied or session-derived:

* `language_id` names the configured provider: `opy` for the shipped
  provider.
* `documents` is optional (#548). When supplied, it is a map from URI to
  `{uri, languageId, version, text}` describing the project as the provider
  sees it. When omitted, the session's loaded project supplies the set:
  every member file read from disk at `version` 0, keyed by `file://` URI,
  the same project view the session serves, so an agent never reconstructs
  the document set the session already holds. The omitted-set request reads
  the program like any other program-reading operation (#471): it triggers
  the deferred load and refuses with the loader's diagnostic on an
  unloadable project. A `providerValidateEdit` `edits[].source` outside
  the set refuses `edit-unknown-source`; `providerSemanticRename` requires
  `position_document_uri` inside the set and refuses provider-returned
  edits outside it with `provider-edit-outside-set`.
* `sources` is optional (#548) and defaults to each document's text. It is
  a map from URI to current text covering every source the transaction may
  edit: each `edits[].source` on `providerValidateEdit` and every document
  the provider's rename may touch on `providerSemanticRename`. It is the
  precondition each `source_identity` verifies against (SHA-256 hex of the
  text; for the entry document this equals `project.inputIdentity`) and
  the text the edits apply to.
* `providerValidateEdit`'s post-edit check blocks on errors in any
  supplied document, so the set should cover the project documents whose
  diagnostics may break.
* `providerSemanticRename` resolves the analyzable project from the
  supplied documents themselves. Include the project entry document
  (e.g. `main.opy`), not only the position file. A member file alone can
  refuse `rename.noSymbolAtPosition` even when the position is valid;
  supply the entry plus every file the rename may touch. The session-
  derived set already contains every loaded member, entry included.
* `providerSemanticRename`'s `position` uses the provider convention,
  0-based line and UTF-16 character, while `transaction` ranges are the
  `EditRange` 1-based half-open columns every edit operation shares. The
  returned transaction's `source` fields are document URIs and each
  `source_identity` is the identity of that document's current text, so
  the caller can replay or apply the edits itself.

Operations carrying caller-supplied documents answer without consulting
the session's loaded project (#471); both refuse cleanly when no provider
is configured.

### Name and signature lookup (#529, ADR-0021)

`lookup` resolves a free-text query, such as a display name, a near spelling,
or a guess, against the language owner's vocabulary, so an agent can learn the
accepted spelling and signature of a Workshop or OverPy name without
external documentation, source inspection, or `wright convert`. The
request's `language` selects the owner (`workshop` or `opy`, the ids
`capabilities.languages` reports); the remaining fields narrow the answer:

* `query`: the free text to resolve.
* `kind`: any entry kind an owner emits (#563): `action`, `value`,
  `event`, `enumMember`, `enum`, `setting`, `parameter`, `settingPath`,
  `memberAction`, `memberValue`, `structural`, or `operator`. A kind one
  language never emits answers `[]` on it.
* `within`: the identity of an enum domain, a callable, or a settings path
  prefix. The result then lists its members, parameters, or child segments,
  which is how a caller enumerates an enum's accepted member spellings or
  a callable's parameters.
* `locale`: the Workshop display-name locale (session configuration, then
  the catalog's primary locale when omitted).
* `limit`: default 3, maximum 10.

Each entry carries its owner `identity`, `kind`, accepted `spelling`, and
`displayName`, plus the owner fact that describes it (`callable`, `enum`,
`parameter`, or `setting`). Callables also carry `signature`, a
human-readable call shape Wright renders from the owner's parameter facts:
required parameters name their type, optional parameters show `name?` or
`name=default`, and an enum domain lists its members inline (a domain
above the member bound renders as `Domain(count members)`).

Workshop answers come from the `workshop-rs` catalog in process. OverPy
answers come from the configured `opy-rs` provider's LPP lookup
capability; when the provider does not negotiate it, the result is an
explicit `unavailable` payload naming `opy-rs`, never a Wright-side
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

[ADR-0025](adr/0025-essential-default-output.md) records the essential-default
shape change across both contracts, including the removal of `brief` and full
external fact dumps. The complete typed Rust API remains available.

One recorded exception applies to the same rule in both contracts, decided
before the 1.0 contract freeze (#134): the `lint` result's `rules` member
lists only each rule's `id` and `effectiveSeverity` (#431). Full rule
metadata, meaning summary, rationale, documentation, known limits, evidence,
and tags, is served once by `lintRules`.

ADR-0020 records a second pre-freeze exception (#472): `sources` is optional
on `validateEditTransaction` and `semanticRename` rather than required, so
agent callers stop resending whole files. The same exception extends to the
provider operations (#548): `documents` and `sources` are optional on
`providerSemanticRename` and `providerValidateEdit`, defaulting to the
session's loaded project and each document's text.

The optional guide installed by `wright agent install` (and distributed by
`wrightkit/skills`) teaches clients to discover and use these capabilities. It
is not required to expose, execute, or validate any semantic operation.
