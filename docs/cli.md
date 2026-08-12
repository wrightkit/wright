# Wright CLI and Driver Contract (M6)

Status: accepted baseline for v0.3 (milestone M6)
Scope: `wright` executable, `wright-driver` crate, and their machine-readable
contracts

This document is the normative contract for the M6 compiler driver and CLI
(issues #37–#41). It defines the shared driver model, the command surface,
exit codes, stdout/stderr ownership, and the `wright-result/v1` envelope that
CI and agents consume.

## Architecture

```text
input (path | stdin)
    ↓  discovery: kind detection, locale, root, identity (wright-driver::input)
CompilerSession (wright-driver)
    ├─ frontend: .opy bridge | native Workshop | protocol JSON
    ├─ validation (WIR)
    ├─ lowering (HIR → WIR)
    ├─ analysis (SemanticService: findings, symbols, references, CFG)
    └─ emission (Workshop text)
            ↓
   Envelope<T> (typed result + diagnostics + exit code)
            ↓
  `wright` CLI: text rendering | JSON serialization
```

The CLI is a thin argv/presentation layer. Library consumers construct a
[`CompilerSession`] directly and receive the same typed envelopes; CLI JSON
output is the serialization of that exact model, never a separately formatted
result.

## Commands

| Command   | Purpose | Text-mode stdout |
| --- | --- | --- |
| `wright compile [INPUT]` | Parse, lower, validate, emit Workshop text | the emitted artifact (or nothing with `-o`) |
| `wright check [INPUT]` | Parse, lower, validate, analyze | `check: ok` (or nothing on failure) |
| `wright analyze [INPUT]` | Parse, lower, analyze | findings and summary |
| `wright inspect [INPUT]` | Parse, lower, inspect structure | rules, symbols, references summary |

All commands accept a file path or `-`/omitted for stdin. Input kind is
detected from the extension (`.opy`, `.json`, `.txt`/`.ws`) or stdin content
(protocol JSON starts with `{`, otherwise Workshop text) and can be overridden
with `--kind auto|opy|workshop|protocol`. `--locale` overrides Workshop
client-locale detection; `--root` sets the `.opy` include root; `-o/--output`
writes compiled output to a file.

## Exit codes

| Code | Meaning | Examples |
| --- | --- | --- |
| 0 | success | clean check, compiled artifact produced |
| 1 | source/user error | parse error, validation error, ambiguous input, unknown input kind, unreadable input |
| 2 | usage error | unknown command/flag, missing option value |
| 3 | recognized but unsupported | `.opy` on stdin (adapter bridge needs a file) |
| 4 | internal/environment failure | catalog corruption, adapter bridge missing, I/O failure writing output |

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
  "diagnostics": [
    {
      "code": "min-wait-loop",
      "stage": "analysis",
      "severity": "warning",
      "message": "loop body waits at the workshop minimum rate; ...",
      "span": { "file": 0, "path": "program.txt", "start": { "line": 28, "col": 9 }, "end": { "line": 31, "col": 13 } },
      "source": { "kind": "workshop", "locale": "en-us" }
    }
  ],
  "result": { "program": { "...": "..." }, "findings": [ "..."] }
}
```

Stable contract fields: `wright.contract`, `command`, `ok`, `exit`,
`diagnostics[].code/stage/severity/span/source`, and each command's
`result` shape. Human-readable `message` wording is explicitly not part of the
machine contract.

Diagnostic codes are stable per stage: `parse-error`, `unknown-*`,
`unsupported-construct` (frontend), `convert-error`/`lower-error` (lowering),
`validation-error` (validation), `input-*`/`stdin-*` (discovery), `output-io`
(emission), analysis findings reuse the analyzer's codes, and `*-internal` /
`*-unavailable` (internal).

## Determinism

For identical inputs and configuration, JSON output is byte-deterministic
(no timestamps, no environment-dependent ordering). Input identity is the
SHA-256 of the input bytes (`result.output.input_identity`); emitted artifacts
carry their own SHA-256 (`result.output.sha256`).

## The `.opy` frontend bridge

Until the native `.opy` frontend (M7), `.opy` inputs are converted through the
pinned OverPy adapter (`adapter/bin/wright-adapter.js`) via Node. The driver
locates the adapter from `WRIGHT_ADAPTER_PATH`, the repository root, or the
executable's location; a missing bridge is an environment failure (exit 4,
code `adapter-unavailable`) with actionable guidance. The M7 native frontend
replaces the bridge behind the same driver contract.

## Library reuse

External Rust consumers depend on `wright-driver` (never the CLI) and drive
`CompilerSession::new(config)` → `compile`/`check`/`analyze`/`inspect`, each
returning a typed `Envelope<T>`. Loading is idempotent (`Session::load`), and
the driver exposes the resolved locale, input identity, and origin metadata.
See `crates/wright-driver/tests/driver.rs` for the reusable test surface.
