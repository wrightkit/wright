# ADR-0015: Canonical facts, declarative lint rules, and project policy

- Status: Accepted (backfilled)
- Date: 2026-09-12 (backfill date)
- Clarifies: [ADR-0011: Distinct check, lint, and analyze workflows](0011-distinct-tooling-workflows.md) and [ADR-0010: Independent language implementations and Wright integration](0010-independent-implementations-and-wright-integration.md)
- Related: [Issue #309](https://github.com/wrightkit/wright/issues/309),
  [PR #310](https://github.com/wrightkit/wright/pull/310),
  [SPEC-309](../specs/SPEC-309-extensible-lint-rules.md),
  [Issue #262](https://github.com/wrightkit/wright/issues/262),
  [PR #308](https://github.com/wrightkit/wright/pull/308)

## Historical note

This record was backfilled on 2026-09-12. The decision emerged through the
design and review of Issue #309 and PR #310. Issues #262 and #308 supplied
motivating analysis evidence but did not independently establish this
extension architecture. This ADR did not exist when those changes landed.

## Context

Wright's built-in lint rules should remain small and low false-positive, while
users need reusable Workshop-oriented policy across raw Workshop and
owner-backed source languages. Analysis becomes contextual quickly: a
persistent object or loop pattern can be legitimate depending on reevaluation,
lifecycle, and cleanup behavior. Encoding every heuristic as a built-in rule
would make the analyzer both brittle and hard to extend.

Workshop semantics, identities, and locale mappings already have canonical
owners. Wright needs a policy boundary that consumes those facts without
duplicating language semantics or exposing WIR/CFG implementation details.

## Decision

Wright separates four responsibilities:

1. **Canonical semantic facts** come from owner-backed Workshop evidence and
   expose structured identities, nodes, spans, and measurements.
2. **Rule definitions** describe what to match and how to explain it. Local
   YAML is the human-facing authoring form for bounded Workshop-shaped rules;
   rule identity, metadata, matcher, and explanation belong here.
3. **Project policy** independently enables/disables rules, assigns `off`,
   `warn`, or `error`, and supplies bounded options. Rule authors do not set
   project severity.
4. **Findings and skips** are Wright results that preserve stable rule IDs,
   source/provenance, evidence classification, and explicit unavailable/skip
   reasons when an owner fact is missing.

Rules canonicalize localized or canonical Workshop spellings through
`workshop-rs`. The YAML vocabulary is intentionally bounded and Workshop
shaped; it is not a general query language, plugin ABI, package registry, or
automatic edit contract. Native rules remain valid where a declarative matcher
would distort a richer analysis.

External rules use a namespaced identity; Wright-owned built-ins retain their
existing bare stable IDs and reserve the `wright` namespace. Declarative
metadata may explain evidence and limitations, but cannot self-certify
correctness or override Wright's evidence classification.

## Alternatives considered

- **Put all logic in native built-in rules:** rejected because it does not
  provide local reusable policy and encourages heuristic accumulation.
- **Expose a general-purpose YAML query/programming language:** rejected
  because it would duplicate Wright internals and create an unbounded public
  language surface.
- **Let rule definitions assign severity/evidence:** rejected because project
  policy and Wright/owner-backed evidence must remain independent of author
  declarations.
- **Use a remote registry or plugin runtime initially:** deferred because the
  concrete workflow only requires local files/directories; distribution and
  executable extension contracts need separate evidence.

## Consequences

- `analyze` can grow semantic facts without turning each fact into a lint
  finding, consistent with ADR-0011.
- The same canonical rule semantics can be reused across source languages when
  their owners provide sufficient canonical evidence and provenance.
- Missing owner capability is explicit and does not become a guessed finding
  or a whole-run failure.
- The rule and project configuration surfaces are stable enough for CLI,
  agent, embedding, query, and documentation consumers, while execution and
  source edits remain Wright-owned concerns.
- Programmable extensions require a separate evidence-backed decision.

## Compatibility impact

Built-in rule IDs remain stable. The contract strengthens the separation
between semantic facts, rule definitions, and project severity; it does not
move Workshop or source-language semantics into Wright. JSON remains the
machine-readable output form, while YAML is the local authoring form.

## Scope boundaries

- Additional facts and matcher capabilities require evidence from real rules
  and owner-backed workflows.
- Remote rule distribution and programmable runtimes are outside this ADR and
  require a separate decision.
