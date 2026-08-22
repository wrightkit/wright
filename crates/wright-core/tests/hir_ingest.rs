//! Opy HIR v1 ingestion tests: valid payloads, deterministic dumps, and
//! structured failures for malformed, forward-version, and unsupported
//! payloads.

use std::path::{Path, PathBuf};

use wright_core::hir::{self, HirError};

fn fixture_path(fixture_id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../adapter/fixtures")
        .join(format!("{fixture_id}.json"))
}

fn read_fixture(fixture_id: &str) -> String {
    std::fs::read_to_string(fixture_path(fixture_id))
        .unwrap_or_else(|error| panic!("cannot read adapter fixture {fixture_id}: {error}"))
}

const ADAPTER_FIXTURES: &[&str] = &[
    "synthetic/basic-rule",
    "synthetic/control-flow",
    "synthetic/declarations-rules",
    "synthetic/expressions-values",
    "synthetic/preprocessing",
    "real-world/overpy-cake",
];

#[test]
fn loads_every_adapter_fixture_and_dumps_deterministically() {
    for fixture_id in ADAPTER_FIXTURES {
        let program = hir::parse_str(&read_fixture(fixture_id))
            .unwrap_or_else(|error| panic!("{fixture_id} must parse: {error}"));
        program
            .validate()
            .unwrap_or_else(|error| panic!("{fixture_id} must validate: {error}"));
        let first = program.dump();
        let second = program.dump();
        assert_eq!(first, second, "{fixture_id} dump must be deterministic");
        assert!(!first.is_empty(), "{fixture_id} dump must not be empty");
    }
}

#[test]
fn dump_matches_golden_for_basic_rule() {
    let program = hir::parse_str(&read_fixture("synthetic/basic-rule")).unwrap();
    let golden = std::fs::read_to_string(golden_path("synthetic/basic-rule.dump"))
        .unwrap_or_else(|error| panic!("missing golden dump: {error}"));
    assert_eq!(program.dump(), golden);
}

#[test]
fn dump_matches_golden_for_control_flow() {
    let program = hir::parse_str(&read_fixture("synthetic/control-flow")).unwrap();
    let golden = std::fs::read_to_string(golden_path("synthetic/control-flow.dump"))
        .unwrap_or_else(|error| panic!("missing golden dump: {error}"));
    assert_eq!(program.dump(), golden);
}

#[test]
fn dump_matches_golden_for_declarations_rules() {
    let program = hir::parse_str(&read_fixture("synthetic/declarations-rules")).unwrap();
    let golden = std::fs::read_to_string(golden_path("synthetic/declarations-rules.dump"))
        .unwrap_or_else(|error| panic!("missing golden dump: {error}"));
    assert_eq!(program.dump(), golden);
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

#[test]
fn rejects_unknown_protocol_name() {
    let error =
        hir::parse_str(r#"{"protocol":{"name":"other/thing","version":"1.0.0"}}"#).unwrap_err();
    assert_eq!(error.code(), "incompatible-protocol");
    match error {
        HirError::IncompatibleProtocol { expected, received } => {
            assert!(expected.contains("wright/opy-hir"));
            assert!(received.contains("other/thing"));
        }
        other => panic!("expected incompatible-protocol, got {other}"),
    }
}

#[test]
fn rejects_forward_major_version_before_body_inspection() {
    // A v2 payload whose body has a completely different shape must still be
    // rejected as incompatible, not as malformed: the envelope is checked
    // first.
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "2.0.0" },
        "futureBody": { "shape": "unknown" }
    }"#;
    let error = hir::parse_str(payload).unwrap_err();
    assert_eq!(error.code(), "incompatible-protocol");
    match error {
        HirError::IncompatibleProtocol { expected, received } => {
            assert!(expected.contains("v1"));
            assert!(received.contains("2.0.0"));
        }
        other => panic!("expected incompatible-protocol, got {other}"),
    }
}

