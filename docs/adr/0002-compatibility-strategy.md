# ADR-0002: Compatibility strategy

- Status: Partially Superseded by [ADR-0018](0018-tests-first-integration-verification.md)
- Date: 2026-08-12
- Related: [`docs/compatibility.md`](../compatibility.md),
  [ADR-0008](0008-tooling-first-semantic-platform.md)

## Context

Wright must preserve supported source-language and Workshop behavior at the
correct ownership boundary without reducing compatibility to identical
generated text. Syntax acceptance, diagnostics, normalized output, and
observable runtime behavior are different claims and require different tests or
owner comparisons.

## Decision

The historical compatibility levels remain a vocabulary for the owning
language repositories and for a named reference comparison:

* **S (syntax)**: accept/reject agreement for a defined owner corpus;
* **D (diagnostic)**: structured category and source-region agreement;
* **N (normalized output)**: comparison after a documented normalizer removes
  only presentation or volatile differences; and
* **E (semantic)**: repeatable observable behavior for a defined scenario and
  target/runtime.

`opy-rs` and `deltin-rs` own the language-level method, corpus, reference
identity, and support claims. `workshop-rs` owns canonical Workshop semantics
and its real-project contract. Wright tests only its integration boundary and
must not infer language completeness from a green product test.

## Current application

Wright does not store generic S/D/N/E results, a source-language corpus, or
reference snapshots. A Wright result that intentionally compares with an owner
must identify the owner contract, input identity/source attribution, comparison
method, and unsupported or inconclusive outcome in the test or CI output.
Ordinary Wright CI is the Rust provider/product test suite described by
ADR-0018.

## Consequences

Semantic compatibility remains the product goal, while output identity and
formatting remain subordinate unless a public contract makes them observable.
Reference pinning and licensing remain governed by ADR-0004 and ADR-0007.
Wright's old compatibility snapshot directory and snapshot-integrity gate are
retired by ADR-0018.
