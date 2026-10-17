# Lessons

Short review queue of reusable failure classes from merged Wright work. Each entry names a prevention action and the evidence behind it. Promote an entry into a regression test, an owner contract, or an existing engineering standard, then remove it here. The procedure is the `wrightkit-retro` skill in `wrightkit/.agents`.

### A changed contract takes its old claims with it
- **Rule:** When a PR changes behavior a contract describes, search comments, test names, and ADR/docs text for the old claim and fix them in the same PR. A test name must describe the property its assertion pins.
- **Why:** Stale prose reads as authoritative to humans and agents, and a test named for the abandoned behavior hides the contract it now protects. Release tooling and CI path filters drifted the same way — a default version and declared file coverage nobody checked against the shipped code.
- **Prevention home:** existing policy (`engineering-quality.md` "Prefer code over commentary", `pr-review.md`), via the reviewer entry point tracked in `wrightkit/.github#83`.
- **Incidents:** [wright#573](https://github.com/wrightkit/wright/pull/573), [wright#619](https://github.com/wrightkit/wright/pull/619), [wright#654](https://github.com/wrightkit/wright/pull/654), observed 2026-10-08, 2026-10-10, 2026-10-13

### Sibling command surfaces must share the input contract
- **Rule:** When two surfaces accept the same inputs — discovery and fingerprint, `inspect` beside `check`/`lint`/`analyze`, loaded and re-read views — they must consume the shared contract path rather than re-derive acceptance. Add an agreement check across the siblings, not a second membership list.
- **Why:** Divergent subsets shipped: `inspect` refused provider-backed OPY input every sibling accepted, and discovery and the disk fingerprint disagreed on hidden files, `.txt`, and BOM handling, so the same project loaded differently per entry point.
- **Prevention home:** shared decode/membership/load paths plus sibling-agreement tests (landed in the fixing PRs). Escalation: `wrightkit/workshop-rs#451` for the cross-repository class.
- **Incidents:** [wright#647](https://github.com/wrightkit/wright/pull/647), [wright#659](https://github.com/wrightkit/wright/pull/659), observed 2026-10-13

### Wire boundaries must reject malformed input instead of absorbing it
- **Rule:** Protocol surfaces enforce the contract's bounds and rejection path: frame-size limits, UTF-8 validation before dispatch, and the spec's error code for malformed or out-of-lifecycle messages. A bad input must never end the session as success or produce no output.
- **Why:** `wright-lsp` exited on a JSON parse error and allocated bodies without bounds; `serve` treated a non-UTF-8 line as clean EOF and dropped an id-less `tools/call` silently. Malformed input was absorbed, not rejected.
- **Prevention home:** boundary robustness fixtures per transport (enforcement and tests landed in the fixing PRs); `wrightkit/.github` `docs/robustness-testing.md`. Escalation: #728.
- **Incidents:** [wright#648](https://github.com/wrightkit/wright/pull/648), [wright#653](https://github.com/wrightkit/wright/pull/653), observed 2026-10-13

### Every consumer of rule content handles `disabled`
- **Rule:** Code that walks rules, conditions, or actions must say whether disabled elements count and have a test with a disabled rule, condition, and action; a disabled block opener's `End` belongs to it.
- **Why:** Disabled code never runs, so counting it makes graders pass vacuously and analyses report phantom loops or cost.
- **Prevention home:** test, owned by the consuming component (analyzer and benchmark grader tests).
- **Incidents:** [wright#625](https://github.com/wrightkit/wright/pull/625), [wright#644](https://github.com/wrightkit/wright/pull/644), observed 2026-10-11, 2026-10-13. #644 fixed older code reported through an Issue rather than a review finding, so an owner should confirm it counts as the second independent incident.
