//! The failure-classification table for the `check` surface — one of
//! the three public surfaces the classification contract covers;
//! `lookup` keeps its own table in `lookup_failures.rs` and the
//! language-service diagnostic surface keeps its own in
//! `wright-language`. Every row drives `session.check()` end to end and
//! pins the error diagnostic's `code`, its `class`, and the envelope
//! `exit` the class resolves to: a wrapper that re-classifies a failure
//! — a refusal reported as `unavailable`, a session diagnostic relabeled
//! `provider-refusal-*`, or a dropped `Diagnostic::class` — fails its
//! row.
//!
//! "Provider failure kinds" are the `wright_lpp::ProviderError`
//! taxonomy; "session failure kinds" are the session machinery's own
//! refusals — `CompilerSession::new` diagnostics, `session-config-changed`,
//! and the `source-provider-*` load refusals. Input-resolution
//! diagnostics (`input-*`, `stdin-*`, `hidden-only`, `cwd-io`) belong to
//! the input layer's own contract and stay out of this table.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use wright_driver::source_provider::{
    LppSourceProvider, SourceCompilation, SourceLanguage, SourceProvider, SourceProviderError,
    SourceTarget,
};
use wright_driver::{
    CompilerSession, ExitClass, FindingSelection, InputSpec, SessionConfig, Severity, SourceKind,
};
use wright_lpp::{LocalProviderErrorKind, LppErrorKind, ProviderError};

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-check-failures-{}-{}-{tag}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_file(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, text).expect("write file");
    path
}

/// A negotiated `LanguageProvider` whose `compile`/`check` entries fail
/// with one typed `ProviderError` — the request-side transport failure
/// an LPP provider produces after a healthy `lpp/initialize`.
struct FailingLppProvider {
    negotiated: wright_lpp::NegotiatedCapabilities,
    failure: ProviderError,
}

fn negotiated() -> wright_lpp::NegotiatedCapabilities {
    wright_lpp::NegotiatedCapabilities {
        protocol_version: wright_lpp::LPP_PROJECT_LOADING_PROTOCOL_VERSION.to_string(),
        server_info: wright_lpp::ServerInfo {
            name: "fake".to_string(),
            version: "0".to_string(),
        },
        languages: vec![wright_lpp::LanguageInfo {
            id: "opy".to_string(),
            extensions: vec!["opy".to_string()],
        }],
        capabilities: wright_lpp::Capabilities {
            check: true,
            compile: true,
            project_loading: true,
            reconstruct: false,
            symbols: false,
            definition: false,
            references: false,
            rename: false,
            edit_validation: false,
            source_identity: false,
            lookup: false,
            resolve_ids: false,
            other: Default::default(),
        },
    }
}

