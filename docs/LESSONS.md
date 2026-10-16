# Lessons

Short review queue of reusable failure classes from merged Wright work. Each entry names a prevention action and the evidence behind it. Promote an entry into a regression test, an owner contract, or an existing engineering standard, then remove it here. The procedure is the `wrightkit-retro` skill in `wrightkit/.agents`.

### A changed contract takes its old claims with it
- **Rule:** When a PR changes behavior a contract describes, search comments, test names, and ADR/docs text for the old claim and fix them in the same PR. A test name must describe the property its assertion pins.
- **Why:** Stale prose reads as authoritative to humans and agents, and a test named for the abandoned behavior hides the contract it now protects.
- **Prevention home:** existing policy (`engineering-quality.md` "Prefer code over commentary", `pr-review.md`), via the reviewer entry point tracked in `wrightkit/.github#83`.
- **Incidents:** [wright#573](https://github.com/wrightkit/wright/pull/573), [wright#619](https://github.com/wrightkit/wright/pull/619), observed 2026-10-08, 2026-10-10

### Every consumer of rule content handles `disabled`
- **Rule:** Code that walks rules, conditions, or actions must say whether disabled elements count and have a test with a disabled rule, condition, and action; a disabled block opener's `End` belongs to it.
- **Why:** Disabled code never runs, so counting it makes graders pass vacuously and analyses report phantom loops or cost.
- **Prevention home:** test, owned by the consuming component (analyzer and benchmark grader tests).
- **Incidents:** [wright#625](https://github.com/wrightkit/wright/pull/625), [wright#644](https://github.com/wrightkit/wright/pull/644), observed 2026-10-11, 2026-10-13. #644 fixed older code reported through an Issue rather than a review finding, so an owner should confirm it counts as the second independent incident.
