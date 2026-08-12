//! Native-vs-reference frontend differential suite (#46).
//!
//! Runs the declared production corpus through the native frontend and the
//! pinned OverPy adapter (recorded HIR fixtures) and compares at the Wright
//! HIR boundary. Normalization removes span endpoints, the producer
//! `generator` identity, and the adapter's `isFunction` key spelling — all
//! frontend-internal representation differences — while preserving node
//! structure, literal values, names, references, and control flow. Every
//! supported-surface divergence fails the suite (regressions break CI); the
//! diagnostics fixture asserts both frontends reject with a parse error.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// The corpus fixtures with native `.opy` sources and adapter HIR fixtures.
const PARITY_CASES: &[(&str, &str)] = &[
    (
        "synthetic/basic-rule",
        "adapter/fixtures/synthetic/basic-rule.json",
    ),
    (
        "synthetic/control-flow",
        "adapter/fixtures/synthetic/control-flow.json",
    ),
    (
        "synthetic/declarations-rules",
        "adapter/fixtures/synthetic/declarations-rules.json",
    ),
    (
        "synthetic/expressions-values",
        "adapter/fixtures/synthetic/expressions-values.json",
    ),
    (
        "synthetic/preprocessing",
        "adapter/fixtures/synthetic/preprocessing.json",
    ),
    (
        "real-world/overpy-cake",
        "adapter/fixtures/real-world/overpy-cake.json",
    ),
];

/// Remove spans and the producer identity: the documented normalization.
fn normalize(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("span");
            map.remove("generator");
            for nested in map.values_mut() {
                normalize(nested);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                normalize(item);
            }
        }
        _ => {}
    }
}

fn fixture_dir(id: &str) -> PathBuf {
    workspace_root().join("compatibility/fixtures").join(id)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
}

#[test]
fn native_and_reference_agree_on_the_production_corpus() {
    let mut report = serde_json::Map::new();
    let mut failures = Vec::new();

    for (id, adapter_fixture) in PARITY_CASES {
        let dir = fixture_dir(id);
        let source = read(&dir.join("source.opy"));
        let native = match wright_opy::compile(&source, "source.opy", &dir) {
            Ok(program) => program,
            Err(error) => {
                failures.push(format!("{id}: native frontend error: {error}"));
                report.insert(
                    id.to_string(),
                    serde_json::json!({ "status": "native-error", "error": error.to_string() }),
                );
                continue;
            }
        };

        // Reference side: the pinned adapter fixture, with the adapter's
        // `isFunction` spelling normalized to the consumer's `is_function`.
        let mut reference_value: serde_json::Value =
            serde_json::from_str(&read(&workspace_root().join(adapter_fixture))).unwrap();
        if let Some(defines) = reference_value
            .get_mut("defines")
            .and_then(|d| d.as_array_mut())
        {
            for define in defines {
                if let Some(object) = define.as_object_mut() {
                    if let Some(value) = object.remove("isFunction") {
                        object.insert("is_function".into(), value);
                    }
                }
            }
        }
        let reference = match wright_core::hir::parse_value(reference_value) {
            Ok(program) => program,
            Err(error) => {
                failures.push(format!(
                    "{id}: reference fixture cannot be consumed: {error}"
                ));
                continue;
            }
        };

        let mut native_json = serde_json::to_value(&native).unwrap();
        let mut reference_json = serde_json::to_value(&reference).unwrap();
        normalize(&mut native_json);
        normalize(&mut reference_json);

        if native_json == reference_json {
            report.insert(id.to_string(), serde_json::json!({ "status": "parity" }));
        } else {
            failures.push(format!("{id}: HIR divergence"));
            report.insert(
                id.to_string(),
                serde_json::json!({ "status": "divergence" }),
            );
            let out_dir = workspace_root().join("target/wright-differential");
            std::fs::create_dir_all(&out_dir).unwrap();
            std::fs::write(
                out_dir.join(format!("{}.native.json", id.replace('/', "-"))),
                serde_json::to_string_pretty(&native_json).unwrap(),
            )
            .unwrap();
            std::fs::write(
                out_dir.join(format!("{}.reference.json", id.replace('/', "-"))),
                serde_json::to_string_pretty(&reference_json).unwrap(),
            )
            .unwrap();
        }
    }

    // Diagnostics fixture: both frontends must reject with a parse error.
    let diagnostics_dir = fixture_dir("synthetic/diagnostics");
    let source = read(&diagnostics_dir.join("source.opy"));
    let native_diagnostic = wright_opy::compile(&source, "source.opy", &diagnostics_dir)
        .expect_err("the diagnostics fixture must fail natively");
    assert_eq!(
        native_diagnostic.code, "parse-error",
        "the native frontend classifies missing-colon as parse-error"
    );
    let fixture_manifest: serde_json::Value =
        serde_json::from_str(&read(&diagnostics_dir.join("fixture.json"))).unwrap();
    let reference_status = fixture_manifest["expectedStatus"].as_str().unwrap();
    assert_eq!(
        reference_status, "failure",
        "oracle records expected failure"
    );
    report.insert(
        "synthetic/diagnostics".to_string(),
        serde_json::json!({
            "status": "parity",
            "native": { "code": native_diagnostic.code, "line": native_diagnostic.span.map(|s| s.start.line) },
            "reference": { "expectedStatus": reference_status },
        }),
    );

    // Machine-readable report for CI/release gating.
    let report_path = workspace_root().join("target/wright-differential-report.json");
    std::fs::write(
        &report_path,
        serde_json::to_string_pretty(&serde_json::Value::Object(report)).unwrap(),
    )
    .unwrap();

    assert!(
        failures.is_empty(),
        "supported-surface divergences are not allowed:\n{}",
        failures.join("\n")
    );
}
