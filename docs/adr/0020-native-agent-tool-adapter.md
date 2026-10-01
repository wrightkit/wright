# ADR-0020: Native coding-agent tools as an MCP adapter over `ToolService`

- Status: Proposed
- Date: 2026-10-01
- Related: [Issue #470](https://github.com/wrightkit/wright/issues/470), [Agent contract](../agent-contract.md), [ADR-0017](0017-domain-intelligence-query-contract.md), [Agent benchmark](../agent-benchmark.md)

## Context

`ToolService` is the executable semantic layer behind `wright-agent/v1`, and
`wright serve` already maps it onto two thin transports (stdio lines and
JSON-RPC). A coding agent can reach it only through shell plus JSON or by
writing a session client. Harnesses with native tool calling cannot discover or
invoke Wright operations directly, which keeps semantic queries from replacing
grep/read work in the agent benchmark.

Reading the current code against this goal shows three properties of the
contract that make a thin native-tool mapping unsuitable as it stands:

1. **Edit operations require the caller to resend whole files.**
   `validateEditTransaction` and `semanticRename` take `sources`, the full
   current text of every touched file. That suits a client that already holds
   the text; as a tool parameter it makes the model paste files into
   arguments.
2. **A session never observes disk changes.** `ToolService::new` loads the
   project once and `handle` has no refresh path. A native-tool session spans
   edits made by the agent itself, so queries after the first edit would return
   stale semantics, and the client cannot restart the process as a shell user
   can.
3. **The contract has no known consumer outside this repository.** In-tree
   consumers are `wright-consumer`, `wright-bench`, `scripts/smoke-native.py`,
   and the agent benchmark scenarios. The `wrightkit/skills` guide draft uses
   only `rules` and `symbols`. Released binaries exist, but the contract is
   frozen at the 1.0 boundary tracked by #134, and the contract document
   already records a pre-freeze shape change (#431).

## Decision

### Transport and shape

1. **First native-tool target is MCP over stdio**, exposed as
   `wright serve --transport mcp [INPUT]`. It is a third transport beside
   `stdio` and `jsonrpc`: no new crate, no new binary, no separate runtime.
   Harness-specific adapters are not built; each would be a separate mapping
   to maintain and could not be compared in one benchmark.
2. **The adapter is a mapping only.** It performs MCP initialization, answers
   `tools/list`, and turns `tools/call` into a `ToolRequest` handled by
   `ToolService::handle`. It implements no symbol, reference, lint, cost,
   rename, or provider behavior and has no textual or grep fallback.
3. **One tool per operation, not one generic request tool.** A generic
   `request{op, ...}` pushes operation selection and a union schema onto the
   model. Per-operation tools give each a precise description and input schema.
   Tool names are `wright_` plus the operation name in snake case. Input schemas
   are derived from the `ToolRequest` fields, not written a second time.
4. **Results pass through unchanged.** A successful `result` becomes the tool
   result content as JSON. A service refusal becomes a tool result with
   `isError: true` carrying the same `{code, message}`. The adapter adds no
   wrapper or reinterpretation; MCP protocol failures stay separate from
   service refusals, as with JSON-RPC.
5. **Discovery follows `capabilities.operations`.** At startup the adapter reads
   `capabilities` once and lists a tool only when its operation is advertised
   and is in the initial set below. An operation that is not advertised is
   never listed, so tool presence and the contract cannot disagree.
6. **Lifetime matches `wright serve`.** One process binds one project given by
   `INPUT` (default: the current directory). There is no open-project tool;
   several projects use several server entries in the harness configuration.

### Initial tool set

Chosen from the benchmark's scenario flow (understand, navigate, validate,
edit), not by mirroring the contract:

| Stage | Operations |
| --- | --- |
| Overview | `project`, `symbols` |
| Navigate | `references`, `usage`, `callGraph` |
| Validate | `check`, `lint`, `costEstimate` |
| Edit | `semanticRename`, `validateEditTransaction` |

Not exposed initially: `rules`, `cfg`, `findings`, `persistentObjects`,
`lintRules`, `targetMetadata`, `inspect`, `analyze`, `compile`, and the
`provider*` operations. The `provider*` operations need a provider document set
and LSP coordinates that a model cannot reasonably construct. An operation joins
the set only on benchmark or workflow evidence, and its description cost is
counted when it does.

### Contract changes made first, in `ToolService`

These belong to the semantic layer so that CLI, `serve`, embedding, and MCP see
identical behavior. Neither is an adapter feature.

1. **Freshness.** `ToolService` checks the loaded inputs against disk before
   handling each request and reloads when they changed. Every response reflects
   the project as of the request. No `reload` operation is added: an extra tool
   the model must remember to call is a failure mode, and the check is a cheap
   fingerprint comparison. A failed reload is a structured refusal, never a
   silent fall back to the old snapshot.
2. **Edit sources default to disk.** In `validateEditTransaction` and
   `semanticRename`, `sources` becomes optional. When absent, the service reads
   the current text of the files the request names. When present it is still the
   authoritative precondition text, so an embedder with an unsaved buffer
   (for example `wright-consumer`) keeps that ability and `edit-stale-source`
   semantics are unchanged. The MCP tool schemas omit `sources`.

### Changing `wright-agent/v1` in place

Because no consumer outside this repository is known and the 1.0 freeze has not
happened, these changes modify `wright-agent/v1` and its committed schema
directly, as #431 did, instead of introducing `wright-agent/v2` or keeping the
previous shapes. In-tree consumers and tests are updated in the same changes,
and tests that exist only to preserve the old shapes are removed. This is a
recorded pre-freeze exception to the additive-only rule in the agent contract; it
needs owner approval with this ADR. After the 1.0 freeze the rule applies again
without exception.

Further schema changes that make operations easier for agents (for example
merging `findings` and `lint`) are not decided here. Each needs benchmark or
workflow evidence and its own Issue.

### Verification

- **Equivalence.** For every exposed tool, a test runs the same request through
  the MCP adapter and through `ToolService::handle` on the same fixture and
  requires identical result values. It covers refusal paths (`unknown-symbol`,
  `ambiguous-symbol`, `edit-stale-source`, `edit-requires-provider`) to show the
  adapter neither swallows nor rewrites them, and asserts that `tools/list` is a
  subset of `capabilities.operations`.
- **Freshness.** A test edits a project file on disk and requires the next
  query to reflect it, with and without the MCP adapter in the path.
- **Benchmark.** `wright` gains a level `mcp` beside `bin`, in the same
  condition grid, so scenario, prompt, grader, model, and trial identity stay
  fixed. The adapter registers the server for the agent; the Wright trace
  records MCP calls so `wrightUse` and friction cover them. Reports compare
  `mcp` with `bin` on usable/passed rate with confidence intervals, and on
  search/read/tool-call counts, turns, and tokens. Tool-schema context is
  counted in the `mcp` cost, so an apparent saving in calls cannot hide a larger
  fixed prompt. This level is orthogonal to the docs/skill conditions in #466.

## Consequences

- Native-tool access ships with the existing binary and one contract; MCP can
  be dropped or replaced without touching `ToolService`.
- Edit operations become usable by an agent without resending files, for every
  surface, not only MCP.
- A long-lived session stays correct across the agent's own edits.
- The tool set is small, so fixed schema context is bounded; the benchmark
  decides growth.
- A pre-freeze change to `wright-agent/v1` is made rather than avoided.

## Compatibility impact

No source-language, diagnostic, or compiler-output compatibility claim changes.
`wright-agent/v1` changes as described: `sources` becomes optional on two
operations and the service refreshes stale state. The `wright-result/v1` result
envelopes and CLI behavior are unchanged. Operation results are unchanged, so
the existing equivalence between CLI queries and `ToolService` (#429) still
holds.

## Scope boundaries

- No new operations, no new semantics, and no change to OPY, DEL/OSTW, or
  Workshop ownership.
- Provider-backed edits through MCP, and any tool beyond the initial set, are
  deferred to benchmark evidence.
- Other harness-specific adapters, an agent framework, and a planner are out of
  scope.
- Whether MCP server configuration should be installed by the first-party guide
  installer belongs to #415.
- Whether to use an existing Rust MCP SDK or a minimal handwritten protocol
  layer is an implementation choice for the adapter Issue, made after checking
  the candidate's current documentation and type support.
