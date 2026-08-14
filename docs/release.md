# Wright Release Process and Distribution Contract (M8, issue #54; v0.1 contract, issue #101)

Status: v1 release automation contract

## Local release artifact

`scripts/release.sh [version]` (default `0.1.0`) produces
`target/wright-<version>.tar.gz` containing:

* the standalone `wright` and `wright-lsp` release binaries;
* `version.json` with the version, `wright-result/v1` contract identity, git
  commit, build timestamp, and the runtime-dependency claim
  (`"requires": { "node": false, "overpy": false }`).

This is the local staging path and the validation suite behind the public
tag-driven release workflow; the GitHub workflow publishes the per-platform
archives, and this script verifies and packages the host platform.

## What the release script verifies before stamping

1. **Quality gates** — `cargo fmt --check`, `cargo clippy -D warnings`,
   `cargo test --workspace --all-targets --all-features`.
2. **N-level gate** — `scripts/v1-gates.py` against the release binary
   (`target/v1-gates-report.json`).
3. **E-level scenarios** — `scripts/run-scenarios.py` against the release
   binary (`target/scenarios-report.json`).
4. **Benchmarks** — `wright-bench` with declared regression thresholds
   (`target/wright-bench-report.json`).
5. **Standalone proof** — the packaged binaries run `compile`/`check` over the
   corpus with `PATH=/usr/bin:/bin` (Node and OverPy absent), and
   `wright-lsp --version` reports the release version.

Any gate failure aborts the release before the version is stamped.

## Version metadata

The binaries report the workspace implementation version (one authoritative
`version = "0.1.0"` in `[workspace.package]`; every crate inherits it via
`version.workspace = true`). `wright version` / `wright --version` prints the
CLI banner, `wright-lsp --version` prints the LSP banner, and the LSP
`initialize` response carries `serverInfo.version`. Every `wright-result/v1`
envelope carries `wright.version` + `wright.contract`. The release archive's
`version.json` is the authoritative stamp for a shipped artifact.

## Public distribution contract (issue #101)

A `v*` tag push (e.g. `v0.1.0`) drives `.github/workflows/release.yml`:

1. **release-gates** verifies the tag version equals the workspace
   implementation version (drift guard), then runs the full `scripts/release.sh`
   gate suite.
2. **build** compiles `wright` + `wright-lsp` for the initial target matrix and
   packages each platform-appropriate archive.
3. **publish** verifies the complete artifact set and creates the GitHub
   Release from the tag with archives and checksums attached.

A failure in any gate or any required target aborts the workflow before
publication; there is no partial release.

### Creating a release

The tag is the release decision point. Before tagging, bump the version in
`[workspace.package]` of the root `Cargo.toml` (and land it via a normal PR);
the release workflows reject a tag whose version does not match the workspace
implementation version. Then either:

* **From the command line:** `git tag v0.1.0 && git push origin v0.1.0` — the
  tag push triggers `release.yml` directly; or
* **From GitHub:** run the **Release tag** workflow
  (Actions → Release tag → Run workflow) with the version, e.g. `0.1.0`. It
  validates the semver and the workspace-version match, fails if the tag
  already exists, pushes `v<version>`, and then dispatches `release.yml`
  (a tag pushed by CI would otherwise not retrigger the tag workflow).

Both paths run the same release gates, build the same target matrix, and
publish through the same `publish` job.

### Target matrix and artifact naming

Artifacts use the stable scheme `wright-<version>-<target-triple>.<ext>`:

| Platform | Target triple | Archive |
| --- | --- | --- |
| Linux x86_64 | `x86_64-unknown-linux-gnu` | `wright-0.1.0-x86_64-unknown-linux-gnu.tar.gz` |
| macOS x86_64 | `x86_64-apple-darwin` | `wright-0.1.0-x86_64-apple-darwin.tar.gz` |
| macOS arm64 | `aarch64-apple-darwin` | `wright-0.1.0-aarch64-apple-darwin.tar.gz` |
| Windows x86_64 | `x86_64-pc-windows-msvc` | `wright-0.1.0-x86_64-pc-windows-msvc.zip` |

Each archive contains `wright` (`wright.exe`), `wright-lsp`
(`wright-lsp.exe`), and `version.json`. Every archive has a
`<archive>.sha256` checksum, and the Release also carries a combined
`SHA256SUMS`. Binaries are unstripped release builds; signing/notarization,
installers, and package-manager distribution are deferred (below).

### Installing from GitHub Releases

The download URL is deterministic:

```text
https://github.com/wrightkit/wright/releases/download/v<version>/wright-<version>-<target-triple>.<ext>
```

For example, Linux x86_64 at `v0.1.0`:

```sh
VERSION=0.1.0
TARGET=x86_64-unknown-linux-gnu
BASE="https://github.com/wrightkit/wright/releases/download/v$VERSION"
curl -fsSL -O "$BASE/wright-$VERSION-$TARGET.tar.gz"
curl -fsSL -O "$BASE/wright-$VERSION-$TARGET.tar.gz.sha256"
shasum -a 256 -c "wright-$VERSION-$TARGET.tar.gz.sha256"   # verify before use
tar -xzf "wright-$VERSION-$TARGET.tar.gz"
export PATH="$PWD/wright-$VERSION-$TARGET:$PATH"
wright --version
```

Consumers should verify the checksum before use; the recorded name inside a
`.sha256` file is the archive basename, so `shasum -a 256 -c` works from the
directory holding both files. Windows consumers can download the `.zip` and
matching `.zip.sha256`, then extract with `tar -xf` or Explorer.

### Release smoke test

Each build leg smoke-tests its **packaged archive** (not workspace binaries):
it extracts the archive, runs `wright --version` and `wright-lsp --version`,
asserts both report the tagged version, and compiles/checks the
`synthetic/basic-rule` and `scenarios/loops` fixtures. The publish job
re-verifies that every declared target's archive and checksum are present
before the Release is created.

### Deferred distribution channels

The v0.1 binary contract deliberately does not solve: crates.io publication,
Homebrew/Scoop/Winget packages, npm wrappers, auto-update behavior,
signed/notarized installers, or independent crate-by-crate versioning. Those
can be added after the binary contract is proven by real consumers
(e.g. `wrightkit/agent-lab`).
