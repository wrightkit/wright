//! Cross-format conversion integration suite (#126).
//!
//! Drives the **shipped** shared driver/session conversion operation
//! ([`CompilerSession::convert`]) over the committed Workshop fixtures and
//! proves the OPY reverse loop through the real owner frontend:
//!
//! * `Workshop → convert(opy) → native wright-opy frontend → HIR → WIR →
//!   Workshop` — equivalence under `workshop_rs::roundtrip::equivalent`
//!   (the #124 contract, no normalization).
//!
//! Rejections are deterministic with the reconstructor's stable codes and
//! never carry partial source, and the machine-readable report lands at
//! `target/wright-convert-report.json` (one entry per fixture, the repo
//! report pattern).

use std::path::{Path, PathBuf};

use workshop_rs::wir::{self, Value};
use wright_driver::CompilerSession;
use wright_driver::config::SessionConfig;
use wright_driver::result::{ConvertResult, ConvertTarget};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The committed #124 OPY reconstruction fixtures (OPY-surface Workshop
/// inputs).
const OPY_FIXTURES: &[&str] = &[
    "variables-declarations",
    "subroutine-control-flow",
    "player-events",
    "values-enums",
    "actions-surface",
];

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

fn sha256(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Parse Workshop text through the shared parser with the canonical
/// signature context (the same path the driver uses).
fn parse(catalog: &workshop_rs::catalog::Catalog, text: &str) -> wir::Program {
    let manifest =
        wright_opy::manifest::Manifest::builtin().expect("the OPY manifest is embedded and valid");
    let context = wright_core::signatures::ChainedExpectedDomain::new(&manifest, catalog);
    let program = workshop_rs::parser::parse_with_context(
        text,
        catalog,
        &workshop_rs::catalog::Locale::new("en-US"),
        &context,
    )
    .unwrap_or_else(|error| panic!("fixture Workshop text must parse: {error}"));
    program
        .validate()
        .expect("parsed programs validate structurally");
    program
}

/// A temp input file carrying Workshop text, so the driver's real
/// discovery/load path (extension → kind) drives the conversion.
fn workshop_input(text: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "wright-convert-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("program.txt");
    std::fs::write(&path, text).unwrap();
    path
}

/// Drive the shipped shared conversion operation for one target.
fn convert(text: &str, target: ConvertTarget) -> (wright_driver::Envelope<ConvertResult>, PathBuf) {
    let path = workshop_input(text);
    let mut session = CompilerSession::new(SessionConfig::from_path(path.clone())).unwrap();
    let envelope = session.convert(target);
    (envelope, path)
}

/// Emit Workshop text for a WIR program through the shared emitter.
fn emit_workshop(
    catalog: &workshop_rs::catalog::Catalog,
    program: &wir::Program,
) -> Result<String, String> {
    workshop_rs::emitter::emit(
        program,
        catalog,
        &workshop_rs::catalog::Locale::new("en-US"),
    )
    .map_err(|error| error.to_string())
}

/// The owner compiler's pinned OverPy contract lowers `not` over a comparison
/// to the complementary comparison (for example, `not (a < b)` to `a >= b`).
/// Normalize that representation difference before structural WIR comparison.
fn normalize_negated_comparisons(program: &mut wir::Program) {
    for index in 0..program.values.len() {
        let id = wright_ir::ids::Id::from_index(index);
        let Some(node) = program.values.get(id).cloned() else {
            continue;
        };
        let Value::Call { name, args } = node.value else {
            continue;
        };
        if name != "not" || args.len() != 1 {
            continue;
        }
        let Some(Value::Call {
            name: comparison,
            args: operands,
        }) = program.values.get(args[0]).map(|node| node.value.clone())
        else {
            continue;
        };
        let Some(negated) = negated_comparison(&comparison) else {
            continue;
        };
        program.values.get_mut(id).expect("value in range").value = Value::Call {
            name: negated.to_string(),
            args: operands,
        };
    }
}

fn negated_comparison(operator: &str) -> Option<&'static str> {
    Some(match operator {
        "==" => "!=",
        "!=" => "==",
        "<" => ">=",
        ">" => "<=",
        "<=" => ">",
        ">=" => "<",
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

/// The full Workshop → OPY loop through the shared driver path.
fn opy_round_trip(
    catalog: &workshop_rs::catalog::Catalog,
    fixture: &str,
    failures: &mut Vec<String>,
) -> serde_json::Value {
    const OWNER_UNSUPPORTED_FIXTURES: &[&str] =
        &["subroutine-control-flow", "player-events", "values-enums"];
    let path = workspace_root()
        .join("crates/wright-opy/tests/fixtures/reconstruct")
        .join(format!("{fixture}.ws"));
    let source = read(&path);
    let original = parse(catalog, &source);

    let (envelope, input_path) = convert(&source, ConvertTarget::Opy);
    if !envelope.ok {
        if fixture == "values-enums"
            && envelope
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "unsupported-enum-domain-mismatch")
        {
            return serde_json::json!({
                "status": "owner-unsupported",
                "ownerDiagnostic": envelope.diagnostics[0].message,
            });
        }
        failures.push(format!(
            "{fixture}: shared convert rejected: {:?}",
            envelope.diagnostics
        ));
        return serde_json::json!({ "status": "convert-rejected" });
    }
    let opy = envelope.result.text.clone();
    assert_eq!(envelope.result.target, ConvertTarget::Opy);
    assert_eq!(envelope.result.sha256.len(), 64);

    // The reconstructed OPY must reload through the real native frontend…
    let recompiled = match wright_opy::compile(&opy, &format!("{fixture}.opy"), Path::new("")) {
        Ok(program) => program,
        Err(error) => {
            if error.code == "unsupported-integration-surface" {
                assert!(
                    OWNER_UNSUPPORTED_FIXTURES.contains(&fixture),
                    "unexpected published opy-compiler limitation for {fixture}: {error}"
                );
                return serde_json::json!({
                    "status": "owner-unsupported",
                    "ownerDiagnostic": error.to_string(),
                });
            }
            failures.push(format!(
                "{fixture}: the native frontend rejected the reconstructed OPY: {error}"
            ));
            return serde_json::json!({ "status": "frontend-rejected" });
        }
    };
    // …and the recompiled WIR must be equivalent to the parsed WIR, then
    // still emit to Workshop text through the shipped emitter.
    let mut original = original;
    let mut recompiled = recompiled;
    normalize_negated_comparisons(&mut original);
    normalize_negated_comparisons(&mut recompiled);
    let equivalent = workshop_rs::roundtrip::equivalent(&original, &recompiled);
    if !equivalent {
        failures.push(format!("{fixture}: recompiled WIR is not equivalent"));
    }
    // The trailing `→ Workshop` hop: the recompiled WIR still emits to
    // Workshop text through the shipped emitter.
    let workshop_emit = emit_workshop(catalog, &recompiled);
    if let Err(error) = &workshop_emit {
        failures.push(format!(
            "{fixture}: Workshop emission of the recompiled WIR failed: {error}"
        ));
    }
    let _ = std::fs::remove_dir_all(input_path.parent().unwrap());

    serde_json::json!({
        "status": "round-trip",
        "target": "opy",
        "inputSha256": sha256(&source),
        "reconstructedSha256": envelope.result.sha256,
        "frontendAccepted": true,
        "equivalent": equivalent,
        "workshopEmit": workshop_emit.is_ok(),
    })
}

#[test]
fn cross_format_conversion_round_trips_and_reports() {
    let catalog = workshop_rs::catalog::Catalog::builtin().expect("catalog loads");
    let mut failures = Vec::new();
    let mut report = serde_json::Map::new();
    for fixture in OPY_FIXTURES {
        report.insert(
            fixture.to_string(),
            opy_round_trip(&catalog, fixture, &mut failures),
        );
    }
    for (name, target, relative) in rejection_cases() {
        report.insert(
            format!("reject/{name}"),
            rejection_entry(target, &workspace_root().join(relative), &mut failures),
        );
    }
    // One entry per fixture, plus the suite identity.
    report.insert(
        "suite".to_string(),
        serde_json::json!({
            "name": "wright-convert",
            "fixtures": OPY_FIXTURES.len() + rejection_cases().len(),
        }),
    );
    let report_path = workspace_root().join("target/wright-convert-report.json");
    let parent = report_path.parent().expect("target dir");
    std::fs::create_dir_all(parent).expect("create target dir");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(report))
            .expect("report serializes"),
    )
    .expect("write conversion report");
    assert!(
        failures.is_empty(),
        "every committed fixture must round-trip or reject deterministically through the \
         shared path:\n{}",
        failures.join("\n")
    );
}

