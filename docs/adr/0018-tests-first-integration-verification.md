# ADR-0018: Tests-first integration verification

- Status: Accepted
- Date: 2026-09-20
- Supersedes: the Wright-owned result-storage and result-taxonomy portions of
  [ADR-0002](0002-compatibility-strategy.md) and
  [ADR-0007](0007-reference-pinning-policy.md)
- Related: [Issue #371](https://github.com/wrightkit/wright/issues/371),
  [Issue #60](https://github.com/wrightkit/.github/issues/60),
  [ADR-0010](0010-independent-implementations-and-wright-integration.md)

## Context

Wright owns product integration and tooling, while `opy-rs`, `deltin-rs`, and
`workshop-rs` own their language or Workshop contracts. A source-language
corpus, reference output archive, and integrity test in Wright duplicate an
owner responsibility without testing a Wright-owned behavior.

Wright still needs focused inputs and direct integration checks. Removing the
duplicate store must not remove provider failure coverage, canonical owner
contract checks, public CLI behavior, or real-project validation.

## Decision

1. Language syntax, semantics, compatibility methods, reference pins, and
   support claims stay in the owning repository.
2. Wright verifies its own behavior with ordinary unit, integration, provider,
   CLI, embedding, and real-project tests. The test asserts a Wright-owned
   result or compares Wright with a public owner contract.
3. A committed input is allowed only when a named Wright test or smoke check
   consumes it. Keep the smallest input and expected result that protects that
   contract. Do not add a generic corpus, oracle, snapshot archive, or
   self-consistency test.
4. A redistributed input keeps the source identity, immutable revision,
   license metadata, source attribution, and reproducibility information needed
   to review its inclusion. This attribution does not transfer semantic
   ownership to Wright.
5. Provider refusal, unsupported capability, malformed input, and owner
   failures remain explicit product results. Removing duplicated reference
   data must not turn them into success or an empty result.

## Consequences

Wright's CI runs the Rust tests that exercise product and provider contracts.
Owner-side corpus refreshes no longer require synchronized Wright snapshots or
metadata updates. A new source-language behavior is implemented and tested in
the owner repository first; Wright adds only the integration regression needed
to protect its own boundary.

The historical S/D/N/E compatibility model remains useful to the owner
repositories and to reference comparison discussions. It is not a Wright
fixture taxonomy or a current Wright release gate.

## Compatibility impact

This decision removes Wright's duplicated OPY corpus, recorded reference
outputs, and snapshot-integrity gate. It does not remove Wright's provider
integration, Workshop owner-contract checks, diagnostic propagation, or public
tooling tests. Wright must not claim source-language semantic completeness from
its integration suite.

## Scope boundaries

Reference pinning, clean-room/licensing rules, and source mapping/owner
attribution remain durable requirements where an owner repository or a named
Wright test needs them. A future compatibility or differential result belongs to
the owning repository unless it directly protects a Wright-owned public contract.
