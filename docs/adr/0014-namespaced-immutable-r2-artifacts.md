# ADR-0014: Repository-namespaced immutable R2 release artifacts

- Status: Accepted (backfilled)
- Date: 2026-09-12 (backfill date)
- Related: [Issue #261](https://github.com/wrightkit/wright/issues/261),
  [Issue #283](https://github.com/wrightkit/wright/issues/283),
  [PR #279](https://github.com/wrightkit/wright/pull/279),
  [PR #291](https://github.com/wrightkit/wright/pull/291),
  [PR #285](https://github.com/wrightkit/wright/pull/285),
  [docs/release.md](../release.md)

## Historical note

This record was backfilled on 2026-09-12. Issue #261 established the initial
R2 distribution direction; Issue #283 refined and superseded its root-level
and duplicate-latest object shape. The final contract emerged through PRs
#279, #291, and #285. This ADR records the superseding decision rather than
pretending that the final namespace existed when #261 was implemented.

## Context

R2 gives Wright's installer a stable distribution endpoint without making
ordinary installs depend on the GitHub Releases API. The initial design
allowed root-level release paths and duplicate artifact copies under a mutable
`latest` directory. That created two representations of one artifact and
made the latest route both a pointer and an artifact alias.

Wright needs reproducible pinned installs, a simple latest-install lookup, and
an explicit publication ordering while retaining GitHub Releases as the
canonical release record.

## Decision

Wright's public R2 objects live under the repository-owned `/wright` namespace:

```text
GET /wright/latest/version
GET /wright/releases/<version>/wright-<version>-<target>.<ext>
GET /wright/releases/<version>/wright-<version>-<target>.<ext>.sha256
```

Each archive and checksum is published once at its immutable versioned path.
`/wright/latest/version` is the only mutable latest pointer; it resolves a
version and is not an artifact directory or alias. Latest and pinned installs
therefore use the same immutable versioned artifact path after version
resolution.

The release workflow reuses verified release artifacts, publishes and publicly
verifies the complete versioned set, and advances the latest pointer only
after that verification. Versioned publication uses storage-level no-overwrite
semantics. GitHub Releases remains the canonical release and provenance
record; R2 is an exact installer distribution copy.

The same repository-namespace/latest-pointer/versioned-artifact shape is used
for first-party provider releases where the owner repository publishes that
contract, but Wright's binary release and provider release remain separate
owned products.

## Alternatives considered

- **Keep root-level release objects:** rejected because a shared release domain
  needs repository ownership and collision-free paths.
- **Publish duplicate artifacts under `latest/<artifact>`:** rejected because
  it creates a second mutable artifact route and duplicate state.
- **Resolve latest through the GitHub Releases API:** rejected because the
  installer should use a small static pointer and avoid API availability/rate
  limits.
- **Reconcile mutable objects with a workflow-level state machine:** rejected
  because versioned no-overwrite objects plus a verified pointer provide the
  needed retry and failure behavior with less mutable state.

## Consequences

- Installers have one latest lookup and one immutable artifact route.
- Partial publication cannot advance the pointer to an incomplete version.
- Existing released installers can be handled as cutover compatibility, but
  legacy root-level paths and latest-artifact aliases are not the durable
  contract.
- Release workflow, installer, and fixture changes must keep the namespace and
  pointer semantics aligned.

## Compatibility impact

GitHub Release artifacts, checksums, target naming, package-manager ownership,
and canonical release provenance remain unchanged. The R2 object layout is the
supported installer contract; the superseded root-level and duplicate latest
artifact paths are not retained as a second supported route.

## Scope boundaries

- Cache policy and infrastructure enforcement are operational concerns of the
  R2 deployment; the pointer-versus-immutable-object distinction is part of
  this contract.
- This ADR does not decide whether additional repositories adopt the same
  shape; each adoption requires its own owner and release evidence.
