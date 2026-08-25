# Wright Documentation

Welcome to the Wright documentation. This directory contains the authoritative
product, architecture, compatibility, API, and development contracts for the
Wright platform.

## Documentation Model and Authority

Wright organizes documentation by durable contract rather than historical
milestone. Documents belong to one of the following classes:

```text
Authority Hierarchy:
  GitHub Issues (Product scope, roadmap, acceptance)
    └─ ADRs (Accepted architecture decisions & historical rationale)
        └─ Active Specs (Cross-role coordination for in-progress work)
            └─ Living Reference Contracts (Normative API, CLI, language, & process docs)
                └─ Implementation & Executable Evidence (Code, tests, fixtures, CI)
```

### 1. Repository Entry Points
Root-level files provide concise navigation entry points:
- [`README.md`](../README.md): Public product overview, feature summary, ecosystem table, and quick start.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md): Contributor onboarding, toolchain policy, and check workflows.
- [`AGENTS.md`](../AGENTS.md): Agent routing rules, architectural boundaries, and task protocols.
- [`LICENSE`](../LICENSE): Canonical GNU AGPL v3.0 license text.

### 2. Living Durable Contracts & Reference Docs
Maintained specifications describing Wright as it currently exists. When a
feature or contract evolves, its living document is updated directly:
- **Architecture**: [`architecture.md`](architecture.md): Component responsibilities, IR boundaries, dependency rules.
- **Compatibility**: [`compatibility.md`](compatibility.md): S/D/N/E compatibility levels, gate criteria, priority order.
- **Compatibility Matrix**: [`v1-matrix.md`](v1-matrix.md): Frozen input surfaces, release gate definitions, intentional differences.
- **Licensing & Clean-Room Boundary**: [`licensing.md`](licensing.md): Independent implementation policy and external reference isolation.
- **CLI & Driver Contract**: [`cli.md`](cli.md): Commands, exit codes, stdio ownership, `wright-result/v1` envelope.
- **Embedding & Tool API**: [`embedding.md`](embedding.md): Programmatic session API, `ToolService`, safe source refactoring.
- **Language Services & LSP**: [`language-services.md`](language-services.md): Editor-neutral analysis, responsiveness contract, `wright-lsp`.
- **Release & Distribution**: [`release.md`](release.md): Binary packaging, checksum verification, GitHub Releases automation.
- **Agent Team Coordination**: [`agent-team.md`](agent-team.md): Multi-role governance, authority ordering, and blocked states.
- **Compatibility Planning & Reference**:
  - [Upstream / Reference Inventory](compatibility/upstream-references.md): Centralized pinned-reference provenance (OverPy 9.7.10, OSTW v3.4.0).
  - [Proactive OPY Compatibility Baseline](opy/compatibility-baseline.md): Tiered inventory of the OPY surface by semantic category and support dimension.
  - [Proactive OSTW Compatibility Baseline](ostw/compatibility-baseline.md): OSTW reference inventory, tiered surface, corpus plan, and M13 milestones.
  - [OPY Semantic Compatibility Manifest Spec](opy/compat-manifest-spec.md): Machine-readable compatibility contract for builtins, signatures, enums, aliases.
- **Language Support Matrices**:
  - [`.opy` Support Matrix](opy/support-matrix.md): Supported syntax, declarations, expressions, settings, diagnostics.
  - [Workshop Support Matrix](workshop/support-matrix.md): Evidenced actions, values, events, enums, localized catalog.
- **Protocol & Pipeline Specifications**:
  - [Opy HIR v1 Protocol](hir/opy-hir-v1.md): JSON schema for the `.opy` frontend HIR interchange boundary.
  - [Workshop Catalog Data Pipeline](workshop/catalog-pipeline.md): Canonical catalog generation, localization data, validation.

### 3. Architecture Decision Records (ADRs)
[`docs/adr/`](adr/README.md) captures point-in-time architecture decisions, their
context, and accepted consequences. Accepted ADRs are historical records; they
are not edited retroactively to hide past decisions. When a decision changes
(e.g., ADR-0008 rebaselining ADR-0001), a new ADR is adopted and links to its
predecessor.

### 4. Active Feature Specs
Active specifications live under [`docs/specs/`](specs/) during development to
coordinate multi-role implementation (PM, Architect, Engineer, QA). Specs use
the `wright-spec/v1` schema with stable requirement IDs (`REQ-*`).

