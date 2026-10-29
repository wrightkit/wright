//! The failure-classification table for the language-service diagnostic
//! surface — the third table beside the driver-side `check_failures.rs`
//! and `lookup_failures.rs` tables, which cover the other two surfaces.
//! Every row drives `LanguageService::diagnostics` on an
//! open `.opy` buffer and pins the published diagnostic's `code` and its
//! `error` severity: this surface carries no exit class — a boundary
//! failure publishes one error diagnostic whose code is the only
//! classification that crosses it — so a wrapper that re-classifies the
//! failure (a refusal mapped to a capability gap, a session diagnostic
//! relabeled `provider-refusal-*`) fails its row.
//!
//! "Provider failure kinds" are the `wright_lpp::ProviderError`
//! taxonomy; "session failure kinds" are the session machinery's own
//! refusals — `CompilerSession::new` diagnostics carried verbatim. The
//! client-side session-state errors (`provider-not-initialized`,
//! `provider-already-initialized`, `provider-shutdown`) have no real
//! trigger after a successful connect, `provider-not-configured` cannot
//! be produced for the resolved `opy` id, `provider-io` has no
//! deterministic process trigger, and `session-config-changed` cannot be
//! reached — the service's session configuration is private and cloned
//! at construction.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use wright_language::document::Document;
use wright_language::{DiagnosticSeverity, LanguageService};

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-language-failures-{}-{}-{tag}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn open_opy(service: &mut LanguageService, dir: &Path) -> String {
    let uri = url::Url::from_file_path(dir.join("main.opy"))
        .expect("entry uri")
        .to_string();
    service.store.open(Document::new(
        uri.clone(),
        "rule 'x': pass\n".to_string(),
        dir.to_path_buf(),
    ));
    uri
}

fn registered(
    command: impl Into<PathBuf>,
    args: Vec<String>,
    timeout: Option<Duration>,
) -> wright_language::SessionConfig {
    let mut config = wright_language::SessionConfig::default();
    let mut provider = wright_lpp::ProviderConfig::new("opy", command, args);
    if let Some(timeout) = timeout {
        provider.request_timeout = timeout;
    }
    config
        .providers
        .register(provider)
        .expect("provider registers");
    config
}

