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
- **What does this symbol or rule do, and what breaks if I change it?** `inspect` (symbols, refs, cfg, callgraph, cost) resolves by name and follows semantics where grep only matches text. For OPY, these queries inspect the provider's compiled artifact (see the language notes).
- **Many questions about one project?** `wright serve` loads it once.
- **Need more detail?** Results show the verdict and actionable content by default. Long lists show at most 20 items, their total, and how to see the rest. Use `--all` for every item in a list and the `inspect` subcommands for specific detail.
- **Unsure of a name, or a name was rejected?** `wright lookup <query> --language workshop|opy` resolves the owner-accepted spelling and signature; `--within <enum or callable>` lists its members or parameters. Check before writing in a language you do not know, and again when `check` rejects a name.
- **Need Workshop output, or OPY from raw Workshop?** `compile` writes the program to `<input stem>.ow` (`-o PATH` chooses another file) and reports only the verdict, diagnostics, path, and digest; pass `--force` to replace an existing file, because you cannot answer its overwrite prompt. `compile --to <LOCALE>` (e.g. `en-US`, `zh-CN`) re-emits the program in another Workshop client locale, leaving the input untouched — that is the way to move a project between client locales. `convert` reconstructs, it does not recover your source.

Read text output by default and pass `--renderer plain`, so results stay deterministic and undecorated in non-terminal contexts. Use `--format json` when a program processes the result; it contains the same information as text, in structured form. Take a baseline before editing and compare after, so you can separate your findings from existing ones.

## Judgment that is easy to miss

- Report each finding with its evidence class. Static indicators and heuristics are prompts for your judgment, not measured load. Only the game runtime confirms server behavior.
- A refusal or unsupported result is an answer. Do not route around it with textual edits or hand analysis that pretends to be Wright's. Say what stays unverified.
- Wright delegates language semantics to its engines. A wrong or missing result is a bug to report to the owner (`workshop-rs`, `opy-rs`), not something to patch in the user's project.

For language-specific behavior (OPY versus raw Workshop, edits, compile and convert limits), read [references/language-notes.md](references/language-notes.md) when the task touches it.
