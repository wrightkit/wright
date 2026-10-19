# ADR-0031: Paginated vocabulary listing for structured editors

- Status: Proposed
- Date: 2026-10-19
- Related: [Issue #782](https://github.com/wrightkit/wright/issues/782), [Issue #781](https://github.com/wrightkit/wright/issues/781), [Issue #798](https://github.com/wrightkit/wright/issues/798), [ADR-0021](0021-name-and-signature-lookup.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0024](0024-structured-rule-content.md), [ADR-0029](0029-structured-statement-edits.md), [Agent contract](../agent-contract.md)

Amends ADR-0021 decision 3 ("The service never returns a whole catalog or settings table") for one new operation. `lookup` itself is unchanged.

## Context

ADR-0029 names a structured view as a consumer of statement edits: a form-based editor that presents rules as conditions and actions, like the in-game Workshop editor. In that editor every argument slot is a dropdown. A person opens it and picks from the full list of actions, of values, or of an enum's members, and typing to filter is optional.

The structured editor prototype builds these forms from `lookup` and cannot fill the dropdowns:

- An empty query returns nothing, so the prototype cannot list all actions or values without the user typing first. On `practice-range`, `lookup` with `kind: action` and `query: ""` returns `[]`.
- `limit` is at most 10. Listing an enum domain through `within` is truncated: `Map` and `Hero` both have more than 10 members, so the dropdown for `createDummyBot.hero` can never show all heroes.
- The `enum` facts on a callable parameter list member display names only. A form needs member identities to build a `{enum, member}` node, so it has to call `within` anyway.

These limits are deliberate. ADR-0021 bounds results for agent token cost, and its evidence still holds: tool results stay in the agent's context and are re-read on every turn. A UI has the opposite need. It holds the list in memory, renders it once, and must show every choice so the person never has to guess a name.

The owner already has the data. `workshop-rs` iterates its entries in catalog order (`Catalog::entries_of`, `Catalog::enum_domains`), and `Catalog::lookup_within` without a query returns every member of an enum domain in owner order. It has no public call that returns full lookup facts (`LookupMatch` with its `Signature`) for every entry of a kind. Wright would have to rebuild signatures itself, which ADR-0021 decision 4 rules out.

Issue #782 compared three options: a separate paginated operation, having the UI read `workshop-rs` directly, and lifting the cap on `lookup`. This ADR records the first.

## Decision

1. **A new operation, `listEntries`, on the shared `ToolService` surface.** It lists the owner's vocabulary of one kind in owner order, page by page. It does not match or rank: there is no `query`. `lookup` keeps its bounds, its defaults, and its ranking, and ADR-0021 decision 3's bound now applies to `lookup` only.

2. **Request.**
   - `language`: `workshop`. Any other language id gets the `unavailable` result of decision 6.
   - `kind` (required): `action`, `value`, `event`, `enum`, or `enumMember`. Other `lookup` kinds (settings, parameters, structural forms, operators) are not listable here. They are added when a consumer needs them.
   - `within`: required for `enumMember` and refused for the other kinds. It is an enum domain identity as `lookup` returns it, for example `Hero`.
   - `locale` (optional): resolved as in `lookup`.
   - `pageSize` (optional): the default is 100 and the maximum is 500. One page of the largest current kind is therefore the whole kind, but a consumer must not rely on that.
   - `cursor` (optional): the `nextCursor` of the previous page. Without it the first page is returned.

3. **Result.**
   - `language`, `owner`, and `entries`, as in `lookup`.
   - `entries` uses exactly the `lookup` entry shape for the same kind, including `callable` facts and the rendered `signature`. A consumer parses one entry type, and any member later added to `lookup` entries (for example a return type under #781 or `variadic` under #798) appears here too.
   - `total`: the number of entries the request lists across all pages.
   - `nextCursor`: present when more entries follow, absent on the last page.
   - Order is the owner's order. Wright does not sort or group entries. A UI that shows the in-game alphabetical order sorts by `displayName` itself.
   - **Deterministic.** The same request against the same owner version returns the same pages.

4. **Cursors are opaque and bound to the request and the owner version.** A cursor encodes the position, the request's `language`, `kind`, `within`, and `locale`, and the owner's catalog identity. A cursor that does not match the request or the running owner version is refused with `invalid-cursor`. The consumer restarts from the first page. Wright keeps no server-side cursor state.

5. **`listEntries` does not read a program.** Like `lookup`, it answers without requiring or triggering a project load (ADR-0021 decision 5).

6. **OverPy is out of scope for now.** A `listEntries` request for `opy` returns `unavailable` naming `opy-rs` and the `listEntries` capability. Listing OverPy vocabulary needs a negotiated LPP capability. That is decided when an OverPy structured editor exists, under the protocol's additive change rules.

7. **Owners supply the listing.** `workshop-rs` adds a public call that returns the lookup facts for every entry of a kind in catalog order, the same facts `Catalog::lookup` returns per match. Wright pages that list and renders entries with the code that renders `lookup` entries. Enum members already come from `Catalog::lookup_within` without a query. Wright stores no names and assembles no signatures.

8. **Surfaces.** `capabilities.operations` lists `listEntries`, and `wright serve` answers it on both the stdio and JSON-RPC transports. It is not in the MCP tool set (ADR-0020): it exists for UIs, an agent has `lookup`, and a listing tool would put the token cost ADR-0021 rejected into every session. No CLI command is added. A terminal user has `wright lookup`, and a person who browses a whole kind uses an editor, not the terminal. ADR-0022 is unaffected: `listEntries` returns the same model to every surface that exposes it, and it is not a project-semantic query.

9. **Contract change.** The operation is additive within `wright-agent/v1`. The committed schema and `docs/agent-contract.md` gain the request and result.

## Alternatives considered

- **The UI reads `workshop-rs` directly** (Rust API or an exported catalog snapshot). Rejected. The UI would integrate with two components, which goal principle 1 rules out, and a snapshot would be a second copy of owner data next to the one `serve` uses.
- **Lift the cap on `lookup` for an explicit `kind` with an empty query.** Rejected. It contradicts ADR-0021, and an agent that sends an empty query would get a whole catalog in its context.
- **One unpaginated response per kind.** Rejected. The current sizes would fit, but nothing bounds a catalog's growth. A page limit keeps every response bounded with little cost to the consumer.
- **Server-side grouping into in-game categories.** Rejected. The in-game dropdowns are alphabetical lists, and grouping would be Wright presentation policy that no owner fact backs.
- **Offset and limit instead of a cursor.** Rejected. An offset carries no owner version, so a page fetched after an upgrade would silently skip or repeat entries.

## Consequences

- A structured editor can fill every action, value, event, and enum dropdown through `serve`, without the person typing first. Enum slots offer every member with its identity.
- Agents are unaffected. `lookup` and the MCP tool set do not change.
- Entries stay with their owner, and their shape is shared with `lookup`, so a fact added to one appears in both.
- Follow-up work:
  - A `workshop-rs` issue for the listing call in decision 7.
  - The Wright operation, the schema, and the agent-contract documentation.
  - The prototype switches its dropdowns from `lookup` to `listEntries`.
- Filtering a value list by an argument slot's type still needs return types (#781). Until then a UI lists every value for every slot.

## Compatibility impact

No syntax, diagnostic, normalized-output, or semantic behavior changes. The operation is additive under `wright-agent/v1`. The implementation must show:

- For each listable kind, the identities across all pages equal the owner's entries of that kind, in owner order, with no duplicates.
- Each listed entry equals the `lookup` entry for the same identity.
- Repeated requests return identical pages.
- A cursor from another request, or one with a changed owner identity, is refused with `invalid-cursor`.
- `listEntries` answers when the project cannot load, and does not trigger a load.
- `wright serve` stdio and JSON-RPC return the same result for the same request.

## Scope boundaries

This ADR does not decide:

- type-based filtering of values, which depends on #781;
- listing settings, parameters, structural forms, or operators;
- the OverPy listing capability, its wire shape, or DEL/OSTW;
- a CLI command or an MCP tool for listing, which are added only for a demonstrated terminal or agent workflow;
- how a UI sorts, groups, or caches entries.
