#!/usr/bin/env bash
# Unit tests for the pinned git candidate matcher in check-workshop-dependency.sh.

set -euo pipefail

# shellcheck source=scripts/check-workshop-dependency.sh
source "$(dirname "${BASH_SOURCE[0]}")/check-workshop-dependency.sh"

failures=0
revision="ac5a6a4cf15bfccc5597cfd6ccb7b5028dfd5053"
repo="git+https://github.com/wrightkit/workshop-rs.git"

expect_accept() {
  if ! is_pinned_git_candidate "$1"; then
    echo "FAIL: expected accept: $1" >&2
    failures=$((failures + 1))
  fi
}

expect_reject() {
  if is_pinned_git_candidate "$1"; then
    echo "FAIL: expected reject: $1" >&2
    failures=$((failures + 1))
  fi
}

# Exact revision pin.
expect_accept "$repo?rev=$revision#$revision"

# Git sources without an explicit full revision.
expect_reject "$repo#$revision"
expect_reject "$repo?branch=main#$revision"
expect_reject "$repo?rev=v1.0.0#$revision"
expect_reject "$repo?rev=$revision#0000000000000000000000000000000000000000"
expect_reject "git+https://github.com/other/workshop-rs.git?rev=$revision#$revision"

# Missing source.
expect_reject ""

if [[ "$failures" -ne 0 ]]; then
  echo "$failures check-workshop-dependency test(s) failed" >&2
  exit 1
fi
echo "check-workshop-dependency tests passed"
