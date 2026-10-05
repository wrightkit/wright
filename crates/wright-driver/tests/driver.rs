//! Driver contract tests after the OPY provider cutover.
//!
//! Raw Workshop remains an in-process product path. OPY source behavior is
//! exercised at the provider boundary; these tests ensure a missing provider
//! cannot silently select a removed static frontend.

use std::path::{Path, PathBuf};

use wright_driver::edit::{EditTransaction, RenameTarget};
use wright_driver::service::{Address, ToolService};
use wright_driver::{
    CompilerSession, Diagnostic, InputSpec, SessionConfig, SourceBackend, SourceKind,
};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn legacy_protocol() -> &'static str {
    r#"{"protocol":{"name":"wright/opy-hir","version":"1.1.0"}}"#
}

fn workshop_fixture(id: &str) -> PathBuf {
    workspace_root()
        .join("tests/fixtures/workshop")
        .join(id)
        .with_extension("ws")
}

#[test]
fn workshop_runs_all_product_workflows() {
    let path = workshop_fixture("synthetic/control-flow");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");

    assert!(session.check().ok, "check: {:?}", session.diagnostics());
    assert!(session.compile().ok, "compile: {:?}", session.diagnostics());
    assert!(session.analyze().ok, "analyze: {:?}", session.diagnostics());
    assert!(session.lint().ok, "lint: {:?}", session.diagnostics());
    assert!(session.inspect().ok, "inspect: {:?}", session.diagnostics());
}

#[test]
fn workshop_check_reports_catalog_residuals() {
    let path = workshop_fixture("synthetic/raw-settings");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    assert!(result.ok, "check: {:?}", session.diagnostics());
    let residual = result
        .diagnostics
        .iter()
        .find(|d| d.code == "workshop.raw-setting.zzwrightsyntheticunknownkey")
        .expect("raw setting residual is reported through check");
    assert_eq!(residual.severity, wright_driver::Severity::Warning);
    assert_eq!(
        residual.status,
        Some(wright_driver::provider::Status::Partial)
    );
    assert_eq!(
        residual.span.as_ref().map(|s| (s.start.line, s.start.col)),
        Some((6, 9))
    );
}

/// `sqrt` is a foldable spelling that the canonical catalog does not model, so
/// it survives parsing as an unknown-value residual; `fold-constants` would
/// remove it from the transformed program. Completeness diagnostics describe
/// the authored source regardless of the profile (#442).
#[test]
fn workshop_check_reports_residuals_under_transform_profile() {
    let path = workshop_fixture("synthetic/foldable-unknown-value");
    for profile in [wright_driver::Profile::Off, wright_driver::Profile::Compat] {
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(path.clone()),
            kind: SourceKind::Workshop,
            profile,
            ..SessionConfig::default()
        })
        .expect("session creates");
        let result = session.check();
        let residual = result
            .diagnostics
            .iter()
            .find(|d| d.code == "workshop.unknown-value.sqrt")
            .unwrap_or_else(|| {
                panic!(
                    "unknown-value residual is reported under {profile:?}: {:?}",
                    result.diagnostics
                )
            });
        assert_eq!(
            residual.status,
            Some(wright_driver::provider::Status::Unsupported)
        );
    }
}

/// Raw Workshop check and compile must apply the owner's catalog-aware
/// canonical validation, not only the structural `validate()` pass: calls
/// that violate declared catalog signatures are rejected with the owner's
/// error and source span instead of reporting clean success.
#[test]
fn workshop_check_and_compile_reject_catalog_signature_violations() {
    let directory = workspace_root()
        .join("target")
        .join(format!("wright-driver-canonical-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory creates");
    for (name, call) in [
        ("missing-required-argument", "Wait();"),
        (
            "catalog-invalid-slot-type",
            "Small Message(All Players(All Teams), Array(1, 2));",
        ),
    ] {
        let path = directory.join(format!("{name}.ws"));
        std::fs::write(
            &path,
            format!("rule (\"r\")\n{{\n    event\n    {{\n        Ongoing - Global;\n    }}\n    actions\n    {{\n        {call}\n    }}\n}}\n"),
        )
        .expect("fixture writes");
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(path),
            kind: SourceKind::Workshop,
            ..SessionConfig::default()
        })
        .expect("session creates");
        let check = session.check();
        assert!(
            !check.ok,
            "{name}: check must reject the canonical violation: {:?}",
            check.diagnostics
        );
        let diagnostic = check
            .diagnostics
            .iter()
            .find(|d| d.severity == wright_driver::Severity::Error)
            .unwrap_or_else(|| {
                panic!(
                    "{name}: the owner rejection is reported: {:?}",
                    check.diagnostics
                )
            });
        assert_eq!(diagnostic.code, "unsupported-construct", "{name}");
        let span = diagnostic.span.as_ref().unwrap_or_else(|| {
            panic!("{name}: the rejection keeps its source span: {diagnostic:?}")
        });
        assert_eq!(
            (span.start.line, span.end.line),
            (9, 9),
            "{name}: the span locates the violating call"
        );
        let compile = session.compile();
        assert!(
            !compile.ok
                && compile
                    .diagnostics
                    .iter()
                    .any(|d| d.code == "unsupported-construct"),
            "{name}: compile reports the same owner rejection: {:?}",
            compile.diagnostics
        );
    }
    let _ = std::fs::remove_dir_all(&directory);
}

