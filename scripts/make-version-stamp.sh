#!/usr/bin/env bash
# Wright release version stamp (#101).
#
# Compatibility wrapper for callers that already invoke the shell entrypoint.
# The authoritative implementation is Python so release packaging has identical
# behavior on Linux, macOS, and Windows without depending on a Bash executable.
#
# Usage: scripts/make-version-stamp.sh <version> <output.json>

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
exec python3 "$ROOT/scripts/make-version-stamp.py" "$@"
