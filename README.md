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
- **Validated edits.** Renames that preview a diff before writing.
- **Agent and editor integration.** `wright serve` (JSON-RPC and MCP) and
  `wright-lsp`.
- **CI-friendly output.** JSON reports and GitHub Actions annotations.

## Language support

| Source | Status |
| --- | --- |
| Raw Workshop | ✅ Supported |
| OverPy (`.opy`) | 🟡 `check`, `compile`, `lint`, `analyze` via the `opy-rs` provider |
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

# macOS and Linux
curl -fsSL https://install.wrightkit.dev/wright/install.sh | bash
```

On Windows, use the PowerShell installer at
`https://install.wrightkit.dev/wright/install.ps1`. Archives, the nightly
channel, and building from source are covered in [`docs/release.md`](docs/release.md).

Update with `wright update`.

## Usage

```sh
wright check [INPUT]            # correctness diagnostics
wright lint [INPUT]             # lint findings
wright analyze [INPUT]          # cost, hotspots, and risk indicators
wright inspect [INPUT]          # semantic summary
wright compile [INPUT]          # write <input>.ow; --to LOCALE selects the output locale
wright serve [INPUT]            # agent session over stdio or MCP
```

`INPUT` is a file, a project directory, or `-` for stdin; omit it to use the
current directory. Add `--format json` for machine-readable output. See
[`docs/cli.md`](docs/cli.md) for the full command reference.

## Documentation

See [`docs/README.md`](docs/README.md) for the documentation index.

## Contributing

Read [`AGENTS.md`](AGENTS.md) and [`docs/CONTRIBUTING.md`](docs/CONTRIBUTING.md)
first. Deliver changes through branches and pull requests.

## License

AGPL-3.0-or-later. Third-party reference inputs keep their own licenses; see
[`docs/licensing.md`](docs/licensing.md).
