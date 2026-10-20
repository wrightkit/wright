# Lessons

Short review queue of reusable failure classes from merged Wright work. Each entry names a prevention action and the evidence behind it. Promote an entry into a regression test, an owner contract, or an existing engineering standard, then remove it here. The procedure is the `merged-pr-retro` skill in `wrightkit/.agents`.

### A changed contract takes its old claims with it
- **Rule:** When a PR changes behavior a contract describes, search comments, test names, and ADR/docs text for the old claim and fix them in the same PR. A test name must describe the property its assertion pins.
- **Why:** Stale prose reads as authoritative to humans and agents, and a test named for the abandoned behavior hides the contract it now protects. Release tooling and CI path filters drifted the same way — a default version and declared file coverage nobody checked against the shipped code.
- **Prevention home:** existing policy (`engineering-quality.md` "Prefer code over commentary", `pr-review.md`), via the reviewer entry point tracked in `wrightkit/.github#83`.
- **Incidents:** [wright#573](https://github.com/wrightkit/wright/pull/573), [wright#619](https://github.com/wrightkit/wright/pull/619), [wright#654](https://github.com/wrightkit/wright/pull/654), [wright#741](https://github.com/wrightkit/wright/pull/741) (fixes [wright#736](https://github.com/wrightkit/wright/issues/736): docs claimed a `reconstruct` capability the provider did not advertise), observed 2026-10-08, 2026-10-10, 2026-10-13, 2026-10-18

### Sibling command surfaces must share the input contract
- **Rule:** When two surfaces accept the same inputs — discovery and fingerprint, `inspect` beside `check`/`lint`/`analyze`, loaded and re-read views — they must consume the shared contract path rather than re-derive acceptance. Add an agreement check across the siblings, not a second membership list.
- **Why:** Divergent subsets shipped: `inspect` refused provider-backed OPY input every sibling accepted, and discovery and the disk fingerprint disagreed on hidden files, `.txt`, and BOM handling, so the same project loaded differently per entry point.
- **Prevention home:** shared decode/membership/load paths plus sibling-agreement tests (landed in the fixing PRs). Escalation: `wrightkit/workshop-rs#451` for the cross-repository class.
- **Incidents:** [wright#647](https://github.com/wrightkit/wright/pull/647), [wright#659](https://github.com/wrightkit/wright/pull/659), [wright#819](https://github.com/wrightkit/wright/pull/819) (fixes [wright#788](https://github.com/wrightkit/wright/issues/788): `analyze` and `inspect cost` derived rule and finding counts from different paths), observed 2026-10-13, 2026-10-19

### Every consumer of rule content handles `disabled`
- **Rule:** Code that walks rules, conditions, or actions must say whether disabled elements count and have a test with a disabled rule, condition, and action; a disabled block opener's `End` belongs to it.
- **Why:** Disabled code never runs, so counting it makes graders pass vacuously and analyses report phantom loops or cost.
- **Prevention home:** test, owned by the consuming component (analyzer and benchmark grader tests).
- **Incidents:** [wright#625](https://github.com/wrightkit/wright/pull/625), [wright#644](https://github.com/wrightkit/wright/pull/644), observed 2026-10-11, 2026-10-13. #644 fixed older code reported through an Issue rather than a review finding, so an owner should confirm it counts as the second independent incident.

### The MSRV gate catches syntax the local toolchain accepts
- **Rule:** Code must compile on the declared `rust-version` (1.85.0), so the workspace gates run on that toolchain — `cargo +1.85.0 clippy --workspace --all-targets --all-features -- -D warnings` — before pushing. Syntax stabilized after the MSRV (for example let-chains in 1.88) compiles under a newer local toolchain and fails only in the `Rust (1.85.0)` CI job.
- **Why:** Two PRs written under a newer local toolchain shipped let-chains; each needed a pushed fix after the MSRV job failed. `workshop-rs` already carried the same rule in its `AGENTS.md` since 2026-10-15, but the rule had not propagated to sibling repositories.
- **Prevention home:** the `cargo +1.85.0` pre-push gate now stated in `AGENTS.md` (ported from workshop-rs#457); the `Rust (1.85.0)` CI job remains the backstop.
- **Incidents:** [wright#743](https://github.com/wrightkit/wright/pull/743), [wright#763](https://github.com/wrightkit/wright/pull/763) (a let-chain in the refactor batch needed an unwrap commit after the MSRV job), observed 2026-10-18; same class in [opy-rs#574](https://github.com/wrightkit/opy-rs/pull/574) the same day, recorded in opy-rs's `docs/LESSONS.md` and the escalation issue `wrightkit/.github#101`.

### Usage validation that needs no loaded state runs before project load
- **Rule:** A new flag's or argument's usage-error check must run where its inputs exist. When the verdict needs only session-construction fields (catalog, declared locales), gate before `load_with_operation`, not after.
- **Why:** `compile --to <bad-locale>` spawned the provider and loaded the project before reporting `invalid-locale`; a review follow-up moved the gate first.
- **Prevention home:** CLI review judgment plus the landed ordering test — a bad `--to` on an unloadable source reports `invalid-locale` (landed in #768).
- **Incidents:** [wright#760](https://github.com/wrightkit/wright/pull/760), fixed by [wright#768](https://github.com/wrightkit/wright/pull/768) — observed 2026-10-18

### A test asserts only on state the test or an owner guarantees
- **Rule:** A fixture or test must pin its assumption to a guarantee: a rejection the owning repository's own tests assert stays rejected, or a capability the test stages itself — never ambient environment state that can legitimately change.
- **Why:** The zh-CN rejected-value fixture keyed on a spelling workshop-rs#479 then legitimately accepted as the secondary-fire switch's off spelling, and would have broken on the next dependency bump. Two `convert --target opy` tests asserted the ambient provider lacked `reconstruct` and failed on main the moment opy-provider 0.3.19 shipped it.
- **Prevention home:** the re-pinned rejection fixture keyed on an owner-guaranteed rejection (#766) and the provider-staged capability tests (#815).
- **Incidents:** [wright#757](https://github.com/wrightkit/wright/pull/757), fixed by [wright#766](https://github.com/wrightkit/wright/pull/766) — observed 2026-10-18; [wright#815](https://github.com/wrightkit/wright/pull/815) — observed 2026-10-19

### An out-of-contract input or a failed computation surfaces as an error, never a plausible value
- **Rule:** When input falls outside the declared contract or a computation cannot produce its result, reject or report `unavailable` — never clamp, coerce, or print a zero. A plausible-looking result is worse than an honest failure.
- **Why:** `lookup` silently clamped `limit: 11` to 10 so the caller could not tell its request was changed, and `inspect cost` printed `0 element(s) of the 0-element budget` when the count had failed. The same class as the wire-boundary escalation in #728, now on request and result surfaces.
- **Prevention home:** the `malformed-request` rejection and `unavailable` reporting landed in #811 and #816; org escalation [wright#728](https://github.com/wrightkit/wright/issues/728).
- **Incidents:** [wright#811](https://github.com/wrightkit/wright/pull/811) (fixes [wright#783](https://github.com/wrightkit/wright/issues/783)), [wright#816](https://github.com/wrightkit/wright/pull/816) (fixes [wright#790](https://github.com/wrightkit/wright/issues/790)) — observed 2026-10-19

### A multi-resource mutation stages every change before committing
- **Rule:** A transaction over several files or resources stages every replacement before touching the first target; a mid-sequence failure leaves the set unchanged or names exactly what was written.
- **Why:** `write_previews` replaced files one at a time under an atomic contract, so a failure on a later file left earlier files committed and the diagnostic silent about which — a half-edited project.
- **Prevention home:** stage-then-commit with rollback plus the mid-failure tests landed in #742.
- **Incidents:** [wright#742](https://github.com/wrightkit/wright/pull/742) (fixes [wright#734](https://github.com/wrightkit/wright/issues/734)) — observed 2026-10-18
