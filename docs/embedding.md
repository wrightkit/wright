# Wright Embedding and Tool API (M9)

Status: public contract baseline for milestone M9 (issue #56)
Scope: `wright-driver`'s embedding surface, the session-aware tool service,
safe source-edit contracts, and the transport adapters

## Public / experimental / internal boundaries

| Surface | Status | Notes |
| --- | --- | --- |
| `wright_driver::{CompilerSession, SessionConfig, InputSpec, SourceKind, OutputFormat, Profile}` | **stable** | One driver for compile/check/analyze/inspect; `load()` is idempotent |
| `wright_driver::{Envelope, CompileResult, CheckResult, AnalyzeResult, InspectResult, Diagnostic, CompiledOutput}` | **stable** | `wright-result/v1` machine contract (`docs/cli.md`) |
| `wright_driver::service::{ToolService, ToolRequest, ToolResponse, Capabilities}` | **stable** | Session-aware tool queries (project/rules/symbols/references/usage/CFG/findings/callGraph/costEstimate/targetMetadata/capabilities) |
| `wright_driver::edit::{SourceEdit, EditValidation, RenameRequest, rename_symbol, validate_edit}` | **stable** | Safe source-oriented edits; validated through the compiler pipeline |
| `wright_driver::{input_identity, EMBEDDING_CONTRACT}` | **stable** | `wright-embedding/v1` |
| Internal HIR/WIR arenas, parser/CST, emitter internals | **internal** | Never part of the public contract |
| `wright-serve` stdio/JSON-RPC adapters | **stable** | Thin mappings over `ToolService`; MCP not implemented (no agent evidence) |
| `wright-transform` passes | experimental per pass | Only evidence-backed passes ship in `compat`; `aggressive` is an explicit experimental marker |

## Embedding contract

External consumers depend on `wright-driver` only (never internal crates, no
CLI subprocess, no text scraping):

```rust
use wright_driver::{CompilerSession, InputSpec, SessionConfig, SourceKind, Profile};

let mut session = CompilerSession::new(SessionConfig {
    input: InputSpec::Path("program.opy".into()),
    kind: SourceKind::Opy,
    profile: Profile::Compat,
    ..SessionConfig::default()
})?;
let check = session.check();      // typed Envelope<CheckResult>
let compile = session.compile();  // typed Envelope<CompileResult>
```

## Session-aware tool service

`ToolService::new(&mut session)` loads the program eagerly and answers typed
[`ToolRequest`]s with owned [`ToolResponse`]s. `Capabilities` negotiates the
service version, contract (`wright-result/v1`), operations, languages, and
profiles. Cost inspection (`costEstimate`) distinguishes exact
target-resource counts (emitted bytes, WIR nodes, waits) from static
findings and from compiler-host performance (measured by `wright-bench`, not
in-process). Target/catalog metadata enables reasoning about Workshop
actions, values, events, enum domains, and locales.

## Safe edits

Proposed edits are source-oriented ([`SourceEdit`]) with a source identity;
[`validate_edit`] rejects stale versions, invalid ranges, and compiled
errors, and returns the previewed edited source. The first evidence-backed
refactoring is symbol rename ([`rename_symbol`]) with whole-word replacement
and pipeline validation. Raw HIR/WIR mutation is never public.

## Transports

`wright-serve` exposes the same operations over stdio JSON-lines and
JSON-RPC 2.0; both are thin mappings with identical semantics to in-process
consumers (equivalence tested). MCP is intentionally absent in v1 — no
agent-integration evidence justified it (the issue's non-goals are honored).

## Versioning

* `wright-result/v1` and `wright-embedding/v1` are additive within major
  version 1: new optional fields and new operations are allowed; removed or
  renamed fields/ops require a major version.
* Envelope `wright.version` + `wright.contract` identify the producer;
  `ToolService::capabilities()` identifies the service.
* The release tarball's `version.json` is the authoritative artifact stamp.

## External consumer evidence

`crates/wright-consumer` is a committed consumer that depends only on
`wright-driver` and runs compile/check/analyze, all tool queries, and a
validated rename over the corpus (`wright-consumer/tests/consumer.rs`).
