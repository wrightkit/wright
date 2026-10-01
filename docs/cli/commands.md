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
   Envelope<T> (typed result + diagnostics + exit code; finding selection
   narrows reported sets without touching the verdict, #430)
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
| `wright compile [INPUT]` | Parse, lower, validate, emit Workshop text; warn on known client import limits | the emitted artifact (or nothing with `-o`) |
| `wright convert [INPUT] --target opy\|ostw` | Reconstruct validated Workshop input as canonical OPY or OSTW source | the reconstructed source |
| `wright check [INPUT]` | Parse, lower, validate, and report correctness diagnostics | verdict and validation diagnostics |
| `wright analyze [INPUT]` | Summarize Workshop cost, ranked complexity hotspots, performance/stability risk indicators, and cross-cutting state | bounded semantic report with exact/static/heuristic evidence labels |
| `wright lint [INPUT]` | Parse, lower, lint; report findings | findings, rule id/severity summary, and effective configuration |
| `wright inspect [INPUT]` | Parse, lower, and inspect exhaustive semantic facts | rules, symbols, references summary, and the detail command per area |
| `wright inspect symbols [INPUT] [--only KIND]` | List semantic symbols, optionally narrowed to one kind | the symbol list with resolved locations |
| `wright inspect refs <NAME> [INPUT]` | References and usage counts for one symbol, addressed by name | usage-count header plus the reference list |
| `wright inspect cfg <RULE> [INPUT]` | Control-flow graph of one rule, addressed by name | block/edge listing |
| `wright inspect callgraph [INPUT]` | Subroutine call graph (caller rules → callee subroutines) | call edges |
| `wright inspect cost [INPUT]` | Exact generated-resource counts plus static findings | resource counts and findings |
| `wright rename <NAME> <NEW_NAME> [INPUT]` | Semantically rename a Workshop variable or subroutine | per-source diff of the validated edits; `--write` applies them |
| `wright serve [INPUT]` | Serve `wright-agent/v1` over stdio or JSON-RPC 2.0 | one structured response per request |
| `wright completion <SHELL>` | Generate static completion script for bash, zsh, fish, or powershell | the generated completion script |
| `wright completion install [SHELL]` | Install generated completion into standard user-local directory | installation progress and guidance |
| `wright update [self\|provider [NAME]]` | Update Wright-managed components: a standalone installation and installed first-party providers | update progress (text only) |

`wright --version` prints the implementation version banner
(`wright <version> (wright-driver <version>)`); the version is the single
authoritative workspace implementation version and is also reported inside
every `wright-result/v1` envelope. `wright --help` is the canonical help
surface.

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

Wright surfaces known exact Workshop client import constraints as warnings
without failing the command (#488): a program whose canonical element count
exceeds the Overwatch client's 32768-element import limit still compiles,
emits its artifact, and exits 0, carrying a `target-element-limit` warning
that reports the observed count and names the largest contributing rule. The
same warning rides the `lint`, `analyze`, and `inspect` envelopes — anywhere
the whole program is evaluated — while `check` does not evaluate client
importability and `wright analyze` remains the detailed cost and hotspot
surface. The distinction is deliberate: `compile` means Wright produced
valid Workshop text, not that the current client imports it under every
resource limit.

The rationale for current-directory defaults, directory targets, and explicit
ownership ambiguity is recorded in
[`ADR-0016`](../adr/0016-current-directory-and-directory-project-targets.md).

Commands that report findings (`check`, `analyze`, `lint`, `inspect cost`)
share the finding-selection options `--severity`, `--rule-id`, `--file`, and
`--max`, which narrow reported diagnostics/findings without changing verdicts
or exit codes; see [lint configuration and findings](lint.md) and
[presentation](presentation.md).

## Semantic query commands (#429)

`inspect` owns the semantic query surface: the bare command prints the
bounded summary, and its five subcommands — `inspect symbols`,
`inspect refs`, `inspect cfg`, `inspect callgraph`, `inspect cost` — expose
each detail area. They run the same operations the agent contract serves
(`symbols`, `references` + `usage`, `cfg`, `callGraph`, `costEstimate`)
through the session's `ToolService`, so a CLI `result` payload equals the
agent operation's payload for the same input. Nesting them under `inspect`
keeps the top-level command surface to distinct user intents (#439); the
agent request names stay flat operation names, not command paths.

`inspect refs <NAME>` and `inspect cfg <RULE>` address their target by its
declared name — a `variables`/`subroutines` entry or a `rule("name")` —
resolved against the loaded program's semantic index inside the driver.
Callers never need the program's numbering, where symbol ids and rule
indexes are different spaces (the agent contract keeps accepting numeric
ids too). An unmatched name is a structured `unknown-symbol`/`unknown-rule`
diagnostic with exit 1; a name shared by several candidates is
`ambiguous-symbol`/`ambiguous-rule` listing the numeric ids to fall back to —
resolution never guesses.

* `inspect refs` reports the `usage` counts (`reads`, `writes`, `calls`,
  `rules`) as the header of the reference list; there is no separate usage
  command. The reference list includes the declaration entry alongside
  reads and writes, with identifier spans reported by the analyzer (#433).
* `inspect symbols --only <KIND>` narrows the list to `globalVariable`,
  `playerVariable`, `subroutine`, or `rule` (kebab-case aliases work). It is
  spelled `--only` because `--kind` already selects the input frontend.
* `inspect cost` accepts the finding-selection options, applied to its
  findings list exactly as on `costEstimate`.
* `persistentObjects` has no standalone command; `analyze` reports the same
  facts under `result.facts.persistentObjects`.

Text output follows the human-first presentation described in
[presentation](presentation.md): each query leads with the target identity and
its summary — the program inventory and a bounded rule preview for bare
`inspect`, kind-grouped symbols with primary locations for `symbols`, the
usage counts before the reference list for `refs`, the graph shape before
block detail for `cfg`, fan-in/fan-out highlights before the edge list for
`callgraph`, and exact totals before findings for `cost`. Long lists show a
first page of ten entries followed by the withheld count; `--format json`
always prints the complete result.

## `wright rename` — semantic rename for raw Workshop (#434)

`wright rename <NAME> <NEW_NAME> [INPUT]` renames a global variable, player
variable, or subroutine in raw Workshop input. `NAME` is the declared symbol
name — the same addressing `inspect refs` uses — resolved against the loaded
program's semantic index; an unmatched name is `unknown-symbol` and a name
shared by several symbols is `ambiguous-symbol` (exit 1).

The rename is semantic, not textual: every edit rewrites exactly the
identifier span `workshop-rs` records for one declaration or reference, so
`Global.score` rewrites `score` and leaves the `Global.` prefix, and comments
or string literals containing the name stay untouched. The proposed
transaction is validated by reparsing the edited source through the session's
own `workshop-rs` path; a name that does not survive reparsing, or one whose
references no longer bind to the renamed symbol, refuses with
`rename-mismatch` and no partial edit set.

* Default is **preview only**: text mode prints the diff as `-`/`+` line
  pairs, and JSON mode returns the validated `transaction` and per-source
  `preview` inside the `wright-result/v1` envelope — callers (and agents via
  `semanticRename`) carry the same atomic edit set.
* `--write` applies the validated transaction to the input file atomically
  (a sibling temporary file and rename). The write rechecks each source's
  identity hash first; a file that changed since validation refuses with
  `edit-stale-source` and writes nothing.
* Other source kinds are provider surfaces: OPY input refuses with
  `edit-requires-provider` naming `providerSemanticRename`, and kinds without
  a shipped provider keep their `source-provider-unavailable` refusal.

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
