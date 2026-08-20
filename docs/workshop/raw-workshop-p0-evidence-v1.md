# Raw Workshop P0 evidence (2026-08-21)

The five artifacts are pinned by the owning `workshop-rs` manifest:
[`raw-workshop-p0-v1.json`](https://github.com/wrightkit/workshop-rs/blob/main/docs/evidence/raw-workshop-p0-v1.json).
The full artifacts remain external because the manifest records unresolved or
non-asserted redistribution rights.

All five standalone `workshop-rs-cli parse --locale ...` runs completed with
exit 0 on the recorded artifact hashes. Wright was run from PR #190's exact
head after switching the dependency to published `workshop-rs 0.1.2` and
refreshing `Cargo.lock`.

| artifact | locale | check | lint | semantic-incomplete diagnostics | lint findings | rule path |
| --- | --- | --- | --- | ---: | ---: | --- |
| ai-pve-zh-CN | zh-CN | expected blocked diagnostic | expected blocked diagnostic | 2651 | 2 | 5 rules |
| bastion-en-US | en-US | expected blocked diagnostic | expected blocked diagnostic | 1562 | 18 | 5 rules |
| defend-the-castle-en-US | en-US | expected blocked diagnostic | expected blocked diagnostic | 2017 | 73 | 5 rules |
| illari-zh-CN | zh-CN | expected blocked diagnostic | expected blocked diagnostic | 398 | 4 | 5 rules |
| overwatch-rework-en-US | en-US | expected blocked diagnostic | expected blocked diagnostic | 153 | 4 | 5 rules |

The `workshop-semantic-incomplete` diagnostics are intentional: they identify
raw settings, unknown catalog calls, and `rawWorkshopAction` preservation with
source spans, and make the envelope non-OK so findings cannot be presented as
definitive. Lint still executes its five registered rules, which is recorded
separately from the blocked semantic-confidence result.

Finding review classification for this rerun is `uncertain` for every finding
until the corresponding source construct is semantically understood; no
finding is accepted as a high-confidence default result merely because the
parser or structural WIR validation succeeded. The observed rule families were
`duplicate-condition`, `repeated-value`, `while-without-wait`, and
`min-wait-loop`.
