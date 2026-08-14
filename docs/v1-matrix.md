# Wright v1 Compatibility Matrix and Release Gates

Status: v1 release contract (milestone M8, issue #49); semantic compatibility
priority clarified post-M11 by [ADR-0008](adr/0008-tooling-first-semantic-platform.md)
Scope: the frozen input surfaces, target/runtime claims, S/D/N/E gate
thresholds, unsupported constructs, and intentional differences of the v1
release

## Frozen v1 input surfaces

| Surface | Owner | Frozen by |
| --- | --- | --- |
| Native `.opy` (lexer/preprocess/parser/resolve/lower) | `wright-opy` | `docs/opy/support-matrix.md` (M7) |
| Localized Workshop text (catalog/lexer/parser/emitter) | `wright-workshop` | `docs/workshop/support-matrix.md` (M5) |
| Driver/CLI result contract | `wright-driver`/`wright-cli` | `docs/cli.md` (M6) |

Supported Workshop target: the Overwatch Workshop surface evidenced by the
v1 corpus (`compatibility/fixtures/**`), pinned OverPy 9.7.10 as the
reference oracle, en-US locale (additional locales are a data change).

## Compatibility levels and gates

| Gate | Claim | Evidence set | Status (v0.3) |
| --- | --- | --- | --- |
| S — syntax | Native and reference agree on accept/reject for the corpus; accepted inputs classify into the same supported subset | `crates/wright-opy/tests/differential.rs` (HIR parity, full corpus) | PASS |
| D — diagnostics | Malformed inputs produce the same diagnostic category and source region | Diagnostics fixture (`synthetic/diagnostics`) both reject with a parse error at the same line; structured `wright-result/v1` diagnostics | PASS |
| N — normalized output | Compiled Workshop text equals the reference after the documented normalizer | `scripts/v1-gates.py` report (`target/v1-gates-report.json`); `compat` profile | PASS with documented debug/print differences (below) |
| E — semantic | High-risk behaviors have repeatable scenario evidence | `scripts/run-scenarios.py` (`target/scenarios-report.json`) | PASS (compile-time WIR evidence; client execution is out of scope, see below) |

The v1 release does **not** claim:
* compatibility outside the declared corpus surface;
* historical OverPy feature breadth beyond the frozen matrix;
* client-side runtime equivalence beyond the recorded scenario evidence.

> **Post-M11 priority note (ADR-0008):** N-level gate status reflects
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

## Unsupported / deferred (v1)

* Custom-game-settings blocks (outside the HIR corpus boundary; the adapter
  rejects them too).
* `%`/`**`/`//` arithmetic operators in `.opy` (no corpus evidence; explicit
  `unsupported-operator` diagnostics).
* Additional locales, rule-disabled markers, subroutine parameters, named
  arguments.
* Client-automation for E-level scenarios (deferred until evidence shows it
  is required).

## Running the gates

```sh
cargo test --workspace --all-targets          # S/D evidence (differential suite)
python3 scripts/v1-gates.py                   # N report -> target/v1-gates-report.json
python3 scripts/run-scenarios.py              # E report -> target/scenarios-report.json
cargo run -p wright-bench --bin wright-bench  # resource/regression thresholds
```

Each report records the corpus identity (fixture hashes), the reference
version (pinned OverPy 9.7.10), the Wright commit, and the comparison method,
so every claim is reproducible.
