# Post-ADR-0010 decision inventory

Audit date: 2026-09-12

This inventory records the Wright-side candidates examined for the post-
ADR-0010 history audit. It separates historical architecture decisions from
implementation reality and current execution state. The five backfilled ADRs
were reconstructed from the cited Issues, PRs, current contracts, and review
evidence; they did not exist when the original work landed.

## Classification

| Candidate | Classification | Result |
| --- | --- | --- |
| #209: distinct `check` / `lint` / `analyze` semantics | Backfill required | [ADR-0011](0011-distinct-tooling-workflows.md) |
| #240/#244: first-party provider distribution, lazy bootstrap, local activation, and explicit updates | Backfill required | [ADR-0012](0012-first-party-provider-distribution.md) |
| #243/#246: entry-based source-provider seam and canonical/provenance handoff | Backfill required | [ADR-0013](0013-entry-based-source-provider-seam.md) |
| #261/#283: R2 distribution, finalized as repository-namespaced immutable artifacts with `latest/version` pointer | Backfill required; #283 supersedes the initial #261 object shape | [ADR-0014](0014-namespaced-immutable-r2-artifacts.md) |
| #309: semantic facts, declarative rules, project lint policy, and findings | Backfill required | [ADR-0015](0015-canonical-facts-and-declarative-lint-policy.md) |
| #234/#235: consume owner APIs and keep Wright's internal crates out of crates.io | Existing ADR-covered / implementation contract | Owner boundaries and dependency direction follow ADR-0009/0010; publication intent is maintained in the release contract, not a new architecture decision. |
| #286/#288: R2 provider route and HTTP transport changes | Non-ADR implementation detail | These implement the provider/distribution decisions; they do not change ownership or the public product boundary. |
| #284/#285: Windows and installer consumers of the R2 contract | Non-ADR implementation detail | Consumer integration follows ADR-0014. |
| #294/#298/#301/#302/#306/#313: cleanup, provider-contract relocation, static dependency removal, and bootstrap hardening | Existing ADR consequence | These complete or simplify the ownership/provider decisions in ADR-0009/0010/0012/0013; they do not independently establish a new durable choice. |
| Broader provider families, richer source-span mapping, and programmable/remote rules | Unresolved or explicitly deferred | No accepted historical decision is invented. New evidence should produce a new Issue and, if material, a future ADR. |

## Existing-ADR coverage

ADR-0008 already establishes tooling-first priority, observable semantic
compatibility, Workshop-centered conversion, source-oriented edits, and
corpus-defined support claims. ADR-0010 establishes independent language
implementations, the meaning of frontend/provider, the dependency direction,
and Wright's product boundary. The later cleanup and migration work listed
above is therefore a consequence of those decisions unless it changes a
durable public contract.

The backfilled ADRs link to those earlier records instead of restating them.
Current architecture documents remain the authority for present invariants;
the ADRs preserve why the boundaries were selected.

## Evidence used

- Product workflow: [Issue #209](https://github.com/wrightkit/wright/issues/209),
  [PR #211](https://github.com/wrightkit/wright/pull/211), and
  [PR #218](https://github.com/wrightkit/wright/pull/218).
- Provider distribution and seam: [Issue #240](https://github.com/wrightkit/wright/issues/240),
  [Issue #243](https://github.com/wrightkit/wright/issues/243),
  [PR #247](https://github.com/wrightkit/wright/pull/247),
  [PR #248](https://github.com/wrightkit/wright/pull/248),
  [PR #250](https://github.com/wrightkit/wright/pull/250),
  [PR #252](https://github.com/wrightkit/wright/pull/252), and
  [PR #304](https://github.com/wrightkit/wright/pull/304).
- R2 distribution: [Issue #261](https://github.com/wrightkit/wright/issues/261),
  [Issue #283](https://github.com/wrightkit/wright/issues/283),
  [PR #279](https://github.com/wrightkit/wright/pull/279),
  [PR #291](https://github.com/wrightkit/wright/pull/291), and
  [PR #285](https://github.com/wrightkit/wright/pull/285).
- Analysis/lint contract: [Issue #309](https://github.com/wrightkit/wright/issues/309),
  [PR #310](https://github.com/wrightkit/wright/pull/310), and the current
  [SPEC-309](../specs/SPEC-309-extensible-lint-rules.md).
