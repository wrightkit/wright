# Wright

[![CI Status](https://github.com/wrightkit/wright/actions/workflows/ci.yml/badge.svg)](https://github.com/wrightkit/wright/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.0-blue.svg)](rust-toolchain.toml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/wrightkit/wright?include_prereleases)](https://github.com/wrightkit/wright/releases)

Wright is the unified developer CLI and language tooling layer for Overwatch
Workshop development in WrightKit. It provides diagnostics, analysis, code
editing, and language server support across raw Workshop, OverPy, and
provider-backed DEL/OSTW projects. DEL/OSTW entrypoints are recognized, but
their Wright workflow requires a configured source provider.

Wright delegates source parsing and semantic lowering to dedicated WrightKit
engines instead of reimplementing them:

- [`workshop-rs`](https://github.com/wrightkit/workshop-rs): canonical Workshop
  semantics, WIR, and catalog;
- [`opy-rs`](https://github.com/wrightkit/opy-rs): standalone OverPy frontend and
  compiler;
- [`del-rs`](https://github.com/wrightkit/del-rs): standalone DEL and OSTW
  frontend, consumed by Wright through a provider boundary.

Wright coordinates these implementations to deliver consistent developer tooling
across languages.

```text
                    Wright
       unified tooling / orchestration
 check · lint · analyze · inspect · edit
       agent · CI · LSP · embedding
                      │
        ┌─────────────┼─────────────┐
        ▼             ▼             ▼
     opy-rs       provider     workshop-rs
     OverPy      boundary       Workshop
 implementation  DEL/OSTW    implementation
        │             │             ▲
        └─────────────┴─────────────┘
              canonical Workshop
```

## Core focus

Wright prioritizes developer tooling over compiler reimplementation:

- deterministic `check` diagnostics;
- lint rules and complexity analysis;
- symbol inspection and semantic queries;
- safe source rewrites and syntax-preserving refactoring;
- agent integrations and embedding APIs;
- machine-readable CI output;
- server stability checks and complexity analysis;
- compilation and format conversion when supported by the underlying engine.

Compilation remains essential, but Wright does not invent surrogate syntax or
simulate missing language features. When a language capability is missing, it
belongs in the owning engine.

## Current compatibility

Wright's product claims are bounded by the current state of the owning
implementations. A command existing in Wright does not by itself mean the
underlying language surface is complete.

| Source form | Owning implementation | Current WrightKit status |
| --- | --- | --- |
| Raw Workshop | `workshop-rs` | ✅ Canonical parsing/WIR/validation/emission baseline is available |
| OverPy (`.opy`) | `opy-rs` | 🟡 Standalone source analysis exists; builtin/member/catalog breadth and end-to-end compilation are still being closed |
| DEL / OSTW (`.del`, `.ostw`) | `del-rs` via provider | ⚪ Recognized by Wright, but unavailable until a source provider is configured; no static DEL dependency |

Workshop → OPY and Workshop → DEL reconstruction are not treated as supported
WrightKit capabilities until the owning language implementations provide and
evidence those reconstruction paths.

Compatibility targets observable semantics, not compiler-output identity,
formatting, temporary variables, optimizer shape, or internal architecture.

## Why Wright exists alongside standalone implementations

Standalone engines focus on parsing and lowering for a single syntax. Wright
solves developer workflow needs across formats:

- consistent commands for checking, linting, and inspecting source files;
- shared performance, stability, and syntax checks that run against semantic models;
- atomic, syntax-preserving refactoring;
- unified interfaces for autonomous agents and external tooling;
- consistent JSON reports and CI annotations;
- editor-neutral language server protocols (LSP);
- coordination across language frontends and canonical Workshop data.

An LPP provider is an integration role that an implementation may expose to
Wright. Neither `opy-rs` nor `del-rs` depends on Wright internals.

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

Commands reflect product targets. When a backing engine encounters unsupported
syntax, Wright reports the missing feature cleanly rather than failing silently
or falling back to unverified runtimes.

Machine-readable workflows use the documented JSON contracts, for example:

```sh
wright lint input.opy --format json
```

## How it works

Wright consumes the owning implementation for each source form rather than
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
