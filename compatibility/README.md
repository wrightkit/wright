# Wright Compatibility Regression Evidence

This directory contains **Wright consumer regression evidence** for the source
and integration paths that still exist in this repository. It does not own the
live upstream compatibility oracles for OPY or DEL/OSTW.

Repository ownership is defined by
[`ADR-0009`](../docs/adr/0009-language-ownership-licensing-boundaries.md):

- [`wrightkit/opy-rs`](https://github.com/wrightkit/opy-rs) owns OPY language
  semantics and the pinned OverPy oracle/corpus used to establish OPY
  compatibility evidence;
- [`wrightkit/deltin-rs`](https://github.com/wrightkit/deltin-rs) owns DEL/OSTW
  language semantics and the pinned OSTW reference evidence;
- [`wrightkit/workshop-rs`](https://github.com/wrightkit/workshop-rs) owns
  canonical Workshop semantics and WIR;
- Wright owns tooling, orchestration, provider integration, and regression
  coverage for the language paths it currently consumes.

## What remains in Wright

`fixtures/` contains immutable source inputs plus **recorded reference
snapshots** used by current Wright migration/product regression tests. The
historical filename `oracle.json` is retained as a data-format compatibility
choice; its presence does not mean Wright owns or executes the upstream oracle.

Each fixture has the form:

```text
compatibility/fixtures/<category>/<name>/
  fixture.json   # fixture identity, expected status, and provenance
  source.opy     # input, or the source path named by fixture.json
  oracle.json    # immutable recorded reference result consumed by Wright tests
```

Wright validates these records without installing an upstream compiler:

```sh
python3 -m unittest discover -s compatibility/tests
```

`compatibility/tests/test_evidence.py` checks fixture identity, source hashes,
expected status, snapshot structure, output hashes, and imported-source
provenance. It deliberately has no Node, .NET, OverPy, or OSTW runtime
dependency.

Current Wright-native differential and release-gate tests may consume selected
recorded snapshots while `wright-opy` remains the shipped source adapter.
DEL/OSTW compatibility evidence stays in `deltin-rs`; Wright has no static
DEL/OSTW integration and reports an unavailable provider boundary. These tests
protect Wright's current integration behavior; they are not the authoritative
language compatibility suite.

## Updating OPY reference evidence

Do not regenerate OPY reference results in Wright. The live OverPy pin,
acquisition environment, oracle runner, corpus evolution, differential
expectations, and support matrix are owned by `wrightkit/opy-rs`.

When Wright needs updated OPY evidence:

1. establish and review the reference change in `opy-rs`;
2. import only the immutable evidence required by a Wright consumer regression;
3. preserve source identity, reference identity, hashes, and provenance; and
4. update the Wright regression that consumes the evidence in the same change.

Wright does not carry its own OverPy npm package, lockfile, live oracle runner,
or generic owner-side differential harness.

## OSTW owner boundary

`deltin-rs` is the durable owner of the pinned OSTW reference identity,
corpus, probes, recorded observations, reconstruction boundary, provenance, and
reproduction workflow. Wright does not carry an OSTW oracle, corpus, live
reference runner, or static OSTW integration. DEL/OSTW inputs are covered only
by the explicit `source-provider-unavailable` boundary.

## Fixture provenance

Every redistributed fixture must have reviewable provenance. `fixture.json`
records at least:

- unique fixture `id` and source path;
- expected compile status;
- origin and license;
- whether redistribution is permitted; and
- for imported sources, an immutable source commit and modification status.

Third-party code, generated output, or user data must not be added without a
redistribution/provenance review. When source cannot be redistributed, record a
hash or acquisition procedure in the owning repository instead of committing
it here.

## CI boundary

Default Wright CI may validate:

- committed evidence integrity;
- Wright-native migration/product differentials against recorded evidence;
- LPP/provider integration;
- compiler/tooling behavior and distribution contracts.

Default Wright CI must not require live OverPy or OSTW runtimes merely to prove
that an unrelated Wright change is mergeable. Live upstream oracle
reproduction belongs to the corresponding language repository's evidence
workflow.

See [`docs/compatibility.md`](../docs/compatibility.md) for Wright's compatibility
claim model and priority rules.
