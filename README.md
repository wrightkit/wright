# Wright

Wright is a Rust-based compiler and tooling project for the Overwatch Workshop / OverPy ecosystem.

## Status

Wright is in the early project-bootstrap stage. The repository now contains a
minimal `wright-core` Rust workspace, but does not yet publish a stable
compiler binary, CLI, or release workflow. Those contracts will be introduced
incrementally and documented when they become available.

## Project direction

Wright is intended to provide a clear, testable foundation for working with Workshop-oriented source code and generated output. The project prioritizes:

* semantic correctness over text-only transformations;
* measurable compatibility with supported reference behavior;
* explicit diagnostics for invalid or unsupported input;
* deterministic output and inspectable implementation boundaries.

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

The bootstrap workspace intentionally does not yet contain a parser, backend,
CLI, or OverPy integration.

## License

Wright is distributed under the [GNU Affero General Public License v3.0 or later](LICENSE).

AGPL permits commercial use, but a modified version that users interact with
over a network must offer those users access to its corresponding source code.
See the license text for the exact requirements.
