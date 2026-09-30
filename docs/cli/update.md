# CLI update contract

[← CLI contract index](../cli.md)

## `wright update` (maintenance)

`wright update` is the single maintenance entry point for the components
Wright itself manages: a standalone installation and the first-party
providers recorded in Wright's provider store. It is not a compiler
workflow, so it is text-only and produces no `wright-result/v1` envelope.

* `wright update`: update every Wright-managed target — the standalone
  installation, plus already-installed first-party providers. It never
  installs providers that are not already installed, and a skipped or failed
  target does not block the remaining ones.
* `wright update --check`: resolve the same targets and report available
  updates without modifying anything.
* `wright update self [--version <VERSION>]`: update only the standalone
  installation through the R2 release distribution — the same
  `latest/version` pointer and immutable `releases/<version>/` archive and
  checksum routes the canonical installers consume (see
  `docs/release.md`). The checksum is verified before anything is replaced,
  then `wright` and `wright-lsp` are swapped atomically as a matched pair and
  smoke-checked against the new version. `--version` installs an exact
  version instead of the latest stable release and refuses a downgrade
  (exit 1).
* `wright update provider`: update installed first-party providers only.
* `wright update provider <NAME> [--version <VERSION>]`: install or update
  the named first-party provider (currently `opy`). Provider updates come
  from the provider release channel and are checksum-verified; a bare update
  keeps an installation that is already at — or newer than — the resolved
  release, and an explicit `--version` is the only downgrade path.

Supported self-update platforms mirror `install.sh`: Linux x86_64 and macOS
(x86_64/arm64), mapped to the release target matrix in `docs/release.md`.
On Windows, standalone self-update is unsupported with guidance to
`winget upgrade WrightKit.Wright` / `scoop update wright`.

Package-manager-managed installations are detected from the executable's
location (Homebrew/Cellar, Scoop, WinGet paths). Through `wright update` the
self target is skipped with guidance to the channel's own upgrade command
while Wright-managed providers still update; the explicit
`wright update self` refuses with exit 3. Wright never overwrites a binary
it does not own. A missing `wright-lsp` next to `wright`, or an unwritable
installation directory, fails the self target with reinstall guidance
(exit 4).

Environment overrides (test/advanced hooks):

* `WRIGHT_INSTALL_BASE_URL`: base URL of the R2-backed Wright release
  distribution (the `latest/version` and `releases/<version>/` routes live
  under it, matching `install.sh` and `install.ps1`)
* `WRIGHT_INSTALL_OS` / `WRIGHT_INSTALL_ARCH`: override self-update platform
  detection (matching `install.sh`)
* `WRIGHT_PROVIDER_DATA_DIR`: provider store root
* `WRIGHT_OPY_PROVIDER_LATEST_URL` / `WRIGHT_OPY_PROVIDER_BASE_URL`: provider
  release routes
