#!/usr/bin/env bash
# Synchronize release-please's checked-in version and distribution metadata.

set -euo pipefail

PR_JSON="${PR:-}"
if [[ -z "$PR_JSON" ]]; then
  echo 'No release-please PR requires metadata synchronization.'
  exit 0
fi

pr_number="$(jq -r '.number // empty' <<<"$PR_JSON")"
if [[ -z "$pr_number" ]]; then
  echo 'release-please returned a PR without a number.' >&2
  exit 1
fi

gh pr checkout "$pr_number"
version="$(tr -d '[:space:]' < version.txt)"
python3 scripts/sync-release-version.py --version "$version"
python3 scripts/update-dist-manifests.py --version "$version"
python3 scripts/verify-dist.py

git config user.name 'github-actions[bot]'
git config user.email '41898282+github-actions[bot]@users.noreply.github.com'
git add Cargo.toml Cargo.lock dist
git diff --cached --check
if git diff --cached --quiet; then
  echo "release metadata already synchronized for $version"
  exit 0
fi
git commit -m 'chore(release): synchronize Wright product metadata'
git push
