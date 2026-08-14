#!/usr/bin/env bash
# Wright installer for Linux and macOS (#108).
#
# Installs the standalone `wright` and `wright-lsp` binaries from the
# canonical Wright GitHub Release archives. It is a thin release-artifact
# installer, not a package manager or source-build frontend: it resolves the
# platform artifact, verifies the published SHA-256 checksum, extracts the two
# binaries, and smoke-checks the installed version.
#
# Usage:
#   install.sh                   # latest stable release into ~/.local/bin
#   install.sh --version 0.1.0   # exact version (CI/agents)
#   install.sh --dir ~/bin       # custom installation directory
#
# Environment overrides (test/advanced hooks, not the primary interface):
#   WRIGHT_INSTALL_BASE_URL  base URL of release artifacts
#   WRIGHT_API_URL           URL used to resolve the latest release
#   WRIGHT_INSTALL_OS        override OS detection (linux | darwin)
#   WRIGHT_INSTALL_ARCH      override CPU detection (x86_64 | aarch64)

set -euo pipefail

WRIGHT_INSTALL_BASE_URL="${WRIGHT_INSTALL_BASE_URL:-https://github.com/wrightkit/wright/releases/download}"
WRIGHT_API_URL="${WRIGHT_API_URL:-https://api.github.com/repos/wrightkit/wright/releases/latest}"

VERSION=""
INSTALL_DIR=""
TMP_DIR=""

usage() {
  sed -n '2,11p' "$0" | sed 's/^# \?//'
  echo
  echo "Options:"
  echo "  --version <version>  Install an exact Wright version (e.g. 0.1.0);"
  echo "                       defaults to the latest stable release."
  echo "  --dir <directory>    Install into <directory> (default \$HOME/.local/bin)."
  echo "  --help               Show this help."
}

fail() {
  echo "error: $*" >&2
  exit 1
}

cleanup() {
  [[ -n "$TMP_DIR" ]] && rm -rf "$TMP_DIR"
}
trap cleanup EXIT

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "missing dependency '$1'; install it and re-run"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      [[ $# -ge 2 ]] || fail "--version requires a value"
      VERSION="$2"
      shift 2
      ;;
    --dir)
      [[ $# -ge 2 ]] || fail "--dir requires a value"
      INSTALL_DIR="$2"
      shift 2
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      fail "unknown option '$1' (run install.sh --help)"
      ;;
  esac
done

if [[ -n "$VERSION" ]] && ! printf '%s' "$VERSION" | grep -Eq '^v?[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$'; then
  fail "invalid version '$VERSION' (expected semver like 0.1.0)"
fi
VERSION="${VERSION#v}"

echo "==> detecting platform"
OS="${WRIGHT_INSTALL_OS:-}"
ARCH="${WRIGHT_INSTALL_ARCH:-}"
[[ -z "$OS" ]] && case "$(uname -s)" in
  Linux)  OS=linux ;;
  Darwin) OS=darwin ;;
  *)      fail "unsupported operating system '$(uname -s)' (supported: Linux x86_64, macOS arm64, macOS x86_64)" ;;
esac
[[ -z "$ARCH" ]] && case "$(uname -m)" in
  x86_64|amd64)         ARCH=x86_64 ;;
  arm64|aarch64)        ARCH=aarch64 ;;
  *)                    fail "unsupported CPU architecture '$(uname -m)' (supported: x86_64, arm64)" ;;
esac

# Platform -> artifact mapping (must match the release target matrix in
# docs/release.md).
case "$OS/$ARCH" in
  linux/x86_64)  TARGET=x86_64-unknown-linux-gnu ;;
  darwin/x86_64) TARGET=x86_64-apple-darwin ;;
  darwin/aarch64) TARGET=aarch64-apple-darwin ;;
  *)
    fail "unsupported platform $OS/$ARCH (supported: linux/x86_64, darwin/x86_64, darwin/aarch64)"
    ;;
esac
echo "    platform: $OS $ARCH ($TARGET)"

require_command curl
require_command tar
if command -v shasum >/dev/null 2>&1; then
  SHA_CMD=(shasum -a 256)
else
  require_command sha256sum
  SHA_CMD=(sha256sum)
fi

