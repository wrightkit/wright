# Wright

Wright is a **tooling-first semantic platform** for the Overwatch Workshop and
OverPy ecosystem. It provides native `.opy` and Workshop semantic frontends,
typed intermediate representations, lint and static analysis, editor language
services, compilation and inspection, and reusable library and agent APIs.

## Current capabilities

The repository currently includes:

- the `wright` CLI for compiling, checking, analyzing, and inspecting source;
- native Rust support for the documented `.opy` subset and Workshop text;
- typed HIR/WIR models with source provenance and validation;
- deterministic diagnostics and the `wright-result/v1` JSON contract;
- `wright-serve` stdio/JSON-RPC tool adapters;
- `wright-lsp`, a thin LSP adapter over editor-neutral language services;
- compatibility fixtures, differential checks, and benchmark tooling.

The project is still under active development. Supported syntax and
compatibility claims are limited to the documented matrices and test corpus;
see [`docs/v1-matrix.md`](docs/v1-matrix.md),
[`docs/opy/support-matrix.md`](docs/opy/support-matrix.md), and
[`COMPATIBILITY.md`](COMPATIBILITY.md).

## Install

Pinned binary releases are published as GitHub Releases from `v*` tags
(issue #101). Download a platform archive and its checksum, verify, and add
`wright`/`wright-lsp` to `PATH`:

```sh
VERSION=0.1.0
TARGET=x86_64-unknown-linux-gnu   # or aarch64-apple-darwin / x86_64-apple-darwin / x86_64-pc-windows-msvc
BASE="https://github.com/wrightkit/wright/releases/download/v$VERSION"
curl -fsSL -O "$BASE/wright-$VERSION-$TARGET.tar.gz"
curl -fsSL -O "$BASE/wright-$VERSION-$TARGET.tar.gz.sha256"
shasum -a 256 -c "wright-$VERSION-$TARGET.tar.gz.sha256"
tar -xzf "wright-$VERSION-$TARGET.tar.gz"
export PATH="$PWD/wright-$VERSION-$TARGET:$PATH"
```

Windows uses `wright-<version>-x86_64-pc-windows-msvc.zip`. See
[`docs/release.md`](docs/release.md) for the full distribution contract.

## CLI

Build the CLI from the workspace:

```sh
cargo build -p wright-cli
```

The executable supports file and stdin workflows:

```sh
wright compile input.opy          # emit Workshop text
wright check input.opy            # validate and report diagnostics
wright analyze input.opy          # report semantic findings
wright inspect input.opy          # inspect the structural model
cat input.opy | wright check -    # read from stdin
```

Use `--format json` for the machine-readable `wright-result/v1` envelope. The
complete command, input, exit-code, and output contract is in
[`docs/cli.md`](docs/cli.md).

## Workspace layout

| Crate or directory | Responsibility |
| --- | --- |
| `wright-opy` | Native `.opy` lexer, parser, preprocessing, resolution, and lowering |
| `wright-workshop` | Workshop lexer, parser, catalog, validation, and emitter |
| `wright-ir` / `wright-core` | Typed intermediate representations and compiler core |
| `wright-analyzer` | Symbols, references, control flow, and semantic findings |
| `wright-driver` | Shared compiler/session and embedding API |
| `wright-language` / `wright-lsp` | Editor-neutral language services and LSP transport |
| `compatibility/` | Pinned reference oracle and compatibility fixtures |

Architecture boundaries and accepted design decisions are documented in
[`ARCHITECTURE.md`](ARCHITECTURE.md) and [`docs/adr/README.md`](docs/adr/README.md).

## Development

Read [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md) before
making changes. The workspace uses Rust Edition 2024 and requires Rust 1.85.0
or newer.

Run the repository quality checks from the root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

For embedding and language-service contracts, see
[`docs/embedding.md`](docs/embedding.md) and
[`docs/language-services.md`](docs/language-services.md). Release validation
is described in [`docs/release.md`](docs/release.md).

## License

Wright is distributed under the [GNU Affero General Public License v3.0 or
later](LICENSE). The compatibility adapter, oracle, and imported fixtures have
their own provenance and licensing boundaries; see
[`LICENSE-BOUNDARY.md`](LICENSE-BOUNDARY.md).
