# ADR-0026: The agent benchmark lives with the benchmark suite

- Status: Accepted
- Date: 2026-10-13
- Related: [Issue #665](https://github.com/wrightkit/wright/issues/665), [Issue #669](https://github.com/wrightkit/wright/issues/669), [Issue #670](https://github.com/wrightkit/wright/issues/670), [SPEC-414](../specs/SPEC-414-agent-benchmark-comparison.md) Q-004, [SPEC-534](../specs/SPEC-534-seeded-defect-injection.md), [Agent benchmark](../agent-benchmark.md), [Agent-facing metrics](../agent-metrics.md)

## Context

`benchmarks/` in this repository holds three different things:

- `benchmarks/agent/` and `benchmarks/defects/`: the agent benchmark harness, adapters, grading, reporting, result publishing, training scenarios, the public sample, and the defect generator. Results are stochastic, the harness has its own contract versions, and it reaches Wright only through the released binary and its public contracts.
- `benchmarks/metrics/`: the deterministic agent-facing size check, which must change together with the output it measures.
- `crates/wright-bench`: a performance benchmark built on `wright_driver` internals.

The held-out scenarios already live in the private `wrightkit/benchmark-suite`, whose README requires a checkout of "the matching Wright harness". The instrument therefore lives inside, and changes in the same commits as, the product it measures, and the private suite depends on this repository's Python code. Because `benchmarks/**` counts as a core path, harness-only changes run the full Rust CI, and every product change runs the harness tests and installs its publishing dependencies.

## Decision

1. **The agent benchmark moves to `wrightkit/benchmark-suite`.** The harness, adapters, grading, reporting, publishing, training scenarios, the public sample, the defect generator and their documentation (including SPEC-414 and SPEC-534) leave this repository.
2. **The benchmark pins the Wright it measures.** It runs against an explicit Wright release or binary; moving to a new Wright is its own change in the benchmark repository. Wright's CI no longer runs the harness.
3. **Held-out scenarios stay separated inside the suite.** They live in their own directory; harness tests use public fixtures only, and the repository's agent instructions keep held-out content out of an agent's reads except through validation.
4. **`benchmarks/metrics/` and `crates/wright-bench` stay in Wright.** Product tests and the metrics corpus stop reading benchmark files first (#669).
5. **Reproduction outside the organization is not a requirement.** The published score bundles remain the public output.

## Consequences

- One instrument version can compare several Wright versions.
- A Wright change that moves agent behavior is detected by the benchmark after release instead of being adjusted alongside it.
- Wright's CI stops paying for harness tests on product changes.
- The repository routing table in `wrightkit/.github` gains the benchmark suite as the owner of the agent benchmark.
- Evaluation work in `agent-lab` is consolidated into the same home rather than kept as a third location.

## Compatibility impact

None for Wright's product contracts. Benchmark contracts (`wright-agent-bench`, `wright-agent-score`, `wright-agent-results`) move with the harness; published results keep their schema.

## Scope boundaries

- Which metrics corpus projects Wright keeps is decided in #669.
- The suite's internal layout beyond the held-out separation is the benchmark repository's decision.