impl wright_lpp::LanguageProvider for FailingLppProvider {
    fn initialize(
        &mut self,
        _client_info: Option<&wright_lpp::ClientInfo>,
    ) -> Result<wright_lpp::InitializeResult, ProviderError> {
        unreachable!()
    }
    fn capabilities(&self) -> Result<&wright_lpp::NegotiatedCapabilities, ProviderError> {
        Ok(&self.negotiated)
    }
    fn check(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::CheckResult, ProviderError> {
        unreachable!()
    }
    fn check_entry(
        &mut self,
        _entry: &wright_lpp::ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<wright_lpp::CheckResult, ProviderError> {
        Err(self.failure.clone())
    }
    fn compile(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::CompileResult, ProviderError> {
        unreachable!()
    }
    fn compile_entry(
        &mut self,
        _entry: &wright_lpp::ProjectEntry,
        _project_root: Option<&str>,
        _locale: Option<&str>,
    ) -> Result<wright_lpp::CompileResult, ProviderError> {
        Err(self.failure.clone())
    }
    fn reconstruct(
        &mut self,
        _artifact: &wright_lpp::WorkshopArtifact,
    ) -> Result<wright_lpp::ReconstructResult, ProviderError> {
        unreachable!()
    }
    fn symbols(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::SymbolsResult, ProviderError> {
        unreachable!()
    }
    fn definition(
        &mut self,
        _document: &wright_lpp::Document,
        _position: wright_lpp::Position,
    ) -> Result<wright_lpp::LocationsResult, ProviderError> {
        unreachable!()
    }
    fn references(
        &mut self,
        _document: &wright_lpp::Document,
        _position: wright_lpp::Position,
        _include_declaration: bool,
    ) -> Result<wright_lpp::LocationsResult, ProviderError> {
        unreachable!()
    }
    fn rename(
        &mut self,
        _documents: &wright_lpp::DocumentSet,
        _position_document_uri: &str,
        _position: wright_lpp::Position,
        _new_name: &str,
        _project_root: Option<&str>,
    ) -> Result<wright_lpp::RenameResult, ProviderError> {
        unreachable!()
    }
    fn validate_edits(
        &mut self,
        _document: &wright_lpp::Document,
        _edits: &[wright_lpp::TextEdit],
    ) -> Result<wright_lpp::ValidateEditsResult, ProviderError> {
        unreachable!()
    }
    fn shutdown(&mut self) -> Result<(), ProviderError> {
        Ok(())
    }
    fn exit_status(&self) -> Option<i32> {
        None
    }
}

/// A `SourceProvider` whose `compile` (and the `check` fallback that
/// delegates to it) answers with one fixed result — the source-provider
/// layer's own refusal kinds that an LPP provider never emits.
struct StubSourceProvider {
    answer: Result<SourceCompilation, SourceProviderError>,
}

impl SourceProvider for StubSourceProvider {
    fn language(&self) -> SourceLanguage {
        SourceLanguage::Opy
    }
    fn compile(
        &mut self,
        _target: &SourceTarget,
    ) -> Result<SourceCompilation, SourceProviderError> {
        self.answer.clone()
    }
}

fn opy_entry(dir: &Path) -> PathBuf {
    write_file(dir, "main.opy", "rule 'x': pass\n")
}

/// `check`'s failure table: every provider failure kind and every
/// session failure kind that reaches the `session.check()` envelope,
/// pinned by the error diagnostic's `code`, its `class`, and the
/// envelope `exit` the class resolves to.
#[test]
fn check_surfaces_every_failure_kind_with_its_code_and_exit_class() {
    let lpp = |kind, details| ProviderError::lpp(kind, details, "the provider failed");
    let refusal = |code: &str| {
        ProviderError::lpp(
            LppErrorKind::Refusal,
            serde_json::json!({"refusalCode": code}),
            "the provider refused",
        )
    };
    let local = |kind| ProviderError::Local {
        kind,
        message: "resolution failed".to_string(),
    };

    // Every `ProviderError` variant reaches `check` as a request failure
    // through the shared `provider_error` classification — the same
    // function the connect path wraps its failures with.
    let provider_rows: &[(ProviderError, &str, ExitClass)] = &[
        (
            ProviderError::NotConfigured {
                language_id: "opy".to_string(),
            },
            "provider-not-configured",
            ExitClass::Internal,
        ),
        (
            ProviderError::Spawn {
                message: "spawn failed".to_string(),
            },
            "provider-spawn",
            ExitClass::Internal,
        ),
        (
            ProviderError::Io {
                message: "pipe broke".to_string(),
            },
            "provider-io",
            ExitClass::Internal,
        ),
        (
            ProviderError::Exited {
                status: Some(9),
                message: "the provider exited".to_string(),
            },
            "provider-exited",
            ExitClass::Internal,
        ),
        (
            ProviderError::Timeout {
                method: "lpp/compile".to_string(),
                duration: Duration::from_millis(1500),
            },
            "provider-timeout",
            ExitClass::Internal,
        ),
        (
            ProviderError::Malformed {
                detail: "no result".to_string(),
            },
            "provider-malformed",
            ExitClass::Internal,
        ),
        (
            ProviderError::JsonRpc {
                code: -32600,
                message: "bad request".to_string(),
            },
            "jsonrpc-error",
            ExitClass::Internal,
        ),
        (
            lpp(
                LppErrorKind::ProtocolVersionMismatch,
                serde_json::json!({"supportedProtocolVersions": ["1.0"]}),
            ),
            "protocol-version-mismatch",
            ExitClass::Internal,
        ),
        (
            lpp(LppErrorKind::InvalidRequest, serde_json::json!({})),
            "invalid-request",
            ExitClass::Internal,
        ),
        (
            lpp(LppErrorKind::InvalidLanguage, serde_json::json!({})),
            "invalid-language",
            ExitClass::Internal,
        ),
        (
            lpp(LppErrorKind::InvalidDocument, serde_json::json!({})),
            "invalid-document",
            ExitClass::Internal,
        ),
        (
            lpp(LppErrorKind::InvalidEntry, serde_json::json!({})),
            "invalid-entry",
            ExitClass::Internal,
        ),
        (
            lpp(LppErrorKind::ProjectLoadFailed, serde_json::json!({})),
            "project-load-failed",
            ExitClass::SourceError,
        ),
        (
            lpp(LppErrorKind::InvalidPosition, serde_json::json!({})),
            "invalid-position",
            ExitClass::Internal,
        ),
        (
            lpp(LppErrorKind::InvalidArtifact, serde_json::json!({})),
            "invalid-artifact",
            ExitClass::Internal,
        ),
        (
            lpp(
                LppErrorKind::CapabilityUnavailable,
                serde_json::json!({"capability": "compile", "method": "lpp/compile"}),
            ),
            "capability-unavailable",
            ExitClass::Unsupported,
        ),
        (
            lpp(LppErrorKind::Refusal, serde_json::json!({})),
            "provider-refusal",
            ExitClass::SourceError,
        ),
        (
            refusal("check.tooLarge"),
            "provider-refusal-check.tooLarge",
            ExitClass::SourceError,
        ),
        (
            refusal("invalid-selection"),
            "provider-refusal-invalid-selection",
            ExitClass::SourceError,
        ),
        (
            refusal("session-config-changed"),
            "session-config-changed",
            ExitClass::SourceError,
        ),
        (
            lpp(
                LppErrorKind::Unknown("futureKind".to_string()),
                serde_json::json!({}),
            ),
            "lpp-error",
            ExitClass::Internal,
        ),
        (
            ProviderError::NotInitialized {
                method: "compile".to_string(),
            },
            "provider-not-initialized",
            ExitClass::Internal,
        ),
        (
            ProviderError::AlreadyInitialized,
            "provider-already-initialized",
            ExitClass::Internal,
        ),
        (
            ProviderError::ShutDown {
                method: "compile".to_string(),
            },
            "provider-shutdown",
            ExitClass::Internal,
        ),
        (
            ProviderError::ProtocolVersionMismatch {
                supported: vec!["1.0".to_string()],
                message: "no mutual version".to_string(),
            },
            "protocol-version-mismatch",
            ExitClass::Internal,
        ),
        (
            local(LocalProviderErrorKind::Missing),
            "provider-missing",
            ExitClass::Internal,
        ),
        (
            local(LocalProviderErrorKind::UnsupportedPlatform),
            "provider-unsupported-platform",
            ExitClass::Internal,
        ),
        (
            local(LocalProviderErrorKind::Offline),
            "provider-offline",
            ExitClass::Internal,
        ),
        (
            local(LocalProviderErrorKind::Download),
            "provider-download",
            ExitClass::Internal,
        ),
        (
            local(LocalProviderErrorKind::Integrity),
            "provider-integrity",
            ExitClass::Internal,
        ),
        (
            local(LocalProviderErrorKind::Install),
            "provider-install",
            ExitClass::Internal,
        ),
    ];

    for (failure, code, class) in provider_rows {
        let dir = temp_dir("check-provider");
        let entry = opy_entry(&dir);
        let provider = LppSourceProvider::new(
            Box::new(FailingLppProvider {
                negotiated: negotiated(),
                failure: failure.clone(),
            }),
            None,
        );
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(entry),
                kind: SourceKind::Opy,
                ..SessionConfig::default()
            },
            Box::new(provider),
        )
        .expect("session");
        let envelope = session.check();
        let diagnostic = envelope
            .diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error)
            .unwrap_or_else(|| {
                panic!(
                    "{failure:?} produced no error diagnostic: {:?}",
                    envelope.diagnostics
                )
            });
        assert_eq!(diagnostic.code, *code, "{failure:?} lost its code");
        assert_eq!(diagnostic.class, *class, "{failure:?} lost its class");
        assert_eq!(envelope.exit, class.exit_code(), "{failure:?}");
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    // The session machinery's own refusal kinds: construction
    // diagnostics (observed as the session boundary's `Err`), the
    // fixed-config refusal, and the `source-provider-*` load refusals.
    let dir = temp_dir("check-construction");
    for (configure, code, class) in [
        (
            Box::new(|config: &mut SessionConfig| {
                config.selection = FindingSelection {
                    rule: Some("no-such-rule".to_string()),
                    ..FindingSelection::default()
                };
            }) as Box<dyn FnOnce(&mut SessionConfig)>,
            "invalid-selection",
            ExitClass::Usage,
        ),
        (
            Box::new(|config: &mut SessionConfig| {
                config.lint_rule_paths = vec![dir.join("missing-rule.opy")];
            }) as Box<dyn FnOnce(&mut SessionConfig)>,
            "lint-rule-error",
            ExitClass::SourceError,
        ),
    ] {
        let mut config = SessionConfig::default();
        configure(&mut config);
        let Err(diagnostic) = CompilerSession::new(config) else {
            panic!("{code} must refuse session construction");
        };
        assert_eq!(diagnostic.code, code);
        assert_eq!(diagnostic.class, class);
        assert_eq!(diagnostic.class.exit_code(), class.exit_code());
    }
    std::fs::remove_dir_all(&dir).expect("cleanup");

    // `session-config-changed` — the fixed-config refusal.
    {
        let dir = temp_dir("check-config");
        let entry = opy_entry(&dir);
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(entry),
                kind: SourceKind::Opy,
                ..SessionConfig::default()
            },
            Box::new(StubSourceProvider {
                answer: Err(SourceProviderError::NotConfigured {
                    language: SourceLanguage::Opy,
                }),
            }),
        )
        .expect("session");
        session.config.locale = Some("fr-FR".to_string());
        let envelope = session.check();
        assert!(
            envelope.diagnostics.iter().any(|d| {
                d.code == "session-config-changed" && d.class == ExitClass::SourceError
            })
        );
        assert_eq!(envelope.exit, ExitClass::SourceError.exit_code());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    // The loader's `source-provider-*` refusal kinds.
    let stub_failures: &[(
        &str,
        Result<SourceCompilation, SourceProviderError>,
        ExitClass,
    )] = &[
        (
            "not-configured",
            Err(SourceProviderError::NotConfigured {
                language: SourceLanguage::Opy,
            }),
            ExitClass::Internal,
        ),
        (
            "unsupported",
            Err(SourceProviderError::Unsupported {
                message: "the provider declined".to_string(),
            }),
            ExitClass::Unsupported,
        ),
    ];
    for (tag, answer, class) in stub_failures {
        let dir = temp_dir(&format!("check-stub-{tag}"));
        let entry = opy_entry(&dir);
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(entry),
                kind: SourceKind::Opy,
                ..SessionConfig::default()
            },
            Box::new(StubSourceProvider {
                answer: answer.clone(),
            }),
        )
        .expect("session");
        let envelope = session.check();
        let code = match answer {
            Err(SourceProviderError::NotConfigured { .. }) => "source-provider-not-configured",
            Err(SourceProviderError::Unsupported { .. }) => "source-provider-unsupported",
            _ => unreachable!(),
        };
        assert!(
            envelope
                .diagnostics
                .iter()
                .any(|d| { d.code == code && d.class == *class })
        );
        assert_eq!(envelope.exit, class.exit_code());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }

    // `source-provider-unsupported` via the loader itself (a provider
    // backend cannot serve a non-path input).
    {
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Text {
                    text: "x".to_string(),
                    path: None,
                },
                kind: SourceKind::Opy,
                ..SessionConfig::default()
            },
            Box::new(StubSourceProvider {
                answer: Err(SourceProviderError::NotConfigured {
                    language: SourceLanguage::Opy,
                }),
            }),
        )
        .expect("session");
        let envelope = session.check();
        assert!(envelope.diagnostics.iter().any(|d| {
            d.code == "source-provider-unsupported" && d.class == ExitClass::Unsupported
        }));
        assert_eq!(envelope.exit, 3);
    }

    // `source-provider-unavailable` (an OSTW source has no provider to
    // connect), `source-provider-kind` (a provider backend handed a
    // non-OPY input), `source-provider-identity` (a directory compile
    // answer without its source identity), and `profile-workshop-only`.
    {
        let dir = temp_dir("check-kinds");
        let ostw = write_file(&dir, "main.ostw", "x\n");
        let mut session = CompilerSession::new(SessionConfig {
            input: InputSpec::Path(ostw),
            kind: SourceKind::Ostw,
            ..SessionConfig::default()
        })
        .expect("session");
        let envelope = session.check();
        assert!(envelope.diagnostics.iter().any(|d| {
            d.code == "source-provider-unavailable" && d.class == ExitClass::Internal
        }));
        assert_eq!(envelope.exit, ExitClass::Internal.exit_code());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
    {
        let dir = temp_dir("check-kind-mismatch");
        let ws = write_file(&dir, "main.ws", "rule (\"x\") {}\n");
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(ws),
                kind: SourceKind::Workshop,
                ..SessionConfig::default()
            },
            Box::new(StubSourceProvider {
                answer: Err(SourceProviderError::NotConfigured {
                    language: SourceLanguage::Opy,
                }),
            }),
        )
        .expect("session");
        let envelope = session.check();
        assert!(
            envelope
                .diagnostics
                .iter()
                .any(|d| { d.code == "source-provider-kind" && d.class == ExitClass::SourceError })
        );
        assert_eq!(envelope.exit, ExitClass::SourceError.exit_code());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
    {
        let dir = temp_dir("check-identity");
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(dir.clone()),
                kind: SourceKind::Opy,
                ..SessionConfig::default()
            },
            Box::new(StubSourceProvider {
                answer: Ok(SourceCompilation::success("rule (\"x\") {}\n")),
            }),
        )
        .expect("session");
        let envelope = session.check();
        assert!(envelope.diagnostics.iter().any(|d| {
            d.code == "source-provider-identity" && d.class == ExitClass::SourceError
        }));
        assert_eq!(envelope.exit, ExitClass::SourceError.exit_code());
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
    {
        let dir = temp_dir("check-profile");
        let entry = opy_entry(&dir);
        let mut session = CompilerSession::with_source_provider(
            SessionConfig {
                input: InputSpec::Path(entry),
                kind: SourceKind::Opy,
                profile: wright_driver::Profile::Compat,
                ..SessionConfig::default()
            },
            Box::new(StubSourceProvider {
                answer: Err(SourceProviderError::NotConfigured {
                    language: SourceLanguage::Opy,
                }),
            }),
        )
        .expect("session");
        let envelope = session.check();
        assert!(
            envelope
                .diagnostics
                .iter()
                .any(|d| { d.code == "profile-workshop-only" && d.class == ExitClass::Usage })
        );
        assert_eq!(envelope.exit, 2);
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}
