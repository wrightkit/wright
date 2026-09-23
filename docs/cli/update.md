# CLI self-update contract

[← CLI contract index](../cli.md)

## `wright update` (self-update)

`wright update` upgrades a **standalone** installation (one created by
`install.sh` or by unpacking a release archive manually) from the canonical
GitHub Release artifacts (the same archives and checksums the installer and
the package-manager manifests consume). It is not a compiler workflow, so it
is text-only and produces no `wright-result/v1` envelope.

* `wright update`: resolve the latest stable release, download the platform
  archive and its published SHA-256 checksum, verify the checksum before
  touching anything, extract, and atomically replace `wright` and
  `wright-lsp` in the running executable's directory, then smoke-check both
  binaries report the new version.
* `wright update --check`: resolve and report whether an update is
  available without modifying the installation.
* `wright update --version <VERSION>`: install an exact version instead of
  the latest stable release. Refuses a downgrade (the installed version is
  newer) with exit 1.

Supported platforms mirror `install.sh`: Linux x86_64 and macOS
(x86_64/arm64), mapped to the release target matrix in `docs/release.md`.
On Windows, standalone self-update is refused with guidance to
`winget upgrade WrightKit.Wright` / `scoop update wright` (exit 3).

Package-manager-managed installations are detected from the executable's
location (Homebrew/Cellar, Scoop, WinGet paths) and refused with guidance to
the channel's own upgrade command (exit 3); `wright update` never overwrites
a binary it does not own. A missing `wright-lsp` next to `wright`, or an
unwritable installation directory, fails with reinstall guidance (exit 4).

Environment overrides (test/advanced hooks, matching `install.sh`):

* `WRIGHT_INSTALL_BASE_URL`: base URL of release artifacts
* `WRIGHT_API_URL`: URL used to resolve the latest release
* `WRIGHT_INSTALL_OS` / `WRIGHT_INSTALL_ARCH`: override platform detection
