//! Catalog tests (#29): canonical identities resolve localized spellings to
//! locale-independent ids and back, and catalog validation rejects
//! malformed or colliding data.

use wright_workshop::catalog::{Catalog, Kind, Locale};

fn builtin() -> Catalog {
    Catalog::builtin().expect("built-in catalog validates")
}

fn en() -> Locale {
    Locale::new("en-US")
}

#[test]
fn builtin_catalog_loads_and_declares_en_us() {
    let catalog = builtin();
    assert!(catalog.supports(&en()));
    assert_eq!(catalog.locales().len(), 1);
    assert_eq!(catalog.locales()[0], en());
}

#[test]
fn localized_spelling_resolves_to_canonical_id_and_back() {
    let catalog = builtin();

    // Action: "Disable Inspector Recording" -> disableInspector -> spelling.
    let entry = catalog
        .resolve(Kind::Action, &en(), "Disable Inspector Recording")
        .expect("spelling resolves");
    assert_eq!(entry.id, "disableInspector");
    assert_eq!(
        catalog.spelling(Kind::Action, &en(), "disableInspector"),
        Some("Disable Inspector Recording")
    );

    // Value: multi-word "Count Of" -> countOf.
    let entry = catalog
        .resolve(Kind::Value, &en(), "Count Of")
        .expect("count of resolves");
    assert_eq!(entry.id, "countOf");
    assert_eq!(
        catalog.spelling(Kind::Value, &en(), "countOf"),
        Some("Count Of")
    );

    // Structural: "For Global Variable" -> forGlobalVariable.
    let entry = catalog
        .resolve(Kind::Structural, &en(), "For Global Variable")
        .expect("structural resolves");
    assert_eq!(entry.id, "forGlobalVariable");
}

#[test]
fn enums_resolve_members_to_canonical_identity() {
    let catalog = builtin();
    assert_eq!(
        catalog.resolve_enum_member("Beam", &en(), "Grapple Beam"),
        Some(("Beam".to_string(), "GRAPPLE".to_string()))
    );
    assert_eq!(
        catalog.enum_spelling("Beam", &en(), "GRAPPLE"),
        Some("Grapple Beam")
    );
    assert_eq!(
        catalog.resolve_enum_member("Color", &en(), "Yellow"),
        Some(("Color".to_string(), "YELLOW".to_string()))
    );
    assert_eq!(
        catalog.resolve_enum_member("Wait", &en(), "Ignore Condition"),
        Some(("Wait".to_string(), "IGNORE_CONDITION".to_string()))
    );
}

#[test]
fn unknown_spellings_and_ids_do_not_resolve() {
    let catalog = builtin();
    assert!(
        catalog
            .resolve(Kind::Action, &en(), "Totally Unknown Thing")
            .is_none()
    );
    assert!(catalog.entry(Kind::Value, "noSuchId").is_none());
    assert!(
        catalog
            .resolve_enum_member("Beam", &en(), "Purple Beam")
            .is_none()
    );
}

#[test]
fn locale_normalization_is_case_insensitive() {
    let catalog = builtin();
    let en_upper = Locale::new("EN-US");
    assert_eq!(en_upper, en());
    assert!(catalog.supports(&en_upper));
    assert_eq!(
        catalog.spelling(Kind::Action, &en_upper, "disableInspector"),
        Some("Disable Inspector Recording")
    );
}

#[test]
fn duplicate_aliases_fail_validation() {
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
    let error = Catalog::load(bad).expect_err("colliding aliases must fail");
    assert!(error.to_string().contains("duplicate"));
}

#[test]
fn missing_locale_alias_fails_validation() {
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
    assert!(error.to_string().contains("missing"));
}

#[test]
fn undeclared_locale_fails_validation() {
    let bad = r#"{
        "schemaVersion": 1,
        "locales": ["en-US"],
        "target": { "game": "g", "format": "f", "surface": "s" },
        "provenance": { "generator": "g", "generatorVersion": "0", "source": "s", "license": "l", "reviewed": true },
        "structural": [
            { "id": "if", "aliases": { "en-US": "If", "zh-CN": "如果" } }
        ]
    }"#;
    let error = Catalog::load(bad).expect_err("undeclared locale must fail");
    assert!(error.to_string().contains("undeclared locale"));
}
