# ADR-0009: Language ownership and licensing boundaries

- Status: Accepted
- Date: 2026-08-16
- Amends: [ADR-0008: Tooling-first semantic platform rebaseline](0008-tooling-first-semantic-platform.md)
  (frontend-ownership wording only)
- Related: [Issue #136](https://github.com/wrightkit/wright/issues/136),
  [Issue #135](https://github.com/wrightkit/wright/issues/135),
  [docs/architecture.md](../architecture.md),
  [docs/licensing.md](../licensing.md),
  [language-provider-protocol#1](https://github.com/wrightkit/language-provider-protocol/issues/1)

## Context

ADR-0008 decision 2 records Wright as owner of its semantic frontends:
"Wright owns independent semantic frontends where required for standalone
compilation, source-aware analysis, agent source editing, CI, WASM/embedding,
and long-term ecosystem independence", listing Vanilla Workshop, OPY, and OSTW
as Wright-owned frontends.

The ecosystem had created dedicated repositories for these responsibilities:
`workshop-rs` (canonical Workshop core), `opy-rs` (OPY provider), `del-rs`
(independent DEL/OSTW-compatible provider), and
`language-provider-protocol` (neutral provider contract). This decision places
Wright's integration boundary at `workshop-rs` and LPP-conformant providers.

Repository layout is not an ownership authority. In-repository implementation
locations and extraction work do not change the ownership declared below.

Licensing facts differ per upstream reference (recorded in
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md)):
OverPy is treated as GPL-3.0-only (engineering assumption), while the OSTW
compiler is unlicensed and must not be copied or redistributed. A single
repository-wide frontend licensing assumption is therefore wrong.

## Decision

### 1. Repository ownership

Durable ownership of the ecosystem architecture is:

- **`wrightkit/workshop-rs`** owns canonical Workshop semantics, actions,
  values, AST, parser, emitter, multi-locale action/value catalogs, and
  Workshop IR (WIR).
- **`wrightkit/language-provider-protocol`** owns the neutral language
  provider protocol contract (LPP).
- **`wrightkit/opy-rs`** owns first-party OPY language semantics and
  implementation.
- **`wrightkit/del-rs`** owns the independent DEL/OSTW-compatible
  implementation.
- **`wrightkit/wright`** owns tooling and orchestration: CLI/service
  orchestration, diagnostics, static analysis, generic source-edit transaction
  safety, semantic refactoring, agent and embedding APIs, language services,
  and integration adapters (LSP, LPP client).

### 2. LPP is the stable process boundary

The Language Provider Protocol is the stable boundary between Wright tooling
and language providers: a documented process and data contract owned by
`language-provider-protocol`. It is not a Rust trait, a dylib, or an FFI ABI,
and it does not imply any cross-repository Rust dependency. Wright implements
an LPP client; providers implement an LPP server.

### 3. Dependency direction

Providers and the canonical core must not depend back on Wright tooling
internals. Wright consumes provider contracts and `workshop-rs` public
contracts; it must not leak tooling internals into them.

### 4. Amendment of ADR-0008 decision 2

ADR-0008 decision 2 (semantic frontend ownership) is amended: Wright no longer
claims durable ownership of OPY, DEL/OSTW, or canonical Workshop frontend
implementations; those belong to their provider repositories. Wright retains
coordination and integration responsibility through LPP and the shared
tooling contracts.

The rest of ADR-0008 remains normative:

- tooling-first priority (decision 1);
- semantic compatibility over output identity (decision 3);
- legacy and reference quirk separation (decision 4);
- no source-language forking (decision 5);
- Workshop-centered conversion matrix (decision 6);
- source-oriented semantic edits as the default mutation model (decision 7);
- corpus-defined support claims (decision 8).

### 5. Provider-specific provenance and licensing

Provenance and licensing are recorded per provider; there is no single
repository-wide frontend licensing assumption:

- **`opy-rs`** treats pinned OverPy as a GPL-3.0-only compatibility oracle
  (engineering assumption, not a legal conclusion) and follows the ADR-0004
  clean-room boundary: no linking, copying, or internal-type import.
- **`del-rs`** treats pinned OSTW as an unlicensed reference: its source may
  be read for behavior but must not be copied, imported, or redistributed;
  only the MIT-licensed VS Code extension subdirectory is MIT.
- **`workshop-rs`** must not import Blizzard-IP-adjacent game-derived data
  (for example OSTW `Elements.json`) into its canonical catalog; catalog
  provenance and version boundaries are its own contracts.
- **Wright** remains AGPL-3.0-or-later until a provenance and contributor
  audit enables a different license; final Wright relicensing is not decided
  here.

None of the above is a legal safe harbor. Process, JSON, or protocol
separation is engineering isolation, not a legal determination that works may
be combined or distributed.

### 6. Unlicensed upstream internals are not an implementation source

Unlicensed upstream implementation internals (for example the OSTW compiler)
are not an implementation source for independently compatible providers.
Behavior observed through documented, lawful compatibility tests and pinned
oracles is a permitted input; copying or mechanically translating unlicensed
implementation internals is not.

## Consequences

- ADR-0008's frontend-ownership wording is amended; its tooling-first and
  semantic-compatibility decisions remain normative.
- [`docs/architecture.md`](../architecture.md) and
  [`docs/licensing.md`](../licensing.md) distinguish repository ownership from
  implementation location and provider provenance.
- Repository extraction and provider cutover are execution concerns; they do
  not alter the ownership or dependency direction in this ADR.
- LPP request/response schema design belongs to `language-provider-protocol`
  (issue language-provider-protocol#1), not to Wright.

## Compatibility impact

No compatibility level is removed or weakened. The S/D/N/E measurement
contracts from ADR-0002 and ADR-0008 remain normative, and the declared OPY
and OSTW semantic compatibility surfaces remain binding when ownership moves
to `opy-rs` and `del-rs`. Tooling-first priority, semantic compatibility over
output identity, Workshop-centered interoperability, and source-oriented
mutation are unchanged.

## Scope boundaries

- License selection and relicensing are outside this ADR; each repository or
  product owner must decide them from its provenance and contributor evidence.
- LPP v1 schema and conformance requirements belong to
  `language-provider-protocol` and are outside this ownership decision.
