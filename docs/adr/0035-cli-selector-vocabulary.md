# ADR-0035: One meaning per selector spelling

- Status: Accepted
- Date: 2026-10-22
- Related: [Issue #888](https://github.com/wrightkit/wright/issues/888), [Issue #889](https://github.com/wrightkit/wright/issues/889), [Issue #891](https://github.com/wrightkit/wright/issues/891), [Issue #892](https://github.com/wrightkit/wright/issues/892), [Issue #895](https://github.com/wrightkit/wright/issues/895), [ADR-0016](0016-current-directory-and-directory-project-targets.md), [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0024](0024-structured-rule-content.md), [ADR-0027](0027-project-run-cache.md), [ADR-0029](0029-structured-statement-edits.md), [ADR-0034](0034-snapshot-scoped-numeric-addresses.md), [Agent contract](../agent-contract.md)

## Context

A coding agent that needs one rule's element cost has to infer the command shape. In an observed run it guessed `wright inspect cost "<rule name>" src/main.opy`, because `inspect refs <NAME>` and `inspect cfg <RULE>` take the name as a positional argument. The command failed and the agent recovered through `--help`. ADR-0022 sets the standard: help and errors describe themselves, and the agent guide shrinks as they improve.

The command grammar already follows one rule: a required subject is a positional argument, and optional narrowing is a flag. The failures come from the selector vocabulary. Measured with wright 0.29.0:

**One spelling, several meanings.**

| Spelling | Meaning in some commands | Meaning in others |
| --- | --- | --- |
| `--rule` | `inspect rules\|refs\|cost`: a declared rule name | `lint`, `serve`, `ui`: a YAML lint rule file or directory |
| `--only` | `inspect symbols\|refs\|cfg`: a category of result | `inspect rules`: a declared rule name |
| `--kind` | every workflow command: the input frontend | `lookup`: an entry category |

**One noun, two resolution behaviors.** With two rules named `tick`, `inspect rules --rule tick` fails with `ambiguous-rule`, while `inspect rules --only tick` lists both rules. `inspect callgraph --caller tick` merges both rules into one caller with no error. `wright serve` has the same split: `rules` `rule` fails, `rules` `name` lists both.

**A duplicated rule name has no CLI selector.** The ambiguity error is `ambiguous rule 'tick': rule 0, rule 1`. No CLI option accepts a rule index: `--rule 0` is `unknown rule '0'`. A Workshop rule may be unnamed, and every unnamed rule shares the empty name.

**Rules make symbol names ambiguous.** `inspect refs <NAME>` resolves every symbol kind, rules included. A global variable `score` and a rule named `score` make `refs score` fail with `ambiguous-symbol`, although a rule has no references other than its declaration, which `inspect rules` already reports. Raw Workshop also allows a global variable, a player variable, and a subroutine to share one name, and the error names symbol ids that the CLI does not accept.

**An unmatched `--file` is silently empty.** `inspect symbols --file nope.opy` exits 0 with `selected: symbols=0/4`, and `analyze --file nope.opy` exits 0. A misspelled path looks like a clean result. `wright serve` returns `"symbols": []` for the same `file` value.

## Decision

### Selector spellings

Each selector spelling has exactly one meaning in every command that accepts it.

| Spelling | Meaning | Where |
| --- | --- | --- |
| `--rule <RULE>` | One declared Workshop rule, by rule address | `inspect rules`, `inspect refs`, `inspect cost` |
| `--caller <RULE>` | One declared Workshop rule, by rule address, as the calling side of an edge | `inspect callgraph` |
| `--callee <SUBROUTINE>` | One declared subroutine, as the called side of an edge | `inspect callgraph` |
| `--only <CATEGORY>` | Narrow the result to one category of its own domain: symbol kind, reference kind, block kind, or lookup entry kind | `inspect symbols`, `inspect refs`, `inspect cfg`, `lookup` |
| `--kind <KIND>` | The input frontend | commands that take an `INPUT` |
| `--rule-id <ID>` | A lint rule, by stable id | `check`, `analyze`, `lint`, `inspect cost` |
| `--rule-file <PATH>` | A local YAML lint rule file or directory (repeatable) | `lint`, `serve`, `ui` |
| `--file <PATH>` | One source file of the loaded program | every command that has it today |
| `--max <N>` | Unchanged | unchanged |

The noun convention is: bare `--rule`, `--caller`, and the positional `<RULE>` name a Workshop rule. A `--rule-` prefix (`--rule-id`, `--rule-file`, `--rule-severity`) and `--disable-rule` name a lint rule.

Changes to current spellings:

1. The lint policy option `--rule <PATH>` becomes `--rule-file <PATH>` on `lint`, `serve`, and `ui`.
2. `inspect rules --only <NAME>` is removed, and so is the `rules` operation's `name` field in `wright serve`. A rule is selected with `--rule` (`rule` in `wright serve`). The full rule list shows every rule address, so a list of rules with one name is still one query away.
3. `lookup --kind <CATEGORY>` becomes `lookup --only <CATEGORY>`. `lookup` takes no `INPUT`, so `--kind` means the input frontend everywhere it appears.
4. A removed spelling is a usage error. No alias is kept.

Grammar rules that stay in force:

- A required subject is a positional argument (`refs <NAME>`, `cfg <RULE>`). Optional narrowing is a flag. `INPUT` is the last optional positional argument and defaults to the current directory (ADR-0016).
- The CLI addresses rules and symbols by name. Numeric addresses belong to `wright serve` and carry a `snapshot` (ADR-0034).

### Rule addresses

A rule address is a string that resolves to exactly one rule. The same resolver serves the CLI and `wright serve` (ADR-0022). It accepts two forms:

- **`NAME`**, the declared rule name.
- **`NAME#N`**, where `N` is a positive decimal integer with no leading zero. It means the `N`th rule, counted from 1 in program rule order, among the rules whose declared name is `NAME`. Program rule order is the order `inspect rules` lists. The form is valid for every rule, not only for duplicated names. An unnamed rule has the empty name, so its address is `#N`.

Resolution:

1. Find the candidates: the rules whose declared name equals the string, plus, when the string ends in `#N`, the `N`th rule named by the part before the last `#`.
2. One candidate resolves.
3. No candidate is `unknown-rule`.
4. More than one candidate is ambiguous. This covers a duplicated `NAME`, and a string that is both a declared name and a valid `NAME#N` of another rule. For example, with rules `a`, `a`, and `a#2`, the string `a#2` is ambiguous.

The ambiguity error lists each candidate with its location and the canonical address of each candidate.

The **canonical address** of a rule is the address that Wright prints and the address that callers pass back:

- `NAME` when the name is unique and is not also a valid `NAME#N` of another rule.
- Otherwise `NAME#N`, when that string resolves to this rule alone.

In the rare case where neither form resolves to one rule (the second `a` in the example), the rule has no name address. Its canonical address is printed as `NAME#N`, and passing it back fails as ambiguous, never as a different rule. The ambiguity error tells the user to rename one of the conflicting rules. In `wright serve` the numeric index with `snapshot` stays available.

Text output prints a rule by its canonical address wherever it names one: `inspect rules`, the owning rule of a reference, the caller of an edge, finding records, and the `inspect cost` share list. JSON and `wright serve` results carry the same canonical address beside the rule's `name`, so every surface returns the same facts (ADR-0022). A rule printed by Wright can be passed back to any rule selector and selects that rule or fails.

The address grammar applies wherever a rule is addressed by name: `--rule`, `--caller`, and `cfg <RULE>` on the CLI, and in `wright serve` the string `rule` of `rules`, `references`, `cfg`, and `costEstimate`, the `caller` of `callGraph`, and the rule targets of `editStatements` (ADR-0029). An ambiguous `--caller` fails as `--rule` does. It no longer merges rules that share a name.

`NAME#N` is a name address: it ignores `snapshot` (ADR-0034 decision 4). A change that adds or removes a rule with the same name earlier in the program changes which rule `NAME#N` means. This is the same exposure a declared name already has when a rule is renamed. A `wright serve` session that must survive reloads uses the numeric index with `snapshot`.

The diagnostic codes do not change. A rule that is the subject of an operation (`cfg`, `rules`, `editStatements`) fails with `unknown-rule` or `ambiguous-rule`. A rule used as a filter (`references`, `costEstimate`, `callGraph`) fails with `invalid-selection`. The message lists the same candidates in both cases.

### Symbol addresses

`inspect refs <NAME>`, and the name form of `symbol` in the `references` and `usage` operations, resolve declared non-rule symbols only: global variables, player variables, and subroutines. A rule that shares a name with a variable does not make the name ambiguous. Rules are addressed with rule addresses. A numeric `symbol` id is unchanged.

When two non-rule symbols share a name, the symbol address `KIND:NAME` selects one. `KIND` is a value of `inspect symbols --only` other than `rule`: `globalVariable`, `playerVariable`, or `subroutine`. Variable and subroutine names are identifiers and cannot contain `:`, so the form never conflicts with a declared name. Because refs no longer resolves rules, a rule named `globalVariable:x` does not conflict either. The `ambiguous-symbol` error lists the `KIND:NAME` form and the location of each candidate.

### Selectors narrow or fail

A selector narrows the result or fails. It is never ignored or silently empty.

- A `--file` value (`file` in `wright serve`) that matches no source file of the loaded program is an `invalid-selection` error. The source files of the loaded program are the files that contribute to it, not every file in the directory. The error lists those files, bounded, with the total. Path resolution is unchanged: as passed, root-relative, or absolute, with no suffix matching. A `--file` that matches a program file and selects nothing is a valid empty result.
- This applies to finding selection (`check`, `analyze`, `lint`, `inspect cost`) and to semantic query selection (`inspect rules|symbols|refs`).
- A combination that a command cannot apply stays a usage error, as `inspect cost --rule` with `--severity`, `--rule-id`, `--max`, or `--file` already is. Without `--rule`, `inspect cost --file` selects the static findings in that file.

### Error text

Error text is part of the vocabulary. A usage error or an `invalid-selection` error names a runnable form of the command, and an unknown-name error names how to list the declared names. The driver returns one structured hint. The CLI renders it as a command (`wright inspect rules`), and `wright serve` keeps the operation name (`rules`). Issue #888 specifies the wording.

## Consequences

- An agent that learns `--rule <RULE>` on one query can use the same address on every rule selector, in the CLI and in `wright serve`, and can pass back any rule that Wright printed.
- Every rule is addressable by name except a rule whose name collides with another rule's `NAME#N`. That case fails as ambiguous and never selects the wrong rule.
- `--caller` with a duplicated rule name changes from a merged result to an error.
- Breaking change for scripts that pass `lint|serve|ui --rule <PATH>`, `inspect rules --only <NAME>`, or `lookup --kind`, for `wright serve` clients that send `rules` `name`, and for callers that relied on an unmatched `file` returning an empty result. This is acceptable before 1.0. No compatibility layer is kept.
- Documentation changes with the implementation (#889): `docs/cli/commands.md`, `docs/cli/lint.md`, `docs/cli/presentation.md`, `docs/agent-contract.md`, `docs/specs/SPEC-309-extensible-lint-rules.md`, and the vendored agent guide. ADRs keep the spellings they recorded.
- Consumers outside this repository: the `wrightkit/skills` guide contains no removed spelling. The `benchmark-suite` trace classifier counts `--only` as a bounded flag, so `lookup --only` changes its count when the suite's Wright pin advances past this change.

## Compatibility impact

No source-language syntax, compilation output, or diagnostic code changes.

The public CLI spellings change as listed. `wright-agent/v1` changes in place, as ADR-0034 did before 1.0:

- the `rules` `name` field is removed;
- rule address strings accept `NAME#N` and fail on a collision;
- results that identify a rule gain its canonical address (an additive field);
- `references` and `usage` name resolution excludes rules and accepts `KIND:NAME`;
- an unmatched `file` is `invalid-selection`;
- an ambiguous `callGraph` `caller` is `invalid-selection`.

The JSON schema, agent contract, and MCP tool definitions change with the implementation.

Required tests, through both the CLI and `wright serve`:

- each renamed spelling works, and each removed spelling is a usage error;
- `NAME#N` selects the `N`th rule with that name;
- an out-of-range `N` fails;
- a rule literally named `a#2` is selected by `a#2` when no other reading exists;
- rules `a`, `a`, `a#2` make `a#2` ambiguous, and `a#2#1` selects the literal rule;
- unnamed rules are addressed by `#N`;
- every rule address printed by `inspect rules` selects its rule or fails as ambiguous;
- `refs` with a variable and a rule of one name returns the variable;
- `KIND:NAME` resolves a variable or subroutine collision;
- an unmatched `--file` fails;
- a matched `--file` with no results passes.

## Scope boundaries

- **Agent-contract field names.** The `symbols`, `references`, and `cfg` operations call the category filter `kind`, where the CLI says `--only`. The meaning is the same, so this is a naming question for `wright serve` and is not decided here.
- **Output streams and text determinism.** Where a failed text result writes its diagnostics, and how the docs describe `inspect cost` text that depends on `.wright/` state, are tracked in #891. `docs/cli/machine-contract.md` owns that contract. This ADR does not change ADR-0027.
- **Reference locations in OPY projects.** Read, write, and call references in OPY projects have no source span. The owner of that mapping is classified in #892.
- **Cost entry points.** `analyze`, bare `inspect`, `inspect cost`, and `compile --baseline` all report element cost. Whether to reduce them is a separate product decision.
- **Subject-first commands.** A shape such as `wright rule <NAME> --cost` is not decided here. There is no evidence yet that it outweighs ADR-0022's rule that the CLI uses the same names as the agent operations.
- **Renderer defaults.** The agent guide asks for `--renderer plain` on every call. Whether `--renderer auto` should select plain output for non-terminal output is separate and untested here.