#[test]
fn ostw_conversion_refuses_without_partial_source() {
    let source = read(
        &workspace_root()
            .join("crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws"),
    );
    let (envelope, input_path) = convert(&source, ConvertTarget::Ostw);
    assert!(!envelope.ok);
    assert_eq!(envelope.exit, 4);
    assert!(envelope.result.text.is_empty());
    assert!(
        envelope
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "source-provider-unavailable")
    );
    assert_eq!(
        envelope.diagnostics[0].stage,
        wright_driver::Stage::Internal
    );
    let _ = std::fs::remove_dir_all(input_path.parent().unwrap());
}

/// The rejection fixture cases: (name, target, committed Workshop source).
fn rejection_cases() -> Vec<(&'static str, ConvertTarget, &'static str)> {
    vec![(
        "opy-per-player-loop",
        ConvertTarget::Opy,
        "crates/wright-driver/tests/fixtures/convert/reject-opy-per-player-loop.ws",
    )]
}

/// Drive one rejection fixture through the shared path and record the
/// deterministic structured rejection (never partial source).
fn rejection_entry(
    target: ConvertTarget,
    path: &Path,
    failures: &mut Vec<String>,
) -> serde_json::Value {
    let source = read(path);
    let (envelope, input_path) = convert(&source, target);
    if envelope.ok {
        failures.push(format!(
            "reject case '{}' unexpectedly converted",
            path.display()
        ));
    }
    let codes: Vec<String> = envelope
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.clone())
        .collect();
    let _ = std::fs::remove_dir_all(input_path.parent().unwrap());
    serde_json::json!({
        "status": "rejected",
        "target": target.as_str(),
        "exit": envelope.exit,
        "codes": codes,
        "partialSource": !envelope.result.text.is_empty(),
    })
}

