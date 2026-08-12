# Wright

Wright is a Rust-based compiler and tooling project for the Overwatch Workshop / OverPy ecosystem.

## Status

Wright is under active development. The repository contains a `wright` CLI and
reusable `wright-driver` (compiler/session) crate over the Rust IR core, the
native Workshop frontend/emitter, and the semantic analyzer. The native
`.opy` frontend and further milestones are in progress; see the GitHub issues
and roadmap for the current contract.

## Project direction

Wright is intended to provide a clear, testable foundation for working with Workshop-oriented source code and generated output. The project prioritizes:

* semantic correctness over text-only transformations;
* measurable compatibility with supported reference behavior;
* explicit diagnostics for invalid or unsupported input;
* deterministic output and inspectable implementation boundaries.
20→
## Development

Read [`AGENTS.md`](AGENTS.md) and [`CONTRIBUTING.md`](CONTRIBUTING.md) before
making changes. They describe the repository boundary, source-of-truth order,
compatibility expectations, validation policy, and delivery workflow.

The local default uses the stable Rust toolchain. The minimum supported Rust
version is 1.85.0. Run the repository quality checks with:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

## CLI

```sh
wright compile input.opy           # emit Workshop text
wright check program.txt          # parse, validate, analyze
wright analyze program.txt        # semantic findings
wright inspect program.txt        # structural model
echo 'rule ...' | wright check -  # stdin workflows
```

See [`docs/cli.md`](docs/cli.md) for the exit-code, stdout/stderr, and
`wright-result/v1` machine-readable contracts.

## License

Wright is distributed under the [GNU Affero General Public License v3.0 or later](LICENSE).

AGPL permits commercial use, but a modified version that users interact with
over a network must offer those users access to its corresponding source code.
See the license text for the exact requirements.
