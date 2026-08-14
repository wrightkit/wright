# ADR-0007: OverPy reference pinning policy

- Status: Accepted
- Date: 2026-08-14
- Related: [`docs/compatibility.md`](../compatibility.md) ("supported OverPy
  version range and extension policy" open question),
  [ADR-0002](0002-compatibility-strategy.md),
  [ADR-0004](0004-overpy-licensing-boundary.md),
  [Issue #82](https://github.com/wrightkit/wright/issues/82),
  [Issue #86](https://github.com/wrightkit/wright/issues/86)

## Context

The compatibility oracle is pinned to `overpy@9.7.10` (npm content == git
commit `889d974`, tag `v9.7.10`). M11 phase-1 evidence (issue #81/#82) flagged
five constructs as "inconclusive on version" — it was unverified whether a
newer OverPy accepts them. The Track B investigation (issue #82)
installed the newest reference (npm `9.7.13`, content == master HEAD commit
`d854bf0`) in isolation and measured every evidence construct: **every accept
and every reject is identical across the 9.7.10 → 9.7.13 range**, with
byte-identical diagnostics and normalized Workshop output. The only source
differences are hero/settings schema data (`customGameSettingsSchema.json`,
`src/data/*.ts`); no lexer, parser, preprocessor, or compiler logic changed.
Newer versions ship in same-day batches (three releases on 2026-08-12), so
"latest" is a moving target that would demand constant re-pinning.

### Provenance nuance

The npm registry `gitHead` field lags the tarball content by one release
(release pipeline bumps the version, builds, publishes, then commits and
tags). For the pinned oracle: content == `889d974` (tag `v9.7.10`), while the
recorded `gitHead` `1e268895` is the `v9.7.9` tag commit. The integrity hash
in `oracle-metadata.json` pins the content and makes reproduction safe;
reviewers should not treat the recorded `gitHead` as the content commit.

## Decision

The primary oracle pin is **version-exact and content-pinned**, and it is
changed only on **demonstrated behavioral need — never on release recency**.

1. **Version-exact.** The pin is an exact npm version plus its integrity hash,
   recorded in `oracle/package.json`, `oracle/pnpm-lock.yaml`, and
   `oracle-metadata.json`. No range specifiers, no `latest`, no caret.
2. **Content-pinned.** The recorded identity includes the npm integrity hash
   and the byte-verified git content commit. A version bump alone is not an
   oracle change.
3. **Demonstrated need only.** "Demonstrated" means a version-sensitivity run
   — the minimal repro plus the evidence source against candidate versions —
   showing a different accept/reject outcome or a different normalized output
   for a construct the corpus needs. Re-running the sensitivity matrix is the
   decision tool at every evaluation point; absence of measured divergence is
   a no-change decision.
4. **Single reference by default.** A second reference is added only when a
   divergence is demonstrated and a single reference cannot represent it.
   Without a demonstrated divergence, a second reference is maintenance
   without evidence.
5. **Identity in every result.** Every compatibility result continues to
   record the exact pinned identity (version, content commit, integrity) as
   required by [`docs/compatibility.md`](../compatibility.md), so historical
   claims remain interpretable after any future re-baseline.

## Consequences

* **No churn-driven re-baselines.** The oracle is not re-pinned to match npm
  "latest"; the 2026-08-12 same-day release batch does not trigger a change.
* **Historical claims stay interpretable.** "overpy@9.7.10" results mean
  exactly the pinned content (`889d974`); a future re-baseline re-stamps new
  results with the new identity instead of rewriting old ones.
* **Newest-hero settings unavailable until needed.** Settings-schema data
  newer than the pin (dmon/domina/mizuki/vendetta hero settings) is not
  available to fixtures until a demonstrated need triggers the upgrade.
* **Matrix re-run at each evaluation point.** The sensitivity matrix in
  the Track B investigation (issue #82)
  is the reusable decision tool; a new OverPy release triggers a re-run when
  an evaluation point (fixture acquisition, corpus extension, or claimed
  behavior) depends on it.
* **Structured review path.** A pin change follows the existing review steps:
  `oracle-metadata.json`, lockfile, `run_oracle.py --update` snapshot review,
  adapter fixtures, and provenance notes in one reviewed change.

## Compatibility impact

S/D/N claims for the current corpus are unaffected within the measured
9.7.10–9.7.13 range: accept/reject outcomes, diagnostics, and normalized
Workshop output are byte-identical (verified in the Track B matrix). A future
re-baseline changes only the oracle identity block of `oracle.json` and the
adapter `frontend` stamp; differential parity is version-insensitive because
the generator identity is stripped before comparison
(`crates/wright-opy/tests/differential.rs`). No E-level claim uses the oracle.

## Open questions

* Whether a post-9.7.13 OverPy release adds `"""` docstrings, `#!obfuscate`,
  custom `_hp_*` members, or inline `if` without `else` — unresolved; the
  matrix must be re-run before any new acceptance is claimed.
* Whether a future fixture needs hero settings newer than the pin — answered
  by the fixture acquisition pipeline when such a candidate appears.
* Whether the npm `gitHead`-lags-content behavior persists in future release
  pipelines — the content commit must be byte-verified, not assumed from the
  registry field.
