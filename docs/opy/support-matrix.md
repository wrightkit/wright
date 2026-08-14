# Native .opy Frontend Support Matrix

Status: accepted baseline — living .opy frontend support matrix
Scope: the `.opy` source-language surface Wright's native frontend supports,
with production/corpus evidence for each feature and explicitly deferred
constructs

Every claimed feature is backed by the compatibility corpus
(`compatibility/fixtures/**/source.opy` and pinned adapter HIR fixtures) or
marked as investigation. The architecture is `lexer → preprocess → CST/parser →
resolve/lower → Opy HIR` (see [`docs/architecture.md`](../architecture.md) and
`crates/wright-opy`).

## Evidence sources

| Source | Use |
| --- | --- |
| `compatibility/fixtures/{basic-rule,control-flow,declarations-rules,expressions-values,preprocessing,diagnostics}/source.opy` | Synthetic corpus surface |
| `compatibility/fixtures/real-world/overpy-cake/source.opy` | Real-world surface (arrays, macros, effects) |
| `adapter/fixtures/**/*.json` | Pinned OverPy 9.7.10 HIR reference for each source |
| `crates/wright-opy/tests/differential.rs` | Native-vs-reference parity suite (machine-readable report at `target/wright-differential-report.json`) |

## Supported surface (corpus-evidenced)

### Lexing
- Identifiers, integer and decimal number literals (source text preserved),
  double-quoted strings with `\n`/`\t`/`\\` escapes, `true`/`false`/`None`.
- Line comments (`#`), block comments (`/* */`), `#!` directives.
- Operators: `+ - * / // % ** == != < <= > >= = += -= *= /= //= %= and or not`,
  plus `.`/`,`/`:`/`(`/`)`/`[`/`]`/`@`. (`in` is only the `for ... in`
  header keyword; expression-level `in`/`not in` membership operators are not
  supported — see the deferred list.)

### Declarations
- `globalvar name` / `globalvar name = expr` / `globalvar name <index>`
  (the bare-integer form is an explicit Workshop variable index, matching the
  reference; integer-`0` literal initializers are dropped from HIR (matching
  the reference adapter); non-zero and non-integer numeric initializers are
  preserved, e.g. `j = 5` and `k = 0.0` keep the source spelling through
  emission).
- `playervar name` (same forms).
- `subroutine name`.
- `def name():` subroutine bodies (parameters are outside the declared
  surface; rejected explicitly).
- `enum Name: MEMBER, ...` — members fold to numeric constants (`Phase.FINISHED`
  → `1`), matching the reference.
- `macro name(params):` statement bodies with `MacroParam` references.

### Preprocessing
- `#!include "file.opy"` — root-relative include resolution, cycle detection
  (`include-cycle`), missing-file diagnostics (`include-not-found`), included
  files registered in the HIR file registry (reference behavior).
- `#!define NAME value` — object-like macros; recursive expansion at use sites
  (a define may reference earlier defines); recursion guard
  (`macro-recursion`).
- `#!define name(args) value` — function-like macros with argument
  substitution (`cakeBeam(start, end, yPos) → createBeam(...)`).
- `#!undef NAME`.
- Unsupported directives fail explicitly (`unsupported-directive`).

### Rules and directives
- `rule "name":` with `@Event global` / `@Event eachPlayer` / `@Condition <expr>`.
- `@Team`/`@Slot` are accepted only without arguments (corpus events use
  OverPy defaults); other `@` directives fail explicitly.
- Statements: expression statements, `=` and augmented assignment,
  `if`/`elif`/`else`, `for x in range(...)`, `while`, `pass`.

### Expressions and resolution
- Literals, arrays `[...]`, parenthesized expressions.
- Calls (`range`, `len`, `abs`, `sqrt`, `debug`, `print`, `wait`, `createBeam`,
  `playEffect`, `getAllPlayers`, `disableInspector`, …).
- `vect(x, y, z)` → HIR `Vector` (3 arguments required; other arities are an
  explicit error).
