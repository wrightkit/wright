# ADR-0011: Distinct check, lint, and analyze workflows

- Status: Accepted (backfilled)
- Date: 2026-09-12 (backfill date)
- Clarifies: [ADR-0008: Tooling-first semantic platform rebaseline](0008-tooling-first-semantic-platform.md) and [ADR-0010: Independent language implementations and Wright integration](0010-independent-implementations-and-wright-integration.md)
- Related: [Issue #209](https://github.com/wrightkit/wright/issues/209),
  [PR #211](https://github.com/wrightkit/wright/pull/211),
  [PR #218](https://github.com/wrightkit/wright/pull/218)

## Historical note

This record was backfilled on 2026-09-12. The decision emerged through Issue
#209 and its implementation/review history in PRs #211 and #218; this ADR did
not exist when those changes landed.

## Context

Wright's tooling-first direction makes correctness checks, lint policy, and
semantic analysis three different user needs. Before #209, the commands
overlapped because one analyzer path produced findings that were presented as
`check`, `lint`, and `analyze` output. That made ordinary lint policy look like
a correctness failure and left analysis coupled to the lint registry.

ADR-0008 establishes tooling as the primary product surface, and ADR-0010
defines Wright as the cross-language tooling layer. Neither record specifies
the observable boundary between these three workflows.

## Decision

Wright exposes three distinct workflows over shared owner-backed semantic
inputs:

1. **`check`** is the correctness gate for the stages exposed by the selected
   owner/workflow. It loads the selected source and reports frontend, project,
   and semantic diagnostics; when the workflow provides lowering and
   canonical validation, their diagnostics are included as well. A complete
   compiler backend is not a prerequisite for diagnostic `check`, and an
   unavailable requested stage remains an explicit owner/provider outcome.
   Ordinary configurable lint findings are not part of this gate by default.
2. **`lint`** applies configurable Wright or local rules to semantic facts and
   reports rule findings. Findings retain stable rule identity, effective
   severity, evidence classification, boundedness where applicable, and
   source/provenance information available from the owner boundary.
3. **`analyze`** reports semantic facts and measurements such as program
   structure, symbol usage, and CFG-derived indicators. It is not another
   presentation of the lint registry or its finding list.

The commands may share loading and analysis work internally, but their typed
results, human-readable descriptions, and machine-readable payloads preserve
these roles. Analysis facts may feed lint rules; lint findings do not define
the analysis contract.

The initial analysis surface remains deliberately bounded. A larger analysis
framework is not required before a useful, deterministic report exists.

## Alternatives considered

- **One analyzer result under three command names:** rejected because it
  conflates correctness, policy, and information workflows.
- **Make `analyze` the lint registry with extra presentation:** rejected
  because it prevents facts from being reused independently of rule policy.
- **Build a general analysis framework first:** deferred because the product
  boundary can be established with a narrow semantic report and expanded from
  real workflow evidence.

## Consequences

- CLI, driver, tool, and agent consumers must preserve the workflow-specific
  result types while sharing common envelopes and semantic inputs.
- Changes to lint policy do not silently change the correctness gate.
- New analysis facts can be introduced without creating a lint rule or
  assigning project severity.
- The `AnalyzeResult` payload is a facts/report contract rather than a finding
  list; consumers must not infer lint output from it.
- Documentation and help text must describe the three workflows separately.

## Compatibility impact

The shared `wright-result/v1` envelope and deterministic JSON behavior remain
the machine-readable boundary. The command payload distinction is a product
contract change: `analyze` reports facts, while `lint` owns findings and rule
metadata. Existing owner semantics and canonical Workshop ownership are
unchanged.

## Open questions

- Which additional semantic measurements justify promotion into the bounded
  `analyze` report remains evidence-driven.
- Runtime behavior and server cost remain separate from static analysis
  evidence, as described by the current tooling contract.
