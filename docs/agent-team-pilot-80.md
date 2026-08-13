# Agent Team pilot report: #80

related_issue: "#80"
freshness: snapshot
as_of_commit: 2345536984d897c1f220a2680bf0d94dd2c702d3
status: blocked-verification

This is a point-in-time assessment of the #80 pilot. It is not a replacement
for the live GitHub issue graph or the #69 acceptance gate.

## Goal and scope exercised

The pilot exercised the role contracts in [`docs/agent-team.md`](agent-team.md)
against the M10 gate without adding a model, vendor, or orchestration binding to
Wright's repository contract. A non-authoritative controller launched exactly
four independent, one-level role sessions in parallel:

- PM: product scope, requirements, and dependency readiness;
- Architect: boundaries, contracts, and ADR needs;
- Engineer: implementation and implementation-test inspection only;
- QA: independent evidence review and verification.

All role sessions were read-only. The controller only routed work and collected
reports; it did not make a product, architecture, implementation, or QA
decision.

## Evidence snapshot

The snapshot was taken on `main` at commit
`2345536984d897c1f220a2680bf0d94dd2c702d3`, with a clean working tree.

The live issue graph observed by the roles was:

- #78 and #79: closed/completed;
- #73: open/reopened;
- #69: open/reopened, with #73 as its only outstanding dependency among
  #63--#68, #72--#75;
- #80: open and dependent on #73 and #78.

QA independently recorded:

- `cargo test -p wright-language`: 55 passed, 0 failed;
- `cargo test -p wright-lsp --test lsp`: 22 passed, 0 failed;
- GitHub Actions run `31698428421` for this commit: six jobs executed, none
  skipped, all passed.

These are implementation and CI evidence, not M10 acceptance. #69's own gate
rules prohibit executing or accepting the gate while #73 remains open/reopened.

## Acceptance assessment

| #80 criterion | Result | Evidence boundary |
| --- | --- | --- |
| Run #69 only after its real dependencies complete | blocked | #73 is open/reopened; #69 was not bypassed |
| Independent QA evidence, not Engineer self-certification | pass for this snapshot | QA independently ran the two targeted suites and inspected CI |
| Preserve #78 authority under parent/subagent execution | pass for the exercised routing | Four isolated role sessions retained separate authority scopes |
| No recursive or circular delegation | pass for the exercised routing | Exactly one controller level and one child level were used |
| Durable findings and decisions | pass | This report records the results in the repository |
| Concise capability/limitation/manual-step assessment | pass for this snapshot | See the sections below; final #80 closure remains gated on #69 |

The final pilot status is **not passed: blocked-verification**. The non-gated
role-routing demonstration is complete, but the required #69 acceptance run was
not executed because its real dependency #73 is not complete in the live issue
record.

## What worked

- Native one-level subagent execution launched all four roles in parallel with
  isolated context.
- Role prompts and read-only tool permissions kept the authority boundaries
  explicit. QA did not consume an Engineer completion claim as acceptance
  proof.
- The controller was sufficient for deterministic launch, routing, and result
  aggregation; no custom cross-session orchestrator was needed for this
  demonstration.
- Existing repository artifacts were enough to coordinate the roles without
  adding a second roadmap or changing the M10 acceptance criteria.

## Limitations and manual intervention

- Child sessions cannot communicate directly; authority-sensitive handoffs must
  pass through the controller or a user.
- The controller receives role summaries rather than continuously observable
  intermediate reasoning. A summary is not a durable decision or acceptance
  record.
- Read-only pilot sessions cannot close issues, write issue comments, create
  the final QA/spec artifacts, or reconcile contract documentation. Those are
  explicit write-authorized follow-up actions.
- The boundary between deterministic routing and decision-classification
  judgment is not fully specified in the current governance contract. This is
  a governance clarification for PM/Architect, not a reason for the controller
  to make the decision itself.
- The Architect reported possible documentation drift in the HIR span
  provenance and language-service rename contracts. Those findings require
  separate contract review and are not silently changed by this pilot.

## Required next steps

1. Independently audit and complete #73 through the owning role and the live
   issue record.
2. Re-run #69 as a fresh QA-owned M10 gate only after all real dependencies are
   complete, including current green CI and the required protocol/project
   evidence.
3. Run the remaining #80 role-protocol acceptance work around that gate, then
   record the final capability assessment and manual steps in the live issue or
   another authorized durable artifact.

Until those steps are complete, this report must not be used to close #69 or
#80.
