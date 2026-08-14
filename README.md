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
an independent, standalone toolchain that does not rely on Node.js or Python
runtimes for core compilation and analysis.

Wright treats developer tooling — linting, diagnostics, semantic queries,
editor assistance, and safe source transformations — as first-class product
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

## Ecosystem Compatibility

Wright implements native semantic frontends while maintaining compatibility
with the existing ecosystem:

- **Workshop Target**: Vanilla Overwatch Workshop text is Wright's canonical
  target and interoperability layer.
- **`.opy` Frontend**: Native parser and preprocessor supporting includes,
  defines, macros, enums, custom-game-settings blocks, and language constructs
  documented in the [OPY Support Matrix](docs/opy/support-matrix.md).
- **Four-Level Verification**: Compatibility is measured under the S/D/N/E
  framework:
  - **S (Syntax)**: Agrees on accepting valid inputs and rejecting unsupported syntax.
  - **D (Diagnostics)**: Reports structured diagnostics and accurate source spans.
  - **N (Normalized Output)**: Produces equivalent Workshop output under versioned normalization.
  - **E (Observable Semantics)**: High-risk runtime semantics are verified against repeatable behavioral scenarios.
- **Clean-Room Boundary**: Pinned reference oracles (such as OverPy 9.7.10)
  serve purely as evaluation references and are completely isolated from
  Wright's runtime code. See [Licensing Policy](docs/licensing.md).

---

## Installation

### Prebuilt Binaries

Download precompiled release archives for your platform from
[GitHub Releases](https://github.com/wrightkit/wright/releases):

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

Windows distributions are provided as `.zip` archives. Full packaging details
are in the [Release Documentation](docs/release.md).

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
Owned Semantic Frontend (wright-opy / wright-workshop)
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
  - [System Architecture](docs/architecture.md) — Module boundaries, responsibilities, and data flow.
  - [Architecture Decision Records (ADRs)](docs/adr/README.md) — Recorded architectural decisions and historical context.
- **Contracts & Interfaces**:
  - [CLI & Driver Contract](docs/cli.md) — Command interface, exit codes, and JSON result envelope.
  - [Embedding & Tool API](docs/embedding.md) — Programmatic Rust API and session tool service.
  - [Language Services & LSP](docs/language-services.md) — Editor language features and transport specifications.
  - [Opy HIR v1 Protocol](docs/hir/opy-hir-v1.md) — Frontend interchange protocol.
- **Compatibility & Standards**:
  - [Compatibility Policy](docs/compatibility.md) — S/D/N/E verification methodology and release gates.
  - [Compatibility Matrix](docs/v1-matrix.md) — Declared surfaces and release gate status.
  - [OPY Support Matrix](docs/opy/support-matrix.md) — Supported `.opy` syntax and feature boundary.
  - [Workshop Support Matrix](docs/workshop/support-matrix.md) — Evidenced Workshop actions, values, and enums.
- **Engineering & Operations**:
  - [Licensing & Clean-Room Policy](docs/licensing.md) — Intellectual property boundary and clean-room development rules.
  - [Release Process](docs/release.md) — Versioning scheme, CI packaging, and distribution gates.
  - [Agent Team Contract](docs/agent-team.md) — Multi-agent collaboration protocol and decision governance.

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