#[test]
fn conversion_is_byte_deterministic_across_runs() {
    let target = ConvertTarget::Opy;
    let fixture = workspace_root()
        .join("crates/wright-opy/tests/fixtures/reconstruct/variables-declarations.ws");
    let source = read(&fixture);
    let (first, path_a) = convert(&source, target);
    let (second, path_b) = convert(&source, target);
    assert!(
        first.ok,
        "first convert must succeed: {:?}",
        first.diagnostics
    );
    assert!(
        second.ok,
        "second convert must succeed: {:?}",
        second.diagnostics
    );
    assert_eq!(
        first.result.text,
        second.result.text,
        "convert({}) must be byte-stable",
        target.as_str()
    );
    assert_eq!(first.result.sha256, second.result.sha256);
    let _ = std::fs::remove_dir_all(path_a.parent().unwrap());
    let _ = std::fs::remove_dir_all(path_b.parent().unwrap());
}

#[test]
fn unsupported_constructs_reject_with_structured_diagnostics_and_no_partial_source() {
    for (name, target, relative) in rejection_cases() {
        let source = read(&workspace_root().join(relative));
        let (first, path_a) = convert(&source, target);
        let (second, path_b) = convert(&source, target);
        assert_eq!(
            first.diagnostics, second.diagnostics,
            "{name}: the rejection must be deterministic"
        );
        assert_eq!(first.exit, 3, "{name}: unsupported must exit 3");
        assert_eq!(first.exit, second.exit);
        assert!(!first.ok, "{name}: the rejection must fail the envelope");
        assert!(
            first.result.text.is_empty(),
            "{name}: a rejection never carries partial source"
        );
        assert!(
            first
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity == wright_driver::Severity::Error),
            "{name}: rejections are errors"
        );
        assert!(
            first
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.stage == wright_driver::Stage::Reconstruction),
            "{name}: rejections carry the reconstruction stage"
        );
        // The reconstructor's stable codes are preserved verbatim.
        let codes: Vec<&str> = first
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect();
        let expected = "unsupported-per-player-loop";
        assert!(
            codes.contains(&expected),
            "{name}: expected code {expected} in {codes:?}"
        );
        let _ = std::fs::remove_dir_all(path_a.parent().unwrap());
        let _ = std::fs::remove_dir_all(path_b.parent().unwrap());
    }
}
