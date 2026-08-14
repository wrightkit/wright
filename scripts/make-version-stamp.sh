#!/usr/bin/env bash
# Wright release version stamp (#101).
#
# Writes the authoritative release `version.json` for a shipped artifact: the
# implementation version, the `wright-result/v1` contract identity, the git
# commit, the build timestamp, and the runtime-dependency claim. Shared by
# scripts/release.sh and the GitHub release workflow so the local and CI
# stamps cannot drift.
#
# Usage: scripts/make-version-stamp.sh <version> <output.json>

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:?usage: make-version-stamp.sh <version> <output.json>}"
OUT="${2:?usage: make-version-stamp.sh <version> <output.json>}"

COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
BUILT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

mkdir -p "$(dirname "$OUT")"
cat > "$OUT" <<EOF
{
  "version": "$VERSION",
  "contract": "wright-result/v1",
  "commit": "$COMMIT",
  "built": "$BUILT",
  "requires": {"node": false, "overpy": false}
}
EOF