/// `diagnostics` on an `.opy` document: every provider failure kind the
/// connect or `lpp/check` channel can produce, the negotiated-capability
/// refusal, and the session-construction failures — each pinned to the
/// `code` (and `error` severity) it publishes.
#[test]
fn opy_diagnostics_surface_every_failure_kind_with_its_code() {
    let mut cases: Vec<(&'static str, wright_language::SessionConfig, &'static str)> = vec![
        // The resolution/spawn failures need no provider process.
        (
            "provider-missing",
            wright_language::SessionConfig {
                opy_provider: wright_language::OpyProviderConfig::with_executable(
                    "/nonexistent/opy-provider",
                ),
                ..wright_language::SessionConfig::default()
            },
            "provider-missing",
        ),
        (
            "provider-spawn",
            registered("/nonexistent/opy-provider", Vec::new(), None),
            "provider-spawn",
        ),
        // Session-construction diagnostics keep their own codes — never
        // a fabricated `provider-refusal-*`.
        (
            "session:invalid-selection",
            wright_language::SessionConfig {
                selection: wright_driver::FindingSelection {
                    rule: Some("no-such-rule".to_string()),
                    ..wright_driver::FindingSelection::default()
                },
                ..wright_language::SessionConfig::default()
            },
            "invalid-selection",
        ),
        (
            "session:lint-rule-error",
            wright_language::SessionConfig {
                lint_rule_paths: vec![PathBuf::from("/nonexistent/rule.opy")],
                ..wright_language::SessionConfig::default()
            },
            "lint-rule-error",
        ),
    ];

    #[cfg(unix)]
    {
        let dir = temp_dir("provider-script");
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("fake-provider");
            std::fs::write(&path, MODE_PROVIDER).expect("provider script");
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
            path
        };
        let spec = |spec: &str| registered(script.clone(), vec![spec.to_string()], None);
        let timed = |spec: &str| {
            registered(
                script.clone(),
                vec![spec.to_string()],
                Some(Duration::from_millis(250)),
            )
        };
        cases.extend([
            // `lpp/initialize` failures (the connect channel) publish
            // `provider_failure_code` verbatim — no `unavailable` exists
            // on this surface.
            (
                "init:mismatch",
                spec("init:mismatch:1.0"),
                "protocol-version-mismatch",
            ),
            (
                "init:capability-unavailable",
                spec("init:lpp:capabilityUnavailable"),
                "capability-unavailable",
            ),
            ("init:exited", spec("init:exit:3"), "provider-exited"),
            (
                "init:refusal-coded",
                spec("init:refusal:init.tooBusy"),
                "provider-refusal-init.tooBusy",
            ),
            (
                "init:refusal-invalid",
                spec("init:refusal:invalid-selection"),
                "provider-refusal-invalid-selection",
            ),
            (
                "init:refusal-plain",
                spec("init:refusal"),
                "provider-refusal",
            ),
            (
                "init:refusal-config",
                spec("init:refusal:session-config-changed"),
                "session-config-changed",
            ),
            (
                "init:invalid-request",
                spec("init:lpp:invalidRequest"),
                "invalid-request",
            ),
            (
                "init:invalid-language",
                spec("init:lpp:invalidLanguage"),
                "invalid-language",
            ),
            (
                "init:invalid-document",
                spec("init:lpp:invalidDocument"),
                "invalid-document",
            ),
            (
                "init:invalid-entry",
                spec("init:lpp:invalidEntry"),
                "invalid-entry",
            ),
            (
                "init:invalid-position",
                spec("init:lpp:invalidPosition"),
                "invalid-position",
            ),
            (
                "init:invalid-artifact",
                spec("init:lpp:invalidArtifact"),
                "invalid-artifact",
            ),
            (
                "init:project-load-failed",
                spec("init:lpp:projectLoadFailed"),
                "project-load-failed",
            ),
            (
                "init:unknown-kind",
                spec("init:lpp:unknownFuture"),
                "lpp-error",
            ),
            ("init:jsonrpc", spec("init:raw:-32600"), "jsonrpc-error"),
            (
                "init:malformed",
                spec("init:malformed"),
                "provider-malformed",
            ),
            ("init:timeout", timed("init:sleep"), "provider-timeout"),
            // A provider that negotiated without `check` declines
            // through the surface's own unavailability code.
            (
                "init:no-check-capability",
                spec("init:nocheck"),
                "source-provider-unavailable",
            ),
            // `lpp/check` failures (the request channel) classify
            // through the same `provider_failure_code`.
            (
                "op:refusal-coded",
                spec("op:refusal:check.tooLarge"),
                "provider-refusal-check.tooLarge",
            ),
            (
                "op:refusal-invalid",
                spec("op:refusal:invalid-selection"),
                "provider-refusal-invalid-selection",
            ),
            ("op:refusal-plain", spec("op:refusal"), "provider-refusal"),
            (
                "op:refusal-config",
                spec("op:refusal:session-config-changed"),
                "session-config-changed",
            ),
            (
                "op:invalid-request",
                spec("op:lpp:invalidRequest"),
                "invalid-request",
            ),
            (
                "op:invalid-language",
                spec("op:lpp:invalidLanguage"),
                "invalid-language",
            ),
            (
                "op:invalid-document",
                spec("op:lpp:invalidDocument"),
                "invalid-document",
            ),
            (
                "op:invalid-entry",
                spec("op:lpp:invalidEntry"),
                "invalid-entry",
            ),
            (
                "op:invalid-position",
                spec("op:lpp:invalidPosition"),
                "invalid-position",
            ),
            (
                "op:invalid-artifact",
                spec("op:lpp:invalidArtifact"),
                "invalid-artifact",
            ),
            (
                "op:project-load-failed",
                spec("op:lpp:projectLoadFailed"),
                "project-load-failed",
            ),
            (
                "op:capability-unavailable",
                spec("op:lpp:capabilityUnavailable"),
                "capability-unavailable",
            ),
            (
                "op:protocol-mismatch",
                spec("op:lpp:protocolVersionMismatch"),
                "protocol-version-mismatch",
            ),
            ("op:unknown-kind", spec("op:lpp:unknownFuture"), "lpp-error"),
            ("op:jsonrpc", spec("op:raw:-32600"), "jsonrpc-error"),
            ("op:malformed", spec("op:malformed"), "provider-malformed"),
            ("op:exited", spec("op:exit:3"), "provider-exited"),
            ("op:timeout", timed("op:sleep"), "provider-timeout"),
        ]);
    }

    for (name, config, code) in cases {
        let dir = temp_dir(&format!("service-{name}"));
        let mut service = LanguageService::with_config(dir.clone(), config);
        let uri = open_opy(&mut service, &dir);
        let diagnostics = service.diagnostics(&uri);
        assert_eq!(diagnostics.len(), 1, "{name}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, code, "{name}");
        assert_eq!(
            diagnostics[0].severity,
            DiagnosticSeverity::Error,
            "{name}: boundary failures publish as errors"
        );
        std::fs::remove_dir_all(&dir).expect("cleanup");
    }
}

/// The scripted LPP provider for the table: `argv[1]` is
/// `<phase>:<behavior>` where `init:` fails the `lpp/initialize` answer
/// and `op:` fails `lpp/check`; `init:nocheck` negotiates without the
/// `check` capability. Behaviors: `refusal[:<refusalCode>]`,
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

def negotiated(id, request, check):
    reply(id, {"protocolVersion": request["params"]["protocolVersion"],
               "serverInfo": {"name": "fake", "version": "0"},
               "languages": [{"id": "opy", "extensions": ["opy"]}],
               "capabilities": {"check": check}})

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
            if behavior == "nocheck":
                negotiated(id, request, False)
            else:
                fail(id, behavior)
        else:
            negotiated(id, request, True)
    elif method == "lpp/check" and phase == "op":
        fail(id, behavior)
    elif method == "lpp/check":
        reply(id, {"documents": []})
    else:
        reply(id, {})
"#;
