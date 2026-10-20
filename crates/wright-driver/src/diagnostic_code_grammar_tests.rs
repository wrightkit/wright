//! Contract test for the diagnostic-code grammar declared in
//! `docs/cli/machine-contract.md`: every diagnostic code wright-driver
//! emits is a whitespace-free token of `.`-separated
//! `[a-z0-9]+(-[a-z0-9]+)*` segments, except that an owner- or
//! provider-typed token inside a code keeps its external spelling
//! verbatim (whitespace-free still applies).
//!
//! The test enumerates the vocabulary rather than sampling it:
//!
//! - every fixed code literal is collected from the code positions in
//!   this crate's own source (`driver_codes`);
//! - `unknown-<kind>` is driven through `workshop_diag` with every
//!   `kind` the pinned `workshop-rs` engine can report — the census
//!   (`engine_kinds`) reads the engine's own production source and
//!   fails loudly on any site it cannot classify, per the acceptance
//!   rule "enumerate or stop", never sample;
//! - `workshop.<kind>.<name>` is driven through `map_issue` with real
//!   `SemanticIssue` values produced by the engine's `inspect` for
//!   every `IncompletenessKind` variant;
//! - provider failure codes are driven through `provider_failure_code`
//!   with every `ProviderError`/`LppErrorKind`/`LocalProviderErrorKind`
//!   variant and every refusal shape;
//! - a code position the census cannot attribute to one of these
//!   shapes fails the test instead of going unchecked.
//!
//! `#[non_exhaustive]` types (`SemanticIssue`, `CatalogError`) cannot
//! be constructed here, so their mappings are exercised through the
//! engine's public producers (`inspect`) and the fixed-literal census.

mod driver_codes;
mod engine_kinds;
mod source;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use workshop_rs::catalog::{Catalog, Locale};
use workshop_rs::{Action, Event, Program, Rule, Value, WorkshopError};

use crate::config::SourceKind;
use crate::diag::Origin;
use crate::input::{InputTarget, ResolvedInput};

// ===== the declared grammar =====

