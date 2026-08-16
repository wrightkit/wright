# Wright

[![CI Status](https://github.com/wrightkit/wright/actions/workflows/ci.yml/badge.svg)](https://github.com/wrightkit/wright/actions/workflows/ci.yml)
[![MSRV](https://img.shields.io/badge/MSRV-1.85.0-blue.svg)](rust-toolchain.toml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/wrightkit/wright?include_prereleases)](https://github.com/wrightkit/wright/releases)

Wright is a **tooling-first semantic platform** for the Overwatch Workshop and
OverPy ecosystem. Built natively in Rust, it provides independent semantic
frontends, typed intermediate representations, static analysis and linting,
editor language services, and reusable embedding APIs for developers, CI, and AI
agents.

---

## Introduction

Creating and maintaining complex Overwatch Workshop scripts requires reliable
compilation, semantic validation, and modern developer tooling. Wright delivers
an independent, standalone Rust toolchain with zero runtime dependencies (requiring
neither Node.js, .NET, nor external interpreters) for core compilation and analysis.

Wright treats developer tooling (linting, diagnostics, semantic queries,
editor assistance, and safe source transformations) as first-class product
surfaces alongside compiler code generation.

---

## Features

- **High-Performance Compiler**: Compile `.opy` source scripts and Workshop
  text to optimized, localized Workshop code.
- **Deterministic Diagnostics**: Structured errors, warnings, and source
  spans surfaced in human-readable terminal format and the machine-readable
  `wright-result/v1` JSON contract.
- **First-Class Linting (`wright lint`)**: Static stability and performance
  analysis with configurable rules (`min-wait-loop`, `duplicate-condition`,
  `expensive-loop-check`, `repeated-value`, `while-without-wait`) and explicit
  evidence labeling.
- **Language Server (`wright-lsp`)**: Lightweight LSP implementation
  providing hover documentation, definition navigation, reference searches,
  project-wide identifier rename, and semantic syntax highlighting.
- **Embedding & Agent Tooling**: Session-based driver (`wright-driver`) with
  stdio/JSON-RPC adapters (`wright-serve`) enabling programmatic inspection,
  cost estimation, and verified source-level refactoring.
- **Standalone Distribution**: Self-contained binaries with zero external
  runtime dependencies.

---

## Ecosystem Compatibility and Conversion

Wright is designed as a multi-frontend semantic platform for the Overwatch
Workshop ecosystem, with Vanilla Workshop text as the canonical
interoperability boundary.

### High-Level Ecosystem Support

| System | Role in Wright | Current Capabilities | Status & Support Boundary | Authority Contract |
| --- | --- | --- | --- | --- |
| **Vanilla Workshop** | Canonical target and interoperability layer | Canonical parser, localized catalog, validation, and deterministic emitter (`workshop-rs`, consumed through the `wright-workshop` adapter). Supports rules, actions, values, events, enums, variables, subroutines, and custom-game-settings emission. | Supported (`en-US` baseline; data-driven localization extensible) | [`docs/workshop/support-matrix.md`](docs/workshop/support-matrix.md) |
| **OPY / OverPy** | Native compatible semantic frontend | Native Rust parser (`wright-opy`), preprocessor (`#!include`, `#!define`), macro expansion, declarations, expressions, enums, custom-game-settings (JSONC blocks), lowering to Wright HIR and Workshop IR. Pinned `overpy@9.7.10` acts strictly as an external compatibility oracle under clean-room isolation. | Supported (corpus-evidenced native compiler and tooling) | [`docs/opy/support-matrix.md`](docs/opy/support-matrix.md), [`docs/licensing.md`](docs/licensing.md) |
| **OSTW (Overwatch Script To Workshop)** | Compatible semantic frontend (native OSTW frontend) | Native Rust parser/CST (`wright-ostw`), project settings (`ds.toml`), import-closure resolution, and semantic lowering to Wright HIR for the protect-ban slice. Pinned OSTW `v3.4.0` serves strictly as an external compatibility oracle. | In progress (native frontend & HIR lowering baseline) | [`docs/ostw/compatibility-baseline.md`](docs/ostw/compatibility-baseline.md), [`docs/architecture.md`](docs/architecture.md) |

### Conversion Matrix

Workshop is the canonical hub for cross-language conversion. The required
long-term conversion directions are:

```text
OPY      → Workshop   (Supported: native wright-opy → HIR → WIR → workshop-rs)
OSTW     → Workshop   (In progress: native wright-ostw → HIR → WIR → workshop-rs)
Workshop → OPY        (Planned: decompilation / translation via canonical WIR)
Workshop → OSTW       (Planned: translation via canonical WIR)
Workshop → Workshop   (Supported: localized catalog parse → canonical WIR → deterministic emit)
```

Direct OPY ↔ OSTW source translation is an optional side path and does not
drive core architecture.

### Four-Level Verification Model

Compiler compatibility is rigorously measured under the S/D/N/E framework:

- **S (Syntax)**: Agrees on accepting valid inputs and rejecting unsupported syntax across the corpus.
- **D (Diagnostics)**: Reports structured diagnostic categories, codes, and accurate source spans for diagnosed inputs.
- **N (Normalized Output)**: Produces equivalent Workshop output under versioned normalization.
- **E (Observable Semantics)**: High-risk runtime semantics are verified against repeatable behavioral scenarios.

> **Semantic Priority ([ADR-0008](docs/adr/0008-tooling-first-semantic-platform.md)):**
> Observable semantics and valid Workshop syntax outrank byte-identical text formatting:
> `E (semantics) > D (diagnostics) > S (syntax) > N (text output)`. Clean-room licensing
> boundaries isolate external evaluation oracles from Wright runtime code
> ([`docs/licensing.md`](docs/licensing.md)).

---

## Installation

Wright ships standalone `wright` and `wright-lsp` binaries for Linux x86_64,
macOS (Apple Silicon and Intel), and Windows x86_64. All installation paths
below consume the same canonical [GitHub Release](https://github.com/wrightkit/wright/releases)
archives and verify the published checksums; none of them build Wright from
source.

### macOS

Homebrew is the recommended path (install the WrightKit tap once, then
`wright` as a normal formula):

```sh
brew tap wrightkit/tap
brew install wrightkit/tap/wright
```

Alternatively, use the Unix installer:

```sh
curl -fsSL https://wrightkit.dev/install.sh | bash
```

### Linux

```sh
curl -fsSL https://wrightkit.dev/install.sh | bash
```

The installer detects your platform, downloads the matching release archive,
verifies its SHA-256 checksum, and installs `wright` and `wright-lsp` into
`~/.local/bin` (add that directory to your `PATH` if the installer asks).

### Windows

WinGet is the recommended path (available after the package clears upstream
review):

```powershell
winget install WrightKit.Wright
```

Scoop is an alternative package-manager path:

```powershell
scoop bucket add wrightkit https://github.com/wrightkit/scoop-bucket
scoop install wright
```

Both WinGet and Scoop consume the same Windows release ZIP.

### npm / npx

For Node.js workflows and agent tooling, install Wright as a platform-native npm package:

```sh
# Run on-demand with npx
npx @wrightkit/wright --version

# Or add to a project
npm install @wrightkit/wright
npx wright check main.opy
```

Downstream JavaScript/TypeScript packages can depend on `@wrightkit/wright` directly and resolve the native binary path programmatically without runtime download scripts:

```javascript
const { getBinaryPath } = require('@wrightkit/wright');
const wrightBin = getBinaryPath('wright');
```

### CI / agents

Pin an exact version non-interactively for deterministic installs:

```sh
curl -fsSL https://wrightkit.dev/install.sh | bash -s -- --version 0.1.0
```

or consume the release archives directly (see below). Installer options:
`--version <version>` pins an exact version, `--dir <directory>` selects a
custom installation directory, `--help` lists all options.

### Manual release archives (fallback)

Download precompiled release archives and verify them by hand when you need
full control:

```sh
VERSION=0.1.0
TARGET=x86_64-unknown-linux-gnu # or aarch64-apple-darwin / x86_64-apple-darwin / x86_64-pc-windows-msvc
BASE="https://github.com/wrightkit/wright/releases/download/v$VERSION"

curl -fsSL -O "$BASE/wright-$VERSION-$TARGET.tar.gz"
curl -fsSL -O "$BASE/wright-$VERSION-$TARGET.tar.gz.sha256"
shasum -a 256 -c "wright-$VERSION-$TARGET.tar.gz.sha256"
tar -xzf "wright-$VERSION-$TARGET.tar.gz"
export PATH="$PWD/wright-$VERSION-$TARGET:$PATH"
```

Windows distributions are provided as `.zip` archives. Full packaging,
checksum, and package-manager publication details are in the
[Release Documentation](docs/release.md) and
[Distribution Documentation](dist/README.md).

### Updating

Standalone installations (the Unix installer or manual archives) update
themselves in place:

```sh
wright update          # upgrade wright and wright-lsp to the latest stable release
wright update --check  # report whether an update is available, without modifying anything
wright update --version 0.2.0  # install an exact version
```

`wright update` downloads the same checksum-verified release archives as the
installer, replaces both binaries atomically, and never touches
package-manager-managed installations; Homebrew, Scoop, and WinGet installs
should upgrade through their own channel (`brew upgrade wrightkit/tap/wright`,
`scoop update wright`, `winget upgrade WrightKit.Wright`).

### Building From Source

Build all tools from the root workspace using Rust 1.85.0+:

```sh
cargo build --release -p wright-cli -p wright-lsp
```

Binaries will be placed in `target/release/wright` and `target/release/wright-lsp`.

---

## Quick Start

### CLI Workflows

```sh
# Compile source to Workshop text
wright compile input.opy

# Validate and report diagnostics
wright check input.opy

# Run stability and performance lint checks
wright lint input.opy

# Perform deep semantic analysis
wright analyze input.opy

# Inspect structural model, rules, and symbols
wright inspect input.opy

# Stdin piping
cat input.opy | wright lint -

# Machine-readable JSON output for CI / agent integration
wright lint input.opy --format json
```

### Language Server

Integrate `wright-lsp` into your preferred editor (VS Code, Neovim, Emacs,
Zed) by registering `wright-lsp` as the language server binary for `.opy` and
`.ws` filetypes over standard I/O.

---

## How It Works

Wright processes source code through a modular, owned pipeline:

```text
Source Input (.opy or Workshop text)
    ↓
Owned Semantic Frontend (wright-opy / workshop-rs via wright-workshop adapter)
    ↓
Wright High-Level Intermediate Representation (HIR)
    ↓
Wright Workshop IR (WIR) & Semantic Analysis (wright-analyzer)
    ├─→ Compiler Backend → Emitted Workshop text
    ├─→ Lint & Static Analysis (wright lint)
    ├─→ Language Services & LSP (wright-language / wright-lsp)
    └─→ Embedding & Tool APIs (wright-driver / wright-serve)
```

1. **Owned Frontends**: Parse source into typed representations with full source
   provenance without leaking third-party AST types.
2. **Wright HIR & WIR**: Provide frontend-independent semantic and target-level
   data structures.
3. **Semantic Layer**: Performs symbol resolution, control-flow graph (CFG)
   construction, reference indexing, and lint evaluation.
4. **Target Emission**: Generates deterministic, catalog-verified Workshop
   output.

---

## Documentation

Full architectural specifications, API references, and policy contracts are
indexed in [`docs/README.md`](docs/README.md):

- **Architecture & Design**:
  - [System Architecture](docs/architecture.md): Module boundaries, responsibilities, and data flow.
  - [Architecture Decision Records (ADRs)](docs/adr/README.md): Recorded architectural decisions and historical context.
- **Contracts & Interfaces**:
  - [CLI & Driver Contract](docs/cli.md): Command interface, exit codes, and JSON result envelope.
  - [Embedding & Tool API](docs/embedding.md): Programmatic Rust API and session tool service.
  - [Language Services & LSP](docs/language-services.md): Editor language features and transport specifications.
  - [Opy HIR v1 Protocol](docs/hir/opy-hir-v1.md): Frontend interchange protocol.
- **Compatibility & Standards**:
  - [Compatibility Policy](docs/compatibility.md): S/D/N/E verification methodology and release gates.
  - [Compatibility Matrix](docs/v1-matrix.md): Declared surfaces and release gate status.
  - [OPY Support Matrix](docs/opy/support-matrix.md): Supported `.opy` syntax and feature boundary.
  - [Workshop Support Matrix](docs/workshop/support-matrix.md): Evidenced Workshop actions, values, and enums.
- **Engineering & Operations**:
  - [Licensing & Clean-Room Policy](docs/licensing.md): Intellectual property boundary and clean-room development rules.
  - [Release Process](docs/release.md): Versioning scheme, CI packaging, and distribution gates.
  - [Agent Team Contract](docs/agent-team.md): Multi-agent collaboration protocol and decision governance.

---

## Contributing

Contributions are welcome! Please read [`AGENTS.md`](AGENTS.md) and
[`CONTRIBUTING.md`](CONTRIBUTING.md) before making changes.

### Local Development & Quality Gates

Wright requires Rust Edition 2024 (MSRV 1.85.0). Run standard quality gates
before submitting changes:

```sh
# Code formatting
cargo fmt --all -- --check

# Linter checks
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Test suite
cargo test --workspace --all-targets --all-features </dev/null
```

---

## License

Wright is distributed under the terms of the [GNU Affero General Public License
v3.0 or later](LICENSE).

Third-party compatibility references, test fixtures, and adapter components are
isolated and governed by their own recorded licenses and provenance; see
[`docs/licensing.md`](docs/licensing.md).
