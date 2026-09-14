# ADR-0016: Current-directory and directory project targets

- Status: Accepted (backfilled)
- Date: 2026-09-14 (backfill date)
- Clarifies: [ADR-0013: Entry-based source-provider integration seam](0013-entry-based-source-provider-seam.md)
- Related: [Issue #317](https://github.com/wrightkit/wright/issues/317),
  [PR #318](https://github.com/wrightkit/wright/pull/318),
  [LPP ADR-0002: Directory project targets](https://github.com/wrightkit/language-provider-protocol/blob/main/docs/adr/0002-directory-project-targets.md),
  [LPP ADR-0003: Owner-selected source identity](https://github.com/wrightkit/language-provider-protocol/blob/main/docs/adr/0003-owner-selected-source-identity.md)

## Historical note

This record preserves the product and integration decision introduced by Issue
#317 and implemented and reviewed in PR #318. It was recorded after that work
was merged; it does not change the current CLI contract.

## Context

ADR-0013 established a narrow source-provider seam around a user-selected entry
path. That decision intentionally left project discovery and source closure to
the source-language owner, but it did not decide how a CLI invocation with no
path should select a target or how a project directory should cross the seam.

Real source projects are commonly checked from their repository root. Treating
an omitted input as stdin makes that invocation depend on pipe state and can
turn an empty stdin stream into a misleading input result. Conversely, making
stdin implicit prevents a caller from distinguishing a piped source from a
filesystem target. Directory targets therefore need an explicit product
boundary that does not turn Wright into a second project loader.

## Decision

Wright resolves command input as follows:

- an explicitly supplied file remains a file target;
- an explicitly supplied directory is a project target;
- an omitted input targets the current working directory; and
- `-` remains the explicit stdin target.

For a directory target, Wright performs only the minimum inspection needed to
identify a source owner. It does not recursively collect a source closure,
choose an effective entry, or apply a language-precedence table. If the
available ownership signals identify more than one source owner, including a
raw Workshop candidate alongside a language candidate, Wright reports explicit
ambiguity instead of hiding a candidate behind an implicit precedence rule.

After ownership is selected, the source implementation owns effective-entry
selection, project-root interpretation, source closure, and project semantics.
Provider-backed directory targets cross the owner/provider boundary as
filesystem targets; Wright does not introduce a Wright workspace, manifest,
or generic project graph. The protocol's wire representation and owner-selected
source-identity rationale remain owned by
[LPP ADR-0002: Directory project targets](https://github.com/wrightkit/language-provider-protocol/blob/main/docs/adr/0002-directory-project-targets.md)
and
[LPP ADR-0003: Owner-selected source identity](https://github.com/wrightkit/language-provider-protocol/blob/main/docs/adr/0003-owner-selected-source-identity.md).
Wright preserves that owner identity in its result contract rather than
reconstructing it from a directory path.

## Alternatives considered

- **Keep omitted input as stdin:** rejected because ordinary invocation from a
  project root would not select the project, while an empty pipe could be
  mistaken for a valid default input.
- **Recursively load the directory in Wright:** rejected because it duplicates
  OPY/DEL project discovery and source-closure semantics and cannot define one
  correct rule for every source owner.
- **Prefer a language candidate over raw Workshop:** rejected because hidden
  precedence makes mixed ownership dependent on an undocumented heuristic and
  can select the wrong source owner.
- **Synthesize source identity from the directory path:** rejected because a
  path identifies a target, not the owner-selected source text, and changes to
  project contents would not be reflected in the result identity.

## Consequences

- `wright check`, `compile`, `lint`, `analyze`, and supported `inspect` flows
  can be invoked from a project directory without a Wright-owned project
  loader.
- Users and scripts retain a stable, explicit distinction between current
  directory selection and stdin.
- Ambiguous directories fail visibly and can be resolved by an explicit kind or
  target selection where the CLI supports it.
- Source-language owners remain responsible for project behavior, and provider
  protocol details remain adapter/protocol concerns rather than new Wright
  semantic authority.
- Directory-backed provider results retain the source identity selected by the
  owner; Wright does not claim that the directory path is source identity.

## Compatibility impact

This decision extends the entry-based product seam without changing the
semantics of explicit file targets or the owner/provider ownership boundary.
Existing `-` stdin workflows remain explicit. Provider-backed directory
support depends on the owner and protocol contracts; unsupported owners and
ambiguous targets remain structured failures rather than silently falling back
to another source implementation.

## Scope boundaries

- Current CLI behavior and diagnostic details remain authoritative in
  [`docs/cli.md`](../cli.md), not in this ADR.
- Source-language project discovery, effective-entry selection, and source
  closure remain owned by the relevant language implementation.
- LPP wire fields, capability negotiation, conformance, and source-identity
  semantics remain owned by `language-provider-protocol`; this ADR links to
  that rationale without duplicating it.
- This ADR does not define a Wright workspace/project model, recursive loader,
  or language-precedence table.
