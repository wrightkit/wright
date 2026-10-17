# ADR-0028: Transformation profiles apply to Workshop input only

- Status: Accepted
- Date: 2026-10-17
- Related: [Issue #725](https://github.com/wrightkit/wright/issues/725), [ADR-0008](0008-tooling-first-semantic-platform.md), [ADR-0010](0010-independent-implementations-and-wright-integration.md), [Compatibility](../compatibility.md)

## Context

`--profile off|compat|aggressive` selects a pass set that rewrites the loaded Workshop program (WIR) before analysis and emission. Today the only pass is constant folding.

Two facts were checked on a real 324-rule OPY project and on a small fixture:

- For an OPY source, `off`, `compat`, and `aggressive` produce byte-identical output and identical element counts. The OPY compiler already folds constants, so the pass has nothing left to change. A user trying to reduce a project's element count with `--profile` gets nothing, and the flag gives no sign of why.
- For the same arithmetic written as raw Workshop (`Add(Multiply(2, 3), 4)` and similar), `compat` folds it and the element count drops (37 to 17 in the fixture). The profile is useful there.

`aggressive` selects exactly the `compat` pass set. It has no behavior of its own and is documented as an experimental marker.

Separately, goal principle 7 makes the upstream compiler the executable specification for OPY and OSTW output: supported constructs produce the same canonical Workshop structure, and structural rewrites are not accepted even when they look cheaper. Nothing stops a profile from being applied to a source language's compiled output, so a future pass that changes structure would silently break that principle for OPY and OSTW.

## Decision

1. **Profiles apply to Workshop input only.** The pass pipeline runs on raw Workshop input. OPY and OSTW own their compilation, including their own optimization and compression; Wright does not run a second optimization over their output.
2. **`--profile` with an OPY or OSTW input is a usage error** (exit code 2) that says the option applies to Workshop input only. It is not accepted and ignored, because an option that is accepted and does nothing is the confusion this decision removes. The same check applies to `wright serve`: at startup when `--kind` fixes the input type, and per request when the kind is resolved from the input.
3. **`aggressive` is removed.** The values are `off` and `compat`. It has no behavior distinct from `compat`; a value is added back only with a pass that needs it.
4. **`off` remains the default.** `compat` remains the evidence-backed, semantics-preserving pass set for raw Workshop.

## Consequences

- OPY and OSTW compilation can no longer deviate from the upstream compiler through a Wright pass, and a future pass cannot do so by accident. The structural convergence of principle 7 is enforced by where the pipeline runs, not by documentation.
- This is a breaking change to the public CLI and agent contract before 1.0: `--profile aggressive` is rejected, and `--profile compat` on an OPY or OSTW input is rejected. Both previously had no effect on OPY or OSTW output.
- Callers in this repository that pass `--profile compat` with an OPY fixture are updated together with the implementation: `scripts/release.sh`, `scripts/smoke-native.py`, and `.github/workflows/ci.yml`. Calls with Workshop input keep the option.
- Documentation to update with the implementation: `docs/cli/commands.md` and `docs/agent-contract.md` (the profile values and the applicability rule), and `docs/compatibility.md` (OPY and OSTW output is not transformed by a profile).
- Users reducing element count on an OPY project do so in the source or its generators; Wright reports where the cost is (`inspect cost`), it does not rewrite the output.

## Compatibility impact

No change to OPY or OSTW compiled output: with the default profile it was never transformed, and with `compat` the fixture and the real project produced identical bytes. Tests required with the implementation:

- `--profile compat` with an OPY input, and with an OSTW input, exits 2 with the stated reason, on both `compile` and `inspect`; with a Workshop input it is accepted and folds as before.
- `--profile aggressive` is rejected as an unknown value, with `off` and `compat` listed.
- `wright serve` rejects the combination at startup when `--kind` fixes an OPY or OSTW input, and per request when the kind is resolved from the input.
- The smoke and release scripts and the CI workflow pass without `--profile` on OPY fixtures.

## Scope boundaries

- Which Workshop passes ship in `compat`, and any new pass, is decided case by case with before and after evidence; this ADR does not add passes.
- An optional element-reduction facility for any source language is not decided here and is not planned without demonstrated demand and an approved exception under principle 7.
