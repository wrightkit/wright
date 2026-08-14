#!/usr/bin/env bash
# Wright standalone release build (#54).
#
# Builds the release artifacts, runs every v1 gate, proves standalone
# operation without Node/OverPy, and packages the artifacts with version
# metadata. The gates must pass before the release is stamped. This is the
# local staging path and the validation suite behind the public tag-driven
# release workflow (issue #101); the GitHub workflow publishes the per-target
# archives, this script verifies and packages the host platform.
#
# Usage: scripts/release.sh [version]

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-0.1.0}"
STAMP_DIR="$ROOT/target/release-stamp"
ARTIFACT_DIR="$ROOT/target/release-package"

echo "==> wright release $VERSION (root: $ROOT)"

cd "$ROOT"

echo "==> quality gates"
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features

echo "==> release build"
cargo build --release -p wright-cli -p wright-lsp
BIN="$ROOT/target/release/wright"
LSP_BIN="$ROOT/target/release/wright-lsp"
test -x "$BIN" || { echo "release binary missing"; exit 1; }
test -x "$LSP_BIN" || { echo "release LSP binary missing"; exit 1; }

echo "==> N-level gate (compat profile)"
python3 scripts/v1-gates.py --wright "$BIN"

echo "==> E-level scenarios"
python3 scripts/run-scenarios.py --wright "$BIN"

echo "==> benchmarks"
cargo run -p wright-bench --release > /dev/null

echo "==> standalone proof (Node/OverPy absent from PATH)"
SANDBOX="$(mktemp -d)"
mkdir -p "$SANDBOX"
cp "$BIN" "$SANDBOX/wright"
cp "$LSP_BIN" "$SANDBOX/wright-lsp"
(
  export PATH=/usr/bin:/bin
  command -v node >/dev/null 2>&1 && { echo "node unexpectedly present"; exit 1; }
  "$SANDBOX/wright" compile "$ROOT/compatibility/fixtures/synthetic/basic-rule/source.opy" \
    --profile compat >/dev/null
  "$SANDBOX/wright" compile "$ROOT/compatibility/fixtures/real-world/overpy-cake/source.opy" \
    --profile compat >/dev/null
  "$SANDBOX/wright" check "$ROOT/scenarios/loops.opy" --profile compat >/dev/null
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
