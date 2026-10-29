//! The failure-classification table for the `lookup` surface — one of
//! the three public surfaces the classification contract covers;
//! `check` keeps its own table in `check_failures.rs` and the
//! language-service diagnostic surface keeps its own in
//! `wright-language`. Every row drives the boundary that surfaces the
//! diagnostic — the tool-service `Lookup` response end to end for the
//! provider rows, `CompilerSession::new`/`ToolService::new` for the
//! session-boundary rows — and pins the error's `code`, its `class`,
//! and the exit code the class resolves to (`unavailable` answers are
//! the surface's unsupported verdict — exit 3): a wrapper that
//! re-classifies
//! a failure — a refusal reported as `unavailable`, an `invalid-*` code
//! defaulted to `SourceError`, or a session diagnostic relabeled
//! `provider-refusal-*` — fails its row.
//!
//! "Provider failure kinds" are the `wright_lpp::ProviderError`
//! taxonomy; "session failure kinds" are the session machinery's own
//! refusals — `CompilerSession::new` diagnostics carried verbatim and
//! the fixed-config `session-config-changed` refusal. Connect-channel
//! failures reach the surface through `connect_provider`; request-channel
//! failures through the provider's `lpp/lookup` answer.
//! `provider-not-configured` cannot reach this surface (`opy` always
//! resolves through the configured resolver), the client-side
//! session-state errors (`provider-not-initialized`,
//! `provider-already-initialized`, `provider-shutdown`) have no real
//! trigger after a successful connect, and a clean `provider-io` has no
//! deterministic process trigger.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use wright_driver::service::{ToolRequest, ToolResponse, ToolService};
use wright_driver::{
    CompilerSession, ExitClass, FindingSelection, InputSpec, SessionConfig, SourceKind,
};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workshop_path() -> PathBuf {
    workspace_root().join("tests/fixtures/workshop/synthetic/control-flow.ws")
}

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-lookup-failures-{}-{}-{tag}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// `lookup`'s failure table: every provider failure kind and every
/// session failure kind that reaches the tool-service `Lookup`
/// response, plus the session-boundary refusals observed at
/// `CompilerSession::new`/`ToolService::new`.
#[test]
fn lookup_surfaces_every_failure_kind_with_its_code_and_exit_class() {
    enum Setup {
        /// A provider registered under the `opy` id.
        Registered {
            command: PathBuf,
            args: Vec<String>,
            timeout: Option<Duration>,
        },
        /// `opy_provider` resolves to a nonexistent executable.
        MissingExecutable,
        /// A registered command that does not exist.
        MissingCommand,
    }

    #[derive(Debug)]
    enum Expect {
        Error {
            code: &'static str,
            class: ExitClass,
        },
        Unavailable,
    }

    fn service(setup: Setup) -> ToolService<'static> {
        let mut config = SessionConfig {
            input: InputSpec::Path(workshop_path()),
            kind: SourceKind::Workshop,
            ..SessionConfig::default()
        };
        match setup {
            Setup::Registered {
                command,
                args,
                timeout,
            } => {
                let mut provider = wright_lpp::ProviderConfig::new("opy", command, args);
                if let Some(timeout) = timeout {
                    provider.request_timeout = timeout;
                }
                config
                    .providers
                    .register(provider)
                    .expect("provider registers");
            }
            Setup::MissingExecutable => {
                config.opy_provider =
                    wright_driver::OpyProviderConfig::with_executable("/nonexistent/opy-provider");
            }
            Setup::MissingCommand => {
                config
                    .providers
                    .register(wright_lpp::ProviderConfig::new(
                        "opy",
                        "/nonexistent/opy-provider",
                        Vec::new(),
                    ))
                    .expect("provider registers");
            }
        }
        let session = CompilerSession::new(config).expect("session");
        let session = Box::leak(Box::new(session));
        ToolService::new(session).expect("service")
    }

    fn request() -> ToolRequest {
        ToolRequest::Lookup {
            language: "opy".to_string(),
            query: Some("hud".to_string()),
            kind: None,
            within: None,
            locale: None,
            limit: None,
        }
    }

    let mut cases: Vec<(&'static str, Setup, Expect)> = vec![
        (
            "provider-missing",
            Setup::MissingExecutable,
            Expect::Unavailable,
        ),
        ("provider-spawn", Setup::MissingCommand, Expect::Unavailable),
    ];

    #[cfg(unix)]
    {
        let dir = temp_dir("lookup-provider");
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("fake-provider");
            std::fs::write(&path, MODE_PROVIDER).expect("provider script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            path
        };
        let spec = |spec: &str| Setup::Registered {
            command: script.clone(),
            args: vec![spec.to_string()],
            timeout: None,
        };
        let timed = |spec: &str| Setup::Registered {
            command: script.clone(),
            args: vec![spec.to_string()],
            timeout: Some(Duration::from_millis(250)),
        };
        let error = |code: &'static str, class: ExitClass| Expect::Error { code, class };
        cases.extend([
            // `lpp/initialize` failures (the connect channel): the
            // capability gaps degrade to `unavailable`; everything else
            // keeps `provider_failure_code` + `lookup_failure_class`.
            (
                "init:mismatch",
                spec("init:mismatch:1.0"),
                Expect::Unavailable,
            ),
            (
                "init:capability-unavailable",
                spec("init:lpp:capabilityUnavailable"),
                Expect::Unavailable,
            ),
            ("init:exited", spec("init:exit:3"), Expect::Unavailable),
            (
                "init:refusal-coded",
                spec("init:refusal:init.tooBusy"),
                error("provider-refusal-init.tooBusy", ExitClass::SourceError),
            ),
            (
                "init:refusal-invalid",
                spec("init:refusal:invalid-selection"),
                error("provider-refusal-invalid-selection", ExitClass::Usage),
            ),
            (
                "init:refusal-plain",
                spec("init:refusal"),
                error("provider-refusal", ExitClass::SourceError),
            ),
            (
                "init:refusal-config",
                spec("init:refusal:session-config-changed"),
                error("session-config-changed", ExitClass::SourceError),
            ),
            (
                "init:invalid-request",
                spec("init:lpp:invalidRequest"),
                error("invalid-request", ExitClass::Usage),
            ),
            (
                "init:invalid-language",
                spec("init:lpp:invalidLanguage"),
                error("invalid-language", ExitClass::Usage),
            ),
            (
                "init:invalid-document",
                spec("init:lpp:invalidDocument"),
                error("invalid-document", ExitClass::Usage),
            ),
            (
                "init:invalid-entry",
                spec("init:lpp:invalidEntry"),
                error("invalid-entry", ExitClass::Usage),
            ),
            (
                "init:invalid-position",
                spec("init:lpp:invalidPosition"),
                error("invalid-position", ExitClass::Usage),
            ),
            (
                "init:invalid-artifact",
                spec("init:lpp:invalidArtifact"),
                error("invalid-artifact", ExitClass::Usage),
            ),
            (
                "init:project-load-failed",
                spec("init:lpp:projectLoadFailed"),
                error("project-load-failed", ExitClass::SourceError),
            ),
            (
                "init:unknown-kind",
                spec("init:lpp:unknownFuture"),
                error("lpp-error", ExitClass::SourceError),
            ),
            (
                "init:jsonrpc",
                spec("init:raw:-32600"),
                error("jsonrpc-error", ExitClass::Internal),
            ),
            (
                "init:malformed",
                spec("init:malformed"),
                error("provider-malformed", ExitClass::Internal),
            ),
            (
                "init:timeout",
                timed("init:sleep"),
                error("provider-timeout", ExitClass::Internal),
            ),
            // An unnegotiated `lookup` capability is `unavailable`.
            (
                "init:no-lookup-capability",
                spec("init:nolookup"),
                Expect::Unavailable,
            ),
            // `lpp/lookup` failures (the request channel): the carried
            // code is the refusal's own `refusalCode`, verbatim.
            (
                "op:refusal-invalid",
                spec("op:refusal:invalid-selection"),
                error("invalid-selection", ExitClass::Usage),
            ),
            (
                "op:refusal-unknown-within",
                spec("op:refusal:lookup.unknownWithin"),
                error("lookup.unknownWithin", ExitClass::SourceError),
            ),
            (
                "op:refusal-coded",
                spec("op:refusal:check.tooLarge"),
                error("check.tooLarge", ExitClass::SourceError),
            ),
            (
                "op:refusal-plain",
                spec("op:refusal"),
                error("refusal", ExitClass::SourceError),
            ),
            (
                "op:refusal-config",
                spec("op:refusal:session-config-changed"),
                error("session-config-changed", ExitClass::SourceError),
            ),
            (
                "op:invalid-request",
                spec("op:lpp:invalidRequest"),
                error("invalid-request", ExitClass::Usage),
            ),
            (
                "op:invalid-language",
                spec("op:lpp:invalidLanguage"),
                error("invalid-language", ExitClass::Usage),
            ),
            (
                "op:invalid-document",
                spec("op:lpp:invalidDocument"),
                error("invalid-document", ExitClass::Usage),
            ),
            (
                "op:invalid-entry",
                spec("op:lpp:invalidEntry"),
                error("invalid-entry", ExitClass::Usage),
            ),
            (
                "op:invalid-position",
                spec("op:lpp:invalidPosition"),
                error("invalid-position", ExitClass::Usage),
            ),
            (
                "op:invalid-artifact",
                spec("op:lpp:invalidArtifact"),
                error("invalid-artifact", ExitClass::Usage),
            ),
            (
                "op:project-load-failed",
                spec("op:lpp:projectLoadFailed"),
                error("project-load-failed", ExitClass::SourceError),
            ),
            (
                "op:capability-unavailable",
                spec("op:lpp:capabilityUnavailable"),
                error("capability-unavailable", ExitClass::Unsupported),
            ),
            (
                "op:protocol-mismatch",
                spec("op:lpp:protocolVersionMismatch"),
                error("protocol-version-mismatch", ExitClass::Internal),
            ),
            (
                "op:unknown-kind",
                spec("op:lpp:unknownFuture"),
                error("lpp-error", ExitClass::SourceError),
            ),
            (
                "op:jsonrpc",
                spec("op:raw:-32600"),
                error("jsonrpc-error", ExitClass::Internal),
            ),
            (
                "op:malformed",
                spec("op:malformed"),
                error("provider-malformed", ExitClass::Internal),
            ),
            (
                "op:exited",
                spec("op:exit:3"),
                error("provider-exited", ExitClass::Internal),
            ),
            (
                "op:timeout",
                timed("op:sleep"),
                error("provider-timeout", ExitClass::Internal),
            ),
        ]);
    }

    for (name, setup, expect) in cases {
        let mut service = service(setup);
        let response = service.handle(&request());
        match (&expect, response) {
            (Expect::Unavailable, ToolResponse::Ok { result }) => {
                assert_eq!(
                    result["unavailable"]["capability"], "lookup",
                    "{name}: the unavailable answer names the capability"
                );
                assert_eq!(result["entries"], serde_json::json!([]), "{name}");
            }
            (Expect::Error { code, class }, ToolResponse::Error { error: info }) => {
                assert_eq!(&info.code, code, "{name}");
                assert_eq!(&info.class, class, "{name}");
                assert_eq!(info.class.exit_code(), class.exit_code(), "{name}");
            }
            (expect, response) => panic!("{name}: wrong response for {expect:?}: {response:?}"),
        }
    }

    // The session machinery's own failure kinds at the service
    // boundary: construction diagnostics keep their own code and class,
    // and `session-config-changed` refuses `ToolService::new` itself.
    // `lint-rule-error` is deliberately unpinned: its declared
    // `SourceError` class disagrees with the CLI wrappers, which exit
    // `USAGE` for every session-construction failure — the discrepancy
    // awaits a separate owner decision.
    let session_failures: &[(&str, ExitClass)] = &[
        ("invalid-selection", ExitClass::Usage),
        ("session-config-changed", ExitClass::SourceError),
    ];
    for (code, class) in session_failures {
        let diagnostic = match *code {
            "session-config-changed" => {
                let mut session = CompilerSession::new(SessionConfig {
                    input: InputSpec::Path(workshop_path()),
                    kind: SourceKind::Workshop,
                    ..SessionConfig::default()
                })
                .expect("session");
                session.config.locale = Some("fr-FR".to_string());
                let Err(diagnostic) = ToolService::new(&mut session) else {
                    panic!("a changed configuration must refuse the service");
                };
                diagnostic
            }
            "invalid-selection" => {
                let Err(diagnostic) = CompilerSession::new(SessionConfig {
                    selection: FindingSelection {
                        rule: Some("no-such-rule".to_string()),
                        ..FindingSelection::default()
                    },
                    ..SessionConfig::default()
                }) else {
                    panic!("an unknown rule must refuse the session");
                };
                diagnostic
            }
            _ => unreachable!("{code} has no session-boundary trigger"),
        };
        assert_eq!(diagnostic.code, *code);
        assert_eq!(diagnostic.class, *class);
        assert_eq!(diagnostic.class.exit_code(), class.exit_code());
    }
}

