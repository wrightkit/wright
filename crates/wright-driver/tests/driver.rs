use std::path::{Path, PathBuf};

use wright_driver::edit::EditTransaction;
use wright_driver::service::ToolService;
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
    assert_eq!(residual.status, Some(wright_driver::Status::Partial));
    assert_eq!(
        residual.span.as_ref().map(|s| (s.start.line, s.start.col)),
        Some((6, 9))
    );
}

#[test]
fn workshop_raw_setting_residual_suggests_the_canonical_spelling() {
    let text = r#"settings
{
    main
    {
        Mode Nmae: "Synthetic"
        zzWrightSyntheticUnknownKey: 1
    }
    modes
    {
        Team Deathmatch
        {
            Enabled Mpas
            {
                Busan
            }
        }
    }
}
"#;
    let mut session = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: text.to_string(),
            path: None,
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("session creates");
    let result = session.check();
    assert!(result.ok, "check: {:?}", session.diagnostics());
    let message = |code: &str| {
        result
            .diagnostics
            .iter()
            .find(|d| d.code == code)
            .map(|d| d.message.clone())
            .unwrap_or_else(|| panic!("{code} is reported: {:?}", result.diagnostics))
    };
    assert_eq!(
        message("workshop.raw-setting.mode-nmae"),
        "Workshop construct 'Mode Nmae' is unsupported (catalog-spelling-near-miss) \
         (did you mean 'Mode Name'?)"
    );
    assert_eq!(
        message("workshop.raw-setting.enabled-mpas"),
        "Workshop construct 'Enabled Mpas' is unsupported (catalog-spelling-near-miss) \
         (did you mean 'enabled maps'?)"
    );
    assert_eq!(
        message("workshop.raw-setting.zzwrightsyntheticunknownkey"),
        "Workshop construct 'zzWrightSyntheticUnknownKey' is partially supported \
         (project-defined-construct)"
    );
}

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
        assert_eq!(residual.status, Some(wright_driver::Status::Unsupported));
    }
}

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

#[test]
fn workshop_check_reports_opaque_residuals_beside_canonical_validation() {
    let directory = workspace_root()
        .join("target")
        .join(format!("wright-driver-opaque-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("fixture directory creates");
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
    assert_eq!(residual.status, Some(wright_driver::Status::Unsupported));
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
    assert!(refused_config_change(
        &session.symbols(None, None, None).diagnostics
    ));
    assert!(refused_config_change(&session.lint_fix(false).diagnostics));

    session.config.kind = SourceKind::Workshop;
    assert!(session.check().ok, "check: {:?}", session.diagnostics());
}

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

    let error = ToolService::new(&mut session)
        .err()
        .expect("service refuses");
    assert_eq!(error.code, "session-config-changed");
    assert!(error.message.contains("lint"), "{error:?}");
}

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

#[test]
fn workshop_text_input_replaces_the_disk_read() {
    let path = workshop_fixture("synthetic/raw-settings");
    let text = std::fs::read_to_string(&path).expect("fixture");
    let codes_and_spans = |diagnostics: &[Diagnostic]| {
        diagnostics
            .iter()
            .map(|d| (d.code.clone(), d.severity, d.span.clone()))
            .collect::<Vec<_>>()
    };

    let mut on_disk = CompilerSession::new(SessionConfig {
        input: InputSpec::Path(path.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("path session");
    let mut in_memory = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text,
            path: Some(path.clone()),
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("text session");

    assert_eq!(
        codes_and_spans(&in_memory.check().diagnostics),
        codes_and_spans(&on_disk.check().diagnostics),
        "the buffer text yields the check diagnostics its bytes on disk yield"
    );

    let unsaved_source = concat!(
        "rule (\"unsaved\") {\n",
        "    event {\n",
        "        Ongoing - Global;\n",
        "    }\n",
        "}\n",
    );
    let mut unsaved = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: unsaved_source.to_string(),
            path: Some(PathBuf::from("unsaved-buffer.ws")),
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("unsaved session");
    let unsaved_check = unsaved.check();
    assert!(
        unsaved_check.ok,
        "unsaved buffer: {:?}",
        unsaved_check.diagnostics
    );

    let mut kindless = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: String::new(),
            path: None,
        },
        ..SessionConfig::default()
    })
    .expect("session creates");
    let error = match kindless.load() {
        Ok(_) => panic!("anonymous text cannot detect a kind"),
        Err(error) => error,
    };
    assert_eq!(error.code, "input-kind-unknown");

    let mut anonymous = CompilerSession::new(SessionConfig {
        input: InputSpec::Text {
            text: unsaved_source.to_string(),
            path: None,
        },
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    })
    .expect("anonymous session");
    let anonymous_check = anonymous.check();
    assert!(
        anonymous_check.ok,
        "anonymous buffer: {:?}",
        anonymous_check.diagnostics
    );
}

#[test]
fn compile_to_asks_before_replacing_and_never_replaces_the_input() {
    use wright_driver::{CompileWrite, OutputDest, Overwrite};

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("compile-to-ask");
    std::fs::create_dir_all(&dir).unwrap();
    let input = dir.join("mode.ws");
    let source = std::fs::read_to_string(workshop_fixture("synthetic/basic-rule")).unwrap();
    std::fs::write(&input, &source).unwrap();
    let out = dir.join("mode.ow");
    let config = || SessionConfig {
        input: InputSpec::Path(input.clone()),
        kind: SourceKind::Workshop,
        ..SessionConfig::default()
    };

    std::fs::write(&out, "old").unwrap();
    let mut asked = Vec::new();
    let mut decline = |path: &Path| {
        asked.push(path.to_path_buf());
        false
    };
    let declined = CompilerSession::new(config())
        .unwrap()
        .compile_to(CompileWrite {
            dest: OutputDest::Path(out.clone()),
            overwrite: Overwrite::Ask(&mut decline),
            to: None,
        });
    assert_eq!(asked, vec![out.clone()]);
    assert_eq!(declined.exit, 1);
    assert!(declined.result.output.is_none());
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "old");

    let mut accept = |_: &Path| true;
    let accepted = CompilerSession::new(config())
        .unwrap()
        .compile_to(CompileWrite {
            dest: OutputDest::Path(out.clone()),
            overwrite: Overwrite::Ask(&mut accept),
            to: None,
        });
    assert!(accepted.ok, "{:?}", accepted.diagnostics);
    assert_ne!(std::fs::read_to_string(&out).unwrap(), "old");

    let mut never = |_: &Path| panic!("the input is refused before any prompt");
    let refused = CompilerSession::new(config())
        .unwrap()
        .compile_to(CompileWrite {
            dest: OutputDest::Path(input.clone()),
            overwrite: Overwrite::Ask(&mut never),
            to: None,
        });
    assert_eq!(refused.exit, 2);
    assert_eq!(std::fs::read_to_string(&input).unwrap(), source);
    let _ = std::fs::remove_dir_all(&dir);
}
