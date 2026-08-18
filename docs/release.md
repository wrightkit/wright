# Wright Release Process and Distribution Contract

Status: accepted baseline — release automation and distribution contract
Scope: release artifact packaging, validation gates, version stamping, and
public distribution

## Local release artifact

`scripts/release.sh [version]` (default `0.1.0`) produces
`target/wright-<version>.tar.gz` containing:

* the standalone `wright` and `wright-lsp` release binaries;
* `version.json` with the version, `wright-result/v1` contract identity, git
  commit, build timestamp, and the runtime-dependency claim
  (`"requires": { "node": false, "overpy": false }`).

This is the local staging path and the validation suite behind the release-plz
workflow; the reusable GitHub workflow publishes the per-platform archives,
and this script verifies and packages the host platform.

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
`version = "<release version>"` in `[workspace.package]`; every crate inherits it via
`version.workspace = true`). `wright version` / `wright --version` prints the
CLI banner, `wright-lsp --version` prints the LSP banner, and the LSP
`initialize` response carries `serverInfo.version`. Every `wright-result/v1`
envelope carries `wright.version` + `wright.contract`. The release archive's
`version.json` is the authoritative stamp for a shipped artifact.

## Public distribution contract

A merge to `main` drives `.github/workflows/release-plz.yml`. The release
workflow is the single product release path:

1. `release-plz release-pr` maintains a Release PR for the single product
   package `wright-cli`. `release-plz.toml` uses `git_only = true`, so no
   workspace crate is published to crates.io.
2. The Release PR updates the shared workspace version, `Cargo.lock`, and the
   checked-in `dist/` metadata. All workspace crate changes are included in the
   product changelog decision.
3. Merging that Release PR runs `release-plz release`, which creates exactly
   one `vX.Y.Z` tag and a draft GitHub Release. The job passes the release-plz
   tag and merge commit to the reusable `release.yml` workflow.
4. The reusable workflow verifies the tag/revision and version identity, runs
   `scripts/release.sh` and `scripts/verify-dist.py`, builds and smoke-tests the
   native matrix, attaches archives/checksums/manifests/npm tarballs to the
   draft, publishes downstream registries and the Homebrew tap, and only then
   marks the GitHub Release public.

A failure in any gate or required downstream stage leaves the same draft
Release/tag available for a retry; it does not create a new product version.

### Creating a release

The Release PR is the release decision point. Maintainers do not enter a
version, edit version files, create a tag, or dispatch a second workflow for
the normal case. Review and merge the automatically maintained Release PR;
release-plz derives the next version from the shared workspace history and
creates the one product tag/release.

The `release.yml` workflow is reusable and is intentionally not triggered by a
tag or Release event. The default Actions token cannot start a new workflow
from a tag push; passing release-plz outputs through a job dependency keeps
the release in one run.

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
`SHA256SUMS`. Binaries are unstripped release builds; code
signing/notarization remains deferred. Package-manager distribution is
supported through the channels below.

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
shasum -a 256 -c "wright-$VERSION-$TARGET.tar.gz.sha256" # verify before use
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
`synthetic/basic-rule` and `scenarios/loops` fixtures. The upload job
re-verifies that every declared target's archive and checksum are present
before attaching them to the draft Release.

### Repository configuration

Enable Actions to create and approve pull requests, and grant the default
repository `GITHUB_TOKEN` `contents: write` and `pull-requests: write` for the
release-plz workflow. The reusable distribution workflow also needs
`id-token: write` for npm provenance and `packages: write` for GitHub Packages.
Create a protected `release` environment if publication approval is required;
the release job is the only job that uses it.

Configure these optional/required environment secrets:

* `NPM_TOKEN` enables npmjs.org publication. If absent, npmjs.org is skipped.
* `GH_TOKEN` is a fine-grained token with write access to
  `wrightkit/homebrew-tap`; it is required for automatic Homebrew tap updates.
* The workflow's built-in `GITHUB_TOKEN` publishes GitHub Packages and updates
  the draft GitHub Release.

## Supported installation channels

All channels consume the canonical GitHub Release archives above; none of
them rebuild Wright. Metadata lives under [`dist/`](dist/README.md), generated
by `scripts/update-dist-manifests.py`, and is regenerated by the
the release PR maintenance step and again by the `package-manifests` job from the published per-target
checksums, then attached to the Release as
`wright-<version>.homebrew.rb`, `wright-<version>.winget.zip`, and
`wright-<version>.scoop.json`. The `publish-tap` job then pushes the generated
Homebrew formula into `wrightkit/homebrew-tap` automatically; see
[`dist/README.md`](dist/README.md) for the required `GH_TOKEN`
secret and the per-channel publication process and boundaries.

