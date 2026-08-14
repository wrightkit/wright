//! Reusable driver tests (#37/#41): the compiler/session driver serves every
//! core workflow over both Workshop and protocol inputs without the CLI, and
//! CLI and library consumers share this single orchestration path.

use std::path::{Path, PathBuf};

use wright_driver::CompilerSession;
use wright_driver::config::{InputSpec, SessionConfig, SourceKind};
use wright_driver::result::exit;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn fixture(fixture_id: &str) -> PathBuf {
    workspace_root()
        .join("compatibility/fixtures")
        .join(fixture_id)
        .join("oracle.json")
}

fn adapter_fixture(fixture_id: &str) -> PathBuf {
    workspace_root()
        .join("adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn corpus_workshop_text(fixture_id: &str) -> String {
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(fixture(fixture_id)).unwrap(),
    )
    .unwrap();
    oracle["compile"]["workshop"].as_str().unwrap().to_string()
}

fn corpus_source_opy(fixture_id: &str) -> String {
    std::fs::read_to_string(
        workspace_root()
            .join("compatibility/fixtures")
            .join(fixture_id)
            .join("source.opy"),
    )
    .unwrap()
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-file-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-driver-dir-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The path of `path` expressed relative to the process cwd, so a test can
/// address the same file through a relative spelling (as a shell would) and
/// prove input-spelling independence without changing the cwd.
fn cwd_relative(path: &Path) -> PathBuf {
    let cwd = std::env::current_dir().expect("cwd is readable");
    let mut path_parts = path.components().peekable();
    let mut cwd_parts = cwd.components().peekable();
    while let (Some(path_component), Some(cwd_component)) = (path_parts.peek(), cwd_parts.peek()) {
        if path_component == cwd_component {
            path_parts.next();
            cwd_parts.next();
        } else {
            break;
        }
    }
    let mut out = PathBuf::new();
    for _ in cwd_parts {
        out.push("..");
    }
    for component in path_parts {
        out.push(component);
    }
    out
}

fn workshop_session(text: &str) -> CompilerSession {
    let path = temp_file("program.txt", text);
    CompilerSession::new(SessionConfig::from_path(path)).unwrap()
}

#[test]
fn workshop_compile_emits_corpus_text() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let mut session = workshop_session(&text);
    let envelope = session.compile();
    assert!(
        envelope.ok,
        "compile must succeed: {:?}",
        envelope.diagnostics
    );
    let output = envelope.result.output.expect("compiled output");
    assert_eq!(output.locale, "en-us");
    assert_eq!(output.input_identity.len(), 64);
    assert_eq!(output.sha256.len(), 64);
    assert!(
        output.text.contains("Disable Inspector Recording"),
        "emitted text: {}",
        output.text
    );
    // Compilation to a file path uses the same driver path as stdout.
    let out_path = temp_file("out.txt", "");
    let mut session = workshop_session(&text);
    session.config.output = Some(out_path.clone());
    let envelope = session.compile();
    assert!(envelope.ok);
    let stored = std::fs::read_to_string(&out_path).unwrap();
    assert_eq!(stored, output.text);
    let _ = std::fs::remove_dir_all(out_path.parent().unwrap());
}

#[test]
fn workshop_check_surfaces_analysis_findings_as_diagnostics() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let mut session = workshop_session(&text);
    let envelope = session.check();
    assert!(envelope.ok, "check passes with warnings only");
    assert!(
        envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "min-wait-loop"),
        "check must attach analysis findings: {:?}",
        envelope.diagnostics
    );
    assert_eq!(envelope.exit, exit::SUCCESS);
}

#[test]
fn workshop_analyze_reports_program_and_findings() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let mut session = workshop_session(&text);
    let envelope = session.analyze();
    assert!(envelope.ok);
    assert_eq!(envelope.result.program["origin"]["kind"], "workshop");
    assert_eq!(envelope.result.program["origin"]["locale"], "en-us");
    assert_eq!(envelope.result.program["rules"], 2);
    let findings = envelope.result.findings.as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "findings: {findings:?}"
    );
}

// ── Lint (M12, #98) ──────────────────────────────────────────────────────────

