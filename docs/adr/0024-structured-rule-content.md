# ADR-0024: Structured rule content for agents and graders

- Status: Accepted
- Date: 2026-10-11
- Related: [Issue #615](https://github.com/wrightkit/wright/issues/615), [Issue #536](https://github.com/wrightkit/wright/issues/536), [Issue #595](https://github.com/wrightkit/wright/issues/595), [ADR-0015](0015-canonical-facts-and-declarative-lint-policy.md), [ADR-0021](0021-name-and-signature-lookup.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [SPEC-414](../specs/SPEC-414-agent-benchmark-comparison.md), [Agent contract](../agent-contract.md)

## Context

There is no way for an agent or a grader to ask Wright what a rule does. In Wright 0.15.3, `rules` returns `id`, `name`, `event`, and `span` for each rule. `cfg` returns blocks, action indices, successors, and `waits`. The internal `GetRule` returns counts only. Neither surface returns any condition or action.

Wright already holds that content. Every loaded program is a `workshop_rs::Program`: raw Workshop is parsed directly, and OverPy provider output is reparsed with its source map applied. That program has `Rule { event, conditions, actions }`, a flat action stream, and `Value` call trees named by locale-independent catalog ids (`applyImpulse`, `setStatusEffect`, `Status`/`STUNNED`). Statement spans are available through `condition_span` and `action_span`. However, nothing serializes this content. `workshop-rs` derives no `Serialize` on these types, and some identities it needs, such as `PlayerEventKind::catalog_id` and `ModifyOp::catalog_id`, are crate-private. Wright already works around this by keeping its own copy of the event-id mapping in `public_event_id`.

This has two consequences:

- Agents read source text to learn what a rule does, although goal principle 4 promises structured access without scraping.
- The benchmark can only grade requirements with `compiled-contains`, which runs regexes over emitted Workshop text. In the first post-lookup comparison this rejected `Event Player != Victim` because the check expected `Attacker != Victim`, and rejected a win score held in a variable because the check expected a literal `, 7`. SPEC-414 REQ-010 requires a structured check wherever a structured field exists, and none exists.

## Decision

### 1. Content model: one node form, owned by `workshop-rs`

A rule's content is a JSON object in the versioned format `workshop-rs/rule-content-v1`. `workshop-rs` defines it, produces it from a `Program` rule, and publishes its JSON Schema. Wright does not define it.

A value node is exactly one of these forms:

| Form | Meaning |
| --- | --- |
| JSON number, `true`, `false`, `null` | Literal |
| `{"string": text}` | Custom string literal |
| `{"localizedString": id}` | Preset string, by catalog id |
| `{"enum": domain, "member": id}` | Enum member, by catalog domain and member id |
| `{"variable": name}` | Declared global or player variable name used as an argument |
| `{"subroutine": name}` | Declared subroutine name used as an argument |
| `{"call": id, "args": [node, ...]}` | Any action, value, or operator, by catalog id |

Every invocation uses `call`, including the forms the public Rust model specializes. Variable reads, `Event Player`, vectors, arrays, `Set`/`Modify` variable actions, `Call Subroutine`, and the control-flow statements `If`, `Else If`, `Else`, `While`, `For`, and `End` are all calls with their catalog ids. A grader therefore needs one matcher. `args` lists every argument the canonical program holds, in catalog parameter order, so an argument is identified by its position. Lookup (ADR-0021) names the parameter at each position. Where a construct has no catalog entry, `workshop-rs` assigns a stable reserved id in the format. Internal sentinels such as `__ambiguous_enum` are not part of the format.

A rule's content is:

```json
{
  "format": "workshop-rs/rule-content-v1",
  "event": { "id": "eachPlayer", "team": "all", "player": "all" },
  "disabled": false,
  "conditions": [ { "value": <node> } ],
  "actions": [ { "call": "applyImpulse", "args": [<node>, <node>, 150, <node>, <node>] } ]
}
```

- `event.id` is the catalog event id, the same value `rules` reports today. Event filters are present only for events that take them, and the subroutine name is present for a subroutine rule. Filter ids come from `workshop-rs`.
- `actions` is flat. Index `i` is `Rule.actions[i]`, the same index space that `cfg`, `references`, and findings already use. Nesting can be recovered from `If`/`End` pairs, or taken from `cfg`, without a second structure.
- A disabled condition or action carries `"disabled": true`.

For a source language, the content is the canonical program the owner produced. OverPy macros, inline functions, and other lowering are expanded. The source location is kept through spans (decision 3), and target-language names come through `names` (decision 4).

### 2. Owner split

| Owner | Responsibility |
| --- | --- |
| `workshop-rs` | Defines `rule-content-v1` and its JSON Schema. Provides a public API that produces it from a `Program` rule. Exposes every catalog id the format uses. Has contract tests for each node form. |
| `language-provider-protocol` | Adds one optional, negotiated capability that maps a set of canonical ids to the language's spellings, under its additive change rules. The wire shape is decided there. |
| `opy-rs` | Implements that capability from its own mapping from Workshop names to OverPy spellings. |
| Wright | Selection, size bound, rule `id`, statement spans, the `names` composition, the result schema, and presentation in the CLI, `serve`, and MCP. |

Wright does not serialize `Program` itself, translate names, or normalize content.

### 3. Surface: `rules` gains `content` and `rule`

`rules` accepts two optional fields:

- `content: true` adds a `content` member, in the decision 1 form, to each returned rule entry.
- `rule` is an id or name, the same `Address` that `cfg` accepts, and selects exactly one rule. The existing `name` filter is unchanged and still matches every rule with that declared name.

Wright adds `span` to each condition and action when the program has one, using the same span presentation as the rest of the result. It does not add argument-level spans.

**Size bound.**
- A `content` request without `max` applies a default `max` of 20 rules.
- A `content` request always returns the selection shape `{"rules": [...], "selection": {"total", "withheld"}}`.
- An explicit `max`, `name`, `file`, or `rule` narrows the set as defined in the semantic query selection contract.
- A selected rule's content is never truncated, because a partial tree would make assertions silently wrong.
- `inspect --brief` is unchanged.

**Surfaces.** The CLI adds `wright inspect rules`, with `--content`, `--rule`, and the existing `--only`/`--file`/`--max` selection. `serve` takes `{"op": "rules", "content": true, ...}`. The MCP tool `wright_rules` gets the fields through its generated input schema. All three return the same driver model with the same defaults (ADR-0022).

Rule ids follow the current freshness rules and whatever #595 decides. Content adds no new id space.

### 4. Target-language names

When the loaded program comes from a source language whose spellings differ from Workshop (today OverPy), a `content` result also carries one result-level member. The spellings below are illustrative only:

```json
"names": {
  "language": "opy",
  "calls": { "setStatusEffect": "setStatusEffect" },
  "enums": { "Status": { "STUNNED": "Status.STUNNED" } }
}
```

- `names` covers only the ids that occur in the returned content. Content nodes keep their canonical ids, so assertions stay language-independent.
- An id that has no target-language spelling is left out of the map. This happens, for example, with a construct that only exists after lowering.
- Wright gets the map from the provider capability in decision 2 and invents no spelling.
- If the provider does not advertise the capability, the result carries `unavailable` naming `opy-rs` instead of `names`, and the content is unaffected.
- Raw Workshop results carry no `names`. The Workshop display spelling for a locale is a `lookup` question.

### 5. Benchmark check: `program-calls`

The benchmark gains a structured check kind, in line with SPEC-414 REQ-010:

```json
{ "id": "repels-with-150", "kind": "program-calls", "layer": "requirement",
  "call": "applyImpulse", "args": { "2": 150 }, "min": 2 }
```

- The grader compiles `source` (`oracle` or `wright`, with the same defaults as `compiled-contains`) to Workshop text. It loads that text with `wright inspect rules --content --max <n>`. The input is raw Workshop, so the result has no `names`. If `withheld` is not 0, the check fails as incomplete instead of being decided on part of the program.
- The grader walks every condition, action, and nested argument node of every rule. Optionally, `event` restricts the walk to rules with that event id.
- A node matches when `call` is equal and every listed position matches its pattern. A pattern is a node. Numbers compare within 1e-9, objects compare as a recursive subset, and the alternatives form `{"any": [pattern, ...]}` is allowed. The check passes when the match count is within `min`/`max`.
- The matcher is benchmark code over the published format. It is not a Wright operation or a query language.

**What it is stable against.** Spelling, locale, formatting, OverPy versus Workshop surface names, and macro or inline-function expansion. All of these produce the same canonical ids and argument positions.

**What it is not stable against.** Semantically equivalent restructuring: a value held in a variable instead of inline, `a != b` versus `not(a == b)`, operand order, or one call over an array versus two calls. Wright does not normalize these, because constant propagation and canonicalization are out of scope. A scenario covers such forms explicitly. It lists them under `any`, or it asserts the call and the value separately. For a win score of 7, for example, it lists the inline comparison and the assignment `{"call": "setGlobalVariable", "args": {"1": 7}}` as alternatives. Every `program-calls` check ships with a positive and a negative fixture, like `compiled-contains`.

### 6. Executable scenario: Proximity-repel

The fixture is a rule that, while an enemy is within 6 meters, applies two impulses of strength 150 (away and up) and applies `Set Status(..., Stunned, 0.75)` twice (to the event player and to the nearest enemy). It is committed in two forms:

- OverPy, compiled through the provider;
- Workshop, written in two locales (`en-US` and `zh-CN`).

These checks must pass on all three:

```json
{ "kind": "program-calls", "call": "applyImpulse", "args": { "2": 150 }, "min": 2 }
{ "kind": "program-calls", "call": "setStatusEffect",
  "args": { "2": { "enum": "Status", "member": "STUNNED" }, "3": 0.75 }, "min": 2 }
```

A negative fixture with one stun at 0.5 must fail the second check. A Wright driver test asserts the same content through `rules` with `content: true` on the OverPy form. That test also asserts that `names` maps `applyImpulse` to the OverPy spelling once the provider capability exists, and that the result reports `unavailable` before then. No assertion reads source text.

## Alternatives considered

- **Wright serializes `workshop_rs::Program` itself:** rejected. Wright would become a second authority for the shape of canonical content, `#[non_exhaustive]` additions in `workshop-rs` would drift silently, and other consumers would not share the format.
- **Derive `Serialize` on the public program types:** rejected. It exposes the Rust enum layout (`SetGlobalVariable`, `Disabled { action }`, `EventPlayer`) instead of uniform call trees, and it couples the public JSON format to internal refactors.
- **A separate operation such as `ruleContent`:** rejected. It would duplicate the selection, freshness, and transport plumbing that `rules` already has, and a grader needs a multi-rule walk anyway.
- **Extend the full `inspect` result:** rejected. It is already the large catch-all payload, and content would make it unbounded.
- **Nested block structure in `actions`:** rejected. It would add a second index space next to the one `cfg`, `references`, and findings share, and `cfg` already provides the structure.
- **Named arguments (`{"speed": 150}`):** rejected for size, consistent with ADR-0021. Positions are fixed by the catalog, and `lookup` names them.
- **Use the ADR-0015 declarative lint matcher as the grader:** rejected. It would turn lint into an assertion mechanism, it matches only within one rule's scopes, and it does not give agents readable content.
- **Canonical ids only, with OverPy names left for later:** rejected by the maintainer for this decision. OverPy agents need their own spellings when they act on content.

## Consequences

- Agents can read what a rule does in a bounded, deterministic form, and they can go from a `references` or `cfg` index to the statement it names.
- Once the owner exposes event ids, Wright's own copy of the event-id mapping (`public_event_id`) can be replaced by the owner's ids.
- Requirement checks can assert on canonical calls and arguments. That removes false failures caused by spelling, locale, and surface-name differences, while restructured equivalents remain an explicit scenario concern.
- Follow-up work is in four repositories. `workshop-rs`: the format, the schema, the API, and public ids. `language-provider-protocol`: the capability. `opy-rs`: its implementation. Wright: the `rules` fields, the CLI subcommand, the schema `$defs`, the agent contract, the `program-calls` grader, its fixtures, and SPEC-414. The Wright surface can ship before the LPP capability, reporting `unavailable` for `names`.

## Compatibility impact

- **`wright-agent/v1`:** additive. `content` and `rule` are new optional request fields, and `content`, `names`, and `unavailable` are new optional result members. Requests without `content` get the current shapes unchanged.
- **Committed schema:** Wright's committed schema describes the owner's format through a contract test against the schema `workshop-rs` publishes. It is not a hand-maintained copy.
- **`rule-content-v1`:** versioned by `workshop-rs` and named in every content object. An incompatible change needs a new format id, and adopting it in Wright follows the agent contract's versioning rules.
- **LPP:** additive, with an optional negotiated capability.
- **Unaffected:** no syntax, Workshop semantics, lowering, diagnostic, or emitted-output behavior changes.
- **Benchmark:** `program-calls` is a new internal check kind. Existing scenarios can migrate from `compiled-contains` one check at a time, with their positive and negative fixtures.

## Scope boundaries

- Not decided: a query or pattern language in Wright, a typed authoring API, normalization of equivalent forms, argument-level spans, and content for DEL/OSTW. DEL/OSTW has no provider today and gets content when it has one.
- This exposes content. It does not judge whether a rule satisfies a requirement, and it does not replace owner compiler or semantic tests.
- Exact event-filter ids, reserved ids for constructs that are not catalog entries, and the API shape belong to `workshop-rs`. The capability's wire shape belongs to `language-provider-protocol`.
