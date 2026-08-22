# Centralized Upstream / Reference Inventory

Status: accepted baseline (centralized upstream-reference documentation, #106,
extended with the OSTW reference by the reference investigation (#113))
Scope: project-level provenance for every upstream implementation Wright studies
or derives compatibility knowledge from; the durable record that lets Wright
read and reference upstream source without per-symbol or per-file provenance
bureaucracy

Wright's compatibility implementation is expected to read and study upstream
OverPy and OSTW source where necessary. Reimplementing compatible language
semantics without inspecting the reference implementation is neither required
nor desirable. This document records the durable project-level facts once;
implementation issues and compatibility entries reference it instead of
repeating provenance notes.

## OverPy reference

### Identity

| Field | Value |
| --- | --- |
| Project | OverPy: high-level language for the Overwatch Workshop |
| Repository | <https://github.com/Zezombye/overpy> |
| Pinned reference | npm `overpy@9.7.10` |
| Content commit | `889d974` (byte-verified; `git describe --tags --exact-match` == `v9.7.10`) |
| Registry integrity | `sha512-oX17nauJcPTaKIrRFY/rD0Rl8atqFUVv9Hg2TKH+A68/fC8+ZO344Mkd1A/Y0oOVp1hr5tktMBjzMEDDnMEYUw==` |
| Recorded `gitHead` | `1e268895` (lags tarball content by one release; it is the `v9.7.9` tag commit; do not treat it as the content commit) |
| License assumption | GPL-3.0-only (recorded in `compatibility/oracle/oracle-metadata.json`; an engineering assumption, not a legal conclusion; see [`docs/licensing.md`](../licensing.md)) |
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
| `workshop-rs` catalog/emission | Canonical en-US spelling validation against oracle-emitted Workshop text; receiver-method and enum emission evidence |
| `compatibility/` harness | Fixture snapshots, oracle identity blocks, S/D/N gate evidence |
| Systematic baseline | Reference-validated probes for builtin action/value/member/enum/signature metadata — implemented as the OPY semantic compatibility manifest (`crates/wright-opy/src/manifest/`): every entry records the probe that validates it, and `probes/validate.py` runs the full probe set against the pinned oracle (accept/reject, normalized emission hash, diagnostic category; wired into `compatibility/tests`) |

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
  OverPy compatibility evidence set is byte-identical across `9.7.10 → 9.7.13`;
  only hero/settings schema data differs. Historical claims stay interpretable
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

## OSTW reference

Facts below were verified against the upstream repository, wiki, and release
artifacts on 2026-08-15 (investigation #113, pinned in #115). The reference is
exercised by the oracle harness (`compatibility/ostw/run_oracle.py`).

### Identity

| Field | Value |
| --- | --- |
| Project | OSTW: Overwatch Script To Workshop (a.k.a. Deltin's Script To Workshop) |
| Repository | <https://github.com/ItsDeltin/Overwatch-Script-To-Workshop> |
| Compiler | C# / .NET 8.0, project `Deltinteger` (namespace `Deltin`), executable `Deltinteger` |
| Default branch | `master`; investigation HEAD `817c1db4` (2026-08-08); 2351 commits, active (≈28 commits in the prior two months) |
| Stable release tags | `v3.4.0` (2026-05-18), `v3.3.1`, `v3.3.0`, `v3.2.3`, `v3.2.2`, `v3.2.1`, `v3.2.0`, `v3.1.1`, `v3.1.0`, … |
| Rolling tag | `latest` (a prerelease "Master Build" rebuilt on every `master` push: win-x64/win-x86/linux-x64 self-contained zips; **never a content pin**) |
| Version constant | `Program.VERSION == "v3.4.0"` still at master HEAD (lags master content; master is ≈2 months ahead of tag `v3.4.0`) |
| Pinned reference | stable tag `v3.4.0` (content-pinned: git tag plus release-asset hashes), consistent with ADR-0007 (version-exact, content-pinned, changed only on demonstrated behavioral need; `latest`/master are not pins) |
| License assumption | Compiler and wiki: **no license file and none in git history** (GitHub API `license: null`); treated as unlicensed / all-rights-reserved for source copying; the extension subdirectory (`overwatch-script-to-workshop/LICENSE`) is MIT (Copyright 2026 ItsDeltin). Engineering assumption, not a legal conclusion; see [`docs/licensing.md`](../licensing.md) |
| Workshop-data provenance | `Elements.json` (265 values, 224 actions, 51 enumerators), `LobbySettings.json`, `Maps.json` are generated from a local Overwatch install via the in-repo `DataTool`; Blizzard-IP-adjacent, not to be imported into Wright's catalog |
| Output languages | en-US default; 13 Workshop locales (`enUS` + 12) via `Elements/OutputLanguage` and `Languages/i18n-*.xml` |

### Oracle role

OSTW is the compatibility **oracle and behavior reference** for Wright's
OSTW frontend (`wright-ostw`), per [`docs/compatibility.md`](../compatibility.md) and the
extension of ADR-0007 pinning policy to a second reference. It is not a
production runtime dependency of the Wright core and is never bundled into
release artifacts. Concretely it serves as:

* the reference for S (syntax), D (diagnostic), and N (normalized-output)
  evidence for the OSTW corpus (`compatibility/ostw/`, with oracle runner
  `compatibility/ostw/run_oracle.py`);
* the reference for the Workshop → OSTW reconstruction surface: the upstream
  `Decompiler` (`TextToElement` workshop-text parser + `ElementToCode`
  `WorkshopDecompiler`) defines the reconstructible surface and its idiomatic
  OSTW naming, compared under a versioned normalizer (never byte-identical
  output);
* the source of behavior probes for the OSTW compatibility baseline
  ([`docs/ostw/compatibility-baseline.md`](../ostw/compatibility-baseline.md)).

### Wright surfaces that use it as reference

| Wright surface | Use of the reference |
| --- | --- |
| `wright-ostw` native frontend | Accept/reject agreement, structured diagnostics, HIR semantic identity for the declared OSTW surface |
| `workshop-rs` emitter/catalog | Canonical en-US emission cross-check against the oracle's `workshopCode` output for shared Workshop surfaces |
| Workshop → OSTW reconstruction (future) | Reference decompiler output for the declared reconstruction surface and quality criteria |
| `compatibility/` harness | OSTW fixture snapshots, oracle identity blocks, S/D/N gate evidence |

### Reference semantics vs Wright-owned architecture

Studying the reference informs Wright's compatibility semantics; it does not
define Wright's implementation. Per ADR-0004 and [`docs/licensing.md`](../licensing.md),
the Wright core:

* never links to, copies source from, or imports internal types of OSTW
  (the compiler source is unlicensed, so copying is additionally restricted);
* never imports OSTW's game-derived data files (`Elements.json` and related);
  the Wright-owned Workshop catalog remains the canonical Workshop identity;
* keeps HIR, Workshop IR, diagnostics, and backend APIs Wright-owned;
* treats observed reference behavior (through documented compatibility tests
  and the oracle boundary) as a permitted input, not as permission to copy an
  implementation.

The proposed OSTW baseline
([`docs/ostw/compatibility-baseline.md`](../ostw/compatibility-baseline.md))
follows this boundary: entries are Wright-authored and oracle-validated, never
mechanically extracted from OSTW source or data files.

### Durable reference limitations

* **Pinning policy.** The proposed pin is the stable tag `v3.4.0`, content-pinned
  per ADR-0007, changed only on demonstrated behavioral need. The rolling
  `latest` tag is rebuilt on every push and is not a stable identity; master
  advances faster than stable tags (≈28 commits / 2 months at investigation
  time), so a pin change must be an explicit, evidence-backed decision.
* **Unlicensed compiler source.** No license file exists in the repository or
  its history for the compiler; the wiki is also unlicensed. Source may be
  read for behavior but must not be copied or redistributed; only the VS Code
  extension subdirectory is MIT.
* **Headless execution.** The default CLI compile path is interactive
  (waits for Enter, copies to clipboard, fails headless without `xclip`);
  `out_file` in `ds.toml` is honored only on the workspace/LSP path. The
  headless oracle paths are the stdio language server (`--langserver`; custom
  `workshopCode`/`elementCount` notifications plus `publishDiagnostics`) and
  the browser-WASM interop (`OstwJavascript`, built with `dotnet publish -r
  browser-wasm`; not shipped in release assets).
* **Platform assets.** Release binaries are x64-only (win-x64/win-x86/
  linux-x64 self-contained, plus framework-dependent zips needing the .NET 8
  runtime); there are no macOS/arm64 builds. Verified: the linux-x64
  self-contained build runs under linux/amd64 container emulation on an arm64
  host and answers `--ping`.
* **Emulator scope.** The upstream emulator (`Emulator/`) ticks Workshop rules
  (players, variables, arrays) and is the upstream's own behavioral oracle in
  `Deltinteger.Tests`, but it is partial — e.g. `Wait` raises
  `NotImplementedException`. E-level claims must state the emulator subset.
* **Reconstruction boundary.** The upstream decompiler reconstructs rules,
  actions, conditions, values, and lobby-settings imports; it does not recover
  original comments, formatting, macros, or abstractions. N-level comparison
  of reconstruction output requires a versioned normalizer.
* **Data newer than the pin.** Workshop data (heroes, lobby settings, maps)
  is game-derived and updated per release; content newer than the pin is
  unavailable to fixtures until a demonstrated need triggers the upgrade.

## Related decisions and documents

* [ADR-0007: OverPy reference pinning policy](../adr/0007-reference-pinning-policy.md)
* [ADR-0004: OverPy licensing and clean-room boundary](../adr/0004-overpy-licensing-boundary.md)
* [ADR-0002: Compatibility strategy](../adr/0002-compatibility-strategy.md)
* [`docs/compatibility.md`](../compatibility.md): S/D/N/E framework and reference boundary
* [`docs/licensing.md`](../licensing.md): component policy and permitted inputs
* [`docs/opy/compatibility-baseline.md`](../opy/compatibility-baseline.md): tiered baseline built on this reference
* [`docs/opy/compat-manifest-spec.md`](../opy/compat-manifest-spec.md): machine-readable manifest specification
