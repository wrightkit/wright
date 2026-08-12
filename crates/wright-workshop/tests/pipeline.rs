//! Pipeline tests (#30): catalog data is regenerable deterministically,
//! validation rejects collisions/missing aliases, and the committed catalog
//! is already canonical.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use wright_workshop::catalog::{Catalog, canonicalize};

fn data_file() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/catalog/data/catalog.json")
}

fn read_data() -> String {
    std::fs::read_to_string(data_file()).expect("catalog data is present")
}

#[test]
fn canonicalize_is_deterministic_and_idempotent() {
    let source = read_data();
    let first = canonicalize(&source).expect("source canonicalizes");
    let second = canonicalize(&first).expect("canonical form canonicalizes");
    assert_eq!(first, second, "canonicalize must be idempotent");
    // The canonical form still loads and validates.
    Catalog::load(&first).expect("canonical form validates");
}

#[test]
fn committed_catalog_is_already_canonical() {
    let source = read_data();
    let canonical = canonicalize(&source).expect("source canonicalizes");
    assert_eq!(
        source, canonical,
        "committed catalog.json must be canonical"
    );
}

#[test]
fn generator_check_passes_on_the_committed_catalog() {
    let bin = env!("CARGO_BIN_EXE_wright-catalog-gen");
    let output = Command::new(bin)
        .args(["check", "--file"])
        .arg(data_file())
        .output()
        .expect("generator runs");
    assert!(
        output.status.success(),
        "check must pass: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.starts_with("OK "),
        "check prints a summary: {stdout}"
    );
}

#[test]
fn generator_build_is_byte_deterministic() {
    let bin = env!("CARGO_BIN_EXE_wright-catalog-gen");
    let dir = std::env::temp_dir().join("wright-catalog-gen-test");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("catalog.json");
    std::fs::write(&out, read_data()).expect("copy data");

    let run_build = |path: &Path| {
        let output = Command::new(bin)
            .args(["build", "--file"])
            .arg(path)
            .output()
            .expect("generator runs");
        assert!(
            output.status.success(),
            "build must pass: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(path).expect("built file exists")
    };

    let first = run_build(&out);
    let second = run_build(&out);
    assert_eq!(first, second, "build must be byte-deterministic");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generator_rejects_colliding_aliases() {
    let bin = env!("CARGO_BIN_EXE_wright-catalog-gen");
    let dir = std::env::temp_dir().join("wright-catalog-gen-bad");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let bad = dir.join("bad.json");
    let bad_json = r#"{
        "schemaVersion": 1,
        "locales": ["en-US"],
        "target": { "game": "g", "format": "f", "surface": "s" },
        "provenance": { "generator": "g", "generatorVersion": "0", "source": "s", "license": "l", "reviewed": true },
        "structural": [
            { "id": "if", "aliases": { "en-US": "If" } },
            { "id": "elseIf", "aliases": { "en-US": "If" } }
        ]
    }"#;
    std::fs::write(&bad, bad_json).expect("write bad catalog");

    let output = Command::new(bin)
        .args(["check", "--file"])
        .arg(&bad)
        .output()
        .expect("generator runs");
    assert_eq!(output.status.code(), Some(1), "collision must fail check");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("duplicate"),
        "error names the problem: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generator_rejects_missing_locale_alias() {
    let bin = env!("CARGO_BIN_EXE_wright-catalog-gen");
    let dir = std::env::temp_dir().join("wright-catalog-gen-missing");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let bad = dir.join("bad.json");
    let bad_json = r#"{
        "schemaVersion": 1,
        "locales": ["en-US"],
        "target": { "game": "g", "format": "f", "surface": "s" },
        "provenance": { "generator": "g", "generatorVersion": "0", "source": "s", "license": "l", "reviewed": true },
        "structural": [
            { "id": "if", "aliases": { "en-US": "If" } }
        ],
        "actions": [
            { "id": "wait", "aliases": {} }
        ]
    }"#;
    std::fs::write(&bad, bad_json).expect("write bad catalog");

    let output = Command::new(bin)
        .args(["check", "--file"])
        .arg(&bad)
        .output()
        .expect("generator runs");
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("missing"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn generator_reports_missing_file_to_stderr() {
    let bin = env!("CARGO_BIN_EXE_wright-catalog-gen");
    let output = Command::new(bin)
        .args(["check", "--file"])
        .arg("/nonexistent/catalog.json")
        .stdin(Stdio::null())
        .output()
        .expect("generator runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot read"));
    assert!(output.stdout.is_empty());
}
