#!/usr/bin/env bash
# Verify a downloaded CI build identity and prepare native executables.
#
# Usage: verify-build-identity.sh --identity PATH --revision SHA --runner-os OS
#          --wright PATH --wright-lsp PATH

set -euo pipefail

identity="" revision="" runner_os="" wright="" wright_lsp=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --identity) identity="$2" ;;
    --revision) revision="$2" ;;
    --runner-os) runner_os="$2" ;;
    --wright) wright="$2" ;;
    --wright-lsp) wright_lsp="$2" ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
  shift 2
done

for name in identity revision runner_os wright wright_lsp; do
  if [[ -z "${!name}" ]]; then
    echo "missing required argument --${name//_/-}" >&2
    exit 2
  fi
done

# Prints the first mismatching "key: actual" pair, or nothing when all match.
mismatch="$(jq -r \
  --arg revision "$revision" \
  --arg runner_os "$runner_os" \
  '. as $identity
  | {
      revision: $revision,
      runner_os: $runner_os,
      toolchain: "stable",
      profile: "dev",
      packages: ["wright-cli", "wright-lsp"]
    }
  | to_entries
  | map(select($identity[.key] != .value))
  | first(.[] | "\(.key): \($identity[.key] | tojson)") // empty' \
  "$identity")"
if [[ -n "$mismatch" ]]; then
  echo "build identity mismatch for ${mismatch}" >&2
  exit 1
fi

if [[ "$runner_os" != Windows ]]; then
  chmod 755 "$wright" "$wright_lsp"
fi