if [[ -z "$VERSION" ]]; then
  echo "==> resolving latest stable release"
  LATEST_JSON="$(curl -fsSL "$WRIGHT_API_URL" 2>/dev/null)" \
    || fail "could not resolve the latest release from $WRIGHT_API_URL (offline or rate-limited?); pin a version with --version"
  VERSION="$(printf '%s' "$LATEST_JSON" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  VERSION="${VERSION#v}"
  [[ -n "$VERSION" ]] || fail "could not parse the latest release tag; pin a version with --version"
  echo "    latest: $VERSION"
fi

ARCHIVE="wright-$VERSION-$TARGET.tar.gz"
ARCHIVE_URL="$WRIGHT_INSTALL_BASE_URL/v$VERSION/$ARCHIVE"
CHECKSUM_URL="$ARCHIVE_URL.sha256"
EXPECTED_DIR="wright-$VERSION-$TARGET"

if [[ -z "$INSTALL_DIR" ]]; then
  INSTALL_DIR="$HOME/.local/bin"
fi

echo "==> downloading $ARCHIVE_URL"
TMP_DIR="$(mktemp -d)"
curl -fsSL -o "$TMP_DIR/$ARCHIVE" "$ARCHIVE_URL" \
  || fail "failed to download $ARCHIVE_URL (does release v$VERSION exist?)"
curl -fsSL -o "$TMP_DIR/$ARCHIVE.sha256" "$CHECKSUM_URL" \
  || fail "failed to download checksum $CHECKSUM_URL"

echo "==> verifying SHA-256 checksum"
EXPECTED_HASH="$(awk 'NR == 1 { print $1 }' "$TMP_DIR/$ARCHIVE.sha256")"
[[ "$EXPECTED_HASH" =~ ^[0-9a-fA-F]{64}$ ]] \
  || fail "invalid checksum file $CHECKSUM_URL"
ACTUAL_HASH="$("${SHA_CMD[@]}" "$TMP_DIR/$ARCHIVE" | awk 'NR == 1 { print $1 }')"
[[ "$ACTUAL_HASH" == "$EXPECTED_HASH" ]] \
  || fail "checksum verification failed for $ARCHIVE (published $EXPECTED_HASH, got $ACTUAL_HASH); the download may be corrupted or tampered with, so nothing was installed"

echo "==> extracting release archive"
tar -xzf "$TMP_DIR/$ARCHIVE" -C "$TMP_DIR" \
  || fail "failed to extract $ARCHIVE"
test -d "$TMP_DIR/$EXPECTED_DIR" \
  || fail "unexpected archive layout: expected directory '$EXPECTED_DIR' inside $ARCHIVE"
test -f "$TMP_DIR/$EXPECTED_DIR/wright" \
  || fail "unexpected archive layout: 'wright' binary missing from $ARCHIVE"
test -f "$TMP_DIR/$EXPECTED_DIR/wright-lsp" \
  || fail "unexpected archive layout: 'wright-lsp' binary missing from $ARCHIVE"

echo "==> installing into $INSTALL_DIR"
if ! mkdir -p "$INSTALL_DIR" 2>/dev/null; then
  fail "install directory $INSTALL_DIR is not writable (permission denied); choose a writable location with --dir (the default $HOME/.local/bin needs no root)"
fi
test -w "$INSTALL_DIR" \
  || fail "install directory $INSTALL_DIR is not writable; choose a writable location with --dir (the default $HOME/.local/bin needs no root)"
cp "$TMP_DIR/$EXPECTED_DIR/wright" "$TMP_DIR/$EXPECTED_DIR/wright-lsp" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/wright" "$INSTALL_DIR/wright-lsp"

echo "==> post-install smoke check"
"$INSTALL_DIR/wright" --version | grep -Fq "$VERSION" \
  || fail "smoke check failed: '$INSTALL_DIR/wright --version' did not report version $VERSION"
"$INSTALL_DIR/wright-lsp" --version | grep -Fq "$VERSION" \
  || fail "smoke check failed: '$INSTALL_DIR/wright-lsp --version' did not report version $VERSION"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "note: add $INSTALL_DIR to your PATH to use wright" >&2 ;;
esac

echo "==> done: wright and wright-lsp $VERSION installed in $INSTALL_DIR"
