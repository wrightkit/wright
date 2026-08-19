# Wright

[![CI Status](https://github.com/wrightkit/wright/actions/workflows/ci.yml/badge.svg)](https://github.com/wrightkit/wright/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.0-blue.svg)](rust-toolchain.toml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/wrightkit/wright?include_prereleases)](https://github.com/wrightkit/wright/releases)

Wright is WrightKit's unified tooling and integration product for Overwatch
Workshop development. It gives users one CLI, language-service, CI, embedding,
and agent-facing surface across raw Workshop, OverPy, and DEL/OSTW projects.

Wright does **not** own the complete implementation of those three source
forms. It depends on independently usable WrightKit implementations:

- [`workshop-rs`](https://github.com/wrightkit/workshop-rs) — raw Workshop
  implementation and canonical Workshop semantics/WIR/catalog;
- [`opy-rs`](https://github.com/wrightkit/opy-rs) — standalone OverPy
  implementation;
- [`del-rs`](https://github.com/wrightkit/del-rs) — standalone DEL/OSTW
  implementation.

Wright adds the integration layer and higher-level tooling that becomes more
valuable when those implementations expose complete, reliable semantic data.

```text
                    Wright
       unified tooling / orchestration
 check · lint · analyze · inspect · edit
       agent · CI · LSP · embedding
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
     opy-rs         del-rs      workshop-rs
     OverPy         DEL/OSTW      Workshop
 implementation  implementation  implementation
        │             │             ▲
        └─────────────┴─────────────┘
              canonical Workshop
```

## Product focus

Wright is tooling-first. The highest-value surfaces are:

- deterministic `check` diagnostics;
- lint and static analysis;
- semantic inspection and query;
- validated source edits and refactoring;
- agent and embedding APIs;
- CI presentation and machine-readable results;
- Workshop stability and cost analysis;
- compilation and conversion where the backing language implementation has
  evidence-backed support.

Compilation is important, but Wright should not duplicate source-language or
raw Workshop semantics to make a command appear supported. Missing language
capabilities belong in their owning implementation.

## Current compatibility

Wright's product claims are bounded by the current state of the owning
implementations. A command existing in Wright does not by itself mean the
underlying language surface is complete.

| Source form | Owning implementation | Current WrightKit status |
| --- | --- | --- |
| Raw Workshop | `workshop-rs` | ✅ Canonical parsing/WIR/validation/emission baseline is available |
| OverPy (`.opy`) | `opy-rs` | 🟡 Standalone source analysis exists; builtin/member/catalog breadth and end-to-end compilation are still being closed |
| DEL / OSTW (`.del`, `.ostw`) | `del-rs` | 🟡 Standalone parsing/semantic/HIR support is substantial; advanced runtime/project lowering and end-to-end compilation remain partial |

Workshop → OPY and Workshop → DEL reconstruction are not treated as supported
WrightKit capabilities until the owning language implementations provide and
evidence those reconstruction paths.

Compatibility targets observable semantics, not compiler-output identity,
formatting, temporary variables, optimizer shape, or internal architecture.

## Why Wright exists if the implementations are standalone

The standalone implementations solve language-specific problems. Wright solves
the cross-language product problem:

- one UX for checking, linting, analyzing, and inspecting different source forms;
- shared lint/stability/cost rules that operate on semantic information;
- source-edit transaction safety and semantic refactoring;
- agent-facing tools and embedding interfaces;
- CI output, GitHub Actions presentation, and stable machine-readable results;
- editor-neutral language services and LSP integration;
- orchestration across source-language and Workshop capabilities.

An LPP **provider** is an integration role that an implementation may expose to
Wright. It is not the identity of `opy-rs` or `del-rs`, and Wright must not make
those repositories depend on Wright tooling internals.

## Installation

### macOS

```sh
brew tap wrightkit/tap
brew install wrightkit/tap/wright
```

or:

```sh
curl -fsSL https://wrightkit.dev/install.sh | bash
```

### Linux

```sh
curl -fsSL https://wrightkit.dev/install.sh | bash
```

### Other distribution paths

Prebuilt release archives and currently supported package-manager channels are
documented in [`docs/release.md`](docs/release.md). Use the release-specific
documentation rather than assuming every package-manager submission is already
published.

### From source

```sh
cargo build --release -p wright-cli -p wright-lsp
```

## CLI

Core product commands include:

```sh
wright check input.opy
wright lint input.opy
wright analyze input.opy
wright inspect input.opy
wright compile input.opy
```

The command surface is broader than the currently complete semantic support.
When a backing implementation reports an unsupported construct, Wright should
surface that limitation explicitly rather than silently falling back to an
upstream runtime or claiming success.

Machine-readable workflows use the documented JSON contracts, for example:

```sh
wright lint input.opy --format json
```

## How it works

Wright should consume the owning implementation for the source form instead of
maintaining a second authoritative language implementation:

```text
source input
   ↓
workshop-rs / opy-rs / del-rs
   ↓
source-language semantic results and canonical Workshop contracts
   ↓
Wright tooling services
   ├─ check / diagnostics
   ├─ lint / analysis / semantic queries
   ├─ validated source edits
   ├─ agent / embedding / CI
   └─ compile / convert when supported
```

The source-language implementations may expose native Rust APIs and/or an LPP
provider process. LPP is a stable integration boundary, not a requirement that
standalone users route through Wright.

See [`docs/adr/0010-independent-implementations-and-wright-integration.md`](docs/adr/0010-independent-implementations-and-wright-integration.md)
for the durable terminology and ownership clarification.

## Development priorities

Real user workflows are the primary evidence. When `wright check`, `lint`, or
`analyze` fails on a real project:

1. reproduce the failure;
2. identify the owning layer;
3. fix source-language semantics in `opy-rs` / `del-rs`, canonical Workshop
   semantics in `workshop-rs`, or integration/tooling behavior in Wright;
4. keep the full-project regression and add a minimized test where practical;
5. do not substitute architecture cleanup or support-matrix bookkeeping for a
   usable workflow.

Architecture work remains important at public/versioned contracts, repository
ownership boundaries, provenance/source-edit correctness, and dependency
direction. Most internal structure is revisable implementation detail.

## Documentation

Architecture, compatibility methodology, APIs, release guidance, ADRs, and
maintainer references are indexed in [`docs/README.md`](docs/README.md).

## Contributing

Read [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md) before
making changes. Do not push directly to `main`; deliver implementation changes
through focused branches and PRs.

## License

Wright is currently distributed under the GNU Affero General Public License v3.0
or later. Third-party compatibility references and fixtures remain governed by
their recorded licenses and provenance; see [`docs/licensing.md`](docs/licensing.md).
