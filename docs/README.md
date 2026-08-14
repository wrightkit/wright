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
Root-level files provide concise, standard navigation entry points:
- [`README.md`](../README.md) — Public product overview, feature summary, and quick start.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md) — Contributor onboarding, toolchain policy, and check workflows.
- [`AGENTS.md`](../AGENTS.md) — Agent routing rules, architectural boundaries, and task protocols.
- [`LICENSE`](../LICENSE) — Canonical GNU AGPL v3.0 license text.

### 2. Living Durable Contracts & Reference Docs
Maintained specifications describing Wright as it currently exists. When a
feature or contract evolves, its living document is updated directly:
- **Architecture**: [`architecture.md`](architecture.md) — Component responsibilities, IR boundaries, dependency rules.
- **Compatibility**: [`compatibility.md`](compatibility.md) — S/D/N/E compatibility levels, gate criteria, priority order.
- **Compatibility Matrix**: [`v1-matrix.md`](v1-matrix.md) — Frozen input surfaces, release gate definitions, intentional differences.
- **Licensing & Clean-Room Boundary**: [`licensing.md`](licensing.md) — Independent implementation policy and external reference isolation.
- **CLI & Driver Contract**: [`cli.md`](cli.md) — Commands, exit codes, stdio ownership, `wright-result/v1` envelope.
- **Embedding & Tool API**: [`embedding.md`](embedding.md) — Programmatic session API, `ToolService`, safe source refactoring.
- **Language Services & LSP**: [`language-services.md`](language-services.md) — Editor-neutral analysis, responsiveness contract, `wright-lsp`.
- **Release & Distribution**: [`release.md`](release.md) — Binary packaging, checksum verification, GitHub Releases automation.
- **Agent Team Coordination**: [`agent-team.md`](agent-team.md) — Multi-role governance, authority ordering, and blocked states.
- **Language Support Matrices**:
  - [`.opy` Support Matrix](opy/support-matrix.md) — Supported syntax, declarations, expressions, settings, diagnostics.
  - [Workshop Support Matrix](workshop/support-matrix.md) — Evidenced actions, values, events, enums, localized catalog.
- **Protocol & Pipeline Specifications**:
  - [Opy HIR v1 Protocol](hir/opy-hir-v1.md) — JSON schema for the `.opy` frontend HIR interchange boundary.
  - [Workshop Catalog Data Pipeline](workshop/catalog-pipeline.md) — Canonical catalog generation, localization data, validation.

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
| **Licensing Policy** | [`licensing.md`](licensing.md) | Clean-room development boundaries, third-party reference isolation. |
| **CLI & Driver API** | [`cli.md`](cli.md) | Command syntax, exit codes, machine-readable JSON envelopes. |
| **Library Embedding** | [`embedding.md`](embedding.md) | Rust embedding API, session-aware query service, AST-safe refactoring. |
| **Editor Services & LSP** | [`language-services.md`](language-services.md) | Hover, definitions, references, rename, semantic tokens, LSP framing. |
| **Release & Packaging** | [`release.md`](release.md) | Platform targets, packaging scripts, automated release validation. |
| **Governance & Process** | [`agent-team.md`](agent-team.md) | Role authority (PM / Architect / Engineer / QA), spec schemas, blocked routes. |
| **OPY Language** | [`opy/support-matrix.md`](opy/support-matrix.md) | Native Rust `.opy` frontend syntax, preprocessing, resolution, settings. |
| **Workshop Language** | [`workshop/support-matrix.md`](workshop/support-matrix.md) | Workshop text parsing, catalog-backed emission, localized enums. |
| **HIR Protocol** | [`hir/opy-hir-v1.md`](hir/opy-hir-v1.md) | Typed JSON AST/HIR representation for `.opy` programs. |
| **Catalog Pipeline** | [`workshop/catalog-pipeline.md`](workshop/catalog-pipeline.md) | Canonical localization data compilation and schema validation. |
| **Architecture History** | [`adr/README.md`](adr/README.md) | Chronological index of accepted architecture decision records. |