#[test]
fn rejects_missing_protocol() {
    let error = hir::parse_str(r#"{"declarations":[],"rules":[]}"#).unwrap_err();
    assert_eq!(error.code(), "incompatible-protocol");
}

#[test]
fn rejects_malformed_json() {
    let error = hir::parse_str("{not json").unwrap_err();
    assert_eq!(error.code(), "malformed-payload");
}

#[test]
fn rejects_missing_required_field() {
    // A rule without its required `event` fails deserialization as malformed.
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [],
        "rules": [ { "name": "broken", "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } } ]
    }"#;
    let error = hir::parse_str(payload).unwrap_err();
    assert_eq!(error.code(), "malformed-payload");
}

#[test]
fn rejects_unknown_node_kind_with_span() {
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [],
        "rules": [ {
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 6 } } },
            "conditions": [],
            "actions": [
                { "kind": "expr", "expr": { "kind": "mysteryNode", "span": { "file": 0, "start": { "line": 3, "col": 5 }, "end": { "line": 3, "col": 9 } } }, "span": { "file": 0, "start": { "line": 3, "col": 5 }, "end": { "line": 3, "col": 9 } } }
            ]
        } ]
    }"#;
    let error = hir::parse_str(payload).unwrap_err();
    assert_eq!(error.code(), "unsupported-node");
    match error {
        HirError::UnsupportedNode { kind, span } => {
            assert_eq!(kind, "mysteryNode");
            let span = span.expect("unsupported node must carry its span");
            assert_eq!(span.start.line, 3);
        }
        other => panic!("expected unsupported-node, got {other}"),
    }
}

#[test]
fn rejects_span_with_unknown_file() {
    let payload = invalid_rule_payload(
        r#"{
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 7, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": []
        }"#,
    );
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "invalid-span");
}

#[test]
fn rejects_zero_based_position() {
    let payload = invalid_rule_payload(
        r#"{
            "name": "r",
            "span": { "file": 0, "start": { "line": 0, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": []
        }"#,
    );
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "invalid-span");
}

#[test]
fn rejects_unresolved_global_reference() {
    let payload = invalid_rule_payload(
        r#"{
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "expr", "expr": { "kind": "call", "name": "debug", "args": [ { "kind": "globalVar", "name": "nope", "span": { "file": 0, "start": { "line": 2, "col": 11 }, "end": { "line": 2, "col": 15 } } } ], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 16 } } }, "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 16 } } }
            ]
        }"#,
    );
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "unresolved-reference");
}

#[test]
fn rejects_duplicate_declaration_name() {
    let payload = r#"{
        "protocol": { "name": "wright/opy-hir", "version": "1.0.0" },
        "generator": { "name": "g", "version": "0", "frontend": "f" },
        "files": [ { "id": 0, "path": "source.opy" } ],
        "declarations": [
            { "kind": "globalVariable", "name": "x", "index": null, "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } }, "initializer": null },
            { "kind": "globalVariable", "name": "x", "index": null, "span": { "file": 0, "start": { "line": 2, "col": 1 }, "end": { "line": 2, "col": 2 } }, "initializer": null }
        ],
        "rules": []
    }"#;
    let error = hir::parse_str(payload).unwrap_err();
    assert_eq!(error.code(), "invalid-identifier");
}

#[test]
fn rejects_for_loop_over_undeclared_variable() {
    let payload = invalid_rule_payload(
        r#"{
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "for", "variable": { "kind": "globalVar", "name": "i", "span": { "file": 0, "start": { "line": 2, "col": 9 }, "end": { "line": 2, "col": 10 } } }, "iterable": { "kind": "call", "name": "range", "args": [], "span": { "file": 0, "start": { "line": 2, "col": 14 }, "end": { "line": 2, "col": 19 } } }, "body": [], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 20 } } }
            ]
        }"#,
    );
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "unresolved-reference");
}