/// A catalog-unknown residual must not mask a later canonical violation:
/// the owner validator is fail-fast, so the residual construct is
/// neutralized before canonical validation judges the rest of the program.
/// The violation is rejected in either order, including under a transform
/// profile that would fold the residual away before emission.
#[test]
fn workshop_check_and_compile_reject_violations_beside_residuals() {
    let directory = workspace_root()
        .join("target")
        .join(format!("wright-driver-mixed-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory creates");
    for (name, calls) in [
        ("residual-then-violation", "Wait(sqrt(4));\n        Wait();"),
        ("violation-then-residual", "Wait();\n        Wait(sqrt(4));"),
    ] {
        let path = directory.join(format!("{name}.ws"));
        std::fs::write(
            &path,
            format!("rule (\"r\")\n{{\n    event\n    {{\n        Ongoing - Global;\n    }}\n    actions\n    {{\n        {calls}\n    }}\n}}\n"),
        )
        .expect("fixture writes");
        for profile in [wright_driver::Profile::Off, wright_driver::Profile::Compat] {
            let mut session = CompilerSession::new(SessionConfig {
                input: InputSpec::Path(path.clone()),
                kind: SourceKind::Workshop,
                profile,
                ..SessionConfig::default()
            })
            .expect("session creates");
            let check = session.check();
            assert!(
                !check.ok
                    && check
                        .diagnostics
                        .iter()
                        .any(|d| d.code == "unsupported-construct"
                            && d.severity == wright_driver::Severity::Error),
                "{name}/{profile:?}: check rejects the canonical violation: {:?}",
                check.diagnostics
            );
            let compile = session.compile();
            assert!(
                !compile.ok
                    && compile
                        .diagnostics
                        .iter()
                        .any(|d| d.code == "unsupported-construct"),
                "{name}/{profile:?}: compile rejects instead of emitting: {:?}",
                compile.diagnostics
            );
        }
    }
    let _ = std::fs::remove_dir_all(&directory);
}

/// Trailing catalog defaults are not required arguments: the owner
/// validator accepts `Wait(1)` and `Wait(1, Ignore Condition)`, and the
/// integration must not reject them.
#[test]
fn workshop_check_accepts_optional_action_defaults() {
    let directory = workspace_root()
        .join("target")
        .join(format!("wright-driver-defaults-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory creates");
    for (name, call) in [
        ("wait-time-only", "Wait(1);"),
        ("wait-full", "Wait(1, Ignore Condition);"),
    ] {
        let path = directory.join(format!("{name}.ws"));
        std::fs::write(
            &path,
            format!("rule (\"r\")\n{{\n    event\n    {{\n        Ongoing - Global;\n    }}\n    actions\n    {{\n        {call}\n    }}\n}}\n"),
        )
        .expect("fixture writes");
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(path),
            kind: SourceKind::Workshop,
            ..SessionConfig::default()
        })
        .expect("session creates");
        let result = session.check();
        assert!(
            result.ok,
            "{name}: optional defaults stay accepted: {:?}",
            result.diagnostics
        );
    }
    let _ = std::fs::remove_dir_all(&directory);
}

