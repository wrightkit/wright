# Wright Release Process (M8, issue #54)

Status: v1 release automation contract

## Release artifact

`scripts/release.sh [version]` (default `0.1.0`) produces
`target/wright-<version>.tar.gz` containing:

* the standalone `wright` release binary;
* `version.json` with the version, `wright-result/v1` contract identity, git
  commit, build timestamp, and the runtime-dependency claim
  (`"requires": { "node": false, "overpy": false }`).

## What the release script verifies before stamping

1. **Quality gates** — `cargo fmt --check`, `cargo clippy -D warnings`,
   `cargo test --workspace --all-targets --all-features`.
2. **N-level gate** — `scripts/v1-gates.py` against the release binary
   (`target/v1-gates-report.json`).
3. **E-level scenarios** — `scripts/run-scenarios.py` against the release
   binary (`target/scenarios-report.json`).
4. **Benchmarks** — `wright-bench` with declared regression thresholds
   (`target/wright-bench-report.json`).
5. **Standalone proof** — the packaged binary runs `compile`/`check` over the
   corpus with `PATH=/usr/bin:/bin` (Node and OverPy absent).

Any gate failure aborts the release before the version is stamped.

## Version metadata

The binary reports its version via `wright version` and every
`wright-result/v1` envelope carries `wright.version` + `wright.contract`.
The release tarball's `version.json` is the authoritative stamp for a
shipped artifact.
