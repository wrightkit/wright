---
name: wright
description: Use in a project of Overwatch Workshop rules or OverPy (.opy) source when you must judge what is wrong, risky, or expensive, or how a change propagates, and text search cannot answer it. Covers Workshop server freezes, infinite loops, rule cost, symbol and rule flow, safe renames, and OPY-to-Workshop compilation, through the `wright` CLI.
license: AGPL-3.0
---

# Use Wright in Workshop and OPY projects

Wright is the semantic tooling for these projects: it parses, validates, lints, analyzes, and compiles them, and its results outrank your reading of the source. Run `wright --help` for the current commands; this guide only helps you decide when and how to lean on it.

## Decide

- **Is it correct?** `check`. It is only the correctness gate: it passes code that will freeze a server, such as a `While(True)` with no `Wait`.
- **Is it risky or expensive?** `lint` for stable-rule findings, `analyze` for element cost, hotspots, and shared state. Run these alongside `check`, before and after a change.
- **What does this symbol or rule do, and what breaks if I change it?** `inspect` (symbols, refs, cfg, callgraph, cost) resolves by name and follows semantics where grep only matches text. It works on raw Workshop, not OPY (see the language notes).
- **Many questions about one project?** `wright serve` loads it once.
- **Need Workshop output, or OPY from raw Workshop?** `compile`, `convert`. `convert` reconstructs, it does not recover your source.

Prefer `--format json` when you will act on the result. Reach for `--brief` on `lint`, `analyze`, and `inspect` first: it returns counts, the highest-priority items, and an expansion hint at a fraction of the full result — drop the flag when you need everything. Take a baseline before editing and compare after, so you can separate your findings from existing ones.

## Judgment that is easy to miss

- Report each finding with its evidence class. Static indicators and heuristics are prompts for your judgment, not measured load. Only the game runtime confirms server behavior.
- A refusal or unsupported result is an answer. Do not route around it with textual edits or hand analysis that pretends to be Wright's. Say what stays unverified.
- Wright delegates language semantics to its engines. A wrong or missing result is a bug to report to the owner (`workshop-rs`, `opy-rs`), not something to patch in the user's project.

For language-specific behavior (OPY versus raw Workshop, edits, compile and convert limits), read [references/language-notes.md](references/language-notes.md) when the task touches it.
