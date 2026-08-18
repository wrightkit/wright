# Wright Package-Manager Distribution (#108, #121)

This directory holds the package-manager, installer, and npm distribution
metadata that make the canonical GitHub Release artifacts installable through
platform-native channels. Nothing in here rebuilds Wright: every manifest and
package consumes the published `wright-<version>-<target-triple>.<ext>`
archives and their `.sha256` checksums from
`https://github.com/wrightkit/wright/releases/download/v<version>/`.

| Channel | File(s) | Consumes |
| --- | --- | --- |
| Unix installer | `../install.sh` (repo root) | Linux/macOS `.tar.gz` + `.sha256`, verified at install time |
| Homebrew | `homebrew/wright.rb` | macOS `.tar.gz` archives (arm64 + x86_64) with per-arch `sha256` |
| WinGet | `winget/manifests/w/WrightKit/Wright/<version>/` | Windows `.zip` with `InstallerSha256` |
| Scoop | `scoop/wright.json` | Windows `.zip` with `hash` |
| npm / npx | `npm/wright/`, `npm/wright-*/` | Native release binaries packaged directly into platform npm packages |

Standalone installs (the Unix installer or manual archives) upgrade in place
with `wright update`, which consumes the same release archives and checksums
and refuses to overwrite binaries managed by any channel above; see
[`docs/cli.md`](../docs/cli.md).

## Generated metadata

All manifest files under `dist/` are generated or kept synchronized by
`scripts/update-dist-manifests.py` and must not be edited by hand:

```sh
python3 scripts/update-dist-manifests.py --version 0.1.0 \
  --linux-x64-hash <sha256> --darwin-arm64-hash <sha256> \
  --darwin-x64-hash <sha256> --windows-x64-hash <sha256>
```

Between releases the checked-in files carry the current workspace version
with all-zero placeholder hashes. `scripts/verify-dist.py` (run in CI) fails
when the committed metadata drifts from the workspace version or when the
install script stops covering the declared target matrix.

## Publication process

The release-plz workflow creates one draft GitHub Release and calls the
reusable release workflow (`release.yml`) with its tag and merge commit. The
workflow keeps the Release draft until native and package-manager distribution
stages complete:

1. `package-manifests` regenerates the Homebrew, WinGet, and Scoop manifests from
   the native release checksums and attaches them to the draft Release as:
   - `wright-<version>.homebrew.rb`
   - `wright-<version>.winget.zip` (unzip into a winget-pkgs checkout)
   - `wright-<version>.scoop.json`

2. `package-npm` packages the release binaries into platform-native npm packages
   via `scripts/package-npm.py`, runs smoke tests on the packaged artifacts,
   attaches the `.tgz` tarballs to the draft Release, and publishes the same
   tarballs to npmjs.org and GitHub Packages.

3. `publish-release` marks the draft Release public only after the Homebrew tap
   and registry jobs succeed. Re-running an already completed registry stage
   skips package versions that already exist.

### Homebrew

- Repository: `wrightkit/homebrew-tap` (create it once under the WrightKit
  organization; taps are plain git repositories with the formula at the root).
- Publication is automatic: the `publish-tap` job of the release workflow
  pushes the generated `wright.rb` (the same formula attached to the Release
  as `wright-<version>.homebrew.rb`) into the tap on every release. It needs a
  fine-grained PAT with `Contents: Read and write` on `wrightkit/homebrew-tap`,
  stored as the `GH_TOKEN` Actions secret available to this repository. The
  formula downloads the exact published macOS archives for both Apple Silicon
  and Intel and installs `wright` and `wright-lsp`; Homebrew verifies the
  per-arch `sha256` before extraction.
- User experience: `brew install wrightkit/tap/wright`.
- The tap repository must never build Wright from source or bottle it.

### WinGet

- Upstream repository: `microsoft/winget-pkgs` (community review, Windows
  Package Manager Community Repository).
- Per release: unzip the attached `wright-<version>.winget.zip` into a
  `winget-pkgs` checkout and open a PR adding
  `manifests/w/WrightKit/Wright/<version>/`. The installer manifest declares
  the release ZIP as a zip of portable executables
  (`NestedInstallerType: portable` with `PortableCommandAlias` for `wright`
  and `wright-lsp`) and pins `InstallerSha256`.
- Availability is not instantaneous: the package appears in WinGet only after
  the upstream PR is merged and indexed. Until then, point users at
  `install.sh` (WSL), Scoop, or the manual ZIP.
- User experience: `winget install WrightKit.Wright`.

### Scoop

- Repository: `wrightkit/scoop-bucket` (create it once under the WrightKit
  organization; buckets are plain git repositories with manifests under
  `bucket/`).
- Per release: copy the attached `wright-<version>.scoop.json` into the
  bucket as `wright.json` and commit it. The manifest pins the release
  ZIP and `hash`, uses `extract_dir` for the versioned payload directory, and
  carries `checkver`/`autoupdate` so `scoop update` keeps the bucket in sync
  with new Wright releases.
- User experience: `scoop bucket add wrightkit
  https://github.com/wrightkit/scoop-bucket && scoop install wright`.

### npm / npx (#121)

Wright distributes native binaries via npm for seamless integration with Node.js
tooling and CI environments without requiring Rust/Cargo compilation or
postinstall download scripts.

- **Meta package**: `@wrightkit/wright` exposes `wright` and `wright-lsp` CLI
  binaries and exports `getBinaryPath()` for programmatic Node.js consumers.
  It selects the matching native platform package via `optionalDependencies`.
- **Platform packages**:
  - `@wrightkit/wright-darwin-arm64` (macOS Apple Silicon)
  - `@wrightkit/wright-darwin-x64` (macOS Intel)
  - `@wrightkit/wright-linux-x64` (Linux x64)
  - `@wrightkit/wright-win32-x64` (Windows x64)
- **User experience**:
  - Direct execution: `npx wright --version`
  - Project dependency: `npm install @wrightkit/wright`
- **Programmatic usage**:
  ```javascript
  const { getBinaryPath } = require('@wrightkit/wright');
  const wrightBin = getBinaryPath('wright');
  ```
- **Boundary**: npm is strictly a package/distribution layer for the native Rust
  CLI; there is no JavaScript reimplementation of the Wright compiler.
- **Registries**: the release workflow publishes to npmjs.org when `NPM_TOKEN`
  is configured, and always publishes to GitHub Packages with the workflow
  `GITHUB_TOKEN`. GitHub Packages consumers must configure the `@wrightkit`
  scope to use `https://npm.pkg.github.com` and authenticate with GitHub.

## Drift detection

- CI runs `scripts/verify-dist.py` on every commit: it regenerates the
  checked-in manifests with the workspace version and fails on any mismatch,
  so a version bump without regenerated metadata is caught before merge.
- CI also runs `scripts/test-npm.py` to package and execute clean-install smoke
  tests across supported platforms.
- The release workflow consumes the published `.sha256` files when generating
  the attached manifests, so the attached metadata cannot drift from the
  actual artifacts of that release.
- Versioned release assets (`wright-<version>.*`) keep release-to-release
  comparison and manual publication review simple.
