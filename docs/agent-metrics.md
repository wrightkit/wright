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
(items, per-kind breakdowns, findings per rule code). Agent operations repeat
`--repeats` times (default 5) and CLI commands `--cli-repeats` times
(default 3), since each CLI run is a full pipeline. The CLI `--brief` forms
share their plain command's pipeline, so they run once and record size and
shape but no latency. Correctness stays
assertion-based in the owning test suites — these metrics watch drift in the
agent-facing cost, not behavior.

## Corpus

`corpus.json` versions the project set. Entries are:

- vendored small projects under `benchmarks/metrics/corpus/`, including the
  `seed-*` projects — always available, no network;
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
`baseline-<corpusVersion>.json`. It reports every change and exits 1 only on
the *blocking* ones below; size and shape drift is reported and never fails,
because a fix legitimately moves it and a baseline cannot tell a fix from a
regression.

Blocking (independent of any recorded number):

- a metric or project that stopped measuring (`error`, `skipped`, or a CLI
  envelope with `ok: false` — a refusal is not a result, so it is never
  recorded as a measurement) unless the baseline recorded the identical marker;
- a metric or project that vanished from the run;
- **brief budget**: every `*?brief` metric — the `brief` request form and the
  CLI `--brief` form (#532) for `lint`, `analyze`, and `inspect` — must stay
  within `briefTokenBudget` (600 estimated tokens on the agent surface, 1000
  in the CLI envelope; recorded in each run and baseline).

Drift (reported, not blocking):

- **tokens and counts**: |delta| > 15% **and** > 30 absolute;
- **latencyMs** (see below): machine-relative — `check` derives a machine factor as the
  median run/baseline latency ratio across every comparable metric (≥10
  pairs) and bands each metric against `baseline * factor`; |delta| > 50%
  AND > 1 ms absolute. A uniformly slower or faster host therefore passes,
  while one operation deviating from the fleet is flagged — and the factor
  itself is reported as a warning (a uniform *product* slowdown is
  indistinguishable from a slower machine, so watch that line). Below 10
  comparable pairs the factor is not trusted and the absolute band applies;

Improvements are reported alongside regressions (a smaller surface is a
change reviewers should see too), and added or newly measurable metrics are
drift. A persistent deterministic skip recorded identically in the baseline
stays a warning.

`check` never rewrites the baseline. Refresh `baseline-<corpusVersion>.json`
in a PR that documents the old/new values and the reason when a reported
change is intended. Bumping `corpus.json`'s `version` opens a new baseline
file so old bands stay auditable.

## In CI

The `Metrics drift` job runs `check --skip-latency`: each metric runs
once and only tokens, bytes, and counts are compared. It fails only on the
blocking changes above; drift appears in the job log and does not fail the PR. Wall time on shared
hosted runners varies by about a factor of two between runs, and the machine
factor cannot cancel a shift that differs by operation (small agent
operations and CLI commands moved in opposite directions on the same run), so
a latency band would fail unrelated changes. On pushes to `main`, a separate
non-blocking `Metrics latency (trend)` job runs the full repeated measurement
against the baseline's latency bands and uploads the run as the
`metrics-latency` artifact. Read it as a trend, and run `check` without
`--skip-latency` on a quiet host when timing matters.

## Release metrics

Each release produces one metrics file beside the agent scores so surface
changes can be correlated with benchmark movement. The recorded `wright`
version and `opyProvider` version travel with the run; a provider-version
mismatch against the baseline is reported as a warning, since provider
releases legitimately move OPY outputs.
