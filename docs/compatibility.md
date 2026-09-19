# Wright integration verification

Status: current product contract

Wright is the tooling and integration layer. `opy-rs` owns OPY syntax,
semantics, compatibility tests, reference pins, and support claims; `deltin-rs`
owns the corresponding DEL/OSTW responsibilities; `workshop-rs` owns canonical
Workshop semantics and owner-side real-project contracts.

Wright does not maintain a source-language compatibility corpus, oracle,
snapshot database, or generic result-integrity test. A Wright test input exists
only for a named Wright-owned contract and is kept under
[`tests/fixtures`](../tests/fixtures/README.md). The retained inputs are small
Workshop/provider/product cases, not a language support matrix.

## Wright-owned verification

Wright's current verification surfaces are ordinary Rust tests:

* `wright-analyzer` tests Workshop semantic queries and source spans;
* `wright-driver` tests provider handoff, failure propagation, and the
  canonical `workshop-rs` contract;
* `wright-cli` tests public command, diagnostic, renderer, and transport
  behavior;
* `wright-consumer` tests the public embedding API; and
* the ignored real-project tests run the released `workshop-rs` corpus through
  Wright and compare diagnostics with the owner `Program` API.

These tests compare Wright behavior with an owner API or assert a Wright-owned
observable contract. They do not claim source-language semantic completeness.
Unsupported provider capabilities and provider failures remain explicit
results.

## Reference and provenance boundaries

Reference versions, language corpora, differential comparisons, and support
matrices belong in `opy-rs` and `deltin-rs`. Wright may retain attribution and
license information for a small redistributed input that a named Wright test
needs; it must not retain the owner's complete corpus or reference output.

The pinned upstream project identities used for provenance are recorded in
[`compatibility/upstream-references.md`](compatibility/upstream-references.md).
That document is an attribution and ownership record, not a Wright test-data
store.

When a Wright integration change needs a source-language case, add the smallest
consumer-specific input and a test at the boundary that owns the claim. When a
language behavior or reference expectation changes, update the owning language
repository first.

## Related decisions

* [ADR-0004: OverPy licensing and clean-room boundary](adr/0004-overpy-licensing-boundary.md)
* [ADR-0007: OverPy reference pinning policy](adr/0007-reference-pinning-policy.md)
* [ADR-0010: Independent language implementations and Wright integration](adr/0010-independent-implementations-and-wright-integration.md)
* [ADR-0018: Tests-first integration verification](adr/0018-tests-first-integration-verification.md)
