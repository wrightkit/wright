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
reserved for requests, so `-` is not a valid source input. The server loads one
project and processes requests until standard input closes. The default
transport is one JSON request per line on stdio:

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
workflows as the session service. The CLI does not add agent-specific semantic
results. The in-process embedding API can call `ToolService::handle` directly.

The released `wright` binary includes `serve`, so each supported installation
channel can use the session contract without a separate runtime. MCP is not a
currently shipped transport. If added later, it must map this contract without
adding operations or changing their meaning.

## Operations

All requests are JSON objects with a required `op` string. Request fields and
the common response/error shapes are defined by the committed JSON Schema. The
table lists every operation advertised by `capabilities.operations` and names
the successful `result` payload.

| Operation | Request fields | Successful `result` |
| --- | --- | --- |
| `capabilities` | none | Service name/version, `wright-agent/v1`, result contract, operation names, languages, and profiles |
| `compile` | none | `wright-result/v1` compile envelope |
| `check` | none | `wright-result/v1` check envelope |
| `analyze` | none | `wright-result/v1` analysis envelope |
| `inspect` | none | `wright-result/v1` inspection envelope |
| `project` | none | Loaded program origin, files, counts, and findings summary |
| `rules` | none | Canonical Workshop rules |
| `symbols` | optional `kind` | Symbols, optionally filtered by kind |
| `references` | required `symbol` | References for the symbol id |
| `usage` | required `symbol` | Usage counts for the symbol id |
| `cfg` | required `rule` | Control-flow graph for the rule id |
| `findings` | optional selection | Wright static-analysis findings; `{"findings": [...], "selection": {...}}` when a selection is applied |
| `persistentObjects` | none | Persistent Workshop object facts |
| `lint` | optional selection | Lint findings, per-rule id/effective severity, effective configuration, and `selection` when applied |
| `lintRules` | none | Registered lint rules with full metadata and effective configuration |
| `callGraph` | none | Subroutine call graph |
| `costEstimate` | optional selection | Exact generated-resource counts, findings, and `selection` when applied |
| `targetMetadata` | none | Canonical target/catalog metadata |
| `validateEditTransaction` | `sources`, `transaction` | Atomic validation status, diagnostics, and previews when valid |
| `semanticRename` | `sources`, `target` | Validated rename transaction or structured refusal |
| `providerSemanticRename` | `language_id`, `documents`, `position_document_uri`, `position`, `new_name`, optional `project_root`, `sources` | Provider-resolved rename transaction or structured refusal |
| `providerValidateEdit` | `language_id`, `documents`, `transaction`, `sources`, optional `project_root` | Provider-validated transaction or structured refusal |

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

Edit transactions use source identities and half-open, 1-based line/column
ranges. Provider positions use 0-based line/character coordinates. Wright
proposes and validates edits; a caller remains responsible for applying them.
Stale, overlapping, unsupported, or semantically invalid edits return an
explicit refusal without a partial edit set.

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

The optional guide distributed by `wrightkit/skills` teaches clients to
discover and use these capabilities. It is not required to expose, execute, or
validate any semantic operation.