#[test]
fn workshop_lint_reports_structured_findings_rules_and_config() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let path = temp_file("flow.txt", &text);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let envelope = session.lint();
    assert!(envelope.ok, "lint must succeed: {:?}", envelope.diagnostics);
    assert_eq!(
        envelope.result.input_identity.len(),
        64,
        "input identity is the SHA-256 hex digest"
    );
    assert_eq!(envelope.result.program["rules"], 2);
    let rules = envelope.result.rules.as_array().unwrap();
    assert_eq!(rules.len(), 5, "all five M12 rules are reported");
    let config_rules = envelope.result.config["rules"].as_object().unwrap();
    assert_eq!(
        config_rules.len(),
        5,
        "the config summary covers every rule"
    );
    let findings = envelope.result.findings.as_array().unwrap();
    let min_wait = findings
        .iter()
        .find(|finding| finding["code"] == "min-wait-loop")
        .expect("control-flow fires min-wait-loop");
    assert_eq!(
        min_wait["evidence"], "static-indicator",
        "findings carry the rule's evidence class"
    );
    let span = min_wait["span"].as_object().expect("findings carry spans");
    assert_eq!(
        span["path"], "flow.txt",
        "file-0 spans resolve root-relative to the include root, not the display path"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn analyze_findings_carry_the_same_span_path_as_lint() {
    // The issue's core acceptance (#102): for the same source location,
    // `analyze` and `lint` findings carry the identical `span.path`.
    let text = corpus_workshop_text("synthetic/control-flow");
    let path = temp_file("flow.txt", &text);
    let mut analyze_session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let analyze = analyze_session.analyze();
    assert!(analyze.ok, "analyze: {:?}", analyze.diagnostics);
    let mut lint_session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let lint = lint_session.lint();
    assert!(lint.ok, "lint: {:?}", lint.diagnostics);
    let analyze_findings = analyze.result.findings.as_array().unwrap();
    let lint_findings = lint.result.findings.as_array().unwrap();
    assert!(
        !analyze_findings.is_empty(),
        "control-flow produces findings"
    );
    assert_eq!(analyze_findings.len(), lint_findings.len());
    for (analyzed, linted) in analyze_findings.iter().zip(lint_findings) {
        assert_eq!(
            analyzed["span"]["path"], linted["span"]["path"],
            "analyze and lint must report the same span.path for the same finding"
        );
    }
    assert_eq!(
        analyze_findings[0]["span"]["path"], "flow.txt",
        "the shared path is the root-relative file name"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn span_path_is_consistent_across_input_spellings() {
    // The issue's inconsistency (#102): an absolute input collapsed to the
    // basename while a relative input kept the path as given. Both spellings
    // of the same file must resolve to the same root-relative span.path, and
    // `analyze` must report the same value as `lint` (the issue's `rule
    // "loop"` repro fires a min-wait-loop finding).
    let dir = temp_dir();
    let loop_source =
        "rule \"loop\":\n    @Event eachPlayer\n    while (true):\n        wait(0.016)\n";
    std::fs::write(dir.join("loop.opy"), loop_source).unwrap();
    let absolute = dir.join("loop.opy");
    let relative = cwd_relative(&absolute);

    let mut abs_session = CompilerSession::new(SessionConfig::from_path(absolute.clone())).unwrap();
    let abs_lint = abs_session.lint();
    assert!(abs_lint.ok, "absolute lint: {:?}", abs_lint.diagnostics);
    let mut rel_session = CompilerSession::new(SessionConfig::from_path(relative.clone())).unwrap();
    let rel_lint = rel_session.lint();
    assert!(rel_lint.ok, "relative lint: {:?}", rel_lint.diagnostics);
    let mut analyze_session = CompilerSession::new(SessionConfig::from_path(absolute)).unwrap();
    let analyze = analyze_session.analyze();
    assert!(analyze.ok, "analyze: {:?}", analyze.diagnostics);

    let abs_findings = abs_lint.result.findings.as_array().unwrap();
    let rel_findings = rel_lint.result.findings.as_array().unwrap();
    let analyze_findings = analyze.result.findings.as_array().unwrap();
    assert!(!abs_findings.is_empty(), "loop.opy fires min-wait-loop");
    assert_eq!(abs_findings.len(), rel_findings.len());
    assert_eq!(abs_findings.len(), analyze_findings.len());
    for (a, (b, c)) in abs_findings
        .iter()
        .zip(rel_findings.iter().zip(analyze_findings))
    {
        assert_eq!(
            a["span"]["path"], "loop.opy",
            "the absolute spelling resolves to the root-relative basename"
        );
        assert_eq!(
            a["span"]["path"], b["span"]["path"],
            "absolute and relative input spellings must agree"
        );
        assert_eq!(
            a["span"]["path"], c["span"]["path"],
            "lint and analyze must agree on the repro shape"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_respects_configured_rule_disabling() {
    let text = corpus_workshop_text("synthetic/control-flow");
    let path = temp_file("flow.txt", &text);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    session.config.lint.disable("min-wait-loop");
    let envelope = session.lint();
    assert!(envelope.ok);
    let findings = envelope.result.findings.as_array().unwrap();
    assert!(
        findings
            .iter()
            .all(|finding| finding["code"] != "min-wait-loop"),
        "the disabled rule must produce no findings"
    );
    let rules = envelope.result.rules.as_array().unwrap();
    let min_wait = rules
        .iter()
        .find(|rule| rule["id"] == "min-wait-loop")
        .expect("the disabled rule is still reported in the rules summary");
    assert_eq!(min_wait["enabled"], false);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_input_lints_through_the_native_frontend() {
    // Supported OPY input lints through the shared native frontend path.
    let source = corpus_source_opy("synthetic/control-flow");
    let path = temp_file("control-flow.opy", &source);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let envelope = session.lint();
    assert!(
        envelope.ok,
        "opy lint must succeed: {:?}",
        envelope.diagnostics
    );
    let findings = envelope.result.findings.as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|finding| finding["code"] == "min-wait-loop"),
        "opy control-flow fires min-wait-loop"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn workshop_inspect_returns_structural_model() {
    let text = corpus_workshop_text("synthetic/declarations-rules");
    let mut session = workshop_session(&text);
    let envelope = session.inspect();
    assert!(envelope.ok);
    let rules = envelope.result.rules.as_array().unwrap();
    assert!(!rules.is_empty(), "inspect lists rules");
    let symbols = envelope.result.symbols.as_array().unwrap();
    assert!(!symbols.is_empty(), "inspect lists symbols");
    let references = envelope.result.references.as_array().unwrap();
    assert_eq!(references.len(), symbols.len(), "references per symbol");
}

#[test]
fn protocol_input_runs_all_workflows() {
    for fixture_id in [
        "synthetic/basic-rule",
        "synthetic/control-flow",
        "synthetic/declarations-rules",
    ] {
        let path = adapter_fixture(fixture_id);
        let mut session = CompilerSession::new(SessionConfig::from_path(path)).unwrap();
        assert!(session.check().ok, "{fixture_id} check");
        assert!(session.analyze().ok, "{fixture_id} analyze");
        let inspect = session.inspect();
        assert!(inspect.ok, "{fixture_id} inspect");
        assert_eq!(inspect.result.program["origin"]["kind"], "protocol");
    }
}

#[test]
fn opy_input_compiles_through_the_native_frontend() {
    let source = corpus_source_opy("synthetic/basic-rule");
    let path = temp_file("basic-rule.opy", &source);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let envelope = session.compile();
    assert!(envelope.ok, "opy compile: {:?}", envelope.diagnostics);
    let output = envelope.result.output.expect("output");
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(fixture("synthetic/basic-rule")).unwrap(),
    )
    .unwrap();
    let expected = oracle["compile"]["workshop"].as_str().unwrap();
    assert_eq!(
        output.text.trim(),
        expected.trim(),
        "the native .opy path must reproduce the oracle Workshop text"
    );
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn malformed_workshop_input_fails_structurally() {
    // Enough locale evidence to pass detection, then a syntax error.
    let mut session =
        workshop_session("rule (\"broken\") { event { Ongoing - Global; } actions { If(True); }");
    let envelope = session.check();
    assert!(!envelope.ok);
    assert_eq!(envelope.exit, exit::SOURCE_ERROR);
    let diagnostic = &envelope.diagnostics[0];
    assert!(diagnostic.span.is_some(), "parse errors carry spans");
    assert_eq!(diagnostic.source.as_ref().unwrap().kind, "workshop");
}

#[test]
fn explicit_locale_override_wins() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let path = temp_file("program.txt", &text);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    session.config.locale = Some("en-US".to_string());
    let envelope = session.check();
    assert!(envelope.ok, "explicit locale: {:?}", envelope.diagnostics);
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn stdin_opy_is_a_supported_kind() {
    // `.opy` on stdin is no longer rejected: the native frontend owns it.
    // (The driver reads the process stdin at load time; an empty stdin fails
    // with `stdin-empty`, proving the kind check no longer blocks `.opy`.)
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Stdin,
        kind: SourceKind::Opy,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.check();
    assert!(
        !envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "stdin-opy-unsupported"),
        "stdin .opy is supported by the native frontend"
    );
}

#[test]
fn loading_is_idempotent() {
    let text = corpus_workshop_text("synthetic/basic-rule");
    let mut session = workshop_session(&text);
    let first = session.load().unwrap();
    let identity = first.input.identity.clone();
    let second = session.load().unwrap();
    assert_eq!(second.input.identity, identity);
    assert_eq!(second.program.rules.len(), 1);
}

#[test]
fn compiled_output_is_deterministic() {
    let text = corpus_workshop_text("synthetic/declarations-rules");
    let mut first = workshop_session(&text);
    let mut second = workshop_session(&text);
    let a = first.compile().result.output.unwrap();
    let b = second.compile().result.output.unwrap();
    assert_eq!(a.text, b.text);
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(a.input_identity, b.input_identity);
}

#[test]
fn opy_lex_error_in_included_file_names_the_included_file() {
    // A lex error inside an included file must be reported under that file's
    // path (and position), not the root file's path (#83).
    let dir = temp_dir();
    std::fs::write(
        dir.join("shared.opy"),
        "rule \"shared\":\n\\\n    disableInspector()\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("main.opy"),
        "#!include \"shared.opy\"\nrule \"main\":\n    disableInspector()\n",
    )
    .unwrap();
    let mut session = CompilerSession::new(SessionConfig::from_path(dir.join("main.opy"))).unwrap();
    let envelope = session.compile();
    assert!(!envelope.ok, "the included lex error must fail the compile");
    let diagnostic = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "lex-error")
        .expect("a lex-error diagnostic is reported");
    let span = diagnostic.span.as_ref().expect("lex errors carry spans");
    assert_eq!(
        span.path, "shared.opy",
        "the diagnostic must name the included file"
    );
    assert_eq!(span.start.line, 2);
    assert_eq!(span.start.col, 1);
    let shared = std::fs::read_to_string(dir.join("shared.opy")).unwrap();
    let line = shared
        .lines()
        .nth(span.start.line as usize - 1)
        .unwrap_or("");
    assert!(
        (span.start.col as usize) <= line.len() + 1,
        "the reported position ({}:{}) must exist in shared.opy",
        span.start.line,
        span.start.col
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opy_include_diagnostics_resolve_through_the_registry() {
    // Include directives keep resolving their span path through the registry
    // after the identity fix (#83): the main file keeps its display path,
    // while a directive inside an included file names that file.
    let dir = temp_dir();
    let main_path = dir.join("main.opy");
    std::fs::write(&main_path, "#!include \"missing.opy\"\n").unwrap();
    let mut session = CompilerSession::new(SessionConfig::from_path(main_path.clone())).unwrap();
    let envelope = session.check();
    assert!(!envelope.ok);
    let not_found = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "include-not-found")
        .expect("include-not-found is reported");
    assert_eq!(
        not_found.span.as_ref().unwrap().path,
        main_path.display().to_string(),
        "include-not-found from the main file keeps the main path"
    );

    std::fs::write(&main_path, "#!include \"main.opy\"\n").unwrap();
    let mut session = CompilerSession::new(SessionConfig::from_path(main_path.clone())).unwrap();
    let envelope = session.check();
    assert!(!envelope.ok);
    let cycle = envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "include-cycle")
        .expect("include-cycle is reported");
    assert_eq!(
        cycle.span.as_ref().unwrap().path,
        "main.opy",
        "the cycle-closing directive lives in the included copy of main.opy"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opy_unknown_settings_key_fails_check_and_compile_identically() {
    // A settings key outside the emission table must fail both `check` and
    // `compile` with the same settings-unknown-key code and span (#86).
    let dir = temp_dir();
    std::fs::write(
        dir.join("main.opy"),
        "settings {\n    \"gamemodes\": {\n        \"general\": {\n            \"scoreToWin\": 3\n        }\n    }\n}\nrule \"r\":\n    pass\n",
    )
    .unwrap();
    let main_path = dir.join("main.opy");
    let mut check_session =
        CompilerSession::new(SessionConfig::from_path(main_path.clone())).unwrap();
    let check = check_session.check();
    assert!(!check.ok);
    let mut compile_session = CompilerSession::new(SessionConfig::from_path(main_path)).unwrap();
    let compile = compile_session.compile();
    assert!(!compile.ok);
    let check_diag = check
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "settings-unknown-key")
        .expect("check reports settings-unknown-key");
    let compile_diag = compile
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "settings-unknown-key")
        .expect("compile reports settings-unknown-key");
    assert_eq!(
        check_diag.span, compile_diag.span,
        "check and compile must report the same settings-unknown-key span"
    );
    assert_eq!(
        check_diag.span.as_ref().unwrap().start.line,
        4,
        "the span points at the offending key"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opy_string_array_initializer_emits_custom_string_elements() {
    // Class-3 remediation (#87): string values in value positions render as
    // `Custom String("...")`, the pinned oracle's spelling. The minimal
    // repro with a real action body must be byte-identical to the pinned
    // oracle artifact (amended AC-1).
    assert_byte_artifact(
        "globalvar x = [\"a\", \"b\"]\n\nrule \"r\":\n    @Event global\n    disableInspector()\n",
        ORACLE_AC1,
    );
}

#[test]
fn opy_long_string_initializers_split_like_the_oracle() {
    // Amended AC-3: 300 decoded chars split 125+{0}, 125+{0}, 50; 1000 chars
    // split into 8 segments — byte-equal to the pinned oracle artifacts.
    assert_byte_artifact(
        &format!(
            "globalvar x = \"{}\"\n\nrule \"r\":\n    @Event global\n    disableInspector()\n",
            "A".repeat(300)
        ),
        ORACLE_AC3_300,
    );
    assert_byte_artifact(
        &format!(
            "globalvar x = \"{}\"\n\nrule \"r\":\n    @Event global\n    disableInspector()\n",
            "A".repeat(1000)
        ),
        ORACLE_AC3_1000,
    );
}

#[test]
fn opy_escaped_value_strings_round_trip_the_oracle_spelling() {
    // Amended AC-4: a decoded newline re-escapes to the literal two-character
    // `\n` (0x5C 0x6E), byte-equal to the pinned oracle artifact.
    assert_byte_artifact(
        "globalvar x = \"a\\nb\"\n\nrule \"r\":\n    @Event global\n    disableInspector()\n",
        ORACLE_AC4,
    );
}

#[test]
fn opy_playervar_augmented_assignments_match_the_oracle_artifacts() {
    // Amended AC-18: playervar augmented assignments lower to
    // `Modify Player Variable(Event Player, p, <op>, 2)` for the oracle's
    // evidenced operator set (+= -= *= /= %=), byte-equal to the pinned
    // oracle artifacts. `//=` is a parse error in both frontends (not an
    // OverPy operator).
    for (op, artifact) in [
        ("+=", ORACLE_PV_ADD),
        ("-=", ORACLE_PV_SUB),
        ("*=", ORACLE_PV_MUL),
        ("/=", ORACLE_PV_DIV),
        ("%=", ORACLE_PV_MOD),
    ] {
        assert_byte_artifact(
            &format!(
                "playervar p\n\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.p {op} 2\n"
            ),
            artifact,
        );
    }
    let dir = temp_dir();
    let main = dir.join("fdiv.opy");
    std::fs::write(
        &main,
        "playervar p\n\nrule \"r\":\n    @Event eachPlayer\n    eventPlayer.p //= 2\n",
    )
    .unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(main),
        root: Some(dir.clone()),
        profile: wright_transform::Profile::Compat,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.compile();
    assert!(
        !envelope.ok,
        "//= is rejected like the pinned oracle (not an OverPy operator)"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opy_numeric_initializers_match_the_oracle_artifact() {
    // Amended AC-11: non-zero and non-integer numeric initializers are
    // preserved (`j = 5`, `k = 0.0` with the source spelling, `playervar
    // p = 7` in the player Initialize rule); integer-`0` (`h = 0`) is
    // dropped. Byte-equal to the pinned oracle artifact.
    assert_byte_artifact(
        "globalvar j = 5\nglobalvar h = 0\nglobalvar k = 0.0\nplayervar p = 7\n\nrule \"r\":\n    @Event global\n    disableInspector()\n",
        ORACLE_AC11,
    );
}

#[test]
fn opy_empty_rules_are_dropped_like_the_oracle() {
    // Amended AC-5: pass-only and condition-without-actions rules emit
    // nothing, byte-equal to the pinned oracle artifacts.
    assert_byte_artifact("rule \"r\":\n    @Event global\n    pass\n", "");
    assert_byte_artifact(
        "globalvar q\n\nrule \"r\":\n    @Event global\n    @Condition q == 1\n",
        ORACLE_AC5_COND,
    );
}

/// Compile `.opy` source with the compat profile and assert the emitted
/// artifact is byte-identical to the quoted pinned-oracle artifact.
fn assert_byte_artifact(source: &str, artifact: &str) {
    let dir = temp_dir();
    let main = dir.join("repro.opy");
    std::fs::write(&main, source).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(main),
        root: Some(dir.clone()),
        profile: wright_transform::Profile::Compat,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.compile();
    assert!(
        envelope.ok,
        "repro must compile: {:?}",
        envelope.diagnostics
    );
    let text = envelope.result.output.expect("output").text;
    assert_eq!(
        text, artifact,
        "artifact must be byte-identical to the oracle"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// Byte-quoted pinned-oracle artifacts (overpy 9.7.10, raw CLI output).
const ORACLE_AC1: &str = "variables {\n    global:\n        0: x\n}\n\nrule (\"Initialize global variables\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(x, Array(Custom String(\"a\"), Custom String(\"b\")));\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";

const ORACLE_AC3_300: &str = "variables {\n    global:\n        0: x\n}\n\nrule (\"Initialize global variables\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(x, Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\"))));\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";

const ORACLE_AC3_1000: &str = "variables {\n    global:\n        0: x\n}\n\nrule (\"Initialize global variables\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(x, Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA{0}\", Custom String(\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\")))))))));\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";

const ORACLE_AC4: &str = "variables {\n    global:\n        0: x\n}\n\nrule (\"Initialize global variables\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(x, Custom String(\"a\\nb\"));\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";

const ORACLE_AC5_COND: &str = "variables {\n    global:\n        0: q\n}\n\n";

const ORACLE_AC11: &str = "variables {\n    global:\n        0: j\n        1: h\n        2: k\n    player:\n        0: p\n}\n\nrule (\"Initialize global variables\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Set Global Variable(j, 5);\n        Set Global Variable(k, 0.0);\n    }\n}\n\nrule (\"Initialize player variables\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Set Player Variable(Event Player, p, 7);\n    }\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Global;\n    }\n    actions {\n        Disable Inspector Recording;\n    }\n}\n\n";

// Pinned oracle artifacts for playervar augmented assignments (AC-18).
const ORACLE_PV_ADD: &str = "variables {\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Modify Player Variable(Event Player, p, Add, 2);\n    }\n}\n\n";

const ORACLE_PV_SUB: &str = "variables {\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Modify Player Variable(Event Player, p, Subtract, 2);\n    }\n}\n\n";

const ORACLE_PV_MUL: &str = "variables {\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Modify Player Variable(Event Player, p, Multiply, 2);\n    }\n}\n\n";

const ORACLE_PV_DIV: &str = "variables {\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Modify Player Variable(Event Player, p, Divide, 2);\n    }\n}\n\n";

const ORACLE_PV_MOD: &str = "variables {\n    player:\n        0: p\n}\n\nrule (\"r\") {\n    event {\n        Ongoing - Each Player;\n        All;\n        All;\n    }\n    actions {\n        Modify Player Variable(Event Player, p, Modulo, 2);\n    }\n}\n\n";

#[test]
fn opy_pixelart_array_strings_match_the_oracle_wrapping() {
    // Class-3 remediation (#87): pixelart's string-array initializer renders
    // with Custom String-wrapped elements like the oracle (the residual
    // divergence, if any, is the reference's long-string splitting).
    let root = workspace_root().join("compatibility/fixtures/real-world/overpy-pixelart");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(root.join("pixelart.opy")),
        root: Some(root.clone()),
        profile: wright_transform::Profile::Compat,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.compile();
    assert!(
        envelope.ok,
        "pixelart must compile: {:?}",
        envelope.diagnostics
    );
    let text = envelope.result.output.expect("output").text;
    assert!(
        text.contains("Set Global Variable(owo, Array(Custom String(\" \u{2001}"),
        "the first string element must wrap like the oracle:\n{text}"
    );
}

#[test]
fn opy_inputhud_settings_section_matches_the_oracle() {
    // inputhud's rules do not compile natively (later expression surface),
    // so the settings section is exercised with the source's real settings
    // block (byte-identical) and a minimal rule body through the full
    // production path. The description's decoded `\n` must round-trip to the
    // oracle's literal two-character spelling (#86).
    let root = workspace_root().join("compatibility/fixtures/real-world/overpy-inputhud");
    let source = std::fs::read_to_string(root.join("inputhud.opy")).unwrap();
    let block_start = source.find("settings {").expect("settings block");
    let block_end = source[block_start..]
        .find("\n}\n")
        .map(|index| block_start + index + 3)
        .expect("settings block close");
    let settings_only = format!(
        "{}\nrule \"r\":\n    pass\n",
        &source[block_start..block_end]
    );
    let dir = temp_dir();
    let main = dir.join("inputhud-settings.opy");
    std::fs::write(&main, &settings_only).unwrap();
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(main),
        root: Some(root.clone()),
        profile: wright_transform::Profile::Compat,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.compile();
    assert!(
        envelope.ok,
        "settings-only inputhud must compile: {:?}",
        envelope.diagnostics
    );
    let text = envelope.result.output.expect("output").text;
    assert!(
        text.contains("Description: \"Keyboard/Controller detector by Zezombye.\\n\\n"),
        "decoded newlines must render as the literal two-character \\n"
    );
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(root.join("oracle.json")).unwrap(),
    )
    .unwrap();
    let oracle_text = oracle["compile"]["workshop"].as_str().unwrap();
    assert_eq!(
        collapse_whitespace(&settings_section(&text)),
        collapse_whitespace(&settings_section(oracle_text)),
        "the emitted inputhud settings section must match the oracle region"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn opy_pixelart_compiles_and_matches_the_oracle_settings_section() {
    // End-to-end: the pixelart fixture compiles with the compat profile and
    // its emitted settings section equals the oracle region
    // (whitespace-collapsed, #86).
    let root = workspace_root().join("compatibility/fixtures/real-world/overpy-pixelart");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(root.join("pixelart.opy")),
        root: Some(root.clone()),
        profile: wright_transform::Profile::Compat,
        ..SessionConfig::default()
    })
    .unwrap();
    let envelope = session.compile();
    assert!(
        envelope.ok,
        "pixelart must compile: {:?}",
        envelope.diagnostics
    );
    let text = envelope.result.output.expect("output").text;
    let oracle = serde_json::from_str::<serde_json::Value>(
        &std::fs::read_to_string(root.join("oracle.json")).unwrap(),
    )
    .unwrap();
    let oracle_text = oracle["compile"]["workshop"].as_str().unwrap();
    assert_eq!(
        collapse_whitespace(&settings_section(&text)),
        collapse_whitespace(&settings_section(oracle_text)),
        "the emitted settings section must match the oracle region"
    );
}

/// The leading `settings` section of a workshop text.
fn settings_section(text: &str) -> String {
    let start = text.find("settings").expect("text has a settings section");
    let mut depth = 0usize;
    let mut end = start;
    for (index, ch) in text[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = start + index + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    text[start..end].to_string()
}

fn collapse_whitespace(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}
