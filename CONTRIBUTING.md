# Contributing to Wright

Wright is being built incrementally. Read [`AGENTS.md`](AGENTS.md),
[`ARCHITECTURE.md`](ARCHITECTURE.md), [`COMPATIBILITY.md`](COMPATIBILITY.md),
and [`LICENSE-BOUNDARY.md`](LICENSE-BOUNDARY.md) before changing compiler
boundaries or compatibility tooling.

## Toolchain policy

The repository's local default is the Rust `stable` toolchain configured by
[`rust-toolchain.toml`](rust-toolchain.toml). The minimum supported Rust
version (MSRV) is Rust 1.85.0, which is also the workspace's Edition 2024
baseline. CI checks both stable and the MSRV.

The MSRV may be raised only as an explicit project decision. A dependency or
language feature that requires a newer compiler must not be introduced without
updating the workspace manifest, CI matrix, and this policy together.

## Local checks

Run the same checks used by CI from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
```

To verify the MSRV locally when it is not already installed:

```sh
rustup toolchain install 1.85.0 --profile minimal
cargo +1.85.0 fmt --all -- --check
cargo +1.85.0 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.85.0 test --workspace --all-targets --all-features
```

The bootstrap workspace contains only the owned `wright-core` library. Do not
add parser, lowering, backend, or OverPy integration code without the focused
contract and tests required by the architecture documents. Compatibility tools,
when introduced, must remain optional to the core quality gates and obey the
license boundary.

## Change conventions

* Keep changes within the owning package or documented boundary.
* Add regression coverage at the lowest layer that proves the behavior.
* Record material contract changes in a new ADR instead of silently rewriting
  an accepted decision.
* Preserve provenance and licensing information for external fixtures,
  generated artifacts, and dependencies.
* Keep commits focused on one functional boundary and do not include unrelated
  working-tree changes.
