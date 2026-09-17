# Wright Release Process and Distribution Contract

Status: accepted baseline (release automation and distribution contract)
Scope: release artifact packaging, validation gates, version stamping, and
public distribution

Decision history: provider distribution and local activation are recorded in
[ADR-0012](adr/0012-first-party-provider-distribution.md); the R2 namespace,
immutable artifact, and latest-pointer contract is recorded in
[ADR-0014](adr/0014-namespaced-immutable-r2-artifacts.md).

## Local release artifact

`scripts/release.sh [version]` (default `0.1.0`) produces
`target/wright-<version>.tar.gz` containing:

* the standalone `wright` and `wright-lsp` release binaries;
* `version.json` with the version, `wright-result/v1` contract identity, git
  commit, build timestamp, and the runtime-dependency claim
  (`"requires": { "node": false, "overpy": false }`).

This is the local staging path and the validation suite behind the release-please
workflow; the reusable GitHub workflow publishes the per-platform archives,
and this script verifies and packages the host platform.

## What the release script verifies before stamping

1. **Quality gates**: `cargo fmt --check`, `cargo clippy -D warnings`,
   `cargo test --workspace --all-targets --all-features`.
2. **Semantic OPY integration gate**: `scripts/v1-gates.py` against the
   release binary (`target/v1-gates-report.json`), using the explicitly pinned
   first-party provider release.
3. **Benchmarks**: `wright-bench` with declared regression thresholds
   (`target/wright-bench-report.json`).
4. **Standalone proof**: the packaged binaries run `compile`/`check` over the
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

A merge to `main` drives `.github/workflows/release-please.yml`. The release
workflow is the single product release path:

1. `release-please-action` maintains one root Release PR for the Wright
   product. `release-please-config.json` uses the `simple` release type, with
   `version.txt` and `CHANGELOG.md` as its product-level version and changelog
   files. No workspace crate is published to crates.io. Every workspace
   package explicitly sets `publish = false`, so Cargo package publication
   cannot become an accidental release surface.
2. The Release PR updates the shared workspace version, `Cargo.lock`, and the
   checked-in `dist/` metadata. All workspace crate changes are included in the
   product changelog decision.
3. Merging that Release PR leaves the exact release commit on `main` without
   creating a tag or GitHub Release. The `CI` workflow validates that commit.
4. After that exact `CI` run succeeds, `release-publication.yml` starts the
   reusable `release.yml` workflow. It verifies the version, builds and
   smoke-tests the native matrix, verifies the complete archive/checksum set,
   and generates package-manager manifests.
5. The final `publish-release` job creates the `vX.Y.Z` tag and public GitHub
   Release against that exact commit with the verified archives, checksums,
   and manifests. Only then do R2 and Homebrew publication run.

A failure before GitHub Release creation leaves no public release or tag for
that candidate; rerun the failed release-publication workflow run to retry the
same CI-qualified commit. If R2 publication fails afterward, the public
GitHub Release remains the canonical record but `latest/version` is not
advanced. A retry reuses only
byte-identical immutable R2 objects and refuses any conflicting object.

### Creating a release

The Release PR is the release decision point. Maintainers do not enter a
version, edit version files, create a tag, or dispatch a second workflow for
the normal case. Review and merge the automatically maintained Release PR;
release-please derives the next version from Conventional Commits and records
the version/changelog that the publication workflow later releases.

The `release-publication.yml` workflow is triggered by the completed `CI`
workflow, so successful exact-commit CI is a native prerequisite. The reusable
`release.yml` workflow is intentionally not triggered by a tag or Release
event. Release creation is the final publication-stage job, so the default
Actions token does not need to start a second workflow from a tag push.

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

### R2 installer distribution

`install.sh` uses the WrightKit R2 custom domain by default. GitHub Releases
remain the canonical release record and provenance source; R2 contains exact
copies of the archives and checksum files that the completed GitHub Release
already exposes.

Pinned installs use immutable versioned objects:

```text
https://releases.wrightkit.dev/wright/releases/<version>/wright-<version>-<target-triple>.<ext>
https://releases.wrightkit.dev/wright/releases/<version>/wright-<version>-<target-triple>.<ext>.sha256
```

Latest installs first read `https://releases.wrightkit.dev/wright/latest/version`,
then download the corresponding version-named archive and checksum from
`/releases/<version>/`. The release workflow publicly verifies every versioned
archive/checksum pair before writing that `latest/version` pointer, so the
installer cannot resolve a new version before its complete artifact set is
available. `latest/version` uses `Cache-Control: no-store`; all archive and
checksum paths are version-named and use long-lived immutable caching. This
avoids stale latest pointers without a separate Worker, API, or GitHub Releases
API lookup.

Versioned R2 objects are uploaded with `If-None-Match: *`; retries may reuse an
already-present object only after comparing its bytes to the release artifact.
The workflow retrieves every public copy and checks its bytes and cache headers
before it advances `latest/version`.

### Manual GitHub Release fallback

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

Windows x86_64 users can install the canonical release ZIP with the first-party
PowerShell installer:

```powershell
irm https://raw.githubusercontent.com/wrightkit/wright/main/install.ps1 | iex
```

For a pinned version or custom user-writable directory:

```powershell
& .\install.ps1 -Version 0.1.0 -InstallDir "$env:LOCALAPPDATA\Programs\Wright\bin"
```

The installer verifies the published `.zip.sha256` before extraction, installs
both `wright.exe` and `wright-lsp.exe`, and runs both binaries directly for a
version smoke check. If the install directory is not already on the user
`PATH`, it prints a copyable PowerShell command and asks you to open a new
terminal. It does not require Cargo, npm, or a source checkout.

### Release smoke test

