#!/usr/bin/env bash
# Exercise the CLI's GitHub Actions renderer contract.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WRIGHT="${1:-$ROOT/target/debug/wright}"
TEMP_ROOT="${RUNNER_TEMP:-$ROOT/target/ci-smoke}"
mkdir -p "$TEMP_ROOT"
TEMP_DIR="$(mktemp -d "$TEMP_ROOT/wright-gha-smoke.XXXXXX")"
cleanup() {
  if command -v trash >/dev/null 2>&1; then
    trash "$TEMP_DIR"
  else
    rmdir "$TEMP_DIR" 2>/dev/null || true
  fi
}
trap cleanup EXIT

fixture="$TEMP_DIR/fixture.ws"
stdout="$TEMP_DIR/stdout"
stderr="$TEMP_DIR/stderr"
cat > "$fixture" <<'EOF'
rule ("gha smoke") {
    event {
        Ongoing - Global;
    }
    actions {
        Disable Inspector Recording;
    }
}
EOF

"$WRIGHT" check "$fixture" > "$stdout" 2> "$stderr"
test ! -s "$stdout"
grep -Fq '::group::wright check' "$stderr"
grep -Fq '::endgroup::' "$stderr"
! grep -Fq 'title=Wright summary' "$stderr"
grep -Fq 'Wright `check`: **PASS** (exit 0)' "${GITHUB_STEP_SUMMARY:?GITHUB_STEP_SUMMARY is required}"
