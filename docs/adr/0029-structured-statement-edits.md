# ADR-0029: Structured statement edits for raw Workshop

- Status: Proposed
- Date: 2026-10-19
- Related: [ADR-0021](0021-name-and-signature-lookup.md), [ADR-0024](0024-structured-rule-content.md), [ADR-0028](0028-profile-applies-to-workshop-input-only.md), [Agent contract](../agent-contract.md), `workshop-rs` `docs/source-preservation.md` and its ADR-0017

## Context

ADR-0024 made rule content readable as `workshop-rs/rule-content-v1` call trees and left "a typed authoring API" undecided. Two consumers now need the write direction:

- **Agents.** To add or change a condition or action, an agent writes Workshop text, using `lookup` (ADR-0021) for spellings. It then checks the result with `validateEditTransaction`. The agent therefore has to know locale spellings, number and string formatting, variable syntax, operator layout, and indentation. Goal principle 4 promises structured access instead.
- **A structured editing view.** A read-only prototype builds the in-game "create condition / create action" forms from `lookup` signatures. Each form produces a correct `rule-content-v1` node: its shape matches the content Wright returns for the same calls in a real project. No operation can turn that node into source text. The only way to write it today would be for the client to compose Workshop text itself.

Wright already has most of the pieces it needs:

- `workshop-rs` keeps the exact authored source, exposes rule, condition, and action spans, and produces checked, fail-closed `SourceEdit` replacements through `Program::edit_source`. Bytes outside an edit are unchanged.
- `validateEditTransaction` applies a text transaction, reparses and revalidates it through the session's `workshop-rs` path, and returns diagnostics or a preview. A stale source refuses with `edit-stale-source`.
- `rule-content-v1` is a deterministic, locale-independent export of canonical statements.

Two pieces are missing:

- A way back from a content node to the public model.
- Statement-level emission. `workshop-rs` emits only complete programs, and canonical whole-program regeneration does not preserve comments or formatting.

## Decision

### 1. Input form: `rule-content-v1` list items

A statement is written in the form `rule-content-v1` already uses for list items:

- A condition is `{"value": <node>}`.
- An action is an action node, `{"call": id, "args": [...]}`.

Either may carry `"disabled": true`. Spans are output-only and are rejected on input. No second authoring format is introduced. A node that a client read from `rules` with `content: true` is valid input unchanged.

### 2. Owner split

| Owner | Responsibility |
| --- | --- |
| `workshop-rs` | Converts a `rule-content-v1` statement into the public model. Unknown ids, wrong argument counts, internal sentinels, and nodes that cannot form a statement fail with a structured error. Produces the checked `SourceEdit` that inserts, replaces, or deletes one condition or action of a parsed program at a statement address. That includes the statement's canonical text in the session's resolved Workshop locale (decision 3), its indentation relative to the surrounding source, statement comments (decision 3), and creating a missing section. Owns the round-trip contract in decision 5. |
| Wright | Owns the operation, addressing and freshness, the transaction it assembles from owner edits, its validation through the existing `validateEditTransaction` path, refusals, and presentation in `serve` and MCP. |

Wright composes no Workshop text and reads no catalog spellings to build it. Locale, spelling, literal formatting, variable syntax, operator layout, and indentation are decided only by `workshop-rs`.

### 3. Surface: `editStatements`

`wright-agent/v1` gains one operation:

```json
{
  "op": "editStatements",
  "inputIdentity": "<project.inputIdentity>",
  "edits": [
    { "rule": 3, "list": "actions", "index": 2, "change": "insert",
      "statement": { "call": "startThrottleInDirection", "args": [ ... ] } },
    { "rule": "Cover enums 108", "list": "conditions", "index": 0, "change": "replace",
      "statement": { "value": { "call": "==", "args": [ ... ] } } }
  ]
}
```

