# ADR-0021: Name and signature lookup for agent authoring

- Status: Proposed
- Date: 2026-10-05
- Related: [Issue #482](https://github.com/wrightkit/wright/issues/482), [Issue #521](https://github.com/wrightkit/wright/issues/521), [ADR-0010](0010-independent-implementations-and-wright-integration.md), [ADR-0017](0017-domain-intelligence-query-contract.md), [ADR-0020](0020-native-agent-tool-adapter.md), [Agent contract](../agent-contract.md)

## Context

An agent that does not already know Workshop or OverPy cannot ask Wright what a construct is called, what it takes, or which enum members and settings keys are valid. Wright reports diagnostics after the agent has guessed. It has no surface for the question the agent actually has.

Evidence comes from the stage 1 agent benchmark (Wright 0.8.0, three models, 144 trials, every `check` and `compile` output and agent action log read):

- Authoring OverPy from scratch is the expensive case: median tokens to a first valid program were about 630K for OverPy against about 91K for raw Workshop, and OverPy greenfield trials averaged 79 failing `check`/`compile` calls against 12.
- Of 1,597 failing OverPy calls, about 58% contained a name-class diagnostic (`unknown-action`, `unknown-value`, `unknown-member`, `unknown-enum-member`, `unknown-identifier`), about 16% a rejected settings key, and about 6% a wrong or missing argument. Syntax and top-level structure were about 10%.
- The guesses are systematic: a Workshop display name converted to camelCase (`createHudText` for `hudText`), enum members tried by variation, and 164 distinct settings keys rejected.
- With no way to ask Wright, agents read the Wright binary with `strings`, searched the host for example files, fetched the upstream OverPy repository, and used `wright convert` on Workshop they had written as a Workshop-to-OverPy dictionary.

Existing surfaces do not cover this:

- `targetMetadata` returns entry counts and enum members, not spellings or signatures.
- `domainIntelligence` (ADR-0017, not implemented, still `Proposed`) selects by canonical identity or source position and returns owner facts and Wright guidance. It does not resolve a free-text guess into an identity, which is the step the agent is missing.

Reaching the owners differs by language. Raw Workshop is in-process through `workshop-rs`. OverPy is reached through the language-provider protocol, where capabilities are optional and negotiated.

## Decision

1. **One new operation, `lookup`, on the shared `ToolService` surface, and one new top-level CLI command, `wright lookup`.** It resolves free text to owner entries. It is not folded into `domainIntelligence`: resolving a guess to an identity and returning facts and guidance about an identity are different jobs, `lookup` can ship without the unimplemented ADR-0017 result model, and `domainIntelligence` can later accept `lookup`'s identities as its canonical selector. It is not nested under `wright inspect`: `inspect` queries a loaded program and defaults to the current directory as its input, while `lookup` queries a language and must work in an empty workspace. Agents start from `wright --help`, and a top-level command is visible there. As with ADR-0020, the MCP tool is `wright_lookup`, with its input schema derived from the request type.

2. **Request.**
   - `language`: `workshop` or `opy`, the ids `capabilities.languages` already reports. One language per request; Wright does not merge results across languages.
   - `query`: free text. It may be a display name, a near spelling, or a guess.
   - `kind` (optional): `action`, `value`, `event`, `enumMember`, or `setting`.
   - `within` (optional): the identity of a callable, an enum domain, or a settings path prefix, to list its parameters, members, or children.
   - `locale` (optional): the locale used to read Workshop display names.
   - `limit` (optional): the default is 3 and the maximum is 10. Both are defined by this contract and do not depend on `capabilities`, which carries no per-operation metadata.

3. **Result.** `entries` and `unavailable`.
   - Each entry has an owner-issued `identity`, `kind`, `spelling`, `displayName` for the locale, and, for a callable, `signature`. The result names the owner once, not per entry, and entry order is the ranking, so entries carry no per-entry `owner` or `match` field.
   - `spelling` is the bare spelling in the requested language, as the owner defines it. It stays stable and is never a rendered signature, so a Workshop display name queried with `language: opy` returns the OverPy spelling the agent will write.
   - `signature` is a single string that Wright renders from owner facts. It is a presentation of those facts, not a second copy of them.
   - Results are ranked and bounded. The service never returns a whole catalog or settings table.
   - `unavailable` reports a missing owner capability with the owner and the capability, never a text-based replacement.

4. **Owners supply facts; Wright renders the signature.**
   - For `workshop`, `workshop-rs` supplies identities, display names, parameter and domain facts, and settings. For `opy`, `opy-rs` supplies OverPy spellings, parameter and domain facts, and settings, including its mapping from Workshop display names to OverPy spellings. Owners match and rank. Wright performs no fuzzy matching of its own, stores no names, and does not translate between languages. Owner behavior is specified in `wrightkit/workshop-rs#377` and `wrightkit/opy-rs#465`.
   - The facts an owner supplies for a callable are its ordered parameters, each with a name, a type, whether it is required, its default, and its enum domain, plus how the callable is invoked where the language needs it (for example a receiver), and for each domain its member spellings. Owners do not apply token-budget rules.
   - Wright renders the signature from those facts. Parameters appear in call order. A required parameter is `name: Type`. An optional parameter shows its default, `name=default`, or `name?` when the default is null. An enum-typed parameter shows its domain. Members are listed inline only for a required parameter whose domain has at most 32 members, once per entry. For an optional enum parameter the default is the answer and members are not listed. A larger domain shows its member count, and `within` lists or filters it. Types of optional non-enum parameters are omitted. These are Wright presentation rules and may change without an owner change.

5. **`lookup` does not read a program.** It joins `capabilities`, `targetMetadata` and the `provider*` operations as an operation that answers without requiring or triggering a project load (#512, [agent contract](../agent-contract.md)). An OverPy request needs the provider process but not a loaded project.

6. **Input stays strict, and diagnostic candidates travel in the message text.**
   - Wright and the owners do not accept non-upstream spellings as aliases for agent convenience. Whether `opy-rs` should stop accepting catalog spellings that the upstream compiler rejects is an owner decision tracked in `wrightkit/opy-rs#466`.
   - The `wright-result/v1` diagnostic has no field for candidates and its schema is closed, and the LPP Diagnostic carries only a range, severity, code, message and source. Owners therefore put the nearest valid candidates in the diagnostic `message`, and no schema or protocol field is added. Wright does not rewrite owner messages, and owners do not name Wright commands.
   - Wright's text presentation of `check` and `compile` adds a one-line pointer to `wright lookup` after a diagnostic with an unknown-name code. The structured result is unchanged. Whether a structured candidate field is worth a later schema and protocol change is decided only if a consumer needs one.

7. **OverPy requires a provider capability.** `lookup` for `opy` needs a new optional, negotiated LPP capability, added through the protocol's additive change rules. This is the provider-backed consumer that ADR-0017 decision 10 required before extending LPP. Its wire shape is decided in `wrightkit/language-provider-protocol`, not here. Until the provider advertises the capability, `lookup` for `opy` returns `unavailable` naming `opy-rs`.

8. **Discovery channels.** `capabilities.operations` lists `lookup` by name only; it has no description field. An agent finds the operation through the `wright_lookup` MCP tool description and through `wright lookup --help`, which `wright --help` lists. Both say to use it before writing in an unfamiliar language and when a name is rejected. In the stage 1 condition (the CLI, `bin` level), `wright --help` and the command help are the channel. Wright skills may mention it but are not required for it to be found or to work.

9. **Contract change.** The operation is additive within `wright-agent/v1`. The MCP adapter gains `wright_lookup`, which updates the initial tool set in ADR-0020 and the agent contract.

10. **Settings paths with templates.** A template segment keeps the owner's spelling verbatim, as in the current diagnostics (`heroes.<team>.<hero>.health`). An entry's `spelling` is that path, and `within` matches a path prefix segment by segment against the owner's spelling, so `heroes` lists the next segment and `heroes.<team>.<hero>` lists the settings under it. Whether a template segment can also be listed with its allowed values is an owner detail in `wrightkit/opy-rs#465` and `wrightkit/workshop-rs#377`.

11. **ADR-0017 is reconciled in this change.** It was approved when #323 closed on 2026-09-14 and `docs/architecture/domain-intelligence.md` was merged as the current contract, but its status stayed `Proposed`. This change marks it `Accepted`, so the design authority this ADR relies on is unambiguous. `lookup` entries use the owner's opaque identity. Where an owner issues a canonical identity, it is the identity ADR-0017's canonical selector accepts. For an OverPy-specific entry with no canonical identity, the identity is owner-specific and Wright invents no cross-language identity.

## Alternatives considered

- **Fold lookup into `domainIntelligence`:** rejected for now because it couples a small resolver to an unimplemented result model and blocks the work on ADR-0017 acceptance. Revisit when `domainIntelligence` ships.
- **Extend `targetMetadata`:** rejected for the reason ADR-0017 gives. It is a bulk summary, and growing it would return the catalog to the agent in one payload.
- **Accept aliases in OverPy:** rejected. Workspace principle 7 makes the upstream compiler the executable specification, and accepting non-upstream spellings would make `check` pass source the reference compiler rejects.
- **Ship a name list or manual in a skill:** rejected. It would be a shadow semantic authority that can drift from the owners, and skills are optional.
- **A structured per-parameter signature in the result (`name`, `type`, `default` objects):** rejected. For `hudText` it is about 204 tokens against about 74 for the rendered string, and no agent consumer needs the breakdown. Owners still supply structured facts to Wright; only the result carries the string.
- **Owners render the signature string:** rejected. It would copy Wright's token-budget policy into two owners and let the string drift from `spelling`.
- **Return the whole name index in one tool result:** rejected for token cost. Bounded, ranked results answer the question the agent has.
- **Typed or AST-style authoring, or Wright-enforced constrained generation:** deferred. Wright does not control the agent's decoding. A machine-readable vocabulary export by the owners could serve harnesses that do, and `lookup` entries should stay derivable from such an export.
- **A program scaffold generator:** deferred. Top-level structure was about 10% of failing OverPy calls; diagnostics and one canonical example per language cover it.

## Consequences

- An agent can find the valid spelling, signature, enum members, and settings keys for a construct in one bounded call instead of guessing, mining the binary, or fetching external documentation.
- Names and signatures stay with their owners. Wright cannot drift from them because it keeps no copy.
- The MCP tool set grows by one tool, and its schema is part of every session's context. Keep the request schema small.
- Cost is dominated by what stays in the agent's context: in the stage 1 benchmark every tool result was re-read each turn, so a result's size counts once per remaining turn. This is why the default `limit` is small and why enum members are listed only where the agent cannot omit the argument.
- Ranking and match quality become visible product behavior and must be measured, not assumed.
- Diagnostic candidates are free text in the owner's message, so their format is an owner choice and Wright cannot parse them.
- Follow-up work: owner implementations (`wrightkit/workshop-rs#377`, `wrightkit/opy-rs#465`); an LPP capability decision in `wrightkit/language-provider-protocol`; the Wright operation, MCP tool, and contract documentation; and the benchmark prerequisites `wrightkit/wright#526` and `wrightkit/benchmark-suite#2`.

## Compatibility impact

No syntax, diagnostic code, span, normalized output, or semantic behavior changes. Under the current `wright-agent/v1` and `wright-result/v1` rules the operation is additive. Candidate lists are additional text in the `message` of existing diagnostic codes, so no `wright-result/v1` schema change and no LPP Diagnostic change is needed. The only protocol change is the optional LPP capability in decision 7.

Verification the implementation must provide:

- Every spelling `lookup` returns for OverPy compiles under the pinned reference comparison when used as returned, and is derived from the data that drives compilation, so a test fails if the two diverge. The owner issues carry the matching checks.
- CLI JSON, `wright serve`, and MCP return the same structured result for the same request.
- `lookup` answers in a session whose project cannot load and does not trigger a load, and an unsupported owner capability is reported as `unavailable`.
- A paired agent run under the same condition as stage 1: the `bin` level with the same `wright-skill`, the same models and scenarios, and network isolation enforced, with `lookup` as the only difference. It shows the failing calls per trial and the tokens to a first valid OverPy program fall from the stage 1 baseline, or attributes what remains to a concrete owner or model limitation, as `wrightkit/wright#482` requires. A comparison at the `mcp` level stays under ADR-0020 and is not attributed to `lookup`.

## Scope boundaries

This ADR does not decide:

- the ranking algorithm, match thresholds, or the exact signature grammar beyond the rules in decision 4;
- the LPP method, capability id, or wire shape;
- whether a structured candidate field on diagnostics is ever added;
- DEL/OSTW, whose implementation is frozen;
- Wright-curated guidance, which remains ADR-0017;
- an example or scaffold operation;
- a machine-readable vocabulary export by the owners;
- the owner of the raw Workshop "insufficient evidence to detect the Workshop client language" result on small files, which still needs an owner.
