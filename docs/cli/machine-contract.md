# CLI machine-readable contract

[← CLI contract index](../cli.md)

## Exit codes

| Code | Meaning | Examples |
| --- | --- | --- |
| 0 | success | clean check, compiled program written, reconstructed source produced |
| 1 | source/user error | parse error, validation error, ambiguous input, unknown input kind, unreadable input, refused downgrade, non-Workshop `convert` input, a deliberate LPP provider refusal (`provider-refusal-<refusalCode>`), an owner-side project load failure (`project-load-failed`) |
| 2 | usage error | unknown command/flag, missing option value, missing/unknown `convert --target`, `compile -o -` with `--format json`, the undeclared `compile --to` locale refusal `invalid-locale`, and the `compile` output refusals `output-exists` (existing file without a terminal or `--force`), `output-is-input`, and `output-unnamed` (carried in the envelope) |
| 3 | recognized but unsupported | `.opy` stdin via the explicit adapter fallback (default path is native), package-manager-managed installation for `update self`, unsupported platform for `update self`, a `convert` reconstruction rejection (a construct outside the declared OPY/OSTW reconstruction surface), an LPP capability the provider did not negotiate (`capability-unavailable`) |
| 4 | internal/environment failure | catalog corruption, adapter bridge missing, I/O failure writing output, `update` network/checksum/extraction failure, provider transport/process failures (`provider-spawn`, `provider-io`, `provider-timeout`, `provider-exited`, `provider-malformed`, `jsonrpc-error`), provider resolution failures (`provider-missing`, `provider-offline`, `provider-download`, `provider-integrity`, `provider-install`, `provider-unsupported-platform`) |

Exit codes are deterministic for identical inputs and configuration and are
also carried inside the JSON envelope (`exit` field), so agents never need to
infer them from process state alone.

## stdout / stderr ownership

* Text mode: the command result goes to stdout; diagnostics go to stderr.
* JSON mode: exactly one envelope goes to stdout; stderr stays empty on
  success. Usage errors are the only case that writes to stderr without an
  envelope (exit 2).
* `wright compile` writes the program to a file (ADR-0025) and never to the
  result: text and JSON carry the verdict, diagnostics, and
  `result.output.{written_to, sha256, locale, input_identity}`, with no
  program text. Only `wright compile -o -` writes the program to stdout, and
  then nothing else (text mode only).

## `wright-result/v1` envelope

```json
{
  "wright": { "version": "0.1.0", "contract": "wright-result/v1" },
  "command": "analyze",
  "ok": true,
  "exit": 0,
  "diagnostics": [],
  "result": {
    "program": { "origin": { "kind": "workshop", "locale": "en-us" }, "rules": 2 },
    "counts": { "risks": { "total": 0, "error": 0, "warning": 0, "info": 0 }, "persistentObjects": 0 },
    "cost": { "elementCount": 21 },
    "hotspots": [],
    "complexity": { "blocks": 4, "edges": 4, "loopBlocks": 1, "waitBlocks": 1 },
    "risks": [],
    "variables": [],
    "conditionTrees": [],
    "persistentObjects": { "total": 0, "byKind": {}, "withoutRetainedIdentity": 0, "visibleToAllPlayers": 0 },
    "expand": "--all / all: true; inspect symbols, refs, cfg, callgraph, cost for detail"
  }
}
```

`check`, `lint`, `analyze`, and `inspect` JSON results carry the same items as
the default text report. JSON is emitted compact. Long lists are capped at 20
items with totals and an expansion hint; `--all` lifts the cap, while explicit
filters and `--max` still apply. The corresponding agent operations accept
`all: true`. Complete fact dumps are removed from `analyze` and bare `inspect`
on external surfaces; query specific details through the `inspect` subcommands.
The Rust `CompilerSession` API still returns complete typed results. Verdicts
and exit codes are based on the complete input, before selection or capping.

Stable contract fields: `wright.contract`, `command`, `ok`, `exit`,
`diagnostics[].code/stage/severity/span/source`, and each command's
`result` shape. Human-readable `message` wording is explicitly not part of the
machine contract.