#[test]
fn default_var_for_binder_validates_and_converts_to_an_implicit_global() {
    // The agent-lab regression payload: `for I in range(0, 10): total += 1`
    // with `I` undeclared. `I` is an OverPy default variable name, so the
    // reference accepts it as an implicit global loop binder (#114): the
    // payload validates, and conversion creates an internal global with the
    // fixed slot (8) and no source span, without adding a protocol
    // declaration.
    let payload = format!(
        r#"{{
            "protocol": {{ "name": "wright/opy-hir", "version": "1.0.0" }},
            "generator": {{ "name": "g", "version": "0", "frontend": "f" }},
            "files": [ {{ "id": 0, "path": "source.opy" }} ],
            "declarations": [
                {{ "kind": "globalVariable", "name": "total", "index": null, "span": {{ "file": 0, "start": {{ "line": 1, "col": 1 }}, "end": {{ "line": 1, "col": 15 }} }}, "initializer": null }}
            ],
            "rules": [ {rule} ]
        }}"#,
        rule = r#"{
            "name": "r",
            "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } },
            "event": { "name": "global", "args": [], "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } } },
            "conditions": [],
            "actions": [
                { "kind": "for", "variable": { "kind": "globalVar", "name": "I", "span": { "file": 0, "start": { "line": 2, "col": 9 }, "end": { "line": 2, "col": 10 } } }, "iterable": { "kind": "call", "name": "range", "args": [ { "kind": "number", "value": 0, "text": "0", "span": { "file": 0, "start": { "line": 2, "col": 20 }, "end": { "line": 2, "col": 21 } } }, { "kind": "number", "value": 10, "text": "10", "span": { "file": 0, "start": { "line": 2, "col": 22 }, "end": { "line": 2, "col": 25 } } } ], "span": { "file": 0, "start": { "line": 2, "col": 14 }, "end": { "line": 2, "col": 26 } } }, "body": [ { "kind": "assign", "target": { "kind": "globalVar", "name": "total", "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 14 } } }, "value": { "kind": "binary", "op": "+", "left": { "kind": "globalVar", "name": "total", "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 14 } } }, "right": { "kind": "globalVar", "name": "I", "span": { "file": 0, "start": { "line": 3, "col": 17 }, "end": { "line": 3, "col": 19 } } }, "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 19 } } }, "span": { "file": 0, "start": { "line": 3, "col": 9 }, "end": { "line": 3, "col": 19 } } } ], "span": { "file": 0, "start": { "line": 2, "col": 5 }, "end": { "line": 2, "col": 10 } } }
            ]
        }"#
    );
    let program = hir::parse_str(&payload).expect("a default-var binder validates");
    assert!(
        program.declarations.len() == 1,
        "only the declared 'total' is in the protocol declarations"
    );
    let model = program.to_ir().expect("converts");
    assert_eq!(model.globals.len(), 2, "one implicit global is created");
    let implicit = model
        .globals
        .iter()
        .find(|global| global.name == "I")
        .expect("the implicit 'I' global exists");
    assert_eq!(implicit.index, Some(8), "the fixed default-var slot");
    assert_eq!(implicit.span, None);
    assert_eq!(implicit.name_span, None);
}

/// Build a minimal valid payload whose single rule is `rule_json`.
fn invalid_rule_payload(rule_json: &str) -> String {
    format!(
        r#"{{
            "protocol": {{ "name": "wright/opy-hir", "version": "1.0.0" }},
            "generator": {{ "name": "g", "version": "0", "frontend": "f" }},
            "files": [ {{ "id": 0, "path": "source.opy" }} ],
            "declarations": [],
            "rules": [ {rule_json} ]
        }}"#
    )
}

const SPAN: &str =
    r#"{ "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 1, "col": 2 } }"#;

