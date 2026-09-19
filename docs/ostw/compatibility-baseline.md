# DEL / OSTW owner boundary

The DEL/OSTW compatibility baseline, reference identity, source corpus,
support claims, and reconstruction semantics belong to
[`deltin-rs`](https://github.com/wrightkit/deltin-rs).

Wright does not ship a DEL/OSTW provider or static adapter. `wright convert
--target ostw` keeps the recognized product boundary and returns the structured
`source-provider-unavailable` result without partial source. Wright does not
replay an owner oracle, keep a DEL/OSTW corpus, or maintain a second support
matrix.

Any future provider work must add the smallest Wright integration test needed to
protect provider selection, structured diagnostics, provenance, or a public
conversion contract. Language behavior and reference comparisons remain
owner-side work.
