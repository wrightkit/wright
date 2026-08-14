# Contributing to Wright

Wright is being built incrementally. Read [`AGENTS.md`](AGENTS.md),
[`docs/architecture.md`](docs/architecture.md), [`docs/compatibility.md`](docs/compatibility.md),
and [`docs/licensing.md`](docs/licensing.md) before changing compiler
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

## Wright-specific Rust policy

The workspace lint baseline in [`Cargo.toml`](Cargo.toml) is intentionally
small. `rustc`/`rustfmt` and the normal Clippy defaults continue to own
mechanical style; the workspace additionally denies unsafe operations that are
not explicitly scoped, unreachable public items, correctness/suspicious
Clippy findings, and unfinished development macros. Performance findings stay
warning-level locally and are errors in the CI commands above. The policy does
not enable Clippy's `pedantic`, `nursery`, or `restriction` groups.

This policy was audited against the workspace manifests, CI, and representative
compiler/tooling crates (`wright-ir`, `wright-core`, `wright-driver`,
`wright-opy`, `wright-workshop`, `wright-language`, and `wright-lsp`):

* Semantic identities use typed IR IDs and arena lookups; raw strings remain at
  protocol, source-name, and presentation boundaries. New public contracts
  should use a typed representation when two raw values could be confused.
* Observable ordering uses insertion-ordered arenas or ordered collections at
  output/service boundaries. Hash maps and sets remain appropriate for private
  lookup and membership indexes, but their iteration order must not leak into a
  compiler or tooling result.
* Diagnostics are structured driver/language-service data. Human-readable
  rendering belongs in the CLI/LSP presentation boundary and must not become a
  substitute for a machine contract.
* Existing `unwrap`/`expect` calls are concentrated in invariant-checked
  compiler paths, serialization boundaries, benchmarks, and tests. They are
  reviewed by context; Wright does not impose a blanket ban. New fallible
  compiler behavior should return a structured error instead of panicking.
* Shared interior mutability, broad cloning, complex lifetimes, and dynamic
  dispatch require a concrete ownership or API reason. Do not add them merely
  to avoid designing that boundary.
* The only current `unsafe` operation is the benchmark's platform-specific
  `getrusage` measurement. New unsafe code requires a concrete need, a local
  invariant comment, focused tests, and the smallest possible unsafe surface.
  Dependencies must preserve the declared Rust 1.85.0 MSRV and crate
  dependency direction.

## Change conventions

* Keep changes within the owning package or documented boundary.
* Add regression coverage at the lowest layer that proves the behavior.
* Record material contract changes in a new ADR instead of silently rewriting
  an accepted decision.
* Preserve provenance and licensing information for external fixtures,
  generated artifacts, and dependencies.
* Keep commits focused on one functional boundary and do not include unrelated
  working-tree changes.
