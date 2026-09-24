#!/usr/bin/env bash
# Validate Wright's single released or candidate workshop-rs dependency contract.

set -euo pipefail

CANDIDATE_SOURCE='^git\+https://github\.com/wrightkit/workshop-rs\.git\?rev=([0-9a-f]{40})#([0-9a-f]{40})$'
REGISTRY_REQUIREMENT='^\^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$'

is_pinned_git_candidate() {
  local source="${1-}"
  [[ "$source" =~ $CANDIDATE_SOURCE ]] && [[ "${BASH_REMATCH[1]}" == "${BASH_REMATCH[2]}" ]]
}

fail() {
  echo "workshop dependency validation failed: $*" >&2
  exit 1
}

main() {
  local root metadata
  root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  if ! metadata="$(cd "$root" && cargo metadata --locked --format-version 1)"; then
    fail "cargo metadata failed"
  fi

  local workshop_count
  workshop_count="$(jq '[.packages[] | select(.name == "workshop-rs")] | length' <<<"$metadata")"
  if [[ "$workshop_count" -ne 1 ]]; then
    local versions
    versions="$(jq -r '[.packages[] | select(.name == "workshop-rs")
      | "\(.version) (\(.source // "unpublished"))"] | join(", ")' <<<"$metadata")"
    fail "expected exactly one resolved workshop-rs package, found $workshop_count: ${versions:-none}"
  fi

  local version source
  version="$(jq -r '.packages[] | select(.name == "workshop-rs") | .version' <<<"$metadata")"
  source="$(jq -r '.packages[] | select(.name == "workshop-rs") | .source // ""' <<<"$metadata")"

  local is_registry=false
  if [[ "$source" == registry+* ]]; then
    is_registry=true
  elif ! is_pinned_git_candidate "$source"; then
    fail "workshop-rs must come from a released registry or pinned git candidate, got ${source:-unpublished}"
  fi

  # One "<package>\t<req>\t<rename>" line per direct workspace consumer.
  local direct
  direct="$(jq -r '
    (.workspace_members) as $members
    | .packages[]
    | select(.id as $id | $members | index($id))
    | .name as $package
    | .dependencies[]
    | select(.name == "workshop-rs")
    | [$package, .req, (.rename // "")] | @tsv' <<<"$metadata")"
  if [[ -z "$direct" ]]; then
    fail "no workspace package directly consumes workshop-rs"
  fi

  local aliases
  aliases="$(awk -F '\t' '$3 != "" { printf "%s%s: %s", sep, $1, $3; sep = ", " }' <<<"$direct")"
  if [[ -n "$aliases" ]]; then
    fail "renamed workshop-rs dependencies are not allowed ($aliases)"
  fi

  local requirements requirement_count requirement listed
  requirements="$(cut -f2 <<<"$direct" | sort -u)"
  requirement_count="$(wc -l <<<"$requirements" | tr -d ' ')"
  requirement="$(head -n1 <<<"$requirements")"
  listed="$(awk -F '\t' '{ printf "%s%s (%s)", sep, $1, $2; sep = ", " }' <<<"$direct")"
  if [[ "$is_registry" == true ]]; then
    if [[ "$requirement_count" -ne 1 || ! "$requirement" =~ $REGISTRY_REQUIREMENT ]]; then
      fail "direct consumers must use one ordinary compatible SemVer requirement, found $listed"
    fi
  elif [[ "$requirement_count" -ne 1 || "$requirement" != "*" ]]; then
    fail "git candidate direct consumers must use '*', found $listed"
  fi

  local consumer_count consumers
  consumer_count="$(wc -l <<<"$direct" | tr -d ' ')"
  consumers="$(cut -f1 <<<"$direct" | LC_ALL=C sort | paste -sd, - | sed 's/,/, /g')"
  echo "workshop-rs contract: $version from $source ($requirement; $consumer_count direct consumers: $consumers)"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
