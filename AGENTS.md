# AGENTS.md

## Project

Wright is a Rust-based compiler and tooling project for the Overwatch Workshop / OverPy ecosystem.

This file is the repository-wide operating guide for coding agents and other automated contributors. It is a routing entrypoint, not a replacement for architecture documents, issue requirements, compatibility fixtures, or tests.

The repository is being developed incrementally. Do not infer a completed compiler, CLI, package layout, or release process from the project name or roadmap language. Treat the files and commands that actually exist in the current checkout as authoritative.

## Repository boundary

* Work within this repository unless the user explicitly asks for a cross-repository change.
* Inspect the current branch, worktree, and working-tree status before editing. Preserve existing or concurrent user changes.
* Do not assume that an external OverPy implementation, fixture, or documentation is part of Wright's implementation boundary.
* Keep public interfaces owned by Wright. Do not expose an external implementation's internal representation merely because it is convenient.

## Rule organization

* This root `AGENTS.md` contains rules that apply across the repository.
* Add a nested `AGENTS.md` only for rules that are genuinely specific to a directory or component; the nearest applicable guide takes precedence for that scope.
* Put durable architecture decisions and rationale in the repository's architecture documents or ADRs when those documents exist. Link them from this file or a focused index instead of duplicating their full contents here.
* Treat Cargo manifests as the source for package and workspace boundaries, and treat repository-defined scripts as the source for build and test commands.
* Treat tests and compatibility fixtures as executable evidence. Do not create or cite a path, command, or rule that is not present in the current checkout.

When a task needs an authority that does not yet exist, do not invent a rule to fill the gap. Record the missing decision, or add the smallest focused document required by the task.

## Current rule index

Before starting work, read the applicable material in this order:

1. The user's request and any linked GitHub issue, including its parent milestone or issue when supplied.
2. This file for repository-wide boundaries and workflow.
3. Relevant architecture documents, ADRs, design notes, and package manifests that exist in the checkout.
4. The source, tests, fixtures, and generated-output rules for the affected component.

Use the following routing triggers as the repository grows:

* For a new feature or a change to compiler behavior, locate the owning crate/module and read its tests and compatibility fixtures before editing.
* For parser, semantic-analysis, lowering, or code-generation work, trace the producer-to-consumer path and identify the contract at each boundary before changing a representation.
* For interoperability with OverPy or Workshop output, establish the compatibility level, provenance, and unsupported cases before implementing a translation or workaround.
* For diagnostics or tooling APIs, inspect the structured data consumed by clients; do not make consumers scrape human-readable text when a typed interface is appropriate.
* For performance work, collect a relevant benchmark, profile, corpus measurement, or real workload before accepting a trade-off.
* For generated files, fixtures, release artifacts, or CI configuration, inspect the repository's existing generation and validation path before modifying outputs directly.

If a later component guide conflicts with this file, resolve the conflict explicitly at the appropriate architecture or project level; do not silently preserve contradictory rules.

## Source of truth and planning

Use this priority when determining intended behavior:

1. Explicit user or task instructions.
2. Accepted repository architecture documents and ADRs.
3. The active GitHub issue and its acceptance criteria.
4. Compatibility fixtures and reference behavior.
5. Existing implementation.
6. Assumptions.

If sources conflict, identify the conflict and resolve it at the appropriate project level. Do not treat this guide as evidence that an adjacent roadmap item is in scope.

Early milestones may be decomposed into concrete implementation issues. Keep later roadmap work high-level until implementation evidence supports a more precise design. If evidence invalidates the current plan, report it and update the plan rather than distorting the implementation to preserve an obsolete assumption.

## Architecture and compatibility

Respect established component boundaries and keep transformations explicit:

