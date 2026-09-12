# ADR-0012: First-party provider distribution and local activation

- Status: Accepted (backfilled)
- Date: 2026-09-12 (backfill date)
- Clarifies: [ADR-0010: Independent language implementations and Wright integration](0010-independent-implementations-and-wright-integration.md)
- Related: [Issue #240](https://github.com/wrightkit/wright/issues/240),
  [Issue #244](https://github.com/wrightkit/wright/issues/244),
  [PR #248](https://github.com/wrightkit/wright/pull/248),
  [PR #250](https://github.com/wrightkit/wright/pull/250),
  [PR #252](https://github.com/wrightkit/wright/pull/252),
  [opy-rs PR #178](https://github.com/wrightkit/opy-rs/pull/178)

## Historical note

This record was backfilled on 2026-09-12. The decision emerged from the
provider-distribution design in Issue #240, implementation Issue #244, and
the owner/consumer history in PRs #248, #250, and #252; this ADR did not exist
when those changes landed.

## Context

Wright consumes independently released language implementations. Linking a
fast-moving OPY implementation into every Wright binary release would couple
the product release cadence to owner-side changes, even though LPP already
provides the execution boundary.

The distribution layer needs to make that boundary usable without becoming a
package manager, provider registry, dependency solver, or second runtime. OPY
is the first provider target; raw Workshop remains a direct `workshop-rs`
workflow.

## Decision

Wright resolves a first-party OPY provider executable in this order:

1. an explicitly configured local executable;
2. the active provider in Wright's per-user versioned provider store;
3. lazy bootstrap of the latest stable target-specific provider artifact
   published by `opy-rs`.

Downloaded artifacts are verified with their published SHA-256 checksum,
installed into versioned temporary/final state, and activated only after the
complete installation succeeds. An unsuccessful download, integrity check, or
installation cannot replace the previous active provider.

The local store and its active pointer support atomic activation, offline
reuse, and recovery from a failed update. They are not project state, a
lockfile, or a dependency graph. Routine `check`, `compile`, `lint`, and
`analyze` invocations reuse the active provider and do not poll for updates.
Provider updates are explicit user operations.

The resolved path is handed to Wright's existing LPP/provider runtime and
`ProviderRegistry`. Distribution metadata does not duplicate language
capabilities or protocol negotiation. Missing, unsupported, offline, and
failed provider operations remain explicit structured failures; Wright does
not silently fall back to a static source-language implementation.

## Alternatives considered

- **Keep linking the latest OPY implementation into Wright:** rejected because
  it recreates release-cadence coupling and exposes owner implementation
  packaging to the product.
- **Add a provider registry, dependency solver, or project lockfile:** rejected
  because the first-party workflow needs one owner release source and local
  activation, not a package ecosystem.
- **Poll or update on every source command:** rejected because ordinary CI and
  offline use should be deterministic; updates are explicit.
- **Introduce a second provider execution abstraction:** rejected because the
  existing LPP runtime already owns process execution and protocol behavior.

## Consequences

- `opy-rs` owns provider binaries and their release artifacts; Wright owns
  resolution, verification, local activation, and handoff.
- Clean installs can acquire a provider only when an OPY workflow needs one;
  raw Workshop and unrelated commands perform no provider work.
- Provider bootstrap and update failures are visible at the product boundary
  rather than masked by a compatibility fallback.
- Source entry/project semantics and owner diagnostics remain governed by the
  entry-based boundary in ADR-0013.

## Compatibility impact

This decision changes packaging and process ownership, not OPY or Workshop
semantics. The provider must satisfy the owner-published LPP contract and
Wright must preserve structured diagnostics and provenance across the handoff.
Provider target matrices and release versions are implementation details outside
this ADR.

## Scope boundaries

- This ADR does not decide whether to support broader provider families; any
  generalization requires an owner-backed, real-project workflow.
- Stronger source-span mapping is a provider/protocol contract question, not a
  reason to weaken the explicit unmapped-evidence behavior.
