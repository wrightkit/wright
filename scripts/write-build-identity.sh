#!/usr/bin/env bash
# Write the machine-readable identity for a CI build artifact.
#
# Usage: write-build-identity.sh --output PATH --revision SHA --runner-os OS
#          --toolchain NAME --profile NAME [--target TRIPLE]

set -euo pipefail

output="" revision="" runner_os="" target="" toolchain="" profile=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --output) output="$2" ;;
    --revision) revision="$2" ;;
    --runner-os) runner_os="$2" ;;
    --target) target="$2" ;;
    --toolchain) toolchain="$2" ;;
    --profile) profile="$2" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift 2
done

for name in output revision runner_os toolchain profile; do
  if [[ -z "${!name}" ]]; then
    echo "missing required argument --${name//_/-}" >&2
    exit 2
  fi
done

if [[ -z "$target" ]]; then
  target="$(rustc -vV | sed -n 's/^host: //p' | tr -d '\r')"
fi

mkdir -p "$(dirname "$output")"
jq -cn \
  --arg revision "$revision" \
  --arg runner_os "$runner_os" \
  --arg target "$target" \
  --arg toolchain "$toolchain" \
  --arg profile "$profile" \
  '{
    revision: $revision,
    runner_os: $runner_os,
    target: $target,
    toolchain: $toolchain,
    profile: $profile,
    packages: ["wright-cli", "wright-lsp"],
    features: []
  }' >"$output"
