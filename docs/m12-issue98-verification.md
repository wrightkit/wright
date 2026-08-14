# Issue #98 independent QA verification report

related_issue: "#98"
freshness: snapshot
as_of_commit: a54433b
status: verification
owner: QA

Independent verification of issue #98 ("[M12] Add first-class lint CLI and
shared structured results") at commit `a54433b8a6ca6e8508ecd9be02ddb30a827d65d2`
("feat(cli): add first-class lint command with shared structured results
(M12)"), working tree clean, CI run 31784510074 fully green. All evidence
re-derived from the working tree, freshly rebuilt binaries, and CI data;
nothing taken from the Engineer report on trust. Report only — no code or
test changes made.

## Scope

- Commit `a54433b` touches 16 files (964 insertions, 19 deletions):
  `wright-analyzer` (evidence class on `Analysis`/`Finding`/`RuleMeta`,
  `Request::LintRules`, `LintConfig::set_severity_by_name`),
  `wright-driver` (`SessionConfig.lint`, `CompilerSession::lint`,
  `LintResult`, `ToolRequest::Lint`/`LintRules`, `input_identity`),
  `wright-cli` (`lint` command + `--disable-rule`/`--rule-severity` +
  text/JSON rendering), `wright-consumer` (embedding proof), and
  `docs/cli.md`.
- **No `Cargo.toml`/`Cargo.lock` change** (`git show a54433b --stat`): no new
  dependencies. **No plugin host, no #96 work** (zero `plugin`/`#96`
  references in the diff). **No changes to `check`/`analyze`/`inspect`/
  `compile` semantics** — the only shared-code changes are additive: the
  `evidence` field on findings (documented in docs/cli.md) and the
  `LintConfig` field on `SessionConfig` (behind `..Default::default()`).
- Binaries freshly rebuilt from HEAD after `cargo clean -p wright-cli -p
  wright-analyzer` (16:39, after the commit) to rule out stale artifacts.

## AC verdicts

