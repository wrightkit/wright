# Wright test inputs

These files are small inputs for named Wright-owned tests and smoke checks.
They are not a source-language compatibility corpus and have no recorded
oracle results.

The synthetic Workshop inputs are authored for Wright's analyzer, driver, CLI,
consumer, and benchmark tests. The `opy/basic-rule.opy` input is a minimal
provider-boundary and distribution smoke input.

The retained real-project Workshop inputs are generated artifacts kept for
specific product tests:

| Input | Consumer | Source provenance |
| --- | --- | --- |
| `workshop/real-world/bastion.ow` | `wright-driver` rule-content span and finding-containment test | `OWBastion/Bastion` `src/main.opy`, commit `c010e1a2d468ec7140f474e334067e5ab8d02d89`, generated with `pnpm run build:main:en` (OverPy 9.7.10, Node 24, pnpm 11.10.0); copied verbatim from `wrightkit/workshop-rs` v1.25.2 `crates/workshop-rs/tests/fixtures/real-projects/bastion.ow`, SHA-256 `44e453ddf7f373be65aea82d019abd45dd60f5ecb57c8d1607d3576a8bc60259`; Apache-2.0, see `workshop/real-world/bastion.LICENSE` |
| `workshop/real-world/overpy-cake.ws` | `wright-driver` semantic comparison test and benchmark | `Zezombye/overpy` `examples/cake.opy`, commit `eea67adbcf6926c4004e35e25ab4be072624a44e`, GPL-3.0-only |
| `workshop/real-world/overpy-client-to-server.ws` | CLI diagnostics-schema test | `Zezombye/overpy` `examples/clientToServer.opy`, commit `eea67adbcf6926c400e35e25ab4be072624a44e`, GPL-3.0-only |
| `workshop/real-world/overpy-pixelart.ws` | CLI analyzer report test | `Zezombye/overpy` `examples/pixelart.opy`, commit `eea67adbcf6926c400e35e25ab4be072624a44e`, GPL-3.0-only |

The source-language owners maintain their own syntax, semantic, reference, and
compatibility test suites. Changes to those contracts belong in `opy-rs` or
`deltin-rs`, not in this directory.

`athena/` holds the `simulate` (ADR-0033) inputs: `athena`'s probe programs,
their recorded client captures, the scenario examples, and the
`practice-range` Workshop build — see [athena/README.md](athena/README.md)
for provenance and the refresh rule.