/// `[a-z0-9]+(-[a-z0-9]+)*`: nonempty runs of lowercase ASCII
/// alphanumerics joined by single hyphens — so leading, trailing, and
/// repeated hyphens all fail.
fn segment_ok(segment: &str) -> bool {
    !segment.is_empty()
        && segment.split('-').all(|run| {
            !run.is_empty()
                && run
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

/// The grammar for a wholly Wright-owned code: `.`-separated segments,
/// each `[a-z0-9]+(-[a-z0-9]+)*`.
fn wright_code_ok(code: &str) -> bool {
    !code.is_empty() && code.split('.').all(segment_ok)
}

fn assert_wright_code(code: &str, site: &str) {
    assert!(
        wright_code_ok(code),
        "{site}: diagnostic code {code:?} violates the declared grammar"
    );
}

/// The one guarantee that also covers codes carrying an owner- or
/// provider-typed token: the emitted code is still one whitespace-free
/// token.
fn assert_whitespace_free(code: &str, site: &str) {
    assert!(!code.is_empty(), "{site}: empty diagnostic code");
    assert!(
        !code.chars().any(char::is_whitespace),
        "{site}: diagnostic code {code:?} contains whitespace"
    );
}

// ===== shared fixtures =====

fn origin() -> Origin {
    Origin {
        kind: "workshop".to_string(),
        locale: Some("en-us".to_string()),
    }
}

fn resolved_input() -> ResolvedInput {
    ResolvedInput {
        kind: SourceKind::Workshop,
        text: String::new(),
        path: None,
        target: InputTarget::File,
        root: PathBuf::from("."),
        cwd: PathBuf::from("."),
        display: "grammar-probe.workshop".to_string(),
        identity: "grammar-probe".to_string(),
        origin: origin(),
    }
}
// ===== the tests =====

/// Every fixed code literal in a production code position matches the
/// declared grammar, and every non-literal code position is one of the
/// enumerated dynamic producers.
#[test]
fn fixed_codes_obey_the_declared_grammar() {
    let (codes, unclassified) = driver_codes::driver_code_census();
    assert!(
        unclassified.is_empty(),
        "code positions the census cannot classify — enumerate them rather \
         than letting them go unchecked:\n  {}",
        unclassified.join("\n  ")
    );
    assert!(!codes.is_empty(), "the census found no fixed codes");
    let unique: BTreeSet<(&PathBuf, usize, &String)> = codes
        .iter()
        .map(|site| (&site.file, site.line, &site.code))
        .collect();
    let vocabulary: BTreeSet<&String> = unique.iter().map(|(_, _, code)| *code).collect();
    eprintln!("enumerated {} fixed code literals:", vocabulary.len());
    for code in &vocabulary {
        eprintln!("  {code}");
    }
    for (file, line, code) in &unique {
        assert_wright_code(code, &format!("{}:{line}", file.display()));
    }
    // Sites only reachable through specific scans prove those scans run —
    // pinned to file + code so vocabulary membership cannot satisfy them
    // accidentally:
    // - `lint-package-not-found` only enters via the `.code =` scan;
    // - `lint-package-remote` only enters when the bare `code` argument of
    //   `Diagnostic::error(code, …)` resolves to its `let` binding and the
    //   binding's `unwrap_or("…")` fallback literal is censused;
    // - `lookup-failed` sits in a `return refusal(` call the earlier guard
    //   skipped;
    // - `clipboard-failed`, `ambiguous-symbol`, and `rename-refused` pin the
    //   wright-cli, wright-analyzer, and wright-language trees.
    for (path_part, code) in [
        ("lint/package.rs", "lint-package-not-found"),
        ("lint/package.rs", "lint-package-remote"),
        ("lookup.rs", "lookup-failed"),
        ("wright-cli", "clipboard-failed"),
        ("wright-analyzer", "ambiguous-symbol"),
        ("wright-language", "rename-refused"),
    ] {
        assert!(
            unique.iter().any(|(file, _, found)| {
                file.to_string_lossy().contains(path_part) && found.as_str() == code
            }),
            "the census missed `{code}` at a `{path_part}` site"
        );
    }
}

/// `unknown-<kind>`: every `kind` the pinned engine can report, driven
/// through the real `WorkshopError` → `workshop_diag` mapping in all three
/// reporting shapes.
#[test]
fn workshop_error_kind_codes_obey_the_grammar() {
    let resolved = resolved_input();
    let locale = Locale::new("en-us");
    let kinds = engine_kinds::engine_error_kinds();
    assert!(
        kinds.len() >= 15,
        "the engine kind census is suspiciously small: {kinds:?}"
    );
    eprintln!("enumerated {} engine error kinds:", kinds.len());
    for kind in &kinds {
        eprintln!("  {kind}");
    }
    for kind in kinds {
        // `WorkshopError` kinds are `&'static str`; leaking the small census
        // set keeps the test honest without an API change.
        let kind: &'static str = Box::leak(kind.into_boxed_str());
        for error in [
            WorkshopError::unknown(kind, "probe", locale.clone(), None),
            WorkshopError::unknown(kind, "probe", locale.clone(), None)
                .with_candidates(vec!["candidate".to_string()]),
            WorkshopError::not_detected(kind, "probe", None),
        ] {
            let diag = crate::session::workshop_diag(error, &resolved);
            let expected = format!("unknown-{}", kind.replace(' ', "-"));
            assert_eq!(diag.code, expected, "kind {kind:?} maps to {expected:?}");
            assert_wright_code(&diag.code, &format!("WorkshopError kind {kind:?}"));
        }
    }
}

/// The remaining `WorkshopError` variants reach fixed codes. `Catalog` is
/// not constructible outside the engine; its `catalog-error` literal is
/// enumerated by the fixed-code census instead.
#[test]
fn workshop_error_variant_codes_obey_the_grammar() {
    let resolved = resolved_input();
    let locale = Locale::new("en-us");
    let cases = [
        WorkshopError::malformed("probe", None),
        WorkshopError::unsupported("probe", None),
        WorkshopError::warning("probe", None),
        WorkshopError::MissingMapping {
            kind: "value",
            id: "probe".to_string(),
            locale: locale.clone(),
        },
    ];
    for error in cases {
        let diag = crate::session::workshop_diag(error, &resolved);
        assert_wright_code(&diag.code, "WorkshopError variant");
    }
}

/// `workshop.<kind>.<name>`: every `IncompletenessKind` the engine reports,
/// exercised end-to-end through the engine's `inspect` → `map_issue`
/// projection on a program built with the public `Program` API. Residual
/// names carrying spaces prove the identity sanitizer keeps segments
/// grammar-valid.
#[test]
fn completeness_residual_codes_obey_the_grammar() {
    let catalog = Catalog::builtin_ref().expect("builtin catalog");
    let mut program = Program::new();
    program.settings = Some(workshop_rs::settings::Settings {
        span: None,
        children: vec![workshop_rs::settings::SettingsNode::Raw {
            name: "Extension Thing".to_string(),
            value: "1".to_string(),
            span: None,
        }],
    });
    program.rule(
        Rule::new("probe", Event::Global)
            .action(Action::call("rawWorkshopAction", []))
            .action(Action::call("future Unknown Action", []))
            // Identity is free text: repeated spaces, parentheses, edge
            // punctuation, and a punctuation-only name must still emit a
            // grammar-valid slug.
            .action(Action::call("double  space", []))
            .action(Action::call("paren(name)", []))
            .action(Action::call("-leading -trailing-", []))
            .action(Action::call("!*!", []))
            .action(Action::SetGlobalVariable {
                variable: "g".to_string(),
                value: Value::call("future Value", [Value::Number(1.0)]),
            })
            .action(Action::SetGlobalVariable {
                variable: "h".to_string(),
                value: Value::call("val(ue)  name", [Value::Number(1.0)]),
            }),
    );
    let issues = workshop_rs::rules::inspect(&program, catalog);
    assert!(
        !issues.is_empty(),
        "the probe program must produce residuals"
    );

    // Compile-time exhaustiveness gate for the kind enum.
    for issue in &issues {
        match issue.kind {
            workshop_rs::rules::IncompletenessKind::RawSetting
            | workshop_rs::rules::IncompletenessKind::UnknownAction
            | workshop_rs::rules::IncompletenessKind::UnknownValue
            | workshop_rs::rules::IncompletenessKind::OpaqueAction => {}
        }
    }

    let mut seen = Vec::new();
    let mut codes = Vec::new();
    for issue in &issues {
        if !seen.contains(&issue.kind) {
            seen.push(issue.kind);
        }
        let diag =
            crate::session::completeness::map_issue(issue, Path::new("probe.workshop"), &origin());
        assert!(
            diag.code.starts_with("workshop."),
            "residual code {:?} must be workshop.<kind>.<name>",
            diag.code
        );
        assert_wright_code(&diag.code, &format!("residual {issue:?}"));
        codes.push(diag.code);
    }
    for expected in [
        "workshop.unknown-action.double-space",
        "workshop.unknown-action.paren-name",
        "workshop.unknown-action.leading-trailing",
        // A punctuation-only name sanitises to an empty slug — the name
        // segment is omitted rather than left dangling.
        "workshop.unknown-action",
        "workshop.unknown-value.val-ue-name",
    ] {
        assert!(
            codes.iter().any(|code| code == expected),
            "no code {expected}"
        );
    }
    for kind in [
        workshop_rs::rules::IncompletenessKind::RawSetting,
        workshop_rs::rules::IncompletenessKind::UnknownAction,
        workshop_rs::rules::IncompletenessKind::UnknownValue,
        workshop_rs::rules::IncompletenessKind::OpaqueAction,
    ] {
        assert!(
            seen.contains(&kind),
            "no SemanticIssue exercised {kind:?}; the census must cover every kind"
        );
    }
}

/// Provider failure codes: every `ProviderError` variant, every
/// `LppErrorKind`, every `LocalProviderErrorKind`, and every refusal shape —
/// including the owner-spelled `refusalCode` kept verbatim under the
/// `provider-refusal-` prefix.
#[test]
fn provider_failure_codes_obey_the_contract() {
    use wright_lpp::{LocalProviderErrorKind, LppErrorKind, ProviderError};

    // Compile-time exhaustiveness gate.
    let probe = ProviderError::AlreadyInitialized;
    match probe {
        ProviderError::NotConfigured { .. }
        | ProviderError::Spawn { .. }
        | ProviderError::Io { .. }
        | ProviderError::Exited { .. }
        | ProviderError::Timeout { .. }
        | ProviderError::Malformed { .. }
        | ProviderError::JsonRpc { .. }
        | ProviderError::Lpp(_)
        | ProviderError::NotInitialized { .. }
        | ProviderError::AlreadyInitialized
        | ProviderError::ShutDown { .. }
        | ProviderError::ProtocolVersionMismatch { .. }
        | ProviderError::Local { .. } => {}
    }

    let mut errors = vec![
        ProviderError::NotConfigured {
            language_id: "opy".to_string(),
        },
        ProviderError::Spawn {
            message: "probe".to_string(),
        },
        ProviderError::Io {
            message: "probe".to_string(),
        },
        ProviderError::Exited {
            status: Some(1),
            message: "probe".to_string(),
        },
        ProviderError::Timeout {
            method: "probe".to_string(),
            duration: std::time::Duration::from_secs(1),
        },
        ProviderError::Malformed {
            detail: "probe".to_string(),
        },
        ProviderError::JsonRpc {
            code: -1,
            message: "probe".to_string(),
        },
        ProviderError::NotInitialized {
            method: "probe".to_string(),
        },
        ProviderError::AlreadyInitialized,
        ProviderError::ShutDown {
            method: "probe".to_string(),
        },
        ProviderError::ProtocolVersionMismatch {
            supported: vec!["1.0".to_string()],
            message: "probe".to_string(),
        },
    ];
    for kind in [
        LppErrorKind::ProtocolVersionMismatch,
        LppErrorKind::InvalidRequest,
        LppErrorKind::InvalidLanguage,
        LppErrorKind::InvalidDocument,
        LppErrorKind::InvalidEntry,
        LppErrorKind::ProjectLoadFailed,
        LppErrorKind::InvalidPosition,
        LppErrorKind::InvalidArtifact,
        LppErrorKind::CapabilityUnavailable,
        LppErrorKind::Refusal,
        LppErrorKind::Unknown("probe".to_string()),
    ] {
        // Compile-time exhaustiveness gate.
        match kind {
            LppErrorKind::ProtocolVersionMismatch
            | LppErrorKind::InvalidRequest
            | LppErrorKind::InvalidLanguage
            | LppErrorKind::InvalidDocument
            | LppErrorKind::InvalidEntry
            | LppErrorKind::ProjectLoadFailed
            | LppErrorKind::InvalidPosition
            | LppErrorKind::InvalidArtifact
            | LppErrorKind::CapabilityUnavailable
            | LppErrorKind::Refusal
            | LppErrorKind::Unknown(_) => {}
        }
        errors.push(ProviderError::lpp(kind, serde_json::json!({}), "probe"));
    }
    for kind in [
        LocalProviderErrorKind::Missing,
        LocalProviderErrorKind::UnsupportedPlatform,
        LocalProviderErrorKind::Offline,
        LocalProviderErrorKind::Download,
        LocalProviderErrorKind::Integrity,
        LocalProviderErrorKind::Install,
    ] {
        // Compile-time exhaustiveness gate.
        match kind {
            LocalProviderErrorKind::Missing
            | LocalProviderErrorKind::UnsupportedPlatform
            | LocalProviderErrorKind::Offline
            | LocalProviderErrorKind::Download
            | LocalProviderErrorKind::Integrity
            | LocalProviderErrorKind::Install => {}
        }
        errors.push(ProviderError::Local {
            kind,
            message: "probe".to_string(),
        });
    }
    // Refusal shapes: a provider-spelled refusalCode, a Wright-originated
    // refusal code kept verbatim, and a refusal carrying no refusalCode.
    errors.push(ProviderError::lpp(
        LppErrorKind::Refusal,
        serde_json::json!({"refusalCode": "compile.tooLarge"}),
        "probe",
    ));
    errors.push(ProviderError::lpp(
        LppErrorKind::Refusal,
        serde_json::json!({"refusalCode": "session-config-changed"}),
        "probe",
    ));
    errors.push(ProviderError::lpp(
        LppErrorKind::Refusal,
        serde_json::json!({}),
        "probe",
    ));

    for error in &errors {
        let code = crate::source_provider::provider_failure_code(error);
        assert_whitespace_free(&code, &format!("{error:?}"));
        if let Some(rest) = code.strip_prefix("provider-refusal-") {
            // `provider-refusal-` is Wright's namespace; the remainder is an
            // owner-spelled refusalCode kept verbatim by contract.
            assert!(!rest.is_empty(), "{error:?}: empty refusal code tail");
            assert_wright_code("provider-refusal", "provider-refusal- prefix");
        } else {
            assert_wright_code(&code, &format!("{error:?}"));
        }
    }

    // The refusal-code passthroughs pin their documented spellings.
    let refusal = ProviderError::lpp(
        LppErrorKind::Refusal,
        serde_json::json!({"refusalCode": "compile.tooLarge"}),
        "probe",
    );
    assert_eq!(
        crate::source_provider::provider_failure_code(&refusal),
        "provider-refusal-compile.tooLarge"
    );
    let wright_refusal = ProviderError::lpp(
        LppErrorKind::Refusal,
        serde_json::json!({"refusalCode": "session-config-changed"}),
        "probe",
    );
    assert_eq!(
        crate::source_provider::provider_failure_code(&wright_refusal),
        "session-config-changed"
    );
    let bare_refusal = ProviderError::lpp(LppErrorKind::Refusal, serde_json::json!({}), "probe");
    assert_eq!(
        crate::source_provider::provider_failure_code(&bare_refusal),
        "provider-refusal"
    );
}
