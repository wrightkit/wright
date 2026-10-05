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
   - `language`: `workshop` or `overpy`. One language per request; Wright does not merge results across languages.
   - `query`: free text. It may be a display name, a near spelling, or a guess.
   - `kind` (optional): `action`, `value`, `event`, `enumMember`, or `setting`.
   - `within` (optional): the identity of a callable, an enum domain, or a settings path prefix, to list its parameters, members, or children.
   - `locale` (optional): the locale used to read Workshop display names.
   - `limit` (optional): at most the maximum advertised in `capabilities`.

3. **Result.** `entries` and `unavailable`.
   - Each entry has an owner-issued `identity`, `kind`, `spelling`, and `displayName` for the locale. The result names the owner once, not per entry, and entry order is the ranking, so entries carry no per-entry `owner` or `match` field.
   - `spelling` is in the requested language and, for a callable, is its signature as a single string in that language's call syntax, so a Workshop display name queried with `language: overpy` returns the OverPy form the agent will write.
   - **Signature form.** Parameters appear in call order. A required parameter is `name: Type`. An optional parameter shows its default: `name=default`, or `name?` when the default is null. An enum-typed parameter shows its domain. The members of that domain are listed inline only for a required parameter whose domain has at most 32 members, and only once per entry. For an optional enum parameter the default is the answer and the members are not listed. A larger domain shows its member count, and `within` lists or filters it. Types of optional non-enum parameters are omitted.
   - The default `limit` is 3.
   - Results are ranked and bounded. The service never returns a whole catalog or settings table.
   - `unavailable` reports a missing owner capability with the owner and the capability, never a text-based replacement.

4. **Owners match, rank, and spell; Wright composes.** For `workshop`, `workshop-rs` supplies identities, display names, signatures, and settings. For `overpy`, `opy-rs` supplies OverPy spellings, signatures, and settings, including its mapping from Workshop display names to OverPy spellings. Wright performs no fuzzy matching of its own, stores no names, and does not translate between languages. Owner behavior is specified in `wrightkit/workshop-rs#377` and `wrightkit/opy-rs#465`.

5. **No loaded project is required.** `lookup` answers from owner data. An OverPy request needs the provider process but not a loaded project. If a `wright serve` session cannot host the operation without an input, that is an implementation constraint to resolve, not a reason to require a project.

6. **Input stays strict.** Wright and the owners do not accept non-upstream spellings as aliases for agent convenience. Agents learn spellings through `lookup`. Unknown-name diagnostics carry the owner's nearest candidates unchanged and name `lookup` as the way to get more; Wright generates no suggestions of its own. Whether `opy-rs` should stop accepting catalog spellings that the upstream compiler rejects is an owner decision tracked in `wrightkit/opy-rs#466`.

7. **OverPy requires a provider capability.** `lookup` for `overpy` needs a new optional, negotiated LPP capability, added through the protocol's additive change rules. This is the provider-backed consumer that ADR-0017 decision 10 required before extending LPP. Its wire shape is decided in `wrightkit/language-provider-protocol`, not here. Until the provider advertises the capability, `lookup` for `overpy` returns `unavailable` naming `opy-rs`.

8. **The tool description is the discovery channel.** `capabilities.operations` advertises `lookup`, and its description says to use it before writing in an unfamiliar language and when a name is rejected. Wright skills may mention it but are not required for it to be found or to work.

9. **Contract change.** The operation is additive within `wright-agent/v1`. The MCP adapter gains `wright_lookup`, which updates the initial tool set in ADR-0020 and the agent contract.

10. **ADR-0017 is unchanged.** `lookup` entries use the owner's opaque identity. Where an owner issues a canonical identity, it is the identity ADR-0017's canonical selector accepts. For an OverPy-specific entry with no canonical identity, the identity is owner-specific and Wright invents no cross-language identity. Reconciling ADR-0017's `Proposed` status is separate work.

## Alternatives considered

- **Fold lookup into `domainIntelligence`:** rejected for now because it couples a small resolver to an unimplemented result model and blocks the work on ADR-0017 acceptance. Revisit when `domainIntelligence` ships.
- **Extend `targetMetadata`:** rejected for the reason ADR-0017 gives. It is a bulk summary, and growing it would return the catalog to the agent in one payload.
- **Accept aliases in OverPy:** rejected. Workspace principle 7 makes the upstream compiler the executable specification, and accepting non-upstream spellings would make `check` pass source the reference compiler rejects.
- **Ship a name list or manual in a skill:** rejected. It would be a shadow semantic authority that can drift from the owners, and skills are optional.
- **A structured per-parameter signature (`name`, `type`, `default` objects):** rejected. For `hudText` it is about 204 tokens against about 74 for the string form, and no consumer needs the breakdown. A machine-readable form can be added as an optional field when one does.
- **Return the whole name index in one tool result:** rejected for token cost. Bounded, ranked results answer the question the agent has.
- **Typed or AST-style authoring, or Wright-enforced constrained generation:** deferred. Wright does not control the agent's decoding. A machine-readable vocabulary export by the owners could serve harnesses that do, and `lookup` entries should stay derivable from such an export.
- **A program scaffold generator:** deferred. Top-level structure was about 10% of failing OverPy calls; diagnostics and one canonical example per language cover it.

## Consequences

- An agent can find the valid spelling, signature, enum members, and settings keys for a construct in one bounded call instead of guessing, mining the binary, or fetching external documentation.
- Names and signatures stay with their owners. Wright cannot drift from them because it keeps no copy.
- The MCP tool set grows by one tool, and its schema is part of every session's context. Keep the request schema small.
- Cost is dominated by what stays in the agent's context: in the stage 1 benchmark every tool result was re-read each turn, so a result's size counts once per remaining turn. This is why the default `limit` is small and why enum members are listed only where the agent cannot omit the argument.
- Ranking and match quality become visible product behavior and must be measured, not assumed.
- Follow-up work: owner implementations (`wrightkit/workshop-rs#377`, `wrightkit/opy-rs#465`); an LPP capability decision in `wrightkit/language-provider-protocol`; the Wright operation, MCP tool, and contract documentation; and the benchmark prerequisites `wrightkit/wright#526` and `wrightkit/benchmark-suite#2`.

## Compatibility impact

No syntax, diagnostic code, span, normalized output, or semantic behavior changes. Under the current `wright-agent/v1` and `wright-result/v1` rules the operation is additive. The candidate lists added to diagnostics are additional content on existing codes.

Verification the implementation must provide:

- Every spelling `lookup` returns for OverPy compiles under the pinned reference comparison when used as returned, and is derived from the data that drives compilation, so a test fails if the two diverge. The owner issues carry the matching checks.
- CLI JSON, `wright serve`, and MCP return the same structured result for the same request.
- The operation does not require a loaded project, and an unsupported owner capability is reported as `unavailable`.
- A paired agent run on the same models and scenarios, with network isolation enforced, shows the failing calls per trial and the tokens to a first valid OverPy program fall from the stage 1 baseline, or attributes what remains to a concrete owner or model limitation, as `wrightkit/wright#482` requires.

## Scope boundaries

This ADR does not decide:

- the ranking algorithm, match thresholds, or the exact grammar of the signature string beyond the rules in decision 3;
- the LPP method, capability id, or wire shape;
- how settings paths with hero or team templates are represented in `entries`;
- DEL/OSTW, whose implementation is frozen;
- Wright-curated guidance, which remains ADR-0017;
- an example or scaffold operation;
- a machine-readable vocabulary export by the owners;
- the owner of the raw Workshop "insufficient evidence to detect the Workshop client language" result on small files, which still needs an owner.