- `"text".format(args)` → HIR `Format`; bare calls of declared subroutines →
  `CallSubroutine` statements; dotted module calls `random.uniform` /
  `random.choice` → `random.<name>` calls; `eventPlayer.member` →
  `PlayerVar`/receiver call on `EventPlayer`; variable receivers
  (`points.append`, `candlePos[i2]`) → `ReceiverCall`/`Index`.
- Builtin Workshop enums from the corpus: `Beam.{GOOD,GRAPPLE}`,
  `Color.{YELLOW,WHITE,RED,…}`, `DynamicEffect.{BAD_EXPLOSION,…}`,
  `EffectReeval.VISIBILITY`, `Wait.IGNORE_CONDITION`. Enum members outside the
  table fail explicitly (`unknown-enum-member`).
- `wait()` / `wait(duration)` default-argument filling: the reference appends
  `Wait.IGNORE_CONDITION` (and `0.016` for the no-argument form); native
  matches.
- Undeclared identifiers, enum types without members, and unsupported member
  accesses are structured, source-located semantic errors
  (`unknown-identifier`, `enum-type-without-member`, `unsupported-member`).

### Diagnostics
- Malformed input produces structured `FrontendError`s (stable codes like
  `parse-error`, `lex-error`) with 1-based source spans; the parser recovers
  at statement boundaries to report multiple useful errors.
- Native diagnostics map into the shared `wright-result/v1` contract (stage
  `frontend`, severity `error`).

### Settings
- Top-of-file `settings { ... }` custom-game-settings blocks (JSONC: quoted
  keys, `"`/`'` strings with escapes, numbers, `true`/`false`, string lists,
  nested groups, trailing commas) — recognized and consumed before lexing
  (scoped lexing: the block never enters the token stream and the lexer
  gains no global braces), parsed into the typed HIR `settings` payload, and
  emitted as the Workshop `settings` section before `variables`.
- Corpus-evidenced keys render per the emission table (fixture-evidenced
  data; see `crates/wright-ir/src/settings/table.rs`); keys, enum values,
  and map/hero list elements outside the table fail explicitly
  (`settings-unknown-key`/`settings-unknown-value`).
- Placement rules: the block must be the first construct in the main file
  (`settings-placement` otherwise); a second block is rejected; a
  `settings "file"` form is rejected (`settings-invalid`); settings blocks
  in included files are rejected (`settings-placement` at the included
  file's keyword span).
- The emitted `settings` section is deliberately not reparseable by the
  Workshop parser (a `.ws` decompiler is a non-goal); the settings-free
  round-trip guarantee is unchanged.

## Deferred / out of scope

- `.opy` reconstruction from Workshop text; decompiler architecture.
- Macro/`#!define` values that require runtime evaluation (no scripting).
- Additional OverPy enum spellings beyond the corpus table (a data change).
- Rule `disabled` markers (no corpus evidence for the source annotation).
- Expression-level `in`/`not in` membership operators — rejected at parsing
  (`for ... in` headers are supported).
- Backslash line continuation (`\` at end of line inside string
  concatenations / macro bodies) — rejected at lexing.
- Postfix increment/decrement (`++`/`--`) — rejected at parsing.
- Dict literals (`{...}`) — rejected at lexing.
- Triple-quoted strings / docstrings (`"""`) — rejected at lexing.
- Subroutine parameters, default `@Team`/`@Slot` overrides, named arguments.
- Full OverPy formatting semantics: `debug()`/`print()` emission
  (`Create HUD Text` etc.) follows the simplified semantic formatting documented
  in [`v1-matrix.md`](../v1-matrix.md).

## Boundary contract

The native frontend produces `wright_core::hir::Program` (Opy HIR v1) with the
same protocol envelope, file registry, declarations, and rules as the
reference adapter — verified by the differential suite at the HIR boundary
(spans and the producer identity normalized away). It never requires Node or
OverPy; the adapter remains available as an explicit `WRIGHT_ADAPTER_PATH`
fallback and as the pinned compatibility oracle.
