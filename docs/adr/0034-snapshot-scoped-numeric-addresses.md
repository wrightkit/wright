# ADR-0034: Snapshot-scoped numeric symbol and rule addresses

- Status: Accepted
- Date: 2026-10-27
- Related: [ADR-0020](0020-native-agent-tool-adapter.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0024](0024-structured-rule-content.md), [Agent contract](../agent-contract.md)

## Context

`references`, `usage`, `cfg`, and a `rules`/`references` numeric `rule`
selection address program entities by the numeric symbol id or rule index a
listing issued. Those ids are only meaningful for the program that produced
them: a content-changing reload renumbers the space, so an id captured
beforehand may silently resolve to a different entity or land in range.

The pre-1.0 contract modeled freshness implicitly: a numeric address was
refused `stale-id` from the reload until the client performed the listing
that "re-established" the id space. That made a read operation a hidden
mutation — `symbols {max: 0}` unlocked ids without returning any — and left
a window where an in-range but wrong id resolved silently when the client
listed before re-reading. It also coupled freshness to which listing ran
last, which a `references` numeric `rule` filter crossed (#592).

## Decision

1. **`snapshot` identifies a program generation.** It is an opaque integer
   the service increments each time it adopts a program snapshot: the
   deferred initial load, every content-changing reload, and every recovery
   after a failed load. A failed load invalidates the served snapshot, so
   restoring byte-identical input adopts a fresh generation rather than
   resurrecting the dropped one. `snapshot` is a generation counter, not a
   content digest: it answers "which program issued this id", not "did the
   bytes change".
2. **Every id-issuing response reports `snapshot`.** `symbols`, `rules`,
   `references`, `usage`, `findings`, `lint`, `lintRules`,
   `persistentObjects`, `project`, and `costEstimate` carry it in the result
   object; `inspect` and `analyze` carry it on the `wright-result/v1`
   envelope. Listing results that were bare arrays become objects —
   `{"symbols": [...], "snapshot": n}` — so the ids and their generation
   always travel together.
3. **Every numeric address must carry `snapshot`.** A request with a numeric
   `symbol` (`references`, `usage`) or `rule` (`cfg`, `rules`, and the
   `references` `rule` filter) whose `snapshot` is absent or differs from
   the served generation is refused `stale-id`. The listing-unlock side
   effect is removed: nothing about `symbols` or `rules` changes which ids
   are valid.
4. **Name addressing ignores `snapshot`.** A request whose addresses are all
   declared names resolves against the current program and is never stale,
   whatever `snapshot` it carries. Names remain the generation-agnostic
   path.
5. **`inputIdentity` is unchanged.** It labels the input's primary entry
   source, not a program generation; member-file edits can change the
   program without touching it. Freshness is `snapshot`, identity is
   `inputIdentity`.
6. **Versioning.** The contract is not frozen until 1.0, so
   `wright-agent/v1` changes in place; the JSON schema, agent contract,
   machine contract, and MCP tool definitions change with the
   implementation.

## Alternatives considered

- **Listing-observed freshness (status quo).** It needs no request member,
  but makes reads mutating, leaves the in-range-wrong-id window, and
  composes poorly with numeric rule filters. Rejected.
- **A content hash as the freshness token.** It would fold "the same bytes
  came back" into "the same program came back", resurrecting ids after a
  failed reload that must stay refused. A generation also has no false
  equality when two programs coincide. Rejected.
- **`inputIdentity` as the freshness token.** It does not cover directory
  member edits and conflates identity with validity. Rejected.
- **`wright-agent/v2`.** The contract is pre-1.0; a new identifier buys
  nothing yet. Rejected.

## Consequences

- Clients must echo `snapshot` whenever they send a numeric address;
  name-addressed clients are unaffected.
- The `symbols`/`rules`/`findings`/`persistentObjects` results are objects
  now; consumers reading bare arrays migrate to the `symbols`/`rules`/…
  member.
- `symbols {max: 0}` is a pure query; nothing about it changes id validity.
- The service's stale-id check is a pure request-time comparison; no
  served-set bookkeeping remains.

## Compatibility impact

This is a deliberate breaking change to the pre-1.0 `wright-agent/v1`
numeric-addressing surface: requests that omit `snapshot` with a numeric
address and results read as bare arrays change behavior. Name-addressed
requests, `inputIdentity` semantics, and non-id-issuing operations are
unchanged.