| Channel | Platforms | Installs | Checksum control |
| --- | --- | --- | --- |
| `install.sh` | Linux x86_64, macOS arm64, macOS x86_64 | `wright` + `wright-lsp` into `~/.local/bin` (or `--dir`) | script verifies the published `.sha256` before extraction |
| Homebrew (`wrightkit/tap`) | macOS arm64 + Intel | `wright` + `wright-lsp` formula | per-arch `sha256` in the formula |
| WinGet (`WrightKit.Wright`) | Windows x86_64 | `wright` + `wright-lsp` portable ZIP | `InstallerSha256` in the manifest |
| Scoop (`wrightkit` bucket) | Windows x86_64 | `wright` + `wright-lsp` ZIP | `hash` in the manifest |
| npm (`@wrightkit/wright`) | Linux x86_64, macOS arm64, macOS x86_64, Windows x86_64 | `wright` + `wright-lsp` native binaries via platform npm packages | packaged binary checksums and signatures verified at release packaging |

`install.sh` is the supported Unix installer: it detects the platform (with
explicit failures for unsupported OS/architecture combinations), resolves the
latest stable release by default or an exact `--version` on request,
downloads the archive and checksum, verifies the SHA-256 before extracting,
installs both binaries, and runs a post-install version smoke check. Its
functional behavior is covered by `scripts/test-install.sh` against a mock
release server on Linux and macOS CI.

### npm distribution channel (#121)

Wright distributes native release binaries through npm packages for seamless
integration with Node.js tooling, language clients, and CI agents:

- **Meta package**: `@wrightkit/wright` exposes `wright` and `wright-lsp` in `bin`,
  and exports `getBinaryPath()` for programmatic Node.js / TypeScript consumers.
  It selects the matching native package via `optionalDependencies`.
- **Platform packages**:
  - `@wrightkit/wright-darwin-arm64`: macOS Apple Silicon (`aarch64-apple-darwin`)
  - `@wrightkit/wright-darwin-x64`: macOS Intel (`x86_64-apple-darwin`)
  - `@wrightkit/wright-linux-x64`: Linux x64 (`x86_64-unknown-linux-gnu`)
  - `@wrightkit/wright-win32-x64`: Windows x64 (`x86_64-pc-windows-msvc`)
- **Execution**:
  - `npx wright --version` or `npm install @wrightkit/wright`
  - Zero source compilation or postinstall download scripts; native binaries are packaged directly in the platform tarballs.
  - npm is strictly a package/distribution layer for the native Rust CLI; there is no JavaScript reimplementation.
- **Registries**: the release workflow publishes the same packages to npmjs.org
  when `NPM_TOKEN` is available and to GitHub Packages using the workflow
  `GITHUB_TOKEN`. GitHub Packages installs require an authenticated npm scope
  mapping for `@wrightkit` to `https://npm.pkg.github.com`.

Standalone installations are also updatable in place: `wright update`
consumes the same release artifacts and checksums (no `install.sh`
re-execution, no second release path), verifies the checksum before
replacing `wright` and `wright-lsp`, and refuses to overwrite
package-manager-managed binaries. See [`docs/cli.md`](cli.md) for the
command contract and the `WRIGHT_INSTALL_BASE_URL`/`WRIGHT_API_URL`/
`WRIGHT_INSTALL_OS`/`WRIGHT_INSTALL_ARCH` test overrides it shares with
`install.sh`.

Package-manager availability is not instantaneous: the Homebrew tap is updated
automatically by the `publish-tap` job, while the Scoop bucket and WinGet
community-repository publication require their own external steps, and the
release pipeline does not assume them. Version drift is
detectable: CI runs `scripts/verify-dist.py`, which regenerates the checked-in
metadata for the current workspace version and fails on any mismatch, and the
release workflow generates the attached manifests from the release's own
checksum files. Registry publication checks whether the exact package/version
already exists before publishing, so rerunning the downstream stage reuses the
same release identity.

### Still deferred

The binary contract deliberately does not solve: crates.io publication,
background/automatic update checks or silent startup updates,
signed/notarized installers, MSI/MSIX/APT/RPM packages, or independent
crate-by-crate versioning. Those can be added later when real consumer
evidence justifies their maintenance cost.