/// One scripted LPP provider for the `lookup` table: `argv[1]` is
/// `<phase>:<behavior>` where `init:` fails the `lpp/initialize` answer
/// and `op:` fails `lpp/lookup`; `init:nolookup` negotiates without the
/// `lookup` capability. Behaviors: `refusal[:<refusalCode>]`,
/// `lpp:<kind>[:<details json>]`, `mismatch:<v1,v2>`,
/// `raw:<json-rpc code>`, `malformed`, `sleep`, `exit:<status>`.
#[cfg(unix)]
const MODE_PROVIDER: &str = r#"#!/usr/bin/env python3
import json, os, sys, time

phase, _, behavior = (sys.argv[1] if len(sys.argv) > 1 else "").partition(":")

def reply(id, result=None, error=None):
    message = {"jsonrpc": "2.0", "id": id}
    message.update({"error": error} if error else {"result": result})
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

def lpp_error(id, kind, details, text):
    reply(id, error={"code": -32000, "message": text,
                     "data": {"lpp": {"kind": kind, "details": details}}})

def negotiated(id, request, lookup):
    reply(id, {"protocolVersion": request["params"]["protocolVersion"],
               "serverInfo": {"name": "fake", "version": "0"},
               "languages": [{"id": "opy", "extensions": ["opy"]}],
               "capabilities": {"check": True, "compile": True,
                                "projectLoading": True, "lookup": lookup}})

