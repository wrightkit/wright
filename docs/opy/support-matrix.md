# Native .opy Frontend Support Matrix

Status: accepted baseline — living .opy frontend support matrix
Scope: the `.opy` source-language surface Wright's native frontend supports,
with production/corpus evidence for each feature and explicitly deferred
constructs

This matrix records the **corpus-evidenced current surface**. The
forward-looking, tiered baseline (what is planned, evidence-prioritized, or
demand-driven) lives in
[`compatibility-baseline.md`](compatibility-baseline.md), and the pinned
reference identity behind both is recorded centrally in
[`docs/compatibility/upstream-references.md`](../compatibility/upstream-references.md).

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
  emission). Initializer semantics are profile-independent: the Initialize
  rules are synthesized by the HIR → WIR lowering, so `off`, `compat`, and
  `aggressive` all preserve them (#112).
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
- `for`-loop binder resolution (#114): the loop variable must resolve to a
  global variable — either a declared `globalvar`, or an OverPy **default
  variable name** (`A`–`Z`, `AA`–`AZ`, …, `DA`–`DX`), which the pinned
  reference accepts as an implicit global at its fixed Workshop slot (e.g.
  `for I in range(0, 10):` with no declaration, the agent-lab regression).
  Nested same-name loops reuse the same implicit variable (no separate
  binding), matching the reference. An undeclared lowercase binder is
  rejected exactly like the reference rejects it (`unknown-identifier`,
  reference: "Unknown function name"). `range(stop)` / `range(start, stop)` /
  `range(start, stop, step)` are all supported.

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
- Builtin action/value/member identity, signatures, receiver categories,
  parameter enum domains, and non-contextual aliases resolve through the OPY
  semantic compatibility manifest
  (`crates/wright-opy/src/manifest/data/manifest.json`, schema v1; spec in
  [`compat-manifest-spec.md`](compat-manifest-spec.md), issue #109) — the
  single authoritative semantic table, replacing the former `KNOWN_ENUMS`
  hardcoded subset. Every manifest entry is probe-validated against the
  pinned OverPy 9.7.10 oracle (`crates/wright-opy/src/manifest/probes/`).
  Unknown or misplaced builtins fail at semantic resolution with structured,
  source-located diagnostics (`unknown-action`, `unknown-value`,
  `unknown-member`, `invalid-arity`, `invalid-receiver`,
  `enum-domain-mismatch`, `action-in-value-position`,
  `value-in-action-position`, `invalid-call-context`, `invalid-iterable`,
  plus the argument-binding codes `unknown-keyword`, `duplicate-argument`,
  `missing-argument`, `positional-after-keyword`, `keyword-required`,
  `keyword-unsupported`, `invalid-argument` for #110),
  never as emitter catalog misses.
- Reference-validated evidence surface: `chaseOverTime(...)` (action;
  3–4 arguments, reevaluation defaults to `DESTINATION_AND_DURATION`),
  `isGameInProgress()` (value), `getPlayersInRadius(...)` (value; team
  `Team.ALL` and `LosCheck.OFF` defaults fill), `worldVector(...)` (value,
  `Transform` argument), and the enum-gated members
  `eventPlayer.setInvisibility(Invis.X)`,
  `eventPlayer.setStatusEffect(..., Status.X, ...)`, `eventPlayer.getThrottle()`.
- Receiver/member calls (`eventPlayer.setMoveSpeed(100)`,
  `eventPlayer.teleport(eventPlayer.getPosition())`,
  `target.setMoveSpeed(50)` on a player-valued global) lower to
  `ReceiverCall` and resolve at emission through the canonical `workshop-rs`
  catalog; the
  corpus-evidenced receiver methods are the `synthetic/receiver-calls`
  fixture methods plus the #106 enum-gated members (en-US spellings per
  [`docs/workshop/support-matrix.md`](../workshop/support-matrix.md),
  oracle-transcribed with provenance in the catalog).
- Non-contextual source aliases resolve to their canonical names
  (`stopChasingVariable` → `stopChasing`; member aliases `getCurrentHero` →
  `getHero`, `hasStatusEffect` → `hasStatus`); their emission spellings are
  not yet catalog-covered (documented emission gap). The `ChaseReeval`
  contextual alias resolves only through the `chase` keyword call context
  (#110) and stays out of the alias table.
- Builtin Workshop enums from the manifest's reference-validated domains:
  `Beam.{GOOD,GRAPPLE}`, `Color.{YELLOW,WHITE,RED,ORANGE,GREEN,BLUE,BLACK,
  PURPLE,AQUA,VIOLET,ROSE}`, `DynamicEffect.{BAD_EXPLOSION,GOOD_EXPLOSION,
  RING_EXPLOSION,GOOD_PICKUP_EFFECT,BAD_PICKUP_EFFECT,BUFF_IMPACT_SOUND,
  DEBUFF_IMPACT_SOUND}`, `EffectReeval.{VISIBILITY,COLOR,VISIBILITY_AND_COLOR}`,
  `Wait.IGNORE_CONDITION`,
  `ChaseTimeReeval.{NONE,DESTINATION_AND_DURATION}` (reference-validated
  against the pinned OverPy 9.7.10 enum block and emission, #105),
  `ChaseRateReeval.{NONE,DESTINATION_AND_RATE}` (`NONE` additionally
  corpus-evidenced by the real-world overpy-meipocalypse `ChaseReeval.NONE`
  rate-chase calls, which the reference resolves to the `ChaseRateReeval`
  domain), plus the evidence domains `Invis.{ALL,ENEMIES,NONE}`,
  `Transform.{ROTATION,ROTATION_AND_TRANSLATION}`,
  `Status.{ASLEEP,BURNING,FROZEN,HACKED,INVINCIBLE,KNOCKED_DOWN,PHASED_OUT,
  ROOTED,STUNNED,UNKILLABLE}`, `LosCheck.{OFF,SURFACES,
  SURFACES_AND_ALL_BARRIERS,SURFACES_AND_ENEMY_BARRIERS}`,
  `Team.ALL`. Members outside the declared domains (including spellings the
  pinned reference rejects, such as `Color.CYAN` or `DynamicEffect.SPARKLES`)
  fail explicitly (`unknown-enum-member`). Enum domains/members beyond the
  declared baseline remain `baseline-planned`; emission coverage stays
  corpus-scoped (a manifest-valid member can still hit a catalog miss at
  emission when no spelling is catalogged).
- `wait()` / `wait(time)` default-argument filling: the reference appends
  `Wait.IGNORE_CONDITION` (and `0.016` for the no-argument form); native
  matches.
- **Named/keyword arguments** (`name = expr` call arguments, #110) bind
  against the manifest's canonical parameter names — the pinned reference's
  declared names (`wait(time=1)`, `wait(waitBehavior=Wait.IGNORE_CONDITION,
  time=2)`, `chaseOverTime(g, 10, duration=3)`,
  `chaseOverTime(g, 10, 3, reevaluation=ChaseTimeReeval.NONE)`,
  `vect(x=1, y=2, z=3)`, `getPlayersInRadius(center=…, radius=…,
  team=Team.ALL)`, `eventPlayer.setStatusEffect(assister=…, status=…,
  duration=…)`, `print(text="x")`, `len(array=…)`, `debug(value=…)`,
  `stopChasing(variable=g)`, member forms like
  `eventPlayer.setMaxHealth(healthPercent=100)`). Keyword arguments may
  appear in any order before the first positional argument; the reference
  rejects positional arguments after keyword arguments
  (`positional-after-keyword`), unknown keyword names (`unknown-keyword`),
  duplicate bindings (`duplicate-argument`), and missing required arguments
  (`missing-argument`) — all structured, source-located diagnostics. The
  reference's generic binder is routed around for `range`, `random.*`, and
  `.format` (keyword arguments on those fail with `keyword-unsupported`),
  and for `macro` invocations.
- **The `chase` keyword form** (#110, reference special form):
  `chase(variable, destination, rate=…, ChaseReeval.MEMBER)` and
  `chase(variable, destination, duration=…, ChaseReeval.MEMBER)` — exactly
  four arguments, the 3rd passed as the `rate`/`duration` keyword and the
  4th as a bare `ChaseReeval.MEMBER` access. `ChaseReeval` resolves **only**
  in this call context: `rate=` selects the `ChaseRateReeval` domain and
  lowers the call to `chaseAtRate`; `duration=` selects `ChaseTimeReeval`
  and lowers to `chaseOverTime`. Members are checked against the selected
  domain (`chase(g, 10, rate=2, ChaseReeval.DESTINATION_AND_DURATION)` is
  rejected with `enum-domain-mismatch`, matching the reference's "Unknown
  chaseratereeval"). Outside the chase signature `ChaseReeval` never
  resolves (a bare `g = ChaseReeval.NONE` is rejected like the reference).
  The first argument must be a variable (`invalid-argument` otherwise);
  emission dispatches on its kind: a global variable emits
  `Chase Global Variable At Rate/Over Time`, a player variable emits
  `Chase Player Variable At Rate/Over Time(player, name, …)` (catalog
  spellings `chaseAtRate`, `chasePlayerVariableAtRate`,
  `chasePlayerVariableOverTime`, oracle-transcribed, #110). The
  parenthesized member form `chase(…, (ChaseReeval.NONE))` is accepted by
  the native frontend though the reference's raw-token check rejects it
  (documented presentation-level difference; the parenthesized form is not
  pinned by probes).
- `chaseOverTime(...)` requires a variable first argument like the
  reference (`invalid-argument` for `chaseOverTime(10, …)`), which also
  selects the global/player emission form.
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

- A full decompiler architecture (comment/formatting-preserving
  reconstruction of arbitrary Workshop text); the declared reconstruction
  surface above is semantic reconstruction, not original-source recovery.
- Macro/`#!define` values that require runtime evaluation (no scripting).
- OverPy enum domains/members beyond the manifest's declared baseline (a
  data change, `baseline-planned` in the compatibility baseline).
- Emission spellings for manifest-valid entries not yet catalog-covered
  (alias targets `stopChasing`/`getHero`/`hasStatus`, and enum members
  without a catalogged spelling); these fail at emission with catalog
  diagnostics, never silently.
- Rule `disabled` markers (no corpus evidence for the source annotation).
- Expression-level `in`/`not in` membership operators — rejected at parsing
  (`for ... in` headers are supported).
- Backslash line continuation (`\` at end of line inside string
  concatenations / macro bodies) — rejected at lexing.
- Postfix increment/decrement (`++`/`--`) — rejected at parsing.
- Dict literals (`{...}`) — rejected at lexing.
- Triple-quoted strings / docstrings (`"""`) — rejected at lexing.
- Subroutine parameters, default `@Team`/`@Slot` overrides, `raycast`
  `include=`/`exclude=` named-argument forms (no reference/corpus evidence
  in the declared surface; the reference's `raycast` special form is not
  manifest-declared), and macro keyword arguments (the reference's macro
  substitution treats them as raw text; rejected explicitly).
- Full OverPy formatting semantics: `debug()`/`print()` emission
  (`Create HUD Text` etc.) follows the simplified semantic formatting documented
  in [`v1-matrix.md`](../v1-matrix.md).
- Emission presentation: variable references emit as `Global.<name>` (the
  native Workshop parser's canonical spelling) where the reference emits the
  bare variable name; observable semantics and round-trip validity are
  unchanged.

## Reconstruction surface (issue #124)

`wright_opy::reconstruct` consumes a validated Workshop IR program and emits
deterministic, byte-stable canonical OPY that the native frontend accepts and
that re-lowers to a structurally equivalent WIR program under
`workshop_rs::roundtrip::equivalent`. The machine-readable support
boundary (supported vs explicitly rejected constructs, with a consistency
test) lives in
`crates/wright-opy/tests/fixtures/reconstruct/boundary.json`; the round-trip
suite is `crates/wright-opy/tests/reconstruct.rs`, which runs
`Workshop → WIR → reconstructed OPY → native frontend → HIR → WIR` per fixture
and writes a per-fixture report to `target/wright-reconstruction-report.json`
with one reconstructed OPY per fixture under `target/wright-reconstruction/`.

### Reconstructed surface

- Variable and player-variable declarations with explicit Workshop indices
  (`globalvar name <index>`, `playervar name <index>`), plus declaration
  initializers reconstructed from the leading `Initialize global
  variables`/`Initialize player variables` rules (`globalvar name = value`).
  Zero-valued initializers are spelled `0.0` because the frontend drops
  integer-`0` initializers (matching the reference adapter).
- Subroutine declarations and `def name():` subroutine bodies.
- `rule "name":` with `@Event global` / `@Event eachPlayer` and
  `@Condition` lines.
- Scalar, string, bool, `None`, array, vector, and enum values; global and
  `eventPlayer.member` variable access; `eventPlayer` itself.
- Binary and unary operator calls in their OPY source spellings
  (`(a + b)`, `(a == b)`, `(a and b)`, `(not a)`, `(-a)`), `format` values
  (`"text".format(...)`), manifest value calls (`isGameInProgress`,
  `getPlayersInRadius`, `worldVector`, …) and manifest member-value calls
  (`eventPlayer.getPosition()`, …).
- Set/Modify global and player variable actions (modify ops `Add`…`Raise To
  Power` as `x = x <op> v`, `Append To Array` as `x.append(v)`), subroutine
  calls, `if`/`elif`/`else`, `while`, `for x in range(start, stop, step)`,
  the manifest action calls (`wait` with full arity, `disableInspector`,
  `playEffect`, `chaseOverTime`, …) and manifest member actions
  (`eventPlayer.setMoveSpeed(100)`, …), and the dedicated `debug(x)` /
  `print(x)` nodes.

### Explicitly rejected constructs

Every WIR construct the OPY frontend cannot recompile identically fails with
a structured diagnostic naming the construct (never partial or misleading
OPY): the per-player loop form (`For Player Variable`), disabled rules,
variable targets on arbitrary (non-`eventPlayer`) player expressions, names
that are not valid OPY identifiers or collide with OPY keywords/literals,
negative and non-finite number literals (the lexer has no negative-literal
token), enums outside the manifest's declared domains, `Remove From Array`
modifies, calls the frontend lowers to dedicated nodes (`debug`, `print`,
`append`, `vect`, `range`, `chase`), Workshop-spelled call names with no
manifest source form (`add`, `countOf`, `createBeamEffect`, …), calls whose
arity/domains the frontend would reject or default-fill, `Set` actions whose
value is a binary over the same variable (they re-lower to `Modify`), and
rule layouts the deterministic re-lowering cannot reproduce (non-leading or
mixed initializer rules, subroutine-body rules after normal rules or out of
table order, unsorted global slots, non-canonical subroutine indices,
initializer-bearing globals whose slot differs from the lowest free slot).

Reconstructed OPY is simple low-level valid OPY: comments, macros, functions,
settings blocks, and source abstractions are not recovered.

### Shared conversion path (#126)

The reconstructor is exposed end-to-end through one shared driver/session
conversion operation: `wright convert --target opy <workshop-input>` (CLI) and
`CompilerSession::convert(ConvertTarget::Opy)` (library) load validated
Workshop input through the driver's own `load()` path and call
`wright_opy::reconstruct::reconstruct` unchanged. The reconstructed source is
the `result.text` of the `wright-result/v1` envelope; a construct outside the
declared surface fails with the reconstructor's stable diagnostics (stage
`reconstruction`, exit code 3) and no partial source. The operation is
Workshop → OPY only: non-Workshop inputs are rejected explicitly, and there is
no direct OPY ↔ OSTW path. The cross-format suite
(`crates/wright-driver/tests/convert.rs`) proves the full loop
`Workshop → convert(opy) → native frontend → HIR → WIR → Workshop` for the
fixtures above and writes `target/wright-convert-report.json`.

## Boundary contract

The native frontend produces `wright_core::hir::Program` (Opy HIR v1) with the
same protocol envelope, file registry, declarations, and rules as the
reference adapter — verified by the differential suite at the HIR boundary
(spans and the producer identity normalized away). It never requires Node or
OverPy; the adapter remains available as an explicit `WRIGHT_ADAPTER_PATH`
fallback and as the pinned compatibility oracle.
