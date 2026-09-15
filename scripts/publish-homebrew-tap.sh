#!/usr/bin/env bash
# Publish the generated Homebrew formula to the dedicated tap repository.

set -euo pipefail

: "${RELEASE_TAG:?RELEASE_TAG is required}"
: "${GH_TOKEN:?GH_TOKEN is required}"
: "${GITHUB_WORKSPACE:?GITHUB_WORKSPACE is required}"
version="${RELEASE_TAG#v}"
formula="$GITHUB_WORKSPACE/formula/wright.rb"
test -f "$formula"
cp "$formula" "$GITHUB_WORKSPACE/tap/wright.rb"
cd "$GITHUB_WORKSPACE/tap"
if git diff --quiet -- wright.rb; then
  echo "wright.rb is already current for $RELEASE_TAG; nothing to push"
  exit 0
fi
git -c user.name="wrightkit-bot" \
    -c user.email="wrightkit-bot@users.noreply.github.com" \
    commit -m "Update wright formula to $version" -- wright.rb
git push origin HEAD
echo "pushed wright.rb for $RELEASE_TAG to wrightkit/homebrew-tap"
