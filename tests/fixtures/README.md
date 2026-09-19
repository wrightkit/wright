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
| `workshop/real-world/overpy-cake.ws` | `wright-driver` semantic comparison test and benchmark | `Zezombye/overpy` `examples/cake.opy`, commit `eea67adbcf6926c4004e35e25ab4be072624a44e`, GPL-3.0-only |
| `workshop/real-world/overpy-client-to-server.ws` | CLI diagnostics-schema test | `Zezombye/overpy` `examples/clientToServer.opy`, commit `eea67adbcf6926c400e35e25ab4be072624a44e`, GPL-3.0-only |
| `workshop/real-world/overpy-pixelart.ws` | CLI analyzer report test | `Zezombye/overpy` `examples/pixelart.opy`, commit `eea67adbcf6926c400e35e25ab4be072624a44e`, GPL-3.0-only |

The source-language owners maintain their own syntax, semantic, reference, and
compatibility test suites. Changes to those contracts belong in `opy-rs` or
`deltin-rs`, not in this directory.
