# `athena` and `practice-range` test inputs

Recorded inputs for `simulate` (ADR-0033): the scheduler probes whose saved
client captures are the comparison oracle, the probes that exercise
`not_modeled`, the `practice-range` Workshop program, and the `athena`
scenarios that drive it.

## Provenance

| Path | Source | License |
| --- | --- | --- |
| `probes/*/probe.workshop`, `probes/*/capture*.txt`, `probes/*/capture.json` | `wrightkit/athena` `probes/` at tag `v0.2.0` (commit `f01b30df13bda5215ba6852936942918cffeb1c2`) | AGPL-3.0 |
| `scenarios/*.json` | `wrightkit/athena` `docs/scenario-examples/valid/` at the same tag | AGPL-3.0 |
| `practice-range.ow` | `wrightkit/practice-range` `src/main.opy` compiled by `opy-rs`, as `build/practice-range.ow` at the `athena` test-pinned commit `6e5ecc6904473de397585e3bf2ff5f815efc3394` | Apache-2.0 |

Each probe directory's `capture.json` records the probe metadata — client
version, hypothesis, `probe_sha256`, and the `match`/`mismatch` verdict the
capture run reached. The `probe_sha256` covers the probe's `probe.zh-CN.ow`
spelling; `athena`'s own fixture suite proves the en-US `probe.workshop`
vendored here simulates identically to it, so the hash pins the same
program.

## What each file is for

- `p0`–`p3` probes: `athena`-modeled scheduler behavior. `capture.txt`,
  `capture-2.txt`, `capture-3.txt` are verbatim Inspector exports saved from
  the client — the recorded oracle `simulate`'s traces are compared against
  with `athena`'s own tolerances (`0.0051` on log numbers, `0.0006` on
  variables; variables absent from an export must be zero-like).
- `p4`–`p6` probes: layer-3 constructs `athena` `v0.2.0` does not model —
  the `not_modeled` coverage (`action waitUntil`, a player world read, the
  `ABORT_WHEN_FALSE` wait behavior). No captures are vendored for them:
  the runs never complete.
- `practice-range.ow` + `scenarios/practice-range.json`: the real project's
  scenario run — one injected final blow takes `score` to `2` and `streak`
  to `3`, matching the scenario's `expect`.
- `scenarios/practice-range-missing-event-player.json`: the same program
  with `eventPlayer` omitted — the `missing_input` coverage.

Refresh rule: these files are a pinned record. Update them only by copying
the same paths from a newer `athena` tag (or a newer `practice-range`
build), and update the pin in this table in the same change.