**Lifecycle**: Once an issue is completed and verified:
1. Any durable interface or semantic contract is integrated into the
   appropriate living reference documents or ADRs.
2. The spec may be retired or removed from `main`, with Git history and the
   closing GitHub issue preserving the full execution record.

### 5. Execution Evidence and QA Reports
Point-in-time verification reports, milestone pilot summaries, intermediate
inventories, and test logs are execution evidence. They belong in GitHub issues,
pull requests, CI artifacts, and Git history. They do not accumulate as living
documents in `main`.

---

## Authority and Contract Map

| Contract Class | Primary Document | Normative Scope |
| --- | --- | --- |
| **System Architecture** | [`architecture.md`](architecture.md) | Component responsibilities, dependency direction, semantic model boundaries. |
| **Compatibility Framework** | [`compatibility.md`](compatibility.md) | S/D/N/E levels, evaluation methodology, oracle boundaries. |
| **Upstream References** | [`compatibility/upstream-references.md`](compatibility/upstream-references.md) | Pinned reference identity, license/provenance, oracle role, reference limitations. |
| **Licensing Policy** | [`licensing.md`](licensing.md) | Clean-room development boundaries, third-party reference isolation. |
| **CLI & Driver API** | [`cli.md`](cli.md) | Command syntax, exit codes, machine-readable JSON envelopes. |
| **Library Embedding** | [`embedding.md`](embedding.md) | Rust embedding API, session-aware query service, AST-safe refactoring. |
| **Editor Services & LSP** | [`language-services.md`](language-services.md) | Hover, definitions, references, rename, semantic tokens, LSP framing. |
| **Release & Packaging** | [`release.md`](release.md) | Platform targets, packaging scripts, automated release validation. |
| **Governance & Process** | [`agent-team.md`](agent-team.md) | Role authority (PM / Architect / Engineer / QA), spec schemas, blocked routes. |
| **OPY Language** | [`opy/support-matrix.md`](opy/support-matrix.md) | Native Rust `.opy` frontend syntax, preprocessing, resolution, settings. |
| **OSTW Language** | [`ostw/support-matrix.md`](ostw/support-matrix.md) | Declared OSTW → Workshop compile surface (#119): accepted targets, lowering decisions, declared normalization and divergences, boundaries. |
| **OPY Baseline** | [`opy/compatibility-baseline.md`](opy/compatibility-baseline.md) | Tiered forward-looking OPY compatibility inventory and residual evidence. |
| **OPY Manifest** | [`opy/compat-manifest-spec.md`](opy/compat-manifest-spec.md) | Machine-readable OPY semantic compatibility manifest specification. |
| **Workshop Language** | [`workshop/support-matrix.md`](workshop/support-matrix.md) | Workshop text parsing, catalog-backed emission, localized enums. |
| **HIR Protocol** | [`hir/opy-hir-v1.md`](hir/opy-hir-v1.md) | Typed JSON AST/HIR representation for `.opy` programs. |
| **Catalog Pipeline** | [`workshop/catalog-pipeline.md`](workshop/catalog-pipeline.md) | Canonical localization data compilation and schema validation. |
| **Architecture History** | [`adr/README.md`](adr/README.md) | Chronological index of accepted architecture decision records. |

---

## Document Asset Inventory and Action Ledger

This ledger records the classification and lifecycle action (`keep`, `update`,
`move`, `merge`, `replace`, `delete`) for every maintained Markdown document,
JSON document asset, and historical artifact across the repository.

### 1. Markdown Documentation Assets

| Asset Path | Document Class | Action | Description & Rationale |
| --- | --- | --- | --- |
| `README.md` | Repository Entry Point | `update` | Refreshed as the public product entry point with feature summaries, ecosystem compatibility table, installation, and quick start. |
| `CONTRIBUTING.md` | Repository Entry Point | `update` | Updated relative contract links to `docs/` (`docs/architecture.md`, `docs/compatibility.md`, `docs/licensing.md`); maintains toolchain & validation policy. |
| `AGENTS.md` | Repository Entry Point | `update` | Updated task routing links to `docs/`; defines cross-role boundaries and verified extension paths. |
| `docs/README.md` | Documentation Index & Governance | `keep` | Central documentation information architecture, authority hierarchy, contract map, and full asset inventory. |
| `docs/architecture.md` (was `ARCHITECTURE.md`) | Living Durable Contract | `move` + `update` | Moved to `docs/`; updated relative links and reframed as living tooling-first architecture baseline (ADR-0008). |
| `docs/compatibility.md` (was `COMPATIBILITY.md`) | Living Durable Contract | `move` + `update` | Moved to `docs/`; updated relative links and codified the S/D/N/E compatibility framework and priority order. |
| `docs/licensing.md` (was `LICENSE-BOUNDARY.md`) | Living Durable Contract | `move` + `update` | Moved to `docs/`; updated relative links and formalized clean-room development and oracle isolation policies. |
| `docs/cli.md` | Living Durable Contract | `update` | Removed stale M6 milestone framing; updated lint configuration and `wright-result/v1` envelope contracts. |
| `docs/embedding.md` | Living Durable Contract | `update` | Removed stale M9 milestone framing; updated `ToolService` operations and session-aware embedding contracts. |
| `docs/language-services.md` | Living Durable Contract | `update` | Removed stale M10 milestone framing; updated editor-neutral responsiveness and LSP adapter contracts. |
| `docs/release.md` | Living Durable Contract | `update` | Removed stale M8 framing; documents release automation, checksum verification, packaging contracts, and supported installation channels. |
| `docs/v1-matrix.md` | Living Durable Contract | `update` | Reconciled custom-game-settings support; documents release gates, supported surfaces, and intentional differences. |
| `docs/agent-team.md` | Living Durable Contract | `update` | Updated links; folded in pilot architectural findings (single-level subagents, non-authoritative router) and spec lifecycle. |
| `docs/opy/support-matrix.md` | Living Durable Contract | `update` | Removed stale M7 framing; updated native `.opy` parser, settings, and preprocessing supported surfaces. |
| `docs/workshop/support-matrix.md` | Living Durable Contract | `update` | Removed stale M5 framing; updated evidenced Workshop syntax, enums, and localization contracts. |
| `docs/workshop/catalog-pipeline.md` | Living Durable Contract | `update` | Removed milestone framing; documents deterministic catalog generation and schema validation pipeline. |
| `docs/compatibility/upstream-references.md` | Living Durable Contract | `update` | Centralized upstream/reference inventory per #106/#113: pinned OverPy 9.7.10 identity and durable OSTW reference entry (repository, proposed `v3.4.0` pin, unlicensed-compiler/MIT-extension license facts, oracle paths, limitations). |
| `docs/ostw/compatibility-baseline.md` | Living Durable Contract | `update` | OSTW reference/corpus/support investigation per #113: upstream architecture map, tiered semantic category inventory, corpus/acquisition plan (MOBAwatch, protect-ban), oracle feasibility report, architecture reuse/boundary findings, proposed bounded M13 decomposition; corrected per #122 to an explicit compile/document-root evidence model with reclassified historical observations and the #119 differential targets; M13 phase C marked implemented per #119 with the declared compile surface in the OSTW support matrix. |
| `docs/ostw/support-matrix.md` | Living Durable Contract | `new` | First declared OSTW → Workshop compile surface (#119): accepted differential targets (p4/p5/p6), lowering decisions, declared semantic normalization and divergences, deterministic rejection boundaries. |
| `docs/opy/compatibility-baseline.md` | Living Durable Contract | `new` | Tiered proactive OPY compatibility baseline per #106: semantic-category inventory, support dimensions, classified #104/#105 residual evidence. |
| `docs/opy/compat-manifest-spec.md` | Living Durable Contract | `new` | Specification for the machine-readable Wright-owned OPY semantic compatibility manifest (builtins, signatures, enum domains, aliases); implementation deferred to a bounded child issue. |
| `docs/hir/opy-hir-v1.md` | Living Protocol Specification | `update` | Updated relative links to `docs/`; documents Opy HIR v1 JSON protocol schema and versioning. |
| `docs/specs/SPEC-99-stability-rules.md` (was `docs/m12-issue99-spec.md`) | Active Feature Specification | `move` + `update` | Moved into dedicated `docs/specs/` directory; updated relative links; active spec for issue #99. |
| `docs/adr/README.md` | Architecture Decision Index | `update` | Updated relative links to `docs/architecture.md` and `docs/compatibility.md`; indexes ADR 0000–0010. |
| `docs/adr/0000-template.md` | Architecture Decision Record | `keep` | Standard ADR template for proposing new architectural decisions. |
| `docs/adr/0001-project-scope.md` | Architecture Decision Record | `update` | Updated link to `docs/compatibility.md`; recorded as superseded by ADR-0008. |
| `docs/adr/0002-compatibility-strategy.md` | Architecture Decision Record | `update` | Updated link to `docs/compatibility.md`; records S/D/N/E strategy (amended by ADR-0008). |
| `docs/adr/0003-ir-boundary.md` | Architecture Decision Record | `update` | Updated link to `docs/architecture.md`; records two-layer HIR/WIR boundary. |
| `docs/adr/0004-overpy-licensing-boundary.md` | Architecture Decision Record | `update` | Updated links to `docs/licensing.md` and `docs/compatibility.md`; records clean-room policy. |
| `docs/adr/0005-opy-hir-v1.md` | Architecture Decision Record | `update` | Updated link to `docs/compatibility.md`; records initial HIR v1 interchange decision. |
| `docs/adr/0006-rust-ir-core.md` | Architecture Decision Record | `keep` | Records typed IDs, arenas, and two-layer IR model decisions. |
| `docs/adr/0007-reference-pinning-policy.md` | Architecture Decision Record | `update` | Updated links to `docs/compatibility.md` and issue #82; records version-exact content-pinned oracle policy. |
| `docs/adr/0008-tooling-first-semantic-platform.md` | Architecture Decision Record | `update` | Updated links to `docs/architecture.md` and `docs/compatibility.md`; establishes tooling-first platform baseline. |
| `docs/adr/0009-language-ownership-licensing-boundaries.md` | Architecture Decision Record | `new` | Accepted multi-repository language ownership, LPP process boundary, and provider-specific provenance/licensing strategy; amends ADR-0008 frontend-ownership wording per #136. |
| `adapter/README.md` | Subsystem Contract | `update` | Updated link to `docs/licensing.md`; documents external OverPy adapter boundary and JSON invocation. |
| `compatibility/README.md` | Subsystem Contract | `update` | Updated link to `docs/compatibility.md`; documents oracle setup, fixture layout, and differential diff runner. |
| `dist/README.md` | Subsystem Contract | `keep` | Documents package-manager distribution metadata (install.sh, Homebrew, WinGet, Scoop), generation, publication process, and drift detection. |
| `crates/wright-analyzer/tests/fixtures/README.md` | Subsystem Contract | `keep` | Documents test fixture provenance and regeneration process for analyzer test fixtures. |

### 2. JSON Schema, Catalogs & Configuration Assets

| Asset Path | Document Class | Action | Description & Rationale |
| --- | --- | --- | --- |
| `workshop-rs` catalog data (`crates/workshop-rs/src/catalog/data/catalog.json`, external repo, pinned rev in root `Cargo.toml`) | Catalog Data Asset (external owner) | `keep` | Canonical localized Workshop catalog data (actions, values, enums, events, keywords) with recorded provenance; owned by `workshop-rs` per ADR-0009/wright#143, consumed by Wright directly. |
| `compatibility/oracle/oracle-metadata.json` | Reference Metadata Asset | `keep` | Records pinned `overpy@9.7.10` tarball integrity, gitHead commit, registry URL, and license assumption. |
| `compatibility/oracle/package.json` | Tooling Package Manifest | `keep` | Pinned npm package manifest for installing external OverPy oracle during compatibility evaluation. |
| `adapter/package.json` | Tooling Package Manifest | `keep` | Pinned npm package manifest for executing the isolated OverPy adapter bridge. |
| `scripts/corpus-manifest.json` | Provenance Manifest Asset | `keep` | SHA-256 integrity manifest for all real-world corpus projects and full include closures. |

### 3. Fixture, Test & Scenario JSON Assets

| Asset Path | Document Class | Action | Description & Rationale |
| --- | --- | --- | --- |
| `compatibility/fixtures/**/fixture.json` (17 files) | Fixture Manifest Asset | `keep` | Metadata manifests (schema v1) for 6 synthetic and 11 real-world compatibility fixtures defining expected compile status and provenance. |
| `compatibility/fixtures/**/oracle.json` (17 files) | Test Snapshot Asset | `keep` | Normalized reference compiler snapshots from pinned `overpy@9.7.10` for differential output testing. |
| `adapter/fixtures/**/*.json` (10 files) | Adapter Test Snapshot Asset | `keep` | Pinned Opy HIR v1 protocol snapshots generated by the adapter bridge for differential frontend testing. |
| `adapter/test/fixtures/*.json` (3 files) | Adapter Test Fixture Asset | `keep` | Unit test expectations for adapter constants, macros, and settings handling. |
| `crates/wright-analyzer/tests/fixtures/*.json` (11 files) | Analyzer Test Fixture Asset | `keep` | Pinned semantic analysis test payloads for lint rule validation (`duplicate-condition`, `expensive-loop`, `repeated-value-*`, `while-without-wait-*`, and the issue #103 `no-yield-*` boundedness fixtures). |
| `scenarios/*.json` (7 files) | E-Level Scenario Manifest Asset | `keep` | Executable semantic scenario manifests (`arrays.json`, `control-flow.json`, `events.json`, `loops.json`, `subroutines.json`, `variables.json`, `waits.json`) for `scripts/run-scenarios.py`. |

### 4. Trashed / Deleted Historical Artifacts

These superseded milestone snapshots, completed pilot reports, and intermediate
verification files were safely removed from `main` using `trash`. Their full
contents and execution evidence remain permanently preserved in Git history
and their referenced GitHub issues.

| Previous Asset Path | Document Class | Action | Rationale for Removal from `main` |
| --- | --- | --- | --- |
| `docs/agent-team-pilot-80.md` | Completed Pilot Report | `delete` | Grok agent-team pilot execution report; completed and recorded in issue #80. Durable conclusions folded into `docs/agent-team.md`. |
| `docs/agent-team-pilot-m10-acceptance.md` | Completed Pilot Report | `delete` | OpenCode agent-team pilot acceptance report; completed and recorded in issue #27. Durable conclusions folded into `docs/agent-team.md`. |
| `docs/m12-issue98-verification.md` | Completed QA Report | `delete` | Point-in-time QA verification report for issue #98 (`wright lint`); completed and permanently recorded in GitHub issue #98. |
| `docs/m12-issue99-evidence-review.md` | Point-in-Time Evidence Review | `delete` | Pre-spec evidence review for issue #99; superseded by `docs/specs/SPEC-99-stability-rules.md` and recorded in issue #99. |
| `docs/opy/m11-gap-inventory.json` | Milestone Snapshot Data | `delete` | Initial M11 parity gap inventory; superseded by ADR-0008 tooling-first rebaseline and `docs/v1-matrix.md`. |
| `docs/opy/m11-gap-inventory.md` | Milestone Snapshot Document | `delete` | Initial M11 parity gap inventory narrative; superseded by ADR-0008 and `docs/opy/support-matrix.md`. |
| `docs/opy/m11-inventory-reassessment.md` | Milestone Triage Document | `delete` | Mid-M11 reassessment report; superseded by ADR-0008. |
| `docs/opy/m11-inventory-rebaseline.md` | Milestone Triage Document | `delete` | Mid-M11 rebaseline proposal; superseded by ADR-0008. |
| `docs/opy/m11-inventory-post86.md` | Milestone Triage Document | `delete` | Post-issue #86 triage snapshot; superseded by ADR-0008. |
| `docs/opy/m11-inventory-final.md` | Milestone Triage Document | `delete` | Final M11 triage inventory; conclusions codified in ADR-0008 and `docs/v1-matrix.md`. |
| `docs/opy/m11-issue86-verification.md` | Completed Verification Report | `delete` | Verification report for settings implementation (issue #86); recorded in issue #86. |
| `docs/opy/m11-issue87-verification.md` | Completed Verification Report | `delete` | Verification report for arithmetic operators (issue #87); recorded in issue #87. |
| `docs/opy/m11-oracle-version-investigation.md` | Completed Investigation Report | `delete` | Track B oracle version investigation; conclusions codified in ADR-0007 and issue #82. |
| `docs/opy/m11-settings-boundary-investigation.md` | Completed Investigation Report | `delete` | Investigation on settings boundary; conclusions codified in issue #86 and `docs/opy/support-matrix.md`. |
| `docs/opy/m11-settings-design-constraints.md` | Design Investigation Notes | `delete` | Temporary design notes; implemented in `crates/wright-opy` and `docs/opy/support-matrix.md`. |
| `docs/opy/m11-settings-free-candidates.md` | Point-in-Time Triage Notes | `delete` | Candidate triage list; completed in issue #86. |
