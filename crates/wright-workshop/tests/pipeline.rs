//! Pipeline tests (#30): the committed canonical catalog data is regenerable
//! deterministically and validation rejects collisions and missing aliases.
//!
//! Cutover note (wright#143): the catalog data, generator binary
//! (`workshop-catalog-gen`), and its pipeline tests are owned by
//! `workshop-rs`. The wright-owned `wright-catalog-gen` binary was removed
//! with the duplicate catalog implementation; the tests that spawned it are
//! superseded by the workshop-rs pipeline suite. These library-level tests
//! remain as adapter regression coverage, running against the re-exported
//! `workshop_rs::catalog` API and the workshop-rs catalog dataset.

use wright_workshop::catalog::{CATALOG_DATA, Catalog, canonicalize};

fn read_data() -> String {
    CATALOG_DATA.to_string()
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
fn catalog_rejects_colliding_aliases() {
    let bad = r#"{
        "schemaVersion": 1,
        "locales": ["en-US"],
        "target": { "game": "g", "format": "f", "surface": "s" },
        "provenance": { "generator": "g", "generatorVersion": "0", "source": "s", "license": "l", "reviewed": true },
        "structural": [
            { "id": "if", "aliases": { "en-US": "If" } },
            { "id": "elseIf", "aliases": { "en-US": "If" } }
        ]
    }"#;
    let error = Catalog::load(bad).expect_err("collision must fail validation");
    assert!(
        error.to_string().contains("duplicate"),
        "error names the problem: {error}"
    );
}

#[test]
fn catalog_rejects_missing_locale_alias() {
    let bad = r#"{
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
    let error = Catalog::load(bad).expect_err("missing alias must fail");
    assert!(
        error.to_string().contains("missing"),
        "error names the problem: {error}"
    );
}