/// A minimal valid settings block (every leaf uses evidenced table keys).
const VALID_SETTINGS: &str = r#"{
    "protocol": { "name": "wright/opy-hir", "version": "1.1.0" },
    "generator": { "name": "g", "version": "0", "frontend": "f" },
    "files": [ { "id": 0, "path": "source.opy" } ],
    "declarations": [],
    "rules": [],
    "settings": {
        "span": { "file": 0, "start": { "line": 1, "col": 1 }, "end": { "line": 9, "col": 2 } },
        "children": [
            {
                "kind": "group", "name": "gamemodes",
                "children": [
                    {
                        "kind": "group", "name": "skirmish",
                        "children": [
                            { "kind": "list", "name": "enabledMaps", "elements": [ { "value": "workshopIsland", "span": __SPAN__ } ], "span": __SPAN__ }
                        ],
                        "span": __SPAN__
                    },
                    {
                        "kind": "group", "name": "assault",
                        "children": [
                            { "kind": "list", "name": "enabledMaps", "elements": [], "span": __SPAN__ },
                            { "kind": "string", "name": "roleLimit", "value": "2OfEachRolePerTeam", "span": __SPAN__ }
                        ],
                        "span": __SPAN__
                    },
                    {
                        "kind": "group", "name": "general",
                        "children": [
                            { "kind": "string", "name": "heroLimit", "value": "off", "span": __SPAN__ },
                            { "kind": "number", "name": "respawnTime%", "value": 30, "span": __SPAN__ },
                            { "kind": "bool", "name": "enableRandomHeroes", "value": true, "span": __SPAN__ }
                        ],
                        "span": __SPAN__
                    }
                ],
                "span": __SPAN__
            },
            {
                "kind": "group", "name": "heroes",
                "children": [
                    {
                        "kind": "group", "name": "allTeams",
                        "children": [
                            { "kind": "list", "name": "enabledHeroes", "elements": [ { "value": "mei", "span": __SPAN__ } ], "span": __SPAN__ }
                        ],
                        "span": __SPAN__
                    }
                ],
                "span": __SPAN__
            }
        ]
    }
}"#;

#[test]
fn valid_settings_payload_validates_and_dumps() {
    let program = hir::parse_str(&VALID_SETTINGS.replace("__SPAN__", SPAN)).unwrap();
    program
        .validate()
        .unwrap_or_else(|error| panic!("valid settings must validate: {error}"));
    let dump = program.dump();
    assert!(dump.contains("settings:\n"), "dump has a settings section");
    assert!(dump.contains("group gamemodes"));
    assert!(dump.contains("list enabledMaps"));
    assert!(dump.contains("element workshopIsland"));
}

#[test]
fn settings_unknown_key_is_rejected_with_span() {
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"name\": \"respawnTime%\"", "\"name\": \"scoreToWin\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "settings-unknown-key");
    match &error {
        HirError::Invalid { span, .. } => assert!(span.is_some(), "error carries a span"),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn settings_unknown_value_is_rejected_with_span() {
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"value\": \"off\"", "\"value\": \"bogus\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "settings-unknown-value");
    match &error {
        HirError::Invalid { span, .. } => assert!(span.is_some(), "error carries a span"),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

#[test]
fn settings_unknown_list_element_is_rejected() {
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"value\": \"workshopIsland\"", "\"value\": \"noSuchMap\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "settings-unknown-value");
}

#[test]
fn settings_unknown_node_kind_is_rejected() {
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"kind\": \"bool\"", "\"kind\": \"enum\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "unsupported-node");
}

#[test]
fn settings_mode_subset_violations_are_rejected() {
    // gamemodes.assault.heroLimit is outside the per-key subsets (heroLimit
    // is evidenced under general only, #86).
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"name\": \"skirmish\"", "\"name\": \"assault\"")
        .replace("\"name\": \"roleLimit\"", "\"name\": \"heroLimit\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "settings-unknown-key");
    assert!(
        matches!(&error, HirError::Invalid { span: Some(_), .. }),
        "the violation carries the key span"
    );
    // The released settings table recognizes general.roleLimit as a key but
    // rejects the inherited `off` value at this path.
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"name\": \"heroLimit\"", "\"name\": \"roleLimit\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "settings-unknown-value");
}

#[test]
fn settings_role_limit_off_is_not_evidenced() {
    // roleLimit "off" exists only in the not-acquired skirmish_elim source;
    // the strict table rejects it (settings-unknown-value).
    let payload = VALID_SETTINGS
        .replace("__SPAN__", SPAN)
        .replace("\"value\": \"2OfEachRolePerTeam\"", "\"value\": \"off\"");
    let error = hir::parse_str(&payload).unwrap_err();
    assert_eq!(error.code(), "settings-unknown-value");
}
