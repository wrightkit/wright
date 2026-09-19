# Upstream reference and license inventory

This document records project identities and licensing context for references
that Wright studies or redistributes in a named product test. It is not a
Wright compatibility corpus, oracle, or result database.

## OverPy

| Field | Value |
| --- | --- |
| Project | OverPy: high-level language for the Overwatch Workshop |
| Repository | <https://github.com/Zezombye/overpy> |
| Pinned reference | npm `overpy@9.7.10`, content commit `889d974` |
| License assumption | GPL-3.0-only; see [`docs/licensing.md`](../licensing.md) |
| Semantic owner | [`opy-rs`](https://github.com/wrightkit/opy-rs) |

The pinned reference and the OPY source corpus are maintained by `opy-rs`.
Wright does not execute the reference runtime in ordinary CI and does not
duplicate its corpus or recorded outputs. Wright's provider tests verify
provider selection, handoff, provenance, and explicit failures.

The retained generated Workshop inputs in
[`tests/fixtures/README.md`](../../tests/fixtures/README.md) identify their
source project, immutable revision, and license because those two named CLI
tests need them.

## OSTW / DEL

The pinned OSTW reference, source corpus, support claims, and reproduction
workflow are maintained by
[`deltin-rs`](https://github.com/wrightkit/deltin-rs). Wright has no shipped
DEL/OSTW provider and does not duplicate that owner infrastructure.

## Boundary

Studying a reference informs the owning language implementation. It does not
authorize copying reference source, internal types, generated artifacts, or
semantic data into Wright. Cross-language behavior claims must be made by the
owning repository and consumed through its public/provider contract.
