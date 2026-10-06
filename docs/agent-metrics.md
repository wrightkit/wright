# Agent-facing metrics

- Contract: `wright-agent-metrics/v1` (a metrics run), versioned per corpus as
  `baseline-<corpusVersion>.json`
- Harness: [`benchmarks/metrics/metrics.py`](../benchmarks/metrics/metrics.py)
- Corpus manifest:
  [`benchmarks/metrics/corpus.json`](../benchmarks/metrics/corpus.json)

The metrics harness measures the surface a coding agent actually pays for:
for every `wright-agent/v1` read operation and every common CLI command, per
corpus project, it records output size in bytes and estimated tokens
(`bytes / 4`), median latency over repeated runs, and result-shape counts
(items, per-kind breakdowns, findings per rule code). Correctness stays
assertion-based in the owning test suites — these metrics watch drift in the
agent-facing cost, not behavior.

## Corpus

`corpus.json` versions the project set. Entries are:

- vendored small projects under `benchmarks/metrics/corpus/` plus the
  non-empty benchmark scenario seeds — always available, no network;
- pinned external repositories (`repository` + `commit`), fetched shallow into
  `benchmarks/metrics/.cache/` by `metrics.py fetch` or on first `run`. Their
  source is never vendored into this repository; only derived metrics are
  stored. OPY entries need the OPY provider (`wright update provider opy`);
  without it they report `skipped` instead of failing.

## Commands

```sh
python3 benchmarks/metrics/metrics.py fetch                  # clone pinned entries
python3 benchmarks/metrics/metrics.py run --out metrics.json # measure everything
python3 benchmarks/metrics/metrics.py check                  # compare vs baseline
```

`check` re-runs the corpus and compares every metric against
`baseline-<corpusVersion>.json`, exiting 1 when anything leaves its band:

- **tokens and counts**: |delta| > 15% **and** > 30 absolute;
- **latencyMs**: |delta| > 50% AND > 1 ms absolute (sub-millisecond medians are
  measurement noise at our timer resolution).

Improvements are reported alongside regressions (a smaller surface is a
change reviewers should see too), and structural changes — added, removed,
or newly measurable metrics — flag the same way. Skipped entries warn; they
do not fail.

`check` never rewrites the baseline. Accepting a change means editing
`baseline-<corpusVersion>.json` in a reviewed PR that documents the old/new
values and the reason. Bumping `corpus.json`'s `version` opens a new baseline
file so old bands stay auditable.

## Release metrics

Each release produces one metrics file beside the agent scores so surface
changes can be correlated with benchmark movement. The recorded `wright`
version and `opyProvider` version travel with the run; a provider-version
mismatch against the baseline is reported as a warning, since provider
releases legitimately move OPY outputs.
