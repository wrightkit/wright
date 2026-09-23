# CLI architecture, commands, and conversion

[← CLI contract index](../cli.md)

## Architecture

```text
input (file | directory | `-` stdin)
    ↓  discovery: kind detection, locale, root, identity (wright-driver::input)
CompilerSession (wright-driver)
    ├─ source adapter: OPY | Workshop | protocol JSON
    ├─ validation (WIR)
    ├─ lowering (HIR → WIR)
    ├─ analysis (SemanticService: semantic facts, symbols, references, CFG)
    ├─ emission (Workshop text)
    └─ reconstruction (WIR → canonical OPY source, #126)
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
| `wright check [INPUT]` | Parse, lower, validate, and report correctness diagnostics | verdict and validation diagnostics |
| `wright analyze [INPUT]` | Summarize project structure, ranked CFG hotspots, and cross-cutting state | bounded semantic report with static evidence labels |
| `wright lint [INPUT]` | Parse, lower, lint; report findings | findings, rule metadata, and effective-configuration summary |
| `wright inspect [INPUT]` | Parse, lower, and inspect exhaustive semantic facts | rules, symbols, references summary |
| `wright completion <SHELL>` | Generate static completion script for bash, zsh, fish, or powershell | the generated completion script |
| `wright completion install [SHELL]` | Install generated completion into standard user-local directory | installation progress and guidance |
| `wright update` | Self-update a standalone installation | update progress (text only) |

`wright version` and `wright --version` print the implementation version
banner (`wright <version> (wright-driver <version>)`); the version is the
single authoritative workspace implementation version and is also reported
inside every `wright-result/v1` envelope.

All commands accept a file path, a project directory, or `-` for stdin. An
omitted input uses the current directory. Input kind is detected from the
extension (`.opy`, `.ostw`/`.del`, `.json`, `.txt`/`.ws`) or, for a directory,
from the source files it contains; mixed source kinds fail with structured
ambiguity guidance. Stdin content is auto-detected (protocol JSON starts with
`{`, otherwise Workshop text). Detection can be overridden with
`--kind auto|opy|ostw|workshop|protocol`. `--locale`
overrides Workshop client-locale detection; `--root` sets the include/project
root; `-o/--output` writes compiled output to a file. `.ostw`/`.del` inputs
remain recognized source kinds for a future provider, but Wright does not ship
a static DEL/OSTW adapter. Every DEL/OSTW workflow fails with the structured
`source-provider-unavailable` diagnostic (exit 4), without partial output or
an upstream/static fallback. DEL/OSTW provider support is not currently
shipped with Wright and is outside this contract. OPY, Workshop, and protocol
inputs continue through their existing owner-backed paths.

The rationale for current-directory defaults, directory targets, and explicit
ownership ambiguity is recorded in
[`ADR-0016`](../adr/0016-current-directory-and-directory-project-targets.md).

## `wright convert` and the reconstruction surface (#126)

`wright convert [INPUT] --target opy|ostw` reconstructs **validated Workshop
input** as canonical OPY source, or refuses the recognized OSTW target because
DEL/OSTW provider support is not currently shipped, through the shared
driver/session conversion operation (`CompilerSession::convert`). The CLI is
a thin passthrough: it parses argv, builds the session, calls the driver
workflow, and renders the envelope. Reconstruction logic lives in the underlying
language crates rather than the CLI layer. The driver reuses its own `load()` path (kind detection, Workshop
parsing, WIR validation) and delegates OPY reconstruction to the language-owned
reconstructor; OSTW is an explicit provider boundary and never uses a static
Wright implementation.

* The target flag is **required and explicit** (`--target opy|ostw`); a
  missing or unknown target is a usage error (exit 2), and `--target` on any
  other command is a usage error too.
* Only Workshop input is accepted: the available conversion surface is
  Workshop → OPY. Workshop → OSTW is recognized but unavailable because no
  DEL/OSTW provider is shipped, with **no direct OPY ↔ OSTW path**.
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
  [`docs/opy/support-matrix.md`](../opy/support-matrix.md) and
  [`docs/ostw/support-matrix.md`](../ostw/support-matrix.md).

The conversion boundary is covered by the CLI and driver contract tests. They
assert the available Workshop → OPY provider handoff and the explicit
Workshop → OSTW refusal; owner reconstruction semantics are tested in the
owning language repository.
