# CLI machine-readable contract

[← CLI contract index](../cli.md)

## Exit codes

| Code | Meaning | Examples |
| --- | --- | --- |
| 0 | success | clean check, compiled artifact produced, reconstructed source produced |
| 1 | source/user error | parse error, validation error, ambiguous input, unknown input kind, unreadable input, refused downgrade, non-Workshop `convert` input |
| 2 | usage error | unknown command/flag, missing option value, missing/unknown `convert --target` |
| 3 | recognized but unsupported | `.opy` stdin via the explicit adapter fallback (default path is native), package-manager-managed installation, unsupported platform for `update`, a `convert` reconstruction rejection (a construct outside the declared OPY/OSTW reconstruction surface) |
| 4 | internal/environment failure | catalog corruption, adapter bridge missing, I/O failure writing output, `update` network/checksum/extraction failure |

Exit codes are deterministic for identical inputs and configuration and are
also carried inside the JSON envelope (`exit` field), so agents never need to
infer them from process state alone.

## stdout / stderr ownership

* Text mode: the command result goes to stdout; diagnostics go to stderr.
* JSON mode: exactly one envelope goes to stdout; stderr stays empty on
  success. Usage errors are the only case that writes to stderr without an
  envelope (exit 2).
* `wright compile` without `-o` writes the raw artifact to stdout in text
  mode; in JSON mode the artifact is the `result.output.text` field of the
  envelope.

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
    "facts": {
      "symbols": [{ "id": 0, "kind": "globalVariable", "name": "counter", "usage": { "reads": 1, "writes": 1, "calls": 0, "rules": 1 } }],
      "rules": [{ "id": 0, "name": "loop", "controlFlow": { "blocks": 4, "edges": 4, "loopBlocks": 1, "waitBlocks": 1 } }]
    }
  }
}
```

Stable contract fields: `wright.contract`, `command`, `ok`, `exit`,
`diagnostics[].code/stage/severity/span/source`, and each command's
`result` shape. Human-readable `message` wording is explicitly not part of the
machine contract.

Diagnostic codes are stable per stage: `parse-error`, `unknown-*`,
`unsupported-construct`, `settings-invalid`, `settings-placement` (frontend),
`settings-unknown-key`, `settings-unknown-value` (validation), `convert-error`/
`lower-error` (lowering), `validation-error` (validation), `input-*`/
`stdin-*` (discovery), `output-io` (emission), analysis findings reuse the
analyzer's codes, and `*-internal` / `*-unavailable` (internal).
`source-provider-unavailable` marks the explicit DEL/OSTW provider boundary
and is reported at the internal stage.
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

## Determinism

For identical inputs and configuration, JSON output is byte-deterministic
(no timestamps, no environment-dependent ordering). Input identity is the
SHA-256 of the input bytes (`result.output.input_identity`); for provider-backed
directory targets, the owner supplies the identity of its selected primary
source text. Emitted artifacts carry their own SHA-256
(`result.output.sha256`).
