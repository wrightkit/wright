# Wright and Upstream Reference License Boundary

Status: accepted engineering policy
This document is not legal advice and does not settle questions that require a
qualified lawyer.

## Purpose

Wright is independently implemented Rust software: a tooling-first semantic
platform for the Overwatch Workshop ecosystem, with Wright-owned frontends
(`wright-opy`, `wright-ostw`, `wright-workshop`). Pinned upstream compilers
(OverPy, OSTW) are compatibility oracles and behavior references, not
production runtime dependencies. The repository's root license is GNU AGPL v3.0
or later, as stated in [`README.md`](../README.md) and provided in
[`LICENSE`](../LICENSE). That license governs Wright's own repository content;
it does not grant permission to copy, modify, bundle, or redistribute OverPy or
another third party's material.

The project treats the OverPy reference used for compatibility work as
GPL-3.0-only, an engineering assumption rather than a legal conclusion. The
exact version, license notice, and distribution terms must be checked for every
version used.

## Component boundary

The policy below applies to every pinned upstream reference Wright uses for
compatibility work, currently OverPy and OSTW. Per-reference identity, license
facts, and invocation records are centralized in
[`compatibility/upstream-references.md`](compatibility/upstream-references.md).

| Component | May invoke or inspect the reference? | Boundary and distribution rule |
| --- | --- | --- |
| Wright Rust core, including HIR, Workshop IR, lowering, diagnostics, and backends | No | Independently implemented code. It must not link to a reference, copy its source, import its internal AST/types, or compile against its generated artifacts. |
| Frontend adapter/bridge (`adapter/`) | Only through an explicitly documented input or process boundary | It may translate a reviewed interchange result or observed frontend behavior into Wright-owned types. It must not make reference internals a Wright API. Ownership, license, and invocation are recorded in its [`README`](../adapter/README.md). |
| Compatibility harness/oracle tool | Yes, for isolated evaluation | It may invoke a separately installed/pinned reference (OverPy or OSTW) and compare documented or generated results. It must remain separable from the core build and runtime distribution. |
| Compatibility fixtures and generated reference artifacts | Only after provenance review | Store identifiers, hashes, generators, or reviewable artifacts only when their license and redistribution status are recorded. Do not add copied reference source or unclear third-party content. |
| CI and development scripts | Yes, when isolated | They may install or invoke a pinned external oracle for a compatibility check, but must not silently turn it into a core dependency or bundled release component. |

No allow-listed path may import reference implementation details into the core
today. When a compatibility component is added, its ownership, license,
invocation method, and distribution status must be named in its own manifest or
README and linked from this document before it is used.

## Permitted inputs to the independent core

The core may be developed from:

* independently authored Wright code;
* public language or output specifications, subject to their own license;
* behavior observed through lawful, documented compatibility tests;
* a separately specified interchange format whose provenance and license are
  known; and
* third-party dependencies whose license and compatibility have been reviewed.

Observed behavior is an interoperability input, not permission to copy an
implementation. A test that passes only by importing a reference module or
reusing its internal representation belongs outside the core boundary.

## Clean-room expectations

Contributors working on the core must:

1. implement Wright-owned data structures and transformations rather than
   mechanically translating reference source or types;
2. keep source provenance for imported examples, fixtures, and generated
   artifacts;
3. record the reference version and acquisition method for compatibility
   evidence; and
4. stop and request review when a proposed dependency, fixture, or code sample
   has unclear licensing or would place a reference implementation detail in a
   core API.

Process or JSON separation is an engineering isolation technique. It is not by
itself a legal determination that two works may be combined or distributed.

## Distribution policy

The default distribution contains Wright's independently implemented Rust
core and its own documentation and tests. It does not bundle OverPy or OSTW,
their source trees or internal libraries, or reference artifacts whose
redistribution has not been reviewed.

An optional compatibility workflow may require users or CI to provide an
external reference installation (for example, OverPy). That workflow must
identify the exact version and must not prevent the core from building,
testing, or running when the oracle is absent, unless a later contract
explicitly says otherwise.

Cargo dependencies, fixtures, generated outputs, release archives, and hosted
services each require their own license review. The repository's AGPL license
does not automatically resolve the terms of any of them.

## Questions requiring legal advice

The project intentionally does not settle:

* whether a particular adapter, process invocation, generated artifact, or
  combined distribution creates a derivative or combined work under the
  applicable licenses;
* how GPL-3.0-only OverPy and AGPL-3.0-or-later Wright may be distributed
  together in a specific packaging or hosted-service model;
* whether any reference-generated output carries additional restrictions; and
* what source-offer, attribution, notice, or network-use obligations apply to a
  particular release.

Before bundling, linking, copying, or publishing a new reference-dependent
component, obtain qualified legal review and record the decision in a new ADR.

## Related decisions

* [ADR-0001: Project scope](adr/0001-project-scope.md) (superseded by ADR-0008)
* [ADR-0003: IR boundary](adr/0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](adr/0004-overpy-licensing-boundary.md)
* [ADR-0007: OverPy reference pinning policy](adr/0007-reference-pinning-policy.md)
* [ADR-0008: Tooling-first semantic platform rebaseline](adr/0008-tooling-first-semantic-platform.md)
* [Centralized upstream/reference inventory](compatibility/upstream-references.md)
