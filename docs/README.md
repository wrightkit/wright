# Wright Documentation

This directory contains Wright's durable product/integration contracts and
supporting references. Keep architecture intent, decision history, current
implementation reality, and mutable execution state separate.

## Documentation model

```text
architecture/README.md       current Wright architecture routing
  ├─ ownership.md            product/repository ownership + dependency direction
  ├─ integration.md          language/provider integration + failure routing
  └─ tooling.md              Wright-owned tooling/edit/agent/CI/LSP model
cli.md / embedding.md / ...  focused public/product contracts
compatibility.md             compatibility methodology
adr/                         point-in-time decisions and rationale
source/tests/CI/integrations current implementation reality
Issues / PRs / releases      mutable execution state
```

For substantive implementation work, start from
[`architecture/README.md`](architecture/README.md), resolve the smallest
relevant current contract, then inspect the affected code/tests/owner and the
Issue contract.

## Current architecture

- [Architecture routing](architecture/README.md)
- [Ownership and dependency contract](architecture/ownership.md)
- [Language/provider integration contract](architecture/integration.md)
- [Wright tooling contract](architecture/tooling.md)
- [`architecture.md`](architecture.md) is retained as a compatibility pointer
  for older links, not a second current architecture authority.

## Focused product contracts

- [CLI & driver](cli.md): commands, exit behavior, and machine-readable result
  contracts.
- [Embedding/tool API](embedding.md): programmatic session/query/edit services.
- [Language services & LSP](language-services.md): editor-neutral language
  services and LSP framing.
- [Compatibility methodology](compatibility.md): compatibility/evidence levels
  and evaluation rules.
- [Licensing/provenance boundary](licensing.md): third-party reference and
  licensing constraints.
- [Release/distribution](release.md): packaging and publication contract.
- [Agent-team governance](agent-team.md): role/authority coordination where
  still applicable.

## Language implementation evidence

Wright is not the durable owner of OPY, DEL/OSTW, or raw Workshop semantics.
Current language support claims must come from the owning repositories:

- `wrightkit/opy-rs` for OverPy syntax/semantics/compiler/reconstruction;
- `wrightkit/deltin-rs` for DEL/OSTW project/type/runtime/compiler/reconstruction;
- `wrightkit/workshop-rs` for canonical raw Workshop/WIR/catalog/settings/
  localization/validation/emission.

Historical Wright-side OPY/OSTW/Workshop support matrices, compatibility
baselines, manifests, HIR migration documents, and catalog-pipeline notes may
remain useful migration/reference evidence while consumers still exist, but
they do not override the current owner contracts or upgrade current support.
Do not create new source-language semantic truth in Wright to keep those
historical assets current.

## Architecture decision history

[`docs/adr/`](adr/README.md) preserves point-in-time decisions and rationale.
Accepted ADRs are historical decision records; they are not proof that current
code still implements the decision. Materially changed architecture is
represented in current contracts and, when needed, a new/superseding ADR rather
than retroactively rewriting history.

## Current reality and execution state

Source, Cargo metadata, tests, CI, provider integrations, releases, and
real-project workflows establish current implementation reality. GitHub Issues
and PRs carry scope, acceptance, sequencing, and transient progress.

Do not maintain feature counts, versions, migration progress, PR state, or
roadmap snapshots in durable architecture documents.

## Repository entry points

- [`README.md`](../README.md): public product overview and quick start.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md): contributor onboarding and checks.
- [`AGENTS.md`](../AGENTS.md): implementation routing, ownership, verification,
  and delivery rules.
- [`LICENSE`](../LICENSE): repository license.
