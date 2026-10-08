# Wright Embedding and Tool API

Status: accepted baseline (living embedding and tool contract)
Scope: `wright-driver`'s embedding surface, the session-aware tool service,
safe source-edit contracts, and the transport adapters

## Public / experimental / internal boundaries

| Surface | Status | Notes |
| --- | --- | --- |
| `wright_driver::{CompilerSession, SessionConfig, InputSpec, SourceKind, OutputFormat, Profile}` | **stable** | One driver for compile/check/analyze/inspect/lint; `load()` is idempotent |
| `wright_driver::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressUnit}` | **stable** | Transport-neutral workflow phase events; no terminal presentation or machine-result mutation |
| `wright_driver::{Envelope, CompileResult, CheckResult, AnalyzeResult, InspectResult, LintResult, Diagnostic, CompiledOutput}` | **stable** | `wright-result/v1` machine contract ([`docs/cli.md`](cli.md)) |
| `wright_driver::service::{ToolService, ToolRequest, ToolResponse, Capabilities}` | **stable** | Session-aware tool contract `wright-agent/v1` ([`docs/agent-contract.md`](agent-contract.md)); capability discovery, queries, workflows, and validated mutation |
| `wright_driver::edit::{SourceEdit, EditRange, EditTransaction, SourcePreview, EditValidation, validate_transaction}` | **stable** | Source-edit transactions; validated through the correct owner-backed project semantics (#128); `EditTransaction::apply` applies ranges against one original source snapshot |
| `wright_driver::edit::{RenameRequest, rename_symbol}` | **deprecated** | `wright-embedding/v1` compatibility helpers retained for source compatibility (#514): a whole-file textual whole-word proposal with no semantic-identity guarantee — strings, comments, and unrelated same-named symbols are not distinguished. The supported rename is `semanticRename` (see below); the helpers may be removed only with a breaking embedding-contract transition |
| `wright_driver::{input_identity, EMBEDDING_CONTRACT}` | **stable** | `wright-embedding/v1` |
| Internal HIR/WIR arenas, parser/CST, emitter internals | **internal** | Never part of the public contract |
| `wright serve` stdio/JSON-RPC/MCP adapters | **stable** | Thin mappings over `ToolService` and `wright-agent/v1`; `wright-serve` remains a workspace binary alias |
| `wright-transform` passes | experimental per pass | Only evidence-backed passes ship in `compat`; `aggressive` is an explicit experimental marker |

`wright_transform::run` (also exported as `run_canonical`) validates input before
transforming it. `run_validated` requires the caller to have successfully called
`Program::validate()` since the last mutation and skips that duplicate traversal.
Both entry points validate after a non-off transform profile. The driver uses
`run_validated` after validating raw Workshop or provider output at load. With
`off`, the driver validates only at load and performs no transforms.

The Rust packages in this workspace are implementation packages for the Wright
product, not a crates.io distribution surface. They are explicitly marked
`publish = false`; the current public release flow is the CLI/LSP binary and
package-manager distribution. A separately reviewed Rust embedding package
would require an intentional public API and publication decision.

## Source-language owner boundary

OPY is integrated through the LPP provider boundary. Wright owns the session,
diagnostic, artifact, and tooling contracts; `opy-rs` owns source semantics,
project loading, compilation, and reconstruction behind the provider. `.ostw`/
`.del` remain recognized source kinds, but their provider boundary is explicit:
provider support is not currently shipped and Wright returns
`source-provider-unavailable`. Wright has no static OPY or DEL implementation
fallback.

The migration is release-coordinated: if an owner contract is not yet
available in a consumable release, the adapter remains on its current released
contract until the owner release and the canonical Workshop dependency are
compatible. It must not introduce a local semantic workaround or a text-based
compatibility layer just to bypass that coordination boundary.

## Embedding contract

Consumers embedding Wright from a checkout depend on `wright-driver` only
(never internal crates, no CLI subprocess, no text scraping):

```rust
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind, Profile};

let mut session = CompilerSession::new(SessionConfig {
    input: InputSpec::Path("program.opy".into()),
    kind: SourceKind::Opy,
    profile: Profile::Compat,
    ..SessionConfig::default()
})?;
let check = session.check();      // typed Envelope<CheckResult>
let lint = session.lint();        // typed Envelope<LintResult>
let compile = session.compile();  // typed Envelope<CompileResult>
```

Consumers that need truthful workflow progress may attach a
`ProgressObserver` before invoking a workflow and clear it before rendering or
otherwise presenting the result. Events describe real orchestration phases and
may carry bounded counts such as lint-rule count; they never contain terminal
strings, ANSI, percentages, or fabricated completion estimates.

`SessionConfig` is construction-time configuration (#511): every field is
fixed once `CompilerSession::new` or `CompilerSession::with_source_provider`
succeeds — input, source kind/backend, locale, root, output/format, transform
profile, lint configuration and rule paths, finding selection, provider
registry, and first-party provider configuration. Changing configuration
means constructing a new session. The public `CompilerSession::config` field
remains readable and writable for source compatibility within
`wright-embedding/v1`, but post-construction mutation is not a supported
capability: the next workflow (`load`, `compile`, `check`, `analyze`,
`inspect`, `lint`, `convert`, `symbols`, `refs`, `cfg`, `callgraph`, `cost`,
`rename`, `validate_edit_transaction`, `semantic_rename`) or `ToolService`
construction refuses with a `session-config-changed` diagnostic naming the
changed fields, rather than reusing state derived from the earlier
configuration. The provider surfaces that consume the provider
registry/first-party provider configuration — `language_provider`,
`run_provider_flow`, and the `provider*` tool operations routed through them
— refuse through the provider refusal channel carrying the same
`session-config-changed` refusal code. The supported runtime changes are
separate explicit capabilities: attaching or clearing a progress observer,
file contents changing on disk under the configured input (the #471
disk-refresh lifecycle), and request-local `ToolRequest`
parameters/documents.

## Session-aware tool service

`ToolService::new(&mut session)` loads the configured project when it can and
answers typed [`ToolRequest`]s with owned [`ToolResponse`]s. A project that
cannot load at construction does not fail the service (#512): the service
exists without a program snapshot — `loaded()` returns `None` rather than a
placeholder program — and each program-reading request retries the load,
refusing with the loader's structured diagnostic until the project heals on
disk. `capabilities`, `targetMetadata`, and the `provider*` operations
carrying caller-supplied documents answer without consulting the project at
all (a `provider*` request that omits `documents` derives it from the loaded
project, #548). Repairing
the input recovers the same running service on the next program-reading
request; no restart or explicit reload is needed.
`Capabilities` negotiates the
service version, `wright-agent/v1` request/response contract,
`wright-result/v1` workflow envelope, operations, languages, and profiles.
The operation schemas, error model, and transport mapping are specified in the
[Wright Agent Contract](agent-contract.md). Cost inspection (`costEstimate`)
distinguishes exact
target-resource counts (emitted bytes, WIR nodes, waits) from static
findings and from compiler-host performance (measured by `wright-bench`, not
in-process). Target/catalog metadata enables reasoning about Workshop
actions, values, events, enum domains, and locales.

## Validated mutation (#130)

Agents and embedding consumers request mutation through two structured
tool operations over the session's project:

* `validateEditTransaction`: validate and preview a caller-supplied
  [`EditTransaction`] against the session project. The request may carry the
  current text of every touched source (keyed by source identity); when
  `sources` is omitted the service reads those files from disk (#472). The
  response returns `ok`, structured diagnostics, and per-source previews
  with the edited text and its new SHA-256 identity.
* `semanticRename`: request a semantic rename by `symbol` or at a 1-based
  position (`source`/`line`/`col`/`to`) through the shared #129 refactoring
  contract, with `sources` optional as for `validateEditTransaction` (#472).
  The response returns the validated exact-range transaction
  (`ok: true`) or structured refusal diagnostics (`ok: false`, no
  transaction).

Both preserve the #128 all-or-nothing semantics: an unsafe, stale,
overlapping, colliding, or unsupported request returns structured
diagnostics and never a partially applicable edit set. Wright
**proposes and validates** edits; applying them to the filesystem is an
explicit consumer responsibility because the semantic and tooling core never writes
files. Capability discovery advertises `validateEditTransaction` and
`semanticRename`; the `serve` transport adapters forward the same operations
unchanged (behaviorally equivalent, transport-tested).

## Safe edits

Proposed edits are source-oriented ([`SourceEdit`]) and travel as
[`EditTransaction`]s: one or more file edits with exact source ranges plus
per-source SHA-256 identity/version preconditions. Ranges address one
original source snapshot. Edits apply in descending position
order per source, so an earlier replacement's length/newline changes can never shift a
later range (`EditTransaction::apply` is the mechanical application; columns
are strict 1-based character columns, `0` or beyond-line columns refuse, and
order-dependent zero-width combinations at one position are refused as
`edit-zero-width-conflict`).
[`validate_transaction`] rejects stale versions, unknown sources,
overlapping/conflicting edits, invalid ranges, and compilation errors, and
returns the previewed edited sources atomically (any failed validation returns
`ok = false` and no validated preview). Validation runs through the
provider-backed project/session semantics where the provider negotiates edit
validation. OPY edit validation currently refuses explicitly because the
first-party provider has no edit capability; it never invokes a removed static
frontend. DEL/OSTW inputs refuse explicitly with
`source-provider-unavailable`; Workshop and Protocol inputs also refuse
explicitly. The supported refactoring is semantic
rename (`semanticRename` above, `ToolRequest::SemanticRename`, or
[`CompilerSession::semantic_rename`]): on raw Workshop it rewrites exactly
the identifier spans the parsed program records and revalidates the edited
project. The deprecated `rename_symbol` helper remains only for
`wright-embedding/v1` source compatibility — a whole-word textual proposal
with no semantic-identity guarantee; it is never a fallback when semantic
rename is unavailable or refused. Raw HIR/WIR mutation is never public, and
application/writing stays an explicit caller responsibility.

## Transports

`wright serve` exposes the same operations over stdio JSON-lines, JSON-RPC
2.0, and MCP (`--transport mcp`); all three map to the same `ToolService`
results as in-process consumers (equivalence tested). Starting the server
does not require the configured project to load (#512): MCP `initialize` and
`tools/list` — and the stable tool set itself — are available over a broken
project, and a program-reading `tools/call` surfaces the loader's refusal
until the project heals. The separate
`wright-serve` workspace binary remains an alias for the same adapter.
JSON-RPC protocol failures use the standard top-level `error` member, while a
service refusal remains an application result under the top-level `result`
member; the MCP adapter instead carries a refusal as an `isError` tool result
with the same `{code, message}` ([`docs/agent-contract.md`](agent-contract.md)).
Non-empty JSON-RPC arrays are handled as batches, with notification responses
omitted.

## Versioning

* `wright-agent/v1`, `wright-result/v1`, and `wright-embedding/v1` are
  additive within major version 1: new optional fields and operations are
  allowed; removed or renamed fields/ops require a major version.
  Deprecated `wright-embedding/v1` compatibility helpers such as
  `RenameRequest`/`rename_symbol` follow the same rule — they may be
  deleted only with an embedding-contract breaking transition.
* Envelope `wright.version` + `wright.contract` identify the result producer;
  `ToolService::capabilities()` identifies the service and agent contract.
* The release tarball's `version.json` is the authoritative artifact stamp.

## External consumer evidence

`crates/wright-consumer` is a committed consumer that depends only on
`wright-driver` and runs compile/check/analyze/lint, all tool queries, and a
validated rename over the corpus (`wright-consumer/tests/consumer.rs`).