* Preserve semantic identity where the contract requires it.
* Preserve source provenance where practical and validate both inputs and outputs.
* Make unsupported syntax, semantics, and compatibility levels explicit and diagnostic.
* Prefer a small, clear representation over an opaque catch-all node or a silent fallback.
* Do not mechanically copy or translate third-party implementations. Imported examples, fixtures, and test cases need clear provenance and appropriate redistribution rights.
* When a compatibility bug appears, reduce it to a stable regression fixture before adding a special case whenever possible.

Compatibility is measurable. Identical text is not proof of identical semantics, and different text is not proof of a regression. State which compatibility level a test or claim covers.

## Rust and implementation practices

Prefer idiomatic, explicit Rust and strong types where they improve semantic safety. Favor typed identifiers, exhaustive matching, deterministic observable behavior, clear ownership boundaries, and simple data structures before complex abstractions.

Avoid unnecessary `unsafe`, highly generic trait hierarchies, dynamic dispatch, lifetime complexity, and premature micro-optimization. Use `unsafe` only for a concrete systems or performance requirement, with documented invariants and tests.

Prioritize correctness, semantic clarity, architectural consistency, inspectability, testability, determinism, maintainability, and then performance. Add an abstraction only when a concrete requirement, repeated use case, or accepted architecture justifies it.

## Validation

Test changes at the lowest layer that can prove the relevant behavior. Include successful and failing cases where the change affects validation, diagnostics, malformed input, invalid references, unsupported constructs, determinism, or compatibility.

Run repository-defined checks when they exist. For a standard Rust workspace, the baseline is:

```sh
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

Do not claim these checks passed when the repository does not yet contain the required Cargo workspace or when an environmental failure prevents them from running. Record the exact limitation and the smallest useful next check.

Before handoff, review the complete diff, run `git diff --check`, and compare the result against every applicable acceptance criterion. Distinguish source-level, local-test, CI, deployment, and production evidence; a successful build or health check is not business-path proof.

## General workflow

1. Confirm the user's goal, repository boundary, branch, worktree, and working-tree status.
2. Read the linked issue and parent context when supplied, then identify acceptance criteria, non-goals, and the owning architectural boundary.
3. Read the relevant documents, source, tests, fixtures, and generated-output instructions before deciding on an implementation.
4. Make the smallest change that satisfies the current contract. Protect unrelated changes and do not use destructive Git commands.
5. Add or update regression coverage and normative documentation when the behavior or a durable contract changes.
6. Run proportionate validation, inspect the final diff, and report verified behavior, unverified areas, risks, and deferred work honestly.

## Git and delivery

* Reuse the existing branch or worktree when one is provided. Do not create nested worktrees or move user changes without a clear need.
* For implementation tasks, commit verified task-owned changes by default unless the user asks to defer or avoid committing.
* Stage only files or hunks belonging to the task. Before committing, inspect the staged diff and run `git diff --check`.
* Use a concise, descriptive commit message. If a supplied issue is actually resolved, include an appropriate closing keyword; use a non-closing reference for partial or related work.
* Do not push, amend, rewrite history, create a pull request, publish artifacts, or modify remote issues unless the user explicitly requests it.
* Do not claim that a remote issue is closed until the relevant commit is pushed to the applicable default branch or merged through a pull request.

## Safety and licensing

* Never place credentials, tokens, private keys, authorization files, or sensitive runtime data in the repository, documentation, logs, fixtures, or commits.
* Do not delete data, overwrite user changes, or run broad cleanup commands without explicit authorization. Prefer recoverable operations for approved deletions.
* Keep third-party code and content within their documented license boundaries. Preserve required notices and attribution.
* Do not infer legal permission from technical separation. If the licensing or redistribution status of a dependency, fixture, or copied example is unresolved, stop and surface the question rather than guessing.
* A license file describes the terms for Wright's own repository content; it does not change the terms of third-party dependencies or imported materials.

## Scope control

Do not expand an active task into adjacent roadmap work, speculative refactors, compatibility layers, configuration, or release automation. When uncertain, implement the smallest stable contract that satisfies the stated requirements and leave future work explicit.
