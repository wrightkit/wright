# ADR-0033: The simulate operation in the driver and `wright-agent/v1`

- Status: Accepted
- Date: 2026-10-20
- Related: [ADR-0020](0020-native-agent-tool-adapter.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0029](0029-structured-statement-edits.md), [ADR-0030](0030-renderers-and-the-cli-action-boundary.md), [ADR-0032](0032-structured-editor-form-factor.md), [Agent contract](../agent-contract.md), `athena` Issues for the simulator entry point and trace format and for the scenario format

## Context

The offline Workshop environment needs the `athena` simulator to reach `wright` the way `analyze` and `inspect` do, so that `wright ui`, `wright serve`, MCP, and CI read one surface. ADR-0032 keeps the UI free of Workshop semantics, so the UI cannot run or interpret a simulation itself.

`athena` owns the execution model. Its entry point takes Workshop text and options and returns a trace. The trace format, including an explicit "not modeled" marker, and the scenario format are `athena` contracts. A scenario states players, injected events, and expected observations, and inputs it omits are reported as missing, never defaulted.

Goal principle 9 requires that simulation results be labelled by how each behavior was established (measured in the client, modeled, or not modeled), and that the toolchain not claim to guarantee runtime behavior.

## Decision

1. **`simulate` is one operation of `wright-agent/v1`,** a thin wrapper over the `athena` entry point. Wright adds no simulation semantics, as ADR-0022 requires of every surface.
2. **Request.** `{"op":"simulate","inputIdentity":…,"sources"?:…,"scenario"?:…}`.
   - `inputIdentity` must equal the current `project.inputIdentity`; otherwise the request refuses with `edit-stale-source` and runs nothing, as in ADR-0029.
   - `sources` is optional and has the meaning it has for `validateEditTransaction` (#472): when supplied, the program is loaded from those texts and `inputIdentity` is checked against that loaded program.
   - `scenario` is an `athena` scenario value. Wright passes it through, does not interpret it, and supplies no defaults.
3. **Result.** The result is a `wright-result/v1` envelope, like `check` and `analyze`, carrying the `athena` trace unchanged. Renderers are pure functions of that model (ADR-0030). Wright never reinterprets or reorders trace content.
4. **Unmodeled behavior and missing inputs are result content, not refusals.**
   - Every behavior in the trace carries its basis from `athena`: measured, modeled, or not modeled. A construct the simulator does not cover is reported as not modeled and never takes a silent default.
   - Inputs the scenario does not supply are listed as `missingInputs`, each naming the input. A partial trace is still returned.
   - A result with unmodeled behavior or missing inputs is not presented as verified. The verdict wording belongs to the presentation contract.
   - The operation refuses only when the request itself cannot be served: a stale `inputIdentity`, a malformed `scenario`, or a project that does not load.
5. **Versioning.** `simulate` is an additive operation within `wright-agent/v1`. The contract is not frozen until 1.0, so the identifier is not bumped.
   - `capabilities` lists `simulate` in `operations` and reports the linked `athena` version.
   - The MCP tool is `wright_simulate`, per ADR-0020.
   - The trace carries its own `traceVersion`, owned and evolved by `athena`.
6. **No CLI command in this decision.** The operation is reachable through `wright serve` and MCP, and the UI reaches it through the driver. A later `wright simulate` command, if a real CI consumer needs it, is a thin `ToolService` command that adds no semantics (ADR-0022) and is decided with the trace format stable.

## Alternatives considered

- **A separate `wright-trace/v1` envelope.** It lets the trace evolve apart from the result model, but adds a second renderer and schema while the trace is already versioned by `athena`. Rejected.
- **Refusing a request with missing inputs.** It is stricter, but an agent could not receive a partial trace or the list of what to supply. Rejected.
- **`wright-agent/v2`.** The cost far exceeds the benefit while the contract is pre-1.0. Rejected.
- **A CLI command now.** It would fix a CLI presentation contract (exit codes, plain records) before the `athena` trace format is stable. Deferred.

## Consequences

- The driver gains a dependency on a tagged `athena` release; the implementing issue is blocked on it.
- `agent-contract.md`, the JSON schema, and the MCP tool table gain `simulate` with the implementation, not with this decision.
- CI that needs simulation before a CLI command exists goes through `wright serve`.

## Compatibility impact

No syntax, diagnostic, normalized-output, or compiler-compatibility claim changes. The change is additive to `wright-agent/v1`. The implementing issue must test `simulate` through `wright serve` on `practice-range` against the recorded probe captures for the scheduler behaviors they cover, and test that unmodeled behavior and missing inputs appear in the result.

## Scope boundaries

- The trace format, the scenario format, the "not modeled" marker, and the basis labels belong to `athena`.
- The exact fields of the result's `missingInputs` entries follow the `athena` scenario contract and are decided by the implementing issue.
- Verdict words and renderer layouts belong to the presentation contract.
- A `wright simulate` command is decided separately when a CI consumer exists.
