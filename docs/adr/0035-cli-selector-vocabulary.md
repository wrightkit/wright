# ADR-0035: One meaning per CLI selector spelling

- Status: Proposed
- Date: 2026-10-22
- Related: [Issue #888](https://github.com/wrightkit/wright/issues/888), [Issue #889](https://github.com/wrightkit/wright/issues/889), [ADR-0016](0016-current-directory-and-directory-project-targets.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0034](0034-snapshot-scoped-numeric-addresses.md), [Agent contract](../agent-contract.md)

## Context

A coding agent that needs one rule's element cost has to infer the command shape. In an observed run it guessed `wright inspect cost "<rule name>" src/main.opy`, because `inspect refs <NAME>` and `inspect cfg <RULE>` take the name as a positional argument. The command failed and the agent recovered through `--help`. ADR-0022 sets the standard: help and errors describe themselves, and the agent guide shrinks as they improve.

The command grammar already follows one rule: a required subject is a positional argument, and optional narrowing is a flag. The failure comes from the flag vocabulary. These spellings mean different things in different commands:

| Spelling | Meaning in some commands | Meaning in others |
| --- | --- | --- |
| `--rule` | `inspect rules\|refs\|cost`: a declared rule name | `lint`, `serve`, `ui`: a YAML rule file or directory |
| `--only` | `inspect symbols\|refs\|cfg`: a category of result | `inspect rules`: a declared rule name |
| `--kind` | every workflow command: the input frontend | `lookup`: an entry category |

One meaning also has two spellings: `inspect rules --rule <RULE>` and `inspect rules --only <NAME>` select a rule by declared name, with identical results.

## Decision

Each selector spelling has exactly one meaning in every command that accepts it.

| Spelling | Meaning | Where |
| --- | --- | --- |
| `--rule <NAME>` | A declared rule, by name | `inspect rules`, `inspect refs`, `inspect cost` |
| `--only <CATEGORY>` | Narrow the result to one category of its own domain (symbol kind, reference kind, block kind, lookup entry kind) | `inspect symbols`, `inspect refs`, `inspect cfg`, `lookup` |
| `--kind <KIND>` | The input frontend | commands that take an `INPUT` |
| `--rule-id <ID>` | A lint rule by stable id | `check`, `analyze`, `lint`, `inspect cost` |
| `--rule-file <PATH>` | A local YAML rule file or directory (repeatable) | `lint`, `serve`, `ui` |
| `--file <PATH>`, `--max <N>` | Unchanged | unchanged |

Consequences for current spellings:

1. The lint policy option `--rule <PATH>` becomes `--rule-file <PATH>`. It sits beside `--rule-id`, `--disable-rule`, and `--rule-severity`.
2. `inspect rules --only <NAME>` is removed. `--rule <NAME>` is the only name selector.
3. `lookup --kind <CATEGORY>` becomes `lookup --only <CATEGORY>`. `lookup` takes no `INPUT`, so `--kind` keeps the input-frontend meaning everywhere it appears.
4. Role selectors keep their names. `inspect callgraph --caller <RULE>` and `--callee <SUBROUTINE>` select edges by direction, and each side has one noun. They do not conflict with `--rule`.

Grammar rules that stay in force:

- A required subject is a positional argument (`refs <NAME>`, `cfg <RULE>`). Optional narrowing is a flag. `INPUT` is the last optional positional argument and defaults to the current directory (ADR-0016).
- The CLI addresses rules and symbols by declared name only. Numeric addresses belong to `wright serve` and carry a `snapshot` (ADR-0034).
- A removed spelling is a usage error. No alias is kept.

Error text is part of the vocabulary. A usage error or `invalid-selection` error names a runnable form of the command, and an unknown-name error names how to list the declared names. The driver returns one structured hint. The CLI renders it as a command (`wright inspect rules`). `wright serve` keeps the operation name (`rules`). This is specified by Issue #888.

## Consequences

- An agent that learns `--rule <NAME>` on one `inspect` query can apply it to the others without a second meaning.
- Breaking change for scripts that pass `lint --rule <PATH>`, `inspect rules --only <NAME>`, or `lookup --kind`. This is acceptable before 1.0 and keeps no compatibility layer.
- `docs/cli/commands.md`, `docs/cli/lint.md`, `docs/agent-contract.md`, `docs/cli/presentation.md`, and the vendored agent guide change in the same Issue (#889).
- The `SKILL.md` text in `wrightkit/skills` needs a separate update after the wording settles.

## Compatibility impact

No source-language compilation, diagnostic code, or normalized-output claim changes. The public CLI spellings change as listed. `wright-agent/v1` operation fields do not change. Tests in `crates/wright-cli/tests/` cover each renamed and removed spelling.

## Scope boundaries

- **Agent-contract field names.** The `rules` operation has both `name` (a declared name) and `rule` (an index or declared name). The `symbols`, `references`, and `cfg` operations use `kind` where the CLI uses `--only`. Whether to merge `name` into `rule` is a `wright serve` contract decision and needs its own record.
- **Cost entry points.** `analyze`, bare `inspect`, `inspect cost`, and `compile --baseline` all report element cost. Whether to reduce them is a separate product decision.
- **Subject-first commands.** A shape such as `wright rule <NAME> --cost` is not decided here. There is no evidence yet that it outweighs ADR-0022's rule that the CLI uses the same names as the agent operations.
- **Renderer defaults.** The agent guide asks for `--renderer plain` on every call. Whether `--renderer auto` should select plain output for non-terminal output is separate and untested here.
