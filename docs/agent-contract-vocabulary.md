# Agent contract: vocabulary operations

Context: for consumers of `lookup` and `listEntries` on `wright-agent/v1`. The rest of the contract is in [agent-contract.md](agent-contract.md).

## Name and signature lookup (#529, ADR-0021)

`lookup` resolves a free-text query, such as a display name, a near spelling,
or a guess, against the language owner's vocabulary, so an agent can learn the
accepted spelling and signature of a Workshop or OverPy name without
external documentation, source inspection, or `wright convert`. The
request's `language` selects the owner (`workshop` or `opy`, the ids
`capabilities.languages` reports); the remaining fields narrow the answer:

* `query`: the free text to resolve.
* `kind`: any entry kind an owner emits (#563): `action`, `value`,
  `event`, `enumMember`, `enum`, `setting`, `parameter`, `settingPath`,
  `memberAction`, `memberValue`, `structural`, or `operator`. A kind one
  language never emits answers `[]` on it.
* `within`: the identity of an enum domain, a callable, or a settings path
  prefix. The result then lists its members, parameters, or child segments,
  which is how a caller enumerates an enum's accepted member spellings or
  a callable's parameters.
* `locale`: the Workshop display-name locale (session configuration, then
  the catalog's primary locale when omitted).
* `limit`: default 3, maximum 10. A `limit` outside that range is a
  `malformed-request` refusal, never a silent clamp (#783).

Each entry carries its owner `identity`, `kind`, accepted `spelling`, and
`displayName`, plus the owner fact that describes it (`callable`, `enum`,
`parameter`, or `setting`). The `callable` fact holds the ordered
`parameters`; when the owner declares them it also carries `returns`, the
callable's return type in the same vocabulary as parameter `type`s (a
Workshop `value` such as `distance` declares one; actions carry none),
and `variadic: true` when the last parameter repeats for extra arguments.
Callables also carry `signature`, a human-readable call shape Wright
renders from the owner's parameter facts: required parameters name their
type, optional parameters show `name?` or `name=default`, and an enum
domain lists its members inline (a domain above the member bound renders
as `Domain(count members)`).

Workshop answers come from the `workshop-rs` catalog in process. OverPy
answers come from the configured `opy-rs` provider's LPP lookup
capability; when the provider does not negotiate it, the result is an
explicit `unavailable` payload naming `opy-rs`, never a Wright-side
textual guess. An unmatched scope identity refuses with
`lookup.unknownWithin`; an unknown `language` or `kind` is a usage error.
The operation reads vocabulary alone, so it works over a session whose
project cannot load.

`wright lookup [QUERY]` exposes the same request on the CLI
(`--language`, `--kind`, `--within`, `--locale`, `--limit`): text mode
prints one signature or spelling per line, and `-f json` prints this
result payload. Text `check`/`compile` output points at the command after
an `unknown-*` diagnostic; structured payloads are unchanged.

## Listing vocabulary (ADR-0031)

`listEntries` serves structured editors that must show every choice, such
as the dropdown of all actions or all members of an enum. It is not an MCP
tool and has no CLI command: an agent has `lookup`, whose result bounds keep
tool output small (ADR-0021).

- `language` is `workshop`; `opy` answers `unavailable` naming `opy-rs` and
  the `listEntries` capability.
- `kind` is `action`, `value`, `event`, `enum`, or `enumMember`. `within`
  names an enum domain identity as `lookup` returns it, and is required for
  `enumMember` and refused for every other kind (`malformed-request`).
- `pageSize` is 1..=500 (default 100); a value outside it is refused as
  `malformed-request`. The request has no `query`: entries are listed, never
  matched or ranked.
- `entries` use exactly the `lookup` entry shape, in the owner's order.
  `total` counts the entries across all pages. `nextCursor` is present
  while more entries follow.
- A `cursor` is opaque and bound to the request's `language`, `kind`,
  `within`, resolved locale, and the owner's catalog identity. A cursor
  from another request or an earlier owner version is refused with
  `invalid-cursor`; restart from the first page. The service keeps no
  cursor state.
- The same request returns the same pages, and the operation reads no
  program and does not load one.
