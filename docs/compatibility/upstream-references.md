# Centralized Upstream / Reference Inventory

Status: accepted baseline — centralized upstream-reference documentation (#106)
Scope: project-level provenance for every upstream implementation Wright studies
or derives compatibility knowledge from; the durable record that lets Wright
read and reference upstream source without per-symbol or per-file provenance
bureaucracy

Wright's compatibility implementation is expected to read and study upstream
OverPy source (and later OSTW source) where necessary. Reimplementing
compatible language semantics without inspecting the reference implementation
is neither required nor desirable. This document records the durable
project-level facts once; implementation issues and compatibility entries
reference it instead of repeating provenance notes.

## OverPy reference

### Identity

| Field | Value |
| --- | --- |
| Project | OverPy — high-level language for the Overwatch Workshop |
| Repository | <https://github.com/Zezombye/overpy> |
| Pinned reference | npm `overpy@9.7.10` |
| Content commit | `889d974` (byte-verified; `git describe --tags --exact-match` == `v9.7.10`) |
| Registry integrity | `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==` |
| Recorded `gitHead` | `1e268895` — lags tarball content by one release (it is the `v9.7.9` tag commit); do not treat it as the content commit |
| License assumption | GPL-3.0-only (recorded in `compatibility/oracle/oracle-metadata.json`; an engineering assumption, not a legal conclusion — see [`docs/licensing.md`](../licensing.md)) |
| Language | en-US (Workshop locale for reference evidence) |

The integrity hash in `oracle-metadata.json` pins the content; reproduction
uses the recorded identity, never `latest` or a range.

### Oracle role

OverPy 9.7.10 (pinned content) is the compatibility **oracle** and **behavior
reference** for Wright's `.opy` frontend, per [`docs/compatibility.md`](../compatibility.md)
and [ADR-0007](../adr/0007-reference-pinning-policy.md). It is not a production
runtime dependency of the Wright core and is never bundled into release
artifacts. Concretely, it serves as:

* the reference for S (syntax), D (diagnostic), and N (normalized-output)
  evidence in the compatibility corpus (`compatibility/fixtures/**`,
  `compatibility/oracle/`);
* the pinned frontend invoked by the adapter (`adapter/`) to produce Opy HIR
  v1 reference fixtures (`adapter/fixtures/**`), compared against the native
  frontend at the HIR boundary by `crates/wright-opy/tests/differential.rs`;
* the source of systematic probe validation for the proactive compatibility
  baseline (see [`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md)
  and [`docs/opy/compat-manifest-spec.md`](../opy/compat-manifest-spec.md)).

### Wright surfaces that use it as reference

| Wright surface | Use of the reference |
| --- | --- |
| `wright-opy` native frontend | Differential HIR parity, accept/reject agreement, structured diagnostics |
| `wright-workshop` catalog/emission | Canonical en-US spelling validation against oracle-emitted Workshop text; receiver-method and enum emission evidence |
| `compatibility/` harness | Fixture snapshots, oracle identity blocks, S/D/N gate evidence |
| Systematic baseline (planned) | Reference-validated probes for builtin action/value/member/enum/signature metadata |

### Reference semantics vs Wright-owned architecture

Studying the reference informs Wright's compatibility semantics; it does not
define Wright's implementation. Per [ADR-0004](../adr/0004-overpy-licensing-boundary.md)
and [`docs/licensing.md`](../licensing.md), the Wright core:

* never links to, copies source from, or imports internal types of OverPy;
* keeps HIR, Workshop IR, diagnostics, and backend APIs Wright-owned;
* treats observed reference behavior (through documented compatibility tests
  and the adapter boundary) as a permitted input, not as permission to copy an
  implementation.

The Wright-owned, reference-validated manifest described in
[`docs/opy/compat-manifest-spec.md`](../opy/compat-manifest-spec.md) follows this
boundary: entries are Wright-authored and oracle-validated, never mechanically
extracted from OverPy's GPL-3.0 data files.

### Durable reference limitations

* **Pinning policy.** The oracle is version-exact and content-pinned and is
  changed only on demonstrated behavioral need, never on release recency
  (ADR-0007). A version bump alone is not an oracle change.
* **Measured stability.** Every accept/reject outcome and diagnostic in the
  M11 evidence set is byte-identical across `9.7.10 → 9.7.13`; only
  hero/settings schema data differs. Historical claims stay interpretable
  because every result records the exact pinned identity.
* **Settings data newer than the pin.** Hero settings newer than the pin
  (e.g. dmon/domina/mizuki/vendetta) are unavailable to fixtures until a
  demonstrated need triggers the upgrade.
* **Unresolved upstream questions.** Whether a post-9.7.13 OverPy adds `"""`
  docstrings, `#!obfuscate`, custom `_hp_*` members, or inline `if` without
  `else` is unverified; the ADR-0007 sensitivity matrix must be re-run before
  any new acceptance is claimed.
* **Round-trip boundary.** Emitted `settings` sections are deliberately not
  reparseable by the Workshop frontend; a `.ws` decompiler is a non-goal.

## Extension: OSTW (deferred)

OSTW becomes a second reference when M13 begins. This document is the single
extension point; the same project-level facts (repository identity, license,
pinned version/commit policy, surfaces using it as reference, reference-vs-
Wright architecture distinction, limitations) are added here rather than
scattered through OSTW implementation issues.

## Related decisions and documents

* [ADR-0007: OverPy reference pinning policy](../adr/0007-reference-pinning-policy.md)
* [ADR-0004: OverPy licensing and clean-room boundary](../adr/0004-overpy-licensing-boundary.md)
* [ADR-0002: Compatibility strategy](../adr/0002-compatibility-strategy.md)
* [`docs/compatibility.md`](../compatibility.md) — S/D/N/E framework and reference boundary
* [`docs/licensing.md`](../licensing.md) — component policy and permitted inputs
* [`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md) — tiered baseline built on this reference
* [`docs/opy/compat-manifest-spec.md`](../opy/compat-manifest-spec.md) — machine-readable manifest specification
