# Wright CLI and Driver Contract

Status: accepted baseline (living driver and CLI contract)
Scope: `wright` executable, `wright-driver` crate, and their machine-readable
contracts

## CLI presentation and completion (#164)

The `wright` command model is defined once with `clap`. The same model drives
argv parsing, generated help, and the static completion entry point:

```text
wright completion bash
wright completion zsh
wright completion fish
wright completion powershell
```

Workflow commands accept these CLI-only presentation options:

* `--format text|json` selects human or machine output (`-f` remains an alias).
* `--renderer auto|terminal|plain|github-actions` selects the presentation
  environment. `auto` selects GitHub Actions when `GITHUB_ACTIONS` is truthy,
  plain output for generic `CI` or a non-TTY, and terminal output otherwise.
* `--color auto|always|never` controls ANSI color. Explicit options take
  precedence over environment detection; GitHub Actions keeps workflow
  command lines free of ANSI even when color is explicitly requested.

JSON output is one `wright-result/v1` envelope on stdout with no ANSI, progress,
or workflow commands. `compile` and `convert` source artifacts remain the only
stdout payload in text mode, including when GitHub Actions presentation is
selected. GitHub Actions diagnostics and findings are emitted as escaped
workflow annotations; grouping is sent to the workflow command stream and a
concise PASS/WARN/ERROR line is appended to `GITHUB_STEP_SUMMARY` when the
runner provides that file. The summary uses the highest structured severity:
errors produce `ERROR`, warnings produce `WARN`, and info/notice-only results
produce `PASS`.

This document is the normative contract for the compiler driver and CLI.
It defines the shared driver model, the command surface, exit codes,
stdout/stderr ownership, and the `wright-result/v1` envelope that CI and
agents consume.

## Architecture

