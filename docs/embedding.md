# Wright Embedding and Tool API

Status: accepted baseline — living embedding and tool contract
Scope: `wright-driver`'s embedding surface, the session-aware tool service,
safe source-edit contracts, and the transport adapters

## Public / experimental / internal boundaries

| Surface | Status | Notes |
| --- | --- | --- |
| `wright_driver::{CompilerSession, SessionConfig, InputSpec, SourceKind, OutputFormat, Profile}` | **stable** | One driver for compile/check/analyze/inspect/lint; `load()` is idempotent |
| `wright_driver::{ProgressEvent, ProgressObserver, ProgressPhase, ProgressUnit}` | **stable** | Transport-neutral workflow phase events; no terminal presentation or machine-result mutation |
| `wright_driver::{Envelope, CompileResult, CheckResult, AnalyzeResult, InspectResult, LintResult, Diagnostic, CompiledOutput}` | **stable** | `wright-result/v1` machine contract ([`docs/cli.md`](cli.md)) |
| `wright_driver::service::{ToolService, ToolRequest, ToolResponse, Capabilities}` | **stable** | Session-aware tool queries (project/rules/symbols/references/usage/CFG/findings/lint/lintRules/callGraph/costEstimate/targetMetadata/capabilities) plus validated mutation (`validateEditTransaction`, `semanticRename`, #130) |
| `wright_driver::edit::{SourceEdit, EditRange, EditTransaction, SourcePreview, EditValidation, RenameRequest, rename_symbol, validate_transaction}` | **stable** | Source-edit transactions; validated through the correct owner-backed project semantics (#128); `EditTransaction::apply` applies ranges against one original source snapshot |
| `wright_driver::{input_identity, EMBEDDING_CONTRACT}` | **stable** | `wright-embedding/v1` |
| Internal HIR/WIR arenas, parser/CST, emitter internals | **internal** | Never part of the public contract |
| `wright-serve` stdio/JSON-RPC adapters | **stable** | Thin mappings over `ToolService`; MCP not implemented (no agent evidence) |
| `wright-transform` passes | experimental per pass | Only evidence-backed passes ship in `compat`; `aggressive` is an explicit experimental marker |

The Rust packages in this workspace are implementation packages for the Wright
product, not a crates.io distribution surface. They are explicitly marked
`publish = false`; the current public release flow is the CLI/LSP binary and
package-manager distribution. A separately reviewed Rust embedding package
would require an intentional public API and publication decision.

## Source-language owner boundary

`wright-opy` and `wright-ostw` are narrow Wright adapters. Their target
boundary is the released `opy-rs` and `deltin-rs` owner APIs for
source-language parsing, semantic behavior, compiler/lowering behavior,
diagnostics, and reconstruction. Wright adapters may translate those contracts
into driver results and compose them with canonical `workshop-rs` WIR/catalog
APIs for Wright-owned analysis, conversion, and emission. They must not depend
on source-language CLI packages, private compiler packages, or recreate owner
behavior locally.

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

## Session-aware tool service

`ToolService::new(&mut session)` loads the program eagerly and answers typed
[`ToolRequest`]s with owned [`ToolResponse`]s. `Capabilities` negotiates the
service version, contract (`wright-result/v1`), operations, languages, and
profiles. Cost inspection (`costEstimate`) distinguishes exact
target-resource counts (emitted bytes, WIR nodes, waits) from static
findings and from compiler-host performance (measured by `wright-bench`, not
in-process). Target/catalog metadata enables reasoning about Workshop
actions, values, events, enum domains, and locales.

## Validated mutation (#130)

Agents and embedding consumers request mutation through two structured
tool operations over the session's project:

* `validateEditTransaction` — validate and preview a caller-supplied
  [`EditTransaction`] against the session project. The request carries the
  current text of every touched source (keyed by source identity); the
  response returns `ok`, structured diagnostics, and per-source previews
  with the edited text and its new SHA-256 identity.
* `semanticRename` — request a semantic rename at a 1-based
  position (`source`/`line`/`col`/`to`) through the shared #129 refactoring
  contract. The response returns the validated exact-range transaction
  (`ok: true`) or structured refusal diagnostics (`ok: false`, no
  transaction).

Both preserve the #128 all-or-nothing semantics: an unsafe, stale,
overlapping, colliding, or unsupported request returns structured
diagnostics and never a partially applicable edit set. Wright
**proposes and validates** edits; applying them to the filesystem is an
explicit consumer responsibility — the semantic/tooling core never writes
files. Capability discovery advertises `validateEditTransaction` and
`semanticRename`; the stdio/JSON-RPC adapters forward the same operations
unchanged (behaviorally equivalent, transport-tested).

## Safe edits

Proposed edits are source-oriented ([`SourceEdit`]) and travel as
[`EditTransaction`]s: one or more file edits with exact source ranges plus
per-source SHA-256 identity/version preconditions. Ranges address one
original source snapshot — per source, edits apply in descending position
order, so an earlier replacement's length/newline changes can never shift a
later range (`EditTransaction::apply` is the mechanical application; columns
are strict 1-based character columns, `0` or beyond-line columns refuse, and
order-dependent zero-width combinations at one position are refused as
`edit-zero-width-conflict`).
[`validate_transaction`] rejects stale versions, unknown sources,
overlapping/conflicting edits, invalid ranges, and compilation errors, and
returns the previewed edited sources atomically (any failed validation returns
`ok = false` and no validated preview). Validation runs through the
owner-backed project/session semantics (`SessionConfig` kind/root,
transformation profile): OPY projects compile through `opy-rs` with edited
includes as in-memory overlays. DEL/OSTW overlays refuse explicitly because
`del-rs` has not exposed an equivalent overlay project contract. Workshop and
Protocol inputs also refuse explicitly. The first evidence-backed refactoring
is symbol rename
([`rename_symbol`]) with whole-word replacement and transaction validation.
Raw HIR/WIR mutation is never public, and application/writing stays an
explicit caller responsibility.

## Transports

`wright-serve` exposes the same operations over stdio JSON-lines and
JSON-RPC 2.0; both are thin mappings with identical semantics to in-process
consumers (equivalence tested).

## Versioning

* `wright-result/v1` and `wright-embedding/v1` are additive within major
  version 1: new optional fields and new operations are allowed; removed or
  renamed fields/ops require a major version.
* Envelope `wright.version` + `wright.contract` identify the producer;
  `ToolService::capabilities()` identifies the service.
* The release tarball's `version.json` is the authoritative artifact stamp.

## External consumer evidence

`crates/wright-consumer` is a committed consumer that depends only on
`wright-driver` and runs compile/check/analyze/lint, all tool queries, and a
validated rename over the corpus (`wright-consumer/tests/consumer.rs`).