/// Catalog-unknown constructs belong to the completeness residual channel:
/// canonical validation defers them rather than aborting the load, so a
/// preserved opaque action still reports as a classified residual.
#[test]
fn workshop_check_reports_opaque_residuals_beside_canonical_validation() {
    let directory = workspace_root()
        .join("target")
        .join(format!("wright-driver-opaque-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory creates");
    // A trailing unmatched structural marker is preserved by the parser as
    // an opaque raw action instead of rejecting the source.
    let path = directory.join("opaque-residual.ws");
    std::fs::write(
        &path,
        "rule (\"r\")\n{\n    event\n    {\n        Ongoing - Global;\n    }\n    actions\n    {\n        Wait(1);\n        Else;\n    }\n}\n",
    )
    .expect("fixture writes");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    let residual = result
        .diagnostics
        .iter()
        .find(|d| d.code == "workshop.opaque-action.rawworkshopaction")
        .unwrap_or_else(|| {
            panic!(
                "the opaque residual is reported through the completeness channel: {:?}",
                result.diagnostics
            )
        });
    assert_eq!(residual.severity, wright_driver::Severity::Error);
    assert_eq!(
        residual.status,
        Some(wright_driver::provider::Status::Unsupported)
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn workshop_compile_is_deterministic_and_idempotent() {
    let path = workshop_fixture("synthetic/basic-rule");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let first = session.compile();
    let second = session.compile();
    assert!(
        first.ok && second.ok,
        "compile diagnostics: {:?}",
        session.diagnostics()
    );
    assert_eq!(
        first.result.output.as_ref().map(|output| &output.text),
        second.result.output.as_ref().map(|output| &output.text)
    );
    assert!(
        first
            .diagnostics
            .iter()
            .all(|d| !d.code.starts_with("target-") && d.code != "element-count-unavailable"),
        "a program under the client element limit gets no target diagnostic: {:?}",
        first.diagnostics
    );
}

/// The smallest Bastion-style case (#488): a program whose canonical element
/// count exceeds the 32768 client limit still emits its artifact, and the
/// compile envelope reports the violation as a warning — not a compilation
/// error. `check` stays a source-correctness workflow and does not evaluate
/// the client import budget.
#[test]
fn compile_warns_over_client_element_limit_but_still_emits() {
    let elements = (0..17_000).map(|_| "1").collect::<Vec<_>>().join(", ");
    let source = format!(
        "variables {{\n    global:\n        0: values\n}}\n\nrule (\"fill\") {{\n    event {{\n        Ongoing - Global;\n    }}\n    actions {{\n        Set Global Variable(values, Array({elements}));\n    }}\n}}\n"
    );
    // The warning's numbers come from the canonical owner, not a guess.
    let catalog = wright_analyzer::catalog::builtin().expect("catalog");
    let locale = workshop_rs::catalog::Locale::new("en-US");
    let expected_total =
        workshop_rs::parser::parse_with_context(&source, &catalog, &locale, &*catalog)
            .expect("generated program parses")
            .element_count(&catalog)
            .expect("generated program counts")
            .total;
    assert!(expected_total > 32768, "fixture is over the client limit");

    let directory = workspace_root()
        .join("target")
        .join(format!("wright-driver-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory creates");
    let path = directory.join("over-limit.ws");
    std::fs::write(&path, source).expect("over-limit fixture writes");

    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");

    let result = session.compile();
    assert!(
        result.ok,
        "over-limit compile still succeeds: {:?}",
        result.diagnostics
    );
    assert_eq!(result.exit, 0);
    let output = result
        .result
        .output
        .as_ref()
        .expect("the artifact is still emitted");
    assert!(output.text.contains("Set Global Variable"));
    let warning = result
        .diagnostics
        .iter()
        .find(|d| d.code == "target-element-limit")
        .expect("the element-limit warning is reported");
    assert_eq!(warning.severity, wright_driver::Severity::Warning);
    assert_eq!(warning.stage, wright_driver::Stage::Emission);
    assert!(
        warning.message.contains(&expected_total.to_string()),
        "the warning reports the canonical observed count: {warning:?}"
    );
    assert!(warning.message.contains("32768"), "{warning:?}");
    assert!(
        warning.message.contains("still emitted"),
        "the warning must not read as a compilation failure: {warning:?}"
    );
    assert!(
        warning.message.contains("\"fill\""),
        "the warning names the largest contributing rule: {warning:?}"
    );
    let span = warning
        .span
        .as_ref()
        .expect("the largest contributing rule is located");
    assert!(span.path.ends_with("over-limit.ws"), "{span:?}");

    let check = session.check();
    assert!(
        check
            .diagnostics
            .iter()
            .all(|d| d.code != "target-element-limit"),
        "check does not evaluate client import limits: {:?}",
        check.diagnostics
    );

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn legacy_protocol_input_is_refused_without_hir_lowering() {
    let path = workspace_root()
        .join("target")
        .join(format!("wright-driver-{}", std::process::id()))
        .join("legacy.json");
    std::fs::create_dir_all(path.parent().unwrap()).expect("fixture directory creates");
    std::fs::write(&path, legacy_protocol()).expect("legacy fixture writes");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Auto,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "input-kind-unsupported");
    let _ = std::fs::remove_dir_all(path.parent().unwrap());
}

#[test]
fn opy_source_never_falls_back_to_a_static_frontend() {
    let path = workspace_root().join("tests/fixtures/opy/basic-rule.opy");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Opy,
        source_backend: SourceBackend::Native,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    assert!(!result.ok);
    assert_eq!(result.diagnostics[0].code, "source-provider-unavailable");
}

fn refused_config_change(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| d.code == "session-config-changed")
}

/// #511: `SessionConfig` is construction-time. A post-construction mutation
/// refuses the next workflow explicitly instead of serving state derived
/// from the earlier configuration — including the already-loaded program
/// cached before the mutation.
#[test]
fn session_config_mutation_refuses_workflows() {
    let path = workshop_fixture("synthetic/control-flow");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");
    assert!(session.check().ok, "check: {:?}", session.diagnostics());

    session.config.kind = SourceKind::Opy;
    assert_eq!(
        session.load().err().expect("load refuses").code,
        "session-config-changed"
    );
    assert!(refused_config_change(&session.check().diagnostics));
    assert!(refused_config_change(&session.compile().diagnostics));
    assert!(refused_config_change(&session.inspect().diagnostics));
    assert!(refused_config_change(&session.lint().diagnostics));
    assert!(refused_config_change(&session.analyze().diagnostics));
    assert!(refused_config_change(&session.symbols(None).diagnostics));
    assert!(refused_config_change(
        &session.rename("x", "y", false).diagnostics
    ));

    // Restoring the construction-time values is not a change: the session
    // serves again.
    session.config.kind = SourceKind::Workshop;
    assert!(session.check().ok, "check: {:?}", session.diagnostics());
}

/// #511: the same refusal covers the non-load edit surfaces and service
/// construction, and the diagnostic names the changed fields.
#[test]
fn session_config_mutation_refuses_edits_and_service_construction() {
    let path = workshop_fixture("synthetic/control-flow");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");

    session.config.lint.disable("min-wait-loop");

    let validation =
        session.validate_edit_transaction(None, &EditTransaction { edits: Vec::new() });
    assert!(!validation.ok);
    assert!(
        validation
            .diagnostics
            .iter()
            .any(|d| d.code == "session-config-changed" && d.message.contains("lint")),
        "{:?}",
        validation.diagnostics
    );

    let rename = session.semantic_rename(
        None,
        &RenameTarget {
            symbol: Some(Address::Name("x".to_string())),
            source: None,
            line: None,
            col: None,
            to: "y".to_string(),
        },
    );
    assert!(!rename.ok);
    assert!(refused_config_change(&rename.diagnostics));

    let error = ToolService::new(&mut session)
        .err()
        .expect("service refuses");
    assert_eq!(error.code, "session-config-changed");
    assert!(error.message.contains("lint"), "{error:?}");
}

/// #511: the provider seam consumes `config.providers`/`config.opy_provider`
/// live; a mutated registry refuses through the provider refusal channel
/// carrying `session-config-changed` instead of spawning from the changed
/// configuration.
#[test]
fn session_config_mutation_refuses_provider_workflows() {
    let path = workshop_fixture("synthetic/control-flow");
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");

    session
        .config
        .providers
        .register(wright_lpp::ProviderConfig::new(
            "x-demo",
            "/bin/false",
            Vec::new(),
        ))
        .expect("provider registers");

    let error = session
        .language_provider("x-demo")
        .err()
        .expect("provider spawn refuses");
    assert_eq!(
        error.refusal_code(),
        Some("session-config-changed"),
        "{error:?}"
    );

    // The provider-mutation flow surfaces the same structured refusal and
    // never runs its flow.
    let mutation = session.run_provider_flow(
        "x-demo",
        &wright_lpp::ClientInfo {
            name: "wright-driver-test".to_string(),
            version: "0".to_string(),
        },
        |_| panic!("the provider flow must not run"),
    );
    assert!(!mutation.ok);
    assert_eq!(
        mutation.provider_code.as_deref(),
        Some("session-config-changed"),
        "{mutation:?}"
    );
}