| AC | Verdict | Evidence summary |
| --- | --- | --- |
| AC-1 (`wright lint` on OPY + raw Workshop via shared pipeline) | **PASS** | raw Workshop text → exit 0, 1 finding; OPY via native frontend → exit 0, 1 finding; both through `CompilerSession::lint` (`crates/wright-driver/src/session.rs`) |
| AC-2 (stable IDs, severity, evidence, source identity/spans in text+JSON) | **PASS** | full envelope verified field-by-field; text mode shows `lint:` summary, `severity[code]` + evidence + real `path` span; JSON envelope byte-deterministic across runs |
| AC-3 (programmatic/tool consumers without CLI) | **PASS with nuance** | `CompilerSession::lint()` + `SessionConfig.lint` (driver tests), `ToolRequest::Lint`/`LintRules` (driver service, exercised via `wright-serve` + `wright-consumer`), standalone `wright-tool`; nuance: tool-API spans carry `file` but no `path` (recorded below) |
| AC-4 (config consistency CLI vs API) | **PASS** | same rule set/severity semantics across `--disable-rule`/`--rule-severity`, `LintConfig::disable`/`set_severity`, `LintConfig::set_severity_by_name`; severity override verified on firing findings at both layers |
| AC-5 (check/analyze compatibility) | **PASS** | `check`/`analyze` unchanged (exit 0, findings identical plus additive `evidence`); exit codes and stdout/stderr ownership intact; `analyze` JSON still byte-deterministic |
| AC-6 (no plugin host / #96 required) | **PASS** | no plugin code, no #96 references, no new dependencies in the commit |
| AC-7 (CI green at acceptance) | **PASS** | run 31784510074 at head `a54433b`: all six jobs success, artifacts uploaded; the only run for that commit |

## AC-1 — `wright lint` runs OPY and raw Workshop through the shared pipeline: **PASS**

Fixtures: `compatibility/fixtures/synthetic/control-flow/oracle.json`
(`compile.workshop` text extracted to `/tmp/wright-m12-flow.txt`) and the
same fixture's `source.opy` (native frontend, no Node).

```text
$ ./target/debug/wright lint /tmp/wright-m12-flow.txt          # raw Workshop
lint: 1 finding(s) across 3 rule(s), 0 diagnostic(s)
  warning[min-wait-loop] (evidence: static-indicator): loop body waits at the workshop minimum rate; the loop runs at maximum frequency
      --> /tmp/wright-m12-flow.txt:28:9
$ echo $?   # 0

$ ./target/debug/wright lint compatibility/fixtures/synthetic/control-flow/source.opy
lint: 1 finding(s) across 3 rule(s), 0 diagnostic(s)
  warning[min-wait-loop] (evidence: static-indicator): loop body waits at the workshop minimum rate; the loop runs at maximum frequency
      --> compatibility/fixtures/synthetic/control-flow/source.opy:15:5
$ echo $?   # 0
```

- Both inputs route through the same `CompilerSession::lint()` →
  `load()` → `SemanticService::with_origin_and_config()` path
  (`crates/wright-driver/src/session.rs`); no CLI-specific rule execution.
- OPY path uses the native `wright-opy` frontend (no Node involved); the
  driver's `opy_input_lints_through_the_native_frontend` test covers the
  same path.
- JSON mode also exit 0 for both inputs (see AC-2).

## AC-2 — structured output: stable IDs, severity, evidence, source identity/spans: **PASS**

`wright lint /tmp/wright-m12-flow.txt -f json` (raw Workshop) envelope,
independently parsed:

```json
{
  "wright": { "version": "0.1.0", "contract": "wright-result/v1" },
  "command": "lint", "ok": true, "exit": 0, "diagnostics": [],
  "result": {
    "input_identity": "c384d4a6923afee76b4ea011d35d14e472934180b849c23f55b11fac5e937421",
    "program": { "origin": { "kind": "workshop", "locale": "en-us" }, "rules": 2, "findings": 1, ... },
    "rules": [
      { "id": "min-wait-loop", "defaultSeverity": "warning", "effectiveSeverity": "warning",
        "enabled": true, "summary": "...", "evidence": "static-indicator",
        "tags": ["performance", "stability"], "knownLimits": "..." },
      { "id": "duplicate-condition", ..., "evidence": "exact", ... },
      { "id": "expensive-loop-check", "defaultSeverity": "info", ..., "evidence": "heuristic", ... }
    ],
    "config": { "rules": { "duplicate-condition": {...}, "expensive-loop-check": {...}, "min-wait-loop": {...} } },
    "findings": [
      { "code": "min-wait-loop", "severity": "warning", "evidence": "static-indicator",
        "message": "...", "rule": 1, "action": 7, "value": null,
        "span": { "file": 0, "path": "/tmp/wright-m12-flow.txt",
                  "start": { "line": 28, "col": 9 }, "end": { "line": 31, "col": 13 } } }
    ]
  }
}
```

Verified per field:

- `command: "lint"`, `ok: true`, `exit: 0`, `diagnostics: []`.
- `result.input_identity` is the 64-hex SHA-256 of the input bytes:
  **independently re-derived** via `shasum -a 256` — Workshop text
  `c384d4a6…37d421` (matches the CLI value and oracle.json's
  `workshopSha256`); OPY source `81abea75…afc0150` (matches the CLI value
  and the fixture's `input.sha256`).
- `result.rules`: 3 entries, each with `id`/`defaultSeverity`/
  `effectiveSeverity`/`enabled`/`summary`/`evidence`/`tags`/`knownLimits`.
  Evidence classes: `min-wait-loop` → `static-indicator`,
  `duplicate-condition` → `exact`, `expensive-loop-check` → `heuristic`
  (all four kebab-case classes exist in the enum; `runtime-validated` is
  reserved and unused, per docs).
- `result.config.rules`: 3 entries (one per registered rule), each with
  `enabled`/`severity`, agreeing with the rule metadata.
- `result.findings`: control-flow fires `min-wait-loop` with
  `evidence: "static-indicator"` and a span whose `path` resolves to the
  input display path; span `28:9`–`31:13` maps to the actual
  `While(Compare(Global.index, <, 3));` block in the source (re-checked
  with `sed -n 26,32p`).
- Findings also carry `rule`/`action`/`value` (IR node ids) — pre-existing
  fields from the `getFindings` service response, not documented in the
  docs example but additive, consistent with the documented envelope.

Text mode additionally shows a `lint:` summary line, `severity[code]`
(`warning[min-wait-loop]`), the evidence class, and the resolved path —
all documented fields present.

**Determinism** (contract: byte-deterministic JSON for identical
inputs/config):

```text
$ ./target/debug/wright lint /tmp/wright-m12-flow.txt -f json  > a.json
$ ./target/debug/wright lint /tmp/wright-m12-flow.txt -f json  > b.json
$ cmp a.json b.json && echo BYTE-IDENTICAL        # BYTE-IDENTICAL
$ ./target/debug/wright lint .../source.opy -f json > a2.json
$ ./target/debug/wright lint .../source.opy -f json > b2.json
$ cmp a2.json b2.json && echo OPY BYTE-IDENTICAL  # OPY BYTE-IDENTICAL
```

### Config flags, usage errors, malformed input

- `--disable-rule min-wait-loop` → exit 0, `findings: []`, the rule entry
  reports `"enabled": false`, and `config.rules.min-wait-loop.enabled`
  is `false`.
- `--rule-severity expensive-loop-check:warning` → the rule's
  `effectiveSeverity` changes to `"warning"` (and `config` agrees). The
  **finding-level** assertion was also executed: the analyzer's
  `crates/wright-analyzer/tests/fixtures/expensive-loop.opy` parses through
  the native frontend and fires `expensive-loop-check`
  (`severity: info` by default); with
  `--rule-severity expensive-loop-check:warning` the CLI finding reports
  `severity: warning, evidence: heuristic`. Similarly
  `--rule-severity min-wait-loop:info` on the firing control-flow input
  reports `min-wait-loop | info | static-indicator`.
- Flags are repeatable: `--disable-rule min-wait-loop --disable-rule
  expensive-loop-check` disables both (enabled states observed
  `{min-wait-loop: False, duplicate-condition: True, expensive-loop-check:
  False}`).
- Usage errors: `wright check <file> --disable-rule min-wait-loop` and
  `wright analyze <file> --rule-severity min-wait-loop:info` both exit **2**,
  stdout empty, stderr
  `wright: --disable-rule is only valid for `lint` (not `check`)` /
  `wright: --rule-severity is only valid for `lint` (not `analyze`)`.
  Malformed `--rule-severity` values exit 2:
  `--rule-severity expects <ID>:<SEVERITY> (got 'expensive-loop-check')`
  and `unknown severity 'fatal' (expected warning|info)`.
- Malformed input: `wright lint` on a broken Workshop file exits **1** with
  the same structured diagnostic contract as `check`/`analyze` (all three
  commands produced the identical `unsupported-construct` frontend
  diagnostic envelope, exit 1, `ok: false`). Unknown rule IDs in
  `--disable-rule` are silently accepted per the documented
  `LintConfig` behavior (no error, no effect on registered rules).
- Stdin: `cat flow.txt | wright lint - -f json` → exit 0, 64-hex
  `input_identity`, span `path` resolves to `<stdin>` (the resolved
  display path for stdin input).

## AC-3 — programmatic/tool consumers without the CLI: **PASS (with nuance)**

### Driver session path

`CompilerSession::lint()` returns the same `Envelope<LintResult>` shape as
the CLI (the CLI is a thin argv wrapper over it), and `SessionConfig.lint`
is the shared configuration source. Driver integration tests
`workshop_lint_reports_structured_findings_rules_and_config`,
`lint_respects_configured_rule_disabling`, and
`opy_input_lints_through_the_native_frontend` cover this (26/26 driver
integration tests pass; see below).

### Driver tool service

`ToolRequest::Lint` (driven by `session.config.lint`) returns
`{inputIdentity, rules, config, findings}`; `ToolRequest::LintRules`
returns the rule metadata + config with evidence classes. Verified
end-to-end over the stdio transport:

```text
$ printf '{"op":"lint"}\n{"op":"lintRules"}\n' \
    | ./target/debug/wright-serve --transport stdio /tmp/wright-m12-flow.txt
{"op":"lint"} -> keys [config, findings, inputIdentity, rules]
  inputIdentity: c384d4a6923afee76b4ea011d35d14e472934180b849c23f55b11fac5e937421
  rules count: 3; findings: [min-wait-loop, evidence static-indicator,
  span 28:9-31:13 with file only]   # same semantic finding as the CLI
{"op":"lintRules"} -> rules: 3, config keys [duplicate-condition, expensive-loop-check, min-wait-loop]
```

The `wright-consumer` embedding proof runs the same requests in-process
(`ToolRequest::Lint`, `ToolRequest::LintRules`) and asserts lint findings
carry `evidence`:

```text
$ ./target/debug/wright-consumer compatibility/fixtures/synthetic/control-flow/source.opy
compile: 1123 bytes emitted, sha256 54f07d66f7689244
analyze: 1 findings
lint: 1 finding(s) across 3 rule(s)
service: "wright-tool-service" v"0.1.0", 17 operations
...
consumer: all public-API workflows succeeded   # exit 0
```

### Standalone `wright-tool` (analyzer-level service)

```text
$ printf '{"op":"lintRules"}\n{"op":"getFindings"}\n' \
    | ./target/debug/wright-tool --program adapter/fixtures/synthetic/control-flow.json
lintRules -> { rules: [3 rules with id/defaultSeverity/effectiveSeverity/enabled/
              summary/evidence/tags/knownLimits], config: { rules: {3 entries} } }
getFindings -> [ { code: min-wait-loop, evidence: static-indicator, severity: warning,
              span: { file: 0, start 15:5, end 15:11 } } ]   # file only, no path
```

### Config consistency across CLI and API

Same rule set (3 rules, canonical registry order) and same severity
semantics on every surface: `--disable-rule`/`--rule-severity` (CLI),
`LintConfig::disable`/`set_severity`/`set_severity_by_name` (API), and the
analyzer service tests (`lint_config_is_applied_to_findings_and_rules`
asserts that `config.disable("min-wait-loop")` removes the findings AND
that `lintRules` reports `enabled: false`, while a severity override on
`expensive-loop-check` changes the effective severity AND the firing
finding's `severity`). No duplicated rule-execution path exists: the
analyzer builds each `Analysis` once and derives both findings and rule
metadata (evidence class) from the same instance
(`crates/wright-analyzer/src/registry.rs`).

### Nuance recorded (see Findings below)

Tool-API finding spans carry `file` (0-based index) but **no `path`**; the
driver `ToolRequest::Lint` response carries `inputIdentity` at the response
level. AC-3 is still satisfied: the same semantic findings are produced,
and source identity is available via `inputIdentity` plus `file` indexing
(the analyzer's `getFindings` never carried a path; only the CLI lint
rendering enriches file-0 spans with the resolved display path).

## AC-4 — rule configuration behaves consistently across CLI and API: **PASS**

Covered under AC-2/AC-3: `set_severity_by_name` accepts the CLI spellings
`warning`/`info` and rejects unknown labels (returns `false`, leaving the
config unchanged — asserted in `crates/wright-analyzer/tests/registry.rs`);
`LintConfig` is the single `SessionConfig.lint` field that both the CLI
flags and `CompilerSession::lint`/`ToolRequest::Lint` read. Verified
behaviorally: disabling `min-wait-loop` via the CLI flag and via
`session.config.lint.disable("min-wait-loop")` produce the same result
(no `min-wait-loop` findings, rule reported `enabled: false`); severity
overrides change `effectiveSeverity` AND the firing finding's `severity`
on both surfaces.

## AC-5 — `check`/`analyze` remain compatible: **PASS**

- `wright check /tmp/wright-m12-flow.txt` → exit 0, `check: ok` on stdout,
  the `min-wait-loop` warning rendered to stderr (pre-existing behavior;
  the commit does not touch check rendering).
- `wright analyze /tmp/wright-m12-flow.txt -f json` → exit 0, envelope
  `command: analyze, ok: true, diagnostics: []`, findings identical to
  before except the additive `evidence: "static-indicator"` field
  (documented in docs/cli.md: "This is additive and does not change any
  previously documented field"). `result` keys unchanged
  (`findings`, `program`).
- `wright compile -f json` → still exit 0 with `result.output` (untouched).
- Exit-code and stream-ownership contracts intact: JSON mode success keeps
  stderr empty (0 bytes verified for both `lint` and `analyze`); usage
  errors are the only stderr-without-envelope case (exit 2, verified).
- `wright analyze` JSON is still byte-deterministic (two runs byte-identical).

## AC-6 — no plugin host or #96 implementation required: **PASS**

The commit adds no plugin-loading code, no third-party rule transport, no
#96 references, and no dependency changes (`Cargo.toml`/`Cargo.lock`
untouched). All rule semantics come from the #97 registry.

## AC-7 — CI green at acceptance: **PASS**

See CI evidence section.

## Commands re-run at `a54433b` (all green unless noted)

- `git status` → clean at `a54433b8a6ca6e8508ecd9be02ddb30a827d65d2`;
  `git show a54433b --stat` → 16 files, 964 insertions, 19 deletions;
  no Cargo changes.
- `cargo clean -p wright-cli -p wright-analyzer` then
  `cargo build -p wright-cli -p wright-analyzer --bin wright --bin wright-tool`
  → fresh binaries at 16:39.
- `./target/debug/wright lint <flow.txt>` (text) → exit 0; `-f json` →
  exit 0, full envelope verified; byte-determinism: `cmp` byte-identical on
  two runs each for Workshop and OPY inputs.
- `./target/debug/wright lint <source.opy>` (text and JSON) → exit 0,
  native frontend.
- `--disable-rule`, `--rule-severity` (both rule metadata and firing-
  finding severity), repeatable flags, unknown-rule-ID, stdin, `--help`
  → all as documented.
- `wright check`/`analyze` with lint-only flags → exit 2, empty stdout,
  stderr message; `--rule-severity` without `:` or with `fatal` → exit 2.
- Malformed input: `wright lint|check|analyze` → exit 1, identical
  `unsupported-construct` diagnostic envelope.
- `printf '{"op":"lint"}\n{"op":"lintRules"}\n' | ./target/debug/wright-serve
  --transport stdio <flow.txt>` → `{inputIdentity, rules, config, findings}`
  shape verified.
- `printf '{"op":"lintRules"}\n{"op":"getFindings"}\n' |
  ./target/debug/wright-tool --program adapter/fixtures/synthetic/control-flow.json`
  → 3 rules with evidence classes, config summary, evidence-tagged findings.
- `./target/debug/wright-consumer compatibility/fixtures/synthetic/control-flow/source.opy`
  → all public-API workflows succeeded (lint path + tool lint path asserted).
- `cargo test -p wright-analyzer -p wright-driver -p wright-cli -p
  wright-consumer` (stable 1.94.0) → all suites `ok`, 0 failures
  (analyzer 55: lib 7 + analysis 7 + cfg 7 + registry 19 + semantic_index 6
  + service 11 + workshop_integration 5; cli 25: cli 20 + serve 5;
  consumer 2; driver 39: lib 6 + driver 26 + service 7; plus 0-test bins).
- `cargo test --workspace --all-targets --all-features` (stable 1.94.0) →
  **367 tests across 47 suites, 0 failures, 0 ignored** (breakdown:
  analyzer 55, cli 25, consumer 2, core 38, driver 39, ir 18, language 55,
  lsp 22, opy 39, transform 6, workshop 68).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` →
  finished, **0 warnings**.
- `cargo fmt --all -- --check` → clean.
- `cargo +1.85.0 test -p wright-analyzer -p wright-driver -p wright-cli -p
  wright-consumer --all-features` (MSRV 1.85.0) → **121 tests, 0 failures**
  (analyzer 55, cli 25, consumer 2, driver 39).
- `python3 -m unittest discover -s compatibility/tests` → 10/10 OK.
- `shasum -a 256` of both inputs → matches the CLI-reported
  `input_identity` values and the fixture-recorded hashes.
- Note: the OverPy oracle (21/21) and adapter (23/23) suites require the
  pinned pnpm/Node environment; they were not re-run locally. They are
  covered by the green CI jobs below (their scope is unaffected by this
  commit — no frontend/emitter/oracle changes).

## CI evidence (GitHub Actions)

- `gh run view 31784510074 --json headSha,conclusion,event,status` →
  `{"conclusion":"success","event":"push","headBranch":"main",
  "headSha":"a54433b8a6ca6e8508ecd9be02ddb30a827d65d2","status":"completed"}`.
- `gh run list --commit a54433b` → exactly one run (31784510074), success,
  display title "feat(cli): add first-class lint command with shared
  structured result…".
- All six jobs **success** (no skips): Rust quality (stable) 94717327498,
  Rust quality (1.85.0) 94717327536, v1 release gates 94717327549, OverPy
  compatibility oracle 94717327528, OverPy-to-HIR adapter 94717327538,
  Native-vs-reference frontend differential 94717327487.
- Artifacts uploaded: `v1-gate-reports` (v1-gates-report.json,
  scenarios-report.json, wright-bench-report.json) and
  `wright-differential-report`.
- The two Rust-quality jobs run exactly what was re-run locally: `cargo fmt
  --all -- --check`, `cargo clippy --workspace --all-targets --all-features
  -- -D warnings`, `cargo test --workspace --all-targets --all-features`
  on stable and 1.85.0 respectively.

## Findings and nuances

1. **Tool-API spans carry `file` but no `path`; CLI lint spans carry
   `path` (doc-level nuance, low severity, no action needed).** The driver
   `ToolRequest::Lint` response and the standalone `wright-tool`
   `getFindings` carry `span.file` only; the CLI lint path enriches
   file-0 spans with the resolved display `path`
   (`enrich_finding_spans`, session.rs). Source identity remains available
   to tool consumers via `inputIdentity` (driver `lint` response) plus the
   `file` index; AC-3 is satisfied. docs/cli.md documents both spellings
   and the tool API's `inputIdentity`. No contract violation.

2. **`input_identity` (CLI envelope, snake_case, matching `CompileResult`)
   vs `inputIdentity` (tool API, camelCase) — record-only.** Both carry
   the identical SHA-256 value (verified). docs/cli.md documents the
   mapping explicitly. Not a contract concern; noted for completeness.

3. **`config.rules` key order is a sorted serde map, not registry order
   (record-only).** The JSON object keys serialize in lexicographic order
   (`duplicate-condition`, `expensive-loop-check`, `min-wait-loop`) while
   the `rules` array keeps registry order. Output is deterministic and
   byte-stable (verified), so the determinism contract holds.

4. **Lint findings carry undocumented `rule`/`action`/`value` IR node ids
   (record-only).** These pre-date #98 (the `getFindings` service response
   shape) and are additive fields; the docs example shows a subset with
   "…". No field documented in docs/cli.md is missing.

5. **Stdin lint span `path` resolves to `<stdin>` (record-only).** For
   stdin input the resolved display path is `<stdin>`; the original-source
   identity/spans remain available. Reasonable presentation for a nameless
   stream.

6. **MSRV subset run** covered the four affected crates (121 tests,
   0 failures) rather than the full workspace on 1.85.0; the CI "Rust
   quality (1.85.0)" job ran the full workspace on 1.85.0 and is green.

No class-3 (product-correctness) findings were surfaced by this
verification. No implementation defects to route to Engineer.

## Verdict

**VERIFIED.** All seven acceptance criteria of #98 are supported by
independently re-derived evidence at `a54433b` (working tree clean; fresh
binaries; 367/367 workspace tests on stable, 121/121 on MSRV 1.85.0;
clippy 0 warnings; fmt clean; compatibility python suite 10/10; CI run
31784510074 all six jobs green at head `a54433b`). `wright lint` runs on
raw Workshop and native OPY through the shared session pipeline; text and
JSON output expose stable rule IDs, configured severity, evidence classes,
and source identity/spans; tool/API consumers reach the same findings
without the CLI; #97 configuration behaves identically across CLI and API;
`check`/`analyze`/`compile`/`inspect` remain compatible (additive
`evidence` field only, documented); no plugin host or #96 work was added.
The recorded nuances (tool-API span `path` absence, `input_identity` vs
`inputIdentity` spelling, sorted `config.rules` keys) are informational
and do not affect acceptance.

Next authoritative role: **PM** — #98 may proceed to acceptance per AC-1..AC-7.