**Addressing.**
- `rule` is the same `Address` that `rules` and `cfg` accept.
- `list` is `conditions` or `actions`.
- `index` is the position in that list. For actions this is the flat index space that `rules`, `cfg`, `references`, and findings already share.
- `change` is `insert`, `replace`, or `delete`. `insert` at the list length appends. `delete` takes no `statement`.

**Atomicity.**
- Every address in a request refers to the program before any edit in that request.
- Two edits at the same address refuse, as zero-width conflicts do in `EditTransaction`. Edits do not see each other's results, so an agent that adds an `If` and its `End` sends both in one request.

**Freshness.** `inputIdentity` must equal the current `project.inputIdentity`. Otherwise the request refuses with `edit-stale-source` and edits nothing, so a client cannot address statements in a program it has not read.

**Unsaved buffers.** The request takes the same optional `sources` as `validateEditTransaction` (#472). When it is omitted or null, the program is loaded from the files on disk. When it is supplied, the program is loaded from those texts, which are also the precondition texts, and `inputIdentity` is checked against the identity of that loaded program. A client holding unsaved editor buffers can therefore address statements in the text it shows.

**Locale.** An emitted statement uses the session's resolved Workshop locale for its source: the `--locale` override when the session has one, otherwise the locale detected for that source. This is the same resolution that reading uses, so a statement is written in the locale its neighbours were read in.

**Statement comments.** A Workshop statement can carry a leading comment string, such as `"BOTTOM LINE"` before an action. `rule-content-v1` has no comment field, so:
- `replace` keeps the replaced statement's comment and changes only the statement;
- `delete` removes the statement together with its comment;
- `insert` writes no comment.

Creating or editing a comment is out of scope (see Scope boundaries).

**Empty lists.** A rule can have no conditions section at all. An `insert` into a list whose section is absent creates the section. After a `delete` of the last statement, the rule must reparse with an empty list; whether the empty section stays in the text is a `workshop-rs` layout decision.

**Result.** The result has the `validateEditTransaction` payload (`ok`, `exit`, `diagnostics`, and `preview` when valid) plus the assembled `transaction`. A client holding editor buffers can apply the transaction itself; other clients write the preview. Nothing is written by the operation: as with every Wright edit, applying it is the caller's write path.

**Validity.** Whether the edited program is valid is decided by revalidation, not by the operation. An `If` without an `End`, or a value of the wrong type, comes back as `ok: false` with owner diagnostics. The operation adds no pairing, typing, or normalization rules of its own.

**Locality.** Only the statements named by the request are converted and emitted. Other statements are left as source bytes. A rule whose existing content `rule-content-v1` cannot express, such as an unresolved ambiguous enum, can still take an insert or an edit to a different statement.

**Surfaces.** `serve` exposes the operation, and the MCP adapter exposes it as `wright_edit_statements` with an input schema derived from the request schema (ADR-0020).

### 4. Raw Workshop only

`editStatements` applies to raw Workshop input. For an OverPy or OSTW input it refuses with a structured error that names the owning implementation and edits nothing. Like ADR-0028's profile rule, it neither silently ignores the request nor accepts it and does nothing. Canonical content of a source language is lowered output, and its statements do not map back to authored source. Source-language authoring needs its own owner capability, negotiated through the language provider protocol, and is not decided here.

### 5. Round-trip contract

For every statement a raw parse can produce whose content `rule-content-v1` can express, `workshop-rs` guarantees:

```
content(parse(emit(statement_from_content(c)))) == c
```

This is checked over the owner's corpora in every supported Workshop locale. The guarantee lets a client read a statement, change one argument, and write it back without any other change to the meaning of that statement. It does not promise byte-identical text: an edited statement is emitted canonically, even if the replaced text was spelled differently.

**Canonical form is visible.** A replaced statement is emitted in its canonical spelling, not in the spelling it was authored in. An alternative spelling the parser accepts, number formatting, and line wrapping all come back the way the canonical emitter writes them. Only the replaced statement changes, but a user who edits one argument can see the whole line respelled. This is accepted: it keeps emission in one place. Preserving the authored spelling would need a spelling-aware edit that the owner does not have.

## Alternatives considered

- **The client composes text from `lookup` spellings and validates it with `validateEditTransaction`:** rejected. Every client, including agents, an editing view, and the benchmark, would carry its own copy of Workshop emission. Validation would catch invalid text, but not a valid statement emitted differently from canonical output.
- **Regenerate the whole program with the canonical emitter and diff it:** rejected. Canonical emission does not preserve comments or formatting, so one inserted action would rewrite the file.
- **A separate typed authoring format or Rust builder as the wire input:** rejected. It would add a second format next to `rule-content-v1` and force readers to translate between them.
- **Addressing nested arguments by path:** rejected. The statement is the unit of span, validation, and index space. Replacing the whole statement covers argument changes without a second address space.
- **An operation that also writes to disk:** rejected. It is inconsistent with the existing edit contract, where `validateEditTransaction`, `semanticRename`, and lint fixes return validated edits and the caller writes them.
- **Extending `validateEditTransaction` to accept structured edits:** rejected. That operation's contract is caller-proposed text, and mixing the two input kinds would blur its refusals and its MCP schema.

## Consequences

- Agents can add, change, and remove conditions and actions by catalog id and argument position, and they get owner diagnostics before any byte is written.
- A structured editing view can write what its forms build without owning any Workshop syntax. Where that view lives and what it looks like is a separate decision.
- `rule-content-v1` becomes an input format as well as an output format. Its stability now affects writers, and an incompatible change needs a new format id under the rules ADR-0024 set.
- Follow-up work:
  - `workshop-rs`:
    - content-to-model conversion and structured errors;
    - statement-level edits with locale, indentation, comment preservation, and section creation;
    - round-trip tests over its corpora, including statements with comments and rules without a conditions section.
  - Wright:
    - `editStatements` in `wright-driver`, the schema `$defs`, the agent contract, and the MCP tool;
    - driver tests on a real raw Workshop project in at least `en-US` and `zh-CN`, including a `--locale` override and supplied `sources`;
    - refusal tests for stale input, conflicts, and source-language input.
- Authoring is limited by what `lookup` describes. Gaps found by the editing prototype are tracked as separate `lookup` and `rule-content-v1` work, outside this decision. They include missing value return types, missing signatures for structural actions such as `Set Global Variable`, partial parameter defaults, and unlocalized parameter names.

## Compatibility impact

- **`wright-agent/v1`:** additive. The new operation is advertised through `capabilities.operations`, and existing operations are unchanged.
- **`workshop-rs`:** additive public API. `rule-content-v1` keeps its format id and schema, and the input direction is a new guarantee on the same schema.
- **Emitted Workshop:** an edited statement is canonical `workshop-rs` emission in the file's locale. Statements and bytes outside the edit are unchanged, as `SourceEdit` already guarantees.
- **Unaffected:** source-language compilation and structural convergence (goal principle 7), lint, `validateEditTransaction`, and every read operation.

## Scope boundaries

- Not decided:
  - rule-level edits: creating, deleting, or reordering rules, and changing a rule's name, event, or disabled state;
  - variable and subroutine declarations;
  - workshop settings, which `SettingSourceEdit` already covers;
  - a CLI command.
  Each needs its own decision when a consumer needs it.
- Not decided: creating or editing statement comments. That needs a comment field in a future `rule-content` format version, which `workshop-rs` owns.
- Not decided: source-language authoring for OverPy or OSTW (decision 4).
- Not decided: the form factor of a local editing view.
- The exact Rust API, error ids, and indentation rules belong to `workshop-rs`. The request field spellings above are illustrative until the Wright schema is written, but the addressing, atomicity, freshness, and write-path rules are decided here.
