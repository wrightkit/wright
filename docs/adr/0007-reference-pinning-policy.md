# ADR-0007: OverPy reference pinning policy

- Status: Partially Superseded by [ADR-0018](0018-tests-first-integration-verification.md)
- Date: 2026-08-14
- Related: [`docs/compatibility.md`](../compatibility.md),
  [ADR-0002](0002-compatibility-strategy.md),
  [ADR-0004](0004-overpy-licensing-boundary.md),
  [Issue #82](https://github.com/wrightkit/wright/issues/82),
  [Issue #86](https://github.com/wrightkit/wright/issues/86)

## Context

The OverPy reference used by `opy-rs` was pinned to `overpy@9.7.10` (npm
content == git commit `889d974`, tag `v9.7.10`). A version-sensitivity run
compared it with npm `9.7.13` (content == master HEAD commit `d854bf0`) and
found the same accept/reject outcomes, diagnostics, and normalized Workshop
output for the tested constructs. The only source differences were hero and
settings schema data (`customGameSettingsSchema.json`, `src/data/*.ts`).
Same-day release batches made an unqualified `latest` reference unsuitable for
reproducible owner tests.

The npm registry `gitHead` field lags the tarball content by one release. For
the pin, content is `889d974` while the recorded `gitHead` `1e268895` is the
`v9.7.9` tag commit. The npm integrity hash and byte-verified content commit
identify the actual reference.

## Decision

`opy-rs` keeps a version-exact, content-pinned OverPy reference and changes it
only when a version-sensitivity test demonstrates a needed behavior difference.

1. The exact npm version and integrity hash are recorded in the owner package
   and lock files. Range specifiers and `latest` are not allowed.
2. The pin records both the npm integrity hash and the byte-verified content
   commit. A version bump alone is not a semantic change.
3. A candidate version is accepted only after a minimal reproducible case or
   owner test distinguishes its accept/reject result, diagnostic, normalized
   output, or required schema.
4. One reference is the default. A second reference requires a demonstrated
   divergence that one reference cannot represent.
5. Owner-side compatibility results identify the exact version, content commit,
   and integrity hash. Wright does not copy the owner result database.

## Consequences

The reference is not re-pinned for release recency. Results owned by `opy-rs`
remain interpretable after a future rebaseline because their reference identity
is explicit. Settings data outside the pin requires a reviewed owner pin change.

Pin changes update the owner metadata, lockfile, owner test outputs, and
provenance records together. Wright changes only when a provider or public
product contract needs a focused integration regression.

## Compatibility impact

This ADR preserves the durable reference identity, clean-room, licensing, and
provenance rationale. ADR-0018 removes the former Wright-side reference-output
snapshot store and integrity gate; those files and comparisons remain an
`opy-rs` responsibility.

## Scope boundaries

This ADR does not establish behavior for OverPy versions after 9.7.13. The
owner sensitivity matrix must be rerun before accepting constructs that depend
on a different version. The npm `gitHead` field alone is not sufficient to
identify tarball content; every owner pin needs a byte-verified content commit.
