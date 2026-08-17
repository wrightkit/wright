# Wright Compatibility Matrix and Release Gates

Status: accepted baseline (release gates and compatibility matrix, ADR-0008)
Scope: frozen input surfaces, target/runtime claims, S/D/N/E gate
thresholds, unsupported constructs, and intentional differences

## Frozen input surfaces

| Surface | Owner | Documented by |
| --- | --- | --- |
| Native `.opy` (lexer/preprocess/parser/resolve/lower/settings) | `wright-opy` | [`opy/support-matrix.md`](opy/support-matrix.md) |
| Native OSTW (lexer/parser/CST/project/lower) | `wright-ostw` | [`ostw/compatibility-baseline.md`](ostw/compatibility-baseline.md) |
| Localized Workshop text (catalog/lexer/parser/emitter) | `workshop-rs` (via `wright-workshop` adapter) | [`workshop/support-matrix.md`](workshop/support-matrix.md) |
| Driver/CLI result contract | `wright-driver`/`wright-cli` | [`cli.md`](cli.md) |

Supported Workshop target: the Overwatch Workshop surface evidenced by the
corpus (`compatibility/fixtures/**`), pinned OverPy 9.7.10 as the reference
oracle, en-US locale (additional locales are a data change).

## Compatibility levels and gates

| Gate | Claim | Evidence set | Status |
| --- | --- | --- | --- |
| S: syntax | Native and reference agree on accept/reject for the corpus; accepted inputs classify into the same supported subset | `crates/wright-opy/tests/differential.rs` (HIR parity, full corpus) | PASS |
| D: diagnostics | Malformed inputs produce the same diagnostic category and source region | Diagnostics fixture (`synthetic/diagnostics`) both reject with a parse error at the same line; structured `wright-result/v1` diagnostics | PASS |
| N: normalized output | Compiled Workshop text equals the reference after the documented normalizer | `scripts/v1-gates.py` report (`target/v1-gates-report.json`); `compat` profile | PASS with documented debug/print differences (below) |
| E: semantic | High-risk behaviors have repeatable scenario evidence | `scripts/run-scenarios.py` (`target/scenarios-report.json`) | PASS (compile-time WIR evidence; client execution is out of scope) |

The compatibility contract does **not** claim:
* compatibility outside the declared corpus surface;
* historical OverPy feature breadth beyond the declared matrix;
* client-side runtime equivalence beyond the recorded scenario evidence.

> **Semantic Priority Note (ADR-0008):** N-level gate status reflects
> normalized-output comparison evidence. Presentation-only N-level differences
> (e.g. the documented `debug()`/`print()` formatting difference below) are not
> product bugs and must not automatically create implementation work.
> Observable Workshop behavior, valid syntax, and declared tooling contracts
> outrank text-output identity.

## Intentional differences (documented, not silent)

1. **`debug()`/`print()` formatting.** The reference renders `debug(x)` as a
   type-aware `Create HUD Text` with expression source text and a padded
   layout. Wright emits a semantically equivalent but presentation-simpler
   `Create HUD Text(..., Custom String("{0}", x), ..., Visible To and String, ...)`.
   The N-level normalizer maps both forms to a canonical marker and records
   the difference; the emitted behavior (display the value/message as HUD
   text) is preserved.
2. **Variable indexes.** Explicit `.opy` indexes (`globalvar x 100`) are
   honored (overpy-cake parity). Implicit indexes are assigned in
   declaration order, matching the reference for the corpus.
3. **Float formatting.** Floats emit with at most 16 significant digits,
   matching the reference snapshots.

## Unsupported / deferred

* Rule `disabled` source annotations (no corpus evidence).
* Subroutine parameters and default `@Team`/`@Slot` parameter overrides.
* Workshop client locales beyond `en-US` (data pipeline ready; requires localization data review).
* Reparsing emitted `settings` sections in the Workshop frontend (`.ws` decompiler is a non-goal).
* Client-automation for E-level scenarios (deferred until evidence shows it is required).

## Running the gates

```sh
cargo test --workspace --all-targets </dev/null # S/D evidence (differential suite)
python3 scripts/v1-gates.py                     # N report -> target/v1-gates-report.json
python3 scripts/run-scenarios.py                # E report -> target/scenarios-report.json
cargo build --locked -p wright-bench
target/debug/wright-bench                       # resource/regression thresholds
```

Each report records the corpus identity (fixture hashes), the reference
version (pinned OverPy 9.7.10), the Wright commit, and the comparison method,
so every claim is reproducible.
