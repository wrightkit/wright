# ADR-0004: OverPy licensing and clean-room boundary

- Status: Accepted engineering policy
- Date: 2026-08-12
- Related: [`docs/licensing.md`](../licensing.md)

## Context

Wright uses existing OverPy as the initial `.opy` frontend/parser and
compatibility oracle, while the Wright core is independently implemented and
licensed under AGPL-3.0-or-later. The project assumption is that the OverPy
reference used for compatibility work is GPL-3.0-licensed, but the exact terms
depend on the version and distribution involved.

The architecture must prevent an evaluation dependency from becoming an
accidental core dependency. It must also avoid presenting process separation or
an interchange format as a legal conclusion.

## Decision

The Wright core may not link to OverPy, copy its source or internal types, or
compile against its generated artifacts. HIR, Workshop IR, diagnostics, and
backend APIs expose Wright-owned types only.

OverPy invocation and any inspection of reference behavior is confined to an
explicitly isolated compatibility harness or development/CI tool. The harness
may use a separately installed and pinned OverPy version, but its presence is
not required for the core to build or run. Adapters consume reviewed,
documented boundaries and translate into Wright-owned representations.

Reference fixtures and generated artifacts require provenance and a
redistribution review before they enter the repository or a release. No
OverPy-dependent component is currently allow-listed in the checkout; a future
component must identify its license, owner, invocation method, and distribution
status before use.

## Consequences

The core remains independently inspectable and can be distributed under its own
license without silently absorbing OverPy implementation details. Compatibility
work has an explicit setup and may require an external oracle. Contributors
must preserve provenance and stop when a dependency or fixture's terms are
unclear.

This policy may require duplicated boundary types or a reviewed interchange
format. That cost is intentional because a third-party representation is not a
Wright public contract.

## Compatibility impact

The policy permits S/D/N/E compatibility evidence under
[`docs/compatibility.md`](../compatibility.md), but none of those levels grants
permission to copy or redistribute OverPy. Reference identity, fixture
provenance, and the invocation or comparison method remain part of the evidence
record.

## Open questions

Qualified legal advice is still required before bundling or linking OverPy,
distributing generated reference artifacts, or selecting a hosted-service
model. The exact license and notices for every OverPy version used must also be
verified before release.