Diagnostic codes are stable per stage: `parse-error`, `unknown-*`,
`unsupported-construct`, `settings-invalid`, `settings-placement` (frontend),
`settings-unknown-key`, `settings-unknown-value` (validation), `convert-error`/
`lower-error` (lowering), `validation-error` (validation), `input-*`/
`stdin-*` / `profile-workshop-only` / `invalid-locale` (discovery), `output-exists` / `output-is-input` / `output-declined` / `output-unnamed` (discovery, compile destination), `output-io` and the compile-time client-import
diagnostics `target-element-limit` / `element-count-unavailable` (emission),
analysis findings reuse the
analyzer's codes, including the name-addressing refusals `unknown-symbol`,
`ambiguous-symbol`, `unknown-rule`, and `ambiguous-rule` (#429), and
`*-internal` / `*-unavailable` (internal).
`source-provider-unavailable` marks the explicit DEL/OSTW provider boundary
and is reported at the internal stage. Provider failures keep the
provider's typed classification in the diagnostic code, on the
source-provider seam and in `providerValidateEdit`
mutation results alike: an LPP refusal is
`provider-refusal-<refusalCode>` (a Wright-originated refusal keeps its own
code, e.g. `session-config-changed`), a capability gap is
`capability-unavailable`, and transport/process failures keep their
`provider-*` codes, so a consumer classifies the failure from `code`/`stage`
without parsing `message` (#569, #570). Provider-edit mutation results also
carry the refusal's `refusalCode` or the failure's typed code verbatim in
`provider_code`.
A `convert`
reconstruction rejection carries the language-owned reconstructor's stable
code (e.g. `unsupported-per-player-loop` from `wright-opy`,
`reconstruct-unsupported-action` from an OSTW provider) with stage
`reconstruction`; `convert-input-kind` (discovery) rejects non-Workshop
`convert` input, and `manifest-error`/`catalog-error` from a reconstructor
map to the internal stage.

The native `.opy` frontend's builtin-resolution stage adds the stable codes
`unknown-action`, `unknown-value`, `unknown-member`, `invalid-arity`,
`invalid-receiver`, `enum-domain-mismatch`, `action-in-value-position`,
`value-in-action-position`, `invalid-call-context`, and `invalid-iterable`
(semantic resolution against the OPY compatibility manifest, #109; all
source-located). Named/keyword argument binding adds `unknown-keyword`,
`duplicate-argument`, `missing-argument`, `positional-after-keyword`,
`keyword-required`, `keyword-unsupported`, and `invalid-argument`
(variable-required parameters; #110).

## `wright-result/v1` evolution

The envelope follows the same rule as `wright-agent/v1`: additive optional
`result` fields are permitted within v1; removing or renaming a field,
changing a field's type or meaning, or changing the envelope or exit-code
model requires a new major contract such as `wright-result/v2`.

Recorded exceptions apply before the 1.0 contract freeze (#134).
[ADR-0025](../adr/0025-essential-default-output.md) replaces full external
results with essential defaults, removes `brief`, adds `all`, and makes JSON
compact without bumping the contract id. Diagnostic codes are single
kebab-case tokens: `unknown-enum member` and the other space-carrying
`unknown-<kind>` codes (owner `kind` phrases such as `enum domain`,
`localized string`, and `event player`) are hyphenated, so consumers match
`unknown-enum-member`, `unknown-enum-domain`, `unknown-localized-string`,
and `unknown-event-player` (#789). `costEstimate`'s `exact.programRules`
counts every declared program rule — disabled rules included — matching the
`program` summary and `elementCount.rules` rather than the live rule subset
(#788), and its `findings`/`skipped` members mirror the configured-registry
set `lint` serves. The earlier exception remains:
the `lint` result's `rules` member lists only each rule's `id` and
`effectiveSeverity` (#431). `lintRules` serves the full rule metadata once,
covering summary, rationale, documentation, known limits, evidence, and tags,
instead of inlining it into every `lint` result.

ADR-0030 prerequisites added two additive members (#791): a finding carries
`repeated` — `{expression, count, spanKind}` where `spanKind` is
`expression` or `enclosing-loop` — when a `repeated-value` rule produced it,
stating as data what the message states in prose; and a `selection` summary
carries `byCode`, per-code counts over the filtered set before `max`
truncation, so a capped result still reports every code's real count. The
same per-code totals appear where a list is truncated: a `truncation`
record's `byCode` counts the capped list's full set, and a `counts` group's
`byCode` counts the set the group totals — both computed before truncation.

## Determinism

For identical inputs and configuration, JSON output is byte-deterministic
(no timestamps, no environment-dependent ordering). Input identity is the
SHA-256 of the input bytes (`result.output.input_identity`); for provider-backed
directory targets, the owner supplies the identity of its selected primary
source text. Emitted artifacts carry their own SHA-256
(`result.output.sha256`).
