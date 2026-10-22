# ADR-0030: One result model, three renderers, and the CLI/action boundary

- Status: Accepted
- Date: 2026-10-19
- Related: [ADR-0022](0022-shared-surface-for-semantic-queries.md), [ADR-0025](0025-essential-default-output.md), [ADR-0027](0027-project-run-cache.md), [CLI presentation contract](../cli/presentation.md), [Machine contract](../cli/machine-contract.md)

## Context

Wright's text output answers a single reader: a person at a terminal. Three other readers use the same commands, and the current output serves them poorly. Measured with wright 0.26.2 on the Bastion project (205 files, 330 rules):

- **A person at a terminal** gets a verdict, then flat sections. The rail frame (#703, #718) draws every item the same way, so nothing outranks anything else. Counts read `205 file(s)`, evidence repeats as a suffix on every line, and a failed computation (`element count unavailable`) shares a sentence with the counts that did succeed.
- **A coding agent** reads the plain renderer, because a piped or non-TTY run selects it. `analyze` is 3,301 bytes in 47 lines. Facts the agent needs are buried in sentences: the repeated expression and its count live inside a message string (`is evaluated 3 times within the same loop scope; the expression's own span is not recorded, so the location is the enclosing loop`). `--all` is 57,199 bytes.
- **A reviewer in GitHub Actions** sees one annotation per finding, a log group containing only `WARN lint`, and one summary line (``Wright `lint`: **WARN** (exit 0)``). `lint` emits 18 annotations (5 warnings, 13 notices), with identical locations repeated. GitHub caps annotations per step and per job and drops the excess silently. The `analyze` body is never rendered under this renderer, so cost, hotspots and coupling are invisible in CI. A pipeline that checks four inputs writes four identical summary lines.

The current presentation contract states that `plain`, GitHub Actions, CI and JSON keep their undecorated layout byte-for-byte and that the GitHub Actions renderer is unchanged. Improving these readers changes that contract, so the decision is recorded here. ADR-0025 already sets the default that agents read text and that text and JSON carry the same content; this ADR extends it and does not replace it.

## Decision

### Renderer model

1. **A command produces one result model and every renderer is a pure function of it.** The terminal, plain and GitHub Actions renderers and JSON present the same facts, verdicts, exit codes, caps and `--all` expansion. A renderer never decides what is true. This is ADR-0022's one shared surface applied to presentation.
2. **A report reads in the order of the reader's next action:** verdict, a one-sentence headline that answers the command's question, sections ordered by what needs action before what is reference data, then the commands to run next. Per-command ordering and content belong to the CLI presentation contract.
3. **Unknown is not a value.** A computation that failed is reported as unavailable with its reason, never as zero, and a known limit such as the element budget is reported even when the count is not.

### Terminal renderer

4. **The terminal renderer has one filled element, the verdict tag, whose word maps to the exit code.** Judging commands (`check`, `compile`, `lint`, `analyze`) show their outcome; query commands show their own name. The rail frame is retired. Colour is semantic, uses only the eight ANSI hues, and status never depends on colour alone. Tag set, glyphs, widths and section layouts are specified in the presentation contract.

### Plain renderer as the agent surface

5. **Plain output is the agent-facing text surface and is defined as line records.** Each line is `kind: key=value …`; a location is one `path:line:col` token; facts are fields, not sentences; ordering matches the terminal renderer; a capped list is followed by a record that states shown, total and the exact expansion command; follow-up commands appear as `next:` records; there is no decoration, plural suffix or wall-clock value. This replaces the prose layout.
6. **There is no separate agent mode or flag.** Renderer selection stays `auto` (GitHub Actions, then plain for CI or non-TTY, then terminal). Structured access remains `-f json` and `wright serve`.
7. **Record kinds and keys are a contract.** They are specified in the presentation contract, stay stable between releases, and are shared with JSON field names where a field exists in both.

### GitHub Actions renderer

8. **Each surface has one job.** Annotations say where to change something, the step summary gives the overall picture, and the log group holds the full facts. The three are rendered from the same model and do not repeat each other.
9. **Annotations are for errors and warnings only.** They are deduplicated by rule and location, ordered by severity, and bounded to the platform's per-step limits. When findings are withheld, one closing notice says where they are. Info-level findings appear in the summary.
10. **The step summary is Markdown with one section per invocation,** titled with the command and its input. It carries the verdict, the headline, the leading items, locations as links built from standard runner variables, and the remainder in a collapsed block, within the platform's size limit. It replaces the one-line summary.
11. **The log group title carries the verdict and headline,** and its body is the plain records, so a collapsed group is readable and an expanded one is complete.

### CLI and action boundary

12. **The CLI is complete without configuration.** Under `GITHUB_ACTIONS` it produces the surfaces above, reading only standard environment variables, never an action's inputs.
13. **Gating is an opt-in CLI option, default off.** A threshold option (for example `--fail-on <severity>`) makes a result at or above the threshold exit 1 with a FAIL verdict. It can only raise a verdict, so the rule that selection never turns a failing command into a passing one is unchanged. A `WARN` result still exits 0 by default. When to enable it is the action's decision.
14. **Baselines in CI use the CI cache and degrade silently.** Explicit `--baseline` and `--save-baseline` read and write `.wright/` even when `CI` is truthy, so an action can restore and save that directory with the platform's cache. Under `CI` the comparison is enabled only when a usable baseline exists. A missing cache, an unknown baseline name, or a record that is unreadable, incompatible or keyed to a different input, kind, locale or profile switches the comparison off without an error, a warning or any other output, and the command's verdict and exit code are unchanged. Outside `CI` an unknown name given with `--baseline` stays a usage error. Implicit previous-run comparison stays disabled under `CI`. This amends ADR-0027 decision 9 (which defers exactly this case) and decision 7 (for `CI` only), and resolves the shared-baseline question its scope boundaries left open. The GitHub Actions renderer shows the comparison when one was produced.
15. **Integrations that need write permission or workflow semantics are not in the CLI.** Pull-request comments, job outputs and matrix roll-ups belong to a thin `wright` action (install, cache, flag policy) or to separate, specialised actions. The `wright` action is not all-in-one.

*Amended by #891: a failed analysis result's diagnostics join the verdict and summary on stdout, so a caller reading stdout alone sees the failure's reason; stderr carries only usage errors (exit 2) and internal failures (exit 4). Gated results, artifact streams, and successful results keep the original split — diagnostics on stderr. Owner-approved 2026-10-22.*

## Alternatives considered

- **An `--agent` renderer.** A second text format to maintain beside plain, which agents already receive automatically. Rejected: the formats would drift.
- **Keeping the prose plain layout.** It costs tokens and hides the facts an agent acts on inside sentences. Rejected.
- **Keeping the rail frame.** It spends two columns and gives every line equal weight, and plain, GitHub Actions and JSON never used it. Rejected.
- **Annotating every finding.** Excess annotations are dropped silently by the platform and notices crowd the pull-request file view. Rejected.
- **Posting pull-request comments from the CLI.** It needs a token with write permission, repeats the annotations, and is an external write. Rejected.
- **An all-in-one action.** It ties the CLI's release to integrations that change more often and need different permissions. Rejected.
- **Failing on warnings by default.** It would break existing pipelines that rely on `WARN` exiting 0. Rejected; the option is opt-in.
- **Writing `GITHUB_OUTPUT`.** No workflow reads it today. Deferred until a real consumer exists.

## Consequences

- The plain text shape, the GitHub Actions annotations and summary, and the terminal layout change. Consumers that parse plain text (the agent guide in `wrightkit/skills`, benchmark adapters, the CI Workshop dogfood) move to the new records or to JSON.
- Per ADR-0025's token standard, scenario costs are expected to fall, and a changed baseline is part of the same pull request.
- The renderers need data the driver does not return today: structured fields for repeated-value findings (expression, count, whether the span is the enclosing loop), per-code finding counts computed before truncation, the element budget when the count is unavailable, rule identity on `inspect refs` entries, a program summary on `check`, and a shared list of follow-up commands. These are prerequisites for the implementing issues, not part of this decision.
- Three inconsistencies found while measuring are defects to fix on their own, not by this ADR: `check` passes where `compile` fails on the same project, the rule count differs between `analyze` (330) and `inspect cost` (320) and the finding total between them (21 and 20), and the diagnostic code `unknown-enum member` contains a space.
- Documentation to update with the implementation: `docs/cli/presentation.md` (renderers, record format, GitHub Actions surfaces), `docs/cli/commands.md` (the gating option and the CI baseline behaviour), and a note on ADR-0027's decisions 7 and 9 pointing here.

## Compatibility impact

A pre-1.0 shape change of the plain and GitHub Actions text, recorded here and released as a breaking change, in line with ADR-0025. The `wright-result/v1` envelope gains fields additively and its contract identifier is not bumped, because the contract is not frozen until 1.0. No source-language syntax, Workshop semantics, compiler output, diagnostic set or default exit code changes; the gating option is off by default.

Required verification: byte-deterministic golden tests for the plain records; an annotation test that checks deduplication and the per-step bound; renderer-parity checks that the same result yields the same verdict, exit code and counts on every renderer; and the ADR-0025 scenario-cost measurements.

## Scope boundaries

- Record kinds, key names, glyphs, colours, widths and per-command layouts are specified in the presentation contract, not here, and may be tuned without a new ADR.
- The exact name and thresholds of the gating option, the shape of the shared follow-up-command field, and the cache key a `wright` action uses for baselines are decided by the implementing issues.
- The `wright` action and any specialised action are designed in their own repositories under their own decisions.
- Suggesting a locale that would accept a rejected name needs a driver capability that does not exist yet and is decided separately.
- Platform limits (annotation counts, summary size) are external and may change; the contract is to stay within the current documented or observed limits.
