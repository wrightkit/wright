#!/usr/bin/env bash
# Usage: scripts/make-version-stamp.sh <version> <output.json>

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/make-version-stamp.py" "$@"
