# OPY integration boundary

`opy-rs` owns the OPY compatibility baseline: the pinned OverPy identity,
source corpus, probes, diagnostics, normalized-output comparison, and support
matrix.

Wright owns only the consumer contract around that implementation: provider
selection, process/protocol handoff, provenance propagation, product result
envelopes, and explicit unsupported or failed operations. Wright's Rust tests
exercise those contracts directly and do not copy owner-side corpus or
reference snapshots.
