# Wright and OverPy License Boundary

Status: accepted engineering policy for v0.1
This document is not legal advice and does not settle questions that require a
qualified lawyer.

## Purpose

Wright is independently implemented Rust software for an OverPy-compatible
workflow. The repository's root license is GNU AGPL v3.0 or later, as stated in
[`README.md`](README.md) and provided in [`LICENSE`](LICENSE). That license
governs Wright's own repository content; it does not grant permission to copy,
modify, bundle, or redistribute OverPy or another third party's material.

The project currently treats the OverPy reference used for compatibility work
as GPL-3.0-licensed according to the v0.1 project assumption. The exact OverPy
version, license notice, and distribution terms must be checked for every
version used. This assumption is not a legal conclusion.

## Component boundary

The following policy applies to components as they are introduced. The current
checkout has no OverPy-dependent source component.

| Component | May invoke or inspect OverPy? | Boundary and distribution rule |
| --- | --- | --- |
| Wright Rust core, including HIR, Workshop IR, lowering, diagnostics, and backends | No | Independently implemented code. It must not link to OverPy, copy its source, import its internal AST/types, or compile against its generated artifacts. |
| Frontend adapter/bridge | Only through an explicitly documented input or process boundary | It may translate a reviewed interchange result or observed frontend behavior into Wright-owned types. It must not make OverPy internal types a Wright API. |
| Compatibility harness/oracle tool | Yes, for isolated evaluation | It may invoke a separately installed/pinned OverPy tool and compare documented or generated results. It must remain separable from the core build and runtime distribution. |
| Compatibility fixtures and generated reference artifacts | Only after provenance review | Store identifiers, hashes, generators, or reviewable artifacts only when their license and redistribution status are recorded. Do not add copied OverPy source or unclear third-party content. |
| CI and development scripts | Yes, when isolated | They may install or invoke a pinned external oracle for a compatibility check, but must not silently turn it into a core dependency or bundled release component. |

There is no allow-listed path that may import OverPy implementation details into
the core today. When a compatibility component is added, its ownership,
license, invocation method, and distribution status must be named in its own
manifest or README and linked from this document before it is used.

## Permitted inputs to the independent core

The core may be developed from:

* independently authored Wright code;
* public language or output specifications, subject to their own license;
* behavior observed through lawful, documented compatibility tests;
* a separately specified interchange format whose provenance and license are
  known; and
* third-party dependencies whose license and compatibility have been reviewed.

Observed behavior is an interoperability input, not permission to copy an
implementation. A test that only passes by importing an OverPy module or
reusing its internal representation belongs outside the core boundary.

## Clean-room expectations

Contributors working on the core must:

1. implement Wright-owned data structures and transformations rather than
   mechanically translating OverPy source or types;
2. keep source provenance for imported examples, fixtures, and generated
   artifacts;
3. record the reference version and acquisition method for compatibility
   evidence; and
4. stop and request review when a proposed dependency, fixture, or code sample
   has unclear licensing or would place an OverPy implementation detail in a
   core API.

Process or JSON separation is an engineering isolation technique. It is not by
itself a legal determination that two works may be combined or distributed.

## Distribution policy

The default v0.1 distribution contains Wright's independently implemented Rust
core and its own documentation and tests. It does not bundle OverPy, its source
tree, its internal libraries, or reference artifacts whose redistribution has
not been reviewed.

An optional compatibility workflow may require users or CI to provide an
external OverPy installation. That workflow must identify the exact version and
must not make the core unable to build, test, or run when the oracle is absent
unless a later contract explicitly says otherwise.

Cargo dependencies, fixtures, generated outputs, release archives, and hosted
services each require their own license review. The repository's AGPL license
does not automatically resolve the terms of any of them.

## Questions requiring legal advice

The project intentionally does not settle:

* whether a particular adapter, process invocation, generated artifact, or
  combined distribution creates a derivative or combined work under the
  applicable licenses;
* how GPL-3.0 OverPy and AGPL-3.0-or-later Wright may be distributed together in
  a specific packaging or hosted-service model;
* whether any OverPy-generated output carries additional restrictions; and
* what source-offer, attribution, notice, or network-use obligations apply to a
  particular release.

Before bundling, linking, copying, or publishing a new OverPy-dependent
component, obtain qualified legal review and record the decision in a new ADR.

## Related decisions

* [ADR-0001: Project scope](docs/adr/0001-project-scope.md)
* [ADR-0003: IR boundary](docs/adr/0003-ir-boundary.md)
* [ADR-0004: OverPy licensing and clean-room boundary](docs/adr/0004-overpy-licensing-boundary.md)