```text
input (path | stdin)
    ↓  discovery: kind detection, locale, root, identity (wright-driver::input)
CompilerSession (wright-driver)
    ├─ frontend: .opy bridge | native Workshop | protocol JSON
    ├─ validation (WIR)
    ├─ lowering (HIR → WIR)
    ├─ analysis (SemanticService: findings, symbols, references, CFG)
    ├─ emission (Workshop text)
    └─ reconstruction (WIR → canonical OPY/OSTW source, #126)
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

| Command | Purpose | Text-mode stdout |
| --- | --- | --- |
| `wright compile [INPUT]` | Parse, lower, validate, emit Workshop text | the emitted artifact (or nothing with `-o`) |
| `wright convert [INPUT] --target opy\|ostw` | Reconstruct validated Workshop input as canonical OPY or OSTW source | the reconstructed source |
| `wright check [INPUT]` | Parse, lower, validate, analyze | `check: ok` (or nothing on failure) |
| `wright analyze [INPUT]` | Parse, lower, analyze | findings and summary |
| `wright lint [INPUT]` | Parse, lower, lint; report findings | findings, rule metadata, and effective-configuration summary |
| `wright inspect [INPUT]` | Parse, lower, inspect structure | rules, symbols, references summary |
| `wright update` | Self-update a standalone installation | update progress (text only) |

`wright version` and `wright --version` print the implementation version
banner (`wright <version> (wright-driver <version>)`); the version is the
single authoritative workspace implementation version and is also reported
inside every `wright-result/v1` envelope.

All commands accept a file path or `-`/omitted for stdin. Input kind is
detected from the extension (`.opy`, `.ostw`/`.del`, `.json`, `.txt`/`.ws`) or
stdin content (protocol JSON starts with `{`, otherwise Workshop text) and can
be overridden with `--kind auto|opy|ostw|workshop|protocol`. `--locale`
overrides Workshop client-locale detection; `--root` sets the include/project
root; `-o/--output` writes compiled output to a file. `.ostw`/`.del` inputs
are parsed by the native OSTW frontend (`wright-ostw`), which loads the
`ds.toml` project closure, resolves the reachable imports, and lowers the
#118 semantic HIR through the same validate→lower→validate path as the other
frontends; `check`, `lint`, `analyze`, and `inspect` then run the shared
analyzer/semantic services over that program and report project-relative
multi-file provenance and the #118 boundary diagnostics. `compile` (#119)
lowers the reachable #118 semantic surface through the shared HIR → WIR →
Workshop pipeline and emits en-US Workshop text; it fails deterministically
with structured, source-located diagnostics when the reachable surface is
outside the declared support matrix (see
[`docs/ostw/support-matrix.md`](ostw/support-matrix.md)) or the project
boundary is unresolved (e.g. missing imports).

## `wright convert` and the reconstruction surface (#126)

`wright convert [INPUT] --target opy|ostw` reconstructs **validated Workshop
input** as canonical source for the selected target through the shared
driver/session conversion operation (`CompilerSession::convert`). The CLI is
a thin passthrough: it parses argv, builds the session, calls the driver
workflow, and renders the envelope — no reconstruction logic lives in the CLI
layer. The driver reuses its own `load()` path (kind detection, Workshop
parsing, WIR validation) and delegates per target to the language-owned
reconstructors, `wright_opy::reconstruct::reconstruct` and
`wright_ostw::reconstruct::reconstruct`, unchanged.

* The target flag is **required and explicit** (`--target opy|ostw`); a
  missing or unknown target is a usage error (exit 2), and `--target` on any
  other command is a usage error too.
* Only Workshop input is accepted: the declared conversion surface is
  Workshop → OPY and Workshop → OSTW, with **no direct OPY ↔ OSTW path**.
  A non-Workshop input fails with the structured `convert-input-kind`
  diagnostic (exit 1).
* The result is **canonical reconstructed source** for the selected target
  (`result.text`) plus its deterministic SHA-256 (`result.sha256`) and the
  target (`result.target`). Reconstruction is semantic, not original-source
  recovery: comments, formatting, macros, functions, and source abstractions
  are not recovered (see the support matrices for the exact reconstructed and
  rejected surfaces).
* Non-representable constructs fail deterministically with the
  reconstructor's stable structured diagnostics (stage `reconstruction`,
  exit code 3) and **never carry partial source**. The supported directions
  and their limits are documented in
  [`docs/opy/support-matrix.md`](opy/support-matrix.md) and
  [`docs/ostw/support-matrix.md`](ostw/support-matrix.md).

The cross-format round-trip acceptance suite lives in
`crates/wright-driver/tests/convert.rs` and writes the machine-readable
report `target/wright-convert-report.json` (one entry per committed fixture:
`Workshop → convert → native frontend → HIR → WIR → Workshop` for both
targets, plus the deterministic rejection entries).

## `wright lint` and the lint configuration

`wright lint` runs through the same compiler/session pipeline as the other
commands and reports structured findings with stable rule IDs, configured
severity, an evidence class, and original source identity/spans where
available. It reuses the lint registry, so rule enable/disable/severity
configuration is deterministic and identical across CLI and programmatic
(`CompilerSession::lint`, tool/agent `lint`) use.

Two lint-only flags configure the registry; both are repeatable:

* `--disable-rule <ID>`: disable a rule by stable ID (`min-wait-loop`,
  `duplicate-condition`, `expensive-loop-check`, `repeated-value`,
  `while-without-wait`).
* `--rule-severity <ID>:<warning|info>`: override a rule's severity.

These flags are usage errors on every other command (exit 2).

The `lint` result envelope carries `input_identity` (the SHA-256 source
identity; the tool/agent API exposes the same value as `inputIdentity`),
`program`, `rules`, `config`, and `findings`:

```json
{
  "wright": { "version": "0.1.0", "contract": "wright-result/v1" },
  "command": "lint",
  "ok": true,
  "exit": 0,
  "diagnostics": [],
  "result": {
    "input_identity": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    "program": { "origin": { "kind": "workshop", "locale": "en-us" }, "rules": 2, "findings": 1 },
    "rules": [
      {
        "id": "min-wait-loop",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "loop body waits at the workshop minimum rate",
        "evidence": "static-indicator",
        "tags": ["performance", "stability"],
        "knownLimits": "Wait durations that are not statically known ..."
      },
      {
        "id": "duplicate-condition",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "condition is evaluated more than once within one rule",
        "evidence": "exact",
        "tags": ["correctness"],
        "knownLimits": "Detection is structural (not value-flow) and rule-local ..."
      },
      {
        "id": "expensive-loop-check",
        "defaultSeverity": "info",
        "effectiveSeverity": "info",
        "enabled": true,
        "summary": "geometry predicate evaluated inside a loop body",
        "evidence": "heuristic",
        "tags": ["performance"],
        "knownLimits": "The expensive-call list is a fixed heuristic ..."
      },
      {
        "id": "repeated-value",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "identical value expression evaluated more than once in one loop scope",
        "evidence": "exact",
        "tags": ["performance", "stability"],
        "knownLimits": "Detection is rule-local and structural ..."
      },
      {
        "id": "while-without-wait",
        "defaultSeverity": "warning",
        "effectiveSeverity": "warning",
        "enabled": true,
        "summary": "while loop body contains no wait call",
        "evidence": "static-indicator",
        "tags": ["stability"],
        "knownLimits": "Counter-pattern detection is conservative and structural: only literal-bound comparisons (<, <=, >, >=) are recognized. A statically-bounded claim additionally requires every direct child ..."
      }
    ],
    "config": { "rules": { "min-wait-loop": { "enabled": true, "severity": "warning" } } },
    "findings": [
      {
        "code": "min-wait-loop",
        "severity": "warning",
        "evidence": "static-indicator",
        "message": "loop body waits at the workshop minimum rate; ...",
        "span": { "file": 0, "path": "program.txt", "start": { "line": 28, "col": 9 }, "end": { "line": 31, "col": 13 } }
      }
    ]
  }
}
```

Analysis findings (`analyze`, `lint`, and the tool/agent `getFindings`/`lint`
responses) carry an `evidence` field classifying how strongly the finding is
supported (`exact`, `static-indicator`, `heuristic`, `runtime-validated`).

Finding spans carry a machine-readable `path` resolved root-relative to the
input include root (`--root`, defaulting to the input's directory): file 0 is
the main input, and additional files in a multi-file program resolve from the
program file registry. The same source location therefore reports the same
`path` across `analyze`, `lint`, and the tool/agent `Findings`/`Lint` surfaces
regardless of how the input was spelled (absolute, relative, or
cwd-relative); stdin inputs report `<stdin>`.

`while-without-wait` findings additionally carry a machine-readable
`boundedness` field (`obviously-unbounded` | `statically-bounded` | `unknown`)
classifying the no-yield loop's repetition evidence, and their severity is
derived from that evidence class: `warning` for an obviously unbounded or
unknown loop, `info` for a statically bounded no-yield loop. A statically
bounded no-yield loop is never treated as equivalent to an unbounded one. For
example, the agent-lab repro `loop-waitless.opy` (wrightkit/agent-lab#68) is a
finite 10-iteration counter loop and reports as a statically bounded `info`
finding:

```json
{ "code": "while-without-wait", "severity": "info", "boundedness": "statically-bounded", "message": "loop body contains no wait call; the loop is statically bounded by a counter against a literal bound, ..." }
```

Non-`while-without-wait` findings carry `"boundedness": null`.

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

## `wright update` (self-update)

`wright update` upgrades a **standalone** installation (one created by
`install.sh` or by unpacking a release archive manually) from the canonical
GitHub Release artifacts (the same archives and checksums the installer and
the package-manager manifests consume). It is not a compiler workflow, so it
is text-only and produces no `wright-result/v1` envelope.

* `wright update`: resolve the latest stable release, download the platform
  archive and its published SHA-256 checksum, verify the checksum before
  touching anything, extract, and atomically replace `wright` and
  `wright-lsp` in the running executable's directory, then smoke-check both
  binaries report the new version.
* `wright update --check`: resolve and report whether an update is
  available without modifying the installation.
* `wright update --version <VERSION>`: install an exact version instead of
  the latest stable release. Refuses a downgrade (the installed version is
  newer) with exit 1.

Supported platforms mirror `install.sh`: Linux x86_64 and macOS
(x86_64/arm64), mapped to the release target matrix in `docs/release.md`.
On Windows, standalone self-update is refused with guidance to
`winget upgrade WrightKit.Wright` / `scoop update wright` (exit 3).

Package-manager-managed installations are detected from the executable's
location (Homebrew/Cellar, Scoop, WinGet paths) and refused with guidance to
the channel's own upgrade command (exit 3); `wright update` never overwrites
a binary it does not own. A missing `wright-lsp` next to `wright`, or an
unwritable installation directory, fails with reinstall guidance (exit 4).

Environment overrides (test/advanced hooks, matching `install.sh`):

* `WRIGHT_INSTALL_BASE_URL`: base URL of release artifacts
* `WRIGHT_API_URL`: URL used to resolve the latest release
* `WRIGHT_INSTALL_OS` / `WRIGHT_INSTALL_ARCH`: override platform detection

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
`unsupported-construct`, `settings-invalid`, `settings-placement` (frontend),
`settings-unknown-key`, `settings-unknown-value` (validation), `convert-error`/
`lower-error` (lowering), `validation-error` (validation), `input-*`/
`stdin-*` (discovery), `output-io` (emission), analysis findings reuse the
analyzer's codes, and `*-internal` / `*-unavailable` (internal). A `convert`
reconstruction rejection carries the language-owned reconstructor's stable
code (e.g. `unsupported-per-player-loop` from `wright-opy`,
`reconstruct-unsupported-action` from `wright-ostw`) with stage
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
SHA-256 of the input bytes (`result.output.input_identity`); emitted artifacts
carry their own SHA-256 (`result.output.sha256`).

## The `.opy` frontend

`.opy` inputs are compiled by the native Rust frontend (`wright-opy`): no
Node, no OverPy, and stdin `.opy` is supported (the include root defaults to the
working directory for stdin, `--root` for files). The pinned OverPy adapter
remains available only as an explicit compatibility fallback by setting
`WRIGHT_ADAPTER_PATH`; it is never selected silently. The frontend surface is
declared in [`opy/support-matrix.md`](opy/support-matrix.md).

## Library reuse

External Rust consumers depend on `wright-driver` (never the CLI) and drive
`CompilerSession::new(config)` → `compile`/`check`/`analyze`/`inspect`/`lint`
or `convert(ConvertTarget)`, each returning a typed `Envelope<T>`. Loading is
idempotent (`Session::load`), and the driver exposes the resolved locale,
input identity, and origin metadata. See `crates/wright-driver/tests/driver.rs`
and `crates/wright-driver/tests/convert.rs` for the reusable test surface.
