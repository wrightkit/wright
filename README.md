# Wright

[![CI Status](https://github.com/wrightkit/wright/actions/workflows/ci.yml/badge.svg)](https://github.com/wrightkit/wright/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.0-blue.svg)](rust-toolchain.toml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/wrightkit/wright?include_prereleases)](https://github.com/wrightkit/wright/releases)

Wright is the unified developer CLI and language tooling layer for Overwatch
Workshop development in WrightKit. It provides diagnostics, cost and risk
analysis, semantic queries, validated edits, and an agent session contract
across raw Workshop and provider-backed OverPy projects. DEL/OSTW entrypoints
are recognized, but DEL/OSTW provider support is not currently shipped with
Wright.

Wright delegates source parsing and semantic lowering to dedicated WrightKit
engines instead of reimplementing them:

- [`workshop-rs`](https://github.com/wrightkit/workshop-rs): canonical Workshop
  semantics, WIR, and catalog;
- [`opy-rs`](https://github.com/wrightkit/opy-rs): standalone OverPy frontend and
  compiler, consumed by Wright through an LPP provider;
- [`deltin-rs`](https://github.com/wrightkit/deltin-rs): standalone DEL and OSTW
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
- lint rules, plus `analyze` reports of Workshop cost, complexity hotspots, and
  server-load risk indicators, each labeled exact, static, or heuristic;
- symbol, reference, control-flow, call-graph, and cost queries under `inspect`;
- validated semantic renames that preview a source diff before writing;
- a versioned agent session contract (`wright serve`) and embedding APIs;
- machine-readable CI output with GitHub Actions annotations;
- compilation and Workshop → OPY conversion when supported by the underlying
  engine.

Compilation remains essential, but Wright does not invent surrogate syntax or
simulate missing language features. When a language capability is missing, it
belongs in the owning engine.

## Current compatibility

Wright's product claims are bounded by the current state of the owning
implementations. A command existing in Wright does not by itself mean the
underlying language surface is complete.

| Source form | Owning implementation | Current WrightKit status |
| --- | --- | --- |
| Raw Workshop | `workshop-rs` | ✅ Canonical parsing, WIR, validation, emission, analysis, and validated rename are available |
| OverPy (`.opy`) | `opy-rs` through LPP | 🟡 `check`, `compile`, `lint`, and `analyze` are provider-backed; language support remains bounded by the provider's published capabilities |
| DEL / OSTW (`.del`, `.ostw`) | `deltin-rs` via future provider | ⚪ Recognized by Wright; every workflow fails with `source-provider-unavailable` (exit 4), with no static DEL dependency or fallback |

`wright convert` reconstructs validated Workshop input as OPY only when the
provider advertises that capability. Workshop → DEL/OSTW reconstruction is not
supported until the owning implementation provides and tests it.

`wright-lsp` currently advertises document synchronization and lifecycle only:
source-language documents publish an explicit `source-provider-unavailable`
diagnostic, and editor features such as hover, completion, or rename are not
advertised until a provider backs them. See
[`docs/language-services.md`](docs/language-services.md).

Compiled output converges structurally on the established upstream compiler
(rule order, element identities, control flow, conditions, values, variable
names and indices, and element cost), compared as canonical Workshop programs
rather than text. Formatting and upstream internal architecture are not
criteria. The owning language implementations hold these compatibility tests.

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
Wright. Neither `opy-rs` nor `deltin-rs` depends on Wright internals.

## Installation

### macOS

```sh
brew tap wrightkit/tap
brew install wrightkit/tap/wright
```

### macOS and Linux

```sh
curl -fsSL https://install.wrightkit.dev/wright/install.sh | bash
```

### Windows

Use the PowerShell installer `https://install.wrightkit.dev/wright/install.ps1`.

Each archive contains `wright` and `wright-lsp`; no Node or OverPy runtime is
required. Prebuilt archives (Linux x86_64, macOS x86_64/arm64, Windows x86_64),
the nightly channel, and package-manager status are documented in
[`docs/release.md`](docs/release.md).

### From source

```sh
cargo build --release -p wright-cli -p wright-lsp
```

Requires Rust 1.85 or newer.

### Updating

```sh
wright update            # standalone install plus installed first-party providers
wright update --check    # report availability only
wright update provider opy
```

Package-manager installs are never overwritten; `wright update` points to the
channel's own upgrade command. See [`docs/cli/update.md`](docs/cli/update.md).

## CLI

```sh
wright check [INPUT]        # correctness diagnostics
wright lint [INPUT]         # lint findings
wright analyze [INPUT]      # cost, hotspots, and risk indicators
wright inspect [INPUT]      # semantic summary
wright inspect symbols|refs|cfg|callgraph|cost
wright rename OLD NEW [INPUT]   # previews a diff; --write applies it
wright compile [INPUT]      # emit Workshop text
wright convert [INPUT] --target opy
wright serve [INPUT]        # wright-agent/v1 over stdio, JSON-RPC 2.0, or MCP
wright update | completion
```

`INPUT` is a file, a project directory, or `-` for stdin; omitting it uses the
current directory. `check`, `analyze`, `lint`, and `inspect cost` accept
finding selection (`--severity`, `--rule-id`, `--file`, `--max`). Exit codes are
`0` success, `1` source/user error, `2` usage error, `3` recognized but
unsupported, `4` internal/environment failure.

When a backing engine encounters unsupported syntax, Wright reports the missing
feature cleanly rather than failing silently or falling back to unverified
runtimes. The full command contract is in [`docs/cli.md`](docs/cli.md).

### Machine-readable output

`--format json` prints exactly one `wright-result/v1` envelope to stdout:

```sh
wright lint input.opy --format json
```

`--renderer github-actions` emits CI annotations.

### Agents and editors

`wright serve` exposes the `wright-agent/v1` session contract (capabilities,
findings, queries, workflows, and validated edits); `--transport mcp` serves
the same contract as native MCP tools to tool-capable harnesses. See
[`docs/agent-contract.md`](docs/agent-contract.md). `wright-lsp` currently
provides document synchronization only, as described above.

## How it works

Wright consumes the owning implementation for each source form rather than
maintaining a second authoritative language implementation:

```text
source input
   ↓
workshop-rs in-process / LPP provider
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
3. fix source-language semantics in `opy-rs` / `deltin-rs`, canonical Workshop
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

Read [`AGENTS.md`](AGENTS.md) and [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md) before
making changes. Do not push directly to `main`; deliver implementation changes
through focused branches and PRs.

## License

Wright is currently distributed under the GNU Affero General Public License v3.0
or later. Third-party reference inputs remain governed by
their recorded licenses and provenance; see [`docs/licensing.md`](docs/licensing.md).
