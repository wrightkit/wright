# Agent Team pilot report: M10 acceptance gate (#80)

related_issue: "#80"
freshness: snapshot
as_of_commit: f84e43e2f40ea27fee0e103b5160accb4fa8b21e
status: accepted

This report records the completed #80 pilot execution: running #69's M10
independent acceptance gate through distinct PM / Architect / Engineer / QA
authority, with the top-level session acting only as a non-authoritative
router. The earlier blocked attempt is preserved as historical evidence in
[`agent-team-pilot-80.md`](agent-team-pilot-80.md) (`as_of` `2345536`,
`status: blocked-verification`).

## What was executed

Two executions of the same role contract (`docs/agent-team.md`) were observed:

1. **Grok Build attempt** (recorded in `agent-team-pilot-80.md`): four
   independent, one-level, read-only role sessions launched in parallel by a
   non-authoritative controller. Role prompts and tool permissions kept
   authority boundaries explicit. The gate could not run because its real
   dependency #73 was still open; the attempt was recorded durably as
   `blocked-verification`.
2. **This execution (OpenCode, native one-level subagents)**: PM, Architect,
   and QA role sessions ran in parallel and read-only against `main`
   (`f84e43e`, clean tree); the controller collected reports, performed only
   deterministic status checks (issue states, CI runs), and executed
   pre-authorized issue-state writes. QA ran fresh local suites and inspected
   GitHub CI independently. The PM role applied its acceptance scope to the QA
   and Architect evidence and decided ACCEPT; the full traceable writeup is in
   the #69 closing comment. Engineer did no implementation work (no blocker
   found). #69 and #27 closed; #80 closes with this report.

## Role/session boundaries that worked

- The four-way authority split prevented self-certification: QA's report never
  relied on Engineer/commit-message claims, Architect's verdict was
  independent of QA's, and PM decided only after both were in hand.
- Read-only role sessions made the write boundary explicit: all issue-state
  writes went through the controller with user pre-authorization.
- Resuming the PM session (rather than spawning a fresh PM) let the PM role
  own the full accept/reject decision with its earlier acceptance scope in
  context; the controller never summarized or filtered evidence.

## Manual intervention needed

- Write-authorized steps (issue comments, closes, doc commit) ran in the
  controller/user session, not in role sessions.
- Routing role output between sessions (PM <- QA + Architect evidence) is a
  manual controller step; child sessions cannot communicate directly.
- The earlier Grok attempt required a human-driven snapshot commit and issue
  state reconciliation after the dependency (#73) landed.

## Router authority

The primary session stayed non-authoritative: it made no product, architecture,
implementation, or verification decisions. Its actions were limited to
launching role sessions, deterministic status checks, and pre-authorized
writes. Each decision class was decided by its owning role.

## Were native subagents sufficient?

Yes, in both harnesses. Native one-level subagents provided isolated context
and parallel execution for all roles; no recursion or circular delegation was
needed. Child-session summaries were sufficient for routing because durable
decisions lived in the issue record and this report, not in session memory.
Grok-specific capabilities from #80 (custom agent definitions, per-role tool
permissions, one-level depth) were exercised in the first attempt; the OpenCode
run exercised the same contracts with different native primitives, supporting
the harness-agnostic claim of #77/#78.

## Overhead review

- No role, state, or document added material overhead. The spec/QA-plan
  schemas from #78 were not needed for a gate (no feature spec exists for
  #69), which matches the contract's intent: use specs only when coordination
  requires them.
- The blocked-attempt snapshot doc proved useful: it preserved the first
  attempt's evidence and required-next-steps without masquerading as live
  status.
- Total added latency vs. a single agent was small (parallel sessions; one
  PM resume round).

## Custom orchestrator verdict

**Not justified.** Two full executions of the pilot completed with native
primitives plus a deterministic routing session. A custom cross-TUI
orchestrator would add infrastructure without improving correctness; the
open coordination points (write authorization, cross-session routing) are
small, explicit, and user-visible. Revisit only if a future milestone needs
real-time inter-session messaging or unattended multi-session orchestration.

## Outcome

- #69: ACCEPTED (traceable four-way classification in the issue comment).
- #27: closed as M10 complete.
- Post-M10 roadmap reassessment: recorded in the #69 acceptance comment from
  current evidence (documentation alignment for rename/span contracts, driver
  overlay API candidate, committed ≥20-file perf measurement, and the #75
  trigger-gated incremental/cancellation candidates). No new implementation
  issues were pre-created per #69's planning note.
- #77 closes after this report; #80 closes with its concise assessment.
