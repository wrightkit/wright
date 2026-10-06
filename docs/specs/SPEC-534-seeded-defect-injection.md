---
kind: wright-spec/v1
id: SPEC-534-seeded-defect-injection
title: Scenario families built by seeded defect injection
status: proposed
related_issue: "#534"
owner: PM
freshness: live
---

This spec designs the generator that #534 requires: scenario families the
benchmark cannot pass without Wright, produced by injecting a known defect
into a clean project. It extends the [agent benchmark contract](../agent-benchmark.md);
generated instances are ordinary scenario directories and are graded and
calibrated by the existing machinery — no grader, materializer, or report
changes.

## Goal

Replace hand-written scenarios — whose fixed expected text is brittle and
whose tasks strong models solve from their own knowledge — with instances
generated from a seed project and a defect class. A scenario family is
accepted only when the discrimination report shows a baseline pass rate that
is neither 0% nor 100% and some condition differs.

## Terms

- **Seed project**: a project that is valid under every authority the family
  uses (`check`, `compile`, the upstream oracle for OPY). The metrics corpus
  projects are the first candidates; a purpose-built seed is vendored under
  `benchmarks/defects/seeds/` when no existing seed offers a defect class's
  sites.
- **Site**: one place a defect class can be injected, identified by content
  (file, symbol, the call locations it would leave dangling), never by byte
  offsets.
- **Instance**: one scenario directory generated from `(seed, defect class,
  site)`; `seed/` holds the defective project, `reference/` the overlay that
  restores the pristine files it replaced plus any required `answer.json`,
  `negative/<name>/` overlays that must fail exactly their declared checks.
- **Behavioral check**: the injected defect is gone, nothing else changed,
  and the project validates. It never compares against a fixed expected
  solution; probes are derived mechanically from the pristine seed's own
  compiled output under the authority that will grade them.

## The defect-injection interface

A defect class is a Python object in `benchmarks/defects/defects.py` with:

```python
class Defect:
    id: str        # "renamed-callable"
    family: str    # "diagnostics" | "cost" | "conversion"
    language: str  # the source language its sites live in

    def sites(self, files: dict[str, str]) -> list[Site]
    def apply(self, files: dict[str, str], site: Site, compiled: str) -> Applied
```

- `sites()` scans the pristine seed's files and returns every injectable
  site in a stable order.
- `apply()` returns `Applied`: the defective file tree; the `reference`
  overlay (the pristine versions of the files it changed); `negatives`
  mapping an overlay name to its files and the check ids it must fail; the
  scenario's `checks`; its `writable` list; and its `prompt.md` text.
- `compiled` is the pristine seed compiled by the authority the checks are
  evaluated against — the upstream oracle for OPY, Wright otherwise — so
  derived probes always match the text the grader will read. `apply` must
  fail, not emit a vacuous check, when a probe it needs cannot be extracted.

## How a seed maps to an instance

`benchmarks/defects/generate.py` reads a seed's files, calls `sites()`, and
picks one site with `random.Random(sha256(defect-id | seed-tree-hash |
canary))`. The pick is therefore deterministic for a given seed and defect —
regeneration reproduces the committed directory byte for byte, which
`generate.py --check` enforces — and different seeds hash to different picks
and different content, so two seeds never yield the same instance. The
committed public instances live in `benchmarks/agent/scenarios/` on the
`train` split and carry the suite canary; held-out instances for the private
suite are generated the same way with `private-NNN` ids. `scenario.json`
records the defect, site, and seed hash under `generated` for provenance.

## The checks a generated scenario gets

`apply` builds checks of three kinds, using the existing check kinds only:

- *validity* — `check`, `wright-compile`, and `oracle` for OPY;
- *defect gone* — a probe only a fix can satisfy (a lint code absent, a
  `compiled-contains` that has no text until the project compiles, an
  `answer` derived from the injection record);
- *nothing else changed* — `compiled-contains` probes derived from the
  pristine seed's compiled output: every rule name, and the injected
  site's own preserved semantics (the callable still invoked, its body
  still present). Edits outside `writable` are already unsafe; because the
  probes name constructs rather than a whole output, any correct fix
  passes — not only the reference one.

`validate` calibrates each instance like a hand-written scenario: the
reference passes, the defective seed fails, each negative fails exactly the
checks it was built to break.

## Families and defect classes

In the issue's order. Classes come from Wright's lint rules, the defect
classes found upstream (`workshop-rs#341`, `workshop-rs#360`), and the
`workshop-rs`/`opy-rs` error classes.

1. **diagnostics** — fix a batch of diagnostics across a multi-file
   project. Classes: `renamed-callable` (implemented end to end: a `def`
   renamed, every call left dangling as `unknown-action`; OPY since a
   Workshop project is one file), and candidates like undeclared-variable
   and wrong-argument-count (`workshop-rs#341`) once the checker reports
   them, settings misspellings (`workshop-rs#360`).
2. **cost** — a project exceeding the element limit or risking server
   load. Classes: `removed-wait` → `while-without-wait` / `min-wait-loop`,
   `hot-path-condition` → `ongoing-condition-hot-path` /
   `expensive-loop-check`, `repeated-value`, and rule/action bloat toward
   the element limit reported by `analyze`.
3. **conversion** — convert an OverPy project to Workshop, verify it is
   valid and equivalent. Validity is `check`; equivalence is judged by the
   upstream compiler as the independent authority, comparing the produced
   `.ws` semantics to the OPY's compiled output with `compiled-contains`
   probes, never a byte compare.

## Proof class: `renamed-callable`

`sites()` finds every `def name()` with at least one call; `apply()` renames
the declaration only, leaving calls referencing a name no `def` defines —
`unknown-action` at each call site across the project's files, so the task
is a diagnostics batch the agent iterates `check` → fix → `check` through.
Both correct fixes pass the checks: restoring the declaration, or retargeting
every call to the new name — the call probe is name-prefixed
(`Call Subroutine\(<name>\w*\)`) exactly so both are accepted, and the
`deleted-def` negative pins that removing the calls or the definition fails
`callable-still-called`, `body-preserved`, `rules-preserved`, and
`missing-name`.

Committed proof instances: `renamed-callable-payload-race` (vendored
multi-file seed, the callable invoked from two files) and
`renamed-callable-understand-opy` (the metrics-corpus scenario seed). The
generator, the seed, and the two instances are validated in CI by the
existing scenario-calibration step and by `benchmarks/defects/test_defects.py`.

## Limits of the current interface

- `_subroutine_body` probes assume flat action blocks; a site whose body
  nests braces is extracted to its first `}` and still yields a correct —
  only weaker — probe. A class needing nested bodies should extract whole
  nested blocks.
- Site matching is per-class; `renamed-callable` matches `def name(` and
  bare `name(` call lines. Multi-line calls or argumented `def`s are not
  sites today; add them when a family needs them.
- The element-limit family needs `analyze` element counts in the seed
  range where a plausible bloat injection crosses the limit; seed choice
  there is open until the family is implemented.
