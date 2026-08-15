# OSTW pinned-reference probes (#118)

Small pinned-reference probes for the behaviors #118's semantic lowering
depends on. Every probe runs the pinned OSTW v3.4.0 language server
(`Deltinteger --langserver`, content `769ce7aab097178cfe905bf21f0326d8e0d12e6b`,
release SHA `1ae882…e3a88`) against a minimal project and records the
diagnostics and emitted Workshop text.

Each probe directory contains:

- `probe.json` — the probe spec (source layout, entry, intent, reference
  identity, provenance);
- `main.ostw` / other sources — the probe input;
- `result.entry-only.json` — accept/reject, diagnostics, element count,
  emitted-code SHA-256;
- `workshop.entry-only.txt` — the canonical emitted Workshop text (the
  source-located reference evidence for the exercised behavior).

The Wright-authored `crates/wright-ostw/src/signature.rs` table derives its
canonical argument order, defaults, and en-US spellings from these recorded
emissions. No OSTW game-derived data (`Elements.json`, `LobbySettings.json`,
`Maps.json`) or upstream compiler tables are copied; observable reference
output only.

| Probe | Question answered |
| --- | --- |
| `p1-entry-closure` | Entry-only vs all-open compilation membership; omitted-event default; rule-priority ordering. |
| `p2a-simple-missing` / `p2b-protectban-shape` / `p2c-ambient-resolution` | Missing-import accept/reject, diagnostic range, and whether an ambient source can satisfy an import. |
| `p3a-variables-auto-explicit` / `p3b-variables-duplicate-ids` / `p3c-variables-player-receiver` | Explicit-ID allocation (`i 127`), duplicate-ID diagnostics, player-variable receiver read/write/modify. |
| `p4-types-expressions` **target** | User-enum member values, null/union, cast, ternary (`If-Then-Else`), `&&`/`||` (`And`/`Or`), short-circuit behavior. |
| `p5-functions-control` **target** | Subroutine vs inlined functions, parameter defaults, C-style for (`For Global Variable`), foreach (allocated counter + `Value In Array`), switch jump table, value-function inlining. |
| `p6-catalog-signatures` **target** | Named/default argument binding against canonical signatures (reordered named args, omitted defaults, `Event Player` restriction). |
| `P5b-loops-switch.json` + `p5b-result.txt` | C-style `for`, `foreach` (allocated counter + `Value In Array`), `switch` jump table, value-function inlining. |
| `P6b-catalog-signatures-extra.json` + `p6b-result.txt` | Additional named-arg signatures exercised by the reachable graph: `CreateEffect`, `CreateProgressBarInWorldText`, `PlayEffect`, `DisableMovementCollisionWithEnvironment`, `EnableMovementCollisionWithEnvironment`, and the `Event Player` restricted-value diagnostic. |

## Differential targets

The **target** probes are the pinned-reference-accepted (entry-only) probes
designated `differential-target` in their manifests: `p4-types-expressions`
(147 elements), `p5-functions-control` (120 elements), and
`p6-catalog-signatures` (68 elements). They form the immutable, Wright-owned
forward-compilation comparison boundary for #119 (see
[`docs/ostw/compatibility-baseline.md`](../../docs/ostw/compatibility-baseline.md));
`run_oracle.py --probes` aggregates them under `differentialTargets` in
`results.json` and fails if any target is reference-rejected.
