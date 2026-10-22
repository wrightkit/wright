#!/bin/sh
set -eu

REPO_ROOT="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
FILTER="$REPO_ROOT/scripts/lib/required-jobs.jq"
CI_YML="$REPO_ROOT/.github/workflows/ci.yml"

SCRATCH="${TMPDIR:-/tmp}/ci-required-jobs.$$"
trap 'rm -rf "$SCRATCH"' EXIT
mkdir -p "$SCRATCH"

spec_json() {
    sed 's/^spec | with_entries.*$/spec/' "$FILTER" | jq -n -f /dev/stdin
}

required_jobs() {
    spec_json | jq -r 'keys[]'
}

expected_needs() {
    { echo 'paths'; required_jobs; }
}

expected_if() {
    spec_json | jq -r --arg job "$1" '
      .[$job]
      | map("needs.paths.outputs." + . + " == '"'"'true'"'"'")
      | join(" || ")
    '
}

ci_result_needs() {
    awk '
        /^  ci-result:/          { in_job = 1 }
        in_job && /^    needs:/  { in_needs = 1; next }
        in_needs && /^      - /  { sub(/^      - /, ""); print; next }
        in_needs                 { exit }
    ' "$CI_YML"
}

job_if() {
    awk -v job="$1" '
        $0 == "  " job ":" { in_job = 1; next }
        in_job && /^    if: / { sub(/^    if: /, ""); print; exit }
        in_job && /^  [a-z]/ { exit }
    ' "$CI_YML"
}

case "${1:-check}" in
    jobs)
        required_jobs
        ;;
    check)
        fail=0

        expected_needs | LC_ALL=C sort >"$SCRATCH/expected"
        ci_result_needs | LC_ALL=C sort >"$SCRATCH/actual"
        if ! diff -u "$SCRATCH/expected" "$SCRATCH/actual" >&2; then
            echo "ci-result.needs drifted from scripts/lib/required-jobs.jq" >&2
            fail=1
        fi

        for job in $(required_jobs); do
            want=$(expected_if "$job")
            got=$(job_if "$job")
            if [ "$got" != "$want" ]; then
                echo "::error file=.github/workflows/ci.yml::$job 'if:' drifted from scripts/lib/required-jobs.jq" >&2
                echo "  expected: $want" >&2
                echo "  found:    $got" >&2
                fail=1
            fi
        done

        exit "$fail"
        ;;
    *)
        echo "usage: $0 check|jobs" >&2
        exit 2
        ;;
esac
