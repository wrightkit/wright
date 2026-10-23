#!/usr/bin/env bash

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-$(sed -n '/^\[workspace\.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -n1)}"
[[ -n "$VERSION" ]] || { echo "release version undetermined: pass it explicitly" >&2; exit 1; }
STAMP_DIR="$ROOT/target/release-stamp"
ARTIFACT_DIR="$ROOT/target/release-package"

echo "==> wright release $VERSION (root: $ROOT)"

cd "$ROOT"

echo "==> quality gates"
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-targets --all-features

echo "==> release build"
cargo build --locked --release -p wright-cli -p wright-lsp -p wright-bench
BIN="$ROOT/target/release/wright"
LSP_BIN="$ROOT/target/release/wright-lsp"
BENCH_BIN="$ROOT/target/release/wright-bench"
test -x "$BIN" || { echo "release binary missing"; exit 1; }
test -x "$LSP_BIN" || { echo "release LSP binary missing"; exit 1; }
test -x "$BENCH_BIN" || { echo "release benchmark binary missing"; exit 1; }

echo "==> first-party provider integration"
"$BIN" update provider opy --version 0.1.38
"$BIN" compile "$ROOT/tests/fixtures/opy/basic-rule.opy" >/dev/null

echo "==> benchmarks"
"$BENCH_BIN" > /dev/null

echo "==> standalone proof (Node/OverPy absent from PATH)"
SANDBOX="$(mktemp -d)"
trap 'rm -rf "$SANDBOX"' EXIT
cp "$BIN" "$SANDBOX/wright"
cp "$LSP_BIN" "$SANDBOX/wright-lsp"
(
  export PATH=/usr/bin:/bin
  command -v node >/dev/null 2>&1 && { echo "node unexpectedly present"; exit 1; }
  "$SANDBOX/wright" compile "$ROOT/tests/fixtures/opy/basic-rule.opy" >/dev/null
  "$SANDBOX/wright" compile "$ROOT/tests/fixtures/opy/basic-rule.opy" >/dev/null
  "$SANDBOX/wright" check "$ROOT/tests/fixtures/workshop/synthetic/control-flow.ws" \
    --profile compat >/dev/null
  LSP_VERSION="$("$SANDBOX/wright-lsp" --version)"
  [[ "$LSP_VERSION" == *"$VERSION"* ]] || { echo "lsp version mismatch: $LSP_VERSION"; exit 1; }
  echo "standalone compile/check/version OK without node"
)

echo "==> stamp version and package"
rm -rf "$STAMP_DIR" "$ARTIFACT_DIR"
mkdir -p "$STAMP_DIR" "$ARTIFACT_DIR"
scripts/make-version-stamp.sh "$VERSION" "$STAMP_DIR/version.json"
cp "$BIN" "$ARTIFACT_DIR/wright"
cp "$LSP_BIN" "$ARTIFACT_DIR/wright-lsp"
cp "$STAMP_DIR/version.json" "$ARTIFACT_DIR/version.json"
tar -C "$ARTIFACT_DIR" -czf "$ROOT/target/wright-$VERSION.tar.gz" \
  wright wright-lsp version.json

echo "==> done: target/wright-$VERSION.tar.gz"
