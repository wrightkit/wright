#!/usr/bin/env bash
# Wright standalone release build (#54).
#
# Builds the release artifact, runs every v1 gate, proves standalone
# operation without Node/OverPy, and packages the artifact with version
# metadata. The gates must pass before the release is stamped.
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
cargo build --release -p wright-cli
BIN="$ROOT/target/release/wright"
test -x "$BIN" || { echo "release binary missing"; exit 1; }

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
(
  export PATH=/usr/bin:/bin
  command -v node >/dev/null 2>&1 && { echo "node unexpectedly present"; exit 1; }
  "$SANDBOX/wright" compile "$ROOT/compatibility/fixtures/synthetic/basic-rule/source.opy" \
    --profile compat >/dev/null
  "$SANDBOX/wright" compile "$ROOT/compatibility/fixtures/real-world/overpy-cake/source.opy" \
    --profile compat >/dev/null
  "$SANDBOX/wright" check "$ROOT/scenarios/loops.opy" --profile compat >/dev/null
  echo "standalone compile/check OK without node"
)

echo "==> stamp version and package"
rm -rf "$STAMP_DIR" "$ARTIFACT_DIR"
mkdir -p "$STAMP_DIR" "$ARTIFACT_DIR"
cat > "$STAMP_DIR/version.json" <<EOF
{
  "version": "$VERSION",
  "contract": "wright-result/v1",
  "commit": "$(git rev-parse HEAD)",
  "built": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "requires": {"node": false, "overpy": false}
}
EOF
cp "$BIN" "$ARTIFACT_DIR/wright"
cp "$STAMP_DIR/version.json" "$ARTIFACT_DIR/version.json"
tar -C "$ARTIFACT_DIR" -czf "$ROOT/target/wright-$VERSION.tar.gz" wright version.json

echo "==> done: target/wright-$VERSION.tar.gz"
