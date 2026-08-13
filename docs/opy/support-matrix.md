# M7 Native `.opy` Frontend Support Matrix

Status: reviewable contract for milestone M7 (issue #42)
Scope: the `.opy` source-language surface Wright's native frontend supports,
with the production/corpus evidence for each feature and the explicitly
deferred constructs

This document is the audit deliverable of [#42]. Every claimed feature is
backed by the compatibility corpus (`compatibility/fixtures/**/source.opy`
and the pinned adapter HIR fixtures) or marked as investigation. The
architecture is `lexer → preprocess → CST/parser → resolve/lower → Opy HIR`
(see [`ARCHITECTURE.md`](../../ARCHITECTURE.md) and `crates/wright-opy`).

[#42]: https://github.com/wrightkit/wright/issues/42

## Evidence sources

| Source | Use |
| --- | --- |
| `compatibility/fixtures/{basic-rule,control-flow,declarations-rules,expressions-values,preprocessing,diagnostics}/source.opy` | Synthetic corpus surface |
| `compatibility/fixtures/real-world/overpy-cake/source.opy` | Real-world surface (arrays, macros, effects) |
| `adapter/fixtures/**/*.json` | The pinned OverPy 9.7.10 HIR reference for each source |
| `crates/wright-opy/tests/differential.rs` | Native-vs-reference parity suite (machine-readable report at `target/wright-differential-report.json`) |

## Supported surface (corpus-evidenced)

### Lexing
- Identifiers, integer and decimal number literals (source text preserved),
  double-quoted strings with `\n`/`\t`/`\\` escapes, `true`/`false`/`None`.
- Line comments (`#`), block comments (`/* */`), `#!` directives.
- Operators: `+ - * / // % ** == != < <= > >= = += -= *= /= //= %= and or not in`,
  plus `.`/`,`/`:`/`(`/`)`/`[`/`]`/`@`.

### Declarations
- `globalvar name` / `globalvar name = expr` / `globalvar name <index>`
  (the bare-integer form is an explicit Workshop variable index, matching the
  reference; literal-number `=` initializers are dropped from HIR, matching
  the reference adapter — non-trivial initializers such as arrays and `vect`
  calls are preserved).
- `playervar name` (same forms).
- `subroutine name`.
- `def name():` subroutine bodies (parameters are outside the declared
  surface; rejected explicitly).
- `enum Name: MEMBER, ...` — members fold to numeric constants (`Phase.FINISHED`
  → `1`), matching the reference.
- `macro name(params):` statement bodies with `MacroParam` references.

### Preprocessing (issue #44)
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

### Expressions and resolution (issue #45)
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

### Diagnostics (issue #43)
- Malformed input produces structured `FrontendError`s (stable codes like
  `parse-error`, `lex-error`) with 1-based source spans; the parser recovers
  at statement boundaries to report multiple useful errors.
- Native diagnostics map into the shared `wright-result/v1` contract (stage
  `frontend`, severity `error`).

## Deferred / out of scope (v0.3)

- `.opy` reconstruction from Workshop text; decompiler architecture.
- Macro/`#!define` values that require runtime evaluation (no scripting).
- Additional OverPy enum spellings beyond the corpus table (a data change).
- Rule `disabled` markers and custom-game-settings blocks (no corpus
  evidence; the adapter already rejects settings outside the boundary).
- Backslash line continuation (`\` at end of line inside string
  concatenations / macro bodies) — rejected at lexing (native `lex-error`
  `unexpected character '\'` on ow1-emulator and 6v6-adjustments); the pinned
  oracle 9.7.10 compiles it (its failures on those fixtures occur later, on
  unrelated constructs).
- Postfix increment/decrement (`++`/`--`) — rejected at parsing (native
  `parse-error` `expected an expression but found '+'` on `++` in
  overpy-cronch, `cronch.opy:32`; `--` occurs in overpy-meipocalypse but
  is never the first failure (the dict-literal lex-error preempts it)).
- Dict literals (`{...}`) — rejected at lexing (native `lex-error`
  `unexpected character '{'` on meipocalypse, `meipocalypse.opy:223`).
- Triple-quoted strings / docstrings (`"""`) — rejected at lexing (native
  `lex-error`, `unterminated string literal`, on zencopter, `heli.opy:38`);
  the pinned oracle 9.7.10 also fails on this construct (recorded reference
  limitation).
- Subroutine parameters, default `@Team`/`@Slot` overrides, named arguments.
- Full OverPy formatting semantics: `debug()`/`print()` emission
  (`Create HUD Text` etc.) is an M8 emission item, not a frontend one.

## Boundary contract

The native frontend produces `wright_core::hir::Program` (Opy HIR v1) with the
same protocol envelope, file registry, declarations, and rules as the
reference adapter — verified by the differential suite at the HIR boundary
(spans and the producer identity normalized away). It never requires Node or
OverPy; the adapter remains available as an explicit `WRIGHT_ADAPTER_PATH`
fallback and as the pinned compatibility oracle.
