# ADR-0002: Compatibility strategy

- Status: Accepted (amended by [ADR-0008](0008-tooling-first-semantic-platform.md))
- Date: 2026-08-12
- Related: [`docs/compatibility.md`](../compatibility.md),
  [ADR-0008](0008-tooling-first-semantic-platform.md)

## Context

Wright must be compatible with supported OverPy behavior without reducing
compatibility to identical generated text. Different evidence is needed for
accepted syntax, diagnostics, canonical output, and runtime behavior.

## Decision

Wright reports compatibility at four separately named levels:

* **S — syntax:** recorded accept/reject agreement for a defined corpus;
* **D — diagnostic:** structured category and source-region agreement for
  diagnosed inputs;
* **N — normalized output:** comparison after a versioned normalizer removes
  only documented presentation or volatile differences; and
* **E — semantic:** repeatable observable behavior for defined scenarios and a
  named target/runtime.

Every result records the reference identity, input/corpus identity, Wright
identity, comparison method, and environment needed to reproduce it. Unsupported
and inconclusive cases are reported separately from passing cases.

## Consequences

The project can make precise claims at an appropriate strength and can identify
whether a regression is in parsing, diagnostics, lowering/output, or behavior.
It also requires corpus provenance, normalization versioning, and execution
environment details before a release can claim higher compatibility.

## Compatibility impact

No level is implied by another. N-level output comparison is not E-level
semantic evidence, and a successful build is not compatibility evidence. The
release gates and fixture requirements are normative in
[`docs/compatibility.md`](../compatibility.md).

[ADR-0008](0008-tooling-first-semantic-platform.md) adds a priority rule over
this framework: **E-level observable semantics outrank N-level output-text
identity**. Presentation-only N-level differences must be evaluated against the
declared observable and documented compatibility surface before creating
implementation work; they are not automatically product bugs.

## Open questions

The project still needs to choose its supported OverPy versions, diagnostic
schema, canonical normalizer, semantic test runtime, and redistributable corpus
policy.
