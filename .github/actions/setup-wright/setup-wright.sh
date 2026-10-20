#!/usr/bin/env bash
# Helper for the setup-wright composite Action (#717).
#
# Subcommands: resolve | verify-cache | install | activate
# Environment:
#   WRIGHT_VERSION           exact version or "latest" (resolve)
#   WRIGHT_INSTALL_BASE_URL  release distribution base URL (test hook; defaults
#                            to the R2 contract in docs/release.md)
#   WRIGHT_SETUP_OS / WRIGHT_SETUP_ARCH  override platform detection (tests)
#   WRIGHT_SETUP_DIR         cache directory holding the verified executable
#   WRIGHT_SETUP_VERSION     concrete version (verify-cache, install)
#   WRIGHT_SETUP_TARGET      target triple (install)
#   GITHUB_OUTPUT / GITHUB_PATH  standard Actions files

set -euo pipefail

BASE_URL="${WRIGHT_INSTALL_BASE_URL:-https://install.wrightkit.dev/wright}"
# Bump when the cached layout (executable plus .sha256 sidecar) changes.
CACHE_LAYOUT="v1"
SEMVER='^[0-9]+\.[0-9]+\.[0-9]+$'

fail() {
  echo "::error::setup-wright: $*" >&2
  exit 1
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  else
    shasum -a 256 "$1" | awk '{ print $1 }'
  fi
}

output() {
  echo "$1=$2" >> "${GITHUB_OUTPUT:?GITHUB_OUTPUT is not set}"
}

exe_name() {
  case "$WRIGHT_SETUP_TARGET" in *windows*) echo wright.exe ;; *) echo wright ;; esac
}

cmd_resolve() {
  local os="${WRIGHT_SETUP_OS:-${RUNNER_OS:-}}" arch="${WRIGHT_SETUP_ARCH:-${RUNNER_ARCH:-}}" target ext
  case "$os/$arch" in
    Linux/X64)   target=x86_64-unknown-linux-gnu;   ext=tar.gz ;;
    macOS/X64)   target=x86_64-apple-darwin;        ext=tar.gz ;;
    macOS/ARM64) target=aarch64-apple-darwin;       ext=tar.gz ;;
    Windows/X64) target=x86_64-pc-windows-msvc;     ext=zip ;;
    *) fail "unsupported platform $os/$arch (supported: Linux X64, macOS X64, macOS ARM64, Windows X64)" ;;
  esac

  local version="${WRIGHT_VERSION:-latest}"
  version="${version#v}"
  if [[ "$version" == latest ]]; then
    version="$(curl -fsSL "$BASE_URL/latest/version" | tr -d '[:space:]')" \
      || fail "could not read $BASE_URL/latest/version; pin an exact version"
    version="${version#v}"
  fi
  [[ "$version" =~ $SEMVER ]] || fail "invalid Wright version '$version' (expected MAJOR.MINOR.PATCH or 'latest')"

  output version "$version"
  output target "$target"
  output archive-ext "$ext"
  output cache-key "setup-wright-$CACHE_LAYOUT-$version-$target"
  output cache-dir "${RUNNER_TEMP:?RUNNER_TEMP is not set}/setup-wright/$CACHE_LAYOUT/$version/$target"
}

# A restored directory is trusted only if its executable matches the sidecar
# SHA-256 written at install time and reports the requested version.
cmd_verify_cache() {
  local dir="${WRIGHT_SETUP_DIR:?}" version="${WRIGHT_SETUP_VERSION:?}" exe
  exe="$(WRIGHT_SETUP_TARGET="${WRIGHT_SETUP_TARGET:?}" exe_name)"
  if [[ -f "$dir/$exe" && -f "$dir/$exe.sha256" ]] \
    && [[ "$(sha256_of "$dir/$exe")" == "$(awk 'NR == 1 { print $1 }' "$dir/$exe.sha256")" ]] \
    && "$dir/$exe" --version 2>/dev/null | grep -Fq "$version"; then
    output valid true
    return
  fi
  if [[ -e "$dir" ]]; then
    echo "::warning::setup-wright: ignoring invalid cached Wright $version; downloading the release instead"
    rm -rf "$dir"
  fi
  output valid false
}

cmd_install() {
  local work
  local dir="${WRIGHT_SETUP_DIR:?}" version="${WRIGHT_SETUP_VERSION:?}" target="${WRIGHT_SETUP_TARGET:?}" ext="${WRIGHT_SETUP_EXT:?}"
  local name="wright-$version-$target" archive exe expected actual
  archive="$name.$ext"
  exe="$(exe_name)"
  work="$(mktemp -d)"
  WORK_DIR="$work"
  trap 'rm -rf "$WORK_DIR"' EXIT

  curl -fsSL -o "$work/$archive" "$BASE_URL/releases/$version/$archive" \
    || fail "failed to download $BASE_URL/releases/$version/$archive (does release $version exist?)"
  curl -fsSL -o "$work/$archive.sha256" "$BASE_URL/releases/$version/$archive.sha256" \
    || fail "failed to download the checksum for $archive"

  expected="$(awk 'NR == 1 { print $1 }' "$work/$archive.sha256")"
  [[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || fail "invalid checksum file for $archive"
  actual="$(sha256_of "$work/$archive")"
  [[ "$(printf %s "$actual" | tr A-F a-f)" == "$(printf %s "$expected" | tr A-F a-f)" ]] \
    || fail "checksum mismatch for $archive (published $expected, got $actual); nothing was installed"

  if [[ "$ext" == zip ]]; then
    # Git Bash's GNU tar cannot read zip; Windows ships bsdtar, which needs native paths.
    "${SYSTEMROOT:-C:/Windows}/System32/tar.exe" -xf "$(cygpath -w "$work/$archive")" -C "$(cygpath -w "$work")" \
      || fail "failed to extract $archive"
  else
    tar -xf "$work/$archive" -C "$work" || fail "failed to extract $archive"
  fi
  [[ -f "$work/$name/$exe" ]] || fail "unexpected archive layout: $name/$exe missing from $archive"

  rm -rf "$dir"
  mkdir -p "$dir"
  cp "$work/$name/$exe" "$dir/$exe"
  chmod +x "$dir/$exe"
  sha256_of "$dir/$exe" > "$dir/$exe.sha256"
  "$dir/$exe" --version | grep -Fq "$version" \
    || { rm -rf "$dir"; fail "installed executable does not report version $version"; }
}

cmd_activate() {
  local dir="${WRIGHT_SETUP_DIR:?}"
  if command -v cygpath >/dev/null 2>&1; then dir="$(cygpath -w "$dir")"; fi
  echo "$dir" >> "${GITHUB_PATH:?GITHUB_PATH is not set}"
}

case "${1:-}" in
  resolve) cmd_resolve ;;
  verify-cache) cmd_verify_cache ;;
  install) cmd_install ;;
  activate) cmd_activate ;;
  *) fail "usage: setup-wright.sh resolve|verify-cache|install|activate" ;;
esac