def fail(id, behavior):
    head, _, rest = behavior.partition(":")
    if head == "refusal":
        details = {"refusalCode": rest} if rest else {}
        lpp_error(id, "refusal", details, "the provider refused")
    elif head == "lpp":
        kind, _, extra = rest.partition(":")
        lpp_error(id, kind, json.loads(extra) if extra else {}, "the provider failed")
    elif head == "mismatch":
        supported = rest.split(",") if rest else []
        lpp_error(id, "protocolVersionMismatch",
                  {"supportedProtocolVersions": supported}, "unsupported version")
    elif head == "raw":
        reply(id, error={"code": int(rest), "message": "transport-level error"})
    elif head == "malformed":
        sys.stdout.write("this is not a json-rpc message\n")
        sys.stdout.flush()
    elif head == "sleep":
        time.sleep(10)
        reply(id, {})
    elif head == "exit":
        os._exit(int(rest))
    else:
        reply(id, {})

for line in sys.stdin:
    try:
        request = json.loads(line)
    except ValueError:
        continue
    id, method = request.get("id"), request.get("method")
    if method == "lpp/initialize":
        if phase == "init":
            if behavior == "nolookup":
                negotiated(id, request, False)
            else:
                fail(id, behavior)
        else:
            negotiated(id, request, True)
    elif method == "lpp/lookup" and phase == "op":
        fail(id, behavior)
    elif method == "lpp/lookup":
        reply(id, {"entries": []})
    else:
        reply(id, {})
"#;