Each build leg smoke-tests its **packaged archive** (not workspace binaries):
it extracts the archive and runs the shared `scripts/smoke-native.py` contract
against the extracted binaries. The contract checks both version banners,
representative OPY compile/check paths, and first-party OPY provider bootstrap
from an empty provider store. The upload job re-verifies that every declared
target's archive and checksum are present before attaching them to the final
GitHub Release.

The normal CI distribution validation uses independent channel legs. It stages
a canonical-shaped local release archive and generated local metadata for
`install.sh` on Linux/macOS, `install.ps1` on Windows, Homebrew on macOS, Scoop
on Windows, and WinGet on Windows. Each leg provisions or configures only its
own package-manager prerequisite, runs that channel's real installation
command, and runs the same native smoke against the installed binaries. These
channel checks are labelled separately from the native runtime smoke and do
not publish or modify any external package-manager repository.

### Repository configuration

Enable Actions to create and approve pull requests. The release-please workflow
uses the repository's `GH_TOKEN` secret as `GITHUB_TOKEN` so it can create and
   update the Release PR.
Create a protected `release` environment if publication approval is required;
the final `publish-release` job is the only job that uses it.

Configure these optional/required environment secrets:

* `GH_TOKEN` is a fine-grained token with write access to
  `wrightkit/homebrew-tap`; it is required for automatic Homebrew tap updates.
* The workflow's built-in `GITHUB_TOKEN` creates the final GitHub Release and
  uploads its verified assets.
* `CLOUDFLARE_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, and
  `R2_SECRET_ACCESS_KEY` grant the release workflow S3 API access to the
  `wrightkit-release` bucket. The bucket must expose `releases.wrightkit.dev`
  as its production custom domain before a release; the workflow verifies that
  public route during publication.

## Supported installation channels

All channels consume canonical released archives and none of them rebuild
Wright. `install.sh` and `install.ps1` consume the R2 copies described above;
the package managers continue to consume GitHub Release archives.
Metadata lives under [`dist/`](../dist/README.md), generated
by `scripts/update-dist-manifests.py`, and is regenerated by the release PR
maintenance step and again by the `package-manifests` job from the published per-target
checksums, then attached to the Release as
`wright-<version>.homebrew.rb`, `wright-<version>.winget.zip`, and
`wright-<version>.scoop.json`. The `publish-tap` job then pushes the generated
Homebrew formula into `wrightkit/homebrew-tap` automatically; see
[`dist/README.md`](../dist/README.md) for the required `GH_TOKEN`
secret and the per-channel publication process and boundaries.

| Channel | Platforms | Installs | Checksum control |
| --- | --- | --- | --- |
| `install.sh` | Linux x86_64, macOS arm64, macOS x86_64 | `wright` + `wright-lsp` into `~/.local/bin` (or `--dir`) | script verifies the published `.sha256` before extraction |
| `install.ps1` | Windows x86_64 | `wright.exe` + `wright-lsp.exe` into `%LOCALAPPDATA%\Programs\Wright\bin` (or `-InstallDir`) | script verifies the published `.sha256` before extraction |
| Homebrew (`wrightkit/tap`) | macOS arm64 + Intel | `wright` + `wright-lsp` formula | per-arch `sha256` in the formula |
| WinGet (`WrightKit.Wright`) | Windows x86_64 | `wright` + `wright-lsp` portable ZIP | `InstallerSha256` in the manifest |
| Scoop (`wrightkit` bucket) | Windows x86_64 | `wright` + `wright-lsp` ZIP | `hash` in the manifest |

`install.sh` is the supported Unix installer: it detects the platform (with
explicit failures for unsupported OS/architecture combinations), resolves the
latest stable release from the R2 `latest` route by default or an exact
`--version` from an immutable versioned route on request, downloads the archive
and checksum, verifies the SHA-256 before extracting, installs both binaries,
and runs a post-install version smoke check. Its functional behavior is covered
by `scripts/test-install.sh` against a mock R2 route on Linux and macOS CI.

`install.ps1` is the supported Windows x86_64 installer. It resolves the latest
stable release by default or an exact `-Version`, downloads the canonical ZIP
and matching `.sha256`, verifies the checksum before extraction, installs both
executables into the user-writable default directory or `-InstallDir`, and
runs a direct native version smoke check. When needed, it prints an actionable
user `PATH` update instruction. Its functional behavior is covered by
`scripts/test-install.ps1` against a local test release server on Windows.


Standalone installations are also updatable in place: `wright update`
continues to consume the canonical GitHub Release artifacts and checksums (no
`install.sh` re-execution, no second build path), verifies the checksum before
replacing `wright` and `wright-lsp`, and refuses to overwrite
package-manager-managed binaries. See [`docs/cli.md`](cli.md) for the
command contract and its `WRIGHT_INSTALL_BASE_URL`/`WRIGHT_API_URL`/
`WRIGHT_INSTALL_OS`/`WRIGHT_INSTALL_ARCH` test overrides. The installer has a
separate R2 route contract and only shares the platform overrides.

Package-manager availability is not instantaneous: the Homebrew tap is updated
automatically by the `publish-tap` job, while the Scoop bucket and WinGet
community-repository publication require their own external steps, and the
release pipeline does not assume them. Version drift is
detectable: CI runs `scripts/verify-dist.py`, which regenerates the checked-in
metadata for the current workspace version and fails on any mismatch, and the
release workflow generates the attached manifests from the release's own
checksum files.

### Still deferred

The binary contract deliberately does not solve: crates.io publication,
background/automatic update checks or silent startup updates,
signed/notarized installers, MSI/MSIX/APT/RPM packages, or independent
crate-by-crate versioning. Those can be added later when real consumer
evidence justifies their maintenance cost.
