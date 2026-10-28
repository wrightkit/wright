# Wright

[![CI Status](https://github.com/wrightkit/wright/actions/workflows/ci.yml/badge.svg)](https://github.com/wrightkit/wright/actions/workflows/ci.yml)
[![License: AGPL-3.0-or-later](https://img.shields.io/badge/License-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/wrightkit/wright?include_prereleases)](https://github.com/wrightkit/wright/releases)

Wright is the developer CLI for Overwatch Workshop projects. It checks, lints,
analyzes, inspects, and safely edits Workshop code, for people and for coding
agents.

- **Diagnostics and lint.** Deterministic `check` and `lint` results.
- **Cost and risk analysis.** Workshop element cost, hotspots, and server-load
  risk indicators, each labeled exact, static, or heuristic.
- **Semantic queries.** Symbols, references, control flow, call graph, cost.
- **Validated edits.** `lint --fix` previews each fix as a validated diff;
  agents validate edit transactions through `wright serve`.
- **Agent and editor integration.** `wright serve` (JSON-RPC and MCP),
  `wright agent install` for the agent guide and MCP registration, and
  `wright-lsp`.
- **CI-friendly output.** JSON reports and GitHub Actions annotations.

## Language support

| Source | Status |
| --- | --- |
| Raw Workshop | ✅ Supported |
| OverPy (`.opy`) | 🟡 `check`, `compile`, `lint`, `analyze`, `inspect`, `lookup`, Workshop → OPY `convert`, and LSP via the `opy-rs` provider |
| DEL / OSTW | ⚪ Recognized, not yet supported |

Language parsing and compilation live in dedicated engines:
[`workshop-rs`](https://github.com/wrightkit/workshop-rs),
[`opy-rs`](https://github.com/wrightkit/opy-rs), and
[`deltin-rs`](https://github.com/wrightkit/deltin-rs). Wright reports missing
language features instead of working around them.

## Install

```sh
# macOS
brew install wrightkit/tap/wright

# macOS and Linux x86_64
curl -fsSL https://install.wrightkit.dev/wright/install.sh | bash
```

```powershell
# Windows x86_64
irm https://install.wrightkit.dev/wright/install.ps1 | iex
```

Every channel installs `wright` and `wright-lsp`. Scoop, WinGet, release
archives, and the nightly channel are covered in [`docs/release.md`](docs/release.md).

Update a standalone install with `wright update`. Homebrew, Scoop, and WinGet
installs upgrade through their package manager.

## Usage

```sh
wright check [INPUT]                 # correctness diagnostics
wright lint [INPUT]                  # lint findings
wright analyze [INPUT]               # cost, hotspots, and risk indicators
wright inspect [INPUT]               # semantic summary
wright compile [INPUT]               # write <input stem>.ow; -o PATH, --to LOCALE
wright convert [INPUT] --target opy  # reconstruct Workshop as OverPy
wright lookup [QUERY]                # resolve a display name or guess to the accepted spelling
wright serve [INPUT]                 # agent session over stdio or MCP
```

`INPUT` is a file or a project directory; omit it to use the current
directory. Most commands also read raw Workshop from `-` (stdin). Add
`--format json` for machine-readable output. See [`docs/cli.md`](docs/cli.md)
for the full command reference.

## Documentation

See [`docs/README.md`](docs/README.md) for the documentation index.

## Contributing

Read [`AGENTS.md`](AGENTS.md) and [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)
first. Deliver changes through branches and pull requests.

## License

AGPL-3.0-or-later. Third-party reference inputs keep their own licenses; see
[`docs/licensing.md`](docs/licensing.md).
