---
kind: wright-spec/v1
id: SPEC-309-extensible-lint-rules
title: Extensible canonical Workshop analysis and lint rules
status: accepted
related_issue: "#309"
owner: Wright
freshness: live
---

## Goal

Provide a small, local, and stable contract for adding analysis and lint
policies without moving Workshop semantics into Wright or introducing a plugin
registry. Semantic facts come from the canonical `workshop-rs` WIR; a rule
definition only selects facts and describes the resulting policy finding.

The contract has three layers:

| Layer | Owner | Contract |
| --- | --- | --- |
| Canonical facts | `workshop-rs` and source owners | WIR rules, events, conditions, actions, values, spans, and canonical identities |
| Rule policy | Wright analyzer | Native Rust analyses or bounded declarative matchers over those facts |
| Project configuration | Project/user | Local YAML rule definitions, enablement, severity, and bounded options |

Wright does not infer runtime behavior from a structural match, duplicate
language semantics, or guess an unknown localized spelling.

## Declarative rule definition

Rules are local `.yaml` or `.yml` files. A directory is loaded in lexical file
order. A rule ID must contain exactly one slash and use a namespace other than
the reserved `wright` namespace. Built-in rules retain their bare stable IDs;
external rules use IDs such as `community/minimum-wait`.

```yaml
id: community/minimum-wait
locale: en-US                 # default: en-US
metadata:
  summary: loop contains a minimum wait
  rationale: minimum waits can create high-frequency loops
  documentation: Finds a minimum wait inside a while scope.
  known-limits: This is structural and does not measure runtime cost.
  tags: [performance]
matcher:
  scope: while
  event: global                # optional canonical or localized event identity
  conditions:                  # optional value pattern and count
    call:
      name: Is Alive
    count: { min: 1, max: 1 }
  actions:
    - kind: call
      name: Wait
      args:
        - number: 0.1
      count: { min: 1 }
```

The public matcher vocabulary is intentionally Workshop-shaped:

- `scope` is `rule`, `while`, `for-global-variable`,
  `for-player-variable`, or `if`. A loop/branch scope includes nested action
  nodes in its body; nested loops are also independently matched.
- `actions` uses direct predicates: `kind`, localized or canonical `name`,
  positional `args`, catalog-backed named `parameters`, `count.min`,
  `count.max`, and `present: false` for an absence predicate. Named parameters
  use canonical catalog labels with locale-independent normalization. An
  omitted `kind` matches any action. A call name and argument shape only match
  call actions.
- value patterns are one of `number`, `string`, `boolean`, `call`, `enum`, or a
  numeric `comparison` using `<`, `<=`, `>`, or `>=`.
  A call and enum recursively use the same canonical name/domain/member
  resolution. An empty value pattern means any value.
- counts and options are static matched-node counts. They are evidence about
  the authored WIR shape, not a runtime execution count.

`locale` is document-wide and defaults to `en-US`. Localized spellings are
resolved through the owning Workshop catalog; canonical identities are also
accepted. Unknown or ambiguous spellings reject the rule at load time, so a
rule cannot silently change meaning between locales. Matching executes against
the canonical identity, not the spelling in the YAML file.

## Project configuration

The project lint file is local YAML and has no registry or package-manager
behavior:

```yaml
rules:
  community/minimum-wait:
    enabled: true
    severity: warn
    options:
      min-matches: 1
      max-matches: 10
```

`enabled` defaults to `true`; `severity` may be `off`, `warn`, or `error`; the
only shared rule options are the bounded `min-matches` and `max-matches`
limits. Evidence classification and a declarative rule's default finding
severity are assigned by Wright from the canonical matcher contract; external
metadata cannot override them.
Unknown configuration keys are rejected. CLI flags apply to the same
configuration object: `wright lint --lint-config project.yaml --rule rules/
--disable-rule ID --rule-severity ID:warn`.

## Query and finding output

`LintRules` exposes every native and declarative rule with stable ID, kind,
summary, rationale, documentation, known limits, evidence, tags, enabled
state, and default/effective severity. `GetFindings`, the driver lint result,
and JSON CLI output use the external ID unchanged in `code`, together with
severity, message, source span, canonical WIR node references, and evidence.

The rule metadata and matcher are loaded together; there is no separate
registry metadata file. The same metadata is available to CLI, driver/tool,
agent, embedding, and documentation consumers. This contract is query-only:
it does not authorize source edits or automatic fixes.

Rules run only after their canonical semantic dependencies have been loaded.
The analyzer never fabricates a finding when a required owner fact is absent;
the lint result exposes an additive `skipped` array containing the rule ID,
Workshop rule index, and machine-readable reason. Invalid local definitions
remain structured `lint-rule-error` diagnostics at load time. Future owner
capabilities must use the same unavailable/skip boundary rather than turning
missing evidence into a lint failure or guessed finding.

## Scope boundaries

This contract does not define a general query language, source-language
grammars, Rust/WIR internals, ABI/WASM/JS/runtime protocols, package
registries, remote rule loading, plugins, or automatic edits. Programmable
extensions remain a separate future design.

## Design ablation

The public surfaces are independently required by the workflow:

1. Removing canonical facts leaves no owner-neutral input for a policy.
2. Removing the declarative matcher leaves local rules unable to express
   nested structural patterns, absence, literals, and static counts.
3. Removing the registry prevents deterministic composition and configuration.
4. Removing metadata/query output prevents CLI, agent, embedding, and docs
   consumers from discovering rule meaning and evidence.
5. Removing local YAML loading changes the contract into a compiled-in rule
   set and violates the project-local extensibility requirement.

The analyzer contract tests cover these seams with a nested `while`/`Wait`
pattern, canonical and localized names, namespaced identity, metadata, config
overrides, bounded options, and unknown-option rejection.
